use crate::matcher::Rule;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Syntax {
    Glob,
    Regexp,
}

/// How an ignore file reads its patterns: the default syntax and whether
/// `syntax:` lines switch it (Mercurial). `None`: not an ignore file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dialect {
    pub syntax: Syntax,
    pub switchable: bool,
}

pub fn dialect(path: &str) -> Option<Dialect> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if !name.ends_with("ignore") && name != "ignore-glob" {
        return None;
    }
    let (syntax, switchable) = match name {
        ".hgignore" => (Syntax::Regexp, true),
        ".boringignore" | ".mtn-ignore" => (Syntax::Regexp, false),
        _ => (Syntax::Glob, false),
    };
    Some(Dialect { syntax, switchable })
}

/// A problem in an ignore file; byte offsets into its text. Each fix replaces
/// a byte range with a text.
#[derive(Debug, PartialEq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    /// Else an error.
    pub warning: bool,
    pub message: String,
    pub fixes: Vec<(&'static str, std::ops::Range<usize>, String)>,
}

pub fn check(text: &str, dialect: Dialect) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut syntax = dialect.syntax;
    let mut seen = HashSet::new();
    let mut line_start = 0;
    let mut entries = Vec::new();

    for raw in text.split_inclusive('\n') {
        let line_end = line_start + raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        let entry = line.trim();
        let start = line_start + (line.len() - line.trim_start().len());
        let end = start + entry.len();
        let whole_line = line_range(text, line_start, line_end);
        line_start = line_end;

        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }

        if dialect.switchable && let Some(value) = entry.strip_prefix("syntax:") {
            match value.trim() {
                "re" | "regexp" => syntax = Syntax::Regexp,
                "glob" | "rootglob" => syntax = Syntax::Glob,
                "include" | "subinclude" => {}
                _ => problems.push(Problem {
                    start,
                    end,
                    code: "syntax-entry",
                    warning: false,
                    message: format!("Unknown syntax in `{entry}`. Use glob or regexp."),
                    fixes: vec![
                        ("Use glob syntax", start..end, "syntax: glob".into()),
                        ("Use regexp syntax", start..end, "syntax: regexp".into()),
                    ],
                }),
            }
            continue;
        }

        entries.push(Entry { start, end, line: whole_line.clone(), text: entry });

        if !seen.insert(entry) {
            problems.push(Problem {
                start,
                end,
                code: "duplicate-entry",
                warning: false,
                message: format!("`{entry}` is already listed above."),
                fixes: vec![("Remove entry", whole_line.clone(), String::new())],
            });
        }

        if has_relative_part(entry) {
            problems.push(Problem {
                start,
                end,
                code: "relative-entry",
                warning: false,
                message: format!("`{entry}` contains a relative path (`.` or `..`)."),
                fixes: vec![("Remove relative part", start..end, remove_relative_parts(entry))],
            });
        }

        if let Some(error) = pattern_error(entry, syntax, dialect.switchable) {
            problems.push(Problem {
                start,
                end,
                code: "incorrect-entry",
                warning: false,
                message: format!("`{entry}` is not a valid pattern: {error}."),
                fixes: Vec::new(),
            });
        }
    }

    if dialect.syntax == Syntax::Glob && !dialect.switchable {
        problems.extend(covered_entries(&entries));
    }

    problems
}

struct Entry<'a> {
    start: usize,
    end: usize,
    line: std::ops::Range<usize>,
    text: &'a str,
}

/// Literal entries another entry already matches, with no entry of the other
/// kind (ignore / unignore) between them. The original plugin compares the
/// files each entry matches on disk; plugins can't list ignored files, so
/// this compares the patterns.
// ponytail: O(n²) over entries; fine for hand-written ignore files.
fn covered_entries(entries: &[Entry]) -> Vec<Problem> {
    let rules: Vec<Option<Rule>> = entries.iter().map(|e| Rule::parse(e.text)).collect();
    let mut problems = Vec::new();

    for (i, (entry, rule)) in entries.iter().zip(&rules).enumerate() {
        let Some(rule) = rule.as_ref() else { continue };
        let Some((path, dir_only)) = covered_path(rule) else { continue };
        let cover = entries.iter().zip(&rules).enumerate().find_map(|(j, (other, other_rule))| {
            let other_rule = other_rule.as_ref()?;
            let same_kind_between = rules[i.min(j)..=i.max(j)].iter().flatten().all(|r| r.negated == rule.negated);
            let covers = other.text != entry.text
                && same_kind_between
                && (rule.anchored || !other_rule.anchored)
                && other_rule.matches(&path, dir_only);
            covers.then_some(other.text)
        });

        if let Some(cover) = cover {
            problems.push(Problem {
                start: entry.start,
                end: entry.end,
                code: "cover-entry",
                warning: true,
                message: format!("`{}` is redundant: `{cover}` matches it too.", entry.text),
                fixes: vec![("Remove entry", entry.line.clone(), String::new())],
            });
        }
    }

    problems
}

