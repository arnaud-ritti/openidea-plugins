//! Spring Data Tools for OpenIDEA: checks derived query method names of
//! Spring Data repositories against the domain class, completes them, links
//! repositories to their entities and checks `@Query` parameters.
//! Text-based: types match by simple name among the project's Java and Kotlin sources.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, JavaFile, Member, TypeDecl, TypeRef};
use spring_text::{SOURCE_GLOBS, is_kotlin, parse_source, range};
use std::collections::BTreeMap;

mod checks;
mod jpql;

const SUBJECTS: [&str; 10] = ["find", "read", "get", "query", "search", "stream", "count", "exists", "delete", "remove"];
/// `CrudRepository`/`JpaRepository` methods: implemented by the store, never derived.
const BUILT_IN: [&str; 9] = [
    "findById",
    "existsById",
    "getById",
    "getReferenceById",
    "deleteById",
    "findAllById",
    "deleteAllById",
    "deleteAllByIdInBatch",
    "findBy",
];
/// Parameters a derived query takes besides its criteria.
const SPECIAL_PARAMS: [&str; 7] = ["Pageable", "Sort", "Limit", "ScrollPosition", "OffsetScrollPosition", "KeysetScrollPosition", "Class"];
/// Types whose element is the property type for path traversal.
const CONTAINERS: [&str; 7] = ["List", "Set", "SortedSet", "Collection", "Iterable", "Map", "Optional"];
/// Operator keywords and the arguments they take (`None`: depends on the store).
const OPERATORS: [(&str, Option<usize>); 58] = [
    ("IsNotNull", Some(0)),
    ("NotNull", Some(0)),
    ("IsNull", Some(0)),
    ("Null", Some(0)),
    ("IsBetween", Some(2)),
    ("Between", Some(2)),
    ("IsLessThanEqual", Some(1)),
    ("LessThanEqual", Some(1)),
    ("IsLessThan", Some(1)),
    ("LessThan", Some(1)),
    ("IsGreaterThanEqual", Some(1)),
    ("GreaterThanEqual", Some(1)),
    ("IsGreaterThan", Some(1)),
    ("GreaterThan", Some(1)),
    ("IsBefore", Some(1)),
    ("Before", Some(1)),
    ("IsAfter", Some(1)),
    ("After", Some(1)),
    ("IsNotLike", Some(1)),
    ("NotLike", Some(1)),
    ("IsLike", Some(1)),
    ("Like", Some(1)),
    ("IsStartingWith", Some(1)),
    ("StartingWith", Some(1)),
    ("StartsWith", Some(1)),
    ("IsEndingWith", Some(1)),
    ("EndingWith", Some(1)),
    ("EndsWith", Some(1)),
    ("IsNotEmpty", Some(0)),
    ("NotEmpty", Some(0)),
    ("IsEmpty", Some(0)),
    ("Empty", Some(0)),
    ("IsNotContaining", Some(1)),
    ("NotContaining", Some(1)),
    ("NotContains", Some(1)),
    ("IsContaining", Some(1)),
    ("Containing", Some(1)),
    ("Contains", Some(1)),
    ("IsNotIn", Some(1)),
    ("NotIn", Some(1)),
    ("IsIn", Some(1)),
    ("In", Some(1)),
    ("IsNear", None),
    ("Near", None),
    ("IsWithin", None),
    ("Within", None),
    ("MatchesRegex", Some(1)),
    ("Matches", Some(1)),
    ("Regex", Some(1)),
    ("Exists", Some(0)),
    ("IsTrue", Some(0)),
    ("True", Some(0)),
    ("IsFalse", Some(0)),
    ("False", Some(0)),
    ("IsNot", Some(1)),
    ("Not", Some(1)),
    ("Is", Some(1)),
    ("Equals", Some(1)),
];
/// What completion offers after a complete property.
const CONTINUATIONS: [&str; 26] = [
    "And",
    "Or",
    "OrderBy",
    "IgnoreCase",
    "Not",
    "In",
    "NotIn",
    "Between",
    "LessThan",
    "LessThanEqual",
    "GreaterThan",
    "GreaterThanEqual",
    "Before",
    "After",
    "IsNull",
    "IsNotNull",
    "Like",
    "NotLike",
    "StartingWith",
    "EndingWith",
    "Containing",
    "NotContaining",
    "True",
    "False",
    "IsEmpty",
    "IsNotEmpty",
];

/// A persistent property: a field or a record component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    pub name: String,
    /// Simple type name, collections unwrapped to their element.
    pub ty: String,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Class {
    pub name: String,
    pub range: Range,
    pub parent: Option<String>,
    pub props: Vec<Prop>,
    /// The JPA entity name of an `@Entity` class.
    pub entity: Option<String>,
}

fn element(ty: &TypeRef) -> String {
    match ty.args.last() {
        Some(arg) if CONTAINERS.contains(&ty.name.as_str()) => arg.name.clone(),
        _ => ty.name.clone(),
    }
}

/// The classes and records of a Java or Kotlin file with their persistent properties.
fn classes(path: &str, text: &str) -> Option<Vec<Class>> {
    let file = parse_source(path, text);
    let kotlin = is_kotlin(path);
    let out: Vec<Class> =
        file.types.iter().filter(|t| t.kind == "class" || t.kind == "record").map(|t| class(text, t, kotlin)).collect();
    (!out.is_empty()).then_some(out)
}

fn class(text: &str, t: &TypeDecl, kotlin: bool) -> Class {
    let prop = |name: &str, at: usize, ty: &TypeRef| Prop { name: name.to_owned(), ty: element(ty), range: range(text, at, at + name.len()) };
    let mut props: Vec<Prop> = t.components.iter().map(|p| prop(&p.name, p.at, &p.ty)).collect();
    let persistent = t.fields().filter(|f| !f.modifiers.iter().any(|m| m == "static" || m == "transient") && !f.has(&["Transient"]));
    for f in persistent {
        let Some(ty) = &f.ty else { continue };
        props.push(prop(&f.name, f.at, ty));
        if kotlin {
            continue;
        }
        // `int a, b;`: the scanner keeps the first declarator only.
        let toks = java::tokens(&text[f.at..f.end]);
        for part in java::split_commas(&toks, true).into_iter().skip(1) {
            if let Some(n) = part.first().filter(|t| t.is_ident()) {
                props.push(prop(n.s, f.at + n.at, ty));
            }
        }
    }
    // Spring Data also sees bean properties without a field (`getFullName()`).
    for m in t.methods().filter(|m| m.params.is_empty() && !m.has(&["Transient"]) && !m.modifiers.iter().any(|x| x == "static")) {
        let Some(ty) = &m.ty else { continue };
        let bare = m.name.strip_prefix("get").or_else(|| m.name.strip_prefix("is"));
        if let Some(bare) = bare.filter(|b| b.starts_with(char::is_uppercase))
            && !props.iter().any(|p| p.name == uncapitalize(bare))
        {
            props.push(Prop { name: uncapitalize(bare), ty: element(ty), range: range(text, m.at, m.at + m.name.len()) });
        }
    }
    let parent = if t.kind == "class" { t.extends.first().map(|e| e.name.clone()) } else { None };
    let entity = t.annotation("Entity").map(|a| a.strings(&["name"]).first().map_or_else(|| t.name.clone(), |s| s.value.clone()));
    Class { name: t.name.clone(), range: range(text, t.at, t.at + t.name.len()), parent, props, entity }
}

