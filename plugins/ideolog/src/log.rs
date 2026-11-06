//! Log parsing and highlighting rules, ported from Ideolog: the default
//! patterns of `DefaultSettingsStoreItems`, format detection of
//! `IdeologDocumentContext`, the lexer of `LogFileLexer`, the highlighting of
//! `LogHighlightingIterator` and the `<LogParsingPattern>` /
//! `<LogHighlightingPattern>` elements of Ideolog's settings export.

use fancy_regex::Regex;
use openidea_plugin::syntax::Highlight;
use std::ops::Range;

const DETECTION_LINES: usize = 25;
const MAX_PARSING_LENGTH: usize = 10_000;

macro_rules! level {
    () => {
        "(?i:TRACE|DEBUG|INFO|NOTICE|WARN(?:ING)?|ERROR|ERR|FATAL|CRIT(?:ICAL)?|SEVERE|ALERT|EMERG(?:ENCY)?)"
    };
}

const SYSLOG_UUID: &str = "0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0005";
const DOCKER: &str = concat!(r"^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:[.,]\d+)?(?:Z|[+-]\d\d:?\d\d)?)\s+(", level!(), r")\s+(.*)$");
const PYTHON_BASIC: &str = r"^(DEBUG|INFO|WARNING|ERROR|CRITICAL):([^:\s]+):(.*)$";
const PYTHON_DASH: &str = concat!(r"^(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d,\d{3}) - (\S+) - (", level!(), r") - (.*)$");
const NGINX: &str = r"^(\d{4}/\d\d/\d\d \d\d:\d\d:\d\d) \[(\w+)] (\d+#\d+): (.*)$";
const SYSLOG: &str = r"^([A-Z][a-z]{2}\s+\d{1,2}\s\d\d:\d\d:\d\d)\s(\S+)\s([^:\s]+(?:\[\d+])?):\s(.*)$";
const MONOLOG: &str = concat!(
    r"^\[(\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d(?:\.\d+)?(?:Z|[+-]\d\d:?\d\d)?)]\s+([\w-]+)\.(",
    level!(),
    r"):\s*(.*)$"
);
const SERILOG: &str =
    r"^(\d{4}-\d\d-\d\d[ T]\d\d:\d\d:\d\d(?:\.\d+)?\s[+-]\d\d:\d\d)\s\[(VRB|DBG|INF|WRN|ERR|FTL)]\s(.*)$";
const ERROR_WORDS: &str = r"\b(error|fatal|fail(ed|ure)?|critical|panic)\b";
const WARN_WORDS: &str = r"\bwarn(ing)?\b";

pub struct Format {
    pub uuid: String,
    pattern: Regex,
    line_start: Regex,
    pub time_column: Option<usize>,
    pub severity_column: Option<usize>,
    pub category_column: Option<usize>,
    /// Formats without a level column (syslog): the level comes from keywords in the message.
    pub level_from_message: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Line,
    Field,
    Match,
}

struct Rule {
    pattern: Regex,
    format: Option<String>,
    column: Option<usize>,
    action: Action,
    kind: Highlight,
}

/// Parsing and highlighting patterns: Ideolog's defaults, then the user's.
pub struct Config {
    formats: Vec<Format>,
    rules: Vec<Rule>,
    pub errors: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, PartialEq, Eq)]
pub struct LogToken {
    pub range: Range<usize>,
    pub separator: bool,
}

pub struct Event<'a> {
    pub text: &'a str,
    pub offset: usize,
    pub lines: Range<u32>,
}

/// An event split into columns, relative to the event.
pub struct Parsed<'a> {
    pub event: &'a str,
    pub tokens: Vec<LogToken>,
}

fn is_match(pattern: &Regex, text: &str) -> bool {
    // A pattern that backtracks too much counts as not matching, like
    // Ideolog's timed-out matchers.
    pattern.is_match(text).unwrap_or(false)
}

fn column(index: i64) -> Option<usize> {
    usize::try_from(index).ok()
}

