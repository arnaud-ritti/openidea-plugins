//! Spring for GraphQL Tools for OpenIDEA: indexes the project's GraphQL schema
//! files and the `@…Mapping` handler methods of its controllers, and links
//! them both ways. Text-based: types and fields match by name, as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Project, Range, Severity, host};
#[cfg(test)]
use spring_text::index::COMMAND_BUDGET;
use spring_text::index::{Index, Indexes, project_of};
use spring_text::java::{self, Annotation, Member, Str, TypeDecl};
use spring_text::kotlin;
use spring_text::tree::Expansion;
use spring_text::{config, contains, range};
use std::collections::BTreeMap;
use std::time::Duration;

mod checks;
mod mapping_docs;
mod wiring;

const SCHEMA_GLOBS: &[&str] = &["**/*.graphqls", "**/*.graphql"];
const JAVA_GLOBS: &[&str] = &["**/*.java", "**/*.kt"];
const CONFIG_GLOBS: &[&str] = &["**/application*.properties", "**/application*.yml", "**/application*.yaml"];
const DOCUMENT_DIR: &str = "/graphql-documents/";
/// (annotation, operation, default root type).
const ROOTS: [(&str, &str, &str); 3] = [
    ("QueryMapping", "query", "Query"),
    ("MutationMapping", "mutation", "Mutation"),
    ("SubscriptionMapping", "subscription", "Subscription"),
];
/// Handler parameters bound from the arguments or the environment, not the source object.
const NOT_SOURCE_ANNOTATIONS: [&str; 5] = ["Argument", "Arguments", "ContextValue", "LocalContextValue", "AuthenticationPrincipal"];
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
    /// Exact SDL argument type references, including list/non-null wrappers.
    pub argument_types: Vec<(String,String)>,
    pub type_reference: String,
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
    /// A schema definition (not merely an extension) disables implicit operation roots.
    pub explicit_definition:bool,
    /// Explicit custom scalar/enum declarations are leaf values.
    pub leaf_types: Vec<String>,
    pub union_types: Vec<String>,
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

fn read_type(toks:&[Tok],from:usize)->Option<(String,usize)>{
    let mut end=from;let mut lists=0;
    while toks.get(end).is_some_and(|t|t.1=="["){lists+=1;if lists>64{return None}end+=1;}
    if !is_name(toks.get(end)?.1){return None}end+=1;
    if toks.get(end).is_some_and(|t|t.1=="!"){end+=1;}
    for _ in 0..lists {if toks.get(end)?.1!="]"{return None}end+=1;if toks.get(end).is_some_and(|t|t.1=="!"){end+=1;}}
    if toks.get(end).is_some_and(|t|matches!(t.1,"!"|"["|"]")){return None}
    Some((toks[from..end].iter().map(|t|t.1).collect::<String>(),end))
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
                let mut argument_types=Vec::new();
                k += 1;
                if toks[k].1 == "(" {
                    let c = close(toks, k);
                    let parameters=fields(text, &toks[k + 1..c]);
                    argument_types=parameters.iter().map(|f|(f.name.clone(),f.type_reference.clone())).collect();
                    args=parameters.into_iter().map(|f|f.name).collect();
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
                let type_reference=read_type(toks,k+1).map(|(ty,_)|ty).unwrap_or_default();
                out.push(Field { name: s.to_owned(), range: range(text, at, at + s.len()), args, argument_types, type_reference, ty: ty.to_owned() });
            }
            _ => k += 1,
        }
    }
    out
}

/// The object, interface and input types of an SDL text and its `schema` block.
/// Operations/fragments are skipped; scalar/enum leaves and union shapes are retained.
pub fn parse_schema(text: &str) -> Schema {
    let toks = tokens(text);
    let mut out = Schema::default();
    let mut k = 0;
    while k < toks.len() {
        let s = toks[k].1;
        match s {
            "scalar" | "enum" | "union" => {
                if let Some((_,name))=toks.get(k+1).filter(|(_,name)|is_name(name)) {
                    if s=="union" {out.union_types.push((*name).into())} else {out.leaf_types.push((*name).into())}
                }
                if s=="enum" {
                    let mut body=k+2;
                    while body<toks.len()&&toks[body].1!="{"&&!DEFINITIONS.contains(&toks[body].1){body+=1}
                    k=if toks.get(body).is_some_and(|t|t.1=="{"){close(&toks,body)+1}else{k+2};
                } else {k+=2;}
            }
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
                    out.explicit_definition |= k==0||toks[k-1].1!="extend";
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
    /// Shortcut annotation operation; its parent remains the fixed annotation type.
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
        "BatchMapping" => named(a).or_else(class_level).or_else(|| Some(m.params.first()?.ty.args.first()?.name.clone())),
        _ => None,
    }?;
    Some((None, ty))
}

/// The annotation attributes naming the field.
fn field_attrs(a: &Annotation) -> &'static [&'static str] {
    if ROOTS.iter().any(|r| r.0 == a.name) { &["name", "value"] } else { &["field", "value"] }
}

fn literal_annotation(a: &Annotation, allowed: &[&str], aliases: &[&str]) -> bool {
    a.args.iter().filter(|arg| !(a.name=="BatchMapping"&&arg.name=="maxBatchSize")).all(|arg| allowed.contains(&arg.name.as_str()) && arg.strings.len()==1 && java::tokens(&arg.text).len()==1 && !arg.text.contains(['\\','$']) && arg.text.trim().starts_with('"') && !arg.text.trim().starts_with("\"\"\""))
        && a.args.iter().filter(|arg|aliases.contains(&arg.name.as_str())).count()<=1
        && a.args.iter().enumerate().all(|(i,arg)|!a.args[..i].iter().any(|before|before.name==arg.name))
}
const ALIASABLE: [(&str, &str); 8] = [
    ("org.springframework.stereotype", "Controller"),
    ("org.springframework.web.bind.annotation", "RestController"),
    ("org.springframework.graphql.data.method.annotation", "SchemaMapping"),
    ("org.springframework.graphql.data.method.annotation", "QueryMapping"),
    ("org.springframework.graphql.data.method.annotation", "MutationMapping"),
    ("org.springframework.graphql.data.method.annotation", "SubscriptionMapping"),
    ("org.springframework.graphql.data.method.annotation", "BatchMapping"),
    ("org.springframework.graphql.data.method.annotation", "Argument"),
];

fn import_alias<'a>(text: &'a str, i: &java::Import) -> Option<&'a str> {
    let tail = text.get(i.end..).unwrap_or("").split('\n').next().unwrap_or("");
    match java::tokens(tail).as_slice() {
        [a, name, ..] if a.s == "as" => Some(name.s),
        _ => None,
    }
}

fn is_exact_alias_import(text: &str, i: &java::Import, simple: &str, package: &str) -> bool {
    !i.is_static && i.path == format!("{package}.{simple}") && import_alias(text, i).is_some_and(|alias| alias != simple)
}

