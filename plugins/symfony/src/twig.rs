//! Twig language support independent of the project index: tag, filter,
//! function and test names (Twig's public documentation) and a check for
//! block tags that are never closed.

use std::ops::Range;

use crate::refs::{is_identifier, literal_at, twig_code_start, word_at};

pub enum Slot {
    Tag,
    Filter,
    Function,
    Test,
}

const PAIRED: &[&str] = &["apply", "autoescape", "block", "cache", "embed", "for", "if", "macro", "sandbox", "verbatim", "with"];

const TAGS: &[&str] = &[
    "apply", "autoescape", "block", "cache", "deprecated", "do", "else", "elseif", "embed", "extends", "flush", "for", "from", "if", "import",
    "include", "macro", "sandbox", "set", "types", "use", "verbatim", "with", "endapply", "endautoescape", "endblock", "endcache", "endembed",
    "endfor", "endif", "endmacro", "endsandbox", "endset", "endverbatim", "endwith",
];

const FILTERS: &[&str] = &[
    "abs", "batch", "capitalize", "column", "convert_encoding", "country_name", "currency_name", "currency_symbol", "data_uri", "date",
    "date_modify", "default", "e", "escape", "filter", "find", "first", "format", "format_currency", "format_date", "format_datetime",
    "format_number", "format_time", "inline_css", "join", "json_encode", "keys", "language_name", "last", "length", "locale_name", "lower", "map",
    "markdown_to_html", "merge", "nl2br", "number_format", "raw", "reduce", "replace", "reverse", "round", "slice", "slug", "sort", "spaceless",
    "split", "striptags", "sum", "timezone_name", "title", "trim", "u", "upper", "url_encode",
];

const FUNCTIONS: &[&str] = &[
    "attribute", "block", "constant", "cycle", "date", "dump", "enum", "enum_cases", "html_classes", "include", "max", "min", "parent", "random",
    "range", "source", "template_from_string",
];

const TESTS: &[&str] = &["constant", "defined", "divisible by", "empty", "even", "iterable", "mapping", "null", "odd", "same as"];

pub fn names(slot: &Slot) -> &'static [&'static str] {
    match slot {
        Slot::Tag => TAGS,
        Slot::Filter => FILTERS,
        Slot::Function => FUNCTIONS,
        Slot::Test => TESTS,
    }
}

/// What kind of name is being typed at byte offset `at` in Twig code, and the
/// word's range.
pub fn slot_at(text: &str, at: usize) -> Option<(Slot, usize, usize)> {
    let code = twig_code_start(text, at)?;
    if literal_at("twig", text, at).is_some() {
        return None;
    }

    let (start, end) = word_at(text, at, is_identifier);
    if text.as_bytes().get(start).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    let before = text[code + 2..start].trim_start_matches(['-', '~']).trim_end();
    if text[code..].starts_with("{%") && before.is_empty() {
        return Some((Slot::Tag, start, end));
    }

    let mut words = before.rsplit(char::is_whitespace);
    let last = words.next().unwrap_or_default();
    let slot = match before.bytes().last() {
        Some(b'|') => Slot::Filter,
        Some(b'.') => return None,
        _ if last == "is" || (last == "not" && words.next() == Some("is")) => Slot::Test,
        _ => Slot::Function,
    };

    Some((slot, start, end))
}

/// Block tags without their `{% end… %}`: the tag name's range and the name.
pub fn unclosed_tags(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut open: Vec<(Range<usize>, &str)> = Vec::new();
    let mut unclosed = Vec::new();
    let mut i = 0;

    while let Some(found) = text[i..].find('{') {
        let at = i + found;
        i = at + 1;

        if text[at..].starts_with("{#") {
            i = text[at..].find("#}").map_or(text.len(), |e| at + e + 2);
            continue;
        }

        if !text[at..].starts_with("{%") {
            continue;
        }

        let Some(close) = text[at..].find("%}").map(|e| at + e) else { break };
        let inside = &text[at + 2..close];
        let trimmed = inside.trim_start_matches(['-', '~']).trim_start();
        let name_start = at + 2 + (inside.len() - trimmed.len());
        let name_len = trimmed.bytes().position(|b| !is_identifier(b)).unwrap_or(trimmed.len());
        let name = &trimmed[..name_len];
        let rest = trimmed[name_len..].trim_end_matches(['-', '~']);
        i = close + 2;

        if let Some(opened) = name.strip_prefix("end") {
            if let Some(position) = open.iter().rposition(|(_, n)| *n == opened) {
                unclosed.extend(open.split_off(position + 1));
                open.pop();
            }
            continue;
        }

        if name == "verbatim" {
            let end = text[i..].find("endverbatim");
            i = end.map_or(text.len(), |e| i + e);
            if end.is_some() {
                continue;
            }
        }

        let inline_block = name == "block" && rest.split_whitespace().count() > 1;
        if PAIRED.contains(&name) && !inline_block {
            open.push((name_start..name_start + name_len, name));
        }
    }

    unclosed.extend(open);
    unclosed
}
