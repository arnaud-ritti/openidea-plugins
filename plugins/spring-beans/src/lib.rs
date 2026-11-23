//! Spring Beans Navigator for OpenIDEA: indexes the beans a project's Java,
//! Kotlin and Spring XML files declare and links injection points, bean-name
//! strings, SpEL `@bean` references and named pointcuts to them.
//! Text-based: types match by simple name through the project's own type
//! hierarchy; beans from libraries and auto-configuration aren't seen.

mod annotation_checks;
mod annotation_owners;
mod aop;
mod cache;
mod effective;
mod checks;
mod commands;
mod hover;
mod property_file_actions;
mod xml_create;
mod indexer;
mod injection_checks;
mod kotlin;
mod kotlin_build;
mod live_templates;
mod model;
mod resources;
mod search;
mod sites;
mod spel;
mod graph;
mod target_checks;
mod xml_actions;
mod xml_checks;
mod xml_class_checks;
mod xml_file_actions;
mod xml_namespace;

pub use model::{Bean, FileModel, Injection, TypeInfo, parse};

pub(crate) const BEAN_MARKER: &str = "icons/marker-bean.svg";

use cache::{Cache, Indexes};
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, LogLevel, Plugin, Position, Project, Range, Severity, TextEdit, host,
};
use sites::{Hit, Site, Word};
use spring_text::index::COMMAND_BUDGET;
use spring_text::java::{self, MemberKind};
use spring_text::tree::Expansion;
use spring_text::{contains, range};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;
use ui_kit::sections::{SectionDef, Switcher};

const GLOBS: &[&str] = &["**/*.java", "**/*.kt", "**/*.xml", "**/*.properties", "**/*.yml", "**/*.yaml", "**/*.sql"];
/// A model-free hint; the packaged page re-renders through its own scope.
fn refresh_beans_view() {
    host::refresh_view(BEANS_VIEW);
    let _ = host::post_view_message(BEANS_VIEW, r#"{"channel":"ui-kit-refresh"}"#);
}

const BEANS_VIEW: &str = "beans";
const SECTIONS: &[SectionDef] =
    &[SectionDef { id: "beans", label: "Beans" }, SectionDef { id: "graph", label: "Graph" }, SectionDef { id: "report", label: "Report" }];
const GRAPH_SECTION: &str = "graph";
const REPORT_SECTION: &str = "report";
const MAX_GRAPH: usize = 400;
const MAX_CLASSES: usize = 200;

/// What providers look up across the project, derived once per cache change:
/// beans by name and by type (active ones only), the type hierarchy both ways,
/// types by simple and qualified name, injection points by the type or name
/// they ask for, configuration literals, related-source links, and the
/// inactive beans.
pub(crate) struct Lookups {
    by_name: BTreeMap<String, Vec<(String, usize)>>,
    by_type: BTreeMap<String, Vec<(String, usize)>>,
    supertypes: BTreeMap<String, Vec<String>>,
    subtypes: BTreeMap<String, Vec<String>>,
    types_by_name: BTreeMap<String, Vec<(String, usize)>>,
    types_by_qualified: BTreeMap<String, Vec<(String, usize)>>,
    injections_by_type: BTreeMap<String, Vec<(String, usize)>>,
    injections_by_name: BTreeMap<String, Vec<(String, usize)>>,
    /// The value of each canonical key set once, to a plain literal, in base
    /// `application.*` files; `None` when repeated, profile-scoped or not plain.
    literals: BTreeMap<String, Option<String>>,
    /// `spring.config.import` appears: no literal can be trusted.
    config_imports: bool,
    /// `spring.profiles.include` or `spring.profiles.group` appears.
    profile_names_set: bool,
    resolver: ui_kit::navigation::Resolver,
    inactive: BTreeSet<(String, usize)>,
}

impl Lookups {
    fn build(files: &BTreeMap<String, FileModel>, complete: bool, modules: &RefCell<BTreeMap<String, Option<String>>>) -> Self {
        let mut lookups = Lookups {
            by_name: BTreeMap::new(),
            by_type: BTreeMap::new(),
            supertypes: BTreeMap::new(),
            subtypes: BTreeMap::new(),
            types_by_name: BTreeMap::new(),
            types_by_qualified: BTreeMap::new(),
            injections_by_type: BTreeMap::new(),
            injections_by_name: BTreeMap::new(),
            literals: BTreeMap::new(),
            config_imports: false,
            profile_names_set: false,
            resolver: ui_kit::navigation::Resolver::new(files.iter().map(|(p, m)| (p.as_str(), &m.navigation))),
            inactive: BTreeSet::new(),
        };
        let imports = spring_text::config::canonical("spring.config.import");
        let profile_names = [spring_text::config::canonical("spring.profiles.include"), spring_text::config::canonical("spring.profiles.group")];
        for config in files.values().filter_map(|m| m.config.as_ref()) {
            for (key, value) in &config.keys {
                let key = spring_text::config::canonical(key);
                lookups.config_imports |= key == imports;
                lookups.profile_names_set |= profile_names.iter().any(|n| key.starts_with(n.as_str()));
                let plain = !config.profile_scoped && !value.is_empty() && !value.contains("${");
                lookups.literals.entry(key).and_modify(|once| *once = None).or_insert(plain.then(|| value.clone()));
            }
        }
        for (path, model) in files {
            for (at, t) in model.types.iter().enumerate() {
                lookups.types_by_name.entry(t.name.clone()).or_default().push((path.clone(), at));
                lookups.types_by_qualified.entry(t.qualified.clone()).or_default().push((path.clone(), at));
                lookups.supertypes.entry(t.name.clone()).or_default().extend(t.supers.iter().cloned());
                for s in &t.supers {
                    lookups.subtypes.entry(s.clone()).or_default().push(t.name.clone());
                }
            }
            for (at, inj) in model.injections.iter().enumerate() {
                let key = (path.clone(), at);
                match (&inj.qualifier, inj.ty.is_empty()) {
                    (_, false) => lookups.injections_by_type.entry(inj.ty.clone()).or_default().push(key),
                    (Some(q), true) => lookups.injections_by_name.entry(q.clone()).or_default().push(key),
                    (None, true) => {}
                }
            }
        }
        let lookups = Rc::new(lookups);
        let inactive: BTreeSet<(String, usize)> = {
            let probe = World { files, lookups: lookups.clone(), complete, modules };
            files
                .iter()
                .flat_map(|(p, m)| m.beans.iter().enumerate().map(move |(at, b)| (p, at, b)))
                .filter(|(p, _, b)| probe.inactive_reason(p, b).is_some())
                .map(|(p, at, _)| (p.clone(), at))
                .collect()
        };
        let Ok(mut lookups) = Rc::try_unwrap(lookups) else { unreachable!("the probe world is gone") };
        for (path, model) in files {
            for (at, bean) in model.beans.iter().enumerate() {
                let key = (path.clone(), at);
                if inactive.contains(&key) {
                    continue;
                }
                let names: BTreeSet<&String> = std::iter::once(&bean.name).chain(&bean.aliases).chain(&bean.qualifiers).collect();
                for name in names {
                    lookups.by_name.entry(name.clone()).or_default().push(key.clone());
                }
                if !bean.ty.is_empty() {
                    lookups.by_type.entry(bean.ty.clone()).or_default().push(key);
                }
            }
        }
        lookups.inactive = inactive;
        lookups
    }
}

/// The project's models, with the lookups providers need.
struct World<'a> {
    files: &'a BTreeMap<String, FileModel>,
    lookups: Rc<Lookups>,
    /// Every file has been read at least once.
    complete: bool,
    /// The build module of each path asked so far, until a build file changes.
    modules: &'a RefCell<BTreeMap<String, Option<String>>>,
}

