//! Spring Beans Navigator for OpenIDEA: indexes the beans a project's Java,
//! Kotlin and Spring XML files declare and links injection points, bean-name
//! strings, SpEL `@bean` references and named pointcuts to them.
//! Text-based: types match by simple name through the project's own type
//! hierarchy; beans from libraries and auto-configuration aren't seen.

mod aop;
mod checks;
mod kotlin;
mod model;
mod sites;
mod xml_checks;

pub use model::{Bean, FileModel, Injection, parse};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, LogLevel, Plugin, Position, Range, Severity, TextEdit, host, register_plugin,
};
use sites::{Hit, Site, Word};
use spring_text::index::{COMMAND_BUDGET, Index, Indexes, PROVIDER_BUDGET};
use spring_text::diagram;
use spring_text::tree::Expansion;
use spring_text::java::{self, MemberKind};
use spring_text::{contains, range};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const GLOBS: &[&str] = &["**/*.java", "**/*.kt", "**/*.xml"];
const BEANS_VIEW: &str = "beans";
const GRAPH_VIEW: &str = "bean-graph";
const MAX_GRAPH: usize = 400;
const MAX_CLASSES: usize = 200;

/// The project's models, with the type hierarchy for lookups.
struct World<'a> {
    files: &'a BTreeMap<String, FileModel>,
    supers: BTreeMap<&'a str, Vec<&'a str>>,
    /// Every file has been read at least once.
    complete: bool,
}

type Found<'a> = (&'a str, &'a Bean);

impl<'a> World<'a> {
    fn new(index: &'a Index<FileModel>) -> Self {
        let mut supers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for m in index.files().values() {
            for t in &m.types {
                supers.entry(&t.name).or_default().extend(t.supers.iter().map(String::as_str));
            }
        }
        Self { files: index.files(), supers, complete: index.complete() }
    }

    fn beans(&self) -> impl Iterator<Item = Found<'a>> + '_ {
        self.files.iter().flat_map(|(p, m)| m.beans.iter().map(move |b| (p.as_str(), b)))
    }

    /// Whether `ty` is `target` or one of its subtypes.
    fn is_a(&self, ty: &str, target: &str) -> bool {
        let mut seen = BTreeSet::new();
        let mut todo = vec![ty];
        while let Some(t) = todo.pop() {
            if t == target {
                return true;
            }
            if seen.insert(t) {
                todo.extend(self.supers.get(t).into_iter().flatten());
            }
        }
        false
    }