/// Equal ignoring case, or one edit away from names of 5+ characters, two from names of 8+.
fn is_near_miss(known: &str, name: &str) -> bool {
    if known.eq_ignore_ascii_case(name) {
        return true;
    }
    let shorter = known.chars().count().min(name.chars().count());
    let distance = edit_distance(known, name);
    (distance == 1 && shorter >= 5) || (distance == 2 && shorter >= 8)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb)).min(above + 1).min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The project's classes by simple name.
struct Model<'a> {
    by_name: BTreeMap<&'a str, Vec<(&'a str, &'a Class)>>,
    /// `@Entity` classes by entity name.
    entities: BTreeMap<&'a str, &'a Class>,
    /// Every project file has been read.
    complete: bool,
}

impl<'a> Model<'a> {
    fn new(index: &'a Index<Vec<Class>>) -> Self {
        let mut by_name: BTreeMap<&str, Vec<(&str, &Class)>> = BTreeMap::new();
        let mut entities = BTreeMap::new();
        for (path, classes) in index.iter() {
            for c in classes {
                by_name.entry(&c.name).or_default().push((path, c));
                if let Some(e) = &c.entity {
                    entities.insert(e.as_str(), c);
                }
            }
        }
        Self { by_name, entities, complete: index.complete() }
    }

    /// The known entity or class name that `name` is probably a typo of.
    fn near_miss(&self, name: &str) -> Option<&'a str> {
        let known = self.entities.keys().chain(self.by_name.keys()).copied();
        known.filter(|k| is_near_miss(k, name)).min_by_key(|k| edit_distance(k, name))
    }

    fn class(&self, name: &str) -> Option<(&'a str, &'a Class)> {
        self.by_name.get(name).and_then(|c| c.first()).copied()
    }

    /// Every property of `name` and its superclasses; `None` unless all of them are project classes.
    fn props(&self, name: &str) -> Option<Vec<(&'a str, &'a Prop)>> {
        let mut out = Vec::new();
        let mut next = Some(name);
        for _ in 0..16 {
            let Some(n) = next else { return Some(out) };
            let found = self.by_name.get(n)?;
            for (path, c) in found {
                out.extend(c.props.iter().map(|p| (*path, p)));
            }
            next = found[0].1.parent.as_deref();
        }
        None
    }
}

/// The domain type of each repository interface: `(index in file.types, simple name)`.
pub(crate) fn repositories(file: &JavaFile) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, t) in file.types.iter().enumerate() {
        if t.kind != "interface" || t.has(&["NoRepositoryBean"]) {
            continue;
        }
        let defined = t.annotation("RepositoryDefinition").and_then(|a| a.arg("domainClass")).map(|a| {
            a.text.trim_end_matches(".class").trim_end_matches("::class").rsplit('.').next().unwrap_or_default().trim().to_owned()
        });
        let extended = || {
            t.extends
                .iter()
                .find(|e| (e.name.ends_with("Repository") || e.name.ends_with("RepositoryBase")) && !e.args.is_empty())
                .map(|e| e.args[0].name.clone())
        };
        if let Some(domain) = defined.or_else(extended) {
            out.push((i, domain));
        }
    }
    out
}

/// A property part of a derived query method name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The property path's bytes in the method name.
    pub from: usize,
    pub to: usize,
    /// Arguments the operator takes (`None`: depends on the store); 0 for an `OrderBy` part.
    pub args: Option<usize>,
}

/// Whether `kw` starts at byte `i` of `s` as a keyword: followed by an upper-case letter.
fn keyword_at(s: &str, i: usize, kw: &str) -> bool {
    s.is_char_boundary(i) && s[i..].starts_with(kw) && s[i + kw.len()..].chars().next().is_some_and(char::is_uppercase)
}

/// `s` split at `kw` keywords: byte ranges of the pieces.
fn split_keyword(s: &str, kw: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..s.len() {
        if i > start && keyword_at(s, i, kw) {
            out.push((start, i));
            start = i + kw.len();
        }
    }
    out.push((start, s.len()));
    out
}

/// The predicate start of a derived query name: the byte after `By`.
fn predicate_start(name: &str) -> Option<usize> {
    let verb = SUBJECTS.iter().find(|v| name.starts_with(**v))?;
    if name[verb.len()..].chars().next().is_some_and(|c| !c.is_uppercase()) {
        return None; // `getter`, `searchable`
    }
    Some(verb.len() + name[verb.len()..].find("By")? + 2)
}

/// The property parts of a derived query method name, per Spring Data's
/// method name grammar; `None` when it isn't one.
pub fn derived(name: &str) -> Option<Vec<Part>> {
    if BUILT_IN.contains(&name) {
        return None;
    }
    let at = predicate_start(name)?;
    let pred = &name[at..];
    let (criteria, order) = match pred.find("OrderBy") {
        Some(i) => (&pred[..i], Some(i + "OrderBy".len())),
        None => (pred, None),
    };
    let criteria = ["AllIgnoreCase", "AllIgnoringCase"].iter().find_map(|s| criteria.strip_suffix(s)).unwrap_or(criteria);
    let mut parts = Vec::new();
    for (of, ot) in split_keyword(criteria, "Or") {
        for (af, ato) in split_keyword(&criteria[of..ot], "And") {
            let part = &criteria[of + af..of + ato];
            let part = ["IgnoreCase", "IgnoringCase"].iter().find_map(|s| part.strip_suffix(s)).unwrap_or(part);
            let op = OPERATORS.iter().filter(|(k, _)| part.len() > k.len() && part.ends_with(k)).max_by_key(|(k, _)| k.len());
            let len = part.len() - op.map_or(0, |(k, _)| k.len());
            let from = at + of + af;
            if len > 0 {
                parts.push(Part { from, to: from + len, args: op.map_or(Some(1), |(_, a)| *a) });
            }
        }
    }
    if let Some(o) = order {
        let clause = &pred[o..];
        let mut start = 0;
        for i in (1..=clause.len()).filter(|&i| clause.is_char_boundary(i)) {
            let end = i == clause.len();
            let dir = ["Asc", "Desc"].iter().find(|d| clause[..i].ends_with(**d) && i - d.len() > start);
            if end || (dir.is_some() && clause[i..].chars().next().is_some_and(char::is_uppercase)) {
                let to = i - dir.map_or(0, |d| d.len());
                if to > start {
                    parts.push(Part { from: at + o + start, to: at + o + to, args: Some(0) });
                }
                start = i;
            }
        }
    }
    Some(parts)
}

