//! `.env` lexer and property model, after the original plugin's JFlex lexer
//! (`DotEnvLexer.flex`) and grammar (`DovEnvGrammar.bnf`). Offsets are UTF-8
//! bytes; every delimiter is ASCII, so token bounds are char boundaries.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Comment,
    Export,
    Key,
    Separator,
    Value,
    Quote,
    NestedStart,
    NestedKey,
    NestedEnd,
    Space,
    Bad,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub range: Range<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Initial,
    AfterExport,
    Value,
    Quoted,
    SingleQuoted,
    Comment,
    Nested,
    NestedEnd,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | 0x0c)
}

fn is_newline(b: u8) -> bool {
    matches!(b, b'\n' | b'\r')
}

fn is_key_byte(b: u8) -> bool {
    !matches!(b, b':' | b'=' | b' ' | b'\t' | 0x0c | b'\n' | b'\r' | b'\\' | b'$' | b'{' | b'}')
}

/// `KEY_CHARACTER+`: anything but separators, blanks and `\${}`, or `\ `.
fn key_len(s: &[u8]) -> usize {
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' && s.get(i + 1) == Some(&b' ') {
            i += 2;
        } else if is_key_byte(s[i]) {
            i += 1;
        } else {
            break;
        }
    }
    i
}

/// `[eE][xX][pP][oO][rR][tT](" ")+`
fn export_len(s: &[u8]) -> usize {
    if s.len() < 7 || !s[..6].eq_ignore_ascii_case(b"export") {
        return 0;
    }
    let spaces = s[6..].iter().take_while(|&&b| b == b' ').count();
    if spaces == 0 {
        return 0;
    }
    6 + spaces
}

fn blank_len(s: &[u8]) -> usize {
    s.iter().take_while(|&&b| is_space(b) || is_newline(b)).count()
}

fn line_len(s: &[u8]) -> usize {
    s.iter().position(|&b| is_newline(b)).unwrap_or(s.len())
}

/// `{WHITE_SPACE}* "}"`
fn nested_end_len(s: &[u8]) -> usize {
    let spaces = s.iter().take_while(|&&b| is_space(b)).count();
    if s.get(spaces) == Some(&b'}') { spaces + 1 } else { 0 }
}

pub fn lex(text: &str) -> Vec<Token> {
    let s = text.as_bytes();
    let mut tokens = Vec::new();
    let mut state = State::Initial;
    let mut at = 0;
    while at < s.len() {
        let rest = &s[at..];
        let blanks = blank_len(rest);
        // The rules every state shares: blanks go back to a key, anything else is a bad character.
        let fallback = |state| {
            if blanks > 0 {
                (Kind::Space, blanks, State::Initial)
            } else {
                (Kind::Bad, text[at..].chars().next().map_or(1, char::len_utf8), state)
            }
        };
        let (kind, len, next) = match state {
            State::Initial => {
                let key = key_len(rest);
                let export = export_len(rest);
                if rest[0] == b'#' {
                    (Kind::Comment, line_len(rest), State::Initial)
                } else if export > key {
                    (Kind::Export, export, State::AfterExport)
                } else if key > 0 {
                    (Kind::Key, key, State::Initial)
                } else if matches!(rest[0], b':' | b'=') {
                    (Kind::Separator, 1, State::Value)
                } else {
                    fallback(state)
                }
            }
            State::AfterExport => match key_len(rest) {
                0 => fallback(state),
                key => (Kind::Key, key, State::Initial),
            },
            State::Value => {
                let spaces = rest.iter().take_while(|&&b| is_space(b)).count();
                if blanks > spaces {
                    (Kind::Space, blanks, State::Initial)
                } else if spaces > 0 {
                    (Kind::Space, spaces, State::Value)
                } else if rest[0] == b'"' {
                    (Kind::Quote, 1, State::Quoted)
                } else if rest[0] == b'\'' {
                    (Kind::Quote, 1, State::SingleQuoted)
                } else if rest[0] == b'#' {
                    (Kind::Comment, 1, State::Comment)
                } else {
                    let first = match rest[0] {
                        b'\\' if rest.get(1).is_some_and(|&b| !is_newline(b)) => 2,
                        b'\\' => 0,
                        _ => 1,
                    };
                    if first == 0 {
                        fallback(state)
                    } else {
                        let tail = rest[first..].iter().take_while(|&&b| !is_newline(b) && b != b'#').count();
                        (Kind::Value, first + tail, State::Initial)
                    }
                }
            }
            State::Quoted => {
                if rest[0] == b'"' {
                    (Kind::Quote, 1, State::Comment)
                } else if rest.starts_with(b"${") {
                    (Kind::NestedStart, 2, State::Nested)
                } else {
                    let mut i = 0;
                    while i < rest.len() {
                        match rest[i] {
                            b'\\' if i + 1 < rest.len() => i += 2,
                            b'\\' | b'"' => break,
                            b'$' => match rest.get(i + 1) {
                                Some(b'{' | b'"') | None => break,
                                Some(_) => i += 2,
                            },
                            _ => i += 1,
                        }
                    }
                    if i > 0 {
                        (Kind::Value, i, State::Quoted)
                    } else if rest.starts_with(b"$\"") {
                        (Kind::Value, 1, State::Quoted)
                    } else {
                        fallback(state)
                    }
                }
            }
            State::SingleQuoted => {
                if rest[0] == b'\'' {
                    (Kind::Quote, 1, State::Comment)
                } else {
                    let mut i = 0;
                    while i < rest.len() {
                        match rest[i] {
                            b'\\' if i + 1 < rest.len() => i += 2,
                            b'\\' | b'\'' => break,
                            _ => i += 1,
                        }
                    }
                    if i > 0 { (Kind::Value, i, State::SingleQuoted) } else { fallback(state) }
                }
            }
            State::Comment => {
                let line = line_len(rest);
                if blanks > line {
                    (Kind::Space, blanks, State::Initial)
                } else {
                    (Kind::Comment, line, State::Comment)
                }
            }
            State::Nested | State::NestedEnd => {
                let key = if state == State::Nested { key_len(rest) } else { 0 };
                let end = nested_end_len(rest);
                if key > 0 {
                    (Kind::NestedKey, key, State::NestedEnd)
                } else if end > 0 {
                    (Kind::NestedEnd, end, State::Quoted)
                } else {
                    fallback(state)
                }
            }
        };
        // `\` + a multi-byte character steps into it; never end a token there.
        let mut end = at + len;
        while !text.is_char_boundary(end) {
            end += 1;
        }
        tokens.push(Token { kind, range: at..end });
        state = next;
        at = end;
    }
    tokens
}

