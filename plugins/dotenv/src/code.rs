//! Environment variable keys in code: `getenv("KEY")`, `os.environ["KEY"]`,
//! `process.env.KEY`, `"${KEY}"` in a `.env` file. The original plugin walks
//! each language's PSI; without one this matches the text before the key.

use std::ops::Range;

/// Text ending right before a key's opening quote (compared lowercased):
/// `getenv(` (PHP, Java, Kotlin, Python, C), `os.Getenv(` (Go), `env(` (Laravel),
/// `$_ENV[` / `ENV[` / `process.env[` / `dotenv[`, `os.environ[` / `.get(`,
/// `ENV.fetch(` (Ruby), `env::var(` / `env!(` (Rust), `dotenv.get(`, `Deno.env.get(`,
/// `<env name=` (PHPUnit)…
const CALLS: &[&str] = &[
    "env(",
    "env[",
    "env!(",
    "env.get(",
    "env.fetch(",
    "environ[",
    "environ.get(",
    "lookupenv(",
    "env::var(",
    "env::var_os(",
    "env::set_var(",
    "env::remove_var(",
    "dotenvy::var(",
    "$_server[",
    "getenvironmentvariable(",
    "system.getproperty(",
    "<env name=",
    "<server name=",
];

/// The longest of [`CALLS`] and [`MEMBERS`] plus some blanks: only this much
/// of the text before a key is compared.
const TAIL: usize = 32;

/// Text ending right before an unquoted key (JavaScript, TypeScript).
const MEMBERS: &[&str] = &["process.env.", "import.meta.env.", "bun.env."];

/// The key around byte `at` of `line` and the text before it. Quoted keys
/// may hold dots (`System.getProperty("app.name")`), member keys can't.
fn word(line: &str, at: usize, dots: bool) -> (Range<usize>, &str) {
    let is_key = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || (dots && b == b'.');
    let bytes = line.as_bytes();
    let start = bytes[..at].iter().rposition(|&b| !is_key(b)).map_or(0, |i| i + 1);
    let end = at + bytes[at..].iter().position(|&b| !is_key(b)).unwrap_or(line.len() - at);
    (start..end, &line[..start])
}

/// The end of `text`, lowercased.
fn tail(text: &str) -> String {
    let mut start = text.len().saturating_sub(TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_ascii_lowercase()
}

/// The environment variable key at byte `at` of a source line, if the caret is in one.
pub fn key_in_code(line: &str, at: usize) -> Option<Range<usize>> {
    let (key, before) = word(line, at, true);
    if let Some(call) = tail(before).strip_suffix(['"', '\'', '`']) {
        return CALLS.iter().any(|c| call.trim_end().ends_with(c)).then_some(key);
    }
    let (key, before) = word(line, at, false);
    let before = tail(before);
    MEMBERS.iter().any(|m| before.ends_with(m)).then_some(key)
}

/// Every key a source line reads (or sets).
pub fn keys_in_line(line: &str) -> Vec<Range<usize>> {
    let lower = line.to_ascii_lowercase();
    if !["env", "getproperty", "server"].iter().any(|w| lower.contains(w)) {
        return Vec::new();
    }
    let bytes = line.as_bytes();
    (1..=bytes.len())
        .filter(|&at| matches!(bytes[at - 1], b'"' | b'\'' | b'`' | b'.'))
        .filter_map(|at| key_in_code(line, at).filter(|key| key.start == at && !key.is_empty()))
        .collect()
}

/// `$KEY` being typed (not yet `${KEY}`) at byte `at` of a `.env` line: from the `$` on.
pub fn dollar_at(line: &str, at: usize) -> Option<Range<usize>> {
    let (key, before) = word(line, at, false);
    let dollar = before.strip_suffix('$')?;
    (!dollar.ends_with('\\') && key.end == at).then_some(key.start - 1..key.end)
}

/// The key at byte `at` of a `.env` line when it is inside `${…}`.
pub fn key_in_nested(line: &str, at: usize) -> Option<Range<usize>> {
    let (key, before) = word(line, at, true);
    before.ends_with("${").then_some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(marked: &str) -> Option<String> {
        let at = marked.find('|').unwrap();
        let line = marked.replace('|', "");
        key_in_code(&line, at).map(|r| line[r].to_owned())
    }

    #[test]
    fn finds_keys_in_env_calls() {
        assert_eq!(key("$a = getenv('DB_|HOST');"), Some("DB_HOST".into()));
        assert_eq!(key("os.environ.get(\"|\")"), Some("".into()));
        assert_eq!(key("val x = System.getenv(\"A|PP\")"), Some("APP".into()));
        assert_eq!(key("ENV.fetch( 'X|' )"), Some("X".into()));
        assert_eq!(key("std::env::var(\"RUST_|LOG\")"), Some("RUST_LOG".into()));
        assert_eq!(key("os.Getenv(`HO|ME`)"), Some("HOME".into()));
        assert_eq!(key("const u = process.env.API_|URL;"), Some("API_URL".into()));
        assert_eq!(key("System.getProperty(\"app.n|ame\")"), Some("app.name".into()));
        assert_eq!(key("print(\"API_|URL\")"), None);
        assert_eq!(key("process.env[name|]"), None);
    }

    #[test]
    fn lists_the_keys_a_line_reads() {
        let line = "$a = env('A', getenv(\"B\")) . $_ENV['C'] . process.env.D . print('E');";
        let keys: Vec<_> = keys_in_line(line).into_iter().map(|r| &line[r]).collect();
        assert_eq!(keys, ["A", "B", "C", "D"]);
        assert_eq!(keys_in_line("<env name=\"APP_ENV\" value=\"testing\"/>").len(), 1);
        assert!(keys_in_line("print('A')").is_empty());
    }

    #[test]
    fn finds_a_dollar_being_typed() {
        let line = "A=\"x $DB\"";
        assert_eq!(dollar_at(line, 8).map(|r| &line[r]), Some("$DB"));
        assert_eq!(dollar_at("A=\"\\$DB\"", 7), None);
        assert_eq!(dollar_at("A=\"x DB\"", 7), None);
    }

    #[test]
    fn finds_nested_keys() {
        let line = "URL=\"http://${HO}/\"";
        let at = line.find("HO").unwrap() + 1;
        assert_eq!(key_in_nested(line, at).map(|r| &line[r]), Some("HO"));
        assert_eq!(key_in_nested(line, 6), None);
    }
}
