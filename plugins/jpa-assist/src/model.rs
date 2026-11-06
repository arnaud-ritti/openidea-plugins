//! A JPA reading of Java and Kotlin source text: comments and strings masked
//! out, annotations, and the persistent attributes of entity, mapped
//! superclass and embeddable classes.

use openidea_plugin::text::position;
use openidea_plugin::{Position, Range, TextEdit};

/// A byte-range replacement in the original text.
#[derive(Debug, PartialEq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Edit {
    pub fn insert(at: usize, text: &str) -> Self {
        Self { start: at, end: at, text: text.into() }
    }

    pub fn to_text_edit(&self, text: &str) -> TextEdit {
        TextEdit { range: Range::new(position(text, self.start), position(text, self.end)), text: self.text.clone() }
    }
}

pub struct Annotation<'a> {
    /// Last segment: `Id` for `@jakarta.persistence.Id` and `@field:Id`.
    pub name: &'a str,
    pub start: usize,
    pub name_end: usize,
    /// Byte range between the parentheses.
    pub args: Option<(usize, usize)>,
}

impl Annotation<'_> {
    pub fn end(&self) -> usize {
        self.args.map_or(self.name_end, |(_, close)| close + 1)
    }
}

/// `Some(is_kotlin)` for the files this plugin reads.
pub fn language_of(path: &str) -> Option<bool> {
    if path.ends_with(".java") {
        return Some(false);
    }

    path.ends_with(".kt").then_some(true)
}

fn find(b: &[u8], from: usize, pattern: &[u8]) -> Option<usize> {
    b.get(from..)?.windows(pattern.len()).position(|w| w == pattern).map(|p| p + from)
}

/// `text` with comments and string literals blanked: same byte offsets, newlines kept.
pub fn mask(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;

    while i < b.len() {
        let rest = &b[i..];
        let end = if rest.starts_with(b"//") {
            find(b, i, b"\n").unwrap_or(b.len())
        } else if rest.starts_with(b"/*") {
            find(b, i + 2, b"*/").map_or(b.len(), |p| p + 2)
        } else if rest.starts_with(b"\"\"\"") {
            find(b, i + 3, b"\"\"\"").map_or(b.len(), |p| p + 3)
        } else if b[i] == b'"' || b[i] == b'\'' {
            let mut j = i + 1;
            while j < b.len() && b[j] != b[i] && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            (j + 1).min(b.len())
        } else {
            i += 1;
            continue;
        };

        for c in &mut out[i..end] {
            if *c != b'\n' {
                *c = b' ';
            }
        }

        i = end;
    }

    String::from_utf8(out).expect("whole characters are replaced by ASCII spaces")
}

pub fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// Offsets of `word` in `s` as a whole identifier.
pub fn words<'a>(s: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let b = s.as_bytes();
    s.match_indices(word).map(|(i, _)| i).filter(move |&i| {
        let before = i == 0 || !is_ident(b[i - 1]);
        let after = b.get(i + word.len()).is_none_or(|&c| !is_ident(c));
        before && after
    })
}

/// The first of `keywords` at or after `from` that opens a declaration: a `class` or `enum` after `.` or `::` is a class literal.
pub fn declaration_keyword<'a>(masked: &str, from: usize, keywords: &[&'a str]) -> Option<(usize, &'a str)> {
    keywords
        .iter()
        .flat_map(|&keyword| words(&masked[from..], keyword).map(move |i| (i + from, keyword)))
        .filter(|&(i, _)| {
            let before = masked[..i].trim_end();
            !(before.ends_with("::") || before.ends_with('.'))
        })
        .min_by_key(|&(i, _)| i)
}

pub fn has_word(s: &str, word: &str) -> bool {
    words(s, word).next().is_some()
}

