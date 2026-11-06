//! `flyway info -outputType=json`: one row per migration with its state.

use serde_json::Value;

use crate::naming::Kind;

pub struct Migration {
    pub label: String,
    pub state: &'static str,
    pub icon: &'static str,
    pub kind: Kind,
    pub version: String,
    pub description: String,
    pub filepath: Option<String>,
}

fn state(raw: &str) -> (&'static str, &'static str) {
    let raw = raw.to_lowercase();
    match raw.as_str() {
        "success" | "baseline" => ("Applied", "success"),
        "pending" => ("Pending", "ci-pending"),
        "ignored" => ("Ignored", "ci-skipped"),
        "deleted" => ("Deleted", "trash"),
        "undone" => ("Undone", "history"),
        "outdated" | "superseded" => ("Superseded", "history"),
        _ if raw.contains("missing") => ("Missing", "warning"),
        _ if raw.contains("failed") => ("Failed", "error"),
        _ if raw.contains("out of order") => ("Out of order", "warning"),
        _ if raw.contains("above target") || raw.contains("below baseline") || raw.contains("future") => ("Not applicable", "clock"),
        _ => ("Unknown", "info"),
    }
}

/// Flyway 10 and 11 print `{"migrations": [...]}` with more fields around it; `mvn` wraps it in log lines.
pub fn parse(stdout: &str) -> Result<Vec<Migration>, String> {
    let value: Value = stdout
        .match_indices('{')
        .find_map(|(from, _)| {
            let first = serde_json::Deserializer::from_str(&stdout[from..]).into_iter::<Value>().next()?;
            first.ok().filter(|value| value.get("migrations").is_some())
        })
        .ok_or_else(|| "Flyway info did not return JSON. Check the arguments setting and the Flyway version (10 or newer).".to_owned())?;
    let rows = value.get("migrations").and_then(Value::as_array).ok_or("Flyway info returned no migrations list.")?;

    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(rows
        .iter()
        .map(|row| {
            let (kind, prefix) = match text(row, "category").to_lowercase().as_str() {
                "repeatable" => (Kind::Repeatable, "R"),
                "undo" => (Kind::Undo, "U"),
                "baseline" => (Kind::Baseline, "B"),
                _ => (Kind::Versioned, "V"),
            };
            let version = text(row, "version");
            let description = text(row, "description");
            let (state, icon) = state(&text(row, "state"));
            Migration {
                label: format!("{prefix}{version} {description}").trim_end().to_owned(),
                state,
                icon,
                kind,
                version,
                description,
                filepath: Some(text(row, "filepath")).filter(|path| !path.is_empty()),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const HOST_ICONS: [&str; 9] = ["success", "ci-pending", "error", "warning", "ci-skipped", "trash", "history", "clock", "info"];

    #[test]
    fn states_map_to_labels_and_host_icons() {
        let cases = [
            ("Success", "Applied", "success"),
            ("Baseline", "Applied", "success"),
            ("Pending", "Pending", "ci-pending"),
            ("Failed", "Failed", "error"),
            ("Future Failed", "Failed", "error"),
            ("Missing (Success)", "Missing", "warning"),
            ("Missing (Failed)", "Missing", "warning"),
            ("Out of Order", "Out of order", "warning"),
            ("Ignored", "Ignored", "ci-skipped"),
            ("Undone", "Undone", "history"),
            ("Superseded", "Superseded", "history"),
            ("Deleted", "Deleted", "trash"),
            ("Above Target", "Not applicable", "clock"),
            ("Whatever", "Unknown", "info"),
        ];
        for (raw, label, icon) in cases {
            assert_eq!(state(raw), (label, icon), "{raw}");
            assert!(HOST_ICONS.contains(&icon), "{icon} is not a host icon");
        }
    }

    #[test]
    fn noisy_stdout_with_braces_still_parses() {
        let out = "[INFO] banner {not json} here\n{\"migrations\":[{\"category\":\"Undo\",\"version\":\"2\",\"description\":\"x\",\"state\":\"Pending\"}]}\n[INFO] done }";
        let migrations = parse(out).unwrap();
        assert_eq!(migrations[0].label, "U2 x");
        assert_eq!(migrations[0].kind, Kind::Undo);
    }
}
