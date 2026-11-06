//! Spring Batch Tools for OpenIDEA: navigation, completion and checks for
//! XML job configuration (steps, flows, transitions, bean references), job
//! names in `spring.batch.job.name`, and Spring Batch 5 migration hints in
//! Java configuration. Text-based: names match as written.

use openidea_plugin::views::{Body, Button, View, ViewEvent, Viewport, Views};
use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::diagram::{self, Edge, Node};
use spring_text::index::{COMMAND_BUDGET, Index, Indexes, PROVIDER_BUDGET};
use spring_text::{beans, config, contains, java, range, xml};
use std::collections::BTreeSet;

mod xml_rules;

const NAMESPACE: &str = "http://www.springframework.org/schema/batch";
/// Elements a transition can lead to.
const STATES: [&str; 4] = ["step", "flow", "split", "decision"];
/// Attributes of the batch namespace that name a bean.
const BEAN_ATTRS: [&str; 13] = [
    "ref",
    "reader",
    "writer",
    "processor",
    "skip-policy",
    "retry-policy",
    "decider",
    "job-repository",
    "transaction-manager",
    "task-executor",
    "job-launcher",
    "partitioner",
    "chunk-completion-policy",
];
const JOB_NAME_KEY: &str = "spring.batch.job.name";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `<job id>`, `new JobBuilder("x", …)`, `jobBuilderFactory.get("x")`.
    Job,
    /// A step, flow, split or decision id (or a Java step name).
    State,
    /// A bean of any kind.
    Bean,
    /// A `@SpringBootApplication` class.
    BootApp,
    /// A job parameter key: `#{jobParameters['key']}` or `addString("key", …)`.
    JobParam,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub name: String,
    pub kind: Kind,
    pub range: Range,
    /// The enclosing job (or top-level flow) element of an XML state.
    pub scope: Option<usize>,
    /// The element index of an XML job or state, for matching transitions to it.
    pub el: usize,
    /// `job`, `step`, `flow`, `split` or `decision`; empty for beans.
    pub detail: &'static str,
    /// Transitions leaving an XML state: target id and its label (`on` value).
    pub goes: Vec<(String, Option<String>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    /// `next`, `to`, `restart`: a state of the same job.
    Target,
    /// `parent`: a step, flow or job anywhere.
    Parent,
    Bean,
}

/// A reference written in a batch XML attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub value: String,
    pub kind: LinkKind,
    pub range: Range,
    pub scope: Option<usize>,
}

/// The nearest enclosing `<job>`, else the outermost enclosing `<flow>`.
fn scope_of(els: &[xml::Element], i: usize) -> Option<usize> {
    let mut flow = None;
    let mut at = els[i].parent;
    while let Some(p) = at {
        match els[p].local() {
            "job" => return Some(p),
            "flow" => flow = Some(p),
            _ => {}
        }
        at = els[p].parent;
    }
    flow
}

/// The names and references of a Spring XML file.
pub fn parse_xml(text: &str) -> (Vec<Name>, Vec<Link>) {
    if !text.contains("www.springframework.org/schema/") {
        return Default::default();
    }
    let els = xml::elements(text);
    let attr_range = |a: &xml::Attr| range(text, a.value_at, a.value_end);
    let mut names: Vec<Name> = xml::bean_ids(&els)
        .into_iter()
        .map(|a| Name { name: a.value.clone(), kind: Kind::Bean, range: attr_range(a), scope: None, el: 0, detail: "", goes: Vec::new() })
        .collect();
    let mut links = Vec::new();
    if !text.contains(NAMESPACE) {
        return (names, links);
    }
    let mut owners = std::collections::BTreeMap::new();
    let mut transitions: Vec<(usize, String, Option<String>)> = Vec::new();
    for (i, e) in els.iter().enumerate().filter(|(i, _)| xml::namespace(&els, *i) == Some(NAMESPACE)) {
        let scope = scope_of(&els, i);
        if let Some(id) = e.attr("id") {
            let kind = match e.local() {
                "job" => Some(Kind::Job),
                l if STATES.contains(&l) => Some(Kind::State),
                _ => None,
            };
            if let Some(kind) = kind {
                let detail = if e.local() == "job" { "job" } else { STATES.iter().find(|s| **s == e.local()).copied().unwrap_or("") };
                owners.insert(i, names.len());
                names.push(Name { name: id.value.clone(), kind, range: attr_range(id), scope, el: i, detail, goes: Vec::new() });
            }
        }
        for a in &e.attrs {
            let kind = match a.local() {
                "next" | "to" | "restart" => LinkKind::Target,
                "parent" => LinkKind::Parent,
                l if BEAN_ATTRS.contains(&l) => LinkKind::Bean,
                _ => continue,
            };
            links.push(Link { value: a.value.trim().to_owned(), kind, range: attr_range(a), scope });
            if kind != LinkKind::Target {
                continue;
            }
            let mut owner = Some(i);
            while let Some(o) = owner.filter(|o| !STATES.contains(&els[*o].local())) {
                owner = els[o].parent;
            }
            let on = e.attr("on").map(|on| on.value.clone());
            let label = if a.local() == "restart" { Some(format!("{} restart", on.unwrap_or_default()).trim().to_owned()) } else { on };
            if let Some(owner) = owner {
                transitions.push((owner, a.value.trim().to_owned(), label));
            }
        }
    }
    for (owner, target, label) in transitions {
        if let Some(&n) = owners.get(&owner) {
            names[n].goes.push((target, label));
        }
    }
    (names, links)
}

