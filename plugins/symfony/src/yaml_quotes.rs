//! Plain YAML scalars Symfony's YAML parser refuses: a value starting with
//! `@` (service reference) or `%` (parameter) must be quoted.

use std::ops::Range;

use crate::index::split_yaml_key;

const MAX_LEN: usize = 500_000;

/// Calls `visit(offset, value, has_key)` for the value of every line outside
/// comments and block scalars (sequence dashes and the key are stripped).
fn for_each_value(text: &str, mut visit: impl FnMut(usize, &str, bool)) {
    if text.len() > MAX_LEN {
        return;
    }

    let mut offset = 0;
    let mut block_indent: Option<usize> = None;

    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();

        let content = line.trim_end();
        let trimmed = content.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        let indent = content.len() - trimmed.len();
        if let Some(scalar_indent) = block_indent {
            if indent > scalar_indent {
                continue;
            }
            block_indent = None;
        }

        if trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }

        let mut value = trimmed;
        while let Some(rest) = value.strip_prefix("- ") {
            value = rest.trim_start();
        }
        let mut has_key = false;
        if let Some((_, after_key)) = split_yaml_key(value) {
            value = after_key;
            has_key = true;
        }

        if value.starts_with(['|', '>']) && is_block_header(value) {
            block_indent = Some(indent);
            continue;
        }

        let value_start = start + (value.as_ptr() as usize - line.as_ptr() as usize);
        visit(value_start, value, has_key);
    }
}

/// `|` or `>`, then indentation and chomping indicators, then end of line or a comment.
fn is_block_header(value: &str) -> bool {
    let rest = value[1..].trim_start_matches(['+', '-', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9']);
    rest.is_empty() || (rest.starts_with(char::is_whitespace) && rest.trim_start().starts_with('#'))
}

/// Byte ranges of the unquoted scalars that start with `@`, `%`, a backtick,
/// or a `|` or `>` that is no block scalar header.
pub fn unquoted(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    for_each_value(text, |start, value, _| scalars(value, start, &mut found));
    found
}

/// Backslashes inside double-quoted scalars (255 characters at most) that
/// start no YAML escape sequence.
pub fn backslashes(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();

    for_each_value(text, |base, value, _| {
        let bytes = value.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            let opens = bytes[i] == b'"' && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'[' | b'{' | b',' | b':'));
            if !opens {
                i += 1;
                continue;
            }

            let Some(close) = closing_quote(bytes, i) else { return };
            if value[i + 1..close].chars().count() <= 255 {
                let mut at = i + 1;
                while at < close {
                    if bytes[at] != b'\\' {
                        at += 1;
                        continue;
                    }
                    let next = value[at + 1..].chars().next().unwrap_or('"');
                    if !ESCAPES.contains(next) {
                        found.push(base + at..base + at + 1);
                    }
                    at += 1 + next.len_utf8();
                }
            }
            i = close + 1;
        }
    });

    found
}

const ESCAPES: &str = "\\0abtnvfre \"/N_LPxuU";

/// Index of the quote closing the double-quoted scalar opened at `open`.
fn closing_quote(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 1,
            b'"' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Values of a key (200 characters at most, one line) that are plain scalars
/// containing `: `, which YAML reads as a nested mapping.
pub fn colons(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();

    for_each_value(text, |base, value, has_key| {
        if !has_key || value.starts_with(['\'', '"', '[', '{', '|', '>', '&', '*', '!']) {
            return;
        }

        let value = value.find(" #").map_or(value, |at| &value[..at]).trim_end();
        if value.is_empty() || value.chars().count() > 200 {
            return;
        }

        let bytes = value.as_bytes();
        let mut quote: Option<u8> = None;
        let mut depth = 0usize;

        for i in 0..bytes.len() {
            let b = bytes[i];
            let boundary = i == 0 || bytes[i - 1].is_ascii_whitespace();

            if let Some(q) = quote {
                if b == q {
                    quote = None;
                }
                continue;
            }

            match b {
                b'\'' | b'"' if boundary => quote = Some(b),
                b'[' | b'{' => depth += 1,
                b']' | b'}' => depth = depth.saturating_sub(1),
                b':' if depth == 0 && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace) => {
                    found.push(base..base + value.len());
                    return;
                }
                _ => {}
            }
        }
    });

    found
}

/// Plain scalars of a value (itself, or the items of a flow collection).
fn scalars(value: &str, base: usize, found: &mut Vec<Range<usize>>) {
    let bytes = value.as_bytes();
    let flow = value.starts_with(['[', '{']);
    let mut quote: Option<u8> = None;
    let mut at_start = true;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if let Some(q) = quote {
            if b == b'\\' && q == b'"' {
                i += 1;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }

        if at_start && !b.is_ascii_whitespace() {
            at_start = false;
            if matches!(b, b'@' | b'%' | b'`') || (i == 0 && matches!(b, b'|' | b'>')) {
                let end = plain_end(value, i, flow);
                found.push(base + i..base + end);
                i = end;
                continue;
            }
        }

        match b {
            b'\'' | b'"' if i == 0 || bytes[i - 1].is_ascii_whitespace() || matches!(bytes[i - 1], b'[' | b'{' | b',' | b':') => {
                quote = Some(b);
            }
            b'#' if i > 0 && bytes[i - 1].is_ascii_whitespace() => break,
            b'[' | b'{' | b',' if flow => at_start = true,
            b':' if flow && bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace) => at_start = true,
            _ => {}
        }
        i += 1;
    }
}

/// End of the plain scalar starting at `from`: before ` #`, or in a flow
/// collection before `,` `]` `}`; without trailing spaces.
fn plain_end(value: &str, from: usize, flow: bool) -> usize {
    let bytes = value.as_bytes();
    let mut end = from;

    while end < bytes.len() {
        let b = bytes[end];
        if flow && matches!(b, b',' | b']' | b'}') {
            break;
        }
        if b == b'#' && end > from && bytes[end - 1].is_ascii_whitespace() {
            break;
        }
        end += 1;
    }

    from + value[from..end].trim_end().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<&str> {
        unquoted(text).into_iter().map(|r| &text[r]).collect()
    }

    #[test]
    fn unquoted_yaml_at_sign_is_reported() {
        let text = "services:\n    app.a:\n        arguments: [@logger, '@ok', %kernel.debug%]\n        factory: @app.factory # why\n        calls:\n            - [setX, [@x]]\n        tags:\n            - @bad\n        decorates: %env(X)%\n";
        assert_eq!(found(text), ["@logger", "%kernel.debug%", "@app.factory", "@x", "@bad", "%env(X)%"]);
    }

    #[test]
    fn quoted_commented_and_block_values_are_quiet() {
        let text = "services:\n    app.a:\n        arguments: ['@logger', \"@x\", '%kernel.debug%']\n        # factory: @app.factory\n        class: App\\A@1\n        calls:\n            - [setX, ['@x', 50%]]\n        description: |\n            @not a value\n            %neither\n        other: 'it''s @fine' # @comment\n        mail: user@example.com\n";
        assert_eq!(found(text), Vec::<&str>::new());
    }
}
