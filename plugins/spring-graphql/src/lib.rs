//! Spring for GraphQL Tools for OpenIDEA: indexes the project's GraphQL schema
//! files and the `@…Mapping` handler methods of its controllers, and links
//! them both ways. Text-based: types and fields match by name, as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin,
};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET, project_of};
use spring_text::java::{self, Annotation, Member, Str, TypeDecl};
use spring_text::kotlin;
use spring_text::{contains, range};
use std::collections::BTreeMap;

mod checks;

const SCHEMA_GLOBS: &[&str] = &["**/*.graphqls", "**/*.graphql"];
const JAVA_GLOBS: &[&str] = &["**/*.java", "**/*.kt"];
const DOCUMENT_DIR: &str = "/graphql-documents/";
/// (annotation, operation, default root type).
const ROOTS: [(&str, &str, &str); 3] = [
    ("QueryMapping", "query", "Query"),
    ("MutationMapping", "mutation", "Mutation"),
    ("SubscriptionMapping", "subscription", "Subscription"),
];
/// Handler parameters bound from the arguments or the environment, not the source object.
const NOT_SOURCE_ANNOTATIONS: [&str; 5] =
    ["Argument", "Arguments", "ContextValue", "LocalContextValue", "AuthenticationPrincipal"];
const NOT_SOURCE_TYPES: [&str; 10] = [
    "DataFetchingEnvironment",
    "GraphQLContext",
    "Principal",
    "Authentication",
    "DataFetchingFieldSelectionSet",
    "SelectionSet",
    "Locale",
    "ArgumentValue",
    "ScrollSubrange",
    "Subrange",
];
/// Keywords starting a top-level definition: a type without a body ends there.
const DEFINITIONS: [&str; 13] = [
    "type",
    "interface",
    "input",
    "enum",
    "union",
    "scalar",
    "schema",
    "extend",
    "directive",
    "query",
    "mutation",
    "subscription",
    "fragment",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub range: Range,
    pub args: Vec<String>,
    /// The named type of the field, without list and non-null wrappers.
    pub ty: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GqlType {
    pub name: String,
    /// `type`, `interface` or `input`.
    pub kind: String,
    pub range: Range,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Schema {
    /// Extensions (`extend type Query`) are listed as types of their own.
    pub types: Vec<GqlType>,
    /// `schema { query: MyQuery }`: (operation, type).
    pub roots: Vec<(String, String)>,
}

type Tok<'a> = (usize, &'a str);

/// Names, string literals (one token each) and punctuation; whitespace,
/// commas and `#` comments are skipped.
fn tokens(text: &str) -> Vec<Tok<'_>> {
    let b = text.as_bytes();
    let name = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let (c, start) = (b[i], i);
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        if c == b'#' {
            i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |p| i + p);
            continue;
        }
        if b[i..].starts_with(b"\"\"\"") {
            i = b[i + 3..].windows(3).position(|w| w == b"\"\"\"").map_or(b.len(), |p| i + 3 + p + 3);
        } else if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else if name(c) {
            while i < b.len() && name(b[i]) {
                i += 1;
            }
        } else {
            i += 1;
        }
        i = i.min(b.len());
        while !text.is_char_boundary(i) {
            i += 1;
        }
        out.push((start, &text[start..i]));
    }
    out
}

fn is_name(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
}

/// Index of the token closing the bracket at `open` (`toks.len()` if unclosed).
fn close(toks: &[Tok], open: usize) -> usize {
    let (o, c) = match toks[open].1 {
        "(" => ("(", ")"),
        "[" => ("[", "]"),
        _ => ("{", "}"),
    };
    let mut depth = 0;
    for (k, t) in toks.iter().enumerate().skip(open) {
        if t.1 == o {
            depth += 1;
        } else if t.1 == c {
            depth -= 1;
            if depth == 0 {
                return k;
            }
        }
    }
    toks.len()
}

/// The fields of a `{…}` body (or the arguments of a `(…)` list): names followed by `(` or `:`.
fn fields(text: &str, toks: &[Tok]) -> Vec<Field> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let (at, s) = toks[k];
        match s {
            "@" => {
                k += 2;
                if toks.get(k).is_some_and(|t| t.1 == "(") {
                    k = close(toks, k) + 1;
                }
            }
            "(" | "[" | "{" => k = close(toks, k) + 1,
            _ if is_name(s) && matches!(toks.get(k + 1).map(|t| t.1), Some("(" | ":")) => {
                let mut args = Vec::new();
                k += 1;
                if toks[k].1 == "(" {
                    let c = close(toks, k);
                    args = fields(text, &toks[k + 1..c]).into_iter().map(|f| f.name).collect();
                    k = c + 1;
                }
                let mut j = k + 1;
                while toks.get(j).is_some_and(|t| t.1 == "[") {
                    j += 1;
                }
                let ty = match toks.get(k) {
                    Some(t) if t.1 == ":" => toks.get(j).map(|t| t.1).filter(|t| is_name(t)).unwrap_or_default(),
                    _ => "",
                };
                out.push(Field { name: s.to_owned(), range: range(text, at, at + s.len()), args, ty: ty.to_owned() });
            }
            _ => k += 1,
        }
    }
    out
}