type Found<'a> = (&'a str, &'a Bean);

impl<'a> World<'a> {
    fn new(cache: &'a Cache) -> Self {
        let (files, complete, modules) = (cache.files(), cache.complete(), &cache.modules);
        let lookups = cache.lookups(|| Lookups::build(files, complete, modules));
        Self { files, lookups, complete, modules }
    }

    /// The bean at `key` of the lookups.
    fn found(&self, (path, at): &(String, usize)) -> Option<Found<'a>> {
        let (path, model) = self.files.get_key_value(path)?;
        Some((path.as_str(), model.beans.get(*at)?))
    }

    fn beans(&self) -> impl Iterator<Item = Found<'a>> + '_ {
        self.files.iter().flat_map(|(p, m)| m.beans.iter().map(move |b| (p.as_str(), b)))
    }

    /// Whether `ty` is `target` or one of its subtypes.
    fn is_a(&self, ty: &str, target: &str) -> bool {
        self.related(ty, &self.lookups.supertypes).contains(target)
    }

    /// `ty` and every type `links` reaches from it.
    fn related<'l>(&self, ty: &'l str, links: &'l BTreeMap<String, Vec<String>>) -> BTreeSet<&'l str> {
        let mut seen = BTreeSet::new();
        let mut todo = vec![ty];
        while let Some(t) = todo.pop() {
            if seen.insert(t) {
                todo.extend(links.get(t).into_iter().flatten().map(String::as_str));
            }
        }
        seen
    }

    /// Whether `a` and `b` share a source set and build module, reading each path's module once.
    fn same_scope(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if checks::is_test_source(a) != checks::is_test_source(b) {
            return false;
        }
        let module = |path: &str| {
            if let Some(known) = self.modules.borrow().get(path) {
                return known.clone();
            }
            let found = checks::build_module(path);
            self.modules.borrow_mut().insert(path.to_owned(), found.clone());
            found
        };
        match (module(a), module(b)) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        }
    }

    /// The type at `key` of the lookups.
    fn type_at(&self, (path, at): &(String, usize)) -> Option<&'a TypeInfo> {
        self.files.get(path)?.types.get(*at)
    }

    /// The one project type `keys` name, if exactly one.
    fn only_type(&self, keys: Option<&Vec<(String, usize)>>) -> Option<&'a TypeInfo> {
        match keys.map(Vec::as_slice) {
            Some([key]) => self.type_at(key),
            _ => None,
        }
    }

    /// The type `start` and its supertypes, when every one is a project type with all members listed.
    /// `qualified` pins the first one; nothing when a name is ambiguous or comes from a library.
    fn member_chain(&self, start: &str, qualified: Option<&str>) -> Option<Vec<&'a TypeInfo>> {
        const LIBRARY_MARKERS: [&str; 6] = ["Object", "Serializable", "Cloneable", "Comparable", "AutoCloseable", "Closeable"];
        let lookups = self.lookups.clone();
        let mut found: Vec<&TypeInfo> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut todo = vec![start.to_owned()];
        while let Some(name) = todo.pop() {
            if LIBRARY_MARKERS.contains(&name.as_str()) || !seen.insert(name.clone()) {
                continue;
            }
            let ty = self.only_type(lookups.types_by_name.get(&name))?;
            if !ty.members_known || (found.is_empty() && qualified.is_some_and(|q| q != ty.qualified)) {
                return None;
            }
            todo.extend(ty.supers.iter().cloned());
            found.push(ty);
        }
        Some(found)
    }

    fn project_type(&self, qualified: &str) -> Option<&'a TypeInfo> {
        let lookups = self.lookups.clone();
        self.only_type(lookups.types_by_qualified.get(qualified))
    }

    fn path_of(&self, ty: &TypeInfo) -> &'a str {
        let lookups = self.lookups.clone();
        let keys = lookups.types_by_qualified.get(&ty.qualified).into_iter().flatten();
        let owner = keys.filter(|key| self.type_at(key).is_some_and(|t| std::ptr::eq(t, ty))).find_map(|(path, _)| self.files.get_key_value(path));
        owner.map_or("", |(p, _)| p.as_str())
    }

    /// Whether a project type's qualified name starts with `prefix`.
    fn declares_under(&self, prefix: &str) -> bool {
        self.lookups.types_by_qualified.range(prefix.to_owned()..).next().is_some_and(|(q, _)| q.starts_with(prefix))
    }

    /// Whether a project type is qualified `qualified`, under any spelling count.
    fn declares(&self, qualified: &str) -> bool {
        self.lookups.types_by_qualified.contains_key(qualified)
    }

    /// Whether a project type extends or implements `name`.
    fn has_subtypes(&self, name: &str) -> bool {
        self.lookups.subtypes.contains_key(name)
    }

    /// The links between related sources of the cached files.
    fn resolver(&self) -> &ui_kit::navigation::Resolver {
        &self.lookups.resolver
    }

    /// The value of a string expression made of literals and `static final` project constants.
    fn constant(&self, expr: &str, own: &str) -> Option<String> {
        let toks = java::tokens(expr);
        let mut out = String::new();
        for operand in toks.split(|t| t.s == "+") {
            if let [single] = operand {
                if let Some(s) = single.string() {
                    out.push_str(&s.value);
                    continue;
                }
            }
            let dotted = operand.iter().enumerate().all(|(i, t)| if i % 2 == 0 { t.is_ident() } else { t.s == "." });
            if operand.is_empty() || !dotted || operand.len() % 2 == 0 {
                return None;
            }
            let path: String = operand.iter().map(|t| t.s).collect();
            let key = match path.rsplit_once('.') {
                Some((ty, name)) => format!("{}.{name}", ty.rsplit('.').next().unwrap_or(ty)),
                None => format!("{own}.{path}"),
            };
            let value = self.files.values().flat_map(|m| &m.constants).find(|(k, _)| *k == key)?;
            out.push_str(&value.1);
        }
        Some(out)
    }

    fn named(&self, name: &str) -> Vec<Found<'a>> {
        let lookups = self.lookups.clone();
        lookups.by_name.get(name).map_or_else(Vec::new, |keys| keys.iter().filter_map(|key| self.found(key)).collect())
    }

    /// The beans Spring would consider for `inj`: by type, then qualifier,
    /// `@Primary`, and the field or parameter name.
    fn candidates(&self, inj: &Injection) -> Vec<Found<'a>> {
        if inj.ty.is_empty() {
            return self.named(inj.qualifier.as_deref().unwrap_or(&inj.name));
        }
        let lookups = self.lookups.clone();
        let mut keys: Vec<&(String, usize)> =
            self.related(&inj.ty, &lookups.subtypes).into_iter().filter_map(|t| lookups.by_type.get(t)).flatten().collect();
        keys.sort();
        let typed: Vec<Found> = keys.into_iter().filter_map(|key| self.found(key)).collect();
        if let Some(q) = &inj.qualifier {
            return typed.into_iter().filter(|(_, b)| b.answers_to(q)).collect();
        }
        if inj.multiple || typed.len() < 2 {
            return typed;
        }
        let primary: Vec<Found> = typed.iter().copied().filter(|(_, b)| b.primary).collect();
        if !primary.is_empty() {
            return primary;
        }
        let by_name: Vec<Found> = typed.iter().copied().filter(|(_, b)| b.answers_to(&inj.name)).collect();
        if by_name.is_empty() { typed } else { by_name }
    }

    /// Where the bean declared at `path`/`range` is injected: (location, into what).
    fn users(&self, path: &str, bean: &Bean) -> Vec<(Location, String)> {
        let lookups = self.lookups.clone();
        let mut asking: BTreeSet<&(String, usize)> = BTreeSet::new();
        if !bean.ty.is_empty() {
            asking.extend(self.related(&bean.ty, &lookups.supertypes).into_iter().filter_map(|t| lookups.injections_by_type.get(t)).flatten());
        }
        for name in std::iter::once(&bean.name).chain(&bean.aliases).chain(&bean.qualifiers) {
            asking.extend(lookups.injections_by_name.get(name).into_iter().flatten());
        }
        let mut out = Vec::new();
        for (p, at) in asking {
            let Some((p, m)) = self.files.get_key_value(p) else { continue };
            let Some(inj) = m.injections.get(*at) else { continue };
            if self.candidates(inj).iter().any(|(cp, b)| *cp == path && b.range == bean.range) {
                let into = inj.owner.map_or_else(|| file_name(p).to_owned(), |o| m.beans[o].name.clone());
                out.push((Location { path: p.clone(), range: inj.range }, into));
            }
        }
        out
    }

    fn pointcuts(&self, name: &str, class: Option<&str>) -> Vec<Location> {
        self.files
            .iter()
            .flat_map(|(p, m)| m.pointcuts.iter().map(move |pc| (p, pc)))
            .filter(|(_, pc)| pc.name == name && class.is_none_or(|c| pc.class == c))
            .map(|(p, pc)| Location { path: p.clone(), range: pc.range })
            .collect()
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Markers of one line and icon join into one gutter button whose click opens the IDE's target popup.
const MAX_LINE_TARGETS: usize = 1000;

/// Says on the line's first marker that `total` targets were cut to the bound.
fn bounded(markers: &mut [LineMarker], total: usize) {
    if total <= MAX_LINE_TARGETS {
        return;
    }
    let first = &mut markers[markers.len() - MAX_LINE_TARGETS];
    first.tooltip = format!("{} ({MAX_LINE_TARGETS} of {total} shown)", first.tooltip);
}

fn describe(b: &Bean) -> String {
    let mut s = if b.kind == "@Bean" { format!("{}() (@Bean)", b.name) } else { format!("{} ({})", b.name, b.kind) };
    if b.primary {
        s += " primary";
    }
    if !b.profiles.is_empty() {
        s += &format!(" [{}]", b.profiles.join(", "));
    }
    s
}

fn field_injection_problems(model: &FileModel) -> Vec<Diagnostic> {
    model
        .injections
        .iter()
        .filter_map(|inj| inj.field_annotation)
        .map(|range| Diagnostic {
            code: Some("field-injection".into()),
            ..Diagnostic::new(
                range,
                Severity::Warning,
                "Field injection: prefer a constructor parameter, which lets the field be final and the class easier to test.",
            )
        })
        .collect()
}

/// Every problem the plugin finds in one source; without a `world` only what needs no other file.
fn problems_of(path: &str, language: &str, text: &str, world: Option<&World>) -> Vec<Diagnostic> {
    let groups = checks::Groups::from_settings();
    let diagnostics = |findings: Vec<spring_text::finding::Finding>| findings.iter().map(|f| f.diagnostic(text)).collect::<Vec<_>>();
    match language {
        "spring-meta" => world.map(|w| diagnostics(target_checks::spring_meta_findings(path, text, w))).unwrap_or_default(),
        "xml" => {
            let els = spring_text::xml::elements(text);
            let Some(world) = world.filter(|_| xml_checks::is_beans_file(&els)) else {
                return Vec::new();
            };
            let mut findings = xml_checks::check_xml(path, text, world, groups);
            findings.extend(xml_class_checks::check(&els, world, groups));
            diagnostics(findings)
        }
        _ => {
            let mut problems = field_injection_problems(&model::parse(path, text).unwrap_or_default());
            if let (Some(world), "java") = (world, language) {
                problems.extend(diagnostics(checks::check_java(path, &java::parse(text), text, world, groups)));
            }
            if language == "kotlin" && groups.kotlin_plugin {
                let file = spring_text::kotlin::parse(text);
                let mut findings = kotlin_build::missing_spring_plugin(path, &file);
                findings.extend(kotlin_build::redundant_open(path, &file, text));
                findings.extend(kotlin::router_path_variables(text));
                findings.extend(kotlin::reified_extension_hints(text));
                problems.extend(diagnostics(findings));
            }
            problems
        }
    }
}

fn hits(doc: &Document) -> Vec<Hit> {
    match doc.language.as_str() {
        "java" => sites::java(&doc.text),
        "xml" => sites::xml(&doc.text),
        _ => Vec::new(),
    }
}

fn lib_hits(path: &str, text: &str) -> Vec<Hit> {
    match path.rsplit('.').next() {
        Some("java") => sites::java(text),
        Some("xml") => sites::xml(text),
        _ => Vec::new(),
    }
}

/// Removes the field annotations of a class's injected fields, makes them
/// final and adds a constructor taking them, or extends the class's only constructor.
fn constructor_injection(text: &str, line: u32) -> Option<Vec<TextEdit>> {
    let mut file = java::parse(text);
    annotation_owners::Owners::new(&file,text,false).filter_definitions(&mut file,text);
    let on_line = |m: &java::Member| position(text, m.start).line <= line && line <= position(text, m.end).line;
    let (class, fields) = file.types.iter().find_map(|t| {
        let fields: Vec<&java::Member> = t.fields().filter(|m| m.has(&["Autowired", "Inject"])).collect();
        fields.iter().any(|m| on_line(m)).then_some((t, fields))
    })?;
    let mut constructors = class.members.iter().filter(|m| m.kind == MemberKind::Constructor);
    let existing = constructors.next();
    if constructors.next().is_some() {
        return None;
    }
    if existing.is_some() && class.members.iter().any(|m| m.kind == MemberKind::Method && m.has(&["Autowired", "Inject"])) {
        return None;
    }
    let mut edits = Vec::new();
    let mut params = Vec::new();
    for f in &fields {
        let a = f.annotations.iter().find(|a| a.name == "Autowired" || a.name == "Inject")?;
        let ty = f.ty.as_ref()?;
        let mut end = a.end;
        while text.as_bytes().get(end).is_some_and(|c| c.is_ascii_whitespace()) {
            end += 1;
        }
        edits.push(TextEdit { range: range(text, a.at, end), text: String::new() });
        if !f.modifiers.iter().any(|m| m == "final") {
            let ty_at = a.end + text[a.end..f.end].find(&ty.text)?;
            edits.push(TextEdit { range: range(text, ty_at, ty_at), text: "final ".into() });
        }
        params.push((ty.text.clone(), f.name.clone()));
    }
    if let Some(constructor) = existing {
        edits.extend(merge_into(text, constructor, &params)?);
        return Some(edits);
    }
    let last = class.fields().last()?;
    let line_start = text[..last.start].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let visibility = if class.modifiers.iter().any(|m| m == "public") { "public " } else { "" };
    let list: Vec<String> = params.iter().map(|(t, n)| format!("{t} {n}")).collect();
    let body: String = params.iter().map(|(_, n)| format!("{indent}    this.{n} = {n};\n")).collect();
    let constructor = format!("\n\n{indent}{visibility}{}({}) {{\n{body}{indent}}}", class.name, list.join(", "));
    edits.push(TextEdit { range: range(text, last.end, last.end), text: constructor });
    Some(edits)
}

/// Adds `params` to the parameter list of `constructor` and assigns them at the end of its body.
fn merge_into(text: &str, constructor: &java::Member, params: &[(String, String)]) -> Option<Vec<TextEdit>> {
    let (from, to) = constructor.body?;
    let body = text.get(from..to)?;
    if !body.starts_with('{') || !body.ends_with('}') || !spring_text::text::balanced(text, false) || body.contains("this(") {
        return None;
    }
    for (_, name) in params {
        if assigns(body, name) {
            return None;
        }
        if constructor.params.iter().any(|p| &p.name == name) {
            return None;
        }
    }
    let open = constructor.at + text[constructor.at..].find('(')?;
    let mut depth = 0;
    let close = text[open..].char_indices().find_map(|(i, c)| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        (depth == 0).then_some(open + i)
    })?;
    if close > from {
        return None;
    }
    let indent = indent_of(text, constructor.start);
    let new: Vec<String> = params.iter().map(|(t, n)| format!("{t} {n}")).collect();
    let mut edits = Vec::new();
    match constructor.params.last() {
        None => edits.push(TextEdit { range: range(text, close, close), text: new.join(", ") }),
        Some(last) => {
            let code_end = last.at + last.name.len();
            let after_last = open + 1 + text[open + 1..close].trim_end().len();
            let (comma, rest) = if text[open + 1..close].contains('\n') {
                let pad = indent_of(text, last.at);
                (",", format!("\n{pad}{}", new.join(&format!(",\n{pad}"))))
            } else {
                (",", format!(" {}", new.join(", ")))
            };
            if code_end == after_last {
                edits.push(TextEdit { range: range(text, code_end, code_end), text: format!("{comma}{rest}") });
            } else {
                edits.push(TextEdit { range: range(text, code_end, code_end), text: comma.into() });
                edits.push(TextEdit { range: range(text, after_last, after_last), text: rest });
            }
        }
    }
    let assignments: String = params.iter().map(|(_, n)| format!("\n{indent}    this.{n} = {n};")).collect();
    let inner = &body[1..body.len() - 1];
    let (at, end, replacement) = if inner.trim().is_empty() {
        (from + 1, to - 1, format!("{assignments}\n{indent}"))
    } else {
        let at = from + 1 + inner.trim_end().len();
        (at, at, assignments)
    };
    edits.push(TextEdit { range: range(text, at, end), text: replacement });
    Some(edits)
}