/// The bracket closing the one at `open` (`(`/`)` or `{`/`}`).
pub fn closing(b: &[u8], open: usize) -> Option<usize> {
    let (opening, closing) = if b.get(open) == Some(&b'{') { (b'{', b'}') } else { (b'(', b')') };
    let mut depth = 0;

    for (i, &c) in b.iter().enumerate().skip(open) {
        if c == opening {
            depth += 1;
        }

        if c == closing {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }

    None
}

pub fn annotations(masked: &str) -> Vec<Annotation<'_>> {
    let b = masked.as_bytes();
    let mut found = Vec::new();

    for (start, _) in masked.match_indices('@') {
        if start > 0 && is_ident(b[start - 1]) {
            continue;
        }

        let mut i = start + 1;
        let mut name_start = i;
        while i < b.len() && (is_ident(b[i]) || b[i] == b'.' || b[i] == b':') {
            if !is_ident(b[i]) {
                name_start = i + 1;
            }
            i += 1;
        }

        if name_start == i {
            continue;
        }

        let open = i + b[i..].iter().take_while(|c| c.is_ascii_whitespace()).count();
        let args = (b.get(open) == Some(&b'('))
            .then(|| closing(b, open))
            .flatten()
            .map(|close| (open + 1, close));

        found.push(Annotation { name: &masked[name_start..i], start, name_end: i, args });
    }

    found
}