/// The names a Java or Kotlin source declares.
pub fn parse_source(path: &str, text: &str) -> Vec<Name> {
    let file = spring_text::parse_source(path, text);
    let name = |name: String, kind, from, to| Name { name, kind, range: range(text, from, to), scope: None, el: 0, detail: "", goes: Vec::new() };
    let mut out: Vec<Name> =
        java::bean_names(&file).into_iter().map(|b| name(b.name, Kind::Bean, b.at, b.at + b.len)).collect();
    out.extend(file.types.iter().filter(|t| t.has(&["SpringBootApplication"])).map(|t| {
        name(t.name.clone(), Kind::BootApp, t.at, t.at + t.name.len())
    }));
    if !text.contains("Builder") {
        return out;
    }
    for c in spring_text::source_calls(path, text) {
        let Some(s) = c.args.first().and_then(|a| a.string.as_ref()) else { continue };
        let receiver = c.receiver.as_deref().unwrap_or_default().to_lowercase();
        let kind = match c.name.as_str() {
            "JobBuilder" if c.is_new => Kind::Job,
            "StepBuilder" if c.is_new => Kind::State,
            "get" if receiver.contains("jobbuilder") => Kind::Job,
            "get" if receiver.contains("stepbuilder") => Kind::State,
            _ => continue,
        };
        out.push(Name { detail: if kind == Kind::Job { "job" } else { "step" }, ..name(s.value.clone(), kind, s.at, s.end) });
    }
    out
}

const PARAMETER_ADDERS: [&str; 5] = ["addString", "addLong", "addDouble", "addDate", "addLocalDateTime"];

/// The job parameter keys a file reads in late-binding expressions or sets through `JobParametersBuilder`.
fn job_parameters(path: &str, text: &str) -> Vec<Name> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("jobParameters[") {
        let open = from + found + "jobParameters[".len();
        from = open;
        let Some(quote) = text[open..].chars().next().filter(|c| matches!(c, '\'' | '"')) else { continue };
        let start = open + 1;
        let Some(len) = text[start..].find(quote) else { continue };
        out.push((text[start..start + len].to_owned(), start, start + len));
    }
    if !path.ends_with(".xml") && text.contains("add") {
        for c in spring_text::source_calls(path, text).iter().filter(|c| PARAMETER_ADDERS.contains(&c.name.as_str())) {
            if let Some(s) = c.args.first().and_then(|a| a.string.as_ref()) {
                out.push((s.value.clone(), s.at, s.end));
            }
        }
    }
    out.into_iter()
        .filter(|(name, ..)| !name.is_empty() && !name.contains(['\n', '#', '$']))
        .map(|(name, from, to)| Name { name, kind: Kind::JobParam, range: range(text, from, to), scope: None, el: 0, detail: "job parameter", goes: Vec::new() })
        .collect()
}

/// The key being typed inside `jobParameters['…`: its bytes up to the closing quote.
fn parameter_key_at(text: &str, caret: usize) -> Option<(usize, usize)> {
    let head = text.get(..caret)?;
    let line_start = head.rfind('\n').map_or(0, |i| i + 1);
    let marker = head[line_start..].rfind("jobParameters[")? + line_start + "jobParameters[".len();
    let quote = head[marker..].chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let start = marker + 1;
    if head[start..].contains(quote) {
        return None;
    }
    let tail = &text[caret..];
    let end = caret + tail.find([quote, '\n']).filter(|&i| tail[i..].starts_with(quote)).unwrap_or(0);
    Some((start, end))
}

fn index_file(path: &str, text: &str) -> Option<Vec<Name>> {
    let mut names = if path.ends_with(".xml") { parse_xml(text).0 } else { parse_source(path, text) };
    if text.contains("jobParameters") || text.contains("JobParametersBuilder") {
        names.extend(job_parameters(path, text));
    }
    Some(names).filter(|n| !beans::ignored(path) && !n.is_empty())
}

