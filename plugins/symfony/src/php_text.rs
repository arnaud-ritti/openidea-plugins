//! Text helpers for PHP files: imports, `use` insertion and attributes. They
//! scan a copy of the text with comments and string contents blanked out;
//! nothing here parses PHP.

use std::ops::Range;

pub struct Import {
    pub fqcn: String,
    pub alias: String,
}

pub struct UseEdit {
    pub offset: usize,
    pub text: String,
    pub short_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,
    Protected,
    Private,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Class {
        name: String,
    },
    Method {
        name: String,
        visibility: Visibility,
        is_static: bool,
        name_range: Range<usize>,
        return_type: Option<String>,
    },
    Property {
        name: String,
    },
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub struct AttributeUse {
    pub name: String,
    pub range: Range<usize>,
    pub arguments: Range<usize>,
    pub target: Target,
}

/// Same length as `text`: comments and string contents become spaces, line breaks stay.
fn mask(text: &str) -> String {
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

fn short_name(fqcn: &str) -> &str {
    fqcn.rsplit('\\').next().unwrap_or(fqcn)
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

pub fn imports(text: &str) -> Vec<Import> {
    scan_header(&mask(text)).imports.into_iter().map(|(import, _)| import).collect()
}

pub fn insert_use(text: &str, fqcn: &str) -> Option<UseEdit> {
    let masked = mask(text);
    let header = scan_header(&masked);
    let fqcn = fqcn.trim_start_matches('\\');
    let short = short_name(fqcn);

    if let Some((import, _)) = header.imports.iter().find(|(import, _)| import.fqcn == fqcn) {
        return Some(UseEdit { offset: 0, text: String::new(), short_name: import.alias.clone() });
    }

    if header.imports.iter().any(|(import, _)| import.alias == short) {
        return None;
    }

    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line = format!("use {fqcn};{eol}");
    let (offset, text) = if let Some((_, end)) = header.imports.last() {
        (*end, line)
    } else if let Some((_, end)) = &header.namespace {
        (*end, format!("{eol}{line}"))
    } else {
        let start = masked.find("<?php")?;
        (line_end(&masked, start), line)
    };

    Some(UseEdit { offset, text, short_name: short.to_owned() })
}

fn resolve(name: &str, header: &Header) -> String {
    if let Some(absolute) = name.strip_prefix('\\') {
        return absolute.to_owned();
    }

    let (first, rest) = match name.split_once('\\') {
        Some((first, rest)) => (first, Some(rest)),
        None => (name, None),
    };

    if let Some((import, _)) = header.imports.iter().find(|(import, _)| import.alias == first) {
        return match rest {
            Some(rest) => format!("{}\\{rest}", import.fqcn),
            None => import.fqcn.clone(),
        };
    }

    match &header.namespace {
        Some((namespace, _)) if !namespace.is_empty() => format!("{namespace}\\{name}"),
        _ => name.to_owned(),
    }
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

/// Index of the bracket closing the one at `open`, in masked text.
fn matching_close(masked: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, byte) in masked.bytes().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

fn target_after(masked: &str, from: usize) -> Target {
    let mut visibility = Visibility::Public;
    let mut is_static = false;
    let mut at = skip_whitespace(masked, from);
    let mut types_seen = 0;

    loop {
        let word = read_word(masked, at);
        if word.is_empty() {
            return Target::Other;
        }

        let text = &masked[word.clone()];
        match text.to_ascii_lowercase().as_str() {
            "public" => visibility = Visibility::Public,
            "protected" => visibility = Visibility::Protected,
            "private" => visibility = Visibility::Private,
            "static" => is_static = true,
            "final" | "abstract" | "readonly" | "var" => {}
            "class" | "interface" | "trait" | "enum" => {
                let name = read_word(masked, skip_whitespace(masked, word.end));
                return match name.is_empty() {
                    true => Target::Other,
                    false => Target::Class { name: masked[name].to_owned() },
                };
            }
            "function" => return method_after(masked, word.end, visibility, is_static),
            "const" | "case" | "fn" | "use" | "namespace" => return Target::Other,
            _ => {
                if let Some(name) = text.strip_prefix('$') {
                    return Target::Property { name: name.to_owned() };
                }

                types_seen += 1;
                if types_seen > 1 {
                    return Target::Other;
                }
            }
        }

        at = skip_whitespace(masked, word.end);
    }
}

fn method_after(masked: &str, after_keyword: usize, visibility: Visibility, is_static: bool) -> Target {
    let mut at = skip_whitespace(masked, after_keyword);
    if masked[at..].starts_with('&') {
        at = skip_whitespace(masked, at + 1);
    }

    let name_range = read_word(masked, at);
    if name_range.is_empty() {
        return Target::Other;
    }

    let open = skip_whitespace(masked, name_range.end);
    if !masked[open..].starts_with('(') {
        return Target::Other;
    }

    let return_type = matching_close(masked, open).and_then(|close| {
        let after = skip_whitespace(masked, close + 1);
        let rest = masked[after..].strip_prefix(':')?;
        let end = rest.find(['{', ';']).unwrap_or(rest.len());
        let declared = rest[..end].trim();
        (!declared.is_empty()).then(|| declared.to_owned())
    });

    Target::Method {
        name: masked[name_range.clone()].to_owned(),
        visibility,
        is_static,
        name_range,
        return_type,
    }
}

pub fn attributes(text: &str) -> Vec<AttributeUse> {
    let masked = mask(text);
    let header = scan_header(&masked);
    let mut found = Vec::new();
    let mut pending: Vec<(String, Range<usize>, Range<usize>)> = Vec::new();
    let mut at = 0;

    while let Some(start) = masked[at..].find("#[").map(|found| at + found) {
        let Some(close) = matching_close(&masked, start + 1) else { break };

        let mut item_start = start + 2;
        let mut depth = 0usize;
        for position in start + 2..=close {
            let byte = masked.as_bytes()[position];
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if position < close => depth -= 1,
                _ => {}
            }

            if (byte == b',' && depth == 0) || position == close {
                if let Some(item) = attribute_in(&masked, item_start..position) {
                    pending.push(item);
                }
                item_start = position + 1;
            }
        }

        at = close + 1;
        let next = skip_whitespace(&masked, at);
        if masked[next..].starts_with("#[") {
            continue;
        }

        let target = target_after(&masked, at);
        for (name, range, arguments) in pending.drain(..) {
            let target = match &target {
                Target::Class { name } => Target::Class { name: name.clone() },
                Target::Method { name, visibility, is_static, name_range, return_type } => Target::Method {
                    name: name.clone(),
                    visibility: *visibility,
                    is_static: *is_static,
                    name_range: name_range.clone(),
                    return_type: return_type.clone(),
                },
                Target::Property { name } => Target::Property { name: name.clone() },
                Target::Other => Target::Other,
            };
            found.push(AttributeUse { name: resolve(&name, &header), range, arguments, target });
        }
    }

    found
}

fn attribute_in(masked: &str, item: Range<usize>) -> Option<(String, Range<usize>, Range<usize>)> {
    let start = skip_whitespace(masked, item.start);
    if start >= item.end {
        return None;
    }

    let name = read_word(masked, start);
    if name.is_empty() || name.end > item.end {
        return None;
    }

    let after_name = skip_whitespace(masked, name.end);
    let (end, arguments) = match masked[after_name..item.end].starts_with('(') {
        true => {
            let close = matching_close(masked, after_name)?;
            (close + 1, after_name + 1..close)
        }
        false => (name.end, name.end..name.end),
    };

    Some((masked[name.clone()].to_owned(), start..end, arguments))
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
    fn insert_use_goes_after_the_namespace_line_without_imports() {
        let text = "<?php\nnamespace App;\n\nclass X {}\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(edit.text, "\nuse Foo\\Bar;\n");
        assert_eq!(&text[..edit.offset], "<?php\nnamespace App;\n");

        let bare = "<?php\n\nclass X {}\n";
        let edit = insert_use(bare, "Foo\\Bar").unwrap();
        assert_eq!(&bare[..edit.offset], "<?php\n");
        assert_eq!(edit.text, "use Foo\\Bar;\n");
    }

    #[test]
    fn insert_use_is_empty_when_already_imported() {
        let text = "<?php\nuse Foo\\Bar as Baz;\n";
        let edit = insert_use(text, "\\Foo\\Bar").unwrap();
        assert_eq!(edit.text, "");
        assert_eq!(edit.short_name, "Baz");
    }

    #[test]
    fn insert_use_refuses_a_short_name_clash() {
        assert!(insert_use("<?php\nuse Other\\Bar;\n", "Foo\\Bar").is_none());
    }

    #[test]
    fn insert_use_keeps_crlf() {
        let text = "<?php\r\nnamespace App;\r\n\r\nuse A\\B;\r\n\r\nclass X {}\r\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(edit.text, "use Foo\\Bar;\r\n");
        assert_eq!(&text[..edit.offset], "<?php\r\nnamespace App;\r\n\r\nuse A\\B;\r\n");
    }

    #[test]
    fn imports_expand_groups_and_aliases() {
        let text = "<?php\nuse A\\B;\nuse A\\C as D;\nuse E\\{F, G as H};\nuse function X\\y;\n";
        let pairs: Vec<(String, String)> = imports(text).into_iter().map(|i| (i.fqcn, i.alias)).collect();
        let expected = [("A\\B", "B"), ("A\\C", "D"), ("E\\F", "F"), ("E\\G", "H")];
        assert_eq!(pairs, expected.map(|(a, b)| (a.to_owned(), b.to_owned())));
    }

    #[test]
    fn attributes_resolve_aliases_and_fqns() {
        let text = "<?php\nnamespace App;\nuse Doctrine\\ORM\\Mapping as ORM;\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n#[ORM\\Table(name: 'x'), \\Foo\\Bar, Route('/a')]\nclass C {}\n";
        let found = attributes(text);
        let names: Vec<&str> = found.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Doctrine\\ORM\\Mapping\\Table", "Foo\\Bar", "Symfony\\Component\\Routing\\Attribute\\Route"]);
        assert_eq!(&text[found[0].arguments.clone()], "name: 'x'");
        assert!(found[1].arguments.is_empty());
        assert_eq!(&text[found[2].range.clone()], "Route('/a')");
    }

    #[test]
    fn attributes_know_their_target() {
        let text = "<?php\n#[A]\nfinal class C\n{\n    #[B]\n    private ?string $p = null;\n\n    #[D]\n    protected static function run(int $a): ?int\n    {\n    }\n}\n";
        let found = attributes(text);
        assert_eq!(found[0].target, Target::Class { name: "C".into() });
        assert_eq!(found[1].target, Target::Property { name: "$p".trim_start_matches('$').into() });
        let Target::Method { name, visibility, is_static, name_range, return_type } = &found[2].target else { panic!() };
        assert_eq!(name, "run");
        assert_eq!(*visibility, Visibility::Protected);
        assert!(*is_static);
        assert_eq!(&text[name_range.clone()], "run");
        assert_eq!(return_type.as_deref(), Some("?int"));
    }

    #[test]
    fn attributes_ignore_comments_and_strings() {
        let text = "<?php\n// #[A]\n/* #[B] */\n$x = '#[C]';\n# note\n#[D]\nclass C {}\n";
        let found = attributes(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "D");
    }
}