impl Default for Config {
    fn default() -> Self {
        let formats = [
            ("b5772998-bf1e-4d9d-ab41-da0b86451163", r"^(?s)([^|]*)\|([^|]*)\|([^|]*)\|(.*)$", r"^\d", [0, 1, 2]),
            (
                "8a0e8992-94cb-4f4c-8be2-42b03609626b",
                r"^([^\[]+)(\[[\s\d]+])\s*(\w*)\s*-\s*(\S*)\s*-(.+)$",
                r"^\d",
                [0, 2, 3],
            ),
            ("e9fa2755-8390-42f5-a41e-a909c58c8cf9", r"^\[([^]]+)](.):\s*(\[[^]]+])?(.*)$", r"^\[", [0, 1, 2]),
            (
                "b8fcb4d4-b1b8-4681-90f1-42f7c02aaf67",
                r"^(.+)(?:\s+\d*\s+\d*\s+)(V|D|I|W|E)\s([^:]+):(.*)$",
                r"^\d",
                [0, 1, 2],
            ),
            (
                "19dd1738-1dc7-4df6-b437-18e0800b7782",
                r"^(\d{4}-\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}\.\d{3}\s)\|(\s[A-Z]*\s*)\|(\s.+:.+:\d+\s-\s.*)$",
                r"^\d",
                [0, 1, 2],
            ),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0001", DOCKER, r"^\d{4}-\d\d-\d\dT", [0, 1, -1]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0002", PYTHON_BASIC, r"^[A-Z]+:", [-1, 0, 1]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0003", PYTHON_DASH, r"^\d{4}-\d\d-\d\d ", [0, 2, 1]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0004", NGINX, r"^\d{4}/\d\d/\d\d ", [0, 1, 2]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0006", MONOLOG, r"^\[\d{4}-", [0, 2, 1]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0007", SERILOG, r"^\d{4}-\d\d-\d\d[ T]", [0, 1, -1]),
            ("0f1d6a52-4c3e-4b7a-9d21-5a0b7c1e0005", SYSLOG, r"^[A-Z][a-z]{2}\s+\d", [0, -1, 2]),
        ]
        .into_iter()
        .map(|(uuid, pattern, line_start, [time, severity, category])| {
            let mut format = Format::new(uuid, pattern, line_start, time, severity, category).unwrap();
            format.level_from_message = pattern == SYSLOG;
            format
        })
        .collect();

        let rules = [
            (r"^\s*(e(rr(or)?)?|severe|fatal|ftl|crit(ical)?|alert|emerg(ency)?)\s*$", Highlight::Keyword, None, None),
            (r"^\s*w(arn(ing)?|rn)?\s*$", Highlight::Tag, None, None),
            (r"^\s*i(nfo|nf)?\s*$", Highlight::String, None, None),
            (ERROR_WORDS, Highlight::Keyword, Some(SYSLOG_UUID), Some(3)),
            (WARN_WORDS, Highlight::Tag, Some(SYSLOG_UUID), Some(3)),
        ]
        .into_iter()
        .map(|(pattern, kind, format, column)| {
            Rule::new(pattern, format.map(str::to_owned), column, Action::Line, kind).unwrap()
        })
        .collect();

        Self { formats, rules, errors: Vec::new() }
    }
}

impl Format {
    fn new(uuid: &str, pattern: &str, line_start: &str, time: i64, severity: i64, category: i64) -> Result<Self, String> {
        // Ideolog compiles parsing patterns with DOTALL.
        let compile = |p: &str, flags: &str| Regex::new(&format!("{flags}{p}")).map_err(|e| format!("{p}: {e}"));
        Ok(Self {
            uuid: uuid.to_owned(),
            pattern: compile(pattern, "(?s)")?,
            line_start: compile(line_start, "")?,
            time_column: column(time),
            severity_column: column(severity),
            category_column: column(category),
            level_from_message: false,
        })
    }
}

impl Rule {
    fn new(pattern: &str, format: Option<String>, column: Option<usize>, action: Action, kind: Highlight) -> Result<Self, String> {
        // Ideolog compiles highlighting patterns CASE_INSENSITIVE.
        let pattern = Regex::new(&format!("(?i){pattern}")).map_err(|e| format!("{pattern}: {e}"))?;
        Ok(Self { pattern, format, column, action, kind })
    }