/// Applies non-overlapping `edits` to `text`; returns the result and its
/// changed lines, each with 3 lines of context, for a confirmation that shows
/// the change rather than the whole file.
pub(crate) fn reviewed(text: &str, edits: &[TextEdit]) -> Result<(String, String), Error> {
    let mut ranges = edits
        .iter()
        .map(|edit| {
            let start = offset(text, edit.range.start).ok_or(Error::Stale)?;
            let end = offset(text, edit.range.end).ok_or(Error::Stale)?;
            if start > end {
                return Err(Error::Stale);
            }
            Ok((start, end, edit.text.as_str()))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    ranges.sort_by_key(|(start, end, _)| (*start, *end));
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(Error::Stale);
    }
    let mut after = String::with_capacity(text.len());
    let mut spans = Vec::new();
    let mut at = 0;
    for (start, end, replacement) in ranges {
        after.push_str(&text[at..start]);
        spans.push((after.len(), after.len() + replacement.len()));
        after.push_str(replacement);
        at = end;
    }
    after.push_str(&text[at..]);
    let starts: Vec<usize> = std::iter::once(0).chain(after.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line = |offset: usize| starts.partition_point(|&start| start <= offset) - 1;
    let last = starts.len() - 1;
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for (from, to) in spans {
        let first = line(from).saturating_sub(3);
        let end = (line(to.saturating_sub(1).max(from)) + 3).min(last);
        match blocks.last_mut() {
            Some(block) if first <= block.1 + 1 => block.1 = block.1.max(end),
            _ => blocks.push((first, end)),
        }
    }
    let shown: Vec<String> = blocks
        .into_iter()
        .map(|(first, end)| {
            let to = starts.get(end + 1).copied().unwrap_or(after.len());
            format!("Lines {}–{}:\n{}", first + 1, end + 1, after[starts[first]..to].trim_end_matches(['\r', '\n']))
        })
        .collect();
    Ok((after, shown.join("\n\n")))
}

fn indent_of(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}

fn assigns(body: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    body.match_indices(name).any(|(i, m)| {
        let before = &body[..i];
        let this_prefixed = before.ends_with("this.");
        let bare = !before.ends_with('.') && !before.ends_with(word);
        let rest = &body[i + m.len()..];
        let boundary = !rest.starts_with(word);
        let rest = rest.trim_start();
        boundary && (this_prefixed || bare) && rest.starts_with('=') && !rest.starts_with("==")
    })
}

/// Events maintain navigation caches; the host owns live diagnostic publication.
fn subscribe_to_file_events() {
    if let Err(error) = host::subscribe_paths(
        "files-changed",
        &[
            "**/*.java",
            "**/*.kt",
            "**/*.xml",
            "**/*.properties",
            "**/*.yml",
            "**/*.yaml",
            "**/*.sql",
            "**/pom.xml",
            "**/gradle.properties",
            "**/build.gradle",
            "**/build.gradle.kts",
            "**/settings.gradle",
            "**/settings.gradle.kts",
        ],
    ) {
        host::log(LogLevel::Warn, &format!("Filesystem navigation updates unavailable: {error:?}"));
    }
    for kind in [EventKind::FileSaved, EventKind::FileOpened] {
        if let Err(error) = host::subscribe(kind) {
            host::log(LogLevel::Warn, &format!("File events unavailable, checks run from the editor only: {error:?}"));
        }
    }
}

/// What the "Check Spring Configuration" and "Dump Spring Configuration" commands last produced.
#[derive(Default)]
enum Report {
    #[default]
    Empty,
    Problems(Vec<(String, Vec<Diagnostic>)>),
    Dump(String),
}

/// The last parse of an open document, shared by the provider calls of one version.
struct Parsed {
    path: String,
    version: u64,
    text: String,
    model: Option<FileModel>,
    java: Option<Rc<java::JavaFile>>,
}

impl Parsed {
    fn of(&self, doc: &Document) -> bool {
        self.path == doc.path && self.version == doc.version && self.text == doc.text
    }
}

pub struct Navigator {
    indexes: Indexes,
    parsed: Vec<Parsed>,
    expansion: Expansion,
    xml_generation: xml_file_actions::Generation,
    property_generation: property_file_actions::Generation,
    xml_creation: xml_create::Generation,
    report: Report,
    report_root: Option<String>,
    report_incomplete: bool,
    sections: Switcher,
    graph: graph::State,
}

impl Default for Navigator {
    fn default() -> Self {
        Self {
            indexes: Indexes::default(),
            parsed: Vec::new(),
            expansion: Expansion::default(),
            xml_generation: xml_file_actions::Generation::default(),
            property_generation: property_file_actions::Generation::default(),
            xml_creation: xml_create::Generation::default(),
            report: Report::default(),
            report_root: None,
            report_incomplete: false,
            sections: Switcher::new("beans.section", SECTIONS),
            graph: graph::State::default(),
        }
    }
}

impl Navigator {
    /// The cache of the project owning `doc`, with `doc` applied once per
    /// version; never starts I/O.
    fn document_cache(&mut self, doc: &Document) -> Option<&mut Cache> {
        let cache = self.indexes.document(&doc.path)?;
        let applied = self.parsed.iter().find(|parsed| parsed.of(doc)).is_some_and(|parsed| cache.files().get(&doc.path) == parsed.model.as_ref());
        if !applied {
            cache.update(&doc.path, &doc.text);
            self.parsed.retain(|parsed| parsed.path != doc.path);
            if self.parsed.len() >= 8 {
                self.parsed.remove(0);
            }
            let model = cache.files().get(&doc.path).cloned();
            self.parsed.push(Parsed { path: doc.path.clone(), version: doc.version, text: doc.text.clone(), model, java: None });
        }
        Some(cache)
    }

    /// The Java syntax of `doc`, parsed once per version.
    fn java_of(&mut self, doc: &Document) -> Rc<java::JavaFile> {
        let Some(parsed) = self.parsed.iter_mut().find(|parsed| parsed.of(doc)) else {
            return Rc::new(java::parse(&doc.text));
        };
        parsed.java.get_or_insert_with(|| Rc::new(java::parse(&doc.text))).clone()
    }

    /// Uses current-document facts and the owning cached project; never starts I/O.
    fn with_doc<R: Default>(&mut self, doc: &Document, f: impl FnOnce(&World, &FileModel) -> R) -> R {
        if doc.text.len() > 128 * 1024 {
            return R::default();
        }
        let beans = doc.language != "xml" || xml_namespace::beans_document(&doc.text);
        let Some(cache) = self.document_cache(doc) else {
            return R::default();
        };
        if !beans {
            return R::default();
        }
        let world = World::new(cache);
        let empty = FileModel::default();
        let model = world.files.get(&doc.path).unwrap_or(&empty);
        f(&world, model)
    }

    fn live_templates(doc: &Document, cursor: usize) -> Vec<CompletionItem> {
        let text = doc.text.as_str();
        let (start, end) = sites::word(text, 0, cursor, cursor);
        let typed = &text[start..end];
        let in_beans_xml = doc.language != "xml"
            || xml_checks::is_beans_file(&spring_text::xml::elements(text)) && live_templates::outside_tag(text, start);
        let line = &text[text[..start].rfind('\n').map_or(0, |n| n + 1)..start];
        let in_literal_or_comment = doc.language != "xml" && (line.matches('"').count() % 2 == 1 || line.contains("//"));
        if typed.is_empty() || !in_beans_xml || in_literal_or_comment {
            return Vec::new();
        }
        let replace = range(text, start, end);
        live_templates::for_language(&doc.language)
            .iter()
            .filter(|t| t.abbreviation.starts_with(typed))
            .map(|t| CompletionItem {
                insert_text: t.snippet.to_owned(),
                snippet: true,
                replace: Some(replace),
                detail: Some(t.description.to_owned()),
                ..CompletionItem::new(t.abbreviation, CompletionKind::Snippet)
            })
            .collect()
    }

    fn bean_items(world: &World, replace: Range) -> Vec<CompletionItem> {
        let mut seen = BTreeSet::new();
        world
            .beans()
            .flat_map(|(_, b)| std::iter::once(&b.name).chain(&b.aliases).map(move |n| (n, b)))
            .filter(|(n, _)| seen.insert(n.as_str()))
            .map(|(n, b)| CompletionItem {
                detail: Some(if b.ty.is_empty() { b.kind.clone() } else { format!("{} {}", b.kind, b.ty) }),
                replace: Some(replace),
                ..CompletionItem::new(n, CompletionKind::Reference)
            })
            .collect()
    }
}

/// Live checks use only the supplied document and already cached project facts.
impl DiagnosticsProvider for Navigator {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !matches!(doc.language.as_str(), "java" | "kotlin" | "xml") {return Vec::new();}
        if doc.text.len() > 512 * 1024 || doc.language == "xml" && !xml_namespace::beans_document(&doc.text) {
            let _ = self.document_cache(doc);
            return Vec::new();
        }
        let in_project = self.document_cache(doc).is_some();
        let java = (doc.language == "java").then(|| self.java_of(doc));
        let mut local = Cache::new();
        let cache: &Cache = match self.indexes.document(&doc.path).filter(|_| in_project) {
            Some(cache) => cache,
            None => {
                local.update(&doc.path, &doc.text);
                &local
            }
        };
        let world = World::new(cache);
        let mut groups = checks::Groups::from_settings();
        // These require external resources or build configuration, which is unknown here.
        groups.aop = false;
        groups.duplicate_names = false;
        groups.unresolved_references = false;
        let findings = match doc.language.as_str() {
            "java" => match java.as_deref() {
                Some(file) => checks::check_java(&doc.path, file, &doc.text, &world, groups),
                None => Vec::new(),
            },
            "xml" => {
                groups.aop = checks::Groups::from_settings().aop;
                let mut findings = xml_checks::check_xml_cached(&doc.path, &doc.text, &world, groups);
                findings.extend(xml_class_checks::check(&spring_text::xml::elements(&doc.text), &world, groups));
                findings
            }
            "kotlin" if groups.kotlin_plugin => {
                let mut findings = kotlin::router_path_variables(&doc.text);
                findings.extend(kotlin::reified_extension_hints(&doc.text));
                findings
            }
            _ => Vec::new(),
        };
        let mut problems: Vec<_> = findings.iter().take(512).map(|f| f.diagnostic(&doc.text)).collect();
        if doc.language != "xml" {
            let model = world.files.get(&doc.path).cloned().unwrap_or_default();
            problems.extend(field_injection_problems(&model).into_iter().take(512 - problems.len()));
        }
        problems
    }
}

impl Providers for Navigator {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.with_doc(doc, |world, model| {
            let mut markers = world
                .resolver()
                .checked(|path| {
                    if path == doc.path {
                        return Some(model.navigation.clone());
                    }
                    world.files.get(path).map(|model| model.navigation.clone())
                })
                .markers(&model.navigation);
            for inj in &model.injections {
                let found = world.candidates(inj);
                let candidate = !inj.multiple && found.len() > 1;
                for (path, bean) in found.iter().take(MAX_LINE_TARGETS) {
                    let described = describe(bean);
                    let tooltip = if candidate { format!("Candidate bean: {described}") } else { format!("Injected bean: {described}") };
                    let target = Location { path: path.to_string(), range: bean.range };
                    markers.push(LineMarker { line: inj.range.start.line, icon: Some(BEAN_MARKER.into()), tooltip, target: Some(target) });
                }
                bounded(&mut markers, found.len());
            }
            for bean in &model.beans {
                let users = world.users(&doc.path, bean);
                for (location, into) in users.iter().take(MAX_LINE_TARGETS) {
                    markers.push(LineMarker {
                        line: bean.range.start.line,
                        icon: Some(BEAN_MARKER.into()),
                        tooltip: format!("Injected into {into}"),
                        target: Some(location.clone()),
                    });
                }
                bounded(&mut markers, users.len());
            }
            markers
        })
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(cursor) = offset(&doc.text, at) else {
            return Vec::new();
        };
        let property = xml_class_checks::property_site(&doc.text, cursor);
        let hit = sites::at(hits(doc), cursor);
        let text = doc.text.as_str();
        self.with_doc(doc, |world, model| {
            if let Some((class, from, to)) = &property {
                let found: Vec<Location> = xml_class_checks::setters(world, class)
                    .into_iter()
                    .filter(|(p, _, _)| p == &text[*from..*to])
                    .map(|(_, m, path)| Location { path: path.into(), range: m.range })
                    .collect();
                if !found.is_empty() {
                    return found;
                }
            }
            let related = || {
                world
                    .resolver()
                    .checked(|path| {
                        if path == doc.path {
                            return Some(model.navigation.clone());
                        }
                        world.files.get(path).map(|model| model.navigation.clone())
                    })
                    .definition(&model.navigation, at)
            };
            let to_locations = |found: Vec<Found>| found.into_iter().map(|(p, b)| Location { path: p.into(), range: b.range }).collect();
            if let Some(inj) = model.injections.iter().find(|i| !i.ty.is_empty() && contains(i.range, at)) {
                return to_locations(world.candidates(inj));
            }
            let Some(hit) = hit else { return related() };
            match &hit.site {
                Site::BeanRef => {
                    let (s, e) = sites::list_item(text, hit.from, cursor, hit.to);
                    to_locations(world.named(&text[s..e]))
                }
                Site::Scope => Vec::new(),
                Site::Profile => related(),
                Site::Class => {
                    let class = text[hit.from..hit.to].replace('$', ".");
                    world
                        .files
                        .iter()
                        .flat_map(|(p, m)| m.types.iter().map(move |t| (p, t)))
                        .filter(|(_, t)| t.qualified == class)
                        .map(|(p, t)| Location { path: p.clone(), range: t.range })
                        .collect()
                }
                Site::Spel(_) | Site::Pointcut => {
                    let (s, e) = sites::word(text, hit.from, cursor, hit.to);
                    match sites::classify(text, &hit, s, e) {
                        Word::Bean => to_locations(world.named(&text[s..e])),
                        Word::Pointcut(class) => {
                            let all = world.pointcuts(&text[s..e], class);
                            let here: Vec<Location> = all.iter().filter(|l| l.path == doc.path).cloned().collect();
                            if class.is_none() && !here.is_empty() { here } else { all }
                        }
                        _ => Vec::new(),
                    }
                }
            }
        })
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if doc.language=="spel" {return spel::completion(doc,at);}
        let Some(cursor) = offset(&doc.text, at) else {
            return Vec::new();
        };
        if let Some((class, from, to)) = xml_class_checks::property_site(&doc.text, cursor) {
            let replace = range(&doc.text, from, to);
            return self.with_doc(doc, |world, _| {
                let mut seen = BTreeSet::new();
                let setters = xml_class_checks::setters(world, &class);
                setters
                    .into_iter()
                    .filter(|(p, _, _)| seen.insert(p.clone()))
                    .map(|(p, _, _)| CompletionItem {
                        detail: Some(format!("setter of {class}")),
                        replace: Some(replace),
                        ..CompletionItem::new(&p, CompletionKind::Property)
                    })
                    .collect()
            });
        }
        if let Some((class, from, to)) = xml_class_checks::constructor_arg_site(&doc.text, cursor) {
            let replace = range(&doc.text, from, to);
            let items = self.with_doc(doc, |world, _| {
                let simple = class.rsplit(['.', '$']).next().unwrap_or(&class);
                let chain = world.member_chain(simple, Some(&class.replace('$', "."))).unwrap_or_default();
                let names: BTreeSet<&String> = chain.first().into_iter().flat_map(|t| &t.ctor_names).flatten().collect();
                names
                    .into_iter()
                    .map(|n| CompletionItem {
                        detail: Some(format!("constructor parameter of {class}")),
                        replace: Some(replace),
                        ..CompletionItem::new(n, CompletionKind::Property)
                    })
                    .collect::<Vec<_>>()
            });
            return items;
        }
        let Some(hit) = sites::at(hits(doc), cursor) else {
            return Self::live_templates(doc, cursor);
        };
        let text = doc.text.as_str();
        self.with_doc(doc, |world, _| match &hit.site {
            Site::BeanRef => {
                let (s, e) = sites::list_item(text, hit.from, cursor, hit.to);
                Self::bean_items(world, range(text, s, e))
            }
            Site::Scope => {
                let replace = range(text, hit.from, hit.to);
                xml_checks::KNOWN_SCOPES
                    .iter()
                    .map(|scope| CompletionItem {
                        detail: Some("bean scope".into()),
                        replace: Some(replace),
                        ..CompletionItem::new(scope, CompletionKind::Keyword)
                    })
                    .collect()
            }
            Site::Profile => {
                let (s, e) = sites::word_in(text, hit.from, cursor, hit.to);
                let replace = range(text, s, e);
                let profiles: BTreeSet<&str> = world
                    .beans()
                    .flat_map(|(_, b)| b.profiles.iter().map(|p| p.trim_start_matches('!')))
                    .filter(|p| !p.is_empty())
                    .collect();
                profiles
                    .into_iter()
                    .map(|p| CompletionItem {
                        detail: Some("profile".into()),
                        replace: Some(replace),
                        ..CompletionItem::new(p, CompletionKind::Reference)
                    })
                    .collect()
            }
            Site::Class => {
                let typed = &text[hit.from..cursor];
                let replace = range(text, hit.from, hit.to);
                let mut classes: Vec<(&str, &model::TypeInfo)> = world
                    .files
                    .values()
                    .flat_map(|m| m.types.iter().map(|t| (t.qualified.as_str(), t)))
                    .filter(|(q, _)| q.contains(typed))
                    .collect();
                classes.sort_by_key(|(q, _)| *q);
                classes.dedup_by_key(|(q, _)| *q);
                classes
                    .into_iter()
                    .take(MAX_CLASSES)
                    .map(|(q, t)| CompletionItem {
                        detail: Some(t.name.clone()),
                        replace: Some(replace),
                        ..CompletionItem::new(q, CompletionKind::Class)
                    })
                    .collect()
            }
            Site::Spel(vars) => {
                let (s, e) = sites::word(text, hit.from, cursor, hit.to);
                let replace = range(text, s, e);
                match sites::classify(text, &hit, s, e) {
                    Word::Bean => Self::bean_items(world, replace),
                    Word::Variable => vars
                        .iter()
                        .map(|v| CompletionItem { replace: Some(replace), ..CompletionItem::new(v, CompletionKind::Variable) })
                        .collect(),
                    _ => Vec::new(),
                }
            }
            Site::Pointcut => {
                let (s, e) = sites::word(text, hit.from, cursor, hit.to);
                let replace = range(text, s, e);
                let pointcut_items = |class: Option<&str>| {
                    world
                        .files
                        .values()
                        .flat_map(|m| &m.pointcuts)
                        .filter(|p| class.is_none_or(|c| p.class == c))
                        .map(|p| CompletionItem {
                            detail: Some(p.class.clone()),
                            insert_text: format!("{}()", p.name),
                            replace: Some(replace),
                            ..CompletionItem::new(&p.name, CompletionKind::Method)
                        })
                        .collect::<Vec<_>>()
                };
                match sites::classify(text, &hit, s, e) {
                    Word::Bean => Self::bean_items(world, replace),
                    Word::Pointcut(class) => pointcut_items(class),
                    Word::Designator => {
                        let after_at = text[..s].ends_with('@');
                        let mut items: Vec<CompletionItem> = sites::DESIGNATORS
                            .iter()
                            .filter(|(d, _)| !after_at || matches!(*d, "within" | "target" | "args" | "annotation"))
                            .map(|(d, snippet)| CompletionItem {
                                insert_text: snippet.to_string(),
                                snippet: true,
                                replace: Some(replace),
                                detail: Some("pointcut designator".into()),
                                ..CompletionItem::new(d, CompletionKind::Keyword)
                            })
                            .collect();
                        if !after_at {
                            items.extend(pointcut_items(None));
                        }
                        items
                    }
                    _ => Vec::new(),
                }
            }
        })
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if doc.language == "xml" {
            return xml_code_actions(doc, selection);
        }
        if doc.language == "kotlin" {
            return kotlin_code_actions(doc, selection);
        }
        if doc.language != "java" {
            return Vec::new();
        }
        let Some(edits) = constructor_injection(&doc.text, selection.start.line) else {
            return Vec::new();
        };
        vec![CodeAction {
            title: "Use constructor injection".into(),
            kind: Some("quickfix".into()),
            preferred: true,
            edits: vec![(doc.path.clone(), edits)],
            data: None,
        }]
    }
}