/// The object, interface and input types of an SDL text and its `schema` block.
/// Operations, fragments, enums, unions and scalars are skipped.
pub fn parse_schema(text: &str) -> Schema {
    let toks = tokens(text);
    let mut out = Schema::default();
    let mut k = 0;
    while k < toks.len() {
        let s = toks[k].1;
        match s {
            "type" | "interface" | "input" | "schema" => {
                let name = toks.get(k + 1).copied().filter(|t| s != "schema" && is_name(t.1));
                let mut j = k + if name.is_some() { 2 } else { 1 };
                while j < toks.len() && toks[j].1 != "{" && !DEFINITIONS.contains(&toks[j].1) {
                    if toks[j].1 == "(" {
                        j = close(&toks, j);
                    }
                    j += 1;
                }
                let body = if toks.get(j).is_some_and(|t| t.1 == "{") {
                    let c = close(&toks, j);
                    let body = &toks[j + 1..c];
                    j = c + 1;
                    body
                } else {
                    &[]
                };
                if s == "schema" {
                    for w in body.windows(3).filter(|w| w[1].1 == ":") {
                        out.roots.push((w[0].1.to_owned(), w[2].1.to_owned()));
                    }
                } else if let Some((at, name)) = name {
                    out.types.push(GqlType {
                        name: name.to_owned(),
                        kind: s.to_owned(),
                        range: range(text, at, at + name.len()),
                        fields: fields(text, body),
                    });
                }
                k = j;
            }
            "{" | "(" | "[" => k = close(&toks, k) + 1,
            _ => k += 1,
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handler {
    /// `query`, `mutation` or `subscription` for a root mapping: its type is the schema's root type.
    pub op: Option<String>,
    pub ty: String,
    pub field: String,
    /// The field name as written in the annotation, else the method name.
    pub range: Range,
    pub method: Range,
    pub annotation: Range,
    /// `Controller.method()`.
    pub label: String,
    /// `@Argument` names.
    pub args: Vec<(String, Range)>,
}

/// The type a mapping annotation `a` of `m` binds: (root operation, type name).
fn target(class: &TypeDecl, m: &Member, a: &Annotation) -> Option<(Option<&'static str>, String)> {
    if let Some((_, op, ty)) = ROOTS.iter().find(|r| r.0 == a.name) {
        return Some((Some(op), (*ty).to_owned()));
    }
    let named = |a: &Annotation| a.strings(&["typeName"]).first().map(|s| s.value.clone()).filter(|s| !s.is_empty());
    let class_level = || class.annotation("SchemaMapping").and_then(named);
    let ty = match a.name.as_str() {
        "SchemaMapping" => named(a).or_else(class_level).or_else(|| {
            m.params
                .iter()
                .find(|p| {
                    !p.annotations.iter().any(|a| NOT_SOURCE_ANNOTATIONS.contains(&a.name.as_str()))
                        && !NOT_SOURCE_TYPES.contains(&p.ty.name.as_str())
                })
                .map(|p| p.ty.name.clone())
        }),
        "BatchMapping" => {
            named(a).or_else(class_level).or_else(|| Some(m.params.first()?.ty.args.first()?.name.clone()))
        }
        _ => None,
    }?;
    Some((None, ty))
}

/// The annotation attributes naming the field.
fn field_attrs(a: &Annotation) -> &'static [&'static str] {
    if ROOTS.iter().any(|r| r.0 == a.name) { &["name", "value"] } else { &["field", "value"] }
}

/// The handler methods of a Java source's controllers.
pub fn handlers(text: &str) -> Vec<Handler> {
    handlers_in(text, false)
}

/// The handler methods of a Java or Kotlin source's controllers.
pub fn handlers_in(text: &str, is_kotlin: bool) -> Vec<Handler> {
    let file = if is_kotlin { kotlin::parse(text) } else { java::parse(text) };
    let span = |at: usize, s: &str| range(text, at, at + s.len());
    let mut out = Vec::new();
    for c in file.types.iter().filter(|t| t.has(&["Controller", "RestController"])) {
        for m in c.methods() {
            for a in &m.annotations {
                let Some((op, ty)) = target(c, m, a) else { continue };
                let (field, at) = match a.strings(field_attrs(a)).first().filter(|s| !s.value.is_empty()) {
                    Some(s) => (s.value.clone(), range(text, s.at, s.end)),
                    None => (m.name.clone(), span(m.at, &m.name)),
                };
                let args = m
                    .params
                    .iter()
                    .filter_map(|p| {
                        let named = p.annotation("Argument")?.strings(&["name", "value"]).first().copied();
                        match named.filter(|s| !s.value.is_empty()) {
                            Some(s) => Some((s.value.clone(), range(text, s.at, s.end))),
                            // An unnamed `@Argument Map` may stand for all the arguments.
                            None if p.ty.name == "Map" => None,
                            None => Some((p.name.clone(), span(p.at, &p.name))),
                        }
                    })
                    .collect();
                out.push(Handler {
                    op: op.map(str::to_owned),
                    ty,
                    field,
                    range: at,
                    method: span(m.at, &m.name),
                    annotation: range(text, a.at, a.end),
                    label: format!("{}.{}()", c.name, m.name),
                    args,
                });
            }
        }
    }
    out
}

fn schema_file(path: &str, text: &str) -> Option<Schema> {
    if path.replace('\\', "/").contains("/node_modules/") {
        return None;
    }
    let schema = parse_schema(text);
    (!schema.types.is_empty() || !schema.roots.is_empty()).then_some(schema)
}

fn java_file(path: &str, text: &str) -> Option<Vec<Handler>> {
    if !text.contains("Mapping") {
        return None;
    }
    let found = handlers_in(text, path.ends_with(".kt"));
    (!found.is_empty()).then_some(found)
}

/// The project's root type for `op`.
fn root_type(schemas: &Index<Schema>, op: &str) -> String {
    schemas
        .iter()
        .flat_map(|(_, s)| &s.roots)
        .find(|(o, _)| o == op)
        .map(|(_, t)| t.clone())
        .or_else(|| ROOTS.iter().find(|r| r.1 == op).map(|r| r.2.to_owned()))
        .unwrap_or_default()
}

fn type_of(h: &Handler, schemas: &Index<Schema>) -> String {
    h.op.as_deref().map_or_else(|| h.ty.clone(), |op| root_type(schemas, op))
}

/// The schema's declarations of `ty.field`, files under `resources/graphql/` first.
fn schema_fields(schemas: &Index<Schema>, ty: &str, field: &str) -> Vec<(String, Field)> {
    let mut found: Vec<(String, Field)> = schemas
        .iter()
        .flat_map(|(path, s)| s.types.iter().filter(|t| t.name == ty).map(move |t| (path, t)))
        .flat_map(|(path, t)| t.fields.iter().filter(|f| f.name == field).map(move |f| (path.to_owned(), f.clone())))
        .collect();
    found.sort_by_key(|(path, _)| !path.replace('\\', "/").contains("/resources/graphql/"));
    found
}

fn describe(handlers: &[(Location, String)]) -> String {
    match handlers {
        [(_, one)] => format!("Handled by {one}"),
        many => format!("Handled by {}", many.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join(", ")),
    }
}

/// What the caret is in: a `typeName`, or a field name of the given (root operation, type).
enum Slot {
    TypeName,
    Field(Option<&'static str>, String),
}

fn annotation_slot(text: &str, at: usize, is_kotlin: bool) -> Option<(Slot, Str)> {
    let file = if is_kotlin { kotlin::parse(text) } else { java::parse(text) };
    for c in &file.types {
        for a in c.annotations.iter().filter(|a| a.name == "SchemaMapping") {
            if let Some(s) = a.strings(&["typeName"]).into_iter().find(|s| s.holds(at)) {
                return Some((Slot::TypeName, s.clone()));
            }
        }
        for m in c.methods() {
            for a in &m.annotations {
                for arg in &a.args {
                    let Some(s) = arg.strings.iter().find(|s| s.holds(at)) else { continue };
                    if arg.name == "typeName" && matches!(a.name.as_str(), "SchemaMapping" | "BatchMapping") {
                        return Some((Slot::TypeName, s.clone()));
                    }
                    if field_attrs(a).contains(&arg.name.as_str())
                        && let Some((op, ty)) = target(c, m, a)
                    {
                        return Some((Slot::Field(op, ty), s.clone()));
                    }
                }
            }
        }
    }
    None
}

pub struct GraphQlTools {
    schemas: Indexes<Schema>,
    java: Indexes<Vec<Handler>>,
}

impl Default for GraphQlTools {
    fn default() -> Self {
        Self { schemas: Indexes::new(SCHEMA_GLOBS, schema_file), java: Indexes::new(JAVA_GLOBS, java_file) }
    }
}

fn is_code(doc: &Document) -> bool {
    matches!(doc.language.as_str(), "java" | "kotlin")
}

/// A field selected in a client document, by byte offsets of the document text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocField {
    /// `query`, `mutation` or `subscription`; `None` in a fragment.
    pub op: Option<String>,
    /// The type a fragment is `on`.
    pub on: Option<String>,
    /// The fields enclosing this one, outermost first.
    pub path: Vec<String>,
    pub name: String,
    pub at: usize,
    pub end: usize,
}

fn skip_directives(toks: &[Tok], mut k: usize) -> usize {
    while toks.get(k).is_some_and(|t| t.1 == "@") {
        k += 2;
        if toks.get(k).is_some_and(|t| t.1 == "(") {
            k = close(toks, k) + 1;
        }
    }
    k
}

fn selection(toks: &[Tok], open: usize, scope: &DocField, out: &mut Vec<DocField>) {
    let end = close(toks, open).min(toks.len());
    let mut k = open + 1;
    while k < end {
        let (mut at, mut s) = toks[k];
        if s == "." {
            while toks.get(k).is_some_and(|t| t.1 == ".") {
                k += 1;
            }
            k += if toks.get(k).is_some_and(|t| t.1 == "on") { 2 } else { 1 };
            k = skip_directives(toks, k);
            if toks.get(k).is_some_and(|t| t.1 == "{") {
                k = close(toks, k);
            }
            k += 1;
            continue;
        }
        if !is_name(s) {
            k = if matches!(s, "(" | "[" | "{") { close(toks, k) + 1 } else { k + 1 };
            continue;
        }
        if toks.get(k + 1).is_some_and(|t| t.1 == ":") && toks.get(k + 2).is_some_and(|t| is_name(t.1)) {
            (at, s) = toks[k + 2];
            k += 2;
        }
        k += 1;
        if toks.get(k).is_some_and(|t| t.1 == "(") {
            k = close(toks, k) + 1;
        }
        k = skip_directives(toks, k);
        let field = DocField { path: scope.path.clone(), name: s.to_owned(), at, end: at + s.len(), ..scope.clone() };
        out.push(field.clone());
        if toks.get(k).is_some_and(|t| t.1 == "{") {
            let mut inner = field;
            inner.path.push(s.to_owned());
            selection(toks, k, &inner, out);
            k = close(toks, k) + 1;
        }
    }
}

/// The fields of the operations and fragments of a GraphQL executable document.
pub fn document_fields(text: &str) -> Vec<DocField> {
    let toks = tokens(text);
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let mut scope = DocField { op: None, on: None, path: Vec::new(), name: String::new(), at: 0, end: 0 };
        match toks[k].1 {
            "query" | "mutation" | "subscription" | "{" | "fragment" => {
                if toks[k].1 == "fragment" {
                    scope.on = toks.get(k + 3).map(|t| t.1.to_owned()).filter(|_| toks.get(k + 2).is_some_and(|t| t.1 == "on"));
                    k += 4;
                } else {
                    scope.op = Some(if toks[k].1 == "{" { "query" } else { toks[k].1 }.to_owned());
                    k += usize::from(toks[k].1 != "{");
                    if toks.get(k).is_some_and(|t| is_name(t.1)) {
                        k += 1;
                    }
                    if toks.get(k).is_some_and(|t| t.1 == "(") {
                        k = close(&toks, k) + 1;
                    }
                }
                k = skip_directives(&toks, k);
                if toks.get(k).is_some_and(|t| t.1 == "{") {
                    selection(&toks, k, &scope, &mut out);
                    k = close(&toks, k);
                }
                k += 1;
            }
            _ => k += 1,
        }
    }
    out
}

/// The client documents embedded in a Java or Kotlin source: `.document("…")` literals.
fn inline_documents(text: &str, is_kotlin: bool) -> Vec<DocField> {
    if !text.contains("document") {
        return Vec::new();
    }
    let calls = if is_kotlin { kotlin::calls(text) } else { java::calls(text) };
    let mut out = Vec::new();
    for call in calls.iter().filter(|c| c.name == "document" && !c.is_new) {
        let Some(s) = call.args.first().and_then(|a| a.string.as_ref()) else { continue };
        let raw = &text[s.at..s.end];
        if raw.contains('\\') {
            continue;
        }
        for mut f in document_fields(raw) {
            f.at += s.at;
            f.end += s.at;
            out.push(f);
        }
    }
    out
}

impl GraphQlTools {
    /// The schema fields (and the handlers of root fields) of the client document field at `at`.
    fn client_document_definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let fields = if is_code(doc) {
            inline_documents(&doc.text, doc.language == "kotlin")
        } else {
            document_fields(&doc.text)
        };
        let Some(hit) = fields.into_iter().find(|f| f.at <= at && at <= f.end) else { return Vec::new() };
        let schemas = self.schemas.get(&project, PROVIDER_BUDGET / 2);
        let mut ty = match (&hit.on, &hit.op) {
            (Some(on), _) => on.clone(),
            (None, Some(op)) => root_type(schemas, op),
            (None, None) => return Vec::new(),
        };
        for name in &hit.path {
            let Some((_, parent)) = schema_fields(schemas, &ty, name).into_iter().next() else { return Vec::new() };
            ty = parent.ty;
        }
        let mut found: Vec<Location> = schema_fields(schemas, &ty, &hit.name)
            .into_iter()
            .map(|(path, f)| Location { path, range: f.range })
            .collect();
        if hit.path.is_empty() && hit.op.is_some() {
            let java = self.java.get(&project, PROVIDER_BUDGET / 2);
            found.extend(
                java.iter()
                    .flat_map(|(path, hs)| hs.iter().map(move |h| (path, h)))
                    .filter(|(_, h)| h.field == hit.name && type_of(h, schemas) == ty)
                    .map(|(path, h)| Location { path: path.to_owned(), range: h.method }),
            );
        }
        found
    }

    /// The open Java file's handlers with the schema fields they map to; publishes its problems.
    fn java_links(&mut self, doc: &Document) -> Vec<(Handler, String, Vec<(String, Field)>)> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let schemas = self.schemas.get(&project, PROVIDER_BUDGET);
        let links: Vec<_> = handlers_in(&doc.text, doc.language == "kotlin")
            .into_iter()
            .map(|h| {
                let ty = type_of(&h, schemas);
                let found = schema_fields(schemas, &ty, &h.field);
                (h, ty, found)
            })
            .collect();
        let mut problems = Vec::new();
        let check_mappings = host::setting_bool("springGraphql.checkUnmatchedMapping").unwrap_or(true);
        for (h, ty, found) in &links {
            if found.is_empty() {
                if !check_mappings {
                    continue;
                }
                let known_type = schemas.iter().any(|(_, s)| s.types.iter().any(|t| &t.name == ty));
                if schemas.complete() && (known_type || h.op.is_some() && !schemas.files().is_empty()) {
                    let message = format!("The GraphQL schema declares no field `{}` on type `{ty}`.", h.field);
                    problems.push(Diagnostic::new(h.range, Severity::Warning, message).code("unknown-schema-field"));
                }
                continue;
            }
            for (name, at) in &h.args {
                if !found.iter().any(|(_, f)| f.args.contains(name)) {
                    let message = format!("`{ty}.{}` has no argument `{name}`.", h.field);
                    problems.push(Diagnostic::new(*at, Severity::Warning, message).code("unknown-graphql-argument"));
                }
            }
        }
        if host::setting_bool("springGraphql.checkBatchMappingReturn").unwrap_or(true) {
            let file = if doc.language == "kotlin" { kotlin::parse(&doc.text) } else { java::parse(&doc.text) };
            problems.extend(checks::batch_mapping_return(&file, &doc.text).iter().map(|f| f.diagnostic(&doc.text)));
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
        links
    }

    /// The open schema file's fields (with their type) and the handlers serving them; publishes its hints.
    fn schema_links(&mut self, doc: &Document) -> Vec<(String, Field, Vec<(Location, String)>)> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let Some(schemas) = self.schemas.for_document(&doc.path, &doc.text, PROVIDER_BUDGET / 2) else {
            return Vec::new();
        };
        let schemas: &Index<Schema> = schemas;
        let java = self.java.get(&project, PROVIDER_BUDGET / 2);
        let roots: Vec<String> = ROOTS.iter().map(|r| root_type(schemas, r.1)).collect();
        let mut problems = Vec::new();
        let mut links = Vec::new();
        for t in parse_schema(&doc.text).types {
            for f in t.fields {
                let found: Vec<(Location, String)> = java
                    .iter()
                    .flat_map(|(path, hs)| hs.iter().map(move |h| (path, h)))
                    .filter(|(_, h)| h.field == f.name && type_of(h, schemas) == t.name)
                    .map(|(path, h)| (Location { path: path.to_owned(), range: h.method }, h.label.clone()))
                    .collect();
                let unmapped_root = found.is_empty() && t.kind == "type" && roots.contains(&t.name);
                if unmapped_root && java.complete() && !java.files().is_empty() {
                    let message = format!("No controller method handles `{}.{}`.", t.name, f.name);
                    problems.push(Diagnostic::new(f.range, Severity::Hint, message).code("unmapped-root-field"));
                }
                links.push((t.name.clone(), f, found));
            }
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
        links
    }
}