    /// `column` counts columns only; Ideolog's field and match rules count
    /// separators too, which no capture group number corresponds to.
    fn applies(&self, format: Option<&Format>, column: usize) -> bool {
        let format_ok = self.format.as_ref().is_none_or(|uuid| format.is_some_and(|f| &f.uuid == uuid));
        format_ok && self.column.is_none_or(|c| c == column)
    }
}

impl Config {
    /// Ideolog's defaults plus the patterns of an Ideolog settings export
    /// (`<LogParsingPattern …/>`, `<LogHighlightingPattern …/>` elements, as in
    /// the files of Ideolog's custom log formats wiki). Invalid ones are left
    /// out and listed in `errors`.
    pub fn with_custom(xml: &str) -> Self {
        let mut config = Self::default();
        let mut custom_formats = Vec::new();
        for (element, attrs) in elements(xml) {
            if attr(&attrs, "enabled") == Some("false") {
                continue;
            }

            let int = |name: &str| attr(&attrs, name).and_then(|v| v.parse::<i64>().ok()).unwrap_or(-1);
            let result = match element {
                "LogParsingPattern" => Format::new(
                    attr(&attrs, "uuid").unwrap_or_default(),
                    attr(&attrs, "pattern").unwrap_or_default(),
                    attr(&attrs, "linePattern").unwrap_or("^"),
                    int("timeId"),
                    int("severityId"),
                    int("categoryId"),
                )
                .map(|format| custom_formats.push(format)),
                _ => custom_rule(&attrs).map(|rule| config.rules.extend(rule)),
            };
            if let Err(e) = result {
                config.errors.push(e);
            }
        }
        // Ahead of the built-ins: the first of equally good formats wins.
        config.formats.splice(0..0, custom_formats);
        config
    }

    /// The format matching most of the first lines, `None` for plain logs.
    pub fn detect(&self, text: &str) -> Option<&Format> {
        let first_lines: Vec<&str> = lines(text).take(DETECTION_LINES).map(|(_, line)| line).collect();
        let mut best: Option<(&Format, usize)> = None;
        for format in &self.formats {
            let matches = first_lines.iter().filter(|line| is_match(&format.pattern, line)).count();
            if matches > 0 && best.is_none_or(|(_, most)| matches > most) {
                best = Some((format, matches));
            }
        }
        best.map(|(format, _)| format)
    }

    /// Highlight kinds of an event's ranges (relative to the event), port of
    /// `LogHighlightingIterator.reparsePieces`. Ranges don't overlap.
    pub fn highlight(&self, format: Option<&Format>, parsed: &Parsed) -> Vec<(Range<usize>, Highlight)> {
        let values = parsed.values();
        let line_kind = self.line_kind(format, &values);
        let time_column = format.and_then(|f| f.time_column);

        let mut out = Vec::new();
        let mut column = 0;
        for token in &parsed.tokens {
            let text = &parsed.event[token.range.clone()];
            if token.separator {
                if let Some(kind) = line_kind {
                    out.push((token.range.clone(), kind));
                }
                continue;
            }

            let field_kind = self
                .rules
                .iter()
                .filter(|r| r.action == Action::Field && r.applies(format, column))
                .find(|r| is_match(&r.pattern, text))
                .map(|r| r.kind);
            let is_coloured_time = line_kind.is_some() && Some(column) == time_column;
            let kind = if is_coloured_time { Some(Highlight::Number) } else { field_kind.or(line_kind) };

            let matches = self.match_pieces(format, column, text);
            let mut at = 0;
            for (piece, piece_kind) in matches {
                if let Some(kind) = kind.filter(|_| piece.start > at) {
                    out.push((token.range.start + at..token.range.start + piece.start, kind));
                }
                out.push((token.range.start + piece.start..token.range.start + piece.end, piece_kind));
                at = piece.end;
            }
            if let Some(kind) = kind.filter(|_| text.len() > at) {
                out.push((token.range.start + at..token.range.end, kind));
            }
            column += 1;
        }
        out
    }

