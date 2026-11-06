//! The custom patterns the user adds from the Patterns view, kept in plugin
//! storage as Ideolog `<LogParsingPattern/>` and `<LogHighlightingPattern/>`
//! elements, one per line, next to the `customPatterns` setting.

use crate::log::{self, Config};
use openidea_plugin::views::{Body, Button, Table, View, ViewEvent};
use openidea_plugin::{Error, NotificationLevel, PickItem, host};

pub const VIEW: &str = "patterns";
const STORAGE_KEY: &str = "patterns";

const COLOURS: [(&str, &str, i32); 6] = [
    ("red", "Red (errors)", -39836),
    ("yellow", "Yellow (warnings)", -6329600),
    ("green", "Green (info)", -10316203),
    ("blue", "Blue", 0xFF4080FFu32 as i32),
    ("purple", "Purple", 0xFF9966CCu32 as i32),
    ("grey", "Grey", 0xFF808080u32 as i32),
];

const ACTIONS: [(&str, &str); 3] = [
    ("HIGHLIGHT_LINE", "Highlight the whole line"),
    ("HIGHLIGHT_FIELD", "Highlight a field"),
    ("HIGHLIGHT_MATCH", "Highlight the matching text"),
];

pub fn stored() -> String {
    host::storage::get(STORAGE_KEY).ok().flatten().unwrap_or_default()
}

