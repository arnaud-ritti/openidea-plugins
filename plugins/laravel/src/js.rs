//! Ziggy route names in JavaScript, TypeScript and Vue: `route('name')` and
//! `route().current('name')`. Line-local scan, no parsing of the document.

use std::ops::Range;

pub const LANGUAGES: &[&str] = &["javascript", "typescript", "vue", "javascriptreact", "typescriptreact"];

pub fn is_script(path: &str, language: &str) -> bool {
    LANGUAGES.contains(&language) || [".js", ".jsx", ".ts", ".tsx", ".vue", ".mjs"].iter().any(|e| path.ends_with(e))
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$')
}

/// Byte range of the route name inside the string literal at `at`, when that
/// string is the first argument of `route(` or `route().current(`.
// ponytail: a string spanning lines or a block comment opened above the line is not seen.
pub fn route_name_at(text: &str, at: usize) -> Option<Range<usize>> {
    if !text.contains("route(") && !text.contains(".current(") {
        return None;
    }

    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[..line_start];
    if before.rfind("/*") > before.rfind("*/") && before.contains("/*") {
        return None;
    }

    let bytes = text.as_bytes();
    let mut open: Option<(u8, usize)> = None;
    let mut i = line_start;
    while i < at {
        let byte = bytes[i];
        match open {
            Some(_) if byte == b'\\' => i += 1,
            Some((quote, _)) if byte == quote => open = None,
            Some(_) if matches!(byte, b'\'' | b'"' | b'`') && is_route_call(&text[..i]) => open = Some((byte, i)),
            Some(_) => {}
            None if matches!(byte, b'\'' | b'"' | b'`') => open = Some((byte, i)),
            None if bytes[i..].starts_with(b"//") || bytes[i..].starts_with(b"/*") => return None,
            None => {}
        }
        i += 1;
    }

    let (quote, quote_at) = open?;
    if !is_route_call(&text[..quote_at]) {
        return None;
    }

    let start = quote_at + 1;
    let mut end = start;
    while end < text.len() && bytes[end] != quote && bytes[end] != b'\n' {
        end += 1;
    }
    Some(start..end)
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