    fn named(&self, name: &str) -> Vec<Found<'a>> {
        self.beans().filter(|(_, b)| b.answers_to(name)).collect()
    }

    /// The beans Spring would consider for `inj`: by type, then qualifier,
    /// `@Primary`, and the field or parameter name.
    fn candidates(&self, inj: &Injection) -> Vec<Found<'a>> {
        if inj.ty.is_empty() {
            return self.named(inj.qualifier.as_deref().unwrap_or(&inj.name));
        }
        let typed: Vec<Found> = self.beans().filter(|(_, b)| !b.ty.is_empty() && self.is_a(&b.ty, &inj.ty)).collect();
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
        let mut out = Vec::new();
        for (p, m) in self.files {
            for inj in &m.injections {
                let maybe = if inj.ty.is_empty() {
                    inj.qualifier.as_deref().is_some_and(|q| bean.answers_to(q))
                } else {
                    !bean.ty.is_empty() && self.is_a(&bean.ty, &inj.ty)
                };
                if maybe && self.candidates(inj).iter().any(|(cp, b)| *cp == path && b.range == bean.range) {
                    let into = inj.owner.map_or_else(|| file_name(p).to_owned(), |o| m.beans[o].name.clone());
                    out.push((Location { path: p.clone(), range: inj.range }, into));
                }
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

fn hits(doc: &Document) -> Vec<Hit> {
    match doc.language.as_str() {
        "java" => sites::java(&doc.text),
        "xml" => sites::xml(&doc.text),
        _ => Vec::new(),
    }
}

/// Removes the field annotations of a class's injected fields, makes them
/// final and adds a constructor taking them, or extends the class's only constructor.
fn constructor_injection(text: &str, line: u32) -> Option<Vec<TextEdit>> {
    let file = java::parse(text);
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
    if !body.starts_with('{') || !body.ends_with('}') || !braces_balanced(text) || body.contains("this(") {
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

fn braces_balanced(text: &str) -> bool {
    let mut depth = 0i32;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

/// Events only refresh diagnostics on open and save, which the providers publish anyway.
fn subscribe_to_file_events() {
    for kind in [EventKind::FileSaved, EventKind::FileOpened] {
        if let Err(error) = host::subscribe(kind) {
            host::log(LogLevel::Warn, &format!("File events unavailable, checks run from the editor only: {error:?}"));
        }
    }
}

pub struct Navigator {
    indexes: Indexes<FileModel>,
    expansion: Expansion,
}

impl Default for Navigator {
    fn default() -> Self {
        Self { indexes: Indexes::new(GLOBS, model::parse), expansion: Expansion::default() }
    }
}

impl Navigator {
    /// Runs `f` on the project of `doc`, its index brought up to date with the document.
    fn with_doc<R: Default>(&mut self, doc: &Document, f: impl FnOnce(&World, &FileModel) -> R) -> R {
        let Some(index) = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return R::default() };
        let world = World::new(index);
        let empty = FileModel::default();
        let model = world.files.get(&doc.path).unwrap_or(&empty);
        f(&world, model)
    }

    /// Publishes the problems of a Java, Kotlin or Spring XML source; a fixed file gets an empty list.
    fn diagnose(&mut self, path: &str, language: &str, text: &str) {
        if language == "xml" {
            let mut problems = Vec::new();
            if let Some(index) = self.indexes.for_document(path, text, PROVIDER_BUDGET) {
                let findings = xml_checks::check_xml(path, text, &World::new(index), checks::Groups::from_settings());
                problems.extend(findings.iter().map(|f| f.diagnostic(text)));
            }
            let _ = host::publish_diagnostics(path, &problems);
            return;
        }
        let mut problems = field_injection_problems(&model::parse(path, text).unwrap_or_default());
        if language == "java" {
            let file = java::parse(text);
            if let Some(index) = self.indexes.for_document(path, text, PROVIDER_BUDGET) {
                let findings = checks::check_java(path, &file, text, &World::new(index), checks::Groups::from_settings());
                problems.extend(findings.iter().map(|f| f.diagnostic(text)));
            }
        }
        let _ = host::publish_diagnostics(path, &problems);
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

impl Providers for Navigator {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.diagnose(&doc.path, &doc.language, &doc.text);
        self.with_doc(doc, |world, model| {
            let mut markers = Vec::new();
            for inj in &model.injections {
                let found = world.candidates(inj);
                let Some((path, first)) = found.first() else { continue };
                let tooltip = match found.as_slice() {
                    [(_, one)] => format!("Injected bean: {}", describe(one)),
                    many => {
                        let names: Vec<String> = many.iter().map(|(_, b)| describe(b)).collect();
                        format!("{} candidate beans: {}", many.len(), names.join(", "))
                    }
                };
                let target = Location { path: path.to_string(), range: first.range };
                markers.push(LineMarker { line: inj.range.start.line, icon: None, tooltip, target: Some(target) });
            }
            for bean in &model.beans {
                let users = world.users(&doc.path, bean);
                let Some((first, _)) = users.first() else { continue };
                let names: BTreeSet<&str> = users.iter().map(|(_, into)| into.as_str()).collect();
                let names: Vec<&str> = names.into_iter().collect();
                markers.push(LineMarker {
                    line: bean.range.start.line,
                    icon: Some("implemented".into()),
                    tooltip: format!("Injected into {}", names.join(", ")),
                    target: Some(first.clone()),
                });
            }
            markers
        })
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(cursor) = offset(&doc.text, at) else { return Vec::new() };
        let hit = sites::at(hits(doc), cursor);
        let text = doc.text.as_str();
        self.with_doc(doc, |world, model| {
            let to_locations =
                |found: Vec<Found>| found.into_iter().map(|(p, b)| Location { path: p.into(), range: b.range }).collect();
            if let Some(inj) = model.injections.iter().find(|i| !i.ty.is_empty() && contains(i.range, at)) {
                return to_locations(world.candidates(inj));
            }
            let Some(hit) = hit else { return Vec::new() };
            match &hit.site {
                Site::BeanRef => {
                    let (s, e) = sites::list_item(text, hit.from, cursor, hit.to);
                    to_locations(world.named(&text[s..e]))
                }
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
        let Some(cursor) = offset(&doc.text, at) else { return Vec::new() };
        let Some(hit) = sites::at(hits(doc), cursor) else { return Vec::new() };
        let text = doc.text.as_str();
        self.with_doc(doc, |world, _| match &hit.site {
            Site::BeanRef => {
                let (s, e) = sites::list_item(text, hit.from, cursor, hit.to);
                Self::bean_items(world, range(text, s, e))
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
        if doc.language != "java" {
            return Vec::new();
        }
        let Some(edits) = constructor_injection(&doc.text, selection.start.line) else { return Vec::new() };
        vec![CodeAction {
            title: "Use constructor injection".into(),
            kind: Some("quickfix".into()),
            preferred: true,
            edits: vec![(doc.path.clone(), edits)],
            data: None,
        }]
    }
}

fn location_key(path: &str, range: Range) -> String {
    format!("b:{}:{}:{path}", range.start.line, range.start.character)
}

impl Navigator {
    fn tree(index: &Index<FileModel>) -> View {
        let world = World::new(index);
        let mut groups: BTreeMap<&str, Vec<Found>> = BTreeMap::new();
        for (path, b) in world.beans() {
            groups.entry(b.kind.as_str()).or_default().push((path, b));
        }
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if groups.is_empty() {
            let text = if index.complete() { "No Spring beans found." } else { "Scanning…" };
            return View::message(text).button(refresh);
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

    fn graph(index: &Index<FileModel>) -> View {
        let world = World::new(index);
        let total = world.beans().count();
        let beans: Vec<Found> = world.beans().take(MAX_GRAPH).collect();
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if beans.is_empty() {
            return View::message(if index.complete() { "No Spring beans found." } else { "Scanning…" }).button(refresh);
        }
        let node_of: BTreeMap<(&str, Range), usize> =
            beans.iter().enumerate().map(|(i, (p, b))| ((*p, b.range), i)).collect();
        let mut edges = BTreeSet::new();
        for (path, m) in world.files {
            for inj in &m.injections {
                let Some(from) = inj.owner.and_then(|o| node_of.get(&(path.as_str(), m.beans[o].range))) else { continue };
                for (p, b) in world.candidates(inj) {
                    if let Some(&to) = node_of.get(&(p, b.range)) {
                        edges.insert((*from, to));
                    }
                }
            }
        }
        let nodes: Vec<diagram::Node> =
            beans.iter().map(|(p, b)| diagram::Node::new(&location_key(p, b.range), &b.name).detail(&b.kind)).collect();
        let edges: Vec<diagram::Edge> = edges.into_iter().map(|(a, b)| diagram::Edge::new(&nodes[a].id, &nodes[b].id)).collect();
        let shown = if total > nodes.len() { format!("{} of {total} beans shown", nodes.len()) } else { bean_count(total) };
        let status = format!(
            "{shown} · {} dependencies · arrows point to dependencies{}",
            edges.len(),
            scanning(index)
        );
        View::new(Body::Html(diagram::html(&nodes, &edges, "No Spring beans found."))).button(refresh).status(&status)
    }
}

fn bean_count(n: usize) -> String {
    if n == 1 { "1 bean".into() } else { format!("{n} beans") }
}

fn scanning(index: &Index<FileModel>) -> &'static str {
    if index.complete() { "" } else { " · scanning…" }
}

impl Views for Navigator {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else { return Ok(View::message("Open a project to see its Spring beans.")) };
        let index = self.indexes.get(&project, COMMAND_BUDGET);
        if !index.complete() {
            host::refresh_view(view);
        }
        let mut shown = if view == GRAPH_VIEW { Self::graph(index) } else { Self::tree(index) };
        if let Body::Tree(items) = &mut shown.body {
            self.expansion.apply(items);
        }
        Ok(shown)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            host::refresh_view(view);
            return Ok(());
        }

        let key = match event {
            ViewEvent::Button(b) if b == "refresh" => {
                // Starting over mid-scan would throw the progress away, and a
                // big project would never finish.
                if let Some(project) = ctx.project() {
                    let index = self.indexes.get(&project, Duration::ZERO);
                    if index.complete() {
                        index.reset();
                    }
                }
                host::refresh_view(BEANS_VIEW);
                host::refresh_view(GRAPH_VIEW);
                return Ok(());
            }
            ViewEvent::Activate(key) | ViewEvent::Message(key) => key,
            _ => return Ok(()),
        };
        let mut parts = key.splitn(4, ':');
        if let (Some("b"), Some(line), Some(col), Some(path)) = (parts.next(), parts.next(), parts.next(), parts.next()) {
            host::open_location(path, line.parse().unwrap_or(0), col.parse().unwrap_or(0))?;
        }
        Ok(())
    }
}

impl Plugin for Navigator {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        subscribe_to_file_events();
        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        let paths: BTreeSet<&str> = events
            .iter()
            .filter_map(|e| match e {
                Event::FileSaved(path) | Event::FileOpened(path) => Some(path.as_str()),
                _ => None,
            })
            .filter(|path| [".java", ".kt", ".xml"].iter().any(|ext| path.ends_with(ext)))
            .collect();
        for path in paths {
            let Some(project) = host::projects().unwrap_or_default().into_iter().find(|p| {
                p.root().is_ok_and(|root| path.starts_with(&format!("{root}/")))
            }) else {
                continue;
            };
            let Ok(text) = project.read_file(path) else { continue };
            let language = match path.rsplit('.').next() {
                Some("kt") => "kotlin",
                Some("xml") => "xml",
                _ => "java",
            };
            self.diagnose(path, language, &text);
        }
    }

    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Navigator, providers, views);

#[cfg(test)]
mod tests;