/// `property ::= key SEPARATOR value? COMMENT? | key COMMENT?`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub key: Range<usize>,
    pub separator: Option<Range<usize>>,
    /// Unquoted characters, or from the opening to the closing quote.
    pub value: Option<Range<usize>>,
    /// `"` or `'` for a quoted value.
    pub quote: Option<char>,
    /// The value without its quotes.
    pub value_text: String,
}

impl Property {
    /// The key with escaped spaces (`\ `) unescaped.
    pub fn name(&self, text: &str) -> String {
        text[self.key.clone()].replace("\\ ", " ").trim().to_owned()
    }

    /// The value as completion shows it: its first line, `...` when there are more.
    pub fn short_value(&self) -> String {
        match self.value_text.split_once('\n') {
            Some((first, _)) => format!("{}...", first.trim().trim_end_matches('\\').trim()),
            None => self.value_text.trim().to_owned(),
        }
    }
}

pub fn properties(text: &str, tokens: &[Token]) -> Vec<Property> {
    let solid: Vec<&Token> = tokens.iter().filter(|t| t.kind != Kind::Space).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < solid.len() {
        if solid[i].kind != Kind::Key {
            i += 1;
            continue;
        }
        let mut property =
            Property { key: solid[i].range.clone(), separator: None, value: None, quote: None, value_text: String::new() };
        i += 1;
        if let Some(t) = solid.get(i).filter(|t| t.kind == Kind::Separator) {
            property.separator = Some(t.range.clone());
            i += 1;
            match solid.get(i).map(|t| t.kind) {
                Some(Kind::Value) => {
                    property.value = Some(solid[i].range.clone());
                    property.value_text = text[solid[i].range.clone()].to_owned();
                    i += 1;
                }
                Some(Kind::Quote) => {
                    let start = solid[i].range.start;
                    property.quote = text[solid[i].range.clone()].chars().next();
                    let mut end = solid[i].range.end;
                    i += 1;
                    while let Some(t) = solid.get(i) {
                        match t.kind {
                            Kind::Value | Kind::NestedStart | Kind::NestedKey | Kind::NestedEnd => {
                                property.value_text.push_str(&text[t.range.clone()]);
                                end = t.range.end;
                                i += 1;
                            }
                            Kind::Quote => {
                                end = t.range.end;
                                i += 1;
                                break;
                            }
                            _ => break,
                        }
                    }
                    property.value = Some(start..end);
                }
                _ => {}
            }
        }
        out.push(property);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, &str)> {
        lex(text).into_iter().map(|t| (t.kind, &text[t.range])).collect()
    }

    #[test]
    fn lexes_like_the_original() {
        use Kind::*;
        assert_eq!(
            kinds("# c\nexport A=1 # x\nB = \"v ${A} $x\" tail\nC='a\nb'\n"),
            [
                (Comment, "# c"),
                (Space, "\n"),
                (Export, "export "),
                (Key, "A"),
                (Separator, "="),
                (Value, "1 "),
                (Comment, "# x"),
                (Space, "\n"),
                (Key, "B"),
                (Space, " "),
                (Separator, "="),
                (Space, " "),
                (Quote, "\""),
                (Value, "v "),
                (NestedStart, "${"),
                (NestedKey, "A"),
                (NestedEnd, "}"),
                (Value, " $x"),
                (Quote, "\""),
                (Comment, " tail"),
                (Space, "\n"),
                (Key, "C"),
                (Separator, "="),
                (Quote, "'"),
                (Value, "a\nb"),
                (Quote, "'"),
                (Space, "\n"),
            ]
        );
        assert_eq!(kinds("exportX=é")[0], (Key, "exportX"));
        assert_eq!(kinds("A=\"é\\é\"")[3], (Value, "é\\é"));
    }

    #[test]
    fn builds_properties() {
        let text = "export A=1 # x\nB=\"v ${A}\nw\"\nC\nD='q'";
        let props = properties(text, &lex(text));
        let summary: Vec<_> = props.iter().map(|p| (p.name(text), p.short_value(), p.quote)).collect();
        assert_eq!(
            summary,
            [
                ("A".into(), "1".into(), None),
                ("B".into(), "v ${A}...".into(), Some('"')),
                ("C".into(), "".into(), None),
                ("D".into(), "q".into(), Some('\'')),
            ]
        );
        assert_eq!(&text[props[1].value.clone().unwrap()], "\"v ${A}\nw\"");
    }
}
