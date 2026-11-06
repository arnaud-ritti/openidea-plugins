//! Reading PHP without a parser: comments, strings, brackets and array
//! literals. Everything works on byte offsets of the original text.

use std::ops::Range;

pub fn is_identifier(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The index after the string literal whose opening quote is at `start`
/// (the end of the text when it isn't closed).
pub fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;

    while i < bytes.len() {
        if bytes[i] == quote {
            return i + 1;
        }
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }

    bytes.len()
}

/// The text with every comment blanked out, so offsets stay the same.
pub fn mask_comments(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let mut blank = |range: Range<usize>| {
        for b in &mut out[range] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    };
    let mut i = 0;

    while i < bytes.len() {
        let next = bytes.get(i + 1).copied();
        match bytes[i] {
            b'\'' | b'"' => i = string_end(bytes, i).min(bytes.len()),
            b'/' if next == Some(b'/') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                blank(i..end);
                i = end;
            }
            b'#' if next != Some(b'[') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                blank(i..end);
                i = end;
            }
            b'/' if next == Some(b'*') => {
                let end = text[i + 2..].find("*/").map_or(bytes.len(), |n| i + 2 + n + 2);
                blank(i..end);
                i = end;
            }
            _ => i += 1,
        }
    }

    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// The bracket closing the `(`, `[` or `{` at `open`, skipping strings.
pub fn closing(text: &str, open: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = open;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                i = string_end(bytes, i);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
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

/// `range` without the whitespace around it.
pub fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    let end = range.end - (slice.len() - slice.trim_end().len());
    start..end.max(start)
}