/// The path another entry must match to cover `rule`: the entry itself when it
/// names one path, else its literal leading directories (`build/` for
/// `build/*.o`). `None` when no such path exists.
fn covered_path(rule: &Rule) -> Option<(String, bool)> {
    if rule.is_literal() {
        return Some((rule.literal_path(), rule.dir_only));
    }
    if !rule.anchored {
        return None;
    }

    let mut directories = Vec::new();
    for segment in rule.pattern.split('/') {
        if !Rule::parse(segment).is_some_and(|r| r.is_literal()) {
            break;
        }
        directories.push(segment);
    }
    // the last segment is never a directory prefix of the entry unless a wildcard follows it
    if directories.is_empty() || directories.len() == rule.pattern.split('/').count() {
        return None;
    }

    Some((crate::check::unescape(&directories.join("/")), true))
}

/// The line with its line break; the last line takes the break before it.
fn line_range(text: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    if start == 0 || text[start..end].ends_with('\n') {
        return start..end;
    }

    let line_break = if text[..start].ends_with("\r\n") { 2 } else { 1 };
    start - line_break..end
}

pub fn unescape(entry: &str) -> String {
    let mut out = String::with_capacity(entry.len());
    let mut chars = entry.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            _ => out.push(c),
        }
    }
    out
}

fn has_relative_part(entry: &str) -> bool {
    unescape(entry).trim_start_matches('!').split('/').any(|s| s == "." || s == "..")
}

pub fn remove_relative_parts(entry: &str) -> String {
    let (negation, path) = entry.strip_prefix('!').map_or(("", entry), |p| ("!", p));
    let path = path.replace("\\.", ".");
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "." => {}
            ".." => {
                if segments.last().is_some_and(|s| !s.is_empty() && *s != "..") {
                    segments.pop();
                }
            }
            _ => segments.push(segment),
        }
    }
    format!("{negation}{}", segments.join("/"))
}

const HG_PREFIXES: [(&str, Syntax); 9] = [
    ("re", Syntax::Regexp),
    ("regexp", Syntax::Regexp),
    ("relre", Syntax::Regexp),
    ("glob", Syntax::Glob),
    ("rootglob", Syntax::Glob),
    ("relglob", Syntax::Glob),
    ("path", Syntax::Glob),
    ("relpath", Syntax::Glob),
    ("rootfilesin", Syntax::Glob),
];

fn pattern_error(entry: &str, syntax: Syntax, switchable: bool) -> Option<String> {
    let (syntax, pattern) = entry
        .split_once(':')
        .filter(|_| switchable)
        .and_then(|(prefix, rest)| HG_PREFIXES.iter().find(|(p, _)| *p == prefix).map(|(_, s)| (*s, rest)))
        .unwrap_or((syntax, entry));

    match syntax {
        Syntax::Regexp => regex_error(pattern),
        Syntax::Glob => regex_error(&glob_to_regex(pattern.trim_start_matches('!'))),
    }
}

fn regex_error(pattern: &str) -> Option<String> {
    use regex_syntax::ast::{ErrorKind, parse::Parser};

    let error = Parser::new().parse(pattern).err()?;
    // Mercurial patterns are Python regexps: look-around and back-references
    // are valid there, only the Rust parser refuses them.
    if matches!(error.kind(), ErrorKind::UnsupportedLookAround | ErrorKind::UnsupportedBackreference) {
        return None;
    }
    Some(error.kind().to_string())
}