fn kotlin_code_actions(doc: &Document, selection: Range) -> Vec<CodeAction> {
    let Some(caret) = offset(&doc.text, selection.start) else {
        return Vec::new();
    };
    if !checks::Groups::from_settings().kotlin_plugin {
        return Vec::new();
    }
    let file = spring_text::kotlin::parse(&doc.text);
    let on_candidate = kotlin_build::candidates(&file).iter().any(|ty| (ty.at..=ty.at + ty.name.len()).contains(&caret));
    if !on_candidate {
        return Vec::new();
    }
    let findings = kotlin_build::missing_spring_plugin(&doc.path, &file);
    if !findings.iter().any(|f| (f.start..=f.end).contains(&caret)) {
        return Vec::new();
    }
    let Some((build_path, edit)) = kotlin_build::fix_for(&doc.path) else {
        return Vec::new();
    };
    vec![CodeAction {
        title: "Add the kotlin-spring Gradle plugin".into(),
        kind: Some("quickfix".into()),
        preferred: true,
        edits: vec![(build_path, vec![edit])],
        data: None,
    }]
}

fn xml_code_actions(doc: &Document, selection: Range) -> Vec<CodeAction> {
    let Some(caret) = offset(&doc.text, selection.start) else {
        return Vec::new();
    };
    let text = &doc.text;
    let mut actions = Vec::new();
    let mut offer = |title: &str, kind: &str, preferred: bool, edits: Vec<TextEdit>| {
        actions.push(CodeAction {
            title: title.into(),
            kind: Some(kind.into()),
            preferred,
            edits: vec![(doc.path.clone(), edits)],
            data: None,
        });
    };
    if let Some((title, edit)) = xml_checks::style_fix(text, selection.start) {
        offer(title, "quickfix", true, vec![edit]);
    }
    if let Some(edits) = xml_actions::use_p_namespace(text, caret) {
        offer("Use p-namespace", "refactor", false, edits);
    }
    if let Some(edits) = xml_actions::update_schema(text) {
        offer("Update XML schema", "refactor", false, edits);
    }
    actions
}