/// Pieces of `range` between commas that aren't inside brackets or strings.
pub fn split(text: &str, range: Range<usize>) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = range.start;
    let mut i = range.start;

    while i < range.end {
        match bytes[i] {
            b'\'' | b'"' => {
                i = string_end(bytes, i).min(range.end);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(start..i);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    if !text[start..range.end].trim().is_empty() {
        parts.push(start..range.end);
    }

    parts
}

/// Arguments of the call whose `(` is at `open`.
pub fn arguments(text: &str, open: usize) -> Vec<Range<usize>> {
    match closing(text, open) {
        Some(close) => split(text, open + 1..close),
        None => Vec::new(),
    }
}

/// Contents and offset of `range` when it is exactly one string literal.
pub fn literal(text: &str, range: Range<usize>) -> Option<(&str, usize)> {
    let range = trimmed(text, range);
    let bytes = text.as_bytes();
    let quote = *bytes.get(range.start)?;

    if (quote != b'\'' && quote != b'"') || string_end(bytes, range.start) != range.end || range.len() < 2 {
        return None;
    }

    Some((&text[range.start + 1..range.end - 1], range.start + 1))
}

/// `App\Foo::class` of `range`: `App\Foo`.
pub fn class_constant(text: &str, range: Range<usize>) -> Option<&str> {
    let range = trimmed(text, range);
    text[range].strip_suffix("::class").map(|class| class.trim_start_matches('\\'))
}

/// Index of a top-level `=>` in `range`.
fn arrow(text: &str, range: Range<usize>) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = range.start;

    while i < range.end {
        match bytes[i] {
            b'\'' | b'"' => {
                i = string_end(bytes, i).min(range.end);
                continue;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 && bytes.get(i + 1) == Some(&b'>') => return Some(i),
            _ => {}
        }
        i += 1;
    }

    None
}

pub struct Item {
    pub key: Option<Range<usize>>,
    pub value: Range<usize>,
}

/// The bracket opening the array literal `range` is, if it is one.
pub fn array_open(text: &str, range: Range<usize>) -> Option<usize> {
    let range = trimmed(text, range);
    let slice = &text[range.clone()];

    if slice.starts_with('[') {
        return Some(range.start);
    }

    let head = slice.get(..5)?;
    let rest = slice[5..].trim_start();
    (head.eq_ignore_ascii_case("array") && rest.starts_with('(')).then(|| range.end - rest.len())
}

/// Elements of the array literal opened by the bracket at `open`.
pub fn items(text: &str, open: usize) -> Vec<Item> {
    let Some(close) = closing(text, open) else { return Vec::new() };

    split(text, open + 1..close)
        .into_iter()
        .map(|part| match arrow(text, part.clone()) {
            Some(at) => Item { key: Some(part.start..at), value: at + 2..part.end },
            None => Item { key: None, value: part },
        })
        .collect()
}

/// Value of `key` in the array literal opened at `open`, when it is a string.
pub fn string_value<'a>(text: &'a str, open: usize, key: &str) -> Option<(&'a str, usize)> {
    items(text, open)
        .into_iter()
        .find(|item| item.key.clone().and_then(|k| literal(text, k)).is_some_and(|(name, _)| name == key))
        .and_then(|item| literal(text, item.value))
}

/// Strings of the array literal opened at `open` (keys when `keys`).
pub fn strings(text: &str, open: usize, keys: bool) -> Vec<String> {
    items(text, open)
        .into_iter()
        .filter_map(|item| {
            let range = if keys { item.key? } else { item.value };
            literal(text, range).map(|(s, _)| s.to_owned())
        })
        .collect()
}

/// The array a file returns: `return [` or `return array(`.
pub fn returned_array(text: &str) -> Option<usize> {
    text.match_indices("return").find_map(|(at, _)| {
        if at > 0 && is_identifier(text.as_bytes()[at - 1]) {
            return None;
        }

        let trimmed = text[at + 6..].trim_start();
        let start = text.len() - trimmed.len();
        if start == at + 6 && !trimmed.starts_with('[') {
            return None;
        }

        array_open(text, start..text.len())
    })
}

/// A key of a nested array: its dotted path, where the key name starts and
/// its value when that is a plain string.
pub struct Leaf {
    pub path: String,
    pub offset: usize,
    pub width: usize,
    pub value: Option<String>,
    /// The value is an array literal.
    pub array: bool,
}

/// Dotted paths of every string key of the array opened at `open`.
pub fn array_paths(text: &str, open: usize, prefix: &str, out: &mut Vec<Leaf>, depth: usize) {
    if depth > 8 {
        return;
    }

    for item in items(text, open) {
        let Some((key, offset)) = item.key.and_then(|k| literal(text, k)) else { continue };
        if key.is_empty() {
            continue;
        }

        let path = if prefix.is_empty() { key.to_owned() } else { format!("{prefix}.{key}") };
        let value = literal(text, item.value.clone()).map(|(v, _)| v.to_owned());
        let nested = array_open(text, item.value);
        out.push(Leaf { path: path.clone(), offset, width: key.len(), value, array: nested.is_some() });

        if let Some(nested) = nested {
            array_paths(text, nested, &path, out, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_comments_but_not_strings() {
        let text = "a('//x'); // c\n# d\n#[Attr]\n/* e\nf */ g";
        let masked = mask_comments(text);
        assert_eq!(masked.len(), text.len());
        assert_eq!(masked.lines().map(str::trim_end).collect::<Vec<_>>(), ["a('//x');", "", "#[Attr]", "", "     g"]);
    }

    #[test]
    fn splits_arguments_at_the_top_level() {
        let text = "f('a,b', [1, 2], g(3, 4), 'c')";
        let args: Vec<_> = arguments(text, 1).into_iter().map(|r| &text[r]).collect();
        assert_eq!(args, ["'a,b'", " [1, 2]", " g(3, 4)", " 'c'"]);
    }

    #[test]
    fn reads_array_literals() {
        let text = "['as' => 'admin.', 'uses' => 'A@b', 'only' => ['index', 'show'], 'x', array('k' => 1)]";
        assert_eq!(string_value(text, 0, "as").map(|(v, _)| v), Some("admin."));
        assert_eq!(string_value(text, 0, "uses").map(|(v, _)| v), Some("A@b"));
        let only = items(text, 0).into_iter().find(|i| i.key.clone().is_some_and(|k| text[k].contains("only"))).unwrap();
        assert_eq!(strings(text, array_open(text, only.value).unwrap(), false), ["index", "show"]);
    }

    #[test]
    fn collects_nested_paths_of_a_returned_array() {
        let text = "<?php\n\nreturn [\n    'name' => env('APP_NAME', 'Laravel'),\n    'db' => [\n        'host' => 'localhost',\n    ],\n];\n";
        let open = returned_array(text).unwrap();
        let mut leaves = Vec::new();
        array_paths(text, open, "app", &mut leaves, 0);
        let paths: Vec<_> = leaves.iter().map(|l| (l.path.as_str(), l.value.as_deref())).collect();
        assert_eq!(paths, [("app.name", None), ("app.db", None), ("app.db.host", Some("localhost"))]);
        assert_eq!(&text[leaves[2].offset..leaves[2].offset + leaves[2].width], "host");
    }

    #[test]
    fn survives_half_typed_code() {
        let text = "<?php return ['a' => ['b' => 'c', 'd' => f('e', [ ";
        for end in 0..=text.len() {
            let prefix = &text[..end];
            let _ = mask_comments(prefix);
            if let Some(open) = returned_array(prefix) {
                array_paths(prefix, open, "", &mut Vec::new(), 0);
            }
        }
    }
}
