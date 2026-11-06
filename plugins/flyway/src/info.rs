//! `flyway info -outputType=json`: one row per migration with its state.

use serde_json::Value;

pub struct Migration {
    pub label: String,
    pub state: &'static str,
    pub icon: &'static str,
    pub repeatable: bool,
    pub version: String,
    pub description: String,
    pub filepath: Option<String>,
}

fn state(raw: &str) -> (&'static str, &'static str) {
    let raw = raw.to_lowercase();
    match raw.as_str() {
        "success" | "baseline" => ("Applied", "success"),
        "pending" => ("Pending", "pending"),
        "ignored" => ("Ignored", "skip"),
        "deleted" => ("Deleted", "trash"),
        "outdated" | "superseded" | "undone" => ("Superseded", "history"),
        _ if raw.contains("failed") => ("Failed", "error"),
        _ if raw.contains("missing") => ("Missing", "warning"),
        _ if raw.contains("out of order") => ("Out of order", "warning"),
        _ if raw.contains("above target") || raw.contains("below baseline") || raw.contains("future") => ("Not applicable", "clock"),
        _ => ("Unknown", "info"),
    }
}

/// Flyway 10 and 11 print `{"migrations": [...]}` with more fields around it; `mvn` wraps it in log lines.
pub fn parse(stdout: &str) -> Result<Vec<Migration>, String> {
    let json = stdout.find('{').zip(stdout.rfind('}')).and_then(|(from, to)| stdout.get(from..=to));
    let value: Value = json
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or_else(|| "Flyway info did not return JSON. Check the arguments setting and the Flyway version (10 or newer).".to_owned())?;
    let rows = value.get("migrations").and_then(Value::as_array).ok_or("Flyway info returned no migrations list.")?;

    let text = |row: &Value, key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    Ok(rows
        .iter()
        .map(|row| {
            let repeatable = text(row, "category").eq_ignore_ascii_case("repeatable");
            let version = text(row, "version");
            let description = text(row, "description");
            let prefix = if repeatable { "R" } else { "V" };
            let (state, icon) = state(&text(row, "state"));
            Migration {
                label: format!("{prefix}{version} {description}").trim_end().to_owned(),
                state,
                icon,
                repeatable,
                version,
                description,
                filepath: Some(text(row, "filepath")).filter(|path| !path.is_empty()),
            }
        })
        .collect())
}
