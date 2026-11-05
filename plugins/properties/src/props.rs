//! The `.properties` grammar of the original `Properties.flex` lexer, Java's
//! escaping rules, and resource-bundle file names.

use openidea_plugin::Range;
use openidea_plugin::text::position;
use std::ops::Range as Span;

/// One `key = value` entry, as byte spans of the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Prop {
    pub key: Span<usize>,
    pub sep: Option<usize>,
    pub value: Span<usize>,
}

impl Prop {
    pub fn delimiter(&self, text: &str) -> char {
        self.sep.map_or(' ', |s| text.as_bytes()[s] as char)
    }

    pub fn contains(&self, at: usize) -> bool {
        self.key.start <= at && at <= self.value.end
    }

    pub fn span(&self) -> Span<usize> {
        self.key.start..self.value.end
    }

    pub fn unescaped_key(&self, text: &str) -> String {
        unescape(&text[self.key.clone()])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Prop(Prop),
    Comment(Span<usize>),
}

impl Item {
    pub fn span(&self) -> Span<usize> {
        match self {
            Item::Prop(p) => p.span(),
            Item::Comment(span) => span.clone(),
        }
    }
}

/// Properties and comments in document order. Comments start with `#` or
/// `!`, a key ends at an unescaped `:`, `=` or whitespace, a backslash before
/// a line break continues the line.
pub fn items(text: &str) -> Vec<Item> {
    let b = text.as_bytes();
    let mut items = Vec::new();
    let mut i = 0;

    while i < b.len() {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\x0c' | b'\n' | b'\r') {
            i += 1;
        }

        if i >= b.len() {
            break;
        }

        if matches!(b[i], b'#' | b'!') {
            let start = i;
            while i < b.len() && !matches!(b[i], b'\n' | b'\r') {
                i += 1;
            }
            items.push(Item::Comment(start..i));
            continue;
        }

        let key_start = i;
        while i < b.len() && !matches!(b[i], b':' | b'=' | b' ' | b'\t' | b'\x0c' | b'\n' | b'\r') {
            i = if b[i] == b'\\' { skip_escape(b, i) } else { i + 1 };
        }
        let key = key_start..i;

        while i < b.len() && matches!(b[i], b' ' | b'\t') {
            i += 1;
        }

        let mut sep = None;
        if i < b.len() && matches!(b[i], b':' | b'=') {
            sep = Some(i);
            i += 1;
            while i < b.len() && matches!(b[i], b' ' | b'\t') {
                i += 1;
            }
        }

        let value_start = i;
        while i < b.len() && !matches!(b[i], b'\n' | b'\r') {
            i = if b[i] == b'\\' { skip_escape(b, i) } else { i + 1 };
        }

        items.push(Item::Prop(Prop { key, sep, value: value_start..i }));
    }

    items
}

pub fn parse(text: &str) -> Vec<Prop> {
    items(text)
        .into_iter()
        .filter_map(|item| match item {
            Item::Prop(p) => Some(p),
            Item::Comment(_) => None,
        })
        .collect()
}

/// Index after the escape at `i`; a backslash before CRLF continues the line.
pub fn skip_escape(b: &[u8], i: usize) -> usize {
    match b.get(i + 1) {
        Some(b'\r') if b.get(i + 2) == Some(&b'\n') => i + 3,
        Some(_) => i + 2,
        None => i + 1,
    }
}

/// `PropertyImpl.unescape`: the key or value as `java.util.Properties` reads it.
pub fn unescape(s: &str) -> String {
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\\' {
            out.extend(c.encode_utf16(&mut [0; 2]).iter());
            continue;
        }

        match chars.next() {
            Some('u') => {
                let hex: String = (0..4).map_while(|_| chars.next_if(char::is_ascii_hexdigit)).collect();
                match u16::from_str_radix(&hex, 16) {
                    Ok(unit) if hex.len() == 4 => out.push(unit),
                    _ => out.extend(format!("\\u{hex}").encode_utf16()),
                }
            }
            Some('t') => out.push('\t' as u16),
            Some('n') => out.push('\n' as u16),
            Some('r') => out.push('\r' as u16),
            Some('f') => out.push('\x0c' as u16),
            Some('\r' | '\n') => {
                chars.next_if_eq(&'\n');
                while chars.next_if(|c| matches!(c, ' ' | '\t')).is_some() {}
            }
            Some(other) => out.extend(other.encode_utf16(&mut [0; 2]).iter()),
            None => {}
        }
    }

    String::from_utf16_lossy(&out)
}