/// Spring Batch 5 hints for a Java source: deprecated builder factories, and
/// `@EnableBatchProcessing` next to Spring Boot.
pub fn java_problems(text: &str, boot: bool) -> Vec<Diagnostic> {
    let file = java::parse(text);
    let imported = |at: usize| file.imports.iter().any(|i| i.at <= at && at <= i.end);
    let deprecated = |from: usize, to: usize, factory: &str| {
        let builder = factory.trim_end_matches("Factory");
        Diagnostic {
            deprecated: true,
            ..Diagnostic::new(
                range(text, from, to),
                Severity::Warning,
                format!("{factory} is deprecated since Spring Batch 5. Use new {builder}(name, jobRepository) instead."),
            )
            .code("deprecated-builder-factory")
        }
    };
    let mut out: Vec<Diagnostic> = java::tokens(text)
        .into_iter()
        .filter(|t| matches!(t.s, "JobBuilderFactory" | "StepBuilderFactory") && !imported(t.at))
        .map(|t| deprecated(t.at, t.end(), t.s))
        .collect();
    for c in java::calls(text).into_iter().filter(|c| c.name == "get") {
        let Some(receiver) = c.receiver.as_deref() else { continue };
        let lower = receiver.to_lowercase();
        let factory = if lower.contains("jobbuilder") {
            "JobBuilderFactory"
        } else if lower.contains("stepbuilder") {
            "StepBuilderFactory"
        } else {
            continue;
        };
        let written = format!("{receiver}.");
        let from = c.at.checked_sub(written.len()).filter(|&f| text[f..c.at] == written).unwrap_or(c.at);
        out.push(deprecated(from, c.at + c.name.len(), factory));
    }
    if boot {
        let enable = file.types.iter().flat_map(|t| &t.annotations).filter(|a| a.name == "EnableBatchProcessing");
        out.extend(enable.map(|a| {
            Diagnostic::new(
                range(text, a.at, a.end),
                Severity::Info,
                "With Spring Boot 3, @EnableBatchProcessing switches off Boot's batch auto-configuration (running jobs at startup, schema initialisation, spring.batch.* properties). Remove it unless you want that.",
            )
            .code("enable-batch-processing-with-boot")
        }));
    }
    out
}

/// The job names of a `spring.batch.job.name` value (comma-separated): `(name, from, to)` bytes.
fn job_name_items(path: &str, text: &str) -> Vec<(String, usize, usize)> {
    let key = config::canonical(JOB_NAME_KEY);
    let mut out = Vec::new();
    for k in config::keys(path, text).into_iter().filter(|k| config::canonical(&k.key) == key) {
        let mut at = k.value_at;
        for part in text[k.value_at..k.value_end].split(',') {
            let lead = part.len() - part.trim_start().len();
            out.push((part.trim().to_owned(), at + lead, at + lead + part.trim().len()));
            at += part.len() + 1;
        }
    }
    out
}

fn file_kind(doc: &Document) -> Option<&'static str> {
    match doc.language.as_str() {
        "xml" if doc.text.contains(NAMESPACE) => Some("xml"),
        "java" if doc.text.contains("batch") => Some("java"),
        _ if config::is_config_file(&doc.path) => Some("config"),
        _ => None,
    }
}

fn all(index: &Index<Vec<Name>>) -> impl Iterator<Item = (&str, &Name)> {
    index.iter().flat_map(|(path, names)| names.iter().map(move |n| (path, n)))
}

fn locations<'a>(found: impl Iterator<Item = (&'a str, &'a Name)>) -> Vec<Location> {
    found.map(|(path, n)| Location { path: path.into(), range: n.range }).collect()
}

fn completions<'a>(names: impl Iterator<Item = &'a str>, detail: &str, replace: Option<Range>) -> Vec<CompletionItem> {
    let names: BTreeSet<&str> = names.filter(|n| !n.is_empty() && !beans::is_expression(n)).collect();
    names
        .into_iter()
        .map(|n| CompletionItem { detail: Some(detail.into()), replace, ..CompletionItem::new(n, CompletionKind::Reference) })
        .collect()
}

pub struct Batch {
    indexes: Indexes<Vec<Name>>,
}

impl Default for Batch {
    fn default() -> Self {
        Self { indexes: Indexes::new(&beans::GLOBS, index_file) }
    }
}

impl Batch {
    fn index(&mut self, doc: &Document) -> Option<&Index<Vec<Name>>> {
        self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).map(|i| &*i)
    }
}

/// The XML link under the caret, with the names of its file.
fn xml_link(doc: &Document, at: Position) -> Option<(Link, Vec<Name>)> {
    let (names, links) = parse_xml(&doc.text);
    let link = links.into_iter().find(|l| contains(l.range, at))?;
    Some((link, names))
}

/// The names of the other files of the project.
fn elsewhere<'a>(index: &'a Index<Vec<Name>>, doc: &Document) -> Vec<(&'a str, &'a Name)> {
    all(index).filter(|(p, _)| *p != doc.path).collect()
}

/// Where an XML link may lead: the same job first, then the file, then the project.
fn targets(link: &Link, names: &[Name], project: &[(&str, &Name)], path: &str) -> Vec<Location> {
    let here = |n: &Name| Location { path: path.into(), range: n.range };
    let elsewhere = |kinds: &[Kind]| {
        locations(project.iter().copied().filter(|(_, n)| kinds.contains(&n.kind) && n.name == link.value))
    };
    if link.kind == LinkKind::Bean {
        let mut found: Vec<_> = names.iter().filter(|n| n.kind == Kind::Bean && n.name == link.value).map(here).collect();
        found.extend(elsewhere(&[Kind::Bean]));
        return found;
    }
    let kinds = [Kind::State, Kind::Job];
    let named = |n: &&Name| kinds.contains(&n.kind) && n.name == link.value;
    let same_job: Vec<_> =
        names.iter().filter(named).filter(|n| link.kind == LinkKind::Target && n.scope == link.scope).map(here).collect();
    if !same_job.is_empty() {
        return same_job;
    }
    let in_file: Vec<_> = names.iter().filter(named).map(here).collect();
    if !in_file.is_empty() {
        return in_file;
    }
    elsewhere(&kinds)
}