/// How a property path resolved.
#[derive(Debug, PartialEq, Eq)]
enum Resolved<'a> {
    /// Each step: its bytes in the method name and the property.
    Found(Vec<(usize, usize, &'a str, &'a Prop)>),
    /// A traversed type isn't a project class: nothing can be said.
    Unknown,
    Missing { from: usize, to: usize, name: String, ty: String },
}

fn uncapitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_lowercase().chain(chars).collect()).unwrap_or_default()
}

/// Resolves the camel-case path `s` (at byte `at` of the name) on `ty`:
/// the whole as one property, else the longest head that is one, then the rest on its type.
fn resolve_camel<'a>(model: &Model<'a>, ty: &str, s: &str, at: usize) -> Resolved<'a> {
    let Some(props) = model.props(ty) else { return Resolved::Unknown };
    let find = |s: &str| {
        let lower = uncapitalize(s);
        props.iter().find(|(_, p)| p.name == s || p.name == lower).copied()
    };
    if let Some((path, p)) = find(s) {
        return Resolved::Found(vec![(at, at + s.len(), path, p)]);
    }
    let heads = s.char_indices().skip(1).filter(|(_, c)| c.is_uppercase()).map(|(i, _)| i).collect::<Vec<_>>();
    for &k in heads.iter().rev() {
        let Some((path, p)) = find(&s[..k]) else { continue };
        match resolve_camel(model, &p.ty, &s[k..], at + k) {
            Resolved::Found(mut steps) => {
                steps.insert(0, (at, at + k, path, p));
                return Resolved::Found(steps);
            }
            Resolved::Unknown => return Resolved::Unknown,
            Resolved::Missing { .. } => {}
        }
    }
    Resolved::Missing { from: at, to: at + s.len(), name: uncapitalize(s), ty: ty.to_owned() }
}

/// Resolves the property path `name[from..to]` on `ty`; `_` separates explicitly.
fn resolve<'a>(model: &Model<'a>, ty: &str, name: &str, from: usize, to: usize) -> Resolved<'a> {
    let mut ty = ty.to_owned();
    let mut steps = Vec::new();
    let mut at = from;
    for piece in name[from..to].split('_') {
        if piece.is_empty() {
            return Resolved::Unknown;
        }
        match resolve_camel(model, &ty, piece, at) {
            Resolved::Found(found) => {
                ty = found.last().map(|s| s.3.ty.clone()).unwrap_or_default();
                steps.extend(found);
            }
            other => return other,
        }
        at += piece.len() + 1;
    }
    Resolved::Found(steps)
}

pub(crate) fn derived_method(m: &Member) -> bool {
    !m.has(&["Query", "NativeQuery", "Procedure"])
        && m.body.is_none()
        && !m.modifiers.iter().any(|x| x == "default" || x == "static")
}