/// Splits `s` at the commas outside brackets.
pub fn split_top_level(s: &str) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (i, c) in s.bytes().enumerate() {
        match c {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b')' | b'>' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push((start, &s[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }

    parts.push((start, &s[start..]));
    parts
}

/// A `:` outside parentheses: a Kotlin class header with a supertype.
pub fn kotlin_supertype(header: &str) -> Option<&str> {
    let mut depth = 0i32;

    for (i, c) in header.bytes().enumerate() {
        match c {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b':' if depth == 0 => return Some(header[i + 1..].trim_start()),
            _ => {}
        }
    }

    None
}

fn is_imported(text: &str, fqcn: &str) -> bool {
    let Some(dot) = fqcn.rfind('.') else { return true };
    let wildcard = format!("{}.*", &fqcn[..dot]);

    text.lines().filter_map(|l| l.trim().strip_prefix("import ")).any(|imported| {
        let imported = imported.trim_end_matches(';').trim();
        imported == fqcn || imported == wildcard
    })
}

/// One edit importing the `fqcns` not imported yet (or covered by a `*`), after the last import or the package line.
pub fn import_edit(text: &str, fqcns: &[&str], kotlin: bool) -> Option<Edit> {
    let semicolon = if kotlin { "" } else { ";" };
    let mut wanted: Vec<&str> = fqcns.iter().copied().filter(|fqcn| !is_imported(text, fqcn)).collect();
    wanted.sort_unstable();
    wanted.dedup();

    if wanted.is_empty() {
        return None;
    }

    let lines: String = wanted.iter().map(|fqcn| format!("import {fqcn}{semicolon}\n")).collect();
    let mut after_imports = None;
    let mut after_package = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        offset += line.len();
        let trimmed = line.trim();

        if trimmed.starts_with("import ") {
            after_imports = Some(offset);
        }

        if trimmed.starts_with("package ") {
            after_package = Some(offset);
        }
    }

    if let Some(at) = after_imports {
        return Some(Edit::insert(at, &lines));
    }

    if let Some(at) = after_package {
        return Some(Edit::insert(at, &format!("\n{lines}")));
    }

    Some(Edit::insert(0, &format!("{lines}\n")))
}

pub fn snake_case(name: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        let after_lower = i > 0 && (chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit());
        if c.is_uppercase() && after_lower {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }

    out
}

pub fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

pub fn decapitalize(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |c| c.to_lowercase().chain(chars).collect())
}

/// `orders_line` → `OrdersLine`, `customer_id` → `customerId` with `upper = false`.
pub fn camel_case(snake: &str, upper: bool) -> String {
    let joined: String = snake.split(['_', '-', ' ']).filter(|p| !p.is_empty()).map(|p| capitalize(&p.to_lowercase())).collect();
    if upper { joined } else { decapitalize(&joined) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Entity,
    MappedSuperclass,
    Embeddable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    None,
    ManyToOne,
    OneToOne,
    OneToMany,
    ManyToMany,
    Embedded,
    ElementCollection,
}

impl Relation {
    pub fn to_one(self) -> bool {
        matches!(self, Self::ManyToOne | Self::OneToOne)
    }

    pub fn to_many(self) -> bool {
        matches!(self, Self::OneToMany | Self::ManyToMany | Self::ElementCollection)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    pub name: String,
    /// As declared, without Kotlin's `?`: `Long`, `List<Order>`, `byte[]`.
    pub ty: String,
    /// The type a property path continues with: the element of a collection, else the type.
    pub target: String,
    pub pos: Position,
    pub relation: Relation,
    pub id: bool,
    /// `IDENTITY`, `SEQUENCE`, `AUTO`, `TABLE` or `UUID`.
    pub generated: Option<String>,
    pub mapped_by: bool,
    /// `@Column(name)` or `@JoinColumn(name)`.
    pub column: Option<String>,
    pub join_table: Option<String>,
    pub join_column: bool,
    pub not_null: bool,
    pub unique: bool,
    pub length: Option<u32>,
    pub precision: Option<u32>,
    pub scale: Option<u32>,
    pub lob: bool,
    /// `@Enumerated`: `Some(true)` for `STRING`.
    pub enum_string: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: String,
    pub kind: Kind,
    pub kotlin: bool,
    pub path: String,
    pub pos: Position,
    pub package: Option<String>,
    pub imports: Vec<String>,
    pub table: Option<String>,
    pub superclass: Option<String>,
    /// `@Id`, `@EmbeddedId` or `@IdClass` appears in the class's file.
    pub has_id: bool,
    pub attributes: Vec<Attribute>,
    /// Offset of the `}` closing the class body.
    pub body_end: Option<usize>,
}

impl Class {
    /// Where `ty` (a simple name used in this class's file) is declared.
    pub fn qualified(&self, ty: &str) -> Option<String> {
        if let Some(known) = well_known(ty) {
            return known.map(str::to_owned);
        }

        let suffix = format!(".{ty}");
        if let Some(import) = self.imports.iter().find(|i| i.ends_with(&suffix)) {
            return Some(import.clone());
        }

        self.package.as_ref().map(|p| format!("{p}.{ty}"))
    }
}

/// `Some(None)`: needs no import; `Some(Some(fqcn))`: from the JDK.
fn well_known(ty: &str) -> Option<Option<&'static str>> {
    let builtin = [
        "String", "Long", "Integer", "Short", "Byte", "Boolean", "Double", "Float", "Character", "Object", "long", "int",
        "short", "byte", "boolean", "double", "float", "char", "Int", "Char", "Any", "ByteArray",
    ];
    if builtin.contains(&ty) || ty.ends_with("[]") {
        return Some(None);
    }

    let jdk = [
        "java.math.BigDecimal",
        "java.math.BigInteger",
        "java.time.LocalDate",
        "java.time.LocalDateTime",
        "java.time.LocalTime",
        "java.time.Instant",
        "java.time.OffsetDateTime",
        "java.time.ZonedDateTime",
        "java.time.Duration",
        "java.util.UUID",
        "java.util.Date",
    ];

    jdk.iter().find(|f| f.rsplit('.').next() == Some(ty)).map(|f| Some(*f))
}

fn string_value(value: &str) -> Option<String> {
    value.trim().strip_prefix('"').and_then(|v| v.strip_suffix('"')).map(str::to_owned)
}

/// The original text of argument `key` of `a` (`name = "x"` → `"x"`).
pub fn arg<'t>(text: &'t str, masked: &str, a: &Annotation, key: &str) -> Option<&'t str> {
    let (start, end) = a.args?;
    let inner = &masked[start..end];

    split_top_level(inner).into_iter().find_map(|(offset, part)| {
        let (k, _) = part.split_once('=')?;
        if k.trim() != key {
            return None;
        }

        let value_start = start + offset + part.find('=')? + 1;
        Some(text[value_start..start + offset + part.len()].trim())
    })
}