/// `Properties.saveConvert` for a key: separators, whitespace and a leading
/// comment character are escaped.
pub fn escape_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            ' ' | ':' | '=' => {
                out.push('\\');
                out.push(c);
            }
            '#' | '!' if i == 0 => {
                out.push('\\');
                out.push(c);
            }
            _ => push_escaped(&mut out, c),
        }
    }
    out
}

/// `Properties.saveConvert` for a value: only a leading space needs escaping.
pub fn escape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, c) in s.chars().enumerate() {
        match c {
            ' ' if i == 0 => out.push_str("\\ "),
            _ => push_escaped(&mut out, c),
        }
    }
    out
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '\\' => out.push_str("\\\\"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\x0c' => out.push_str("\\f"),
        _ => out.push(c),
    }
}

/// `Character.isWhitespace`, which leaves out the no-break spaces.
pub fn java_whitespace(c: char) -> bool {
    matches!(c, '\u{1c}'..='\u{1f}') || (c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}' | '\u{85}'))
}

/// `PropertyImpl.trailingSpaces`: char index where the trailing whitespace of
/// a key or value starts, escaped whitespace (`\t`, ` `…) included.
pub fn trailing_spaces(s: &[char]) -> Option<usize> {
    let len = s.len();
    let mut off = 0;
    let mut start: Option<usize> = None;

    while off < len {
        let c = s[off];
        off += 1;

        if c != '\\' {
            if java_whitespace(c) {
                start.get_or_insert(off - 1);
            } else {
                start = None;
            }
            continue;
        }

        start.get_or_insert(off - 1);
        let Some(&escaped) = s.get(off) else { break };
        off += 1;

        match escaped {
            'u' => {
                let mut value = 0;
                let mut valid = true;
                for i in 0..4 {
                    let digit = s.get(off).and_then(|h| h.to_digit(16));
                    if off < len {
                        off += 1;
                    }
                    let Some(digit) = digit else {
                        off = (off.saturating_sub(i + 1) + 4).min(len);
                        start = None;
                        valid = false;
                        break;
                    };
                    value = (value << 4) + digit;
                }

                if !valid {
                    continue;
                }

                if char::from_u32(value).is_some_and(java_whitespace) {
                    start.get_or_insert(off - 1);
                } else {
                    start = None;
                }
            }
            '\n' => {
                while off < len && matches!(s[off], ' ' | '\t') {
                    off += 1;
                }
            }
            't' | 'r' | 'n' | 'f' => {}
            other if java_whitespace(other) => {}
            _ => start = None,
        }
    }

    start
}

/// The `ignoreVisibleSpaces` variant: only raw spaces and tabs count.
pub fn trailing_visible_spaces(s: &[char]) -> Option<usize> {
    match s.iter().rposition(|c| !matches!(c, ' ' | '\t')) {
        Some(last) if last + 1 == s.len() => None,
        Some(last) => Some(last + 1),
        None => Some(0),
    }
}

/// The property's lines, line break included, for removing it.
pub fn whole_lines(text: &str, p: &Prop) -> Span<usize> {
    let start = text[..p.key.start].rfind('\n').map_or(0, |i| i + 1);
    let rest = &text[p.value.end..];
    let end = p.value.end + if rest.starts_with("\r\n") { 2 } else { usize::from(rest.starts_with(['\n', '\r'])) };
    start..end
}

