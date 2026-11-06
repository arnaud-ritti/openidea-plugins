//! A Java declaration scanner: package, imports, (nested) types with their
//! annotations, supertypes and members, plus the method calls of a file.
//! Method bodies are skipped, not parsed. Offsets are UTF-8 bytes of the text.

const MODIFIERS: [&str; 13] = [
    "public",
    "protected",
    "private",
    "static",
    "final",
    "abstract",
    "synchronized",
    "native",
    "default",
    "transient",
    "volatile",
    "strictfp",
    "sealed",
];
const TYPE_KEYWORDS: [&str; 4] = ["class", "interface", "enum", "record"];
/// Identifiers that may come right before a call (`return foo()`), unlike a
/// type before a declared name (`void foo()`).
const BEFORE_CALL: [&str; 8] = ["return", "new", "throw", "else", "case", "yield", "assert", "await"];
const NOT_CALLS: [&str; 10] = ["if", "for", "while", "switch", "catch", "synchronized", "try", "super", "this", "return"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tok<'a> {
    pub s: &'a str,
    pub at: usize,
}

impl Tok<'_> {
    pub fn end(&self) -> usize {
        self.at + self.s.len()
    }

    pub fn is_ident(&self) -> bool {
        let c = self.s.as_bytes()[0];
        c.is_ascii_alphabetic() || c == b'_' || c == b'$' || c >= 0x80
    }

    /// The contents of a string literal token (`"…"` or a `"""` text block).
    pub fn string(&self) -> Option<Str> {
        let quote = if self.s.starts_with("\"\"\"") {
            3
        } else if self.s.starts_with('"') {
            1
        } else {
            return None;
        };
        let inner = &self.s[quote..];
        let inner = inner.strip_suffix(&self.s[..quote]).unwrap_or(inner);
        Some(Str { value: unescape(inner), at: self.at + quote, end: self.at + quote + inner.len() })
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A string literal: its value and the bytes between its quotes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Str {
    pub value: String,
    pub at: usize,
    pub end: usize,
}

impl Str {
    /// Whether the byte offset `at` is inside the quotes (or right after the last character).
    pub fn holds(&self, at: usize) -> bool {
        self.at <= at && at <= self.end
    }
}

/// The tokens of `text`: words, string/char literals (one token each) and
/// single punctuation characters. Comments are skipped.
pub fn tokens(text: &str) -> Vec<Tok<'_>> {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    let b = text.as_bytes();
    let find = |from: usize, pat: &[u8]| {
        b[from.min(b.len())..].windows(pat.len()).position(|w| w == pat).map_or(b.len(), |p| from + p + pat.len())
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if b[i..].starts_with(b"//") {
            i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i = find(i + 2, b"*/");
            continue;
        }
        if b[i..].starts_with(b"\"\"\"") {
            i = find(i + 3, b"\"\"\"");
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
        } else if word(c) {
            while i < b.len() && word(b[i]) {
                i += 1;
            }
        } else {
            i += 1;
        }
        // A multi-byte character inside a word or literal never splits: `i` is
        // only ever moved past ASCII bytes or to the end.
        while !text.is_char_boundary(i) {
            i += 1;
        }
        out.push(Tok { s: &text[start..i], at: start });
    }
    out
}

/// Index of the token closing the bracket at `open` (the last token if unclosed).
pub fn closing(toks: &[Tok], open: usize) -> usize {
    let (o, c) = match toks[open].s {
        "(" => ("(", ")"),
        "<" => ("<", ">"),
        "[" => ("[", "]"),
        _ => ("{", "}"),
    };
    let mut depth = 0;
    for (k, t) in toks.iter().enumerate().skip(open) {
        if t.s == o {
            depth += 1;
        } else if t.s == c {
            depth -= 1;
            if depth == 0 {
                return k;
            }
        }
    }
    toks.len().saturating_sub(1)
}

/// Splits `toks` at top-level commas; `angle`: `<…>` nests too (type contexts).
pub fn split_commas<'t, 'a>(toks: &'t [Tok<'a>], angle: bool) -> Vec<&'t [Tok<'a>]> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (k, t) in toks.iter().enumerate() {
        match t.s {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "<" if angle => depth += 1,
            ">" if angle => depth -= 1,
            "," if depth == 0 => {
                parts.push(&toks[start..k]);
                start = k + 1;
            }
            _ => {}
        }
    }
    if start < toks.len() {
        parts.push(&toks[start..]);
    }
    parts
}

/// A type as written: `Map<String, List<Foo>>` is `Map` with arguments `String` and `List<Foo>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypeRef {
    /// The simple name (`java.util.List` → `List`).
    pub name: String,
    /// The source text.
    pub text: String,
    /// Byte offset of the simple name.
    pub at: usize,
    pub args: Vec<TypeRef>,
    /// `Foo[]` or `Foo...`.
    pub array: bool,
}

