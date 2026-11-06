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

struct Import {
    fqcn: String,
    alias: String,
}

pub struct UseEdit {
    pub offset: usize,
    pub text: String,
    pub short_name: String,
}

fn short_name(fqcn: &str) -> &str {
    fqcn.rsplit('\\').next().unwrap_or(fqcn)
}

/// Same length as `text`: comments and string contents become spaces, line breaks stay.
fn mask_all(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = bytes.to_vec();
    let blank = |out: &mut Vec<u8>, range: Range<usize>| {
        for byte in &mut out[range] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
    };

    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        let next = bytes.get(i + 1).copied();
        match byte {
            b'/' if next == Some(b'*') => {
                let end = text[i + 2..].find("*/").map_or(bytes.len(), |at| i + 2 + at + 2);
                blank(&mut out, i..end);
                i = end;
            }
            b'/' if next == Some(b'/') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |at| i + at);
                blank(&mut out, i..end);
                i = end;
            }
            b'#' if next != Some(b'[') => {
                let end = text[i..].find('\n').map_or(bytes.len(), |at| i + at);
                blank(&mut out, i..end);
                i = end;
            }
            b'\'' | b'"' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != byte {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                blank(&mut out, i + 1..end);
                i = (end + 1).min(bytes.len());
            }
            _ => i += 1,
        }
    }

    String::from_utf8(out).unwrap_or_default()
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\' | b'$' | b'?' | b'|' | b'&') || byte >= 0x80
}

fn push_import(imports: &mut Vec<Import>, item: &str, prefix: &str) {
    let item = item.trim();
    if item.is_empty() {
        return;
    }

    let (name, alias) = match item.split_once(" as ") {
        Some((name, alias)) => (name.trim(), Some(alias.trim())),
        None => (item, None),
    };
    let fqcn = format!("{prefix}{}", name.trim_start_matches('\\'));
    let alias = alias.unwrap_or_else(|| short_name(&fqcn)).to_owned();
    imports.push(Import { fqcn, alias });
}

/// Imports with the byte offset where each statement's line ends, and the
/// offset after the `namespace` line.
struct Header {
    imports: Vec<(Import, usize)>,
    namespace: Option<(String, usize)>,
}

fn line_end(masked: &str, from: usize) -> usize {
    masked[from..].find('\n').map_or(masked.len(), |at| from + at + 1)
}

fn scan_header(masked: &str) -> Header {
    let mut header = Header { imports: Vec::new(), namespace: None };
    let mut depth = 0usize;
    let mut offset = 0;

    while offset < masked.len() {
        let next_line = line_end(masked, offset);
        let line = masked[offset..next_line].trim();

        if depth == 0 && line.starts_with("use ") {
            let statement_end = masked[offset..].find(';').map_or(masked.len(), |at| offset + at + 1);
            let end = line_end(masked, statement_end.saturating_sub(1));
            let statement = masked[offset + 4..statement_end.saturating_sub(1).max(offset + 4)].trim();
            if !statement.starts_with("function ") && !statement.starts_with("const ") {
                let mut imports = Vec::new();
                match statement.split_once('{') {
                    Some((prefix, rest)) => {
                        let prefix = prefix.trim().trim_start_matches('\\');
                        for item in rest.trim_end_matches('}').split(',') {
                            push_import(&mut imports, item, prefix);
                        }
                    }
                    None => {
                        for item in statement.split(',') {
                            push_import(&mut imports, item, "");
                        }
                    }
                }
                header.imports.extend(imports.into_iter().map(|import| (import, end)));
            }
            offset = end;
            continue;
        }

        if depth == 0 && header.namespace.is_none() && line.starts_with("namespace ") {
            let name: String = line["namespace ".len()..].chars().take_while(|c| !matches!(c, ';' | '{' | ' ')).collect();
            header.namespace = Some((name.trim_start_matches('\\').to_owned(), next_line));
        }

        depth = (depth + line.matches('{').count()).saturating_sub(line.matches('}').count());
        if line.starts_with("namespace ") && line.ends_with('{') {
            depth = 0;
        }
        offset = next_line;
    }

    header
}

pub fn insert_use(text: &str, fqcn: &str) -> Option<UseEdit> {
    let masked = mask_all(text);
    let header = scan_header(&masked);
    let fqcn = fqcn.trim_start_matches('\\');
    let short = short_name(fqcn);

    if let Some((import, _)) = header.imports.iter().find(|(import, _)| import.fqcn == fqcn) {
        return Some(UseEdit { offset: 0, text: String::new(), short_name: import.alias.clone() });
    }

    if header.imports.iter().any(|(import, _)| import.alias.eq_ignore_ascii_case(short)) {
        return None;
    }

    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line = format!("use {fqcn};{eol}");
    let (offset, text) = if let Some((_, end)) = header.imports.last() {
        (*end, line)
    } else if let Some((_, end)) = &header.namespace {
        (*end, format!("{eol}{line}"))
    } else {
        let start = line_end(&masked, masked.find("<?php")?);
        match declare_end(&masked, start) {
            Some(end) => (end, format!("{eol}{line}")),
            None => (start, line),
        }
    };

    Some(UseEdit { offset, text, short_name: short.to_owned() })
}

/// Offset after the line of a `declare(...);` statement that opens the file at `from`.
fn declare_end(masked: &str, from: usize) -> Option<usize> {
    let start = skip_whitespace(masked, from);
    let word = read_word(masked, start);
    if !masked[word.clone()].eq_ignore_ascii_case("declare") {
        return None;
    }

    let semicolon = masked[word.end..].find(';')? + word.end;
    Some(line_end(masked, semicolon))
}

fn skip_whitespace(masked: &str, mut at: usize) -> usize {
    let bytes = masked.as_bytes();
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

fn read_word(masked: &str, at: usize) -> Range<usize> {
    let bytes = masked.as_bytes();
    let mut end = at;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    at..end
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
    fn insert_use_goes_after_the_last_import() {
        let text = "<?php\nnamespace App;\n\nuse A\\B;\nuse A\\C;\n\nclass X {}\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(edit.text, "use Foo\\Bar;\n");
        assert_eq!(&text[..edit.offset], "<?php\nnamespace App;\n\nuse A\\B;\nuse A\\C;\n");
        assert_eq!(edit.short_name, "Bar");
    }

    #[test]
    fn insert_use_refuses_a_short_name_clash() {
        assert!(insert_use("<?php\nuse Other\\Bar;\n", "Foo\\Bar").is_none());
    }

    #[test]
    fn insert_use_handles_namespace_declare_crlf_and_existing_imports() {
        let text = "<?php\nnamespace App;\n\nclass X {}\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(edit.text, "\nuse Foo\\Bar;\n");
        assert_eq!(&text[..edit.offset], "<?php\nnamespace App;\n");

        let text = "<?php\ndeclare(strict_types=1);\n\nclass X {}\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(&text[..edit.offset], "<?php\ndeclare(strict_types=1);\n");

        let text = "<?php\r\nnamespace App;\r\n\r\nuse A\\B;\r\n\r\nclass X {}\r\n";
        assert_eq!(insert_use(text, "Foo\\Bar").unwrap().text, "use Foo\\Bar;\r\n");

        let edit = insert_use("<?php\nuse Foo\\Bar;\n", "\\Foo\\Bar").unwrap();
        assert_eq!(edit.text, "");
    }

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
