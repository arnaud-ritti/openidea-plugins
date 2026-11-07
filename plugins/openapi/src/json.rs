//! Key paths of a JSON text, as the YAML scanner reports them, so the same checks run on both.

use crate::spec::Key;

const MAX_DEPTH: usize = 64;

struct Scanner<'t> {
    text: &'t str,
    at: usize,
    keys: Vec<Key>,
}

pub fn keys(text: &str) -> Vec<Key> {
    let mut scanner = Scanner { text, at: 0, keys: Vec::new() };
    scanner.value("", 0);
    scanner.keys
}

pub fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }

        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => out.push(unicode(&mut chars)),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }

    out
}

/// The offset of the `}` or `]` closing the container opened at `open`.
pub fn container_end(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

struct Frame {
    object: bool,
    key: Option<String>,
    expect_key: bool,
}

/// Where the caret sits in an object: the keys of the enclosing objects, and whether it is
/// on a key or on the value of `key`.
pub struct Caret {
    pub path: Vec<String>,
    pub key: Option<String>,
    pub value: bool,
    pub in_string: bool,
    pub from: usize,
}

pub fn caret(text: &str, offset: usize) -> Option<Caret> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    let mut string_start = None;
    while i < offset {
        match bytes[i] {
            c if c.is_ascii_whitespace() || c == b':' => i += 1,
            b'{' | b'[' => {
                stack.push(Frame { object: bytes[i] == b'{', key: None, expect_key: true });
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                let frame = stack.last_mut()?;
                frame.key = None;
                frame.expect_key = true;
                i += 1;
            }
            b'"' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != b'"' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if end >= offset {
                    string_start = Some(start);
                    break;
                }
                let frame = stack.last_mut()?;
                if frame.object && frame.expect_key {
                    frame.key = Some(unescape(&text[start..end]));
                    frame.expect_key = false;
                }
                i = end + 1;
            }
            _ => {
                while i < offset && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b',' | b'}' | b']' | b':') {
                    i += 1;
                }
                if i >= offset {
                    return None;
                }
            }
        }
    }

    let top = stack.last().filter(|frame| frame.object)?;
    let after_colon = text[..offset].trim_end().ends_with(':');
    let value = !top.expect_key;
    if value && string_start.is_none() && !after_colon {
        return None;
    }
    if !value && string_start.is_none() && after_colon {
        return None;
    }

    let key = top.key.clone();
    let mut path = Vec::new();
    for frame in &stack[..stack.len() - 1] {
        if frame.object {
            path.push(frame.key.clone()?);
        }
    }

    Some(Caret { path, key, value, in_string: string_start.is_some(), from: string_start.unwrap_or(offset) })
}

fn hex4(chars: &mut std::str::Chars) -> Option<u32> {
    let digits: String = chars.clone().take(4).collect();
    let code = u32::from_str_radix(&digits, 16).ok().filter(|_| digits.len() == 4)?;
    chars.nth(3);
    Some(code)
}

fn unicode(chars: &mut std::str::Chars) -> char {
    let Some(high) = hex4(chars) else {
        return '\u{fffd}';
    };
    if !(0xD800..0xDC00).contains(&high) {
        return char::from_u32(high).unwrap_or('\u{fffd}');
    }

    let mut ahead = chars.clone();
    let low = (ahead.next() == Some('\\') && ahead.next() == Some('u')).then(|| hex4(&mut ahead)).flatten();
    match low.filter(|l| (0xDC00..0xE000).contains(l)) {
        Some(low) => {
            *chars = ahead;
            char::from_u32(0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)).unwrap_or('\u{fffd}')
        }
        None => '\u{fffd}',
    }
}