fn location_key(path: &str, range: Range) -> String {
    format!("b:{}:{}:{path}", range.start.line, range.start.character)
}

impl Navigator {
    fn tree(index: &Cache, empty: &str) -> View {
        let world = World::new(index);
        let mut groups: BTreeMap<&str, Vec<Found>> = BTreeMap::new();
        for (path, b) in world.beans() {
            groups.entry(b.kind.as_str()).or_default().push((path, b));
        }
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if groups.is_empty() {
            let text = if index.complete() { empty.to_owned() } else { index.state() };
            return View::message(&text).button(refresh);
        }
        let mut items = Vec::new();
        let mut total = 0;
        for (kind, mut beans) in groups {
            beans.sort_by(|a, b| a.1.name.cmp(&b.1.name));
            total += beans.len();
            let group = format!("g:{kind}");
            let group_icon = match kind {
                "Spring Data repository" => "database",
                k if k.starts_with('<') => "file-xml",
                _ => "annotation",
            };
            let item_icon = match kind {
                "Spring Data repository" => "interface",
                "@Bean" => "method",
                _ => "class",
            };
            items.push(TreeItem::new(&group, kind).icon(group_icon).detail(&beans.len().to_string()).expandable(true));
            for (path, b) in beans {
                let mut detail = file_name(path).to_owned();
                if b.primary {
                    detail += " · primary";
                }
                if !b.profiles.is_empty() {
                    detail += &format!(" · {}", b.profiles.join(", "));
                }
                items.push(TreeItem::new(&location_key(path, b.range), &b.name).parent(&group).icon(item_icon).detail(&detail));
            }
        }
        View::new(Body::Tree(items)).button(refresh).status(&format!("{}{}", bean_count(total), scanning(index)))
    }