/// Wildcards become regexp operators and character classes are copied, so a
/// malformed class (unclosed, reversed range) fails to parse like in the
/// original plugin.
fn glob_to_regex(glob: &str) -> String {
    let mut out = String::new();
    let mut chars = glob.chars().peekable();
    let mut in_class = false;

    while let Some(c) = chars.next() {
        if in_class {
            out.push(c);
            if c == '[' && chars.peek() == Some(&':') {
                while let Some(n) = chars.next() {
                    out.push(n);
                    if n == ':' && chars.peek() == Some(&']') {
                        out.extend(chars.next());
                        break;
                    }
                }
            } else if c == '\\' {
                out.extend(chars.next());
            } else if c == ']' {
                in_class = false;
            }
            continue;
        }

        match c {
            '\\' => out.push_str(&regex_syntax::escape(&chars.next().map(String::from).unwrap_or_default())),
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '[' => {
                in_class = true;
                out.push('[');
                if chars.next_if(|&n| n == '!' || n == '^').is_some() {
                    out.push('^');
                }
                if chars.next_if_eq(&']').is_some() {
                    out.push(']');
                }
            }
            _ => out.push_str(&regex_syntax::escape(c.encode_utf8(&mut [0; 4]))),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIT: Dialect = Dialect { syntax: Syntax::Glob, switchable: false };
    const HG: Dialect = Dialect { syntax: Syntax::Regexp, switchable: true };

    fn codes(text: &str, dialect: Dialect) -> Vec<&'static str> {
        check(text, dialect).iter().map(|p| p.code).collect()
    }

    #[test]
    fn detects_ignore_files_and_their_syntax() {
        assert_eq!(dialect("/p/.gitignore"), Some(GIT));
        assert_eq!(dialect("/p/.hgignore"), Some(HG));
        assert_eq!(dialect("/p/.mtn-ignore").map(|d| d.syntax), Some(Syntax::Regexp));
        assert_eq!(dialect("/p/.fossil-settings/ignore-glob"), Some(GIT));
        assert_eq!(dialect("/p/src/main.rs"), None);
    }

    #[test]
    fn clean_files_have_no_problems() {
        let text = "### Rust template\n# comment\n/target/\n!keep.rs\n*.[oa]\n[Bb]in/\n**/node_modules\n[[:space:]]x\n";
        assert!(check(text, GIT).is_empty(), "{:?}", check(text, GIT));
    }

    #[test]
    fn reports_duplicates_after_the_first_occurrence() {
        assert_eq!(codes("a\nb\n  a  \n", GIT), ["duplicate-entry"]);
        let problem = &check("a\nb\na\n", GIT)[0];
        assert_eq!((problem.start, problem.end), (4, 5));
    }

    #[test]
    fn relative_entries_are_reported_and_fixed() {
        assert_eq!(codes("./build\nfoo.bar/\n", GIT), ["relative-entry"]);
        assert_eq!(remove_relative_parts("./build"), "build");
        assert_eq!(remove_relative_parts("/a/./b/../c/"), "/a/c/");
        assert_eq!(remove_relative_parts("!../x"), "!x");
        assert_eq!(remove_relative_parts("\\./x"), "x");
    }

    #[test]
    fn malformed_patterns_are_reported() {
        assert_eq!(codes("[abc\n", GIT), ["incorrect-entry"]);
        assert_eq!(codes("[z-a]\n", GIT), ["incorrect-entry"]);
        assert_eq!(codes("*.pyc\n", HG), ["incorrect-entry"]);
        assert!(codes("^build/\n(?<=x)y\n", HG).is_empty());
        assert!(codes("glob:*.pyc\nsyntax: glob\n*.o\n", HG).is_empty());
    }

    #[test]
    fn invalid_syntax_values_only_in_mercurial_files() {
        assert_eq!(codes("syntax: perl\n", HG), ["syntax-entry"]);
        assert!(codes("syntax: perl\n", GIT).is_empty());
    }

    #[test]
    fn literal_entries_covered_by_another_entry_are_warned_about() {
        let problems = check("*.log\n/logs/debug.log\nbuild\nbuild/\n/x/build\n", GIT);
        let covered: Vec<(&str, bool)> = problems.iter().map(|p| (p.message.as_str(), p.warning)).collect();
        assert_eq!(
            covered,
            [
                ("`/logs/debug.log` is redundant: `*.log` matches it too.", true),
                ("`build/` is redundant: `build` matches it too.", true),
                ("`/x/build` is redundant: `build` matches it too.", true),
            ]
        );
    }

    #[test]
    fn glob_entries_inside_an_ignored_directory_are_covered() {
        let messages: Vec<String> = check("build/\nbuild/*.o\n/x/**/y\n/x/\n!/z/*.c\n/z/\nsrc/*.o\n*.o\n", GIT).into_iter().map(|p| p.message).collect();
        assert_eq!(
            messages,
            [
                "`build/*.o` is redundant: `build/` matches it too.",
                "`/x/**/y` is redundant: `/x/` matches it too.",
            ]
        );
        assert_eq!(codes("*.log\ndebug.log\n", GIT), ["cover-entry"]);
        assert!(codes("!debug.log\n*.log\n", GIT).is_empty());
    }

    #[test]
    fn cover_respects_anchoring_and_negations() {
        let messages: Vec<String> = check("/debug.log\ndebug.log\n", GIT).into_iter().map(|p| p.message).collect();
        assert_eq!(messages, ["`/debug.log` is redundant: `debug.log` matches it too."], "an anchored entry doesn't cover an unanchored one");
        assert!(codes("logs/\n!logs/keep.txt\nlogs/keep.txt\n", GIT).is_empty(), "an unignore between them");
        let messages: Vec<String> = check("build/\nbuild\n", GIT).into_iter().map(|p| p.message).collect();
        assert_eq!(messages, ["`build/` is redundant: `build` matches it too."], "a directory entry doesn't cover files");
        assert!(codes("^a$\na\n", Dialect { syntax: Syntax::Regexp, switchable: false }).is_empty());
    }
}
