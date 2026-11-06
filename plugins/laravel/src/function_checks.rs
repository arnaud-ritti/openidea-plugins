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
    let mut found = Vec::new();

    for (at, _) in masked.match_indices("env") {
        let before = &masked[..at];
        let call_before = before.chars().next_back().is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '\\' | '>' | ':'));
        let is_definition = before.trim_end().ends_with("function") && before.trim_end().len() < before.len();
        if call_before || is_definition {
            continue;
        }

        let rest = &masked[at + 3..];
        let open = at + 3 + (rest.len() - rest.trim_start().len());
        if masked.as_bytes().get(open) != Some(&b'(') {
            continue;
        }

        let Some(close) = php::closing(&masked, open) else { continue };
        found.push(Finding {
            start: at,
            end: close + 1,
            code: "env-outside-config",
            message: "env() outside config files returns null once the configuration is cached: read a config() value instead.".into(),
            severity: Severity::Warning,
            replacement: None,
        });
    }

    found
}
