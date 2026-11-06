//! Function-call checks of one PHP file.

use openidea_plugin::Severity;

use crate::eloquent_checks::Finding;
use crate::php;

const QUIET_DIRECTORIES: &[&str] = &["/config/", "/bootstrap/", "/vendor/", "/tests/"];

/// `env()` called outside the configuration files returns null once the configuration is cached.
pub fn env_outside_config(path: &str, language: &str, text: &str) -> Vec<Finding> {
    if language != "php" || QUIET_DIRECTORIES.iter().any(|directory| path.contains(directory)) {
        return Vec::new();
    }

    let masked = php::mask_comments(text);
    let bytes = masked.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;

    while at < bytes.len() {
        if bytes[at] == b'\'' || bytes[at] == b'"' {
            at = php::string_end(bytes, at);
            continue;
        }

        if masked[at..].starts_with("<<<") {
            at = heredoc_end(&masked, at);
            continue;
        }

        if !masked[at..].starts_with("env") {
            at += 1;
            continue;
        }

        let before = &masked[..at];
        let is_identifier = |c: char| c.is_alphanumeric() || c == '_';
        let call_before = match before.strip_suffix('\\') {
            Some(namespace) => namespace.chars().next_back().is_some_and(is_identifier),
            None => before.chars().next_back().is_some_and(|c| is_identifier(c) || matches!(c, '$' | '>' | ':')),
        };
        let is_definition = before.trim_end().ends_with("function") && before.trim_end().len() < before.len();
        let rest = &masked[at + 3..];
        let open = at + 3 + (rest.len() - rest.trim_start().len());
        if call_before || is_definition || bytes.get(open) != Some(&b'(') {
            at += 3;
            continue;
        }

        let Some(close) = php::closing(&masked, open) else { break };
        found.push(Finding {
            start: at,
            end: close + 1,
            code: "env-outside-config",
            message: "env() outside config files returns null once the configuration is cached: read a config() value instead.".into(),
            severity: Severity::Warning,
            replacement: None,
        });
        at = close + 1;
    }

    found
}

/// The index after the heredoc or nowdoc that starts at `start` (the end of the text when it isn't closed).
fn heredoc_end(masked: &str, start: usize) -> usize {
    let after = &masked[start + 3..];
    let header_end = after.find('\n').unwrap_or(after.len());
    let label = after[..header_end].trim().trim_matches(|c| c == '\'' || c == '"');
    if label.is_empty() {
        return start + 3;
    }

    let mut line_start = start + 3 + header_end;
    for line in masked[line_start..].split_inclusive('\n').skip(1) {
        line_start += masked[line_start..].find('\n').map_or(0, |i| i + 1);
        let indent = line.len() - line.trim_start().len();
        let tail = &line[indent..];
        let closes = tail.strip_prefix(label).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'));
        if closes {
            return line_start + indent + label.len();
        }
    }

    masked.len()
}