    fn report_view(&self) -> View {
        match &self.report {
            Report::Empty => View::message("Check the project for Spring configuration problems.").button(check_button()),
            Report::Dump(text) => {
                let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                View::new(Body::Html(format!("<pre class=\"ui-code\" style=\"font-family: var(--ui-mono); white-space: pre-wrap\">{escaped}</pre>")))
                    .button(Button::new("dump", "Dump Spring Configuration").icon("refresh"))
            }
            Report::Problems(files) if files.is_empty() => View::message(if self.report_incomplete {
                "No problems found in the files checked so far, but the scan isn't finished. Click Refresh in Spring Beans, then check again."
            } else {
                "No problems found."
            })
            .button(check_button()),
            Report::Problems(files) => {
                let mut items = Vec::new();
                let mut total = 0;
                for (path, problems) in files {
                    let group = format!("f:{path}");
                    items.push(TreeItem::new(&group, file_name(path)).icon("file").detail(&problems.len().to_string()).expandable(true));
                    for problem in problems {
                        let detail = format!("line {} · {}", problem.range.start.line + 1, problem.code.as_deref().unwrap_or("problem"));
                        items.push(TreeItem::new(&location_key(path, problem.range), &problem.message).parent(&group).detail(&detail));
                    }
                    total += problems.len();
                }
                View::new(Body::Tree(items)).button(check_button()).status(&format!(
                    "{total} problems in {} files{}",
                    files.len(),
                    if self.report_incomplete { " · partial source check" } else { "" }
                ))
            }
        }
    }
}