impl Providers for GraphQlTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let Some((slot, s)) = annotation_slot(&doc.text, at, doc.language == "kotlin") else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let schemas = self.schemas.get(&project, PROVIDER_BUDGET);
        let types = schemas.iter().flat_map(|(_, s)| &s.types);
        // Name → detail.
        let (names, kind): (BTreeMap<String, String>, _) = match slot {
            Slot::TypeName => (
                types.filter(|t| t.kind != "input").map(|t| (t.name.clone(), t.kind.clone())).collect(),
                CompletionKind::Class,
            ),
            Slot::Field(op, ty) => {
                let ty = op.map_or(ty, |op| root_type(schemas, op));
                let fields = types.filter(|t| t.name == ty).flat_map(|t| &t.fields);
                (fields.map(|f| (f.name.clone(), ty.clone())).collect(), CompletionKind::Field)
            }
        };
        let replace = range(&doc.text, s.at, s.end);
        names
            .into_iter()
            .map(|(name, detail)| CompletionItem {
                detail: Some(detail),
                replace: Some(replace),
                ..CompletionItem::new(&name, kind)
            })
            .collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if is_code(doc) {
            return self
                .java_links(doc)
                .into_iter()
                .filter_map(|(h, ty, found)| {
                    let (path, f) = found.into_iter().next()?;
                    Some(LineMarker {
                        line: h.method.start.line,
                        icon: None,
                        tooltip: format!("GraphQL schema field {ty}.{}", h.field),
                        target: Some(Location { path, range: f.range }),
                    })
                })
                .collect();
        }
        self.schema_links(doc)
            .into_iter()
            .filter_map(|(_, f, found)| {
                Some(LineMarker {
                    line: f.range.start.line,
                    icon: None,
                    tooltip: describe(&found),
                    target: Some(found.first()?.0.clone()),
                })
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if is_code(doc) {
            let mut found: Vec<Location> = self
                .java_links(doc)
                .into_iter()
                .filter(|(h, _, _)| contains(h.annotation, at) || contains(h.method, at) || contains(h.range, at))
                .flat_map(|(_, _, found)| found.into_iter().map(|(path, f)| Location { path, range: f.range }))
                .collect();
            found.extend(self.client_document_definition(doc, at));
            return found;
        }
        if doc.path.replace('\\', "/").contains(DOCUMENT_DIR) {
            return self.client_document_definition(doc, at);
        }
        self.schema_links(doc)
            .into_iter()
            .filter(|(_, f, _)| contains(f.range, at))
            .flat_map(|(_, _, found)| found.into_iter().map(|(l, _)| l))
            .collect()
    }
}

