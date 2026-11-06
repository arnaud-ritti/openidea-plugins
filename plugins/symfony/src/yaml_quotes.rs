//! Plain YAML scalars Symfony's YAML parser refuses: a value starting with
//! `@` (service reference) or `%` (parameter) must be quoted.

use std::ops::Range;

use crate::index::split_yaml_key;

const MAX_LEN: usize = 500_000;

/// Byte ranges of the unquoted scalars that start with `@` or `%`.
pub fn unquoted(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    if text.len() > MAX_LEN {
        return found;
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
        if let Some((_, after_key)) = split_yaml_key(value) {
            value = after_key;
        }

        if value.starts_with(['|', '>']) {
            block_indent = Some(indent);
            continue;
        }

        let value_start = start + (value.as_ptr() as usize - line.as_ptr() as usize);
        scalars(value, value_start, &mut found);
    }

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
            if b == b'@' || b == b'%' {
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
