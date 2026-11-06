//! Ziggy route names in JavaScript, TypeScript and Vue: `route('name')` and
//! `route().current('name')`. Line-local scan, no parsing of the document.

use std::ops::Range;

pub const LANGUAGES: &[&str] = &["javascript", "typescript", "vue", "javascriptreact", "typescriptreact"];

pub fn is_script(path: &str, language: &str) -> bool {
    LANGUAGES.contains(&language) || [".js", ".jsx", ".ts", ".tsx", ".vue", ".mjs"].iter().any(|e| path.ends_with(e))
}

pub fn mentions_route(text: &str) -> bool {
    text.contains("route(") || text.contains(".current(")
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

/// Byte range of the route name inside the string literal at `at`, when that
/// string is the first argument of `route(` or `route().current(`.
// ponytail: a string spanning lines or a block comment opened above the line is not seen.
pub fn route_name_at(text: &str, at: usize) -> Option<Range<usize>> {
    let (quote, quote_at) = open_string(text, at)?;
    if !is_route_call(&text[..quote_at]) {
        return None;
    }

    if quote == b'`' && text[quote_at..at].contains("${") {
        return None;
    }

    let bytes = text.as_bytes();
    let start = quote_at + 1;
    let mut end = start;
    while end < text.len() && bytes[end] != quote && bytes[end] != b'\n' {
        end += 1;
    }
    Some(start..end)
}

/// The quote and offset of the string literal open at `at`; none inside a
/// comment or outside any string. Quotes end at the line break, backticks don't.
// ponytail: no regex-literal or Vue text awareness; an apostrophe in template text opens a string until the line ends.
fn open_string(text: &str, at: usize) -> Option<(u8, usize)> {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        Str(u8, usize),
        Line,
        Block,
    }

    let bytes = text.as_bytes();
    let mut state = State::Code;
    let mut i = 0;
    while i < at {
        let byte = bytes[i];
        state = match state {
            State::Code if matches!(byte, b'\'' | b'"' | b'`') => State::Str(byte, i),
            State::Code if bytes[i..].starts_with(b"//") => State::Line,
            State::Code if bytes[i..].starts_with(b"/*") => {
                i += 1;
                State::Block
            }
            State::Line if byte == b'\n' => State::Code,
            State::Block if bytes[i..].starts_with(b"*/") => {
                i += 1;
                State::Code
            }
            State::Str(..) if byte == b'\\' => {
                i += 1;
                state
            }
            State::Str(quote, _) if byte == quote => State::Code,
            State::Str(quote, _) if quote != b'`' && byte == b'\n' => State::Code,
            State::Str(_, _) if matches!(byte, b'\'' | b'"' | b'`') && is_route_call(&text[..i]) => State::Str(byte, i),
            other => other,
        };
        i += 1;
    }

    match state {
        State::Str(quote, start) => Some((quote, start)),
        _ => None,
    }
}

fn is_route_call(before: &str) -> bool {
    let Some(before) = before.trim_end().strip_suffix('(') else { return false };
    let before = before.trim_end();

    if let Some(receiver) = before.strip_suffix(".current") {
        let receiver = receiver.trim_end();
        let Some(receiver) = receiver.strip_suffix(')') else { return false };
        let Some(receiver) = receiver.trim_end().strip_suffix('(') else { return false };
        return is_route_identifier(receiver.trim_end());
    }

    is_route_identifier(before)
}

fn is_route_identifier(before: &str) -> bool {
    let Some(rest) = before.strip_suffix("route") else { return false };
    rest.as_bytes().last().is_none_or(|&b| !is_word(b) && b != b'.')
}
