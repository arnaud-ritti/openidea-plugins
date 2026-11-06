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

fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_owned();
    }

    raw.replace("\\\"", "\"").replace("\\/", "/").replace("\\\\", "\\")
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