fn xml_problems(text: &str, index: &Index<Vec<Name>>) -> Vec<Diagnostic> {
    let (names, links) = parse_xml(text);
    let mut out = Vec::new();
    for l in links.iter().filter(|l| !l.value.is_empty() && !beans::is_expression(&l.value)) {
        match l.kind {
            LinkKind::Target if l.scope.is_some() => {
                if !names.iter().any(|n| n.kind == Kind::State && n.scope == l.scope && n.name == l.value) {
                    out.push(
                        Diagnostic::new(l.range, Severity::Error, format!("This job has no step, flow, split or decision '{}'.", l.value))
                            .code("unknown-transition-target"),
                    );
                }
            }
            LinkKind::Bean if index.complete() => {
                let single = !l.value.contains([',', ' ']);
                let known = all(index).any(|(_, n)| n.kind == Kind::Bean && n.name == l.value);
                if single && !known && !beans::INFRASTRUCTURE.contains(&l.value.as_str()) {
                    out.push(
                        Diagnostic::new(l.range, Severity::Warning, format!("No bean named '{}' is declared in the project.", l.value))
                            .code("unknown-bean"),
                    );
                }
            }
            _ => {}
        }
    }
    out
}

/// A `<chunk>` needs a `commit-interval` or a `chunk-completion-policy` (Spring Batch reference, "Configuring a Step"). A step that inherits (`parent`) or is `abstract` may get it from the other side of the inheritance, so it is left alone.
fn chunk_problems(text: &str) -> Vec<Diagnostic> {
    if !text.contains(NAMESPACE) || !text.contains("chunk") {
        return Vec::new();
    }
    let els = xml::elements(text);
    let inherits = |i: usize| {
        let mut at = Some(i);
        while let Some(e) = at.map(|a| &els[a]) {
            if e.local() == "step" && (e.attr("parent").is_some() || e.attr("abstract").is_some()) {
                return true;
            }
            at = e.parent;
        }
        false
    };
    els.iter()
        .enumerate()
        .filter(|(i, e)| e.local() == "chunk" && xml::namespace(&els, *i) == Some(NAMESPACE))
        .filter(|(_, e)| e.attr("commit-interval").is_none() && e.attr("chunk-completion-policy").is_none())
        .filter(|(i, _)| !inherits(*i))
        .map(|(_, e)| {
            Diagnostic::new(
                range(text, e.at, e.at + e.name.len()),
                Severity::Warning,
                "A chunk needs a commit-interval or a chunk-completion-policy; without either the step fails to start.",
            )
            .code("chunk-missing-commit-interval")
        })
        .collect()
}

fn config_problems(doc: &Document, index: &Index<Vec<Name>>) -> Vec<Diagnostic> {
    if !index.complete() {
        return Vec::new();
    }
    job_name_items(&doc.path, &doc.text)
        .into_iter()
        .filter(|(n, _, _)| !n.is_empty() && !beans::is_expression(n))
        .filter(|(n, _, _)| !all(index).any(|(_, x)| x.kind == Kind::Job && x.name == *n))
        .map(|(n, from, to)| {
            Diagnostic::new(range(&doc.text, from, to), Severity::Warning, format!("No job named '{n}' is declared in the project."))
                .code("unknown-job")
        })
        .collect()
}