/// The byte range of argument `key` of `a` (the lone argument counts as `value`), trimmed.
pub fn value_range(text: &str, masked: &str, a: &Annotation, key: &str) -> Option<(usize, usize)> {
    let (start, end) = a.args?;

    split_top_level(&masked[start..end]).into_iter().find_map(|(offset, part)| {
        let at = start + offset;
        let from = match part.split_once('=') {
            Some((k, _)) if k.trim() == key => at + part.find('=')? + 1,
            Some(_) => return None,
            None if key == "value" && !text[at..at + part.len()].trim().is_empty() => at,
            None => return None,
        };

        let raw = &text[from..at + part.len()];
        let lead = raw.len() - raw.trim_start().len();
        Some((from + lead, from + lead + raw.trim().len()))
    })
}

/// The content of the plain string literal at `range`, and where it starts; `None` for anything else (concatenation, text block, template).
pub fn plain_literal(text: &str, (start, end): (usize, usize), kotlin: bool) -> Option<(usize, &str)> {
    let literal = text.get(start..end)?;
    let inner = literal.strip_prefix('"')?.strip_suffix('"')?;
    if literal.len() < 2 || literal.starts_with("\"\"\"") || (kotlin && inner.contains('$')) {
        return None;
    }

    let mut bytes = inner.bytes();
    while let Some(c) = bytes.next() {
        match c {
            b'\\' => {
                bytes.next();
            }
            b'"' => return None,
            _ => {}
        }
    }

    Some((start + 1, inner))
}

fn string_arg(text: &str, masked: &str, a: &Annotation, key: &str) -> Option<String> {
    string_value(arg(text, masked, a, key)?)
}

fn int_arg(text: &str, masked: &str, a: &Annotation, key: &str) -> Option<u32> {
    arg(text, masked, a, key)?.parse().ok()
}

fn bool_arg(text: &str, masked: &str, a: &Annotation, key: &str) -> Option<bool> {
    arg(text, masked, a, key)?.parse().ok()
}

fn last_segment(s: &str) -> &str {
    s.rsplit('.').next().unwrap_or(s).trim()
}

/// `List<Order>` → `Order`, `Map<String, Item>` → `Item`, `Customer?` → `Customer`.
pub fn element_type(ty: &str) -> String {
    let base = match (ty.find('<'), ty.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            split_top_level(&ty[open + 1..close]).last().map_or("", |(_, last)| last.trim())
        }
        _ => ty,
    };

    last_segment(base.trim_end_matches('?').trim_start_matches("out ")).to_owned()
}

/// Splits a class body (or a constructor's parameter list with `params`) into member declarations.
pub fn members(masked: &str, start: usize, end: usize, kotlin: bool, params: bool) -> Vec<(usize, usize)> {
    let b = masked.as_bytes();
    let mut found = Vec::new();
    let (mut braces, mut parens) = (0i32, 0i32);
    let mut segment = start;

    for i in start..end {
        match b[i] {
            b'{' => braces += 1,
            b'}' => {
                braces -= 1;
                if braces == 0 && parens == 0 {
                    found.push((segment, i + 1));
                    segment = i + 1;
                }
            }
            b'(' | b'[' => parens += 1,
            b')' | b']' => parens -= 1,
            b';' if braces == 0 && parens == 0 => {
                found.push((segment, i + 1));
                segment = i + 1;
            }
            b',' if params && braces == 0 && parens == 0 => {
                found.push((segment, i));
                segment = i + 1;
            }
            b'\n' if kotlin && !params && braces == 0 && parens == 0 => {
                let line = without_annotations(masked, segment, i);
                let open_ended = line.trim_end().ends_with(['=', '.', ',', ':', '(', '<']);
                if !line.trim().is_empty() && !open_ended {
                    found.push((segment, i));
                    segment = i;
                }
            }
            _ => {}
        }
    }

    found.push((segment, end));
    found
}

/// The masked slice with its annotations blanked.
pub fn without_annotations(masked: &str, start: usize, end: usize) -> String {
    let mut out = masked[start..end].to_owned();

    for a in annotations(&masked[start..end]) {
        out.replace_range(a.start..a.end(), &" ".repeat(a.end() - a.start));
    }

    out
}

/// The declared type of a field or Kotlin property.
pub fn declared_type(decl: &str, kotlin: bool) -> Option<String> {
    declaration(decl, kotlin).map(|(_, _, ty)| ty)
}

