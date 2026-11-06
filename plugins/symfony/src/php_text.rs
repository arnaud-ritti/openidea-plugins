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
pub(crate) fn mask(text: &str) -> String {
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

pub(crate) fn declares_enum(source: &str, short_name: &str) -> bool {
    let masked = mask(source);
    masked.match_indices("enum ").any(|(at, _)| {
        let starts_word = masked[..at].bytes().last().is_none_or(|byte| !is_word_byte(byte));
        let name = read_word(&masked, skip_whitespace(&masked, at + "enum ".len()));
        starts_word && &masked[name] == short_name
    })
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\' | b'$' | b'?' | b'|' | b'&') || byte >= 0x80
}

pub(crate) fn short_name(fqcn: &str) -> &str {
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

pub(crate) fn line_end(masked: &str, from: usize) -> usize {
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

/// Resolves a class name written in `text` to a fully qualified one.
pub fn resolver(text: &str) -> impl Fn(&str) -> String {
    let header = scan_header(&mask(text));
    move |name| resolve(name, &header)
}

pub fn insert_use(text: &str, fqcn: &str) -> Option<UseEdit> {
    let masked = mask(text);
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

pub(crate) fn skip_whitespace(masked: &str, mut at: usize) -> usize {
    let bytes = masked.as_bytes();
    while at < bytes.len() && bytes[at].is_ascii_whitespace() {
        at += 1;
    }
    at
}

pub(crate) fn read_word(masked: &str, at: usize) -> Range<usize> {
    let bytes = masked.as_bytes();
    let mut end = at;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    at..end
}

/// Index of the bracket closing the one at `open`, in masked text.
pub(crate) fn matching_close(masked: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, byte) in masked.bytes().enumerate().skip(open) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// Class name resolved through `use`, or a lower-case built-in type; `None` when untyped or a union/intersection.
    pub type_name: Option<String>,
    pub typed: bool,
    pub nullable: bool,
    pub has_default: bool,
    pub variadic: bool,
    pub promoted: bool,
    pub has_attribute: bool,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub name_range: Range<usize>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub params: Vec<Param>,
    pub parameters: Range<usize>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassModel {
    pub fqcn: String,
    pub name_range: Range<usize>,
    pub parent: Option<String>,
    pub parent_range: Option<Range<usize>>,
    pub interfaces: Vec<String>,
    pub body: Range<usize>,
    pub has_traits: bool,
    pub methods: Vec<Method>,
}

impl ClassModel {
    pub fn method(&self, name: &str) -> Option<&Method> {
        self.methods.iter().find(|method| method.name.eq_ignore_ascii_case(name))
    }
}

pub(crate) const BUILT_IN_TYPES: [&str; 17] = [
    "int", "string", "array", "bool", "float", "mixed", "callable", "iterable", "object", "self", "static", "null", "false", "true", "void", "never", "parent",
];

fn parameter_of(text: &str, masked: &str, header: &Header, range: Range<usize>) -> Option<Param> {
    let range = crate::php_checks::trimmed(masked, range);
    let mut at = range.start;
    let mut has_attribute = false;

    while masked[at..range.end].starts_with("#[") {
        has_attribute = true;
        at = skip_whitespace(masked, matching_close(masked, at + 1)? + 1);
    }

    let dollar = at + masked[at..range.end].find('$')?;
    let name = read_word(masked, dollar + 1);
    let mut head = masked[at..dollar].trim();
    let variadic = head.ends_with("...");
    head = head.trim_end_matches("...").trim_end().trim_end_matches('&').trim_end();

    let mut promoted = false;
    let mut words: Vec<&str> = head.split_whitespace().collect();
    while words.first().is_some_and(|word| matches!(word.to_ascii_lowercase().as_str(), "public" | "protected" | "private" | "readonly")) {
        promoted = true;
        words.remove(0);
    }

    let declared = words.join(" ");
    let nullable = declared.starts_with('?');
    let plain = declared.trim_start_matches('?');
    let simple = !plain.is_empty() && !plain.contains(['|', '&', '(', ' ']);
    let type_name = simple.then(|| match BUILT_IN_TYPES.contains(&plain.to_ascii_lowercase().as_str()) {
        true => plain.to_ascii_lowercase(),
        false => resolve(plain, header),
    });

    let after = &masked[name.end..range.end];
    Some(Param {
        name: text[dollar + 1..name.end].to_owned(),
        type_name,
        typed: !declared.is_empty(),
        nullable,
        has_default: after.contains('='),
        variadic,
        promoted,
        has_attribute,
        range,
    })
}

fn methods_of(text: &str, masked: &str, header: &Header, body: &Range<usize>) -> (Vec<Method>, bool) {
    let mut methods = Vec::new();
    let mut has_traits = false;
    let mut depth = 0usize;
    let mut boundary = body.start + 1;
    let mut at = body.start + 1;

    while at < body.end {
        let byte = masked.as_bytes()[at];
        match byte {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && byte == b'}' {
                    boundary = at + 1;
                }
            }
            b';' if depth == 0 => boundary = at + 1,
            _ => {}
        }

        let starts_word = at == 0 || !is_word_byte(masked.as_bytes()[at - 1]);
        if depth == 0 && starts_word && masked[at..body.end].starts_with("use ") {
            has_traits = true;
        }

        if depth == 0 && starts_word && masked[at..body.end].starts_with("function") && !is_word_byte(masked.as_bytes().get(at + 8).copied().unwrap_or(b' ')) {
            if let Some(method) = method_at(text, masked, header, boundary, at, body.end) {
                at = method.1;
                methods.push(method.0);
                boundary = at;
                continue;
            }
        }

        at += 1;
    }

    (methods, has_traits)
}

fn method_at(text: &str, masked: &str, header: &Header, boundary: usize, at: usize, limit: usize) -> Option<(Method, usize)> {
    let prefix = &masked[boundary..at];
    let prefix = prefix.rsplit("]").next().unwrap_or(prefix).to_ascii_lowercase();
    let visibility = match () {
        _ if prefix.contains("private") => Visibility::Private,
        _ if prefix.contains("protected") => Visibility::Protected,
        _ => Visibility::Public,
    };

    let mut name_at = skip_whitespace(masked, at + "function".len());
    if masked[name_at..].starts_with('&') {
        name_at = skip_whitespace(masked, name_at + 1);
    }

    let name_range = read_word(masked, name_at);
    let open = skip_whitespace(masked, name_range.end);
    if name_range.is_empty() || !masked[open..].starts_with('(') {
        return None;
    }

    let close = matching_close(masked, open)?;
    let params = crate::php_checks::arguments(masked, open).into_iter().filter_map(|argument| parameter_of(text, masked, header, argument)).collect();
    let after = skip_whitespace(masked, close + 1);
    let end = masked[after..limit].find(['{', ';']).map_or(limit, |found| after + found);
    let return_type = masked[after..end].trim().strip_prefix(':').map(|rest| rest.trim().to_owned());
    let next = match masked.as_bytes().get(end) {
        Some(b'{') => matching_close(masked, end).map_or(limit, |close| close + 1),
        _ => end + 1,
    };

    let method = Method {
        name: text[name_range.clone()].to_owned(),
        name_range,
        visibility,
        is_static: prefix.split_whitespace().any(|word| word == "static"),
        params,
        parameters: open..close + 1,
        return_type,
    };
    Some((method, next))
}

/// Offset after the keyword of the first `class`, `interface`, `trait` or `enum`
/// declaration that starts a line (modifiers allowed), and whether it is an interface.
fn type_declaration(masked: &str) -> Option<(usize, bool)> {
    ["class ", "interface ", "trait ", "enum "]
        .iter()
        .filter_map(|keyword| {
            masked.match_indices(keyword).map(|(at, _)| at).find(|&at| {
                let line_start = masked[..at].rfind('\n').map_or(0, |i| i + 1);
                masked[line_start..at].split_whitespace().all(|word| matches!(word, "final" | "abstract" | "readonly"))
            }).map(|at| (at + keyword.len(), *keyword == "interface "))
        })
        .min_by_key(|(at, _)| *at)
}

/// The first class, interface, trait or enum of the file with its parents, interfaces and methods,
/// every class name resolved through `use`. Nothing here sees traits' members.
pub fn class_model(text: &str) -> Option<ClassModel> {
    let masked = mask(text);
    let header = scan_header(&masked);
    let (at, is_interface) = type_declaration(&masked)?;
    let name_range = read_word(&masked, at);
    let open = at + masked[at..].find('{')?;
    let close = matching_close(&masked, open)?;

    let mut parent = None;
    let mut parent_range = None;
    let mut interfaces = Vec::new();
    let mut cursor = skip_whitespace(&masked, name_range.end);
    let mut collecting_interfaces = false;

    while cursor < open {
        let word = read_word(&masked, cursor);
        let comma = masked[cursor..open].starts_with(',');
        if comma {
            cursor = skip_whitespace(&masked, cursor + 1);
            continue;
        }

        if word.is_empty() {
            cursor += 1;
            continue;
        }

        match masked[word.clone()].to_ascii_lowercase().as_str() {
            "extends" if is_interface => {
                collecting_interfaces = true;
                cursor = skip_whitespace(&masked, word.end);
            }
            "extends" => {
                let target = read_word(&masked, skip_whitespace(&masked, word.end));
                parent = Some(resolve(&masked[target.clone()], &header));
                parent_range = Some(target.clone());
                cursor = skip_whitespace(&masked, target.end);
                collecting_interfaces = false;
            }
            "implements" => {
                collecting_interfaces = true;
                cursor = skip_whitespace(&masked, word.end);
            }
            _ if collecting_interfaces => {
                interfaces.push(resolve(&masked[word.clone()], &header));
                cursor = skip_whitespace(&masked, word.end);
            }
            _ => cursor = skip_whitespace(&masked, word.end),
        }
    }

    let body = open..close;
    let (methods, has_traits) = methods_of(text, &masked, &header, &body);
    let namespace = header.namespace.as_ref().map(|(namespace, _)| format!("{namespace}\\")).unwrap_or_default();

    Some(ClassModel { fqcn: format!("{namespace}{}", &masked[name_range.clone()]), name_range, parent, parent_range, interfaces, body, has_traits, methods })
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
    let mut parameter_depth = 0usize;
    let mut counted = 0;

    while let Some(start) = masked[at..].find("#[").map(|found| at + found) {
        let Some(close) = matching_close(&masked, start + 1) else { break };

        for byte in masked[counted..start].bytes() {
            match byte {
                b'(' => parameter_depth += 1,
                b')' => parameter_depth = parameter_depth.saturating_sub(1),
                _ => {}
            }
        }
        counted = start;

        let mut item_start = start + 2;
        let mut depth = 0usize;
        for position in start + 2..=close {
            let byte = masked.as_bytes()[position];
            match byte {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if position < close => depth = depth.saturating_sub(1),
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

        let target = match target_after(&masked, at) {
            Target::Property { .. } if parameter_depth > 0 => Target::Other,
            target => target,
        };
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
    fn insert_use_compares_aliases_case_insensitively() {
        assert!(insert_use("<?php\nuse Foo\\request;\n", "Symfony\\Component\\HttpFoundation\\Request").is_none());
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

    #[test]
    fn insert_use_goes_after_a_leading_declare() {
        let text = "<?php\ndeclare(strict_types=1);\n\nclass X {}\n";
        let edit = insert_use(text, "Foo\\Bar").unwrap();
        assert_eq!(&text[..edit.offset], "<?php\ndeclare(strict_types=1);\n");
        assert_eq!(edit.text, "\nuse Foo\\Bar;\n");
    }

    #[test]
    fn attributes_survive_unbalanced_brackets() {
        for text in ["<?php\n#[A(]]\nclass C {}\n", "<?php\n#[A)]\nclass C {}\n", "<?php\n#[A(\n"] {
            let _ = attributes(text);
        }
    }

    #[test]
    fn promoted_constructor_parameter_attribute_is_not_a_property() {
        let text = "<?php\nclass C\n{\n    public function __construct(#[Autowire] private $x, #[Other] $y) {}\n}\n";
        let found = attributes(text);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|attribute| attribute.target == Target::Other));
    }
}