impl Providers for Batch {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if let Some(caret) = offset(&doc.text, at)
            && let Some((from, to)) = parameter_key_at(&doc.text, caret)
            && matches!(file_kind(doc), Some("xml" | "java"))
        {
            let Some(index) = self.index(doc) else { return Vec::new() };
            let typing = range(&doc.text, from, to);
            let keys = all(index).filter(|(p, n)| n.kind == Kind::JobParam && !(*p == doc.path && n.range == typing));
            return completions(keys.map(|(_, n)| n.name.as_str()), "job parameter", Some(typing));
        }
        match file_kind(doc) {
            Some("xml") => {
                let Some((link, names)) = xml_link(doc, at) else { return Vec::new() };
                let Some(index) = self.index(doc) else { return Vec::new() };
                let (kinds, detail) = match link.kind {
                    LinkKind::Target => (&[Kind::State][..], "step, flow, split or decision"),
                    LinkKind::Parent => (&[Kind::State, Kind::Job][..], "step, flow or job"),
                    LinkKind::Bean => (&[Kind::Bean][..], "bean"),
                };
                let local = names
                    .iter()
                    .filter(|n| kinds.contains(&n.kind) && (link.kind != LinkKind::Target || n.scope == link.scope));
                let elsewhere = elsewhere(index, doc)
                    .into_iter()
                    .map(|(_, n)| n)
                    .filter(|n| link.kind != LinkKind::Target && kinds.contains(&n.kind));
                completions(local.chain(elsewhere).map(|n| n.name.as_str()), detail, Some(link.range))
            }
            Some("config") => {
                let caret = offset(&doc.text, at).unwrap_or(0);
                let item = job_name_items(&doc.path, &doc.text).into_iter().find(|(_, from, to)| *from <= caret && caret <= *to);
                // An empty YAML value ends at its colon: a caret after it on the line still counts.
                let on_key = config::keys(&doc.path, &doc.text).into_iter().any(|k| {
                    config::canonical(&k.key) == config::canonical(JOB_NAME_KEY)
                        && k.end <= caret
                        && doc.text.get(k.end..caret).is_some_and(|s| !s.contains('\n'))
                });
                if item.is_none() && !on_key {
                    return Vec::new();
                }
                let replace = item.map(|(_, from, to)| range(&doc.text, from, to));
                let Some(index) = self.index(doc) else { return Vec::new() };
                completions(all(index).filter(|(_, n)| n.kind == Kind::Job).map(|(_, n)| n.name.as_str()), "batch job", replace)
            }
            _ => Vec::new(),
        }
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        match file_kind(doc) {
            Some("xml") => {
                let Some((link, names)) = xml_link(doc, at) else { return Vec::new() };
                let Some(index) = self.index(doc) else { return Vec::new() };
                targets(&link, &names, &elsewhere(index, doc), &doc.path)
            }
            Some("config") => {
                let caret = offset(&doc.text, at).unwrap_or(0);
                let Some((name, _, _)) = job_name_items(&doc.path, &doc.text).into_iter().find(|(_, f, t)| *f <= caret && caret <= *t)
                else {
                    return Vec::new();
                };
                let Some(index) = self.index(doc) else { return Vec::new() };
                locations(all(index).filter(|(_, n)| n.kind == Kind::Job && n.name == name))
            }
            _ => Vec::new(),
        }
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let kind = file_kind(doc);
        let Some(index) = kind.and_then(|_| self.index(doc)) else { return Vec::new() };
        let problems = match kind {
            Some("xml") => {
                let mut found = xml_problems(&doc.text, index);
                if host::setting_bool("springBatch.checkChunk").unwrap_or(true) {
                    found.extend(chunk_problems(&doc.text));
                }
                found.extend(xml_rules::problems(&doc.text));
                found
            }
            Some("java") => java_problems(&doc.text, all(index).any(|(_, n)| n.kind == Kind::BootApp)),
            _ => config_problems(doc, index),
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

const VIEW: &str = "diagram";

fn location_key(path: &str, range: Range) -> String {
    format!("b:{}:{}:{path}", range.start.line, range.start.character)
}

/// Jobs and their steps, flows, splits and decisions as nodes; the job points at its first state, states at the states they lead to.
fn diagram_of(index: &Index<Vec<Name>>) -> (Vec<Node>, Vec<Edge>) {
    let shown = |n: &&Name| matches!(n.kind, Kind::Job | Kind::State);
    let mut nodes = Vec::new();
    let mut edges = BTreeSet::new();
    for (path, names) in index.iter() {
        let id = |n: &Name| location_key(path, n.range);
        for n in names.iter().filter(shown) {
            nodes.push(Node::new(&id(n), &n.name).detail(n.detail).accent(n.kind == Kind::Job));
            for (target, label) in &n.goes {
                let found = names.iter().find(|t| t.kind == Kind::State && t.scope == n.scope && t.name == *target);
                if let Some(t) = found {
                    edges.insert((id(n), id(t), label.clone()));
                }
            }
            if n.kind == Kind::Job || n.detail == "flow" {
                if let Some(first) = names.iter().find(|t| t.kind == Kind::State && t.scope == Some(n.el)) {
                    edges.insert((id(n), id(first), None));
                }
            }
        }
    }
    let edges = edges.into_iter().map(|(a, b, label)| {
        let edge = Edge::new(&a, &b);
        match label {
            Some(l) => edge.label(&l),
            None => edge,
        }
    });
    (nodes, edges.collect())
}

impl Views for Batch {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let Some(project) = ctx.project() else { return Ok(View::message("Open a project to see its batch jobs.").button(refresh)) };
        let index = self.indexes.get(&project, COMMAND_BUDGET);
        let complete = index.complete();
        if !complete {
            host::refresh_view(VIEW);
        }
        let (nodes, edges) = diagram_of(index);
        let status = format!("{} nodes · {} transitions{}", nodes.len(), edges.len(), if complete { "" } else { " · scanning…" });
        Ok(View::new(Body::Html(diagram::html(&nodes, &edges, "No batch jobs found."))).button(refresh).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let key = match event {
            ViewEvent::Button(b) if b == "refresh" => {
                // Resetting a scan in progress would throw away what it has read.
                if let Some(project) = ctx.project()
                    && self.indexes.get(&project, std::time::Duration::ZERO).complete()
                {
                    self.indexes.reset();
                }
                host::refresh_view(view);
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

impl Plugin for Batch {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(Batch, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const JOB_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<beans:beans xmlns="http://www.springframework.org/schema/batch"
             xmlns:beans="http://www.springframework.org/schema/beans">
    <job id="importJob" job-repository="jobRepository">
        <step id="load" next="check">
            <tasklet transaction-manager="transactionManager">
                <chunk reader="csvReader" writer="dbWriter" processor="missingProcessor" commit-interval="10"/>
            </tasklet>
        </step>
        <decision id="check" decider="importDecider">
            <next on="FAILED" to="report"/>
            <next on="*" to="lost"/>
        </decision>
        <step id="report" parent="baseStep">
            <tasklet ref="reportTasklet"/>
            <stop on="RETRY" restart="load"/>
        </step>
    </job>
    <step id="baseStep" abstract="true"/>
    <beans:bean id="csvReader" class="com.acme.CsvReader"/>
</beans:beans>
"#;
    const JOB_CONFIG: &str = r#"package com.acme;

import org.springframework.batch.core.configuration.annotation.EnableBatchProcessing;
import org.springframework.batch.core.configuration.annotation.JobBuilderFactory;

@Configuration
@EnableBatchProcessing
public class BatchConfig {
    @Autowired private JobBuilderFactory jobs;

    @Bean
    public Job exportJob(JobRepository repo, Step step) {
        return new JobBuilder("exportJob", repo).start(step).build();
    }

    @Bean
    public Job legacyJob() {
        return jobBuilderFactory.get("legacyJob").start(step()).build();
    }

    @Bean ItemWriter<Row> dbWriter() { return null; }
    @Bean Tasklet reportTasklet() { return null; }
    @Bean JobExecutionDecider importDecider() { return null; }
}
"#;
    const APP: &str = "package com.acme;\n\n@SpringBootApplication\npublic class App {}\n";
    const XML_PATH: &str = "/project/src/main/resources/jobs/import.xml";
    const JAVA_PATH: &str = "/project/src/main/java/com/acme/BatchConfig.java";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/jobs/import.xml", JOB_XML);
        host.add_file("src/main/java/com/acme/BatchConfig.java", JOB_CONFIG);
        host.add_file("src/main/java/com/acme/App.java", APP);
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + 1)
    }

    fn after(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + needle.len())
    }

    #[test]
    fn reads_jobs_states_and_links() {
        let (names, links) = parse_xml(JOB_XML);
        let named = |kind| names.iter().filter(|n| n.kind == kind).map(|n| n.name.as_str()).collect::<Vec<_>>();
        assert_eq!(named(Kind::Job), ["importJob"]);
        assert_eq!(named(Kind::State), ["load", "check", "report", "baseStep"]);
        assert_eq!(named(Kind::Bean), ["importJob", "baseStep", "csvReader"]);
        let targets: Vec<_> = links.iter().filter(|l| l.kind == LinkKind::Target).map(|l| l.value.as_str()).collect();
        assert_eq!(targets, ["check", "report", "lost", "load"]);
        assert!(parse_xml("<beans><job id=\"x\"/></beans>").0.is_empty());

        let java: Vec<_> = parse_source(JAVA_PATH, JOB_CONFIG).into_iter().filter(|n| n.kind == Kind::Job).map(|n| n.name).collect();
        assert_eq!(java, ["exportJob", "legacyJob"], "builder names, not bean names");
    }

    #[test]
    fn navigates_transitions_parents_and_beans() {
        let _host = setup();
        let mut b = Batch::default();
        let d = doc(XML_PATH, "xml", JOB_XML);
        let line = |found: Vec<Location>| found.iter().map(|l| (l.path.rsplit('/').next().unwrap().to_owned(), l.range.start.line)).collect::<Vec<_>>();
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"check\""))), [("import.xml".into(), 9)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"load\"/>"))), [("import.xml".into(), 4)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"baseStep\""))), [("import.xml".into(), 18)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"dbWriter\""))), [("BatchConfig.java".into(), 20)]);
        assert_eq!(line(b.definition(&d, at(JOB_XML, "\"csvReader\""))), [("import.xml".into(), 19)]);
    }

    #[test]
    fn completes_states_and_beans() {
        let _host = setup();
        let mut b = Batch::default();
        let d = doc(XML_PATH, "xml", JOB_XML);
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(b.completion(&d, at(JOB_XML, "\"lost\""), None)), ["check", "load", "report"]);
        let beans = labels(b.completion(&d, at(JOB_XML, "\"csvReader\""), None));
        assert!(beans.contains(&"dbWriter".to_owned()) && beans.contains(&"csvReader".to_owned()) && beans.contains(&"app".to_owned()));
    }

    #[test]
    fn reports_unknown_targets_and_beans() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(XML_PATH, "xml", JOB_XML));
        let found: Vec<_> =
            host.diagnostics()[XML_PATH].iter().map(|d| (d.range.start.line, d.code.clone().unwrap(), d.severity)).collect();
        assert_eq!(found, [
            (6, "unknown-bean".into(), Severity::Warning),
            (11, "unknown-transition-target".into(), Severity::Error),
        ]);
        assert!(host.diagnostics()[XML_PATH][0].message.contains("'missingProcessor'"));
    }

    fn chunk_codes(text: &str) -> Vec<(u32, u32)> {
        chunk_problems(text).iter().map(|d| (d.range.start.line, d.range.start.character)).collect()
    }

    const CHUNKLESS: &str = "<job id=\"j\" xmlns=\"http://www.springframework.org/schema/batch\">\n  <step id=\"é\"><tasklet><chunk reader=\"r\" writer=\"w\"/></tasklet></step>\n</job>";

    #[test]
    fn chunk_missing_commit_interval_warns() {
        assert_eq!(chunk_codes(CHUNKLESS), [(1, 25)]);
    }

    #[test]
    fn chunk_missing_commit_interval_is_quiet_for_library_or_dynamic_name() {
        assert!(chunk_codes(&CHUNKLESS.replace("reader=", "commit-interval=\"${size}\" reader=")).is_empty());
        assert!(chunk_codes(&CHUNKLESS.replace("reader=", "chunk-completion-policy=\"p\" reader=")).is_empty());
        assert!(chunk_codes(&CHUNKLESS.replace("id=\"é\"", "id=\"é\" parent=\"base\"")).is_empty());
        assert!(chunk_codes(&CHUNKLESS.replace("id=\"é\"", "id=\"é\" abstract=\"true\"")).is_empty());
        assert!(chunk_codes(&CHUNKLESS.replace("http://www.springframework.org/schema/batch", "urn:other")).is_empty());
    }

    const BROKEN: &str = "<job xmlns=\"http://www.springframework.org/schema/batch\" restartable=\"yes\">\n  <step><tasklet allow-start-if-complete=\"maybe\"/></step>\n  <decision id=\"d\"><next on=\"*\"/><end/><stop on=\"X\"/></decision>\n</job>";

    #[test]
    fn xml_rules_report_missing_attributes_and_wrong_booleans() {
        let found: Vec<_> = xml_rules::problems(BROKEN)
            .iter()
            .map(|d| (d.code.clone().unwrap(), BROKEN.lines().nth(d.range.start.line as usize).unwrap()[d.range.start.character as usize..d.range.end.character as usize].to_owned()))
            .collect();
        assert_eq!(found, [
            ("missing-attribute".to_owned(), "job".to_owned()),
            ("invalid-attribute-value".into(), "yes".into()),
            ("missing-attribute".into(), "step".into()),
            ("invalid-attribute-value".into(), "maybe".into()),
            ("missing-attribute".into(), "decision".into()),
            ("missing-attribute".into(), "next".into()),
            ("missing-attribute".into(), "end".into()),
            ("missing-attribute".into(), "stop".into()),
        ]);
    }

    #[test]
    fn xml_rules_are_quiet_for_valid_dynamic_and_foreign_elements() {
        assert!(xml_rules::problems(JOB_XML).is_empty());
        let dynamic = BROKEN.replace("restartable=\"yes\"", "id=\"j\" restartable=\"${r}\"").replace("<step>", "<step id=\"s\">").replace("maybe", "true");
        let codes: Vec<_> = xml_rules::problems(&dynamic).iter().map(|d| d.message.clone()).collect();
        assert!(codes.iter().all(|m| m.contains("decider") || m.contains("\"to\"") || m.contains("\"on\"") || m.contains("restart")), "{codes:?}");
        assert!(xml_rules::problems(&BROKEN.replace("http://www.springframework.org/schema/batch", "urn:other")).is_empty());
    }

    const PARAMS_JAVA: &str = "package a;\nimport org.springframework.batch.core.JobParametersBuilder;\nclass P {\n  @Value(\"#{jobParameters['inputFile']}\") String f;\n  void go() { new JobParametersBuilder().addString(\"runId\", \"1\").addLong(\"size\", 2L); }\n}\n";

    #[test]
    fn completes_job_parameter_keys_in_java_and_xml() {
        let host = setup();
        host.add_file("src/main/java/a/P.java", PARAMS_JAVA);
        let mut b = Batch::default();
        let xml = JOB_XML.replace("reader=\"csvReader\"", "reader=\"#{jobParameters['in']}\"");
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        let d = doc(XML_PATH, "xml", &xml);
        assert_eq!(labels(b.completion(&d, after(&xml, "jobParameters['i"), None)), ["inputFile", "runId", "size"]);
        assert!(labels(b.completion(&d, at(&xml, "\"dbWriter\""), None)).contains(&"csvReader".to_owned()), "bean completion stays");
        let j = doc("/project/src/main/java/a/P.java", "java", PARAMS_JAVA);
        let items = b.completion(&j, after(PARAMS_JAVA, "jobParameters['inp"), None);
        assert_eq!(items.len(), 3);
        assert!(b.completion(&j, after(PARAMS_JAVA, "inputFile'"), None).is_empty(), "closed key");
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        host.add_file("a.xml", CHUNKLESS);
        let mut b = Batch::default();
        b.line_markers(&doc("/project/a.xml", "xml", CHUNKLESS));
        let chunks = || {
            host.diagnostics()["/project/a.xml"]
                .iter()
                .filter(|d| d.code.as_deref() == Some("chunk-missing-commit-interval"))
                .count()
        };
        assert_eq!(chunks(), 1);
        let fixed = CHUNKLESS.replace("reader=", "commit-interval=\"5\" reader=");
        b.line_markers(&doc("/project/a.xml", "xml", &fixed));
        assert_eq!(chunks(), 0);
    }

    #[test]
    fn flags_spring_batch_5_migrations() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(JAVA_PATH, "java", JOB_CONFIG));
        let found: Vec<_> = host.diagnostics()[JAVA_PATH]
            .iter()
            .map(|d| (d.range.start.line, d.code.clone().unwrap(), d.deprecated))
            .collect();
        assert_eq!(found, [
            (8, "deprecated-builder-factory".into(), true),
            (17, "deprecated-builder-factory".into(), true),
            (6, "enable-batch-processing-with-boot".into(), false),
        ]);
        let get = &host.diagnostics()[JAVA_PATH][1];
        assert_eq!((get.range.start.character, get.range.end.character), (15, 36));
        assert!(get.message.contains("new JobBuilder(name, jobRepository)"));

        let no_boot = java_problems(JOB_CONFIG, false);
        assert!(no_boot.iter().all(|d| d.code.as_deref() != Some("enable-batch-processing-with-boot")));
    }

    #[test]
    fn links_the_job_name_property() {
        let host = setup();
        let mut b = Batch::default();
        b.line_markers(&doc(XML_PATH, "xml", JOB_XML));
        let props = "spring.batch.job.name=importJob, nightlyJob\n";
        let path = "/project/src/main/resources/application.properties";
        let d = doc(path, "properties", props);
        let found = b.definition(&d, at(props, "importJob"));
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), (XML_PATH, 3));
        b.line_markers(&d);
        let problems = &host.diagnostics()[path];
        assert_eq!(problems.len(), 1);
        assert!(problems[0].message.contains("'nightlyJob'"));
        assert_eq!(problems[0].range.start.character, 33);

        let yaml = "spring:\n  batch:\n    job:\n      name: \n";
        let d = doc("/project/src/main/resources/application.yml", "yaml", yaml);
        let items = b.completion(&d, Position::new(3, 12), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["exportJob", "importJob", "legacyJob"]);
        assert!(b.completion(&doc(path, "properties", "server.port=1\n"), Position::new(0, 13), None).is_empty());
    }

    #[test]
    fn batch_diagram_has_expected_nodes_and_edges() {
        let host = setup();
        let (nodes, edges) = diagram_of(Batch::default().indexes.get(&host.invocation().project().unwrap(), COMMAND_BUDGET));
        let label_of = |id: &str| nodes.iter().find(|n| n.id == id).map(|n| n.label.as_str()).unwrap_or("?");
        let mut labels: Vec<(&str, Option<&str>)> = nodes.iter().map(|n| (n.label.as_str(), n.detail.as_deref())).collect();
        labels.sort();
        assert_eq!(labels, [
            ("baseStep", Some("step")),
            ("check", Some("decision")),
            ("exportJob", Some("job")),
            ("importJob", Some("job")),
            ("legacyJob", Some("job")),
            ("load", Some("step")),
            ("report", Some("step")),
        ]);
        let mut pairs: Vec<(&str, &str, Option<&str>)> =
            edges.iter().map(|e| (label_of(&e.from), label_of(&e.to), e.label.as_deref())).collect();
        pairs.sort();
        assert_eq!(pairs, [
            ("check", "report", Some("FAILED")),
            ("importJob", "load", None),
            ("load", "check", None),
            ("report", "load", Some("RETRY restart")),
        ]);

        let mut b = Batch::default();
        let view = b.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Html(page) = view.body else { panic!("html expected") };
        let job = position(JOB_XML, JOB_XML.find("importJob").unwrap());
        let job_key = format!("b:{}:{}:{XML_PATH}", job.line, job.character);
        assert!(page.contains(&format!("data-id=\"{job_key}\"")), "{page}");
        assert_eq!(view.status.as_deref(), Some("7 nodes · 4 transitions"));
        b.handle(VIEW, ViewEvent::Message(job_key), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [(XML_PATH.to_string(), job.line, job.character)]);
    }

    #[test]
    fn refresh_during_scan_keeps_the_partial_index() {
        let host = setup();
        let mut b = Batch::default();
        let project = host.invocation().project().unwrap();
        b.indexes.get(&project, std::time::Duration::ZERO).update(XML_PATH, JOB_XML);
        b.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(b.indexes.get(&project, std::time::Duration::ZERO).files().len(), 1, "not reset");

        b.indexes.get(&project, COMMAND_BUDGET);
        b.handle(VIEW, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert!(b.indexes.get(&project, std::time::Duration::ZERO).files().is_empty(), "a complete index starts over");
    }

    #[test]
    fn manifest_activates_on_the_diagram_view() {
        assert!(include_str!("../plugin.toml").contains("\"on-view:diagram\""));
    }

    #[test]
    fn batch_diagram_is_empty_without_jobs() {
        let host = MockHost::install();
        let view = Batch::default().render(VIEW, None, &host.invocation()).unwrap();
        let Body::Html(page) = view.body else { panic!("html expected") };
        assert!(page.contains("No batch jobs found."));
    }
}