/// The `:name` parameters of a query, by byte offset; SpEL, quotes and `::` casts skipped.
fn named_parameters(q: &str) -> Vec<(usize, &str)> {
    let b = q.as_bytes();
    let mut out = Vec::new();
    let mut quote = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match (quote, c) {
            (Some(open), _) => {
                if c == open {
                    quote = None;
                }
            }
            (None, b'\'' | b'"') => quote = Some(c),
            (None, b'#') if b.get(i + 1) == Some(&b'{') => {
                i += b[i..].iter().position(|&c| c == b'}').unwrap_or(b.len() - i);
            }
            (None, b':') if b.get(i + 1) == Some(&b':') => i += 1,
            (None, b':') => {
                let start = i + 1;
                let len = b[start..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_').count();
                if len > 0 && !b[start].is_ascii_digit() {
                    out.push((start, &q[start..start + len]));
                }
                i = start + len;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// What a path of properties led to.
enum Walk {
    Type(String),
    /// A type on the way isn't a project class.
    Unknown,
    /// The segment at this index isn't a property.
    Missing(usize, String),
}

fn walk(model: &Model, ty: &str, segments: &[&str]) -> Walk {
    let mut ty = ty.to_owned();
    for (i, segment) in segments.iter().enumerate() {
        let Some(props) = model.props(&ty) else { return Walk::Unknown };
        let Some((_, prop)) = props.into_iter().find(|(_, p)| p.name == *segment) else {
            return Walk::Missing(i, ty);
        };
        ty = prop.ty.clone();
    }
    Walk::Type(ty)
}

/// The type of each identification variable (lower-cased) of a query's declarations.
fn alias_types(model: &Model, declarations: &[jpql::Decl]) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut conflicting = Vec::new();
    for decl in declarations {
        let Some(alias) = decl.alias else { continue };
        let ty = if decl.is_path() {
            let segments: Vec<&str> = decl.target.s.split('.').collect();
            let Some(root) = out.get(&segments[0].to_ascii_lowercase()) else { continue };
            match walk(model, root, &segments[1..]) {
                Walk::Type(ty) => ty,
                _ => continue,
            }
        } else {
            model.entities.get(decl.target.s).map_or_else(|| decl.target.s.to_owned(), |c| c.name.clone())
        };
        let key = alias.to_ascii_lowercase();
        if out.get(&key).is_some_and(|known| *known != ty) {
            conflicting.push(key);
            continue;
        }
        out.insert(key, ty);
    }
    for key in conflicting {
        out.remove(&key);
    }
    out
}

/// A problem of a JPQL query: its bytes in the query and the diagnostic code and message.
struct QueryProblem {
    from: usize,
    to: usize,
    code: &'static str,
    message: String,
}

/// Unknown entities and unknown properties on identification variables of a project entity.
fn query_problems(model: &Model, query: &str, domain_in_project: bool) -> Vec<QueryProblem> {
    let mut out = Vec::new();
    if !model.complete {
        return out;
    }
    let words = jpql::words(query);
    let declarations = jpql::declarations(&words);
    if domain_in_project {
        for decl in declarations.iter().filter(|d| !d.is_path()) {
            if model.entities.contains_key(decl.target.s) || model.class(decl.target.s).is_some() {
                continue;
            }
            let Some(known) = model.near_miss(decl.target.s) else { continue };
            let message = format!("`{}` is not an entity of this project. Did you mean `{known}`?", decl.target.s);
            out.push(QueryProblem { from: decl.target.at, to: decl.target.at + decl.target.s.len(), code: "unknown-query-entity", message });
        }
    }
    let aliases = alias_types(model, &declarations);
    for word in words.iter().filter(|w| w.is_name() && w.s.contains('.')) {
        let segments: Vec<&str> = word.s.split('.').collect();
        let Some(root) = aliases.get(&segments[0].to_ascii_lowercase()) else { continue };
        let rest: Vec<&str> = segments[1..].iter().copied().take_while(|s| *s != "class" && !s.is_empty()).collect();
        if let Walk::Missing(i, ty) = walk(model, root, &rest) {
            let from = word.at + segments[..=i].iter().map(|s| s.len() + 1).sum::<usize>();
            out.push(QueryProblem { from, to: from + rest[i].len(), code: "unknown-query-property", message: format!("`{ty}` has no property `{}`.", rest[i]) });
        }
    }
    out
}

fn warning(text: &str, from: usize, to: usize, code: &str, message: String) -> Diagnostic {
    Diagnostic::new(range(text, from, to), Severity::Warning, message).code(code)
}

/// The problems of the repositories of a file.
fn problems(model: &Model, text: &str, file: &JavaFile) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (ti, domain) in repositories(file) {
        for m in file.types[ti].methods() {
            if let Some(query) = m.annotation("Query") {
                let names: Vec<String> = m
                    .params
                    .iter()
                    .map(|p| {
                        let bound = p.annotation("Param").and_then(|a| a.strings(&["value"]).first().map(|s| s.value.clone()));
                        bound.unwrap_or_else(|| p.name.clone())
                    })
                    .collect();
                for s in query.strings(&["value"]) {
                    if s.value.trim_start().starts_with(['{', '[']) {
                        continue; // a JSON query (MongoDB, Elasticsearch)
                    }
                    let exact = s.value.len() == s.end - s.at;
                    let native = query.arg("nativeQuery").is_some_and(|a| a.text.trim() == "true");
                    let single_literal = query.arg("value").is_none_or(|a| a.strings.len() == 1 && !a.text.contains('+'));
                    if !native && single_literal {
                        for p in query_problems(model, &s.value, model.class(&domain).is_some()) {
                            let (from, to) = if exact { (s.at + p.from, s.at + p.to) } else { (s.at, s.end) };
                            out.push(warning(text, from, to, p.code, p.message));
                        }
                    }
                    for (at, name) in named_parameters(&s.value) {
                        if names.iter().any(|n| n == name) {
                            continue;
                        }
                        let (from, to) = if exact { (s.at + at - 1, s.at + at + name.len()) } else { (s.at, s.end) };
                        let message = format!("No method parameter is bound to `:{name}`. Name a parameter `{name}` or annotate it with @Param(\"{name}\").");
                        out.push(warning(text, from, to, "unbound-query-parameter", message));
                    }
                }
            }
            let delete = m.name.starts_with("delete") || m.name.starts_with("remove");
            if let Some(a) = m.annotation("Modifying")
                && !m.has(&["Query", "NativeQuery"])
                && !(delete && derived_method(m))
            {
                out.push(warning(text, a.at, a.end, "modifying-without-query", "@Modifying only applies to @Query methods.".into()));
            }
            if !derived_method(m) {
                continue;
            }
            let Some(parts) = derived(&m.name) else { continue };
            for part in &parts {
                if let Resolved::Missing { from, to, name, ty } = resolve(model, &domain, &m.name, part.from, part.to) {
                    out.push(warning(text, m.at + from, m.at + to, "unknown-property", format!("`{ty}` has no property `{name}`.")));
                }
            }
            let expected: Option<usize> = parts.iter().map(|p| p.args).sum();
            let declared = m.params.iter().filter(|p| !SPECIAL_PARAMS.contains(&p.ty.name.as_str())).count();
            if let Some(expected) = expected
                && expected != declared
            {
                let message = format!("`{}` needs {expected} argument(s) for its criteria but declares {declared}.", m.name);
                out.push(warning(text, m.at, m.at + m.name.len(), "query-argument-count", message));
            }
        }
    }
    out
}

pub struct DataTools {
    classes: Indexes<Vec<Class>>,
}

impl Default for DataTools {
    fn default() -> Self {
        Self { classes: Indexes::new(SOURCE_GLOBS, classes) }
    }
}

impl DataTools {
    fn index(&mut self, doc: &Document) -> Option<&Index<Vec<Class>>> {
        self.classes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).map(|i| &*i)
    }
}

/// The repository whose body holds byte `at`, and its domain type.
fn repository_at(file: &JavaFile, at: usize) -> Option<(&TypeDecl, String)> {
    repositories(file).into_iter().rev().map(|(i, d)| (&file.types[i], d)).find(|(t, _)| t.start <= at && at <= t.end)
}

/// Property names of `ty` capitalized, plus one level of nested ones (`AddressCity`).
fn completions(model: &Model, ty: &str) -> Vec<(String, String)> {
    let capitalize = |s: &str| {
        let mut chars = s.chars();
        chars.next().map(|c| c.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
    };
    let mut out = Vec::new();
    for (_, p) in model.props(ty).unwrap_or_default() {
        if p.name.is_empty() {
            continue;
        }
        let head = capitalize(&p.name);
        out.push((head.clone(), p.ty.clone()));
        for (_, n) in model.props(&p.ty).unwrap_or_default() {
            if !n.name.is_empty() {
                out.push((format!("{head}{}", capitalize(&n.name)), n.ty.clone()));
            }
        }
    }
    out
}

/// Where the part being typed starts in a predicate, and whether it is in the `OrderBy` clause.
fn tail_start(pred: &str) -> (usize, bool) {
    let (mut start, mut order) = (0, false);
    let word_at = |i: usize, kw: &str| {
        pred.is_char_boundary(i)
            && pred[i..].starts_with(kw)
            && pred[i + kw.len()..].chars().next().is_none_or(char::is_uppercase)
    };
    for i in 0..pred.len() {
        if i < start {
            continue;
        }
        if !order && word_at(i, "OrderBy") {
            (start, order) = (i + "OrderBy".len(), true);
            continue;
        }
        let separators: &[&str] = if order { &["Asc", "Desc"] } else { &["And", "Or"] };
        if let Some(kw) = separators.iter().find(|kw| i > start && word_at(i, kw)) {
            start = i + kw.len();
        }
    }
    (start, order)
}

impl DataTools {
    /// Entity names after `from`/`join` and properties after `alias.` inside a `@Query` string.
    fn query_completion(&mut self, doc: &Document, off: usize) -> Vec<CompletionItem> {
        let file = parse_source(&doc.path, &doc.text);
        let Some((t, _)) = repository_at(&file, off) else { return Vec::new() };
        let Some(query) = t.methods().filter_map(|m| m.annotation("Query")).find(|a| a.at <= off && off <= a.end) else {
            return Vec::new();
        };
        if query.arg("nativeQuery").is_some_and(|a| a.text.trim() == "true") {
            return Vec::new();
        }
        let Some(s) = query.strings(&["value"]).into_iter().find(|s| s.holds(off)) else { return Vec::new() };
        if s.value.len() != s.end - s.at {
            return Vec::new();
        }
        let cursor = off - s.at;
        let typed_start = s.value[..cursor]
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '$' | '.')))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let typed = &s.value[typed_start..cursor];
        let Some(index) = self.index(doc) else { return Vec::new() };
        let model = Model::new(index);
        let item = |label: &str, detail: &str, kind, from: usize| CompletionItem {
            detail: Some(detail.to_owned()),
            replace: Some(range(&doc.text, from, off)),
            ..CompletionItem::new(label, kind)
        };
        let Some((head, _)) = typed.rsplit_once('.') else {
            let after_from = jpql::words(&s.value[..typed_start]).last().is_some_and(|w| w.is("from") || w.is("join") || w.is("update"));
            if !after_from {
                return Vec::new();
            }
            return model.entities.iter().map(|(name, c)| item(name, &c.name, CompletionKind::Class, s.at + typed_start)).collect();
        };
        let words = jpql::words(&s.value);
        let aliases = alias_types(&model, &jpql::declarations(&words));
        let segments: Vec<&str> = head.split('.').collect();
        let Some(root) = aliases.get(&segments[0].to_ascii_lowercase()) else { return Vec::new() };
        let Walk::Type(ty) = walk(&model, root, &segments[1..]) else { return Vec::new() };
        let from = s.at + typed_start + head.len() + 1;
        let mut seen = Vec::new();
        let mut items = Vec::new();
        for (_, p) in model.props(&ty).unwrap_or_default() {
            if !p.name.is_empty() && !seen.contains(&&p.name) {
                seen.push(&p.name);
                items.push(item(&p.name, &p.ty, CompletionKind::Field, from));
            }
        }
        items
    }
}

