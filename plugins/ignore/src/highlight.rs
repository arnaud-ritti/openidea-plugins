use openidea_plugin::syntax::{Highlight, Token};

/// The original plugin's token classes: headers (`###`), sections (`##`),
/// comments, `!`, `/`, wildcards, `[…]` classes and escapes; entry text
/// itself stays plain. Only the plugin's `dotignore` files get here, none of
/// which has Mercurial's `syntax:` lines.
pub fn highlights(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut line_start = 0;

    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let at = line_start + line.len() - line.trim_start().len();
        let content = line.trim();
        line_start += raw.len();
        let mut push = |start: usize, end: usize, kind: Highlight| tokens.push(Token { range: at + start..at + end, kind });

        if content.is_empty() {
            continue;
        }

        if content.starts_with('#') {
            let kind = match () {
                _ if content.starts_with("###") => Highlight::Label,
                _ if content.starts_with("##") => Highlight::DocComment,
                _ => Highlight::Comment,
            };
            push(0, content.len(), kind);
            continue;
        }

        let bytes = content.as_bytes();
        let mut i = 0;
        if bytes[0] == b'!' {
            push(0, 1, Highlight::Operator);
            i = 1;
        }
        while i < bytes.len() {
            let len = match bytes[i] {
                b'\\' => {
                    let len = 1 + content[i + 1..].chars().next().map_or(0, char::len_utf8);
                    push(i, i + len, Highlight::Escape);
                    len
                }
                b'/' => {
                    push(i, i + 1, Highlight::Punctuation);
                    1
                }
                b'*' | b'?' => {
                    let len = bytes[i..].iter().take_while(|&&b| b == b'*' || b == b'?').count();
                    push(i, i + len, Highlight::Operator);
                    len
                }
                b'[' => {
                    let len = class_len(&bytes[i..]).unwrap_or(1);
                    push(i, i + len, Highlight::Regexp);
                    len
                }
                _ => 1,
            };
            i += len;
        }
    }

    tokens
}

/// Length of the `[…]` class at the start of `bytes`, with its brackets.
fn class_len(bytes: &[u8]) -> Option<usize> {
    let skip = 1 + usize::from(matches!(bytes.get(1), Some(b'!' | b'^'))) + 1;
    let close = bytes.get(skip..)?.iter().position(|&b| b == b']')?;
    Some(skip + close + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(&str, Highlight)> {
        highlights(text).into_iter().map(|t| (&text[t.range], t.kind)).collect()
    }

    #[test]
    fn comments_headers_and_sections() {
        assert_eq!(
            kinds("### Rust\n## Build\n  # note\n"),
            [("### Rust", Highlight::Label), ("## Build", Highlight::DocComment), ("# note", Highlight::Comment)]
        );
    }

    #[test]
    fn entries() {
        assert_eq!(
            kinds("!/out/**/*.[oa]\\ x\n[]]\n"),
            [
                ("!", Highlight::Operator),
                ("/", Highlight::Punctuation),
                ("/", Highlight::Punctuation),
                ("**", Highlight::Operator),
                ("/", Highlight::Punctuation),
                ("*", Highlight::Operator),
                ("[oa]", Highlight::Regexp),
                ("\\ ", Highlight::Escape),
                ("[]]", Highlight::Regexp),
            ]
        );
    }
}
