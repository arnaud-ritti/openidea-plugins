//! Jump to source: the file and line of a stack frame, or else the source
//! line that most likely logged the event (port of Ideolog's
//! `LogJumpToSourceIntention`, which ranks files with the IDE's trigram index;
//! here every source file is scanned).

use fancy_regex::Regex;
use openidea_plugin::{Error, Project, host};
use std::ops::Range;
use std::sync::LazyLock;

const MESSAGE_LIMIT: usize = 100;
// ponytail: no index, so Jump to Source reads up to this many source files per
// search; an index (or a host search API) would lift it.
const MAX_SOURCE_FILES: usize = 5_000;
const SOURCE_EXTENSIONS: &[&str] = &[
    "java", "kt", "kts", "scala", "groovy", "cs", "vb", "cpp", "cc", "c", "h", "hpp", "m", "swift", "go", "rs", "py",
    "rb", "php", "js", "jsx", "ts", "tsx", "mjs", "cjs", "dart", "ex", "exs", "erl", "lua",
];

#[derive(Debug, PartialEq, Eq)]
pub struct Frame {
    /// As written, `/` separated; Java frames get their package directories.
    pub path: String,
    /// 1-based, as in stack traces.
    pub line: u32,
    /// Where the frame is in the log line (bytes).
    pub span: Range<usize>,
}

static JAVA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"at\s+([\w$.]+)\.[\w$<>]+\(([\w$-]+\.\w+):(\d+)\)").unwrap());
static PYTHON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"File "([^"]+)", line (\d+)"#).unwrap());
static DOTNET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" in (\S.*?):line (\d+)").unwrap());
static GENERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"((?:[A-Za-z]:)?[\w.\\/@~-]*?[\w-]+\.[A-Za-z]\w*)(?::|\()(\d+)").unwrap());

/// Stack frames of one log line, in line order.
pub fn frames(line: &str) -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::new();
    let mut add = |frame: Frame| {
        if !frames.iter().any(|f| f.span.start < frame.span.end && frame.span.start < f.span.end) {
            frames.push(frame);
        }
    };

    for captures in JAVA.captures_iter(line).map_while(Result::ok) {
        let class = &captures[1];
        let package = class.rsplit_once('.').map_or("", |(package, _)| package);
        let dirs = package.split('.').filter(|p| !p.is_empty()).map(|p| format!("{p}/")).collect::<String>();
        add(Frame {
            path: format!("{dirs}{}", &captures[2]),
            line: captures[3].parse().unwrap_or(1),
            span: captures.get(0).unwrap().range(),
        });
    }
    for pattern in [&*PYTHON, &*DOTNET, &*GENERIC] {
        for captures in pattern.captures_iter(line).map_while(Result::ok) {
            add(Frame {
                path: captures[1].replace('\\', "/"),
                line: captures[2].parse().unwrap_or(1),
                span: captures.get(0).unwrap().range(),
            });
        }
    }
    frames.sort_by_key(|f| f.span.start);
    frames
}

/// The candidate sharing the most trailing path segments with `path` (at
/// least the file name).
pub fn best_path<'a>(path: &str, candidates: &'a [String]) -> Option<&'a String> {
    let wanted: Vec<&str> = path.rsplit('/').filter(|s| !s.is_empty()).collect();
    let shared = |candidate: &String| {
        let segments = candidate.rsplit(['/', '\\']);
        segments.zip(&wanted).take_while(|(a, b)| a == *b).count()
    };
    let best = candidates.iter().max_by_key(|c| (shared(c), std::cmp::Reverse(c.len())))?;
    (shared(best) > 0).then_some(best)
}

fn glob_literal(name: &str) -> String {
    name.chars()
        .map(|c| if "[]{}*?".contains(c) { format!("[{c}]") } else { c.to_string() })
        .collect()
}

/// The project file a frame points at.
pub fn resolve(project: &Project, frame: &Frame) -> Result<Option<String>, Error> {
    let name = frame.path.rsplit('/').next().unwrap_or(&frame.path);
    let found = project.find_files(&format!("**/{}", glob_literal(name)), 200)?;
    Ok(best_path(&frame.path, &found).cloned())
}

/// Ideolog's `FileMatch.calculateFilenameMatch`: 2 for the category, 1 for a
/// prefix of it, or 1 for a word of an event without a category.
pub fn file_name_score(name: &str, category: Option<&str>, event: &str) -> u32 {
    match category {
        Some(category) if name == category => 2,
        Some(category) if name.starts_with(category) && name.len() as f32 <= category.len() as f32 * 1.5 => 1,
        Some(_) => 0,
        None => u32::from(words(event).any(|w| w == name)),
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation()).filter(|w| !w.is_empty())
}