impl Providers for DataTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let file = parse_source(&doc.path, &doc.text);
        let repos = repositories(&file);
        let repository_types: Vec<usize> = repos.iter().map(|(ti, _)| *ti).collect();
        let found: Vec<Diagnostic> =
            checks::check(&file, &repository_types, checks::Groups::from_settings()).iter().map(|f| f.diagnostic(&doc.text)).collect();
        if repos.is_empty() {
            let _ = host::publish_diagnostics(&doc.path, &found);
            return Vec::new();
        }
        let Some(index) = self.index(doc) else { return Vec::new() };
        let model = Model::new(index);
        let mut diagnostics = problems(&model, &doc.text, &file);
        diagnostics.extend(found);
        let _ = host::publish_diagnostics(&doc.path, &diagnostics);
        let mut markers = Vec::new();
        for (ti, domain) in repos {
            let t = &file.types[ti];
            let line = range(&doc.text, t.at, t.at).start.line;
            if let Some((path, c)) = model.class(&domain) {
                let target = Some(Location { path: path.into(), range: c.range });
                markers.push(LineMarker { line, icon: None, tooltip: format!("Domain type: {domain}"), target });
            }
            let custom = format!("{}Impl", t.name);
            if let Some((path, c)) = model.class(&custom) {
                let target = Some(Location { path: path.into(), range: c.range });
                markers.push(LineMarker { line, icon: Some("implemented".into()), tooltip: format!("Custom implementation: {custom}"), target });
            }
        }
        markers
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(off) = offset(&doc.text, at) else { return Vec::new() };
        let file = parse_source(&doc.path, &doc.text);
        let Some((t, domain)) = repository_at(&file, off) else { return Vec::new() };
        let Some(m) = t.methods().find(|m| m.at <= off && off <= m.at + m.name.len() && derived_method(m)) else {
            return Vec::new();
        };
        let Some(parts) = derived(&m.name) else { return Vec::new() };
        let Some(index) = self.index(doc) else { return Vec::new() };
        let model = Model::new(index);
        let rel = off - m.at;
        for part in parts.iter().filter(|p| p.from <= rel && rel <= p.to) {
            if let Resolved::Found(steps) = resolve(&model, &domain, &m.name, part.from, part.to)
                && let Some((_, _, path, p)) = steps.into_iter().find(|s| s.0 <= rel && rel <= s.1)
            {
                return vec![Location { path: path.into(), range: p.range }];
            }
        }
        Vec::new()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(off) = offset(&doc.text, at) else { return Vec::new() };
        let queried = self.query_completion(doc, off);
        if !queried.is_empty() {
            return queried;
        }
        let start = doc.text[..off]
            .char_indices()
            .rev()
            .find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '$'))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let word = &doc.text[start..off];
        let Some(pred_at) = predicate_start(word) else { return Vec::new() };
        let file = parse_source(&doc.path, &doc.text);
        let Some((_, domain)) = repository_at(&file, off) else { return Vec::new() };
        let Some(index) = self.index(doc) else { return Vec::new() };
        let model = Model::new(index);
        let pred = &word[pred_at..];
        let (tail, order) = tail_start(pred);
        let head = &word[..pred_at + tail];
        let typed = &pred[tail..];
        let props = completions(&model, &domain);
        let replace = Some(range(&doc.text, start, off));
        let item = |label: String, detail: &str, kind| CompletionItem {
            detail: Some(detail.to_owned()),
            replace,
            ..CompletionItem::new(&label, kind)
        };
        let mut items: Vec<CompletionItem> =
            props.iter().map(|(p, ty)| item(format!("{head}{p}"), ty, CompletionKind::Field)).collect();
        if let Some((p, _)) = props.iter().filter(|(p, _)| typed.starts_with(p.as_str())).max_by_key(|(p, _)| p.len()) {
            let keywords: &[&str] = if order { &["Asc", "Desc"] } else { &CONTINUATIONS };
            items.extend(keywords.iter().map(|k| item(format!("{head}{p}{k}"), k, CompletionKind::Keyword)));
        }
        items
    }
}