/// The type of the tokens `toks` (a type expression), skipping type-use annotations.
pub fn type_ref(src: &str, toks: &[Tok]) -> Option<TypeRef> {
    let mut i = 0;
    let mut kept = Vec::new();
    while i < toks.len() {
        if toks[i].s == "@" {
            let mut skipped = Vec::new();
            i = annotation(src, toks, i, &mut skipped);
            continue;
        }
        kept.push(toks[i]);
        i += 1;
    }
    let mut toks: &[Tok] = &kept;
    while let Some(first) = toks.first() {
        if matches!(first.s, "?" | "extends" | "super" | "final") && toks.len() > 1 {
            toks = &toks[1..];
        } else {
            break;
        }
    }
    let first = toks.first()?;
    let last = toks.last()?;
    let text = src[first.at..last.end()].to_owned();
    let lt = toks.iter().position(|t| t.s == "<");
    let head = &toks[..lt.unwrap_or(toks.len())];
    let name = head.iter().rev().find(|t| t.is_ident())?;
    let args = match lt {
        Some(lt) => {
            let gt = lt + closing(&toks[lt..], 0);
            split_commas(&toks[lt + 1..gt.max(lt + 1)], true).into_iter().filter_map(|p| type_ref(src, p)).collect()
        }
        None => Vec::new(),
    };
    let array = toks.iter().any(|t| t.s == "[") || text.ends_with("...");
    Some(TypeRef { name: name.s.to_owned(), text, at: name.at, args, array })
}

/// One `name = value` of an annotation; a lone value is named `value`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arg {
    pub name: String,
    /// The value's source text and bytes.
    pub text: String,
    pub at: usize,
    pub end: usize,
    /// Every string literal of the value (`{"a", "b"}` has two).
    pub strings: Vec<Str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    /// The simple name (`org.springframework.stereotype.Service` → `Service`).
    pub name: String,
    /// Byte offset of the `@`.
    pub at: usize,
    /// Byte offset of the simple name.
    pub name_at: usize,
    /// After the closing parenthesis, or the name.
    pub end: usize,
    pub args: Vec<Arg>,
}

impl Annotation {
    pub fn arg(&self, name: &str) -> Option<&Arg> {
        self.args.iter().find(|a| a.name == name)
    }

    /// The string literals of the first of `names` that is set (`["value", "path"]`).
    pub fn strings(&self, names: &[&str]) -> Vec<&Str> {
        names.iter().find_map(|n| self.arg(n)).map(|a| a.strings.iter().collect()).unwrap_or_default()
    }
}

/// Parses the annotation at `i` (an `@`) into `out`; the index after it.
pub fn annotation(src: &str, toks: &[Tok], i: usize, out: &mut Vec<Annotation>) -> usize {
    let mut j = i + 1;
    if toks.get(j).is_none_or(|t| !t.is_ident() || t.s == "interface") {
        return j;
    }
    // A Kotlin use-site target: `@field:Autowired`, `@file:JvmName`.
    if toks.get(j + 1).is_some_and(|t| t.s == ":") && toks.get(j + 2).is_some_and(|t| t.is_ident()) {
        j += 2;
    }
    while j + 2 < toks.len() && toks[j + 1].s == "." && toks[j + 2].is_ident() {
        j += 2;
    }
    let name = toks[j];
    let mut end = name.end();
    let mut args = Vec::new();
    j += 1;
    if toks.get(j).is_some_and(|t| t.s == "(") {
        let close = closing(toks, j);
        for part in split_commas(&toks[j + 1..close.max(j + 1)], false) {
            let (arg_name, value) = if part.len() > 2 && part[1].s == "=" && part[0].is_ident() {
                (part[0].s, &part[2..])
            } else {
                ("value", part)
            };
            let (Some(first), Some(last)) = (value.first(), value.last()) else { continue };
            args.push(Arg {
                name: arg_name.to_owned(),
                text: src[first.at..last.end()].to_owned(),
                at: first.at,
                end: last.end(),
                strings: value.iter().filter_map(|t| t.string()).collect(),
            });
        }
        end = toks[close].end();
        j = close + 1;
    }
    out.push(Annotation { name: name.s.to_owned(), at: toks[i].at, name_at: name.at, end, args });
    j
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Field,
    Method,
    Constructor,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    pub annotations: Vec<Annotation>,
    pub ty: TypeRef,
    pub name: String,
    /// Byte offset of the name.
    pub at: usize,
}