    /// Port of the `HIGHLIGHT_LINE` loop: a rule tied to a column stops at
    /// its first match (or a missing column), the others let later rules win.
    fn line_kind(&self, format: Option<&Format>, values: &[&str]) -> Option<Highlight> {
        let mut kind = None;
        for rule in self.rules.iter().filter(|r| r.action == Action::Line) {
            if !rule.format.as_ref().is_none_or(|uuid| format.is_some_and(|f| &f.uuid == uuid)) {
                continue;
            }

            if let Some(column) = rule.column {
                let Some(value) = values.get(column) else { break };
                if is_match(&rule.pattern, value) {
                    kind = Some(rule.kind);
                    break;
                }
                continue;
            }

            if values.iter().any(|value| is_match(&rule.pattern, value)) {
                kind = Some(rule.kind);
            }
        }
        kind
    }

    /// `HIGHLIGHT_MATCH`: the capture groups of every match, sorted, without overlaps.
    fn match_pieces(&self, format: Option<&Format>, column: usize, text: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut pieces = Vec::new();
        for rule in self.rules.iter().filter(|r| r.action == Action::Match && r.applies(format, column)) {
            for captures in rule.pattern.captures_iter(text).map_while(Result::ok) {
                pieces.extend(captures.iter().skip(1).flatten().filter(|m| !m.range().is_empty()).map(|m| (m.range(), rule.kind)));
            }
        }
        pieces.sort_by_key(|(range, _)| (range.start, range.end));
        let mut end = 0;
        pieces.retain(|(range, _)| {
            let keep = range.start >= end;
            if keep {
                end = range.end;
            }
            keep
        });
        pieces
    }
}

impl<'a> Parsed<'a> {
    pub fn new(format: Option<&Format>, event: &'a str) -> Self {
        Self { event, tokens: tokenize(format, event) }
    }

    pub fn values(&self) -> Vec<&'a str> {
        self.tokens.iter().filter(|t| !t.separator).map(|t| &self.event[t.range.clone()]).collect()
    }

    fn value(&self, column: Option<usize>) -> Option<&'a str> {
        column.and_then(|c| self.values().get(c).copied()).map(str::trim)
    }

    pub fn time(&self, format: Option<&Format>) -> Option<&'a str> {
        self.value(format.and_then(|f| f.time_column))
    }

    pub fn category(&self, format: Option<&Format>) -> Option<&'a str> {
        self.value(format.and_then(|f| f.category_column)).filter(|c| !c.is_empty())
    }

    /// The first line of the last column, Ideolog's `LogEvent.message`.
    pub fn message(&self) -> &'a str {
        let last = self.tokens.iter().rev().find(|t| !t.separator).map(|t| &self.event[t.range.clone()]);
        last.and_then(|m| m.lines().next()).unwrap_or_default().trim()
    }

    /// From the severity column (the first column of plain logs), normalized
    /// like Ideolog's `LogEvent.level`.
    pub fn severity(&self, format: Option<&Format>) -> Option<Severity> {
        if format.is_some_and(|f| f.level_from_message) {
            return message_severity(self.message());
        }

        let column = format.map_or(Some(0), |f| f.severity_column);
        let level = self.value(column)?.to_ascii_uppercase();
        match level.trim_matches(|c: char| !c.is_ascii_alphabetic()) {
            "E" | "ERR" | "ERROR" | "FTL" | "SEVERE" | "FATAL" | "CRIT" | "CRITICAL" | "ALERT" | "EMERG" | "EMERGENCY" => {
                Some(Severity::Error)
            }
            "W" | "WRN" | "WARN" | "WARNING" => Some(Severity::Warning),
            _ => None,
        }
    }
}