/// `(name offset, name, declared type)` of a field or property declaration.
pub fn declaration(decl: &str, kotlin: bool) -> Option<(usize, String, String)> {
    let left = decl.split('=').next().unwrap_or(decl).trim_end().trim_end_matches(';');

    if kotlin {
        let keyword = words(left, "val").chain(words(left, "var")).min()?;
        if ["fun", "class", "object", "interface", "by"].iter().any(|w| has_word(decl, w)) {
            return None;
        }

        let rest = &left[keyword + 3..];
        let name_start = keyword + 3 + rest.len() - rest.trim_start().len();
        let (name, ty) = left[name_start..].split_once(':').unwrap_or((&left[name_start..], ""));
        let name = name.trim();
        return name.bytes().all(is_ident).then(|| (name_start, name.to_owned(), ty.trim().trim_end_matches('?').to_owned()));
    }

    if left.contains('(') || left.contains('{') {
        return None;
    }

    if ["static", "transient", "class", "interface", "enum", "record"].iter().any(|w| has_word(left, w)) {
        return None;
    }

    let trimmed = left.trim_end();
    let name_len = trimmed.bytes().rev().take_while(|&c| is_ident(c)).count();
    let name_start = trimmed.len() - name_len;
    let mut ty = trimmed[..name_start].trim();

    for modifier in ["private", "protected", "public", "final", "volatile"] {
        ty = ty.strip_prefix(modifier).map_or(ty, str::trim_start);
    }

    (name_len > 0 && !ty.is_empty()).then(|| (name_start, trimmed[name_start..].to_owned(), ty.to_owned()))
}

fn attribute(text: &str, masked: &str, all: &[Annotation], (start, end): (usize, usize), kotlin: bool) -> Option<Attribute> {
    let decl = without_annotations(masked, start, end);
    let (offset, name, ty) = declaration(&decl, kotlin)?;
    let anns: Vec<&Annotation> = all.iter().filter(|a| a.start >= start && a.start < end).collect();
    let named = |n: &str| anns.iter().copied().find(|a| a.name == n);

    if named("Transient").is_some() {
        return None;
    }

    let relation = [
        ("ManyToOne", Relation::ManyToOne),
        ("OneToOne", Relation::OneToOne),
        ("OneToMany", Relation::OneToMany),
        ("ManyToMany", Relation::ManyToMany),
        ("Embedded", Relation::Embedded),
        ("EmbeddedId", Relation::Embedded),
        ("ElementCollection", Relation::ElementCollection),
    ]
    .into_iter()
    .find_map(|(n, r)| named(n).map(|_| r))
    .unwrap_or(Relation::None);

    let relation_ann = anns.iter().copied().find(|a| matches!(a.name, "ManyToOne" | "OneToOne" | "OneToMany" | "ManyToMany"));
    let column = named("Column");
    let join_column = named("JoinColumn");
    let column_arg = |key: &str| column.or(join_column).and_then(|c| arg(text, masked, c, key));
    let primitive = ["long", "int", "short", "byte", "boolean", "double", "float", "char"].contains(&ty.as_str());

    let generated = named("GeneratedValue")
        .map(|g| arg(text, masked, g, "strategy").map_or("AUTO".to_owned(), |s| last_segment(s).to_owned()))
        .or_else(|| named("UuidGenerator").map(|_| "UUID".to_owned()));

    let enum_string = named("Enumerated").map(|a| a.args.is_some_and(|(open, close)| has_word(&masked[open..close], "STRING")));

    Some(Attribute {
        pos: position(text, start + offset),
        target: element_type(&ty),
        relation,
        id: named("Id").is_some() || named("EmbeddedId").is_some(),
        generated,
        mapped_by: relation_ann.is_some_and(|a| arg(text, masked, a, "mappedBy").is_some()),
        column: column.or(join_column).and_then(|c| string_arg(text, masked, c, "name")),
        join_table: named("JoinTable").and_then(|j| string_arg(text, masked, j, "name")),
        join_column: join_column.is_some(),
        not_null: primitive
            || column_arg("nullable") == Some("false")
            || relation_ann.is_some_and(|a| bool_arg(text, masked, a, "optional") == Some(false))
            || named("NotNull").is_some()
            || named("NonNull").is_some(),
        unique: column.is_some_and(|c| bool_arg(text, masked, c, "unique") == Some(true)),
        length: column.and_then(|c| int_arg(text, masked, c, "length")),
        precision: column.and_then(|c| int_arg(text, masked, c, "precision")),
        scale: column.and_then(|c| int_arg(text, masked, c, "scale")),
        lob: named("Lob").is_some(),
        enum_string,
        name,
        ty,
    })
}