impl Param {
    pub fn annotation(&self, name: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.name == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub kind: MemberKind,
    pub name: String,
    /// Byte offset of the name.
    pub at: usize,
    pub annotations: Vec<Annotation>,
    pub modifiers: Vec<String>,
    /// The field type or return type (`void` included); none for a constructor.
    pub ty: Option<TypeRef>,
    pub params: Vec<Param>,
    /// A method's `{…}` or a field's initializer, `(from, to)` bytes.
    pub body: Option<(usize, usize)>,
    /// From the first annotation or modifier to the end of the declaration.
    pub start: usize,
    pub end: usize,
}

impl Member {
    pub fn annotation(&self, name: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.name == name)
    }

    pub fn has(&self, names: &[&str]) -> bool {
        self.annotations.iter().any(|a| names.contains(&a.name.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: String,
    /// Byte offset of the name.
    pub at: usize,
    /// `class`, `interface`, `enum`, `record` or `@interface`.
    pub kind: String,
    pub annotations: Vec<Annotation>,
    pub modifiers: Vec<String>,
    pub extends: Vec<TypeRef>,
    pub implements: Vec<TypeRef>,
    /// A record's components.
    pub components: Vec<Param>,
    pub members: Vec<Member>,
    /// Index of the enclosing type in [`JavaFile::types`].
    pub outer: Option<usize>,
    /// From the first annotation or modifier to the closing brace.
    pub start: usize,
    pub end: usize,
}

impl TypeDecl {
    pub fn annotation(&self, name: &str) -> Option<&Annotation> {
        self.annotations.iter().find(|a| a.name == name)
    }

    pub fn has(&self, names: &[&str]) -> bool {
        self.annotations.iter().any(|a| names.contains(&a.name.as_str()))
    }

    /// `extends` and `implements` together.
    pub fn supers(&self) -> impl Iterator<Item = &TypeRef> {
        self.extends.iter().chain(&self.implements)
    }

    pub fn methods(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.kind == MemberKind::Method)
    }

    pub fn fields(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|m| m.kind == MemberKind::Field)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Import {
    /// `java.util.List`, `com.acme.*`, `org.junit.Assert.assertEquals` (static).
    pub path: String,
    pub is_static: bool,
    /// The bytes of the path.
    pub at: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaFile {
    pub package: String,
    pub imports: Vec<Import>,
    /// Every type, nested ones after their enclosing type.
    pub types: Vec<TypeDecl>,
    /// Annotations of the package declaration (`package-info.java`).
    pub package_annotations: Vec<Annotation>,
}

impl JavaFile {
    /// False when the file imports `simple` from a package outside Spring.
    pub fn from_spring(&self, simple: &str) -> bool {
        let suffix = format!(".{simple}");
        self.imports
            .iter()
            .filter(|i| !i.is_static && i.path.ends_with(&suffix))
            .all(|i| i.path.starts_with("org.springframework."))
    }

    /// The fully qualified name of `types[i]` (`com.acme.Outer.Inner`).
    pub fn qualified(&self, i: usize) -> String {
        let mut names = vec![self.types[i].name.as_str()];
        let mut outer = self.types[i].outer;
        while let Some(o) = outer {
            names.push(&self.types[o].name);
            outer = self.types[o].outer;
        }
        names.reverse();
        if self.package.is_empty() { names.join(".") } else { format!("{}.{}", self.package, names.join(".")) }
    }

    /// The fully qualified name `simple` refers to in this file: an import, a
    /// type of the file, or the file's package.
    pub fn resolve(&self, simple: &str) -> String {
        if let Some(i) = self.imports.iter().find(|i| !i.is_static && i.path.rsplit('.').next() == Some(simple)) {
            return i.path.clone();
        }
        if let Some(i) = self.types.iter().position(|t| t.name == simple) {
            return self.qualified(i);
        }
        if self.package.is_empty() { simple.to_owned() } else { format!("{}.{simple}", self.package) }
    }
}

/// Parses the declarations of a Java source.
pub fn parse(text: &str) -> JavaFile {
    let toks = tokens(text);
    let mut out = JavaFile::default();
    let mut annotations: Vec<Annotation> = Vec::new();
    let mut modifiers: Vec<String> = Vec::new();
    let mut start: Option<usize> = None;
    // (index in out.types, brace depth of its body)
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if depth == 0 && (t.s == "package" || t.s == "import") {
            let end = (i..toks.len()).find(|&k| toks[k].s == ";").unwrap_or(toks.len());
            let is_static = toks.get(i + 1).is_some_and(|t| t.s == "static");
            let from = if is_static { i + 2 } else { i + 1 };
            if from < end {
                let (at, to) = (toks[from].at, toks[end - 1].end());
                let path: String = toks[from..end].iter().map(|t| t.s).collect();
                if t.s == "package" {
                    out.package = path;
                    out.package_annotations = std::mem::take(&mut annotations);
                } else {
                    out.imports.push(Import { path, is_static, at, end: to });
                }
            }
            annotations.clear();
            i = end + 1;
            continue;
        }
        if t.s == "@" && toks.get(i + 1).is_some_and(|n| n.s != "interface") {
            start.get_or_insert(t.at);
            i = annotation(text, &toks, i, &mut annotations);
            continue;
        }
        let at_member_level = stack.last().is_none_or(|&(_, d)| d == depth);
        if at_member_level && MODIFIERS.contains(&t.s) && toks.get(i + 1).is_some_and(|n| n.s != "(" && n.s != ".") {
            start.get_or_insert(t.at);
            modifiers.push(t.s.to_owned());
            i += 1;
            continue;
        }
        let annotation_type = t.s == "@" && toks.get(i + 1).is_some_and(|n| n.s == "interface");
        let keyword = if annotation_type { i + 1 } else { i };
        if (annotation_type || TYPE_KEYWORDS.contains(&t.s))
            && (i == 0 || toks[i - 1].s != ".")
            && toks.get(keyword + 1).is_some_and(|n| n.is_ident())
            && at_member_level
        {
            let name = toks[keyword + 1];
            let mut decl = TypeDecl {
                name: name.s.to_owned(),
                at: name.at,
                kind: if annotation_type { "@interface".into() } else { t.s.to_owned() },
                annotations: std::mem::take(&mut annotations),
                modifiers: std::mem::take(&mut modifiers),
                extends: Vec::new(),
                implements: Vec::new(),
                components: Vec::new(),
                members: Vec::new(),
                outer: stack.last().map(|&(o, _)| o),
                start: start.take().unwrap_or(t.at),
                end: text.len(),
            };
            let mut j = keyword + 2;
            while j < toks.len() && toks[j].s != "{" && toks[j].s != ";" {
                match toks[j].s {
                    "<" => j = closing(&toks, j) + 1,
                    "(" => {
                        let close = closing(&toks, j);
                        decl.components = params(text, &toks[j + 1..close.max(j + 1)]);
                        j = close + 1;
                    }
                    "extends" | "implements" | "permits" => {
                        let end = (j + 1..toks.len())
                            .find(|&k| matches!(toks[k].s, "{" | "implements" | "permits" | "extends"))
                            .unwrap_or(toks.len());
                        let list: Vec<TypeRef> =
                            split_commas(&toks[j + 1..end], true).into_iter().filter_map(|p| type_ref(text, p)).collect();
                        match toks[j].s {
                            "extends" => decl.extends = list,
                            "implements" => decl.implements = list,
                            _ => {}
                        }
                        j = end;
                    }
                    _ => j += 1,
                }
            }
            let is_enum = decl.kind == "enum";
            out.types.push(decl);
            if toks.get(j).is_some_and(|t| t.s == "{") {
                depth += 1;
                stack.push((out.types.len() - 1, depth));
                i = j + 1;
                if is_enum {
                    i = skip_enum_constants(&toks, i);
                }
            } else {
                i = j + 1;
            }
            continue;
        }
        match t.s {
            "{" if at_member_level && !stack.is_empty() => {
                // An initializer block.
                i = closing(&toks, i) + 1;
                annotations.clear();
                modifiers.clear();
                start = None;
                continue;
            }
            "{" => depth += 1,
            "}" => {
                if let Some(&(o, d)) = stack.last()
                    && d == depth
                {
                    out.types[o].end = t.end();
                    stack.pop();
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
        if let Some(&(o, d)) = stack.last()
            && d == depth
            && (t.is_ident() || t.s == "<")
            && let Some((member, next)) = member(text, &toks, i, &out.types[o].name)
        {
            let mut member = member;
            member.annotations.splice(0..0, std::mem::take(&mut annotations));
            member.modifiers.splice(0..0, std::mem::take(&mut modifiers));
            member.start = start.take().unwrap_or(member.start);
            out.types[o].members.push(member);
            i = next;
            continue;
        }
        annotations.clear();
        modifiers.clear();
        start = None;
        i += 1;
    }
    out
}

/// Skips an enum's constants: the index after the `;` ending them, or of the closing `}`.
fn skip_enum_constants(toks: &[Tok], mut i: usize) -> usize {
    while i < toks.len() {
        match toks[i].s {
            "(" | "{" => i = closing(toks, i) + 1,
            ";" => return i + 1,
            "}" => return i,
            _ => i += 1,
        }
    }
    i
}

fn params(src: &str, toks: &[Tok]) -> Vec<Param> {
    split_commas(toks, true)
        .into_iter()
        .filter_map(|p| {
            let mut annotations = Vec::new();
            let mut k = 0;
            while k < p.len() {
                if p[k].s == "@" {
                    k = annotation(src, p, k, &mut annotations);
                } else if p[k].s == "final" {
                    k += 1;
                } else {
                    break;
                }
            }
            let rest = &p[k..];
            let name = rest.last().filter(|t| t.is_ident())?;
            if rest.len() < 2 {
                return None;
            }
            let ty = type_ref(src, &rest[..rest.len() - 1])?;
            Some(Param { annotations, ty, name: name.s.to_owned(), at: name.at })
        })
        .collect()
}

/// Parses the field, method or constructor starting at `i`: it and the index after it.
fn member(src: &str, toks: &[Tok], i: usize, class: &str) -> Option<(Member, usize)> {
    let mut j = i;
    let mut decl: Vec<Tok> = Vec::new();
    let mut modifiers: Vec<String> = Vec::new();
    let mut annotations = Vec::new();
    loop {
        let t = *toks.get(j)?;
        match t.s {
            "(" | ";" | "=" | "," => break,
            "{" | "}" => return None,
            "@" => j = annotation(src, toks, j, &mut annotations),
            s if TYPE_KEYWORDS.contains(&s) => return None,
            "<" if decl.is_empty() => j = closing(toks, j) + 1, // a generic method's `<T>`
            "<" => {
                let gt = closing(toks, j);
                decl.extend_from_slice(&toks[j..=gt]);
                j = gt + 1;
            }
            s if decl.is_empty() && MODIFIERS.contains(&s) => {
                modifiers.push(s.to_owned());
                j += 1;
            }
            _ => {
                decl.push(t);
                j += 1;
            }
        }
    }
    let name = *decl.last().filter(|t| t.is_ident())?;
    let start = toks[i].at;
    if toks[j].s == "(" {
        let close = closing(toks, j);
        let constructor = decl.len() == 1 && name.s == class;
        if decl.len() < 2 && !constructor {
            return None;
        }
        let mut k = close + 1;
        while k < toks.len() && toks[k].s != "{" && toks[k].s != ";" && toks[k].s != "}" {
            k += 1;
        }
        let (body, next, end) = match toks.get(k) {
            Some(t) if t.s == "{" => {
                let c = closing(toks, k);
                (Some((t.at, toks[c].end())), c + 1, toks[c].end())
            }
            Some(t) if t.s == ";" => (None, k + 1, t.end()),
            _ => (None, k, toks[close].end()),
        };
        let member = Member {
            kind: if constructor { MemberKind::Constructor } else { MemberKind::Method },
            name: name.s.to_owned(),
            at: name.at,
            annotations,
            modifiers,
            ty: if constructor { None } else { type_ref(src, &decl[..decl.len() - 1]) },
            params: params(src, &toks[j + 1..close.max(j + 1)]),
            body,
            start,
            end,
        };
        return Some((member, next));
    }
    if decl.len() < 2 {
        return None;
    }
    let ty = type_ref(src, &decl[..decl.len() - 1]);
    let mut k = j;
    let mut nesting = 0;
    while k < toks.len() {
        match toks[k].s {
            "{" | "(" | "[" => nesting += 1,
            "}" | ")" | "]" if nesting == 0 => break,
            "}" | ")" | "]" => nesting -= 1,
            ";" if nesting == 0 => break,
            _ => {}
        }
        k += 1;
    }
    let end = toks.get(k).map_or(src.len(), |t| t.end());
    let body = (toks[j].s == "=" && j + 1 < k).then(|| (toks[j + 1].at, toks[k - 1].end()));
    let member =
        Member { kind: MemberKind::Field, name: name.s.to_owned(), at: name.at, annotations, modifiers, ty, params: Vec::new(), body, start, end };
    Some((member, k + 1))
}

/// An argument of a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallArg {
    pub text: String,
    pub at: usize,
    pub end: usize,
    /// The literal, when the argument is exactly one string literal.
    pub string: Option<Str>,
}

/// A method call or constructor call (`new Foo(…)`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    /// Byte offset of the name.
    pub at: usize,
    /// The identifier before the `.`, if any (`kafkaTemplate.send` → `kafkaTemplate`).
    pub receiver: Option<String>,
    /// Called on the result of another call (`….and().csrf()`).
    pub chained: bool,
    pub is_new: bool,
    pub args: Vec<CallArg>,
    /// After the closing parenthesis.
    pub end: usize,
}

/// Every call in `text`, declarations and annotations excluded.
pub fn calls(text: &str) -> Vec<Call> {
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut k = 0;
    while k + 1 < toks.len() {
        let t = toks[k];
        if t.s == "@" {
            let mut skipped = Vec::new();
            k = annotation(text, &toks, k, &mut skipped).max(k + 1);
            continue;
        }
        if !t.is_ident() || NOT_CALLS.contains(&t.s) {
            k += 1;
            continue;
        }
        // Walk back over a qualified name (`new java.util.ArrayList(`).
        let mut first = k;
        while first >= 2 && toks[first - 1].s == "." && toks[first - 2].is_ident() {
            first -= 2;
        }
        let prev = first.checked_sub(1).map(|p| toks[p]);
        let is_new = prev.is_some_and(|p| p.s == "new");
        // `new ArrayList<>(…)`: type arguments between the name and `(`.
        let paren = if is_new && toks[k + 1].s == "<" { closing(&toks, k + 1) + 1 } else { k + 1 };
        if toks.get(paren).is_none_or(|p| p.s != "(") {
            k += 1;
            continue;
        }
        let declaration = match prev {
            Some(p) if p.is_ident() => !BEFORE_CALL.contains(&p.s),
            Some(p) if p.s == ">" => first < 2 || toks[first - 2].s != "-",
            Some(p) if p.s == "]" => true,
            _ => false,
        };
        let declaration = declaration && !(k > 0 && toks[k - 1].s == ".");
        if declaration && !is_new {
            k += 1;
            continue;
        }
        let (receiver, chained) = if k >= 2 && toks[k - 1].s == "." && !is_new {
            let r = toks[k - 2];
            (r.is_ident().then(|| r.s.to_owned()), r.s == ")")
        } else {
            (None, false)
        };
        let close = closing(&toks, paren);
        let args = split_commas(&toks[paren + 1..close.max(paren + 1)], false)
            .into_iter()
            .filter_map(|a| {
                let (first, last) = (a.first()?, a.last()?);
                Some(CallArg {
                    text: text[first.at..last.end()].to_owned(),
                    at: first.at,
                    end: last.end(),
                    string: if a.len() == 1 { first.string() } else { None },
                })
            })
            .collect();
        out.push(Call { name: t.s.to_owned(), at: t.at, receiver, chained, is_new, args, end: toks[close].end() });
        k += 2; // arguments may hold calls too
    }
    out
}

/// Stereotypes that make a class a bean (meta-annotated with `@Component`).
pub const STEREOTYPES: [&str; 12] = [
    "Component",
    "Service",
    "Repository",
    "Controller",
    "RestController",
    "Configuration",
    "SpringBootApplication",
    "ControllerAdvice",
    "RestControllerAdvice",
    "Endpoint",
    "ShellComponent",
    "TestConfiguration",
];

/// A bean a Java file declares by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeanName {
    pub name: String,
    /// Byte offset of the declaring class or method name.
    pub at: usize,
    pub len: usize,
    /// The class name, or the `@Bean` method's return type.
    pub ty: String,
}

/// The named beans of a file: stereotype classes (explicit name or the
/// decapitalized class name) and `@Bean` methods (`name`/`value`, else the
/// method name).
pub fn bean_names(file: &JavaFile) -> Vec<BeanName> {
    let mut out = Vec::new();
    for t in &file.types {
        if let Some(a) = t.annotations.iter().find(|a| STEREOTYPES.contains(&a.name.as_str())) {
            let name = a.strings(&["value"]).first().map(|s| s.value.clone()).filter(|s| !s.is_empty());
            out.push(BeanName {
                name: name.unwrap_or_else(|| crate::decapitalize(&t.name)),
                at: t.at,
                len: t.name.len(),
                ty: t.name.clone(),
            });
        }
        for m in t.methods().filter(|m| m.has(&["Bean"])) {
            let ty = m.ty.as_ref().map(|t| t.name.clone()).unwrap_or_default();
            let names = m.annotation("Bean").map(|a| a.strings(&["name", "value"])).unwrap_or_default();
            if names.is_empty() {
                out.push(BeanName { name: m.name.clone(), at: m.at, len: m.name.len(), ty: ty.clone() });
            }
            for n in names {
                out.push(BeanName { name: n.value.clone(), at: m.at, len: m.name.len(), ty: ty.clone() });
            }
        }
    }
    out
}

/// The request mappings of a controller method: `(HTTP method or "", path)`,
/// with the class-level `@RequestMapping` applied. Empty when it has none.
pub fn mappings(class: &TypeDecl, method: &Member) -> Vec<(String, String)> {
    let paths = |a: &Annotation| -> Vec<String> {
        let s: Vec<String> = a.strings(&["value", "path"]).iter().map(|s| s.value.clone()).collect();
        if s.is_empty() { vec![String::new()] } else { s }
    };
    let prefixes = class.annotation("RequestMapping").map(paths).unwrap_or_else(|| vec![String::new()]);
    let mut out = Vec::new();
    for a in &method.annotations {
        let verb = match a.name.as_str() {
            "GetMapping" => "GET",
            "PostMapping" => "POST",
            "PutMapping" => "PUT",
            "DeleteMapping" => "DELETE",
            "PatchMapping" => "PATCH",
            "RequestMapping" => "",
            _ => continue,
        };
        let verb = if verb.is_empty() {
            a.arg("method").and_then(|m| m.text.rsplit('.').next()).unwrap_or("").trim_matches(['{', '}', ' ']).to_owned()
        } else {
            verb.to_owned()
        };
        for prefix in &prefixes {
            for path in paths(a) {
                out.push((verb.clone(), join_paths(prefix, &path)));
            }
        }
    }
    out
}

/// `/api` + `users/{id}` → `/api/users/{id}`.
pub fn join_paths(prefix: &str, path: &str) -> String {
    let joined = format!("/{}/{}", prefix.trim_matches('/'), path.trim_start_matches('/'));
    let mut out = String::with_capacity(joined.len());
    for c in joined.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    if out.len() > 1 && out.ends_with('/') && !path.ends_with('/') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_spring_accepts_spring_wildcard_and_missing_imports() {
        assert!(parse("package a;\nclass A {}").from_spring("Query"));
        assert!(parse("import org.springframework.data.jpa.repository.*;\nclass A {}").from_spring("Query"));
        assert!(parse("import org.springframework.data.jpa.repository.Query;\nclass A {}").from_spring("Query"));
    }

    #[test]
    fn from_spring_rejects_foreign_import() {
        assert!(!parse("import com.acme.Query;\nclass A {}").from_spring("Query"));
        assert!(parse("import com.acme.Query;\nclass A {}").from_spring("Page"));
    }

    const SRC: &str = r#"package com.acme.web;

import java.util.List;
import static org.junit.Assert.*;

/** @Component in a comment */
@RestController
@RequestMapping({"/api", "/v1"})
public class UserController extends Base<User> implements Api, Other<String, List<Long>> {
    private static final String X = "a;b{";
    @Autowired private UserService users;
    private final Map<String, List<Order>> orders = new HashMap<>() {{ put("x", null); }};
    int a, b;

    static { init(); }

    public UserController(UserService users, @Qualifier("x") final Clock clock) {
        this.users = users;
    }

    @GetMapping(value = "/users/{id}", produces = "application/json")
    public <T> ResponseEntity<User> get(@PathVariable("id") Long id, String... rest) throws IOException {
        return users.find(id).map(u -> ok(u)).orElseThrow();
    }

    @RequestMapping(path = "/search", method = RequestMethod.POST)
    abstract List<User> search(@RequestBody Query q);

    enum Kind { A("a") { void f() {} }, B; int code; }

    record Page(int number, @JsonProperty("sz") int size) {}

    @interface Marker { String value() default "x"; }
}
"#;

    #[test]
    fn parses_declarations() {
        let f = parse(SRC);
        assert_eq!(f.package, "com.acme.web");
        assert_eq!(f.imports.iter().map(|i| (i.path.as_str(), i.is_static)).collect::<Vec<_>>(), [
            ("java.util.List", false),
            ("org.junit.Assert.*", true)
        ]);
        let names: Vec<_> = f.types.iter().map(|t| (t.kind.as_str(), t.name.as_str(), t.outer)).collect();
        assert_eq!(names, [
            ("class", "UserController", None),
            ("enum", "Kind", Some(0)),
            ("record", "Page", Some(0)),
            ("@interface", "Marker", Some(0))
        ]);
        let c = &f.types[0];
        assert_eq!(c.annotations.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["RestController", "RequestMapping"]);
        assert_eq!(c.annotation("RequestMapping").unwrap().strings(&["value"]).len(), 2);
        assert_eq!(c.extends[0].name, "Base");
        assert_eq!(c.extends[0].args[0].name, "User");
        assert_eq!(c.implements.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Api", "Other"]);
        assert_eq!(c.implements[1].args[1].args[0].name, "Long");

        let members: Vec<_> = c.members.iter().map(|m| (m.kind, m.name.as_str())).collect();
        assert_eq!(members, [
            (MemberKind::Field, "X"),
            (MemberKind::Field, "users"),
            (MemberKind::Field, "orders"),
            (MemberKind::Field, "a"),
            (MemberKind::Constructor, "UserController"),
            (MemberKind::Method, "get"),
            (MemberKind::Method, "search"),
        ]);
        assert!(c.members[0].modifiers.contains(&"static".to_owned()));
        assert_eq!(c.members[1].annotations[0].name, "Autowired");
        assert_eq!(c.members[2].ty.as_ref().unwrap().args[1].args[0].name, "Order");
        let ctor = &c.members[4];
        assert_eq!(ctor.params.iter().map(|p| (p.ty.name.as_str(), p.name.as_str())).collect::<Vec<_>>(), [
            ("UserService", "users"),
            ("Clock", "clock")
        ]);
        assert_eq!(ctor.params[1].annotations[0].strings(&["value"])[0].value, "x");
        let get = &c.members[5];
        assert_eq!(get.ty.as_ref().unwrap().name, "ResponseEntity");
        assert_eq!(get.params[0].annotation("PathVariable").unwrap().strings(&["value"])[0].value, "id");
        assert!(get.params[1].ty.array);
        assert_eq!(&SRC[get.at..get.at + 3], "get");
        assert!(get.body.is_some());
        assert_eq!(mappings(c, get), [("GET".into(), "/api/users/{id}".into()), ("GET".into(), "/v1/users/{id}".into())]);
        assert_eq!(mappings(c, &c.members[6])[0], ("POST".into(), "/api/search".into()));
        assert!(c.members[6].body.is_none());
        assert_eq!(f.types[1].members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["code"]);
        assert_eq!(f.types[2].components.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["number", "size"]);
        assert_eq!(f.resolve("List"), "java.util.List");
        assert_eq!(f.resolve("Kind"), "com.acme.web.UserController.Kind");
        assert_eq!(f.resolve("Foo"), "com.acme.web.Foo");
    }

    #[test]
    fn finds_calls() {
        let src = r#"class A {
    void run() {
        kafkaTemplate.send("orders", order);
        rabbit.convertAndSend("ex", "key." + id, msg);
        http.csrf().disable();
        list.forEach(x -> handle(x));
        Object o = new java.util.ArrayList<>(3);
        return build();
    }
    @Bean(name = "x") Queue q() { return new Queue("q1"); }
}"#;
        let calls = calls(src);
        let names: Vec<_> = calls.iter().map(|c| (c.receiver.as_deref(), c.name.as_str(), c.is_new, c.chained)).collect();
        assert_eq!(names, [
            (Some("kafkaTemplate"), "send", false, false),
            (Some("rabbit"), "convertAndSend", false, false),
            (Some("http"), "csrf", false, false),
            (None, "disable", false, true),
            (Some("list"), "forEach", false, false),
            (None, "handle", false, false),
            (None, "ArrayList", true, false),
            (None, "build", false, false),
            (None, "Queue", true, false),
        ]);
        assert_eq!(calls[0].args[0].string.as_ref().unwrap().value, "orders");
        assert!(calls[1].args[1].string.is_none());
        let s = calls[0].args[0].string.as_ref().unwrap();
        assert_eq!(&src[s.at..s.end], "orders");
    }

    #[test]
    fn names_beans() {
        let f = parse(
            r#"@Service("users") class UserServiceImpl {}
@Configuration class AppConfig {
    @Bean Clock clock() { return null; }
    @Bean({"a", "b"}) DataSource ds() { return null; }
}"#,
        );
        let names: Vec<_> = bean_names(&f).into_iter().map(|b| (b.name, b.ty)).collect();
        assert_eq!(names, [
            ("users".to_owned(), "UserServiceImpl".to_owned()),
            ("appConfig".into(), "AppConfig".into()),
            ("clock".into(), "Clock".into()),
            ("a".into(), "DataSource".into()),
            ("b".into(), "DataSource".into()),
        ]);
    }

    #[test]
    fn package_info_annotations() {
        let f = parse("@ApplicationModule(allowedDependencies = {\"order\", \"inventory::api\"})\npackage com.acme.shipping;\n\nimport org.springframework.modulith.ApplicationModule;\n");
        assert_eq!(f.package, "com.acme.shipping");
        assert_eq!(f.package_annotations[0].strings(&["allowedDependencies"]).len(), 2);
    }

    #[test]
    fn joins_paths() {
        assert_eq!(join_paths("", ""), "/");
        assert_eq!(join_paths("/api/", "/x"), "/api/x");
        assert_eq!(join_paths("api", ""), "/api");
    }

    #[test]
    fn survives_broken_and_unicode_source() {
        for src in ["class", "class A {", "@", "class A { void f(", "class É { String s = \"é", "\"\"\"", "enum E { A(", "@interface"] {
            parse(src);
            calls(src);
        }
        // Every prefix: a file being typed.
        for end in (0..=SRC.len()).filter(|&e| SRC.is_char_boundary(e)) {
            parse(&SRC[..end]);
            calls(&SRC[..end]);
        }
    }
}