impl Plugin for DataTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(DataTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const BASE: &str = r#"package com.acme;

@MappedSuperclass
public abstract class BaseEntity {
    @Id private Long id;
    private Instant createdAt;
}
"#;
    const USER: &str = r#"package com.acme;

@Entity
public class User extends BaseEntity {
    private String firstName, lastName;
    private int age;
    private boolean active;
    @Embedded private Address address;
    @ManyToOne private Owner owner;
    @OneToMany private List<Order> orders;
    @Transient private String display;
    private static final long serialVersionUID = 1L;

    public String getFullName() { return firstName + " " + lastName; }
    @Transient public String getDisplay() { return display; }
}

@Embeddable
class Address {
    private String city;
    private String zipCode;
}

record Order(String number, BigDecimal total) {}
"#;
    const REPO: &str = r#"package com.acme;

public interface UserRepository extends JpaRepository<User, Long>, UserRepositoryCustom {
    List<User> findByFirstNameAndLastNameAllIgnoreCase(String first, String last);
    List<User> findDistinctTop3ByAddressZipCodeOrderByAgeDesc(String zip);
    Page<User> findByAgeBetween(int from, int to, Pageable page);
    long countByActiveTrue();
    List<User> findByOrdersNumber(String number);
    List<User> findByAddress_City(String city);
    List<User> findByCreatedAtAfter(Instant at);
    Optional<User> findByEmial(String email);
    List<User> findByAgeGreaterThan();
    List<User> findByAddressStreet(String street);
    List<User> findByDisplay(String d);
    List<User> findByFullNameAndLastName(String full, String last);

    @Query("select u from User u where u.age > :min and u.firstName = :name and u.id = :#{#user.x} and cast(u.age as text)::text = ':nope'")
    List<User> adults(@Param("min") int min, String name);

    @Query("select u from User u where u.lastName = :last")
    List<User> byLast(String name);

    @Modifying
    @Query("update User u set u.active = false")
    void deactivate();

    @Modifying
    void touch();

    default List<User> helper() { return findByAgeGreaterThan(); }
}

@RepositoryDefinition(domainClass = Unknown.class, idClass = Long.class)
interface OtherRepository {
    List<Unknown> findByWhatever(String w);
}
"#;
    const CUSTOMER: &str = "package com.acme;\n@Entity\nclass Customer {\n    private String name;\n}\n";
    const IMPL: &str = "package com.acme;\nclass UserRepositoryImpl implements UserRepositoryCustom {}\n";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/BaseEntity.java", BASE);
        host.add_file("src/main/java/com/acme/User.java", USER);
        host.add_file("src/main/java/com/acme/UserRepositoryImpl.java", IMPL);
        host.add_file("src/main/java/com/acme/Customer.java", CUSTOMER);
        host
    }

    fn doc(text: &str) -> Document {
        Document { path: "/project/src/main/java/com/acme/UserRepository.java".into(), language: "java".into(), version: 1, text: text.into() }
    }

    fn parts(name: &str) -> Vec<(&str, Option<usize>)> {
        derived(name).unwrap().into_iter().map(|p| (&name[p.from..p.to], p.args)).collect()
    }

    #[test]
    fn parses_derived_query_names() {
        assert_eq!(parts("findByFirstNameAndLastNameAllIgnoreCase"), [("FirstName", Some(1)), ("LastName", Some(1))]);
        assert_eq!(parts("findDistinctTop3ByAddressZipCodeOrderByAgeDescNameAsc"), [
            ("AddressZipCode", Some(1)),
            ("Age", Some(0)),
            ("Name", Some(0))
        ]);
        assert_eq!(parts("countByActiveTrueOrAgeIsNotNull"), [("Active", Some(0)), ("Age", Some(0))]);
        assert_eq!(parts("findByAgeBetweenAndNameContainingIgnoreCase"), [("Age", Some(2)), ("Name", Some(1))]);
        assert_eq!(parts("findByOrderDateAndOrigin"), [("OrderDate", Some(1)), ("Origin", Some(1))]);
        assert_eq!(parts("findTopByOrderByAgeDesc"), [("Age", Some(0))]);
        assert_eq!(parts("findByLocationNear"), [("Location", None)]);
        assert_eq!(derived("findAll"), None);
        assert_eq!(derived("getter"), None);
        assert_eq!(derived("findById"), None);
        assert_eq!(derived("save"), None);
        assert_eq!(named_parameters("a = :x and b = :#{#y} and ':z' and c::text and :1 and :y_2"), [(5, "x"), (55, "y_2")]);
    }

    #[test]
    fn checks_derived_queries_and_query_annotations() {
        let host = setup();
        let mut tools = DataTools::default();
        tools.line_markers(&doc(REPO));
        let problems = &host.diagnostics()["/project/src/main/java/com/acme/UserRepository.java"];
        let found: Vec<_> = problems
            .iter()
            .map(|d| (d.code.as_deref().unwrap(), &REPO[offset(REPO, d.range.start).unwrap()..offset(REPO, d.range.end).unwrap()]))
            .collect();
        assert_eq!(found, [
            ("unknown-property", "Emial"),
            ("query-argument-count", "findByAgeGreaterThan"),
            ("unknown-property", "AddressStreet"),
            ("unknown-property", "Display"),
            ("unbound-query-parameter", ":last"),
            ("modifying-without-query", "@Modifying"),
        ]);
        assert_eq!(problems[0].message, "`User` has no property `emial`.");
        assert_eq!(problems[2].message, "`User` has no property `addressStreet`.");
    }

    #[test]
    fn links_repository_to_entity_and_properties() {
        let _host = setup();
        let mut tools = DataTools::default();
        let markers = tools.line_markers(&doc(REPO));
        let targets: Vec<_> =
            markers.iter().map(|m| (m.line, m.tooltip.as_str(), m.target.as_ref().unwrap().path.rsplit('/').next().unwrap())).collect();
        assert_eq!(targets, [
            (2, "Domain type: User", "User.java"),
            (2, "Custom implementation: UserRepositoryImpl", "UserRepositoryImpl.java")
        ]);

        let line = REPO.lines().position(|l| l.contains("findDistinctTop3")).unwrap() as u32;
        let col = REPO.lines().nth(line as usize).unwrap().find("ZipCode").unwrap() as u32 + 2;
        let found = tools.definition(&doc(REPO), Position::new(line, col));
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("User.java"));
        assert_eq!(&USER.lines().nth(found[0].range.start.line as usize).unwrap()[found[0].range.start.character as usize..], "zipCode;");

        let line = REPO.lines().position(|l| l.contains("findByCreatedAtAfter")).unwrap() as u32;
        let found = tools.definition(&doc(REPO), Position::new(line, 23));
        assert!(found[0].path.ends_with("BaseEntity.java"), "inherited from the mapped superclass");
    }

    #[test]
    fn completes_derived_query_names() {
        let _host = setup();
        let mut tools = DataTools::default();
        let typing = REPO.replace("    long countByActiveTrue();", "    List<User> findByFirstNameAndAdd");
        let line = typing.lines().position(|l| l.contains("findByFirstNameAndAdd")).unwrap() as u32;
        let items = tools.completion(&doc(&typing), Position::new(line, 36), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"findByFirstNameAndAddress"));
        assert!(labels.contains(&"findByFirstNameAndAddressZipCode"));
        assert!(labels.contains(&"findByFirstNameAndId"));
        assert!(!labels.iter().any(|l| l.ends_with("Display")));
        assert_eq!(items[0].replace.unwrap().start, Position::new(line, 15));

        let typing = REPO.replace("    long countByActiveTrue();", "    long countByAge");
        let items = tools.completion(&doc(&typing), Position::new(line, 19), None);
        assert!(items.iter().any(|i| i.label == "countByAgeGreaterThan"));
        let typing = REPO.replace("    long countByActiveTrue();", "    long countByAgeOrderByAge");
        let items = tools.completion(&doc(&typing), Position::new(line, 29), None);
        assert!(items.iter().any(|i| i.label == "countByAgeOrderByAgeDesc"));
        assert!(tools.completion(&doc(REPO), Position::new(0, 3), None).is_empty());
    }

    const QUERY_REPO: &str = r#"package com.acme;