/// The entity, mapped superclass or embeddable declared in `text`.
pub fn parse_class(text: &str, path: &str) -> Option<Class> {
    let kotlin = language_of(path)?;
    let masked = mask(text);
    let anns = annotations(&masked);
    let (marker, kind) = anns.iter().find_map(|a| match a.name {
        "Entity" => Some((a, Kind::Entity)),
        "MappedSuperclass" => Some((a, Kind::MappedSuperclass)),
        "Embeddable" => Some((a, Kind::Embeddable)),
        _ => None,
    })?;

    let (class_at, keyword) = declaration_keyword(&masked, marker.end(), &["class", "interface", "enum", "record"])?;
    if keyword != "class" {
        return None;
    }
    let after = &masked[class_at + "class".len()..];
    let name_start = class_at + "class".len() + after.len() - after.trim_start().len();
    let name_len = masked[name_start..].bytes().take_while(|&c| is_ident(c)).count();
    let name = &masked[name_start..name_start + name_len];
    let header_end = masked[name_start..].find('{').map_or(masked.len(), |p| p + name_start);
    let b = masked.as_bytes();
    let body = (header_end < masked.len()).then(|| (header_end, closing(b, header_end)));

    let mut attributes = Vec::new();
    if kotlin {
        if let Some(open) = masked[name_start..header_end].find('(').map(|p| p + name_start) {
            let close = closing(b, open).unwrap_or(header_end);
            for member in members(&masked, open + 1, close, true, true) {
                attributes.extend(attribute(text, &masked, &anns, member, true));
            }
        }
    }

    if let Some((open, Some(close))) = body {
        for member in members(&masked, open + 1, close, kotlin, false) {
            attributes.extend(attribute(text, &masked, &anns, member, kotlin));
        }
    }

    let header = &masked[name_start + name_len..header_end];
    let superclass = if kotlin {
        kotlin_supertype(header).map(|s| s.split(['(', '<', ',', ' ', '{']).next().unwrap_or(s))
    } else {
        words(header, "extends").next().map(|i| header[i + "extends".len()..].trim_start()).map(|s| s.split(['<', ' ', '{']).next().unwrap_or(s))
    };

    let statements = |keyword: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(keyword))
            .map(|r| r.trim().trim_end_matches(';').trim().to_owned())
            .collect()
    };

    Some(Class {
        name: name.to_owned(),
        kind,
        kotlin,
        path: path.to_owned(),
        pos: position(text, name_start),
        package: statements("package ").into_iter().next(),
        imports: statements("import "),
        table: anns.iter().find(|a| a.name == "Table").and_then(|t| string_arg(text, &masked, t, "name")),
        superclass: superclass.filter(|s| !s.is_empty()).map(|s| last_segment(s).to_owned()),
        has_id: anns.iter().any(|a| matches!(a.name, "Id" | "EmbeddedId" | "IdClass")),
        attributes,
        body_end: body.and_then(|(_, close)| close),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA: &str = r#"package shop;

import jakarta.persistence.*;
import shop.money.Currency;
import java.math.BigDecimal;

@Entity
@Table(name = "purchase_orders")
public class Order extends BaseEntity {
    @Id
    @GeneratedValue(strategy = GenerationType.IDENTITY)
    private Long id;

    @Column(name = "order_number", nullable = false, length = 20, unique = true)
    private String number;

    private int quantity;
    private BigDecimal total = BigDecimal.ZERO;

    @Enumerated(EnumType.STRING)
    private Currency currency;

    @ManyToOne(fetch = FetchType.LAZY, optional = false)
    @JoinColumn(name = "buyer_id")
    private Customer customer;

    @OneToMany(mappedBy = "order")
    private List<OrderLine> lines = new ArrayList<>();

    @Transient
    private String cached;
    private static final long serialVersionUID = 1L;

    public Long getId() { return id; }

    @PrePersist
    void touch() {}
}
"#;

    const KOTLIN: &str = r#"package shop

import jakarta.persistence.*

@Entity
class Customer(
    @Column(nullable = false)
    var name: String,
    plain: Int,
) : BaseEntity() {
    @Id
    @GeneratedValue
    var id: Long? = null

    @OneToMany(mappedBy = "customer")
    var orders: MutableList<Order> = mutableListOf()

    var email: String? = null
        get() = field?.lowercase()

    fun rename(to: String) {
        name = to
    }
}
"#;

    fn names(c: &Class) -> Vec<&str> {
        c.attributes.iter().map(|a| a.name.as_str()).collect()
    }

    #[test]
    fn reads_a_java_entity() {
        let c = parse_class(JAVA, "/p/Order.java").unwrap();
        assert_eq!((c.name.as_str(), c.kind, c.table.as_deref()), ("Order", Kind::Entity, Some("purchase_orders")));
        assert_eq!(c.superclass.as_deref(), Some("BaseEntity"));
        assert_eq!(names(&c), ["id", "number", "quantity", "total", "currency", "customer", "lines"]);

        let a = |n: &str| c.attributes.iter().find(|a| a.name == n).unwrap();
        assert!(a("id").id && a("id").generated.as_deref() == Some("IDENTITY"));
        assert_eq!((a("number").column.as_deref(), a("number").length, a("number").unique), (Some("order_number"), Some(20), true));
        assert!(a("number").not_null && a("quantity").not_null && !a("total").not_null);
        assert_eq!(a("currency").enum_string, Some(true));
        assert_eq!((a("customer").relation, a("customer").column.as_deref()), (Relation::ManyToOne, Some("buyer_id")));
        assert!(a("customer").not_null);
        assert_eq!((a("lines").relation, a("lines").target.as_str(), a("lines").mapped_by), (Relation::OneToMany, "OrderLine", true));
        assert_eq!(a("id").pos, Position::new(11, 17));
        assert_eq!(c.qualified("Currency").as_deref(), Some("shop.money.Currency"));
        assert_eq!(c.qualified("Customer").as_deref(), Some("shop.Customer"));
        assert_eq!(c.qualified("BigDecimal").as_deref(), Some("java.math.BigDecimal"));
        assert_eq!(c.qualified("Long"), None);
        assert_eq!(&JAVA[c.body_end.unwrap()..], "}\n");
    }

    #[test]
    fn reads_a_kotlin_entity_with_constructor_properties() {
        let c = parse_class(KOTLIN, "/p/Customer.kt").unwrap();
        assert_eq!(names(&c), ["name", "id", "orders", "email"]);
        assert_eq!(c.superclass.as_deref(), Some("BaseEntity"));
        assert!(c.attributes[0].not_null);
        assert_eq!(c.attributes[1].generated.as_deref(), Some("AUTO"));
        assert_eq!(c.attributes[2].target, "Order");
        assert_eq!(c.attributes[3].ty, "String");
    }

    #[test]
    fn cases() {
        assert_eq!(snake_case("OrderLine"), "order_line");
        assert_eq!(snake_case("customerID2x"), "customer_id2x");
        assert_eq!(camel_case("order_line", true), "OrderLine");
        assert_eq!(camel_case("CUSTOMER_ID", false), "customerId");
    }

    #[test]
    fn masking_keeps_offsets_and_unicode() {
        let text = "a /* é */ \"x\\\"y\" 'c' // ü\nb";
        let masked = mask(text);
        assert_eq!(masked.len(), text.len());
        assert_eq!(masked.trim_matches(' ').replace(' ', ""), "a\nb");
    }

    #[test]
    fn imports_go_after_the_package_without_other_imports() {
        let text = "package a;\n\nclass B {}\n";
        let edit = import_edit(text, &["lombok.Setter", "lombok.Getter", "lombok.Getter"], false).unwrap();
        assert_eq!(edit.start, 11);
        assert_eq!(edit.text, "\nimport lombok.Getter;\nimport lombok.Setter;\n");
    }
}