/// Kotlin `import <spring annotation> as Alias`: the alias is renamed to the real annotation and literal uses of the hidden real name are made unrecognisable.
fn canonicalise_kotlin_aliases(file: &mut java::JavaFile, text: &str) {
    let mut renames: Vec<(String, &'static str)> = Vec::new();
    for (package, simple) in ALIASABLE {
        let aliases: Vec<&str> = file
            .imports
            .iter()
            .filter(|i| is_exact_alias_import(text, i, simple, package))
            .filter_map(|i| import_alias(text, i))
            .collect();
        let [alias] = aliases.as_slice() else { continue };
        let taken = file.imports.iter().filter(|i| !i.is_static && (import_alias(text, i) == Some(*alias) || i.path.rsplit('.').next() == Some(*alias) && import_alias(text, i).is_none())).count() != 1
            || file.types.iter().any(|t| t.name == *alias)
            || java::tokens(text).windows(2).any(|p| p[0].s == "typealias" && p[1].s == *alias);
        if taken {
            continue;
        }
        renames.push((alias.to_string(), simple));
    }
    if renames.is_empty() {
        return;
    }
    let rename = |a: &mut java::Annotation| {
        if let Some((_, real)) = renames.iter().find(|(alias, _)| *alias == a.name) {
            a.name = (*real).to_owned();
        } else if renames.iter().any(|(_, real)| *real == a.name) {
            a.name = format!("!{}", a.name);
        }
    };
    for t in &mut file.types {
        t.annotations.iter_mut().for_each(rename);
        for m in &mut t.members {
            m.annotations.iter_mut().for_each(rename);
            for p in &mut m.params {
                p.annotations.iter_mut().for_each(rename);
            }
        }
    }
}

fn owned_annotation(file:&java::JavaFile,text:&str,simple:&str,package:&str)->bool {
    !file.types.iter().any(|t|t.name==simple) && !java::tokens(text).windows(2).any(|p|p[0].s=="typealias"&&p[1].s==simple) && !file.imports.iter().any(|i|{
        let tail=text.get(i.end..).unwrap_or("").split('\n').next().unwrap_or("");let tokens=java::tokens(tail);
        let alias=match tokens.as_slice(){[a,name,..] if a.s=="as"=>Some(name.s),_=>None};
        !i.is_static && !is_exact_alias_import(text,i,simple,package) && (alias==Some(simple) || i.path.rsplit('.').next()==Some(simple) && (i.path!=format!("{package}.{simple}")||alias.is_some()))
    })
}

/// The handler methods of a Java source's controllers.
pub fn handlers(text: &str) -> Vec<Handler> {
    handlers_in(text, false)
}

/// The handler methods of a Java or Kotlin source's controllers and its literal `RuntimeWiring` registrations.
pub fn handlers_in(text: &str, is_kotlin: bool) -> Vec<Handler> {
    let mut out = controller_handlers(text, is_kotlin);
    out.extend(wiring::wired(text).handlers);
    out
}

fn controller_handlers(text: &str, is_kotlin: bool) -> Vec<Handler> {
    let mut file = if is_kotlin { kotlin::parse(text) } else { java::parse(text) };
    if is_kotlin {
        canonicalise_kotlin_aliases(&mut file, text);
    }
    let span = |at: usize, s: &str| range(text, at, at + s.len());
    let mut out = Vec::new();
    for c in file.types.iter().filter(|t| t.annotations.iter().any(|a| matches!(a.name.as_str(),"Controller"|"RestController") && owned_annotation(&file,text,&a.name,if a.name=="Controller"{"org.springframework.stereotype"}else{"org.springframework.web.bind.annotation"}))) {
        if c.annotation("SchemaMapping").is_some_and(|a| !owned_annotation(&file,text,"SchemaMapping","org.springframework.graphql.data.method.annotation") || !literal_annotation(a,&["typeName","field","value"],&["field","value"])) {continue}
        for m in c.methods() {
            for a in &m.annotations {
                if !owned_annotation(&file,text,&a.name,"org.springframework.graphql.data.method.annotation") || !literal_annotation(a,if ROOTS.iter().any(|r|r.0==a.name){&["name","value"]}else{&["typeName","field","value"]},field_attrs(a)) {continue}
                let Some((op, ty)) = target(c, m, a) else { continue };
                let (field, at) = match a.strings(field_attrs(a)).first().filter(|s| !s.value.is_empty()) {
                    Some(s) => (s.value.clone(), range(text, s.at, s.end)),
                    None => (m.name.clone(), span(m.at, &m.name)),
                };
                let gql_name=|s:&str| !s.is_empty() && s.len()<=128 && s.bytes().enumerate().all(|(i,b)|b.is_ascii_alphabetic()||b==b'_'||i>0&&b.is_ascii_digit());
                if !gql_name(&ty)||!gql_name(&field){continue}
                let args = m
                    .params
                    .iter()
                    .filter_map(|p| {
                        let annotation=p.annotation("Argument")?;
                        if !owned_annotation(&file,text,"Argument","org.springframework.graphql.data.method.annotation") || !literal_annotation(annotation,&["name","value"],&["name","value"]) {return None}
                        let named = annotation.strings(&["name", "value"]).first().copied();
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
    if path.replace('\\', "/").contains("/node_modules/") || path.replace('\\', "/").contains(DOCUMENT_DIR) {
        return None;
    }
    let schema = parse_schema(text);
    (!schema.types.is_empty() || schema.explicit_definition || !schema.roots.is_empty() || !schema.leaf_types.is_empty() || !schema.union_types.is_empty()).then_some(schema)
}

fn java_file(path: &str, text: &str) -> Option<Vec<Handler>> {
    if !text.contains("Mapping") && !text.contains("dataFetcher") {
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
        .or_else(|| (!schemas.iter().any(|(_,s)|s.explicit_definition)).then(||ROOTS.iter().find(|r|r.1==op).map(|r|r.2.to_owned())).flatten())
        .unwrap_or_default()
}

fn type_of(h: &Handler, _schemas: &Index<Schema>) -> String {
    h.ty.clone()
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

/// The host's navigate arrow for every link this module draws: explicit so same-line markers
/// group predictably, and apart from the web route icon so they never merge with its URL markers.
const MARKER_ICON: &str = "navigate";

/// Most markers one line gets; the rest are named in the tooltip.
const MAX_LINE_TARGETS: usize = 200;

/// One marker per target on `line`: the IDE folds same-line, same-icon markers into one gutter
/// button that jumps to a single target or opens its chooser for several.
fn per_target(line: u32, tooltip: &str, targets: Vec<Location>) -> Vec<LineMarker> {
    let total = targets.len();
    let tooltip = if total > MAX_LINE_TARGETS { format!("{tooltip} (first {MAX_LINE_TARGETS} of {total})") } else { tooltip.to_owned() };
    targets.into_iter().take(MAX_LINE_TARGETS).map(|t| LineMarker { line, icon: Some(MARKER_ICON.into()), tooltip: tooltip.clone(), target: Some(t) }).collect()
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

/// A `spring.graphql.*` or context path setting: key, value and where it is written.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Setting {
    key: String,
    value: String,
    at: Range,
}

fn endpoint_settings(path: &str, text: &str) -> Option<Vec<Setting>> {
    let found: Vec<Setting> = config::keys(path, text)
        .into_iter()
        .map(|k| (config::canonical(&k.key), k))
        .filter(|(key, k)| (key.starts_with("spring.graphql.") || key == "server.servlet.contextpath") && !k.value.is_empty())
        .map(|(key, k)| Setting { key, value: k.value, at: range(text, k.at, k.end) })
        .collect();
    (!found.is_empty()).then_some(found)
}

const MAPPING_MARKERS: [&str; 5] = ["@SchemaMapping", "@BatchMapping", "@QueryMapping", "@MutationMapping", "@SubscriptionMapping"];

/// Hand wiring, or a mapping annotation the literal handler parser did not accept (dynamic, class-level, aliased): the handler set is then unprovable.
fn mentions_wiring(path: &str, text: &str) -> Option<bool> {
    if wiring::wired(text).closed {
        return Some(true);
    }
    let marked: usize = MAPPING_MARKERS.iter().map(|m| text.matches(m).count()).sum();
    (marked > 0 && marked != controller_handlers(text, path.ends_with(".kt")).len()).then_some(true)
}

/// What one Java or Kotlin file says to this module, read once.
#[derive(Clone)]
pub struct JavaFacts {
    handlers: Vec<Handler>,
    /// Hand wiring, or a mapping the literal parser did not accept: the handler set is unprovable.
    wired: bool,
}

fn java_facts(path: &str, text: &str) -> Option<JavaFacts> {
    let wired = mentions_wiring(path, text).unwrap_or(false);
    let handlers = java_file(path, text).unwrap_or_default();
    (wired || !handlers.is_empty()).then_some(JavaFacts { handlers, wired })
}

fn handler_files_exist(java: &Index<JavaFacts>) -> bool {
    java.iter().any(|(_, f)| !f.handlers.is_empty())
}

pub struct GraphQlTools {
    schemas: Indexes<Schema>,
    java: Indexes<JavaFacts>,
    settings: Indexes<Vec<Setting>>,
    expansion: Expansion,
}

impl Default for GraphQlTools {
    fn default() -> Self {
        Self {
            schemas: Indexes::new(SCHEMA_GLOBS, schema_file),
            java: Indexes::new(JAVA_GLOBS, java_facts),
            settings: Indexes::new(CONFIG_GLOBS, endpoint_settings),
            expansion: Expansion::default(),
        }
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

const MAX_SELECTION_DEPTH: usize = 64;

fn selection(toks: &[Tok], open: usize, scope: &DocField, depth: usize, out: &mut Vec<DocField>) {
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
            if depth < MAX_SELECTION_DEPTH {
                let mut inner = field;
                inner.path.push(s.to_owned());
                selection(toks, k, &inner, depth + 1, out);
            }
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
                    selection(&toks, k, &scope, 0, &mut out);
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
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let fields = if is_code(doc) { inline_documents(&doc.text, doc.language == "kotlin") } else { document_fields(&doc.text) };
        let Some(hit) = fields.into_iter().find(|f| f.at <= at && at <= f.end) else { return Vec::new() };
        let Some(schemas) = self.schemas.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        let mut ty = match (&hit.on, &hit.op) {
            (Some(on), _) => on.clone(),
            (None, Some(op)) => root_type(schemas, op),
            (None, None) => return Vec::new(),
        };
        for name in &hit.path {
            let Some((_, parent)) = schema_fields(schemas, &ty, name).into_iter().next() else { return Vec::new() };
            ty = parent.ty;
        }
        let mut found: Vec<Location> =
            schema_fields(schemas, &ty, &hit.name).into_iter().map(|(path, f)| Location { path, range: f.range }).collect();
        if hit.path.is_empty() && hit.op.is_some() {
            let Some(java) = self.java.for_cached_document(&doc.path, &doc.text) else { return found };
            found.extend(
                java.iter()
                    .flat_map(|(path, f)| f.handlers.iter().map(move |h| (path, h)))
                    .filter(|(_, h)| h.field == hit.name && type_of(h, schemas) == ty)
                    .map(|(path, h)| Location { path: path.to_owned(), range: h.method }),
            );
        }
        found
    }

    /// The open Java file's handlers with the schema fields they map to; publishes its problems.
    fn java_links(&mut self, doc: &Document) -> Vec<(Handler, String, Vec<(String, Field)>)> {
        let Some(schemas) = self.schemas.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        let links: Vec<_> = handlers_in(&doc.text, doc.language == "kotlin")
            .into_iter()
            .map(|h| {
                let ty = type_of(&h, schemas);
                let found = schema_fields(schemas, &ty, &h.field);
                (h, ty, found)
            })
            .collect();
        if cfg!(feature = "embedded") {
            return links;
        }
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
        publish(&doc.path, &problems);
        links
    }

    /// The open schema file's fields (with their type) and the handlers serving them; publishes its hints.
    fn schema_links(&mut self, doc: &Document) -> Vec<(String, Field, Vec<(Location, String)>)> {
        let Some(schemas) = self.schemas.for_cached_document(&doc.path, &doc.text) else {
            return Vec::new();
        };
        let schemas: &Index<Schema> = schemas;
        let Some(java) = self.java.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        let roots: Vec<String> = ROOTS.iter().map(|r| root_type(schemas, r.1)).collect();
        let mut problems = Vec::new();
        let mut links = Vec::new();
        for t in parse_schema(&doc.text).types {
            for f in t.fields {
                let found: Vec<(Location, String)> = java
                    .iter()
                    .flat_map(|(path, f)| f.handlers.iter().map(move |h| (path, h)))
                    .filter(|(_, h)| h.field == f.name && type_of(h, schemas) == t.name)
                    .map(|(path, h)| (Location { path: path.to_owned(), range: h.method }, h.label.clone()))
                    .collect();
                let unmapped_root = found.is_empty() && t.kind == "type" && roots.contains(&t.name);
                if !cfg!(feature = "embedded") && unmapped_root && java.complete() && handler_files_exist(java) {
                    let message = format!("No controller method handles `{}.{}`.", t.name, f.name);
                    problems.push(Diagnostic::new(f.range, Severity::Hint, message).code("unmapped-root-field"));
                }
                links.push((t.name.clone(), f, found));
            }
        }
        if !cfg!(feature = "embedded") && host::setting_bool("springGraphql.reportUnmappedFields").unwrap_or(true) {
            if java.complete() && java.iter().all(|(_, f)| !f.wired) {
                let objects: Vec<String> = parse_schema(&doc.text).types.into_iter().filter(|t| t.kind == "type").map(|t| t.name).collect();
                let interface_handles = |field: &str| {
                    java.iter().flat_map(|(_, f)| &f.handlers).any(|h| {
                        h.field == field && schemas.iter().any(|(_, s)| s.types.iter().any(|t| t.name == h.ty && t.kind == "interface"))
                    })
                };
                for (ty, f, found) in &links {
                    if found.is_empty() && schemas.complete() && !f.args.is_empty() && !roots.contains(ty) && objects.contains(ty) && !interface_handles(&f.name) {
                        let message = format!(
                            "`{ty}.{}` takes arguments, but no controller method or data fetcher handles it; the default property fetcher ignores them.",
                            f.name
                        );
                        problems.push(Diagnostic::new(f.range, Severity::Info, message).code("unmapped-field"));
                    }
                }
            }
        }
        publish(&doc.path, &problems);
        links
    }
}

impl Providers for GraphQlTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let Some((slot, s)) = annotation_slot(&doc.text, at, doc.language == "kotlin") else { return Vec::new() };
        let Some(schemas) = self.schemas.for_cached_document(&doc.path, &doc.text) else { return Vec::new() };
        let types = schemas.iter().flat_map(|(_, s)| &s.types);
        // Name → detail.
        let (names, kind): (BTreeMap<String, String>, _) = match slot {
            Slot::TypeName => {
                (types.filter(|t| t.kind != "input").map(|t| (t.name.clone(), t.kind.clone())).collect(), CompletionKind::Class)
            }
            Slot::Field(_op, ty) => {
                let fields = types.filter(|t| t.name == ty).flat_map(|t| &t.fields);
                (fields.map(|f| (f.name.clone(), ty.clone())).collect(), CompletionKind::Field)
            }
        };
        let replace = range(&doc.text, s.at, s.end);
        names
            .into_iter()
            .map(|(name, detail)| CompletionItem { detail: Some(detail), replace: Some(replace), ..CompletionItem::new(&name, kind) })
            .collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if is_code(doc) {
            return self
                .java_links(doc)
                .into_iter()
                .flat_map(|(h, ty, found)| {
                    let targets = found.into_iter().map(|(path, f)| Location { path, range: f.range }).collect();
                    per_target(h.method.start.line, &format!("GraphQL schema field {ty}.{}", h.field), targets)
                })
                .collect();
        }
        self.schema_links(doc)
            .into_iter()
            .flat_map(|(_, f, found)| per_target(f.range.start.line, &describe(&found), found.into_iter().map(|(l, _)| l).collect()))
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

const DEFAULT_HTTP_PATH: &str = "/graphql";
const DEFAULT_GRAPHIQL_PATH: &str = "/graphiql";

fn join_url(prefix: &str, path: &str) -> String {
    format!("{}/{}", prefix.trim_end_matches('/'), path.trim_start_matches('/'))
}

/// The endpoint rows of the `spring.graphql.*` settings: label, what the path comes from and where it is written.
fn endpoint_rows(settings: &[Setting]) -> Vec<(String, String, Option<Range>)> {
    let find = |key: &str| settings.iter().find(|s| s.key == config::canonical(key));
    let enabled = |key: &str| find(key).is_some_and(|s| s.value == "true");
    let prefix = find("server.servlet.context-path").map_or("", |s| s.value.as_str());
    let mut rows = Vec::new();
    let mut row = |label: String, key: &str, source: Option<&Setting>| {
        let detail = source.map_or_else(|| "default".to_owned(), |_| key.to_owned());
        rows.push((label, detail, source.map(|s| s.at)));
    };
    let http = find("spring.graphql.path");
    row(format!("POST {}", join_url(prefix, http.map_or(DEFAULT_HTTP_PATH, |s| s.value.as_str()))), "spring.graphql.path", http);
    if let Some(ws) = find("spring.graphql.websocket.path") {
        row(format!("WebSocket {}", join_url(prefix, &ws.value)), "spring.graphql.websocket.path", Some(ws));
    }
    if enabled("spring.graphql.graphiql.enabled") {
        let graphiql = find("spring.graphql.graphiql.path");
        row(
            format!("GET {}", join_url(prefix, graphiql.map_or(DEFAULT_GRAPHIQL_PATH, |s| s.value.as_str()))),
            "spring.graphql.graphiql.path",
            graphiql,
        );
    }
    if enabled("spring.graphql.schema.printer.enabled") {
        let base = http.map_or(DEFAULT_HTTP_PATH, |s| s.value.as_str());
        row(
            format!("GET {}", join_url(prefix, &format!("{}/schema", base.trim_end_matches('/')))),
            "spring.graphql.schema.printer.enabled",
            find("spring.graphql.schema.printer.enabled"),
        );
    }
    rows
}

pub struct RequestDraft {
    pub path:String,
    pub operation:String,
    pub field:String,
    pub query:String,
    pub variables:Vec<(String,String)>,
}

impl GraphQlTools {
    /// Positive operation ownership for source actions; no nonroot action is advertised.
    pub fn source_operations(&mut self,doc:&Document)->Vec<(Range,String)>{
        if !is_code(doc)||doc.text.len()>128<<10{return Vec::new()}
        let Some(schemas)=self.schemas.for_cached_document(&doc.path,&doc.text)else{return Vec::new()};
        if !schemas.complete(){return Vec::new()}
        handlers_in(&doc.text,doc.language=="kotlin").into_iter().filter_map(|h|{
            let mut operations=Vec::new();
            for (op,default) in [("query","Query"),("mutation","Mutation"),("subscription","Subscription")] {
                let mut roots=schemas.iter().flat_map(|(_,s)|&s.roots).filter(|(o,_)|o==op).map(|(_,t)|t.as_str()).collect::<Vec<_>>();roots.sort();roots.dedup();
                let root=match roots.as_slice(){[] if !schemas.iter().any(|(_,s)|s.explicit_definition)=>default,[one]=>*one,_=>continue};
                if h.ty==root&&h.op.as_deref().is_none_or(|o|o==op)&&!schema_fields(schemas,root,&h.field).is_empty(){operations.push(op)}
            }
            (operations.len()==1).then(||(h.method,operations[0].to_owned()))
        }).collect()
    }
    /// Builds a request from one admitted current source handler and a complete
    /// local SDL model. Unknown or conflicting schema contracts are refused.
    pub fn request_draft(&mut self,doc:&Document,handler:&Handler)->Result<RequestDraft,Error>{self.operation_draft(doc,handler,false)}
    pub fn subscription_draft(&mut self,doc:&Document,handler:&Handler)->Result<RequestDraft,Error>{self.operation_draft(doc,handler,true)}
    fn operation_draft(&mut self,doc:&Document,handler:&Handler,subscription:bool)->Result<RequestDraft,Error>{
        let project=project_of(&doc.path).ok_or_else(||Error::Failed("Open the GraphQL project first.".into()))?;
        self.schemas.reset(); self.settings.reset();
        let schemas=self.schemas.get(&project,spring_text::index::COMMAND_BUDGET);
        if !schemas.complete(){return Err(Error::Failed("GraphQL schema index is incomplete; refresh before generating a request.".into()))}
        let admitted=|op:&str|if subscription{op=="subscription"}else{matches!(op,"query"|"mutation")};
        let operation = if let Some(op)=handler.op.as_deref() {
            if !admitted(op){return Err(Error::Invalid("Handler operation does not match the selected GraphQL workflow.".into()))} op
        } else {
            let mut operations=schemas.iter().flat_map(|(_,s)|&s.roots).filter(|(_,ty)|ty==&handler.ty).map(|(op,_)|op.as_str()).collect::<Vec<_>>();
            for (op,ty) in [("query","Query"),("mutation","Mutation"),("subscription","Subscription")] {
                if handler.ty==ty && !schemas.iter().any(|(_,s)|s.explicit_definition||s.roots.iter().any(|(o,_)|o==op)){operations.push(op)}
            }
            operations.sort();operations.dedup();
            if operations.len()!=1||!admitted(operations[0]){return Err(Error::Invalid("SchemaMapping parent is not one unambiguous selected operation root.".into()))}
            operations[0]
        };
        let mut roots=schemas.iter().flat_map(|(_,s)|&s.roots).filter(|(op,_)|op==operation).map(|(_,ty)|ty.clone()).collect::<Vec<_>>();roots.sort();roots.dedup();
        if roots.len()>1{return Err(Error::Failed("GraphQL schema has conflicting operation root declarations.".into()))}
        if roots.is_empty()&&schemas.iter().any(|(_,s)|s.explicit_definition){return Err(Error::Invalid("The explicit schema does not declare the selected operation root.".into()))}
        let root=roots.first().cloned().unwrap_or_else(||match operation{"query"=>"Query","mutation"=>"Mutation",_=>"Subscription"}.into());
        if handler.ty!=root{return Err(Error::Failed("The annotation parent type is not the current SDL operation root; use an explicit root SchemaMapping.".into()))}
        let fields=schemas.iter().flat_map(|(_,s)|&s.types).filter(|t|t.name==root).flat_map(|t|&t.fields).filter(|f|f.name==handler.field).collect::<Vec<_>>();
        let field=fields.first().ok_or_else(||Error::Failed("The current GraphQL schema does not declare this handler field.".into()))?;
        if field.type_reference.is_empty()||field.argument_types.iter().any(|(_,ty)|ty.is_empty())||field.argument_types.iter().enumerate().any(|(i,(name,_))|field.argument_types[..i].iter().any(|(before,_)|before==name)){return Err(Error::Failed("GraphQL field has an unsupported or malformed type reference.".into()))}
        if fields.iter().any(|f|f.type_reference!=field.type_reference||f.argument_types!=field.argument_types){return Err(Error::Failed("GraphQL field declarations have conflicting signatures.".into()))}
        let leaf=["String","ID","Int","Float","Boolean"].contains(&field.ty.as_str())||schemas.iter().any(|(_,s)|s.leaf_types.contains(&field.ty));
        let object=schemas.iter().any(|(_,s)|s.types.iter().any(|t|t.name==field.ty&&matches!(t.kind.as_str(),"type"|"interface"))||s.union_types.contains(&field.ty));
        if !leaf&&!object{return Err(Error::Failed("GraphQL return type is not declared in the current SDL.".into()))}
        let variables=field.argument_types.iter().filter(|(_,ty)|ty.ends_with('!')).cloned().collect::<Vec<_>>();
        let definitions=variables.iter().map(|(name,ty)|format!("${name}: {ty}")).collect::<Vec<_>>().join(", ");
        let arguments=variables.iter().map(|(name,_)|format!("{name}: ${name}")).collect::<Vec<_>>().join(", ");
        let query=format!("{operation} OpenIdeaRequest{} {{\n  {}{}{}\n}}",if definitions.is_empty(){String::new()}else{format!("({definitions})")},field.name,if arguments.is_empty(){String::new()}else{format!("({arguments})")},if object{" { __typename }"}else{""});
        if subscription{return Ok(RequestDraft{path:String::new(),operation:operation.into(),field:field.name.clone(),query,variables})}
        let settings=self.settings.get(&project,spring_text::index::COMMAND_BUDGET);
        if !settings.complete(){return Err(Error::Failed("GraphQL configuration index is incomplete.".into()))}
        let mut values=Vec::new();
        for (_,entries) in settings.iter(){for entry in entries {
            if values.iter().any(|v:&Setting|v.key==entry.key&&v.value!=entry.value) && ["spring.graphql.path","server.servlet.contextpath"].contains(&entry.key.as_str()) {return Err(Error::Failed("GraphQL HTTP path varies across configuration sources; choose an explicit active context.".into()))}
            if !values.iter().any(|v:&Setting|v.key==entry.key){values.push(entry.clone())}
        }}
        let row=endpoint_rows(&values).into_iter().find(|(label,_,_)|label.starts_with("POST ")).ok_or_else(||Error::Failed("GraphQL HTTP endpoint is unavailable.".into()))?;
        let path=row.0.trim_start_matches("POST ").to_owned();
        if !path.starts_with('/')||path.starts_with("//")||path.chars().any(|c|c.is_whitespace()||c.is_control())||path.contains(['$','{','}', '\\', '?', '#']){return Err(Error::Failed("GraphQL HTTP path is unresolved or invalid.".into()))}
        Ok(RequestDraft{path,operation:operation.into(),field:field.name.clone(),query,variables})
    }
}

impl GraphQlTools {
    fn endpoints_view(&mut self, view: &str, project: &Project, budget: Duration) -> View {
        let settings_index = self.settings.get(project, budget / 2);
        let settings_note = spring_text::index::coverage_note(settings_index);
        let mut settings: Vec<Setting> = Vec::new();
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        for (path, found) in settings_index.iter() {
            for s in found {
                if !settings.iter().any(|x| x.key == s.key) {
                    settings.push(s.clone());
                    files.insert(s.key.clone(), path.to_owned());
                }
            }
        }
        let schemas = self.schemas.get(project, budget / 2);
        let schemas_note = spring_text::index::coverage_note(schemas);
        if settings_index.scanning() || schemas.scanning() {
            refresh(view);
        }
        let refresh = Button::new("refresh", "Refresh").icon("refresh");

        let mut items = vec![TreeItem::new("endpoints", "Endpoints").icon("web")];
        for (n, (label, detail, at)) in endpoint_rows(&settings).into_iter().enumerate() {
            let source = settings.iter().find(|s| Some(s.at) == at).and_then(|s| files.get(&s.key).map(|f| (f.clone(), s.at)));
            let key = match source {
                Some((path, at)) => format!("c:{}:{}:{path}", at.start.line, at.start.character),
                None => format!("d:{n}"),
            };
            items.push(TreeItem::new(&key, &label).parent("endpoints").detail(&detail).icon("web"));
        }
        let mut schema_files: Vec<_> = schemas.iter().map(|(p, s)| (p.to_owned(), s.types.len())).collect();
        schema_files.sort();
        items.push(TreeItem::new("schemas", "Schema files").detail(&schema_files.len().to_string()).icon("folder"));
        for (path, types) in schema_files {
            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
            items.push(TreeItem::new(&format!("c:0:0:{path}"), &name).parent("schemas").detail(&format!("{types} types")).icon("file-graphql"));
        }
        self.expansion.apply(&mut items);
        let status = match settings_note.into_iter().chain(schemas_note).min_by_key(|note| note != spring_text::index::SCANNING_NOTE) {
            None => "GraphQL endpoints".to_owned(),
            Some(note) => format!("GraphQL endpoints · {note}"),
        };
        View::new(Body::Tree(items)).button(refresh).status(&status)
    }
}

impl Views for GraphQlTools {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its GraphQL endpoints."));
            };
            Ok(self.endpoints_view(view, &project, spring_text::index::VIEW_BUDGET))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            refresh(view);
            return Ok(());
        }
        match event {
            ViewEvent::Button(b) if b == "refresh" => {
                if let Some(project) = ctx.project()
                    && !self.settings.get(&project, Duration::ZERO).scanning()
                    && !self.schemas.get(&project, Duration::ZERO).scanning()
                {
                    self.settings.reset();
                    self.schemas.reset();
                }
                refresh(view);
            }
            ViewEvent::Activate(key) => {
                let mut parts = key.splitn(4, ':');
                if let (Some("c"), Some(line), Some(column), Some(path)) = (parts.next(), parts.next(), parts.next(), parts.next())
                    && let (Ok(line), Ok(column)) = (line.parse(), column.parse())
                {
                    host::open_location(path, line, column)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Plugin for GraphQlTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

pub const WINDOW_SECTION: &str = "graphql";

const OWNER_WINDOW: &str = "endpoints";

fn window(view: &str) -> &str {
    if cfg!(feature = "embedded") { OWNER_WINDOW } else { view }
}

pub(crate) fn refresh(view: &str) {
    host::refresh_view(window(view));
}

thread_local! {
    static PROBLEMS: std::cell::RefCell<Vec<openidea_plugin::Diagnostic>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The diagnostics belong to the owner plugin, which publishes every module's together: a publish replaces the whole file.
fn publish(path: &str, problems: &[openidea_plugin::Diagnostic]) {
    if cfg!(feature = "embedded") {
        PROBLEMS.set(problems.to_vec());
        return;
    }
    let _ = host::publish_diagnostics(path, problems);
}

pub fn take_problems() -> Vec<openidea_plugin::Diagnostic> {
    PROBLEMS.take()
}

const DEPENDENCY_MARKERS: [&str; 1] = ["graphql"];

/// True when unsure (see `spring_text::build::applies`).
pub fn applies(project: &openidea_plugin::Project) -> bool {
    spring_text::build::applies(project, &DEPENDENCY_MARKERS)
}

#[cfg(not(feature = "embedded"))]
openidea_plugin::register_plugin!(GraphQlTools, providers, views);

#[cfg(test)]
#[cfg_attr(feature = "embedded", allow(dead_code, unused_imports))]
mod tests {
    fn warmed() -> GraphQlTools {
        let mut tools = GraphQlTools::default();
        if let Some(project) = host::projects().unwrap_or_default().first() {
            tools.live_warm(project, COMMAND_BUDGET, true);
        }
        tools
    }
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    #[test]
    fn deep_graphql_selection_no_overflow() {
        let text = format!("query {{ {}", "a { ".repeat(20_000));
        let fields = document_fields(&text);
        assert!(!fields.is_empty() && fields.len() <= MAX_SELECTION_DEPTH + 1);
    }

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
    fn malformed_and_excessive_sdl_type_wrappers_are_not_request_signatures(){
        for source in ["type Query { field: String!! }".to_owned(),format!("type Query {{ field: {}String{} }}","[".repeat(10000),"]".repeat(10000))] {
            let schema=parse_schema(&source);assert!(schema.types[0].fields[0].type_reference.is_empty());
        }
    }
    #[test]
    fn request_draft_uses_exact_schema_root_argument_wrappers_and_configured_path(){
        let host=MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls","union Result=Book | Author type Query { find(ids:[ID!]!, optional:Int): Result } type Book {id:ID} type Author {id:ID}");
        host.add_file("src/main/resources/application.properties","server.servlet.context-path=/app\nspring.graphql.path=/api/gql\n");
        let d=doc("/project/C.java","java","@Controller class C { @QueryMapping(\"find\") Result search(@Argument List<String> ids){} }");
        let h=handlers(&d.text).remove(0);let draft=warmed().request_draft(&d,&h).unwrap();
        assert_eq!(draft.path,"/app/api/gql");assert!(draft.query.contains("$ids: [ID!]!"));assert!(draft.query.contains("find(ids: $ids) { __typename }"));assert!(!draft.query.contains("optional"));
    }
    #[test]
    fn foreign_dynamic_and_conflicting_mapping_sources_do_not_create_handlers(){
        for source in [
            "import foreign.QueryMapping; @Controller class C { @QueryMapping String find(){} }",
            "import foreign.Controller; @Controller class C { @QueryMapping String find(){} }",
            "@Controller class C { @QueryMapping(NAME) String find(){} }",
            r#"@Controller class C { @QueryMapping("x\ny") String find(){} }"#,
            "@Controller class C { @QueryMapping(\"bad field\") String find(){} }",
            "@Controller class C { @QueryMapping(value=\"a\",name=\"b\") String find(){} }",
            "@Controller @SchemaMapping(typeName=TYPE) class C { @SchemaMapping String find(){} }",
        ] {assert!(handlers(source).is_empty(),"{source}");}
        let h=handlers("@Controller class C { @QueryMapping String find(@Argument(NAME) String wrong){} }").remove(0);assert!(h.args.is_empty());
        let batch=handlers("@Controller class C { @BatchMapping(typeName=\"Book\",maxBatchSize=LIMIT) Object author(List<Book> books){} }");assert_eq!(batch.len(),1);assert_eq!(batch[0].ty,"Book");
        let alias=|imports:&str,body:&str|handlers_in(&format!("{imports}\n@Ctl class C {{ {body} }}"),true);
        let spring="import org.springframework.stereotype.Controller as Ctl\nimport org.springframework.graphql.data.method.annotation.QueryMapping as QM\nimport org.springframework.graphql.data.method.annotation.Argument as Arg";
        let found=alias(spring,"@QM fun find(@Arg id:String):String=\"x\"");assert_eq!(found.len(),1);assert_eq!((found[0].ty.as_str(),found[0].field.as_str()),("Query","find"));assert_eq!(found[0].args[0].0,"id");
        assert!(alias(spring,"@QueryMapping fun find():String=\"x\"").is_empty(),"hidden real name");
        assert!(alias(&format!("{spring}\nimport other.QM"),"@QM fun find():String=\"x\"").is_empty());
        assert!(alias("import foreign.Controller as Ctl\nimport org.springframework.graphql.data.method.annotation.QueryMapping as QM","@QM fun find():String=\"x\"").is_empty());
        assert!(alias(&format!("{spring}\ntypealias QM = Foo"),"@QM fun find():String=\"x\"").is_empty());
        assert!(handlers_in("import other.X as QueryMapping\n@Controller class C { @QueryMapping fun find():String=\"x\" }",true).is_empty());
    }
    #[test]
    fn shortcut_parent_is_fixed_and_custom_sdl_root_does_not_remap_it(){
        let host=MockHost::install();host.add_file("src/main/resources/graphql/schema.graphqls","schema {query: Root} type Root { find: String } type Query { find: String }");
        let d=doc("/project/C.java","java","@Controller class C { @QueryMapping String find(){} }");let h=handlers(&d.text).remove(0);
        assert_eq!(h.ty,"Query");
        let mut tools=warmed();
        let project=project_of(&d.path).unwrap();let schemas=tools.schemas.get(&project,spring_text::index::COMMAND_BUDGET);
        assert_eq!(type_of(&h,schemas),"Query");
        assert!(tools.request_draft(&d,&h).is_err());
        let explicit=doc("/project/C.java","java","@Controller class C { @SchemaMapping(typeName=\"Root\",field=\"find\") String search(){} }");
        let h=handlers(&explicit.text).remove(0);let draft=tools.request_draft(&explicit,&h).unwrap();assert_eq!(draft.operation,"query");assert!(draft.query.contains("find"));
    }
    #[test]
    fn subscription_document_uses_exact_sdl_and_never_chooses_http_transport(){
        let host=MockHost::install();host.add_file("src/main/resources/graphql/schema.graphqls","type Subscription { changed(ids:[ID!]!): Item } type Item {id:ID}");
        let d=doc("/project/C.java","java","@Controller class C { @SubscriptionMapping Item changed(){} }");let h=handlers(&d.text).remove(0);let mut tools=warmed();
        let draft=tools.subscription_draft(&d,&h).unwrap();assert!(draft.path.is_empty());assert_eq!(draft.operation,"subscription");assert!(draft.query.contains("$ids: [ID!]!"));assert!(draft.query.contains("changed(ids: $ids) { __typename }"));assert!(tools.request_draft(&d,&h).is_err());
        host.add_file("src/main/resources/graphql/schema.graphqls","schema {subscription: Events} type Events {changed:String}");assert!(tools.subscription_draft(&d,&h).is_err());
        let d=doc("/project/C.java","java","@Controller class C { @SchemaMapping(typeName=\"Events\",field=\"changed\") String observe(){} }");let h=handlers(&d.text).remove(0);assert!(tools.subscription_draft(&d,&h).unwrap().query.starts_with("subscription "));
    }
    #[test]
    fn generated_client_document_directory_is_not_reindexed_as_schema(){
        assert!(schema_file("/project/graphql-documents/subscription-type.graphql","subscription { type { name } }").is_none());
        assert!(schema_file("/project/src/main/resources/graphql/schema.graphqls","type Subscription {changed:String}").is_some());
    }
    #[test]
    fn explicit_schema_does_not_invent_undeclared_subscription_roots(){
        let host=MockHost::install();host.add_file("src/main/resources/graphql/schema.graphqls","schema {query: Query} type Query {now:String} type Subscription {changed:String}");
        let d=doc("/project/C.java","java","@Controller class C {@SubscriptionMapping String changed(){} }");let h=handlers(&d.text).remove(0);let mut tools=warmed();assert!(tools.subscription_draft(&d,&h).is_err());assert!(tools.source_operations(&d).is_empty());
        host.add_file("src/main/resources/graphql/schema.graphqls","type Query {now:String} extend schema {subscription: Subscription} type Subscription {changed:String}");assert!(tools.subscription_draft(&d,&h).is_ok());
    }
    #[test]
    fn request_draft_keeps_scalar_results_leaf_and_refuses_conflicting_schema_or_paths(){
        let host=MockHost::install();host.add_file("src/main/resources/graphql/schema.graphqls","scalar Date enum Status {OPEN CLOSED} type Query { today: Date state: Status }");
        let d=doc("/project/C.java","java","@Controller class C { @QueryMapping Date today(){} }");let h=handlers(&d.text).remove(0);let mut tools=warmed();
        let draft=tools.request_draft(&d,&h).unwrap();assert!(!draft.query.contains("__typename"));assert!(draft.variables.is_empty());
        host.add_file("src/main/resources/graphql/other.graphqls","type Query {today: String}");assert!(tools.request_draft(&d,&h).is_err());
        host.add_file("src/main/resources/graphql/other.graphqls","");
        host.add_file("src/main/resources/application.properties","spring.graphql.path=/graphql\n");host.add_file("src/main/resources/application-dev.properties","spring.graphql.path=/other\n");assert!(tools.request_draft(&d,&h).is_err());
    }
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
        let found: Vec<_> = handlers(CONTROLLER).into_iter().map(|h| (h.op.clone(), h.ty.clone(), h.field.clone(), h.args.len())).collect();
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

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn every_match_of_a_handler_or_field_gets_a_marker_on_its_line() {
        let host = setup();
        host.add_file("src/main/resources/graphql/isbn.graphqls", "extend type Book { isbn: String }\n");
        host.add_file("src/main/resources/graphql/more.graphqls", "extend type Book { isbn: String }\n");
        host.add_file(
            "src/main/java/com/acme/TitleController.java",
            "@Controller\nclass TitleController {\n    @SchemaMapping(typeName = \"Book\")\n    public String title(Book book) { return null; }\n}\n",
        );
        let mut tools = warmed();
        let isbn = at(CONTROLLER, "isbn(").line;
        let handler: Vec<_> = tools.line_markers(&java()).into_iter().filter(|m| m.line == isbn).collect();
        let files: Vec<_> = handler.iter().map(|m| m.target.as_ref().unwrap().path.rsplit('/').next().unwrap().to_owned()).collect();
        assert_eq!(files, ["isbn.graphqls", "more.graphqls"]);
        assert!(handler.iter().all(|m| m.icon.as_deref() == Some(MARKER_ICON)));

        let title = at(SCHEMA, "title(").line;
        let field: Vec<_> = tools.line_markers(&graphql()).into_iter().filter(|m| m.line == title).collect();
        assert_eq!(field.len(), 2, "{field:?}");
        assert!(field.iter().all(|m| m.icon.as_deref() == Some(MARKER_ICON)));
        assert_eq!(tools.line_markers(&graphql()).iter().filter(|m| m.line == at(SCHEMA, "author:").line).count(), 1);
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn links_handlers_to_schema_fields() {
        let host = setup();
        let mut tools = warmed();
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
            [("unknown-graphql-argument", "sort"), ("unknown-schema-field", "removeBook"), ("unknown-schema-field", "isbn"),],
            "Author is not in the schema: no warning for Author.books"
        );
    }

    fn problems_of(host: &MockHost, text: &str) -> Vec<String> {
        let mut tools = warmed();
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
        host.diagnostics()["/project/src/main/java/com/acme/C.java"]
            .iter()
            .filter_map(|d| d.code.clone())
            .filter(|c| c.starts_with("batch"))
            .collect()
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn batch_mapping_group_can_be_switched_off() {
        let host = setup();
        assert_eq!(batch_codes(&host, BATCH), ["batch-mapping-return-type"]);
        host.set_setting("springGraphql.checkBatchMappingReturn", "false");
        assert!(batch_codes(&host, BATCH).is_empty());
    }

    const MAPPED: &str = "@Controller\nclass C {\n    @QueryMapping\n    public Book nope() { return null; }\n}\n";

    #[cfg(not(feature = "embedded"))]
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

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = setup();
        assert_eq!(problems_of(&host, MAPPED).len(), 1);
        let fixed = MAPPED.replace("nope", "stats");
        assert!(problems_of(&host, &fixed).is_empty());
        assert!(host.diagnostics().get("/project/src/main/java/com/acme/C.java").is_none_or(Vec::is_empty));
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn links_schema_fields_to_handlers() {
        let host = setup();
        let mut tools = warmed();
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
        let mut tools = warmed();
        let types = ["Book", "Mutation", "Query"];
        assert_eq!(complete(&mut tools, &CONTROLLER.replace("typeName = \"Author\"", "typeName = \"|\"")), types);
        assert_eq!(complete(&mut tools, &CONTROLLER.replace("(typeName = \"Book\")", "(typeName = \"B|\")")), types);
        let text = CONTROLLER.replace("@QueryMapping(name = \"books\")", "@QueryMapping(name = \"bo|\")");
        assert_eq!(complete(&mut tools, &text), ["authors", "bookById", "books", "stats"]);
        let text = CONTROLLER.replace("@BatchMapping", "@BatchMapping(field = \"|\")");
        assert_eq!(complete(&mut tools, &text), ["author", "id", "title"]);
        assert!(complete(&mut tools, &CONTROLLER.replace("@Argument String sort", "@Argument String so|rt")).is_empty());
    }

    const DOCUMENT: &str = "query Find($id: ID!) { bookById(id: $id) { title author { name } } }\nmutation { addBook(input: {}) { id } }";

    #[test]
    fn client_document_field_links_to_schema() {
        let _host = setup();
        let mut tools = warmed();
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

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn kotlin_query_mapping_links_to_schema() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", SCHEMA);
        host.add_file("src/main/kotlin/com/acme/BookController.kt", KOTLIN);
        let mut tools = warmed();
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

    const DOC_SCHEMA: &str = "schema { query: RootQuery mutation: Mutation subscription: Subscription }\ntype RootQuery { bookById(id: ID): Book other: Book }\ntype Subscription { bookAdded: Book }\ntype Mutation { addBook: Book }\ntype Book { id: ID title: String }";

    fn doc_links(path: &str, language: &str, text: &str, needle: &str) -> Vec<Location> {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", DOC_SCHEMA);
        warmed().definition(&doc(path, language, text), at(text, needle))
    }

    fn quiet_links(text: &str, needle: &str) -> Vec<Location> {
        let path = "src/main/resources/graphql-documents/d.graphql";
        warmed().definition(&doc(path, "graphql", text), at(text, needle))
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
        assert!(warmed().definition(&doc(path, "graphql", spread), last).is_empty());
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
        let mut tools = warmed();
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

    fn tree(view: View) -> Vec<TreeItem> {
        match view.body {
            Body::Tree(items) => items,
            _ => panic!("not a tree"),
        }
    }

    #[test]
    fn endpoint_view_lists_configured_paths_and_schema_files() {
        let host = setup();
        host.add_file(
            "src/main/resources/application.yml",
            "server:\n  servlet:\n    context-path: /app\nspring:\n  graphql:\n    path: /api/gql\n    graphiql:\n      enabled: true\n    websocket:\n      path: /ws\n",
        );
        let mut tools = warmed();
        let project = project_of("/project/x").unwrap();
        let mut view = tools.endpoints_view("endpoints", &project, Duration::from_secs(5));
        if !matches!(view.body, Body::Tree(_)) {
            view = tools.endpoints_view("endpoints", &project, Duration::from_secs(5));
        }
        let labels: Vec<_> = tree(view).into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["Endpoints", "POST /app/api/gql", "WebSocket /app/ws", "GET /app/graphiql", "Schema files", "schema.graphqls"]);
    }

    #[test]
    fn java_provider_cold_cache_returns_fast() {
        let host = setup();
        for n in 0..30 {
            host.add_file(&format!("src/main/java/com/acme/S{n}.java"), &format!("package com.acme;\nclass S{n} {{}}\n"));
        }
        let mut tools = GraphQlTools::default();
        let project = host.invocation().project().unwrap();
        let d = doc("src/main/java/com/acme/BookController.java", "java", CONTROLLER);
        let started = std::time::Instant::now();
        tools.line_markers(&d);
        tools.completion(&d, Position::new(5, 10), None);
        tools.definition(&d, Position::new(5, 10));
        assert!(started.elapsed() < Duration::from_millis(30));
        let cached = tools.java.get(&project, Duration::ZERO).files().len() + tools.schemas.get(&project, Duration::ZERO).files().len();
        assert!(cached <= 2, "a provider on a cold cache read {cached} files");
    }

    #[test]
    fn refresh_resets_incomplete_index() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", "type Query { books: [String] }");
        let mut tools = warmed();
        let project = host.invocation().project().unwrap();
        host.add_file_bytes("src/main/resources/graphql/Legacy.graphqls", b"# caf\xe9");
        tools.schemas.on_events(
            &[openidea_plugin::Event::FileSaved("/project/src/main/resources/graphql/Legacy.graphqls".into())],
            std::time::Instant::now() + COMMAND_BUDGET,
        );
        let schemas = tools.schemas.get(&project, Duration::ZERO);
        assert!(!schemas.complete() && !schemas.scanning());
        let view = tools.render("endpoints", None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().contains("partial index (1 unreadable)"));
        tools.handle("endpoints", ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(tools.schemas.get(&project, Duration::ZERO).files().is_empty(), "an incomplete index starts over");
    }

    #[test]
    fn endpoint_view_defaults_to_graphql() {
        let _host = setup();
        let mut tools = warmed();
        let project = project_of("/project/x").unwrap();
        let labels: Vec<_> =
            tree(tools.endpoints_view("endpoints", &project, Duration::from_secs(5))).into_iter().map(|i| (i.label, i.detail)).collect();
        assert_eq!(labels[1], ("POST /graphql".to_owned(), Some("default".to_owned())));
    }

    const ARGS_SCHEMA: &str = "type Query { author(id: ID!): Author }\ntype Author { name: String books(first: Int): [Book] }\ntype Book { title(upper: Boolean): String }\n";

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn unmapped_fields_with_arguments_are_info_when_nothing_serves_the_type() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", ARGS_SCHEMA);
        host.add_file(
            "src/main/java/com/acme/AuthorController.java",
            "@Controller class A { @QueryMapping public Author author(@Argument String id) { return null; } @SchemaMapping(typeName = \"Book\") public String title(Book b) { return null; } }",
        );
        let schema = doc("src/main/resources/graphql/schema.graphqls", "graphql", ARGS_SCHEMA);
        warmed().line_markers(&schema);
        let found: Vec<_> = host.diagnostics()[&schema.path].iter().map(|d| (d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(found, [("unmapped-field".to_owned(), Severity::Info)], "Author.books only; Book is served");

        host.add_file("src/main/java/com/acme/Wiring.java", "class W { RuntimeWiringConfigurer c() { return null; } }");
        warmed().line_markers(&schema);
        assert!(host.diagnostics().get(&schema.path).is_none_or(Vec::is_empty), "hand wiring anywhere silences it");
    }

    fn wired_pairs(text: &str) -> (Vec<(String, String)>, bool) {
        let w = wiring::wired(text);
        (w.handlers.into_iter().map(|h| (h.ty, h.field)).collect(), w.closed)
    }

    #[test]
    fn registrations_on_the_configure_builder_per_runtime_wiring_builder_type_overloads_are_handlers() {
        let lambda = "import graphql.schema.idl.RuntimeWiring;\n@Configuration class W { @Bean RuntimeWiringConfigurer c() { return builder -> builder.type(\"Query\", b -> b.dataFetcher(\"book\", env -> null).dataFetcher(\"books\", env -> null)).type(\"Book\", (t) -> { t.dataFetcher(\"author\", e -> 1); }); } }";
        let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(wired_pairs(lambda), (pairs(&[("Query", "book"), ("Query", "books"), ("Book", "author")]), false));
        let typed = "class W { RuntimeWiringConfigurer c() { return w -> w.type(TypeRuntimeWiring.newTypeWiring(\"Query\").dataFetcher(\"book\", f)).type(newTypeWiring(\"Book\").dataFetcher(\"id\", g)); } }";
        assert_eq!(wired_pairs(typed), (pairs(&[("Query", "book"), ("Book", "id")]), false));
        let wired = wiring::wired(lambda);
        let h = &wired.handlers[0];
        assert_eq!(h.label, "dataFetcher(\"Query.book\")");
        assert!(handlers_in(typed, false).iter().any(|h| h.ty == "Book" && h.field == "id"));
        let method = "class W implements RuntimeWiringConfigurer { public void configure(RuntimeWiring.Builder builder) { builder.type(\"Query\", b -> b.dataFetcher(\"a\", e -> 1)); builder.type(TypeRuntimeWiring.newTypeWiring(\"Book\").dataFetcher(\"id\", g)); } }";
        assert_eq!(wired_pairs(method), (pairs(&[("Query", "a"), ("Book", "id")]), false));
    }

    #[test]
    fn type_calls_on_receivers_not_proven_to_be_the_runtime_wiring_builder_keep_the_gate_closed() {
        for text in [
            "class W { RuntimeWiringConfigurer c() { return w -> other.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)); } }",
            "class W { void c(Foo wiring) { RuntimeWiringConfigurer r = x -> {}; wiring.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)); } }",
            "class W { void c(RuntimeWiring.Builder w) { w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)); this.o.type(\"R\", b -> b.dataFetcher(\"a\", e -> 1)); } }",
        ] {
            assert!(wired_pairs(text).1, "must stay closed: {text}");
        }
        assert!(wired_pairs("class W { Object c() { return x -> x.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)); } }").0.is_empty());
    }

    #[test]
    fn unrecognised_wiring_use_keeps_the_gate_closed() {
        for text in [
            "class W { RuntimeWiringConfigurer c() { return null; } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(NAME, b -> b.dataFetcher(\"a\", e -> 1)); } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(FIELD, e -> 1)); } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1).defaultDataFetcher(f)); } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)).type(other()); } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)).codeRegistry(r); } }",
            "class W { RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)).wiringFactory(f); } }",
            "class W { DataFetcher<String> f; RuntimeWiringConfigurer c() { return w -> w.type(\"Q\", b -> b.dataFetcher(\"a\", f)); } }",
            "class W { void c(GraphQlSourceBuilder b) { b.configureRuntimeWiring(w -> w.type(\"Q\", t -> t.dataFetcher(\"a\", e -> 1))); } }",
            "class W { Object c() { return DataFetcherFactories.useAsyncExecutor(f, e); } }",
            "class W { void c(RuntimeWiring.Builder w) { w.type(\"Q\", b -> b.dataFetcher(\"a\", e -> 1)); TypeRuntimeWiring.Builder t = null; } }",
            "class W { RuntimeWiringConfigurer c() { return w -> { var t = newTypeWiring(\"Q\"); w.type(t); }; } }",
        ] {
            assert!(wired_pairs(text).1, "must stay closed: {text}");
        }
        assert!(!wired_pairs("class Unrelated { void f(Map m) { m.type(1); } }").1);
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn literal_wiring_serves_fields_and_relaxes_the_hint_gate_only_when_fully_recognised() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", ARGS_SCHEMA);
        let wiring = "@Configuration class W { @Bean RuntimeWiringConfigurer c() { return w -> w.type(\"Author\", b -> b.dataFetcher(\"books\", e -> null)); } }";
        host.add_file("src/main/java/com/acme/Wiring.java", wiring);
        let schema = doc("src/main/resources/graphql/schema.graphqls", "graphql", ARGS_SCHEMA);
        let mut tools = warmed();
        let links = tools.schema_links(&schema);
        let books = links.iter().find(|(t, f, _)| t == "Author" && f.name == "books").unwrap();
        assert_eq!(books.2.len(), 1, "SDL field navigates to the registration");
        let all = host.diagnostics();
        let found: Vec<_> = all[&schema.path].iter().filter(|d| d.severity == Severity::Info).collect();
        assert_eq!(found.len(), 1, "Book.title only: {found:?}");

        host.add_file("src/main/java/com/acme/Wiring.java", &wiring.replace("\"books\"", "FIELD"));
        let mut tools = warmed();
        tools.schema_links(&schema);
        assert!(host.diagnostics().get(&schema.path).is_none_or(|v| v.iter().all(|d| d.severity != Severity::Info)), "an unrecognised registration closes the gate");
    }

    #[cfg(not(feature = "embedded"))]
    #[test]
    fn argument_field_on_served_type_is_info_only_when_handlers_are_provably_complete() {
        let host = MockHost::install();
        host.add_file("src/main/resources/graphql/schema.graphqls", ARGS_SCHEMA);
        let controller = "@Controller class A { @BatchMapping public Map<Author, String> name(List<Author> a) { return null; } }";
        host.add_file("src/main/java/com/acme/A.java", controller);
        let schema = doc("src/main/resources/graphql/schema.graphqls", "graphql", ARGS_SCHEMA);
        warmed().line_markers(&schema);
        let found: Vec<_> = host.diagnostics()[&schema.path].iter().map(|d| (d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(found.iter().filter(|f| f.1 == Severity::Info).count(), 2, "Author.books and Book.title: {found:?}");

        host.add_file("src/main/java/com/acme/A.java", &controller.replace("@BatchMapping", "@BatchMapping(typeName = TYPE)"));
        warmed().line_markers(&schema);
        assert!(host.diagnostics().get(&schema.path).is_none_or(|v| v.iter().all(|d| d.severity != Severity::Info)), "a non-literal mapping makes the set unprovable");
    }

    #[cfg(feature = "embedded")]
    #[test]
    fn refresh_goes_to_the_owner_window_when_embedded() {
        let host = MockHost::install();
        refresh("some-view");
        assert_eq!(host.refreshed_views(), ["endpoints"]);
    }

    #[test]
    fn applies_follows_the_root_build_file() {
        let host = MockHost::install();
        let project = &host::projects().unwrap()[0];
        assert!(applies(project), "unsure without a readable build file");
        host.add_file("pom.xml", "<project><artifactId>commons-lang3</artifactId></project>");
        assert!(!applies(project));
        host.add_file("pom.xml", "<project><artifactId>spring-boot-starter-graphql</artifactId></project>");
        assert!(applies(project));
        host.add_file("pom.xml", "<project><modules><module>app</module></modules></project>");
        assert!(applies(project), "multi-module is unsure");
    }
}

impl GraphQlTools {
    pub fn live_warm(&mut self, project: &openidea_plugin::Project, budget: std::time::Duration, discover: bool) {
        if budget.is_zero() {
            return;
        }
        self.schemas.warm(project, budget / 3, discover);
        self.java.warm(project, budget / 3, discover);
        self.settings.warm(project, budget / 3, discover);
    }
    pub fn live_events(&mut self, events: &[openidea_plugin::Event], deadline: std::time::Instant) {
        self.schemas.on_events(events, deadline);
        self.java.on_events(events, deadline);
        self.settings.on_events(events, deadline);
    }
}

mod live;