impl Scanner<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.text.as_bytes().get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }

        self.text.as_bytes().get(self.at).copied()
    }

    /// The bytes between the quotes of the string at the cursor.
    fn string(&mut self) -> Option<(usize, usize)> {
        let start = self.at + 1;
        let bytes = self.text.as_bytes();
        let mut i = start;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    self.at = i + 1;
                    return Some((start, i));
                }
                _ => i += 1,
            }
        }

        None
    }

    /// The value's bytes and whether it is an empty container; `None` once the text stops being JSON.
    fn value(&mut self, path: &str, depth: usize) -> Option<(usize, usize, bool)> {
        if depth > MAX_DEPTH {
            return None;
        }

        let first = self.peek()?;
        let begin = self.at;
        match first {
            b'{' => self.object(path, depth).map(|empty| (begin, self.at, empty)),
            b'[' => self.array(path, depth).map(|empty| (begin, self.at, empty)),
            b'"' => self.string().map(|(at, end)| (at, end, false)),
            _ => {
                let rest = &self.text.as_bytes()[begin..];
                let len = rest.iter().position(|b| b.is_ascii_whitespace() || matches!(b, b',' | b'}' | b']')).unwrap_or(rest.len());
                self.at = begin + len;
                (len > 0).then_some((begin, begin + len, false))
            }
        }
    }

    fn object(&mut self, path: &str, depth: usize) -> Option<bool> {
        self.at += 1;
        if self.peek()? == b'}' {
            self.at += 1;
            return Some(true);
        }

        loop {
            if self.peek()? != b'"' {
                return None;
            }
            let (at, end) = self.string()?;
            let name = unescape(&self.text[at..end]);
            if self.peek()? != b':' {
                return None;
            }
            self.at += 1;

            let key = if path.is_empty() { name } else { format!("{path}.{name}") };
            let slot = self.keys.len();
            self.keys.push(Key { key: key.clone(), at, end, ..Key::default() });
            self.member(slot, &key, depth)?;

            match self.peek()? {
                b',' => self.at += 1,
                b'}' => {
                    self.at += 1;
                    return Some(false);
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self, path: &str, depth: usize) -> Option<bool> {
        self.at += 1;
        if self.peek()? == b']' {
            self.at += 1;
            return Some(true);
        }

        for index in 0.. {
            let item = format!("{path}[{index}]");
            if matches!(self.peek()?, b'{' | b'[') {
                self.value(&item, depth + 1)?;
            } else {
                let (at, end, _) = self.value(&item, depth + 1)?;
                let value = unescape(&self.text[at..end]);
                self.keys.push(Key { key: item, at, end, value, value_at: at, value_end: end });
            }

            match self.peek()? {
                b',' => self.at += 1,
                b']' => {
                    self.at += 1;
                    return Some(false);
                }
                _ => return None,
            }
        }

        None
    }

    fn member(&mut self, slot: usize, path: &str, depth: usize) -> Option<()> {
        let opener = self.peek()?;
        let (at, end, empty) = self.value(path, depth + 1)?;
        let container = matches!(opener, b'{' | b'[');
        let key = &mut self.keys[slot];
        match (container, empty) {
            (false, _) => {
                key.value = unescape(&self.text[at..end]);
                (key.value_at, key.value_end) = (at, end);
            }
            (true, true) => {
                key.value = if opener == b'{' { "{}" } else { "[]" }.to_owned();
                (key.value_at, key.value_end) = (at, end);
            }
            (true, false) => (key.value_at, key.value_end) = (at, at),
        }

        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_paths_values_and_stops_quietly_on_broken_json() {
        let text = r#"{"a": {"b": "x\"y", "c": [1, {"d": []}], "e": {}}, "n": null}"#;
        let found: Vec<_> = keys(text).into_iter().map(|k| (k.key, k.value)).collect();
        assert_eq!(found, [
            ("a".into(), "".into()),
            ("a.b".into(), "x\"y".into()),
            ("a.c".into(), "".into()),
            ("a.c[0]".into(), "1".into()),
            ("a.c[1].d".into(), "[]".into()),
            ("a.e".into(), "{}".into()),
            ("n".into(), "null".into()),
        ]);
        let key = &keys(text)[1];
        assert_eq!(&text[key.at..key.end], "b");
        for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
            keys(&text[..end]);
        }
    }
}