fn message_severity(message: &str) -> Option<Severity> {
    let words = || message.split(|c: char| !c.is_alphanumeric()).map(str::to_ascii_lowercase);
    if words().any(|w| matches!(w.as_str(), "error" | "fatal" | "fail" | "failed" | "failure" | "critical" | "panic")) {
        return Some(Severity::Error);
    }
    words().any(|w| matches!(w.as_str(), "warn" | "warning")).then_some(Severity::Warning)
}

/// Lines without their terminator, with their byte offset.
pub fn lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_inclusive('\n').scan(0, |offset, line| {
        let start = *offset;
        *offset += line.len();
        Some((start, line.trim_end_matches(['\n', '\r'])))
    })
}

fn is_event_start(format: Option<&Format>, line: &str) -> bool {
    match format {
        Some(format) => is_match(&format.line_start, line),
        None => line.chars().next().is_some_and(|c| !c.is_whitespace()),
    }
}

/// Events: a line starting an event plus the continuation lines after it.
pub fn events<'a>(format: Option<&Format>, text: &'a str) -> Vec<Event<'a>> {
    let mut events: Vec<Event> = Vec::new();
    for (index, (offset, line)) in lines(text).enumerate() {
        let index = index as u32;
        let continues = !events.is_empty() && !is_event_start(format, line);
        if continues {
            let event = events.last_mut().unwrap();
            event.lines.end = index + 1;
            event.text = &text[event.offset..offset + line.len()];
            continue;
        }

        events.push(Event { text: &text[offset..offset + line.len()], offset, lines: index..index + 1 });
    }
    events
}

pub fn floor_char_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `LogFileLexer.lexRegex` / `lexPlainLog`: an event's columns and the
/// separators between them, in offsets relative to the event.
fn tokenize(format: Option<&Format>, event: &str) -> Vec<LogToken> {
    let whole = || vec![LogToken { range: 0..event.len(), separator: false }];
    let Some(format) = format else {
        return match event.find(' ') {
            Some(space) if space > 0 => vec![
                LogToken { range: 0..space, separator: false },
                LogToken { range: space..space + 1, separator: true },
                LogToken { range: space + 1..event.len(), separator: false },
            ],
            _ => whole(),
        };
    };

    let first_line_end = if event.len() > MAX_PARSING_LENGTH {
        Some(floor_char_boundary(event, MAX_PARSING_LENGTH))
    } else {
        event.find('\n')
    };
    let first_line = &event[..first_line_end.unwrap_or(event.len())];
    let Ok(Some(captures)) = format.pattern.captures(first_line.trim_end_matches('\r')) else {
        return whole();
    };

    let mut tokens = Vec::new();
    let mut last_end = 0;
    for group in captures.iter().skip(1).flatten() {
        if group.start() < last_end {
            continue;
        }

        if group.start() > last_end {
            tokens.push(LogToken { range: last_end..group.start(), separator: true });
        }
        tokens.push(LogToken { range: group.range(), separator: false });
        last_end = group.end();
    }

    if tokens.is_empty() {
        return whole();
    }

    if first_line_end.is_some() {
        tokens.last_mut().unwrap().range.end = event.len();
        return tokens;
    }

    let match_end = captures.get(0).unwrap().end();
    if last_end < match_end {
        tokens.push(LogToken { range: last_end..match_end, separator: true });
    }
    tokens
}

fn custom_rule(attrs: &[(String, String)]) -> Result<Option<Rule>, String> {
    let action = match attr(attrs, "action").unwrap_or("HIGHLIGHT_LINE") {
        "HIGHLIGHT_LINE" => Action::Line,
        "HIGHLIGHT_FIELD" => Action::Field,
        "HIGHLIGHT_MATCH" => Action::Match,
        // HIDE: no API to fold or hide lines.
        _ => return Ok(None),
    };
    let colour = attr(attrs, "fg").or_else(|| attr(attrs, "bg")).and_then(colour_kind);
    let Some(kind) = colour else { return Ok(None) };

    let format = attr(attrs, "formatId").filter(|id| !id.is_empty()).map(str::to_owned);
    let column = attr(attrs, "captureGroup").and_then(|v| v.parse::<i64>().ok()).and_then(column);
    Rule::new(attr(attrs, "pattern").unwrap_or_default(), format, column, action, kind).map(Some)
}