fn longest_common_substring(a: &[char], b: &[char]) -> usize {
    let mut previous = vec![0usize; b.len() + 1];
    let mut best = 0;
    for &x in a {
        let mut current = vec![0usize; b.len() + 1];
        for (j, &y) in b.iter().enumerate() {
            if x == y {
                current[j + 1] = previous[j] + 1;
                best = best.max(current[j + 1]);
            }
        }
        previous = current;
    }
    best
}

/// Ideolog's `FileMatch.processFileOffset`: the line with a string literal
/// sharing the longest substring with the message, and that length.
pub fn best_line(message: &str, text: &str) -> Option<(usize, u32)> {
    let message: Vec<char> = message.chars().take(MESSAGE_LIMIT).collect();
    let mut best: Option<(usize, u32)> = None;
    for (index, line) in text.lines().enumerate() {
        let code: Vec<char> = line.trim().chars().take(MESSAGE_LIMIT).collect();
        if !code.contains(&'"') && !code.contains(&'\'') {
            continue;
        }

        let score = longest_common_substring(&message, &code);
        if score > 0 && best.is_none_or(|(most, _)| score > most) {
            best = Some((score, index as u32));
        }
    }
    best
}

/// The source line that most likely logged `message`: files must contain its
/// longest word; the longest shared substring wins, then the file name score.
pub fn find_logging_line(
    project: &Project,
    message: &str,
    category: Option<&str>,
    event: &str,
) -> Result<Option<(String, u32)>, Error> {
    let anchor = words(message).filter(|w| w.len() >= 4).max_by_key(|w| w.len());
    let task = host::begin_task("Finding the source of the log event", true);
    let files: Vec<String> = project
        .find_files("**/*", 200_000)?
        .into_iter()
        .filter(|path| path.rsplit_once('.').is_some_and(|(_, ext)| SOURCE_EXTENSIONS.contains(&ext)))
        .take(MAX_SOURCE_FILES)
        .collect();

    let mut best: Option<((usize, u32), String, u32)> = None;
    for (done, path) in files.iter().enumerate() {
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }

        task.report(done as u32, Some(files.len() as u32), None);
        let Ok(text) = project.read_file(path) else { continue };
        if anchor.is_some_and(|a| !text.contains(a)) {
            continue;
        }

        let Some((score, line)) = best_line(message, &text) else { continue };
        let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let stem = file_name.split('.').next().unwrap_or(file_name);
        let rank = (score, file_name_score(stem, category, event));
        if best.as_ref().is_none_or(|(most, _, _)| rank > *most) {
            best = Some((rank, path.clone(), line));
        }
    }
    Ok(best.map(|(_, path, line)| (path, line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_frames_of_common_stack_traces() {
        let java = frames("\tat com.example.app.Foo$Inner.bar(Foo.java:42)");
        assert_eq!(java.len(), 1);
        assert_eq!((java[0].path.as_str(), java[0].line), ("com/example/app/Foo.java", 42));

        let python = frames(r#"  File "/srv/app/views.py", line 17, in index"#);
        assert_eq!((python[0].path.as_str(), python[0].line), ("/srv/app/views.py", 17));

        let php = frames("#0 /var/www/html/app/Http/Kernel.php(128): Foo->bar()");
        assert_eq!((php[0].path.as_str(), php[0].line), ("/var/www/html/app/Http/Kernel.php", 128));

        let node = frames("    at handler (/app/src/routes/user.ts:10:5)");
        assert_eq!((node[0].path.as_str(), node[0].line), ("/app/src/routes/user.ts", 10));

        let dotnet = frames(r"   at App.Run() in C:\src\App\Program.cs:line 12");
        assert_eq!((dotnet[0].path.as_str(), dotnet[0].line), ("C:/src/App/Program.cs", 12));

        assert!(frames("2024-05-01 10:00:00,123 [   1234]   INFO - #c.i.Startup - IDE started").is_empty());
    }

    #[test]
    fn picks_the_path_sharing_most_segments() {
        let found = ["/p/src/a/Foo.java".to_owned(), "/p/src/com/example/Foo.java".to_owned()];
        assert_eq!(best_path("com/example/Foo.java", &found), Some(&found[1]));
        assert_eq!(best_path("/var/www/html/src/a/Foo.java", &found), Some(&found[0]));
        assert_eq!(best_path("Bar.java", &found), None);
    }

    #[test]
    fn scores_file_names_like_ideolog() {
        assert_eq!(file_name_score("UserService", Some("UserService"), ""), 2);
        assert_eq!(file_name_score("UserServiceImpl", Some("UserService"), ""), 1);
        assert_eq!(file_name_score("Other", Some("UserService"), ""), 0);
        assert_eq!(file_name_score("Billing", None, "error in Billing: card declined"), 1);
    }

    #[test]
    fn finds_the_line_that_logged_the_message() {
        let code = "fun pay() {\n  log.info(\"starting\")\n  log.error(\"card declined for user\")\n}\n";
        assert_eq!(best_line("card declined for user 42", code), Some((22, 2)));
    }
}