pub fn combined(setting: &str) -> String {
    format!("{setting}\n{}", stored())
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn uuid(seed: &str) -> String {
    let hash = seed.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    format!("c0570000-0000-4000-8000-{:012x}", hash & 0xffff_ffff_ffff)
}

pub fn format_element(name: &str, pattern: &str, line_pattern: &str, time: &str, severity: &str, category: &str) -> String {
    format!(
        "<LogParsingPattern enabled=\"true\" name=\"{}\" pattern=\"{}\" linePattern=\"{}\" timeId=\"{}\" severityId=\"{}\" categoryId=\"{}\" uuid=\"{}\" />",
        escape(name),
        escape(pattern),
        escape(line_pattern),
        time.trim(),
        severity.trim(),
        category.trim(),
        uuid(&format!("{name}{pattern}")),
    )
}

pub fn highlight_element(pattern: &str, action: &str, argb: i32) -> String {
    format!(
        "<LogHighlightingPattern enabled=\"true\" pattern=\"{}\" captureGroup=\"-1\" action=\"{action}\" fg=\"{argb}\" uuid=\"{}\" />",
        escape(pattern),
        uuid(&format!("{pattern}{action}{argb}")),
    )
}

fn entries() -> Vec<String> {
    stored().lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect()
}

fn save(entries: &[String]) -> Result<(), Error> {
    host::storage::set(STORAGE_KEY, &entries.join("\n"))
}

fn describe(entry: &str) -> Vec<String> {
    let Some((kind, attrs)) = log::elements(entry).into_iter().next() else { return vec!["?".into(), entry.into(), String::new()] };
    let get = |name: &str| log::attr(&attrs, name).unwrap_or_default().to_owned();
    if kind == "LogParsingPattern" {
        return vec!["Format".into(), get("name"), get("pattern")];
    }
    let how = ACTIONS.iter().find(|(id, _)| *id == get("action")).map_or("", |(_, label)| *label);
    vec!["Highlighting".into(), how.into(), get("pattern")]
}

pub fn render() -> View {
    let rows: Vec<Vec<String>> = entries().iter().map(|e| describe(e)).collect();
    let status = match rows.len() {
        0 => "No patterns yet. The customPatterns setting takes pasted Ideolog XML too.".to_owned(),
        1 => "1 pattern".to_owned(),
        n => format!("{n} patterns"),
    };
    let body = if rows.is_empty() {
        Body::Message("No custom formats or highlighting patterns yet.".into())
    } else {
        Body::Table(Table { columns: vec!["Kind".into(), "Name or action".into(), "Pattern".into()], total_rows: rows.len() as u32, first_row: 0, rows, editable: false })
    };
    View::new(body)
        .button(Button::new("add-format", "Add Format").icon("add"))
        .button(Button::new("add-highlight", "Add Highlighting").icon("add"))
        .button(Button::new("remove", "Remove").icon("remove").enabled(!entries().is_empty()))
        .status(&status)
}

fn ask(prompt: &str, default: &str) -> Result<Option<String>, Error> {
    host::input(prompt, Some(default))
}

fn add(element: String) -> Result<(), Error> {
    let errors = Config::with_custom(&element).errors;
    if let Some(error) = errors.first() {
        host::notify(NotificationLevel::Warning, "Ideolog", &format!("Pattern not added: {error}"));
        return Ok(());
    }

    let mut all = entries();
    all.push(element);
    save(&all)
}

fn add_format() -> Result<(), Error> {
    let Some(name) = ask("Format name", "")? else { return Ok(()) };
    let Some(pattern) = ask("Regular expression with a group per column", "^(\\S+) (\\w+) (.*)$")? else { return Ok(()) };
    let Some(line_pattern) = ask("Regular expression the first line of an event matches", "^")? else { return Ok(()) };
    let Some(time) = ask("Group of the time column (-1: none)", "0")? else { return Ok(()) };
    let Some(severity) = ask("Group of the level column (-1: none)", "1")? else { return Ok(()) };
    let Some(category) = ask("Group of the category column (-1: none)", "-1")? else { return Ok(()) };
    add(format_element(&name, &pattern, &line_pattern, &time, &severity, &category))
}

fn add_highlight() -> Result<(), Error> {
    let Some(pattern) = ask("Regular expression (case-insensitive)", "")? else { return Ok(()) };
    let items: Vec<PickItem> = ACTIONS.iter().map(|(id, label)| PickItem { key: (*id).into(), label: (*label).into(), description: None }).collect();
    let Some(action) = host::pick("What to highlight", &items)? else { return Ok(()) };
    let items: Vec<PickItem> = COLOURS.iter().map(|(id, label, _)| PickItem { key: (*id).into(), label: (*label).into(), description: None }).collect();
    let Some(colour) = host::pick("Colour", &items)? else { return Ok(()) };
    let argb = COLOURS.iter().find(|(id, ..)| *id == colour).map_or(0, |(.., argb)| *argb);
    add(highlight_element(&pattern, &action, argb))
}

fn remove() -> Result<(), Error> {
    let all = entries();
    let items: Vec<PickItem> = all
        .iter()
        .enumerate()
        .map(|(n, e)| {
            let row = describe(e);
            PickItem { key: n.to_string(), label: format!("{}: {}", row[0], row[2]), description: Some(row[1].clone()) }
        })
        .collect();
    let Some(chosen) = host::pick("Remove pattern", &items)? else { return Ok(()) };
    let at = chosen.parse::<usize>().map_err(|_| Error::Invalid(chosen))?;
    save(&all.iter().enumerate().filter(|(n, _)| *n != at).map(|(_, e)| e.clone()).collect::<Vec<_>>())
}

pub fn handle(event: ViewEvent) -> Result<(), Error> {
    let ViewEvent::Button(id) = event else { return Ok(()) };
    match id.as_str() {
        "add-format" => add_format()?,
        "add-highlight" => add_highlight()?,
        "remove" => remove()?,
        _ => return Ok(()),
    }
    host::refresh_view(VIEW);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_colours_map_back_to_their_kind() {
        use openidea_plugin::syntax::Highlight::*;
        let kinds = [Keyword, Tag, String, Function, Property, Comment];
        for ((_, _, argb), kind) in COLOURS.iter().zip(kinds) {
            let config = Config::with_custom(&highlight_element("x", "HIGHLIGHT_LINE", *argb));
            assert!(config.errors.is_empty());
            assert_eq!(log::colour_kind_of(*argb), Some(kind));
        }
    }

    #[test]
    fn a_format_element_round_trips_through_the_parser() {
        let element = format_element("Mine", r#"^(\d+) "(\w+)" (.*)$"#, "^\\d", "0", "1", "-1");
        let config = Config::with_custom(&element);
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        let text = "12 \"WARN\" careful\n";
        let format = config.detect(text).unwrap();
        assert_eq!(format.name, "Mine");
        assert_eq!(format.severity_column, Some(1));
    }
}