pub(crate) const PLUGIN_NAME: &str = "Spring Beans Navigator";
/// One read step of a Refresh; the task's progress reports renew the call's time budget.
const SCAN_STEP: Duration = Duration::from_secs(1);

impl Navigator {
    /// Reads the whole project in steps under a cancellable IDE task, so one Refresh finishes the scan.
    fn scan_project(&mut self, project: &Project) -> String {
        let task = host::begin_task("Scanning Spring beans", true);
        let mut step = 0;
        loop {
            let index = self.indexes.get(project, SCAN_STEP);
            if !index.scanning() || task.is_cancelled() {
                return index.state();
            }
            step += 1;
            task.report(step, None, Some(&format!("Scanning… {} source files read", index.files().len())));
        }
    }

    /// Asks for a bean name or type and focuses the graph on the first match; Escape clears the search.
    fn find_in_graph(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let project = ctx.project().ok_or("Open a project first.")?;
        let current = (!self.graph.query.is_empty()).then(|| self.graph.query.clone());
        let query = host::input("Find bean by name or type", current.as_deref())?.unwrap_or_default();
        let index = self.indexes.get(&project, COMMAND_BUDGET);
        self.graph.search(&query, index);
        Ok(())
    }
}

fn check_button() -> Button {
    Button::new("check", "Check Spring Configuration").icon("check")
}

fn no_beans_message(project: &Project) -> &'static str {
    if checks::BUILD_FILES.iter().any(|file| project.exists(file).unwrap_or(false)) {
        "No Spring beans found."
    } else {
        "No Spring beans found. This project doesn't look like a Spring project: it has no pom.xml or build.gradle."
    }
}

fn bean_count(n: usize) -> String {
    if n == 1 { "1 bean".into() } else { format!("{n} beans") }
}

fn scanning(index: &Cache) -> String {
    if index.complete() { String::new() } else { format!(" · {}", index.state()) }
}