/// The theme kind closest to an Ideolog colour: a signed ARGB integer, or
/// `{"lightRgb": …, "darkRgb": …}` (the light one is used).
fn colour_kind(value: &str) -> Option<Highlight> {
    let digits = value.trim_start_matches(|c: char| c != '-' && !c.is_ascii_digit());
    let end = digits.char_indices().skip(1).find(|(_, c)| !c.is_ascii_digit()).map_or(digits.len(), |(i, _)| i);
    let argb = digits[..end].parse::<i64>().ok()? as u32;
    let [r, g, b] = [(argb >> 16) & 0xff, (argb >> 8) & 0xff, argb & 0xff].map(|c| c as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    if max - min < 0.15 {
        return Some(Highlight::Comment);
    }

    let delta = max - min;
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    Some(match hue {
        h if !(20.0..330.0).contains(&h) => Highlight::Keyword,
        h if h < 70.0 => Highlight::Tag,
        h if h < 170.0 => Highlight::String,
        h if h < 260.0 => Highlight::Function,
        _ => Highlight::Property,
    })
}

/// `<Name attr="…" …/>` elements of the two pattern kinds, attributes unescaped.
fn elements(xml: &str) -> Vec<(&'static str, Vec<(String, String)>)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let name = ["LogParsingPattern", "LogHighlightingPattern"]
            .into_iter()
            .find(|n| rest.strip_prefix(n).is_some_and(|after| after.starts_with(char::is_whitespace)));
        let Some(name) = name else { continue };
        let Some(end) = rest.find('>') else { break };
        out.push((name, attributes(&rest[name.len()..end])));
        rest = &rest[end..];
    }
    out
}