impl Plugin for GraphQlTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(GraphQlTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const SCHEMA: &str = r#"# The library schema
schema { query: Query mutation: Mutation }

"""A book, with a { brace in its description"""
type Book implements Node @key(fields: "id") {
  id: ID!
  "The title"
  title(locale: String = "en", @deprecated upper: Boolean): String
  author: Author @deprecated(reason: "use authors")
}

type Query {
  bookById(id: ID!): Book
  books(filter: BookFilter = {title: "x", year: 1}): [Book!]!
  stats: Int
}

extend type Query { authors: [Author] }

input BookFilter { title: String year: Int }
enum Genre { NOVEL POETRY }
type Mutation { addBook(input: BookFilter!): Book }

query Client { books { title } }
"#;

    const CONTROLLER: &str = r#"package com.acme;

@Controller
@SchemaMapping(typeName = "Book")
class BookController {
    @QueryMapping
    public Book bookById(@Argument String id) { return null; }

    @QueryMapping(name = "books")
    public List<Book> all(@Argument("filter") Filter f, @Argument String sort) { return null; }

    @SchemaMapping
    public Author author(Book book) { return null; }

    @SchemaMapping(typeName = "Author", field = "")
    public List<Book> books(Author author, DataFetchingEnvironment env) { return null; }

    @BatchMapping
    public Map<Book, String> title(List<Book> books) { return null; }

    @MutationMapping
    public Book removeBook(@Argument String id) { return null; }

    @SchemaMapping
    public String isbn(Book book) { return null; }
}
"#;

    #[test]
    fn parses_sdl() {
        let s = parse_schema(SCHEMA);
        assert_eq!(s.roots, [("query".to_owned(), "Query".to_owned()), ("mutation".into(), "Mutation".into())]);
        let types: Vec<_> = s
            .types
            .iter()
            .map(|t| (t.kind.as_str(), t.name.as_str(), t.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()))
            .collect();
        assert_eq!(
            types,
            [
                ("type", "Book", vec!["id", "title", "author"]),
                ("type", "Query", vec!["bookById", "books", "stats"]),
                ("type", "Query", vec!["authors"]),
                ("input", "BookFilter", vec!["title", "year"]),
                ("type", "Mutation", vec!["addBook"]),
            ]
        );
        assert_eq!(s.types[0].fields[1].args, ["locale", "upper"]);
        assert_eq!(s.types[1].fields[1].args, ["filter"]);
        let title = s.types[0].fields[1].range;
        assert_eq!(title.start, position(SCHEMA, SCHEMA.find("title(").unwrap()));
        for end in (0..=SCHEMA.len()).filter(|&e| SCHEMA.is_char_boundary(e)) {
            parse_schema(&SCHEMA[..end]);
        }
    }

    #[test]
    fn finds_handlers_and_their_types() {
        let found: Vec<_> = handlers(CONTROLLER)
            .into_iter()
            .map(|h| (h.op.clone(), h.ty.clone(), h.field.clone(), h.args.len()))
            .collect();
        let root = |op: &str| Some(op.to_owned());
        assert_eq!(
            found,
            [
                (root("query"), "Query".to_owned(), "bookById".to_owned(), 1),
                (root("query"), "Query".into(), "books".into(), 2),
                (None, "Book".into(), "author".into(), 0),
                (None, "Author".into(), "books".into(), 0),
                (None, "Book".into(), "title".into(), 0),
                (root("mutation"), "Mutation".into(), "removeBook".into(), 1),
                (None, "Book".into(), "isbn".into(), 0),
            ]
        );
        let plain = handlers(&CONTROLLER.replace("@SchemaMapping(typeName = \"Book\")\n", ""));
        assert_eq!(plain[2].ty, "Book", "the source parameter's type");
        assert_eq!(plain[4].ty, "Book", "the batch key type");
        assert!(handlers(&CONTROLLER.replace("@Controller", "@Service")).is_empty());
    }

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        host.add_file("src/main/java/com/acme/BookController.java", CONTROLLER);
        host.add_file("web/node_modules/lib/schema.graphql", "type Query { books: Int }");
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: format!("/project/{path}"), language: language.into(), version: 1, text: text.into() }
    }

    fn java() -> Document {
        doc("src/main/java/com/acme/BookController.java", "java", CONTROLLER)
    }

    fn graphql() -> Document {
        doc("src/main/resources/graphql/schema.graphqls", "graphql", SCHEMA)
    }

    fn start(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap())
    }

    fn at(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + 1)
    }

    #[test]
    fn links_handlers_to_schema_fields() {
        let host = setup();
        let mut tools = GraphQlTools::default();
        let markers = tools.line_markers(&java());
        let tips: Vec<_> = markers.iter().map(|m| m.tooltip.as_str()).collect();
        assert_eq!(
            tips,
            [
                "GraphQL schema field Query.bookById",
                "GraphQL schema field Query.books",
                "GraphQL schema field Book.author",
                "GraphQL schema field Book.title",
            ]
        );
        let target = markers[1].target.as_ref().unwrap();
        assert!(target.path.ends_with("resources/graphql/schema.graphqls"), "node_modules is skipped");
        assert_eq!(target.range.start, start(SCHEMA, "books("));

        let found = tools.definition(&java(), at(CONTROLLER, "\"books\""));
        assert_eq!(found.len(), 1);
        assert!(tools.definition(&java(), at(CONTROLLER, "Book book")).is_empty());

        let problems = &host.diagnostics()["/project/src/main/java/com/acme/BookController.java"];
        let problems: Vec<_> = problems
            .iter()
            .map(|d| {
                (
                    d.code.as_deref().unwrap(),
                    &CONTROLLER[offset(CONTROLLER, d.range.start).unwrap()..offset(CONTROLLER, d.range.end).unwrap()],
                )
            })
            .collect();
        assert_eq!(
            problems,
            [
                ("unknown-graphql-argument", "sort"),
                ("unknown-schema-field", "removeBook"),
                ("unknown-schema-field", "isbn"),
            ],
            "Author is not in the schema: no warning for Author.books"
        );
    }

    fn problems_of(host: &MockHost, text: &str) -> Vec<String> {
        let mut tools = GraphQlTools::default();
        tools.line_markers(&doc("src/main/java/com/acme/C.java", "java", text));
        host.diagnostics()
            .get("/project/src/main/java/com/acme/C.java")
            .into_iter()
            .flatten()
            .filter(|d| d.code.as_deref() == Some("unknown-schema-field"))
            .map(|d| d.message.clone())
            .collect()
    }

    const BATCH: &str = "@Controller\nclass C {\n    @BatchMapping\n    public void titles(List<Book> books) {}\n}\n";

    fn batch_codes(host: &MockHost, text: &str) -> Vec<String> {
        problems_of(host, text);
        host.diagnostics()["/project/src/main/java/com/acme/C.java"].iter().filter_map(|d| d.code.clone()).filter(|c| c.starts_with("batch")).collect()
    }

    #[test]
    fn batch_mapping_group_can_be_switched_off() {
        let host = setup();
        assert_eq!(batch_codes(&host, BATCH), ["batch-mapping-return-type"]);
        host.set_setting("springGraphql.checkBatchMappingReturn", "false");
        assert!(batch_codes(&host, BATCH).is_empty());
    }

    const MAPPED: &str = "@Controller\nclass C {\n    @QueryMapping\n    public Book nope() { return null; }\n}\n";

    #[test]
    fn unknown_schema_field_warns() {
        let host = setup();
        let messages = problems_of(&host, MAPPED);
        assert_eq!(messages, ["The GraphQL schema declares no field `nope` on type `Query`."]);
    }

    #[test]
    fn unknown_schema_field_is_quiet_for_library_or_dynamic_name() {
        let host = setup();
        let extended = "@Controller\nclass C {\n    @QueryMapping\n    public List<Author> authors() { return null; }\n}\n";
        assert!(problems_of(&host, extended).is_empty(), "extend type Query counts");
        let library = "@Controller\nclass C {\n    @SchemaMapping(typeName = \"PageInfo\", field = \"x\")\n    public String x() { return null; }\n}\n";
        assert!(problems_of(&host, library).is_empty(), "type from a library schema");
        let empty = MockHost::install();
        empty.add_file("src/main/java/com/acme/C.java", MAPPED);
        assert!(problems_of(&empty, MAPPED).is_empty(), "no schema file");
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = setup();
        assert_eq!(problems_of(&host, MAPPED).len(), 1);
        let fixed = MAPPED.replace("nope", "stats");
        assert!(problems_of(&host, &fixed).is_empty());
        assert!(host.diagnostics().get("/project/src/main/java/com/acme/C.java").is_none_or(Vec::is_empty));
    }

    #[test]
    fn links_schema_fields_to_handlers() {
        let host = setup();
        let mut tools = GraphQlTools::default();
        let markers = tools.line_markers(&graphql());
        let lines: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(
            lines,
            [
                (at(SCHEMA, "title(").line, "Handled by BookController.title()"),
                (at(SCHEMA, "author:").line, "Handled by BookController.author()"),
                (at(SCHEMA, "bookById").line, "Handled by BookController.bookById()"),
                (at(SCHEMA, "books(").line, "Handled by BookController.all()"),
            ]
        );
        let found = tools.definition(&graphql(), at(SCHEMA, "books("));
        assert_eq!(found[0].range.start, start(CONTROLLER, "all("));

        let hints: Vec<_> = host.diagnostics()["/project/src/main/resources/graphql/schema.graphqls"]
            .iter()
            .map(|d| (d.severity, d.message.clone()))
            .collect();
        assert_eq!(
            hints,
            [
                (Severity::Hint, "No controller method handles `Query.stats`.".to_owned()),
                (Severity::Hint, "No controller method handles `Query.authors`.".into()),
                (Severity::Hint, "No controller method handles `Mutation.addBook`.".into()),
            ]
        );
    }

    /// Completion labels at the `|` of `text`.
    fn complete(tools: &mut GraphQlTools, text: &str) -> Vec<String> {
        let at = position(text, text.find('|').unwrap());
        let d = doc("src/main/java/com/acme/BookController.java", "java", &text.replace('|', ""));
        tools.completion(&d, at, None).into_iter().map(|c| c.label).collect()
    }

    #[test]
    fn completes_types_and_fields_in_annotations() {
        let _host = setup();
        let mut tools = GraphQlTools::default();
        let types = ["Book", "Mutation", "Query"];
        assert_eq!(complete(&mut tools, &CONTROLLER.replace("typeName = \"Author\"", "typeName = \"|\"")), types);
        assert_eq!(complete(&mut tools, &CONTROLLER.replace("(typeName = \"Book\")", "(typeName = \"B|\")")), types);
        let text = CONTROLLER.replace("@QueryMapping(name = \"books\")", "@QueryMapping(name = \"bo|\")");
        assert_eq!(complete(&mut tools, &text), ["authors", "bookById", "books", "stats"]);
        let text = CONTROLLER.replace("@BatchMapping", "@BatchMapping(field = \"|\")");
        assert_eq!(complete(&mut tools, &text), ["author", "id", "title"]);
        assert!(
            complete(&mut tools, &CONTROLLER.replace("@Argument String sort", "@Argument String so|rt")).is_empty()
        );
    }

    const DOCUMENT: &str = "query Find($id: ID!) { bookById(id: $id) { title author { name } } }\nmutation { addBook(input: {}) { id } }";

    #[test]
    fn client_document_field_links_to_schema() {
        let _host = setup();
        let mut tools = GraphQlTools::default();
        let file = doc("src/main/resources/graphql-documents/find.graphql", "graphql", DOCUMENT);
        let found = tools.definition(&file, at(DOCUMENT, "bookById"));
        assert_eq!(found.len(), 2, "the schema field and its controller method");
        assert_eq!(found[0].range.start, start(SCHEMA, "bookById"));
        assert_eq!(found[1].range.start, start(CONTROLLER, "bookById("));
        let nested = tools.definition(&file, at(DOCUMENT, "title"));
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].range.start, start(SCHEMA, "title("));
        assert!(tools.definition(&file, at(DOCUMENT, "name")).is_empty(), "Author is not in the schema");
        assert_eq!(tools.definition(&file, at(DOCUMENT, "addBook")).len(), 1);
        assert!(tools.definition(&file, at(DOCUMENT, "query Find")).is_empty());

        let code = "class C {\n    void f() {\n        client.document(\"query { books { title } }\").retrieve(\"books\");\n        client.document(\"\"\"\n            { bookById(id: 1) { title } }\n            \"\"\").execute();\n    }\n}\n";
        let java = doc("src/main/java/com/acme/C.java", "java", code);
        assert_eq!(tools.definition(&java, at(code, "books {")).len(), 2);
        assert_eq!(tools.definition(&java, at(code, "title } }")).len(), 1);
        assert_eq!(tools.definition(&java, at(code, "bookById")).len(), 2);
    }

    const KOTLIN: &str = r#"package com.acme