impl Views for Navigator {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its Spring beans."));
            };
            let root = match project.root() {
                Ok(root) if !root.is_empty() => root,
                _ => return Ok(View::message("Project identity is unavailable.")),
            };
            let section = self.sections.active();
            let mut shown = if section == REPORT_SECTION {
                if self.report_root.as_deref() != Some(root.as_str()) {
                    View::message("No configuration report for this project yet.").button(check_button())
                } else {
                    self.report_view()
                }
            } else {
                let Some(index) = self.indexes.cached(&project) else {
                    return Ok(self.sections.buttons(
                        View::message("Not scanned yet. Click Refresh to find this project's Spring beans.")
                            .button(Button::new("refresh", "Refresh").icon("refresh")),
                        &[],
                    ));
                };
                let empty = no_beans_message(&project);
                if section == GRAPH_SECTION {
                    graph::view(index, empty, &self.graph)
                } else if ui_kit::web::is_rendering() {
                    resources::view(index, empty)
                } else {
                    Self::tree(index, empty)
                }
            };
            if let Body::Tree(items) = &mut shown.body {
                self.expansion.apply(items);
            }
            Ok(self.sections.buttons(shown, &[]))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        // Row actions belong to bean rows, not to the Report's problem rows that share their keys.
        let row_actions = |nav: &Self, _: &str, _: &View| Ok((nav.sections.active() != "beans").then(Vec::new));
        if ui_kit::web::handle_with_actions(self, view, &event, ctx, include_str!("ui_config.json"), row_actions)? {
            return Ok(());
        }

        if self.sections.handle(&event) || self.expansion.handle(&event) {
            refresh_beans_view();
            return Ok(());
        }

        let key = match event {
            ViewEvent::Button(b) if b == "check" => {
                commands::check_configuration(self, ctx)?;
                return Ok(());
            }
            ViewEvent::Button(b) if b == "dump" => {
                commands::dump_configuration(self, ctx)?;
                return Ok(());
            }
            ViewEvent::Button(b) if b == "refresh" => {
                if let Some(project) = ctx.project() {
                    if !self.indexes.cached(&project).is_some_and(|index| index.scanning()) {
                        self.indexes.reset_project(&project);
                    }
                    self.scan_project(&project);
                }
                refresh_beans_view();
                return Ok(());
            }
            ViewEvent::Button(b) if b == graph::FIND => {
                self.find_in_graph(ctx)?;
                refresh_beans_view();
                return Ok(());
            }
            ViewEvent::Message(message) if self.sections.active() == GRAPH_SECTION => {
                let index = ctx.project().and_then(|project| self.indexes.cached(&project));
                match self.graph.handle(&message, index)? {
                    Some(key) => key,
                    None => {
                        refresh_beans_view();
                        return Ok(());
                    }
                }
            }
            ViewEvent::Activate(key) | ViewEvent::Message(key) => key,
            _ => return Ok(()),
        };
        let mut parts = key.splitn(4, ':');
        if let (Some("b"), Some(line), Some(col), Some(path)) = (parts.next(), parts.next(), parts.next(), parts.next()) {
            let (line, col) = (line.parse::<u32>().map_err(|_| Error::Stale)?, col.parse::<u32>().map_err(|_| Error::Stale)?);
            let project = ctx.project().ok_or(Error::Stale)?;
            let root = project.root()?;
            if host::project_of(path).and_then(|project| project.root().ok()).as_deref() != Some(root.as_str()) {
                return Err(Error::Stale);
            }
            let at = Position::new(line, col);
            let bean = self
                .indexes
                .cached(&project)
                .and_then(|index| index.files().get(path))
                .is_some_and(|model| model.beans.iter().any(|bean| bean.range.start == at));
            let report = self.report_root.as_deref() == Some(root.as_str())
                && matches!(&self.report,Report::Problems(files) if files.iter().any(|(file,problems)|file==path&&problems.iter().any(|problem|problem.range.start==at)));
            if !bean && !report {
                return Err(Error::Stale);
            }
            host::open_location(path, line, col)?;
        }
        Ok(())
    }
}

impl Plugin for Navigator {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        subscribe_to_file_events();
        let started = std::time::Instant::now();
        for project in host::projects().unwrap_or_default().into_iter().take(8) {
            let remaining = Duration::from_millis(100).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            self.indexes.get(&project, remaining.min(Duration::from_millis(25)));
        }
        self.indexes.checkpoint();
        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.indexes.prune();
        if self
            .report_root
            .as_ref()
            .is_some_and(|root| !host::projects().unwrap_or_default().iter().any(|project| project.root().ok().as_ref() == Some(root)))
        {
            self.report = Report::Empty;
            self.report_root = None;
            refresh_beans_view();
        }
        let started = std::time::Instant::now();
        let mut changes = BTreeMap::new();
        let mut warm = BTreeMap::new();
        let mut count = 0usize;
        for event in events.iter().take(257) {
            let (paths, removed, opened): (Box<dyn Iterator<Item = &str>>, bool, bool) = match event {
                Event::FileOpened(path) => (Box::new(std::iter::once(path.as_str())), false, true),
                Event::FileSaved(path) => (Box::new(std::iter::once(path.as_str())), false, false),
                Event::FilesChanged(batch) => {
                    for path in batch.removed.iter().take(257) {
                        changes.insert(path.as_str(), true);
                        count += 1;
                    }
                    (Box::new(batch.created.iter().chain(&batch.modified).map(String::as_str)), false, false)
                }
                _ => continue,
            };
            for path in paths.take(257) {
                count += 1;
                if count > 256 {
                    break;
                }
                if opened {
                    if cache::source(path) {
                        changes.insert(path, false);
                        if let Some(project) = host::project_of(path) {
                            if let Ok(root) = project.root() {
                                warm.insert(root, project);
                            }
                        }
                    }
                } else {
                    changes.insert(path, removed);
                }
            }
            if count > 256 {
                break;
            }
        }
        if count > 256 || events.len() > 256 {
            self.indexes.clear();
            refresh_beans_view();
            return;
        }
        let mut changed = false;
        for (path, removed) in changes {
            if started.elapsed() > Duration::from_millis(100) {
                self.indexes.clear();
                refresh_beans_view();
                return;
            }
            if let Some(project) = host::project_of(path) {
                if self.indexes.changed(&project, path, removed) {
                    changed = true;
                    if let Ok(root) = project.root() {
                        warm.insert(root, project);
                    }
                }
            }
        }
        for (_, project) in warm.into_iter().take(8) {
            let remaining = Duration::from_millis(100).saturating_sub(started.elapsed());
            if remaining.is_zero() {
                break;
            }
            self.indexes.get(&project, remaining.min(Duration::from_millis(25)));
            changed = true;
        }
        self.indexes.checkpoint();
        if changed {
            refresh_beans_view();
        }
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "focus-in-graph" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                match _arg {
                    Some(key) => {
                        if !self.indexes.cached(&project).is_some_and(|index| graph::has_bean(index, key)) {
                            return Err(Error::Stale);
                        }
                        self.graph.focus = Some(key.into());
                    }
                    None => self.find_in_graph(ctx)?,
                }
                self.sections.set(GRAPH_SECTION);
                host::reveal_view(BEANS_VIEW)?;
                refresh_beans_view();
                Ok(None)
            }
            "reindex" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                self.indexes.reset_project(&project);
                let state = self.scan_project(&project);
                refresh_beans_view();
                let complete = self.indexes.cached(&project).is_some_and(|index| index.complete());
                Ok(Some(if complete { "Spring Beans finished reindexing this project.".into() } else { state }))
            }
            "generate-setter" => commands::generate(self, ctx, commands::Generate::Setter),
            "generate-constructor-dependency" => commands::generate(self, ctx, commands::Generate::Constructor),
            "generate-autowired-dependency" => commands::generate(self, ctx, commands::Generate::Autowire),
            "generate-event-listener" => commands::generate(self, ctx, commands::Generate::Listener),
            "change-active-profiles" => commands::change_active_profiles(self, ctx),
            "check-spring-configuration" => commands::check_configuration(self, ctx),
            "dump-spring-configuration" => commands::dump_configuration(self, ctx),
            "externalize-annotation-string" => property_file_actions::command(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

// API 1.3 registers the optional exports on the same component.
openidea_plugin::register_plugin!(Navigator, providers, views, indexing, search, diagnostics, file_actions, hover, injections, syntax);

#[cfg(test)]
mod tests;