fn attributes(mut text: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    while let Some(eq) = text.find("=\"") {
        let name = text[..eq].trim().to_owned();
        let value_start = eq + 2;
        let Some(len) = text[value_start..].find('"') else { break };
        attrs.push((name, unescape(&text[value_start..value_start + len])));
        text = &text[value_start + len + 1..];
    }
    attrs
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else { break };
        let decoded = match &rest[1..semi] {
            "quot" => Some('"'),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "apos" => Some('\''),
            entity => entity
                .strip_prefix("#x")
                .map(|hex| u32::from_str_radix(hex, 16))
                .or_else(|| entity.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LARAVEL: &str = r#"<State><parsingPatterns><LogParsingPattern enabled="true" name="Laravel" pattern="^\[([\d\-: ]*)] (?!:)[\s\S]*?\.(.*?(?=:)): ([\s\S]*?(?=(^\[[\d\-: ]*])|\Z))$" timePattern="yyyy-MM-dd HH:mm:ss" linePattern="^\[\d" timeId="0" severityId="1" categoryId="-1" fullmatch="false" uuid="9a75fe1c-24f0-4e5d-8359-ce4dbb9c4c33" /></parsingPatterns></State>"#;

    const SERILOG: &str = r#"<State>
  <highlightingPatterns>
    <LogHighlightingPattern enabled="true" pattern="^\s*err\s*$" captureGroup="-1" action="HIGHLIGHT_LINE" fg="-39836" bold="true" italic="false" stripe="true" uuid="5bcf6a90-78f1-43a4-9a8c-375910f23b12" />
    <LogHighlightingPattern enabled="true" pattern="^\s*wrn\s*$" captureGroup="-1" action="HIGHLIGHT_LINE" fg="-6329600" bold="true" italic="false" stripe="false" uuid="3cb63bd6-2f81-4bb6-8944-0a0a674cb427" />
    <LogHighlightingPattern enabled="true" pattern="^\s*inf\s*$" captureGroup="-1" action="HIGHLIGHT_LINE" fg="-10316203" bold="false" italic="false" stripe="false" uuid="0f8ad928-941b-4ebe-8e50-ec601c49b5a7" />
  </highlightingPatterns>
  <parsingPatterns>
    <LogParsingPattern enabled="true" name="Serilog" pattern="^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} [+-]?\d{2}:\d{2}) \[([A-Z]+)\] (.*)$" timePattern="yyyy-MM-dd HH:mm:ss.SSS XXX" linePattern="^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}" timeId="0" severityId="1" categoryId="-1" fullmatch="false" uuid="496412b0-aed0-4b48-a539-30522f90d3ce" />
  </parsingPatterns>
</State>"#;

    fn kinds(config: &Config, text: &str) -> Vec<(String, Highlight)> {
        let format = config.detect(text);
        let parsed = Parsed::new(format, text);
        config.highlight(format, &parsed).into_iter().map(|(r, k)| (text[r].to_owned(), k)).collect()
    }

    #[test]
    fn imports_ideolog_formats_with_lookaround() {
        let config = Config::with_custom(LARAVEL);
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        let line = "[2024-05-01 10:00:00] production.ERROR: Undefined variable $user";
        let format = config.detect(line).unwrap();
        assert_eq!(format.uuid, "9a75fe1c-24f0-4e5d-8359-ce4dbb9c4c33");
        let parsed = Parsed::new(Some(format), line);
        assert_eq!(parsed.values(), ["2024-05-01 10:00:00", "ERROR", "Undefined variable $user"]);
        assert_eq!(parsed.severity(Some(format)), Some(Severity::Error));
    }

    #[test]
    fn imports_highlighting_patterns_with_their_colour() {
        let config = Config::with_custom(SERILOG);
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        let line = "2024-05-01 10:00:00.000 +02:00 [WRN] disk almost full";
        assert!(kinds(&config, line).contains(&("WRN".to_owned(), Highlight::Tag)));
        let line = "2024-05-01 10:00:00.000 +02:00 [ERR] disk full";
        assert!(kinds(&config, line).contains(&("ERR".to_owned(), Highlight::Keyword)));
    }

    #[test]
    fn field_and_match_rules_colour_parts_of_a_line() {
        let xml = r#"
            <LogHighlightingPattern pattern="^\s*debug\s*$" captureGroup="2" action="HIGHLIGHT_FIELD" fg="-8355712" />
            <LogHighlightingPattern pattern="user=(\w+)" action="HIGHLIGHT_MATCH" fg="-16776961" />
            <LogHighlightingPattern pattern="secret" action="HIDE" fg="-65536" />"#;
        let config = Config::with_custom(xml);
        let line = "2024-05-01 10:00:00,123 [   1234]  DEBUG - #c.i.Auth - login user=alice ok";
        let kinds = kinds(&config, line);
        assert!(kinds.contains(&("DEBUG".to_owned(), Highlight::Comment)));
        assert!(kinds.contains(&("alice".to_owned(), Highlight::Function)));
        assert_eq!(kinds.len(), 2);
    }

    #[test]
    fn invalid_patterns_are_reported_not_fatal() {
        let config = Config::with_custom(r#"<LogParsingPattern pattern="([" linePattern="^" uuid="x" />"#);
        assert_eq!(config.errors.len(), 1);
        assert_eq!(config.formats.len(), Config::default().formats.len());
    }

    #[test]
    fn disabled_patterns_are_skipped() {
        let xml = r#"<LogHighlightingPattern enabled="false" pattern="x" fg="-65536" />"#;
        assert_eq!(Config::with_custom(xml).rules.len(), Config::default().rules.len());
    }

    #[test]
    fn colours_map_to_the_nearest_theme_kind() {
        assert_eq!(colour_kind("-65536"), Some(Highlight::Keyword));
        assert_eq!(colour_kind(r#"{"lightRgb":-16744448,"darkRgb":-10316203}"#), Some(Highlight::String));
        assert_eq!(colour_kind("-8355712"), Some(Highlight::Comment));
        assert_eq!(colour_kind("x"), None);
    }

    #[test]
    fn unescapes_xml_entities() {
        assert_eq!(unescape("msg=&quot;(.*)&quot; &amp;&lt;&gt; &#65;&#x42; &bogus"), r#"msg="(.*)" &<> AB &bogus"#);
    }
}
