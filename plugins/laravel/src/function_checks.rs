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
        let call_before = before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '>' | ':'));
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
    let body_start = start + 3 + header_end;
    if label.is_empty() {
        return start + 3;
    }

    masked[body_start..]
        .match_indices('\n')
        .find(|(at, _)| masked[body_start + at + 1..].trim_start().starts_with(label))
        .map_or(masked.len(), |(at, _)| body_start + at + 1 + masked[body_start + at + 1..].find(label).unwrap_or(0) + label.len())
}
