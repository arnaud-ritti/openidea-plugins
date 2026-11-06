//! Secret handling: decoding `data` from `kubectl get secret -o json`, and
//! building the manifest of a new Opaque Secret. Values only ever live in
//! these functions' arguments and results; nothing here logs or stores them.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        bits = bits << 6 | ALPHABET.iter().position(|a| *a == c)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Some(out)
}

/// Text when the bytes are UTF-8, otherwise their base64.
pub fn display(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => format!("(binary, base64) {}", b64_encode(bytes)),
    }
}

struct Json<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Json<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        if self.peek() != Some(c) {
            return Err(format!("unexpected JSON at byte {}", self.at));
        }
        self.at += 1;
        Ok(())
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = *self.bytes.get(self.at).ok_or("unterminated JSON string")?;
            self.at += 1;
            match c {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
                b'\\' => {
                    let e = *self.bytes.get(self.at).ok_or("unterminated JSON string")?;
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self.bytes.get(self.at..self.at + 4).and_then(|h| std::str::from_utf8(h).ok());
                            let code = hex.and_then(|h| u32::from_str_radix(h, 16).ok());
                            self.at += 4;
                            let ch = code.and_then(char::from_u32).unwrap_or('\u{fffd}');
                            out.extend(ch.to_string().bytes());
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn skip(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(b'"') => self.string().map(drop),
            Some(open @ (b'{' | b'[')) => {
                let close = if open == b'{' { b'}' } else { b']' };
                self.at += 1;
                while self.peek() != Some(close) {
                    if open == b'{' {
                        self.string()?;
                        self.eat(b':')?;
                    }
                    self.skip()?;
                    if self.peek() == Some(b',') {
                        self.at += 1;
                    }
                }
                self.at += 1;
                Ok(())
            }
            Some(_) => {
                while self.bytes.get(self.at).is_some_and(|c| !b",}] \n\r\t".contains(c)) {
                    self.at += 1;
                }
                Ok(())
            }
            None => Err("truncated JSON".into()),
        }
    }
}

/// The decoded top-level `data` of a Secret, in the order kubectl printed it.
pub fn data_entries(json: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut p = Json { bytes: json.as_bytes(), at: 0 };
    let mut entries = Vec::new();
    p.eat(b'{')?;
    while p.peek() != Some(b'}') {
        let key = p.string()?;
        p.eat(b':')?;
        if key != "data" || p.peek() != Some(b'{') {
            p.skip()?;
        } else {
            p.eat(b'{')?;
            while p.peek() != Some(b'}') {
                let name = p.string()?;
                p.eat(b':')?;
                let value = p.string()?;
                let bytes = b64_decode(&value).ok_or_else(|| format!("the value of {name} is not valid base64"))?;
                entries.push((name, bytes));
                if p.peek() == Some(b',') {
                    p.at += 1;
                }
            }
            p.at += 1;
        }
        if p.peek() == Some(b',') {
            p.at += 1;
        }
    }
    Ok(entries)
}

/// A DNS subdomain: what Kubernetes accepts as a Secret name.
pub fn valid_name(name: &str) -> bool {
    let edge = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    name.len() <= 253
        && name.chars().next().is_some_and(edge)
        && name.chars().last().is_some_and(edge)
        && name.chars().all(|c| edge(c) || c == '-' || c == '.')
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 253
        && key != "."
        && key != ".."
        && key.chars().all(|c| c.is_ascii_alphanumeric() || "-._".contains(c))
}

/// An Opaque Secret manifest. Names and keys are restricted to characters that
/// need no escaping and values are base64, so any value round-trips.
pub fn manifest(name: &str, entries: &[(String, String)]) -> Result<String, String> {
    if !valid_name(name) {
        return Err("The Secret name must be lowercase letters, digits, '-' and '.', starting and ending with a letter or digit.".into());
    }
    if entries.is_empty() {
        return Err("Add at least one key.".into());
    }
    let mut out = format!("apiVersion: v1\nkind: Secret\nmetadata:\n  name: \"{name}\"\ntype: Opaque\ndata:\n");
    for (i, (key, value)) in entries.iter().enumerate() {
        if !valid_key(key) {
            return Err(format!("\"{key}\" is not a valid key: use letters, digits, '-', '_' and '.'."));
        }
        if entries[..i].iter().any(|(k, _)| k == key) {
            return Err(format!("The key \"{key}\" is given twice."));
        }
        out.push_str(&format!("  \"{key}\": {}\n", b64_encode(value.as_bytes())));
    }
    Ok(out)
}

/// `text` without the values (raw or base64), for messages.
pub fn redact(text: &str, entries: &[(String, String)]) -> String {
    let mut text = text.to_owned();
    for (_, value) in entries.iter().filter(|(_, v)| !v.is_empty()) {
        text = text.replace(&b64_encode(value.as_bytes()), "***").replace(value.as_str(), "***");
    }
    text
}