public interface UserRepository extends JpaRepository<User, Long> {
    @Query("select u from Custmer u where u.age > 1")
    List<User> unknownEntity();

    @Query("select u from User u join u.address a where u.address.citty = 'x' and a.zipCode = '1' and u.nme = 2")
    List<User> unknownProperties();

    @Query("select u from User u join u.orders o where o.number = '1' and o.total > 1 and u.id = 1 and u.createdAt is null")
    List<User> fine();

    @Query("select u from User u join u.owner o where o.whatever = 1 and u.owner.name.first = 1")
    List<User> libraryTypes();
}
"#;

    fn query_diagnostics(text: &str) -> Vec<(String, String)> {
        let host = setup();
        let mut tools = DataTools::default();
        tools.line_markers(&doc(text));
        let problems = host.diagnostics().get("/project/src/main/java/com/acme/UserRepository.java").cloned().unwrap_or_default();
        problems
            .iter()
            .map(|d| (d.code.clone().unwrap(), text[offset(text, d.range.start).unwrap()..offset(text, d.range.end).unwrap()].to_owned()))
            .collect()
    }

    #[test]
    fn query_unknown_entity_warns() {
        let found = query_diagnostics(QUERY_REPO);
        assert_eq!(found[0], ("unknown-query-entity".to_owned(), "Custmer".to_owned()));
    }

    #[test]
    fn query_unknown_property_warns() {
        let found = query_diagnostics(QUERY_REPO);
        let codes: Vec<_> = found.iter().map(|(c, t)| (c.as_str(), t.as_str())).collect();
        assert_eq!(codes, [
            ("unknown-query-entity", "Custmer"),
            ("unknown-query-property", "citty"),
            ("unknown-query-property", "nme"),
        ]);
    }

    #[test]
    fn query_property_of_library_entity_is_quiet() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from User u join u.owner o where o.whatever = 1 and u.owner.name.first = 1\")\n    List<User> q();\n}\n";
        let found = query_diagnostics(text);
        assert!(found.is_empty(), "{found:?}");
        let library = "package com.acme;\ninterface AccountRepository extends JpaRepository<Account, Long> {\n    @Query(\"select a from Acct a where a.nope = 1\")\n    List<Account> q();\n}\n";
        assert!(query_diagnostics(library).is_empty());
    }

    #[test]
    fn short_names_are_not_near_misses() {
        assert!(!is_near_miss("Rule", "Role") && !is_near_miss("A", "B") && !is_near_miss("Usr", "User"));
        assert!(is_near_miss("Customer", "Custmer") && is_near_miss("User", "user"));
        assert!(!is_near_miss("Customer", "Cstmr") && is_near_miss("CustomerOrder", "CustmerOrdr"));
    }

    #[test]
    fn incomplete_index_stays_quiet() {
        let mut index: Index<Vec<Class>> = Index::new(SOURCE_GLOBS, classes);
        index.update("/p/Customer.java", CUSTOMER);
        let model = Model::new(&index);
        assert!(!model.complete);
        assert!(query_problems(&model, "select c from Custmer c where c.nme = 1", true).is_empty());
        let mut complete = Model::new(&index);
        complete.complete = true;
        assert_eq!(query_problems(&complete, "select c from Custmer c where c.nme = 1", true).len(), 1);
    }

    #[test]
    fn subquery_alias_same_type_still_checked() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from User u where u.age > 1 and exists (select 1 from User u where u.nme = 2)\")\n    List<User> q();\n}\n";
        let found: Vec<_> = query_diagnostics(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, ["nme"]);
    }

    #[test]
    fn kotlin_raw_string_query_is_checked() {
        let host = setup();
        host.add_file("src/main/kotlin/com/acme/Person.kt", "package com.acme\n\n@Entity\nclass Person(@Id val id: Long, var name: String)\n");
        let repo = "package com.acme\n\ninterface PersonRepository : JpaRepository<Person, Long> {\n    @Query(\"\"\"\n        select p from Person p\n        where p.nmae = :name\n    \"\"\")\n    fun byName(name: String): List<Person>\n}\n";
        let kt = Document { path: "/project/src/main/kotlin/com/acme/PersonRepository.kt".into(), language: "kotlin".into(), version: 1, text: repo.into() };
        let mut tools = DataTools::default();
        tools.line_markers(&kt);
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/PersonRepository.kt"];
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "`Person` has no property `nmae`.");
    }

    const QUIET_REPO: &str = r#"package com.acme;