/// An edit adding `line` at the end of `text`, on a line of its own.
pub fn append_line(text: &str, line: &str) -> (Span<usize>, String) {
    let line_break = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    (text.len()..text.len(), format!("{line_break}{line}\n"))
}

const ISO_639_1: &str = "aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce ch co cr cs cu cv cy \
da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ii ik in \
io is it iu iw ja ji jv ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mr ms \
mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg si sk sl sm sn so \
sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu";

/// `PropertiesUtil.getDefaultBaseName`: `messages_fr_CA` → (`messages`, `fr_CA`).
// ponytail: two-letter ISO 639-1 codes only; IntelliJ also accepts the JDK's three-letter ones (add them if a bundle needs it).
pub fn split_locale(stem: &str) -> (&str, &str) {
    for (i, _) in stem.match_indices('_') {
        let suffix = &stem[i + 1..];
        let language = suffix.split('_').next().unwrap_or_default();
        let rest_ok = suffix.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'));
        if rest_ok && language.len() == 2 && ISO_639_1.split(' ').any(|code| code == language) {
            return (&stem[..i], suffix);
        }
    }
    (stem, "")
}

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn stem(path: &str) -> &str {
    let name = file_name(path);
    name.strip_suffix(".properties").unwrap_or(name)
}

pub fn dir(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

pub fn suffix(path: &str) -> &str {
    split_locale(stem(path)).1
}

pub fn range_of(text: &str, span: &Span<usize>) -> Range {
    Range::new(position(text, span.start), position(text, span.end))
}

pub fn locale_name(suffix: &str) -> &str {
    if suffix.is_empty() { "default" } else { suffix }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_separators_comments_and_continuations() {
        let text = "# c\n! c\na=1\nb : 2\nc 3\nd\\=x=4\nlong=one \\\n    two\nempty\n";
        let props = parse(text);
        let pairs: Vec<(String, String)> =
            props.iter().map(|p| (p.unescaped_key(text), unescape(&text[p.value.clone()]))).collect();

        assert_eq!(
            pairs,
            [("a", "1"), ("b", "2"), ("c", "3"), ("d=x", "4"), ("long", "one two"), ("empty", "")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(props[2].delimiter(text), ' ');
        assert_eq!(items(text).len(), 8);
    }

    #[test]
    fn unescapes_like_java_properties() {
        assert_eq!(unescape(r"café\tx\\y\:z"), "café\tx\\y:z");
        assert_eq!(unescape(r"😀"), "😀");
        assert_eq!(unescape(r"bad\u12"), r"bad\u12");
    }

    #[test]
    fn escapes_round_trip() {
        for s in ["my key", "#tag", "a=b:c", "tab\there", "back\\slash"] {
            assert_eq!(unescape(&escape_key(s)), s);
        }
        for s in [" leading", "multi\nline", "x = y # z"] {
            assert_eq!(unescape(&escape_value(s)), s);
        }
        assert_eq!(escape_value("a b"), "a b");
    }

    #[test]
    fn finds_trailing_spaces_like_intellij() {
        let chars = |s: &str| s.chars().collect::<Vec<_>>();

        assert_eq!(trailing_spaces(&chars("value  ")), Some(5));
        assert_eq!(trailing_spaces(&chars(r"value\t")), Some(5));
        assert_eq!(trailing_spaces(&chars(r"value ")), Some(5));
        assert_eq!(trailing_spaces(&chars(r"value\\")), None);
        assert_eq!(trailing_spaces(&chars("value")), None);
        assert_eq!(trailing_visible_spaces(&chars(r"value\t")), None);
        assert_eq!(trailing_visible_spaces(&chars("value \t")), Some(5));
    }

    #[test]
    fn splits_locale_suffixes() {
        assert_eq!(split_locale("messages_fr_CA"), ("messages", "fr_CA"));
        assert_eq!(split_locale("my_messages_de"), ("my_messages", "de"));
        assert_eq!(split_locale("app_db"), ("app_db", ""));
        assert_eq!(split_locale("messages"), ("messages", ""));
    }
}