@Controller
@SchemaMapping(typeName = "Book")
class BookController {
    @QueryMapping
    suspend fun bookById(@Argument id: String): Book? = null

    @QueryMapping("books")
    fun all(@Argument("filter") f: Filter, @Argument sort: String): List<Book> {
        return emptyList()
    }

    @SchemaMapping
    fun author(book: Book): Author = Author()

    @BatchMapping
    fun title(books: List<Book>): Map<Book, String> = emptyMap()
}
"#;

    #[test]
    fn kotlin_query_mapping_links_to_schema() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        host.add_file("src/main/kotlin/com/acme/BookController.kt", KOTLIN);
        let mut tools = GraphQlTools::default();
        let kt = doc("src/main/kotlin/com/acme/BookController.kt", "kotlin", KOTLIN);
        let tips: Vec<_> = tools.line_markers(&kt).into_iter().map(|m| m.tooltip).collect();
        assert_eq!(
            tips,
            [
                "GraphQL schema field Query.bookById",
                "GraphQL schema field Query.books",
                "GraphQL schema field Book.author",
                "GraphQL schema field Book.title",
            ]
        );
        assert_eq!(tools.definition(&kt, at(KOTLIN, "bookById")).len(), 1);
        let back = tools.line_markers(&graphql());
        let target = back.iter().find(|m| m.tooltip.contains("bookById")).unwrap();
        assert_eq!(target.tooltip, "Handled by BookController.bookById()");
        assert!(target.target.as_ref().unwrap().path.ends_with("BookController.kt"));
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/BookController.kt"];
        assert_eq!(problems.iter().filter_map(|d| d.code.as_deref()).collect::<Vec<_>>(), ["unknown-graphql-argument"]);
    }

    const DOC_SCHEMA: &str = "schema { query: RootQuery subscription: Subscription }\ntype RootQuery { bookById(id: ID): Book other: Book }\ntype Subscription { bookAdded: Book }\ntype Mutation { addBook: Book }\ntype Book { id: ID title: String }";

    fn doc_links(path: &str, language: &str, text: &str, needle: &str) -> Vec<Location> {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", DOC_SCHEMA);
        GraphQlTools::default().definition(&doc(path, language, text), at(text, needle))
    }

    fn quiet_links(text: &str, needle: &str) -> Vec<Location> {
        let path = "src/main/resources/graphql-documents/d.graphql";
        GraphQlTools::default().definition(&doc(path, "graphql", text), at(text, needle))
    }

    fn graphql_doc(text: &str, needle: &str) -> Vec<Location> {
        doc_links("src/main/resources/graphql-documents/d.graphql", "graphql", text, needle)
    }

    #[test]
    fn document_alias_links_the_real_field() {
        let text = "{ other: bookById(id: 1) { title } }";
        assert!(graphql_doc(text, "other").is_empty(), "an alias is not a schema field");
        let found = graphql_doc(text, "bookById");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start, start(DOC_SCHEMA, "bookById"));
        assert_eq!(graphql_doc(text, "title").len(), 1);
    }

    #[test]
    fn document_fragments_and_spreads() {
        let text = "fragment B on Book { title }\nquery { bookById(id: 1) { ...B ... on Book { id } } }";
        assert_eq!(graphql_doc(text, "title").len(), 1, "resolved against Book");
        assert!(graphql_doc(text, "B on").is_empty());
        let spread = "fragment title on Book { title }\nquery { bookById(id: 1) { ...title } }";
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", DOC_SCHEMA);
        let last = position(spread, spread.rfind("title").unwrap() + 1);
        let path = "src/main/resources/graphql-documents/d.graphql";
        assert!(GraphQlTools::default().definition(&doc(path, "graphql", spread), last).is_empty());
        assert!(graphql_doc(text, "B ...").is_empty());
        assert!(graphql_doc(text, "id }").is_empty(), "inline fragments are skipped");
        assert_eq!(graphql_doc(text, "bookById").len(), 1);
        let on_query = "fragment Q on RootQuery { bookById(id: 1) { id } }";
        assert_eq!(graphql_doc(on_query, "bookById").len(), 1);
        assert_eq!(graphql_doc(on_query, "id }").len(), 1);
    }

    #[test]
    fn document_subscription_and_mutation_roots() {
        let found = graphql_doc("subscription { bookAdded { id } }", "bookAdded");
        assert_eq!(found[0].range.start, start(DOC_SCHEMA, "bookAdded"));
        assert_eq!(graphql_doc("subscription OnAdd { bookAdded { id } }", "id").len(), 1);
        assert_eq!(graphql_doc("mutation { addBook { id } }", "addBook").len(), 1);
        let renamed = graphql_doc("query Q($i: ID) { bookById(id: $i) { id } }", "bookById");
        assert_eq!(renamed.len(), 1, "schema query rename");
    }

    #[test]
    fn document_unknown_root_field_is_quiet() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", DOC_SCHEMA);
        assert!(quiet_links("{ nope }", "nope").is_empty());
        assert!(quiet_links("{ bookById { nope } }", "nope").is_empty());
        assert_eq!(quiet_links("{ bookById { id } }", "bookById").len(), 1);
        assert!(host.diagnostics().values().all(Vec::is_empty));
        let text = "# bookById é\nquery { bookById(id: \"é\", x: [1 {\n# end\n";
        let mut tools = GraphQlTools::default();
        for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
            let d = doc("src/main/resources/graphql-documents/d.graphql", "graphql", &text[..end]);
            tools.definition(&d, position(&text[..end], end));
        }
        let java = "class C { void f() { client.document(\"query { bookById(\"); } }";
        for end in (0..=java.len()).filter(|&e| java.is_char_boundary(e)) {
            let d = doc("src/main/java/C.java", "java", &java[..end]);
            tools.definition(&d, position(&java[..end], end));
        }
    }

    #[test]
    fn kotlin_inline_document_links_to_schema() {
        let text = "class C {\n    fun f() {\n        client.document(\"\"\"query { bookById(id: 1) { title } }\"\"\").retrieve(\"x\")\n        foo.document(\"{ nope }\")\n    }\n}\n";
        let found = doc_links("src/main/kotlin/C.kt", "kotlin", text, "bookById");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start, start(DOC_SCHEMA, "bookById"));
        assert_eq!(doc_links("src/main/kotlin/C.kt", "kotlin", text, "title").len(), 1);
        assert!(doc_links("src/main/kotlin/C.kt", "kotlin", text, "nope").is_empty());
        let escaped = "class C { void f() { client.document(\"{ bookById(name: \\\"x\\\") { id } }\"); } }";
        assert!(doc_links("src/main/java/C.java", "java", escaped, "bookById").is_empty(), "escapes are skipped");
    }

    #[test]
    fn gql_documents_link_to_schema() {
        let text = "query { bookById(id: 1) { id } }";
        let found = doc_links("src/main/resources/graphql-documents/find.gql", "graphql", text, "bookById");
        assert_eq!(found.len(), 1);
        assert!(doc_links("src/main/resources/other/find.gql", "graphql", text, "bookById").is_empty());
    }
}