public interface UserRepository extends JpaRepository<User, Long> {
    @Query(value = "select * from Custmer u where u.nope = 1", nativeQuery = true)
    List<User> nativeOne();

    @NativeQuery("select * from Custmer u where u.nope = 1")
    List<User> nativeTwo();

    @Query("select u from User u where u.age = :#{u.nope}")
    List<User> spel(String a);

    @Query("select u from Custmer u " + "where u.age = 1")
    List<User> concatenated();

    @Query("select u from User u join AuditEvent e on e.user = u where e.kind = 1")
    List<User> unrelatedEntity();

    @Query("select u from User u where u.age > 1 and exists (select 1 from Order u where u.number = '1')")
    List<User> aliasReusedInSubquery();
}
"#;

    #[test]
    fn query_native_spel_and_concatenation_are_quiet() {
        let found = query_diagnostics(QUIET_REPO);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn query_unrelated_entity_name_is_quiet() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from User u, AuditEvent e where e.kind = 1\")\n    List<User> q();\n}\n";
        assert!(query_diagnostics(text).is_empty());
    }

    #[test]
    fn function_from_is_not_an_entity() {
        let words = jpql::words("select extract(year from current_date), trim(both from n), substring(x from 2) from User u where exists (select 1 from Order o)");
        let targets: Vec<_> = jpql::declarations(&words).into_iter().map(|d| d.target.s).collect();
        assert_eq!(targets, ["User", "Order"]);
    }

    #[test]
    fn query_joins_resolve_fields_of_other_entities() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from User u join Order o on o.number = u.firstName where o.nmbr = 1\")\n    List<User> a();\n    @Query(\"select u from User u join fetch u.orders o where o.totl = 1 and o.total = 2\")\n    List<User> b();\n}\n";
        let found: Vec<_> = query_diagnostics(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, ["nmbr", "totl"]);
    }

    #[test]
    fn kotlin_query_is_checked() {
        let host = setup();
        host.add_file("src/main/kotlin/com/acme/Person.kt", "package com.acme\n\n@Entity\nclass Person(@Id val id: Long, var name: String)\n");
        let repo = "package com.acme\n\ninterface PersonRepository : JpaRepository<Person, Long> {\n    @Query(\"select p from Person p where p.nmae = :name\")\n    fun byName(name: String): List<Person>\n}\n";
        let kt = Document { path: "/project/src/main/kotlin/com/acme/PersonRepository.kt".into(), language: "kotlin".into(), version: 1, text: repo.into() };
        let mut tools = DataTools::default();
        tools.line_markers(&kt);
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/PersonRepository.kt"];
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "`Person` has no property `nmae`.");
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = setup();
        let path = "/project/src/main/java/com/acme/Person.java";
        let bad = "import org.springframework.data.relational.core.mapping.Sequence;\nclass Person {\n    @Sequence(sequence = \"\")\n    Long id;\n}\n";
        let mut tools = DataTools::default();
        let doc = |text: &str| Document { path: path.into(), language: "java".into(), version: 1, text: text.into() };
        tools.line_markers(&doc(bad));
        assert_eq!(host.diagnostics()[path].len(), 1);
        tools.line_markers(&doc(&bad.replace("\"\"", "\"person_seq\"")));
        assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn repository_findings_are_published_with_problems() {
        let host = setup();
        let path = "/project/src/main/java/com/acme/UserRepository.java";
        let text = "package com.acme;\n\nimport org.springframework.data.domain.Page;\n\ninterface UserRepository extends JpaRepository<User, Long> {\n    Page<User> findByFirstName(String firstName);\n}\n";
        let mut tools = DataTools::default();
        tools.line_markers(&Document { path: path.into(), language: "java".into(), version: 1, text: text.into() });
        let codes: Vec<_> = host.diagnostics()[path].iter().map(|d| d.code.clone()).collect();
        assert_eq!(codes, [Some("page-without-pageable".to_owned())]);
    }

    fn complete_in(text: &str, marker: &str) -> Vec<CompletionItem> {
        let _host = setup();
        let mut tools = DataTools::default();
        let cleaned = text.replace(marker, "");
        let off = text.find(marker).unwrap();
        let before = &text[..off];
        let line = before.matches('\n').count() as u32;
        let col = (off - before.rfind('\n').map_or(0, |i| i + 1)) as u32;
        tools.completion(&doc(&cleaned), Position::new(line, col), None)
    }

    #[test]
    fn query_alias_property_completes() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from User u where u.address.CARET\")\n    List<User> q();\n}\n";
        let labels: Vec<_> = complete_in(text, "CARET").into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["city", "zipCode"]);
        let text = text.replace("u.address.CARET", "u.CARET");
        let labels: Vec<_> = complete_in(&text, "CARET").into_iter().map(|i| i.label).collect();
        assert!(labels.contains(&"firstName".to_owned()) && labels.contains(&"createdAt".to_owned()) && labels.contains(&"fullName".to_owned()));
        assert!(!labels.contains(&"display".to_owned()));
    }

    #[test]
    fn query_entity_names_complete_after_from_and_join() {
        let text = "package com.acme;\ninterface UserRepository extends JpaRepository<User, Long> {\n    @Query(\"select u from CARET\")\n    List<User> q();\n}\n";
        let labels: Vec<_> = complete_in(text, "CARET").into_iter().map(|i| i.label).collect();
        assert_eq!(labels, ["Customer", "User"]);
    }

    #[test]
    fn kotlin_repository_derives_entity() {
        let host = setup();
        host.add_file("src/main/kotlin/com/acme/Person.kt", "package com.acme\n\n@Entity\nclass Person(@Id val id: Long, var name: String, var age: Int)\n");
        let repo = "package com.acme\n\ninterface PersonRepository : JpaRepository<Person, Long> {\n    fun findByName(name: String): List<Person>\n    fun findByNmae(name: String): List<Person>\n}\n";
        let kt = Document { path: "/project/src/main/kotlin/com/acme/PersonRepository.kt".into(), language: "kotlin".into(), version: 1, text: repo.into() };
        let mut tools = DataTools::default();
        let markers = tools.line_markers(&kt);
        assert_eq!(markers[0].tooltip, "Domain type: Person");
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/PersonRepository.kt"];
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].message, "`Person` has no property `nmae`.");
    }
}
