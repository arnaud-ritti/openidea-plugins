//! Symfony: completion, navigation, problems, gutter markers and a tool
//! window for routes, Twig templates, services, translations, classes,
//! assets, entries and roles in PHP, Twig, YAML and XML files.
//!
//! Partial port of the Symfony Plugin for IntelliJ (Daniel Espendiller, Adrien
//! Brault; MIT), see README.md for what was and wasn't ported.

mod checks;
mod config_checks;
mod index;
mod intentions;
mod php_text;
mod profiler;
mod refs;
mod twig;
mod twig_checks;
mod yaml_quotes;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, Table, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Diagnostic, Event, EventKind, Location, Position, Range, Severity, TextEdit};
use openidea_plugin::{Error, Invocation, LogLevel, NotificationLevel, Plugin, Project, StatusItem, host, register_plugin, text};

pub use index::Index;
use index::{Entry, Names, Sink};
use refs::{Kind, Reference};

const VIEW: &str = "symfony";
const STATUS_ITEM: &str = "symfony";
const PROFILER_VIEW: &str = "symfony-profiler";
const SOURCE: &str = "Symfony";
const DEFAULT_DOMAIN: &str = "messages";
const CHECKED_LANGUAGES: &[(&str, &str)] = &[(".php", "php"), (".twig", "twig"), (".yaml", "yaml"), (".yml", "yaml"), (".xml", "xml")];
/// Whether a saved file (project-relative) feeds the index.
fn affects_index(relative: &str) -> bool {
    let directory = ["config/", "templates/", "translations/"].iter().any(|d| relative.starts_with(d));
    let source = relative.starts_with("src/") && relative.ends_with(".php");
    directory || source || matches!(relative, "composer.json" | "webpack.config.js" | "importmap.php")
}
/// Parent templates followed for blocks (`extends` chains are short; this stops cycles).
const MAX_PARENTS: usize = 8;

fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn project_for(path: &str) -> Option<Project> {
    host::projects().ok()?.into_iter().filter(|p| p.root().is_ok_and(|root| under(path, &root))).max_by_key(|p| p.root().map_or(0, |r| r.len()))
}

fn language_of(path: &str) -> Option<&'static str> {
    CHECKED_LANGUAGES.iter().find(|(ext, _)| path.ends_with(ext)).map(|(_, language)| *language)
}

fn range_of(text: &str, start: usize, end: usize) -> Range {
    Range::new(text::position(text, start), text::position(text, end))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn location(entry: &Entry, width: u32) -> Option<Location> {
    if entry.path.is_empty() {
        return None;
    }

    let end = Position::new(entry.at.line, entry.at.character + width);
    Some(Location { path: entry.path.clone(), range: Range::new(entry.at, end) })
}

/// Blocks of the templates `text` extends, nearest first, with the template
/// defining each as detail.
fn parent_blocks(index: &Index, project: &Project, text: &str) -> Names {
    let mut blocks = Names::new();
    let mut parent = refs::twig_parent(text).map(str::to_owned);

    for _ in 0..MAX_PARENTS {
        let Some(name) = parent.take() else { break };
        let Some(entry) = index.templates.get(&name) else { break };
        let Ok(parent_text) = project.read_file(&entry.path) else { break };

        for (block, offset) in refs::twig_blocks(&parent_text) {
            blocks.add(block, Some(name.clone()), &entry.path, text::position(&parent_text, offset));
        }
        parent = refs::twig_parent(&parent_text).map(str::to_owned);
    }

    blocks
}

/// The domain a translation reference uses: its own, or `messages`.
fn domain_of(doc_language: &str, text: &str, reference: &Reference) -> String {
    refs::translation_domain(doc_language, text, reference).unwrap_or_else(|| DEFAULT_DOMAIN.into())
}

/// Names a reference can name, for completion and navigation.
fn candidates<'a>(index: &'a Index, path: &str, language: &str, text: &str, reference: &Reference) -> Cow<'a, Names> {
    match reference.kind {
        Kind::Route => Cow::Borrowed(&index.routes),
        Kind::Template => Cow::Borrowed(&index.templates),
        Kind::Service => Cow::Borrowed(&index.services),
        Kind::Class => Cow::Borrowed(&index.classes),
        Kind::Asset => Cow::Borrowed(&index.assets),
        Kind::Entry => Cow::Borrowed(&index.entries),
        Kind::Role => Cow::Borrowed(&index.roles),
        Kind::Translation => {
            if let Some(names) = index.translations.get(&domain_of(language, text, reference)) {
                return Cow::Borrowed(names);
            }

            let mut all = Names::new();
            for names in index.translations.values() {
                for (key, entry) in names {
                    all.entry(key.clone()).or_insert_with(|| entry.clone());
                }
            }
            Cow::Owned(all)
        }
        Kind::Block => Cow::Owned(project_for(path).map(|p| parent_blocks(index, &p, text)).unwrap_or_default()),
    }
}

fn completion_kind(kind: Kind) -> CompletionKind {
    match kind {
        Kind::Route => CompletionKind::Reference,
        Kind::Template | Kind::Asset => CompletionKind::File,
        Kind::Service | Kind::Class => CompletionKind::Class,
        Kind::Translation => CompletionKind::Text,
        Kind::Entry => CompletionKind::Module,
        Kind::Role => CompletionKind::Constant,
        Kind::Block => CompletionKind::Field,
    }
}

/// Variables a template gets from its controllers' render calls.
fn twig_variables(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    let Some((start, end)) = refs::twig_variable_at(&doc.text, offset) else { return Vec::new() };
    let template = index.template_of(&doc.path);
    let mut seen = BTreeSet::new();
    index
        .renders
        .iter()
        .filter(|render| Some(render.template.as_str()) == template)
        .flat_map(|render| render.variables.iter().map(move |variable| (variable, &render.caller)))
        .filter(|(variable, _)| seen.insert(variable.as_str()))
        .map(|(variable, caller)| {
            let mut item = CompletionItem::new(variable, CompletionKind::Variable);
            item.detail = Some(caller.clone());
            item.replace = Some(range_of(&doc.text, start, end));
            item
        })
        .collect()
}

/// Twig tags, filters, functions and tests, plus variables where an expression can start.
fn twig_names(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    let Some((slot, start, end)) = twig::slot_at(&doc.text, offset) else { return Vec::new() };
    let kind = match slot {
        twig::Slot::Tag => CompletionKind::Keyword,
        _ => CompletionKind::Function,
    };
    let replace = range_of(&doc.text, start, end);
    let mut items: Vec<CompletionItem> = twig::names(&slot)
        .iter()
        .map(|name| {
            let mut item = CompletionItem::new(name, kind);
            item.replace = Some(replace);
            item
        })
        .collect();

    if matches!(slot, twig::Slot::Function) {
        items.extend(twig_variables(index, doc, offset));
    }

    items
}

/// A name that is built at run time (`'blog/' ~ name`, `"{$x}"`): not checked.
fn is_dynamic(name: &str) -> bool {
    name.is_empty() || name.contains(['{', '~', '$', '#'])
}

pub(crate) fn is_quoted_config(path: &str) -> bool {
    path.contains("/config/") || path.contains("/translations/")
}

/// Problems of a file: missing routes, templates, services and translation
/// keys (the plugin's *Missing* inspections), and routes defined twice.
pub fn problems(index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    let mut problems = Vec::new();
    let mut problem = |start: usize, end: usize, code: &str, message: String| {
        let severity = match code {
            "missing-translation" => Severity::Info,
            "unclosed-tag" => Severity::Error,
            _ => Severity::Warning,
        };
        problems.push(Diagnostic {
            range: range_of(text, start, end),
            severity,
            message,
            code: Some(code.into()),
            source: Some(SOURCE.into()),
            unnecessary: false,
            deprecated: false,
            related: Vec::new(),
        });
    };

    for reference in refs::references(language, text) {
        let name = &text[reference.start..reference.end];
        if is_dynamic(name) {
            continue;
        }

        match reference.kind {
            Kind::Route if language != "twig" && index.compiled_routes && !index.routes.contains_key(name) => {
                problem(reference.start, reference.end, "missing-route", format!("Route \"{name}\" is not defined."));
            }
            Kind::Template if !name.starts_with('@') && !index.templates.contains_key(name) => {
                problem(reference.start, reference.end, "missing-template", format!("Template \"{name}\" does not exist in templates/."));
            }
            Kind::Service if index.compiled_container && !index.services.contains_key(name) => {
                problem(reference.start, reference.end, "missing-service", format!("Service \"{name}\" is not defined."));
            }
            Kind::Translation => {
                let domain = domain_of(language, text, &reference);
                if index.translations.get(&domain).is_some_and(|keys| !keys.contains_key(name)) {
                    let message = format!("Translation key \"{name}\" is missing from domain \"{domain}\".");
                    problem(reference.start, reference.end, "missing-translation", message);
                }
            }
            _ => {}
        }
    }

    if language == "twig" {
        for (range, name) in twig::unclosed_tags(text) {
            problem(range.start, range.end, "unclosed-tag", format!("Tag \"{name}\" is never closed: missing {{% end{name} %}}."));
        }
    }

    if language == "xml" && !path.contains("/var/cache/") {
        let mut services: Vec<(String, Position)> = Vec::new();
        index::xml_services(path, text, &mut services);
        for (i, (id, at)) in services.iter().enumerate() {
            if services[..i].iter().any(|(earlier, _)| earlier == id) {
                let start = text::offset(text, *at).unwrap_or(0);
                problem(start, start + id.len(), "duplicate-service", format!("Service \"{id}\" is already defined in this file."));
            }
        }
    }

    let mut routes: Vec<(String, Position)> = Vec::new();
    match language {
        "php" => index::php_routes(path, text, &mut routes),
        "yaml" if path.contains("/config/routes") => index::yaml_routes(path, text, &mut routes),
        _ => {}
    }
    for (i, (name, at)) in routes.iter().enumerate() {
        if routes[..i].iter().any(|(earlier, _)| earlier == name) {
            let start = text::offset(text, *at).unwrap_or(0);
            problem(start, start + name.len(), "duplicate-route", format!("Route \"{name}\" is already defined in this file."));
        }
    }

    problems
}

/// Which groups of the text inspections in `checks.rs` are switched on.
#[derive(Clone, Copy)]
#[allow(dead_code)] // the groups added after this foundation are read by their checks
struct Groups {
    controllers: bool,
    services: bool,
    php_calls: bool,
    twig: bool,
    twig_syntax: bool,
    yaml_syntax: bool,
    commands: bool,
    deprecations: bool,
    doctrine: bool,
}

impl Groups {
    #[cfg(test)]
    fn all_on() -> Groups {
        Groups {
            controllers: true,
            services: true,
            php_calls: true,
            twig: true,
            twig_syntax: true,
            yaml_syntax: true,
            commands: true,
            deprecations: true,
            doctrine: true,
        }
    }

    fn from_settings() -> Groups {
        let on = |key: &str| host::setting_bool(key).unwrap_or(true);
        Groups {
            controllers: on("controllerChecks"),
            services: on("serviceChecks"),
            php_calls: on("phpCallChecks"),
            twig: on("twigChecks"),
            twig_syntax: on("twigSyntaxChecks"),
            yaml_syntax: on("yamlSyntaxChecks"),
            commands: on("commandChecks"),
            deprecations: on("deprecationChecks"),
            doctrine: on("doctrineChecks"),
        }
    }
}

/// Findings of the switched-on groups.
fn findings(index: &Index, groups: Groups, read: checks::Reader, path: &str, language: &str, text: &str) -> Vec<checks::Finding> {
    let ctx = checks::Context::new(index, read);
    let mut findings = Vec::new();

    if groups.controllers {
        findings.extend(checks::check_routes(&ctx, path, language, text));
    }

    if groups.services {
        findings.extend(checks::check_services(&ctx, path, language, text));
    }

    if groups.php_calls {
        findings.extend(checks::check_php(&ctx, language, text));
    }

    if groups.twig {
        findings.extend(checks::check_twig(&ctx, language, text));

        if language == "twig" {
            findings.extend(twig_checks::names(&ctx, text));
        }
    }

    if groups.twig_syntax && language == "twig" {
        findings.extend(twig_checks::syntax(text));
    }

    if groups.services {
        findings.extend(config_checks::deprecated_factory_settings(path, language, text));
    }

    if groups.yaml_syntax {
        findings.extend(config_checks::yaml_scalars(path, language, text));
    }

    findings
}

/// Findings of all groups that carry a replacement, for quick fixes.
fn fixes(index: &Index, path: &str, language: &str, text: &str) -> Vec<checks::Finding> {
    let project = project_for(path);
    let read = |file: &str| project.as_ref().and_then(|project| project.read_file(file).ok());

    findings(index, Groups::from_settings(), &read, path, language, text)
        .into_iter()
        .filter(|finding| finding.replacement.is_some())
        .collect()
}

/// Problems of the text inspections, converted to diagnostics.
fn extra_problems(index: &Index, groups: Groups, read: checks::Reader, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    findings(index, groups, read, path, language, text)
        .into_iter()
        .map(|finding| Diagnostic {
            range: range_of(text, finding.start, finding.end),
            severity: finding.severity,
            message: finding.message,
            code: Some(finding.code.into()),
            source: Some(SOURCE.into()),
            unnecessary: false,
            deprecated: false,
            related: Vec::new(),
        })
        .collect()
}

#[derive(Default)]
pub struct Main {
    /// By project root.
    indexes: BTreeMap<String, Index>,
    /// Files with published problems, checked again when the index changes.
    checked: BTreeSet<String>,
    /// Expanded nodes of the tool window.
    expanded: BTreeSet<String>,
}

impl Main {
    fn index_project(&mut self, project: &Project) {
        let Ok(root) = project.root() else { return };

        if !index::is_symfony(project) {
            self.indexes.remove(&root);
            self.update_status_item(&root);
            return;
        }

        let index = Index::build(project);
        if index.partial {
            host::log(LogLevel::Warn, &format!("index of {root} is partial: out of time"));
        }
        self.indexes.insert(root.clone(), index);
        self.update_status_item(&root);

        let checked: Vec<String> = self.checked.iter().filter(|path| under(path, &root)).cloned().collect();
        for path in checked {
            self.check(project, &path);
        }
        host::refresh_view(VIEW);
    }

    fn update_status_item(&self, root: &str) {
        let Some(index) = self.indexes.get(root) else {
            host::remove_status_item(STATUS_ITEM);
            return;
        };

        let text = match index.framework_version {
            Some((major, minor)) => format!("Symfony {major}.{minor}"),
            None => "Symfony".to_owned(),
        };
        let item = StatusItem { text, tooltip: Some(self.summary_of(root, index)), command: Some("reindex".into()) };
        host::set_status_item(STATUS_ITEM, &item);
    }

    fn index_all(&mut self) {
        for project in host::projects().unwrap_or_default() {
            self.index_project(&project);
        }
    }

    fn index_for(&self, path: &str) -> Option<&Index> {
        self.indexes.iter().filter(|(root, _)| under(path, root)).max_by_key(|(root, _)| root.len()).map(|(_, index)| index)
    }

    /// Publishes the problems of a file (none when they are switched off).
    fn check(&mut self, project: &Project, path: &str) {
        let Some(language) = language_of(path) else { return };
        let Some(index) = self.index_for(path) else { return };

        let enabled = host::setting_bool("diagnostics").unwrap_or(true);
        let found = match project.read_file(path) {
            Ok(text) if enabled && !index.partial => {
                let read = |file: &str| project.read_file(file).ok();
                let mut found = problems(index, path, language, &text);
                found.extend(extra_problems(index, Groups::from_settings(), &read, path, language, &text));
                found
            }
            _ => Vec::new(),
        };

        if host::publish_diagnostics(path, &found).is_ok() {
            self.checked.insert(path.to_owned());
        }
    }

    fn console(&mut self, ctx: &Invocation, command: &str) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let task = host::begin_task(&format!("bin/console {command}"), false);
        let args = ["bin/console".to_owned(), command.to_owned()];

        let output = match project.run_process("php", &args, None) {
            Err(Error::Denied(_)) => return Err("Allow the process:php capability in Settings › Plugins.".into()),
            result => result?,
        };

        if output.status != Some(0) {
            let error = if output.stderr.trim().is_empty() { &output.stdout } else { &output.stderr };
            let last: Vec<&str> = error.trim().lines().rev().take(5).collect();
            let tail = last.into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(format!("bin/console {command} failed:\n{tail}").into());
        }

        task.report(1, Some(2), Some("Indexing"));
        self.index_project(&project);
        host::notify(NotificationLevel::Info, SOURCE, &format!("Ran bin/console {command} and reindexed the project."));
        Ok(None)
    }

    fn summary(&self) -> String {
        if self.indexes.is_empty() {
            return "No Symfony project is open.".into();
        }

        let lines: Vec<String> = self.indexes.iter().map(|(root, i)| self.summary_of(root, i)).collect();
        lines.join("\n")
    }

    fn summary_of(&self, root: &str, i: &Index) -> String {
        let partial = if i.partial { " (index incomplete)" } else { "" };
        let translations: usize = i.translations.values().map(Names::len).sum();
        format!(
            "{root}: {} routes, {} templates, {} services, {translations} translations{partial}",
            i.routes.len(),
            i.templates.len(),
            i.services.len(),
        )
    }
}

impl Plugin for Main {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileSaved, EventKind::FileOpened] {
            if let Err(error) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("Symfony problems need the {} event: {error}", kind.name()));
            }
        }
        self.index_all();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "reindex" => {
                let _task = host::begin_task("Indexing Symfony projects", false);
                self.index_all();
                host::notify(NotificationLevel::Info, SOURCE, &self.summary());
                Ok(None)
            }
            "show-profiler" => {
                host::refresh_view(PROFILER_VIEW);
                host::notify(NotificationLevel::Info, SOURCE, "The latest requests are in the Symfony Profiler tool window.");
                Ok(None)
            }
            "cache-warmup" => self.console(ctx, "cache:warmup"),
            "cache-clear" => self.console(ctx, "cache:clear"),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        let mut saved: BTreeSet<&str> = BTreeSet::new();
        let mut opened: BTreeSet<&str> = BTreeSet::new();

        for event in events {
            match event {
                Event::FileSaved(path) => {
                    saved.insert(path);
                }
                Event::FileOpened(path) => {
                    opened.insert(path);
                }
                _ => {}
            }
        }

        for project in host::projects().unwrap_or_default() {
            let Ok(root) = project.root() else { continue };
            let relative = |path: &&str| path.strip_prefix(&format!("{root}/")).map(str::to_owned);

            let changed = saved.iter().filter_map(relative).any(|r| affects_index(&r));
            let unknown = !self.indexes.contains_key(&root) && opened.iter().any(|path| under(path, &root));
            if changed || unknown {
                self.index_project(&project);
            }

            for path in saved.iter().chain(&opened).filter(|path| under(path, &root)) {
                self.check(&project, path);
            }
        }
    }
}

impl Providers for Main {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };

        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else {
            return twig_names(index, doc, offset);
        };
        let replace = range_of(&doc.text, reference.start, reference.end);

        candidates(index, &doc.path, &doc.language, &doc.text, &reference)
            .iter()
            .map(|(name, entry)| {
                let mut item = CompletionItem::new(name, completion_kind(reference.kind));
                item.detail = entry.detail.clone();
                item.replace = Some(replace);
                item
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else { return Vec::new() };

        let name = &doc.text[reference.start..reference.end];
        let names = candidates(index, &doc.path, &doc.language, &doc.text, &reference);
        let Some(entry) = names.get(name) else { return Vec::new() };

        let is_file = matches!(reference.kind, Kind::Template | Kind::Class | Kind::Asset);
        let width = if is_file { 0 } else { name.encode_utf16().count() as u32 };
        location(entry, width).into_iter().collect()
    }

    /// Controller ↔ template, block → parent block, class ↔ service
    /// definition (the plugin's PHP, Twig, Service and YAML line markers).
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let mut markers = Vec::new();
        let mut marker = |line: u32, icon: Option<&str>, tooltip: String, target: Option<Location>| {
            markers.push(LineMarker { line, icon: icon.map(Into::into), tooltip, target });
        };

        match doc.language.as_str() {
            "php" => {
                for reference in refs::references("php", &doc.text).into_iter().filter(|r| r.kind == Kind::Template) {
                    let name = &doc.text[reference.start..reference.end];
                    if let Some(entry) = index.templates.get(name) {
                        let line = text::position(&doc.text, reference.start).line;
                        marker(line, None, format!("Template {name}"), location(entry, 0));
                    }
                }

                if let Some((class, offset)) = index::php_class(&doc.text) {
                    let service = index.services.iter().find(|(id, e)| **id == class || e.detail.as_deref() == Some(&class));
                    if let Some((id, entry)) = service {
                        let line = text::position(&doc.text, offset).line;
                        marker(line, None, format!("Service {id}"), location(entry, id.encode_utf16().count() as u32));
                    }
                }
            }
            "twig" => {
                if let Some(template) = index.template_of(&doc.path) {
                    let renders: Vec<&index::Render> = index.renders.iter().filter(|r| r.template == template).collect();
                    if let Some(first) = renders.first() {
                        let callers: Vec<&str> = renders.iter().map(|r| r.caller.as_str()).collect();
                        let target = Location { path: first.path.clone(), range: Range::new(first.at, first.at) };
                        marker(0, None, format!("Rendered by {}", callers.join(", ")), Some(target));
                    }
                }

                let parents = project_for(&doc.path).map(|p| parent_blocks(index, &p, &doc.text)).unwrap_or_default();
                for (block, offset) in refs::twig_blocks(&doc.text) {
                    let Some(entry) = parents.get(block) else { continue };
                    let from = entry.detail.clone().unwrap_or_default();
                    let line = text::position(&doc.text, offset).line;
                    let target = location(entry, block.len() as u32);
                    marker(line, Some("overriding"), format!("Overrides block \"{block}\" of {from}"), target);
                }
            }
            "yaml" => {
                let lines = |offset| text::position(&doc.text, offset).line;
                for key in index::yaml_keys(&doc.text) {
                    let class = match key.parents.as_slice() {
                        [services] if services == "services" => &key.key,
                        [services, _] if services == "services" && key.key == "class" => &key.value,
                        _ => continue,
                    };
                    if let Some(entry) = index.classes.get(class.trim_matches(['\'', '"'])) {
                        marker(lines(key.offset), None, format!("Class {class}"), location(entry, 0));
                    }
                }
            }
            _ => {}
        }

        markers
    }

    /// Quick fixes (quote YAML scalars, apply check replacements, add a missing translation key) and refactorings (`intentions.rs`).
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let mut actions = Vec::new();

        if doc.language == "yaml" && is_quoted_config(&doc.path) && Groups::from_settings().yaml_syntax {
            actions.extend(
                yaml_quotes::unquoted(&doc.text)
                    .into_iter()
                    .map(|range| (range_of(&doc.text, range.start, range.end), range))
                    .filter(|(range, _)| range.start.line >= selection.start.line && range.start.line <= selection.end.line)
                    .map(|(range, bytes)| {
                        let quoted = format!("'{}'", doc.text[bytes].replace('\'', "''"));
                        CodeAction {
                            title: format!("Quote {quoted}"),
                            kind: Some("quickfix".into()),
                            preferred: true,
                            edits: vec![(doc.path.clone(), vec![TextEdit { range, text: quoted }])],
                            data: None,
                        }
                    }),
            );
        }

        let Some(offset) = text::offset(&doc.text, selection.start) else { return actions };
        let selection_end = text::offset(&doc.text, selection.end).unwrap_or(offset);

        let intentions = match doc.language.as_str() {
            "php" => intentions::php_actions(&doc.text, offset, index.framework_version),
            "twig" if intentions::caret_on_extends(&doc.text, offset) => {
                let blocks: Vec<String> = project_for(&doc.path).map(|project| parent_blocks(index, &project, &doc.text).into_keys().collect()).unwrap_or_default();
                intentions::twig_actions(&doc.text, offset, &blocks)
            }
            _ => Vec::new(),
        };
        actions.extend(intentions.into_iter().map(|action| CodeAction {
            title: action.title,
            kind: Some(action.kind.into()),
            preferred: false,
            edits: vec![(doc.path.clone(), action.edits.into_iter().map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text }).collect())],
            data: None,
        }));

        for finding in fixes(index, &doc.path, &doc.language, &doc.text) {
            let Some(replacement) = finding.replacement else { continue };
            if finding.start > selection_end || finding.end < offset {
                continue;
            }

            actions.push(CodeAction {
                title: format!("Replace with {replacement}"),
                kind: Some("quickfix".into()),
                preferred: true,
                edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: replacement }])],
                data: None,
            });
        }

        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else { return actions };

        let key = &doc.text[reference.start..reference.end];
        let domain = domain_of(&doc.language, &doc.text, &reference);
        let missing = index.translations.get(&domain).is_some_and(|keys| !keys.contains_key(key));
        if reference.kind != Kind::Translation || is_dynamic(key) || !missing {
            return actions;
        }

        let files = index.translation_files.get(&domain).map(Vec::as_slice).unwrap_or_default();
        actions.extend(files.iter().map(|file| CodeAction {
            title: format!("Add translation key \"{key}\" to {}", file_name(file)),
            kind: Some("quickfix".into()),
            preferred: false,
            edits: Vec::new(),
            data: Some(format!("{file}\n{key}")),
        }));
        actions
    }

    fn resolve_code_action(&mut self, _doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let data = action.data.clone().ok_or("Nothing to resolve.")?;
        let (file, key) = data.split_once('\n').ok_or("Malformed code action.")?;
        let project = project_for(file).ok_or("The file is not in an open project.")?;
        let text = project.read_file(file)?;

        let end = text::position(&text, text.len());
        let separator = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        let quoted = format!("'{}'", key.replace('\'', "''"));
        let edit = TextEdit { range: Range::new(end, end), text: format!("{separator}{quoted}: {quoted}\n") };

        action.edits = vec![(file.to_owned(), vec![edit])];
        Ok(action)
    }
}

/// Tool window node keys: `group:<kind>`, `domain:<name>`, `<kind>:<name>`, `translation:<domain>:<key>`.
/// The icon is the group's and, except for translations, its leaves'.
const GROUPS: &[(&str, &str, &str)] = &[
    ("route", "Routes", "web"),
    ("template", "Templates", "file-html"),
    ("service", "Services", "class"),
    ("translation", "Translations", "text"),
];

fn group_names<'a>(index: &'a Index, group: &str) -> Option<&'a Names> {
    match group {
        "route" => Some(&index.routes),
        "template" => Some(&index.templates),
        "service" => Some(&index.services),
        _ => None,
    }
}

fn entry_for<'a>(index: &'a Index, key: &str) -> Option<&'a Entry> {
    let (group, name) = key.split_once(':')?;
    if group == "translation" {
        let (domain, name) = name.split_once(':')?;
        return index.translations.get(domain)?.get(name);
    }
    group_names(index, group)?.get(name)
}

impl Main {
    fn tree(&self, index: &Index) -> Vec<TreeItem> {
        let mut items = Vec::new();
        let leaf = |key: String, name: &str, entry: &Entry, parent: &str, icon: &str| {
            let item = TreeItem::new(&key, name).parent(parent).icon(icon);
            match &entry.detail {
                Some(detail) if !detail.is_empty() => item.detail(detail),
                _ => item,
            }
        };

        for (group, label, icon) in GROUPS {
            let key = format!("group:{group}");
            let count = match group_names(index, group) {
                Some(names) => names.len(),
                None => index.translations.values().map(Names::len).sum(),
            };
            let expanded = self.expanded.contains(&key);
            let item = TreeItem::new(&key, &format!("{label} ({count})")).icon(icon);
            // An expandable item without children shows as loading forever.
            if count == 0 {
                items.push(item);
                continue;
            }

            items.push(item.expandable(expanded));

            if !expanded {
                continue;
            }

            if let Some(names) = group_names(index, group) {
                items.extend(names.iter().map(|(name, entry)| leaf(format!("{group}:{name}"), name, entry, &key, icon)));
                continue;
            }

            for (domain, names) in &index.translations {
                let domain_key = format!("domain:{domain}");
                let domain_expanded = self.expanded.contains(&domain_key);
                let domain_item = TreeItem::new(&domain_key, &format!("{domain} ({})", names.len())).parent(&key).icon("folder");
                items.push(domain_item.expandable(domain_expanded));

                if domain_expanded {
                    items.extend(
                        names.iter().map(|(name, entry)| leaf(format!("translation:{domain}:{name}"), name, entry, &domain_key, "property")),
                    );
                }
            }
        }

        items
    }

    fn view_index(&self, ctx: &Invocation) -> Option<&Index> {
        let root = ctx.project()?.root().ok()?;
        self.indexes.get(&root)
    }
}

/// The profiler's recent requests; the file is read when the view is shown
/// (a missing one is `NotFound` in the mock and `Invalid` in the host).
fn profiler_view(ctx: &Invocation) -> View {
    let refresh = Button::new("refresh-profiler", "Refresh").icon("refresh");
    let Some(project) = ctx.project() else {
        return View::message("No project is open.").button(refresh);
    };
    let Ok(text) = project.read_file(profiler::INDEX_FILE) else {
        return View::message(&format!("No {} yet: open a page of the app in dev mode with the web profiler enabled.", profiler::INDEX_FILE))
            .button(refresh);
    };

    let requests = profiler::rows(&text);
    let rows: Vec<Vec<String>> = requests
        .iter()
        .map(|r| vec![profiler::timestamp(r.time), r.method.clone(), r.status.clone(), r.url.clone(), r.ip.clone(), r.token.clone()])
        .collect();
    let table = Table {
        columns: ["Time (UTC)", "Method", "Status", "URL", "IP", "Token"].map(String::from).to_vec(),
        total_rows: rows.len() as u32,
        first_row: 0,
        rows,
        editable: false,
    };

    View::new(Body::Table(table)).button(refresh).status(&format!("{} latest requests", requests.len()))
}

impl Views for Main {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if view == PROFILER_VIEW {
            return Ok(profiler_view(ctx));
        }

        let reindex = Button::new("reindex", "Reindex").icon("refresh");
        let Some(index) = self.view_index(ctx) else {
            return Ok(View::message("This project is not a Symfony application.").button(reindex));
        };

        let mut status = if index.compiled_container {
            String::from("Compiled container found")
        } else {
            String::from("No compiled container: run Warm Up Cache to index bundle services and routes")
        };
        if index.partial {
            status.push_str(" · Index incomplete (time limit reached)");
        }

        Ok(View::new(Body::Tree(self.tree(index)))
            .button(reindex)
            .button(Button::new("cache-warmup", "Warm Up Cache").icon("build-project"))
            .status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Expand(key) => {
                self.expanded.insert(key);
                host::refresh_view(view);
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
                host::refresh_view(view);
            }
            ViewEvent::Activate(key) => {
                let entry = self.view_index(ctx).and_then(|index| entry_for(index, &key));
                if let Some(entry) = entry.filter(|e| !e.path.is_empty()) {
                    host::open_location(&entry.path, entry.at.line, entry.at.character)?;
                }
            }
            ViewEvent::Button(id) if id == "refresh-profiler" => host::refresh_view(PROFILER_VIEW),
            ViewEvent::Button(id) => {
                self.run_command(&id, ctx, None)?;
                host::refresh_view(view);
            }
            _ => {}
        }
        Ok(())
    }
}

register_plugin!(Main, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn symfony_project(host: &MockHost) {
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        host.add_file("templates/base.html.twig", "<html>{% block title %}{% endblock %}{% block body %}{% endblock %}</html>");
        host.add_file("templates/blog/index.html.twig", "{% extends 'base.html.twig' %}\n{% block body %}\n{{ posts }}\n{% endblock %}\n");
        host.add_file(
            "src/Controller/BlogController.php",
            "<?php\nnamespace App\\Controller;\nclass BlogController\n{\n    #[Route('/blog', name: 'blog_index')]\n    public function index()\n    {\n        return $this->render('blog/index.html.twig', ['posts' => [], 'title' => 'x']);\n    }\n}\n",
        );
        host.add_file("src/Mailer.php", "<?php\nnamespace App;\n\nfinal class Mailer {}\n");
        host.add_file("config/services.yaml", "services:\n    app.mailer:\n        class: App\\Mailer\n");
        host.add_file("config/packages/security.yaml", "security:\n    role_hierarchy:\n        ROLE_ADMIN: ROLE_USER\n");
        host.add_file("translations/messages.en.yaml", "app:\n    title: Blog\n");
        host.add_file("translations/admin.en.yaml", "menu: Menu\n");
        host.add_file("public/img/logo.png", "");
    }

    fn activated(host: &MockHost) -> Main {
        symfony_project(host);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|item| item.label).collect()
    }

    #[test]
    fn code_actions_combine_quote_fix_and_intentions() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let yaml = doc("/project/config/services.yaml", "yaml", "services:\n    app.mailer:\n        arguments: [@foo]\n        class: App\\Mailer\n        factory: @bar\n");
        let titles: Vec<String> = plugin.code_actions(&yaml, Range::new(Position::new(4, 17), Position::new(4, 17))).into_iter().map(|action| action.title).collect();
        assert!(titles.iter().any(|title| title.starts_with("Quote ")), "{titles:?}");

        let php = doc("/project/src/Controller/PostController.php", "php", "<?php\nnamespace App\\Controller;\n\nclass PostController extends AbstractController\n{\n    public function show()\n    {\n    }\n}\n");
        let actions = plugin.code_actions(&php, Range::new(Position::new(5, 22), Position::new(5, 22)));
        let route = actions.iter().find(|action| action.title == "Add #[Route] attribute").expect("route action");
        assert_eq!(route.kind.as_deref(), Some("refactor.rewrite"));
        assert_eq!(route.edits[0].1.len(), 2);

        let twig = doc("/project/templates/blog/new.html.twig", "twig", "{% extends 'base.html.twig' %}\n");
        let actions = plugin.code_actions(&twig, Range::new(Position::new(0, 5), Position::new(0, 5)));
        let titles: Vec<String> = actions.into_iter().map(|action| action.title).collect();
        assert_eq!(titles, ["Override block \"body\"", "Override block \"title\""]);
    }

    #[test]
    fn denied_events_log_a_warning_and_still_activate() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        symfony_project(&host);
        let mut plugin = Main::default();
        assert!(plugin.activate(&host.context("aritti.symfony")).is_ok());
        assert!(host.logs().iter().any(|(level, _)| *level == LogLevel::Warn));
    }

    #[test]
    fn settings_groups_default_on_and_switch_off_independently() {
        let host = MockHost::install();
        let keys = ["controllerChecks", "serviceChecks", "phpCallChecks", "twigChecks", "twigSyntaxChecks", "yamlSyntaxChecks", "commandChecks", "deprecationChecks", "doctrineChecks"];
        let states = |groups: Groups| {
            [groups.controllers, groups.services, groups.php_calls, groups.twig, groups.twig_syntax, groups.yaml_syntax, groups.commands, groups.deprecations, groups.doctrine]
        };
        assert_eq!(states(Groups::from_settings()), [true; 9]);

        for (position, key) in keys.iter().enumerate() {
            host.set_setting(key, "false");
            let mut expected = [true; 9];
            expected[position] = false;
            assert_eq!(states(Groups::from_settings()), expected);
            host.set_setting(key, "true");
        }
    }

    #[test]
    fn completes_and_navigates_from_the_index() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let php = doc("/project/src/Controller/HomeController.php", "php", "<?php\n$this->render('');\n$this->isGranted('');\n");
        assert_eq!(labels(plugin.completion(&php, Position::new(1, 15), None)), ["base.html.twig", "blog/index.html.twig"]);
        assert!(labels(plugin.completion(&php, Position::new(2, 18), None)).contains(&"ROLE_ADMIN".to_owned()));

        let twig = doc(
            "/project/templates/blog/show.html.twig",
            "twig",
            "<a href=\"{{ path('blog_index') }}\">{{ 'app.title'|trans }}{{ 'menu'|trans({}, 'admin') }}</a>",
        );
        let route = plugin.definition(&twig, Position::new(0, 20));
        assert_eq!(route[0].path, "/project/src/Controller/BlogController.php");
        assert_eq!(route[0].range, Range::new(Position::new(4, 28), Position::new(4, 38)));

        let translation = plugin.definition(&twig, Position::new(0, 40));
        assert_eq!(translation[0].path, "/project/translations/messages.en.yaml");
        assert_eq!(plugin.definition(&twig, Position::new(0, 64))[0].path, "/project/translations/admin.en.yaml");

        let yaml = doc(
            "/project/config/services.yaml",
            "yaml",
            "services:\n    App\\Newsletter:\n        arguments: ['@app.mailer']\n    App\\Mailer: ~\n",
        );
        assert_eq!(plugin.definition(&yaml, Position::new(2, 25))[0].range.start, Position::new(1, 4));
        assert_eq!(plugin.definition(&yaml, Position::new(3, 8))[0].path, "/project/src/Mailer.php");

        let asset = doc("/project/templates/a.html.twig", "twig", "{{ asset('img/') }}");
        assert_eq!(labels(plugin.completion(&asset, Position::new(0, 14), None)), ["img/logo.png"]);
    }

    #[test]
    fn completes_twig_variables_and_parent_blocks() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let text = "{% extends 'base.html.twig' %}\n{% block  %}\n{{ p }}\n";
        let twig = doc("/project/templates/blog/index.html.twig", "twig", text);

        assert_eq!(labels(plugin.completion(&twig, Position::new(1, 9), None)), ["body", "title"]);
        let variables = labels(plugin.completion(&twig, Position::new(2, 4), None));
        assert_eq!(variables.iter().filter(|v| ["posts", "title"].contains(&v.as_str())).count(), 2);
    }

    #[test]
    fn completes_tags_filters_functions_tests_from_static_list() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let complete = |plugin: &mut Main, text: &str, character: u32| {
            let twig = doc("/project/templates/a.html.twig", "twig", text);
            labels(plugin.completion(&twig, Position::new(0, character), None))
        };

        let tags = complete(&mut plugin, "{% ", 3);
        assert!(["if", "for", "block", "extends", "endblock", "endif", "verbatim", "types"].iter().all(|t| tags.contains(&(*t).to_owned())));

        let filters = complete(&mut plugin, "{{ name|", 8);
        assert!(filters.contains(&"upper".to_owned()) && !filters.contains(&"if".to_owned()));

        let functions = complete(&mut plugin, "{{ ", 3);
        assert!(functions.contains(&"range".to_owned()) && !functions.contains(&"upper".to_owned()));

        let tests = complete(&mut plugin, "{% if x is not ", 15);
        assert!(tests.contains(&"empty".to_owned()) && !tests.contains(&"range".to_owned()));

        assert!(complete(&mut plugin, "{{ 'ra' }}", 5).is_empty());
        assert!(complete(&mut plugin, "plain ra", 8).is_empty());
    }

    #[test]
    fn extends_and_include_paths_complete_from_templates_dir() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let names = ["base.html.twig", "blog/index.html.twig"];

        for tag in ["extends", "include", "embed", "import", "from"] {
            let text = format!("{{% {tag} '' %}}");
            let twig = doc("/project/templates/a.html.twig", "twig", &text);
            let at = Position::new(0, (tag.len() + 5) as u32);
            assert_eq!(labels(plugin.completion(&twig, at, None)), names, "{tag}");
        }
    }

    #[test]
    fn unclosed_block_tag_is_reported() {
        let host = MockHost::install();
        let plugin = activated(&host);
        let index = plugin.index_for("/project/x").unwrap();
        let found = |text: &str| -> Vec<(String, u32)> {
            problems(index, "/project/templates/a.html.twig", "twig", text)
                .into_iter()
                .filter(|d| d.code.as_deref() == Some("unclosed-tag"))
                .map(|d| (d.message, d.range.start.character))
                .collect()
        };

        let reported = found("{% block a %}\n{% if x %}{% endif %}");
        assert_eq!(reported, [("Tag \"block\" is never closed: missing {% endblock %}.".to_owned(), 3)]);

        for name in ["for x in y", "macro m()", "embed 'a.html.twig'", "apply upper", "autoescape", "with {}", "cache 'k'", "sandbox"] {
            assert_eq!(found(&format!("{{% {name} %}}")).len(), 1, "{name}");
        }

        assert_eq!(found("{% if a %}{% for x in y %}{% endif %}").len(), 1);
        assert!(found("{% block a %}{% if x %}{% else %}{% endif %}{% endblock %}{% block t 'x' %}{% set v = 1 %}").is_empty());
        assert!(found("{# {% block a %} #}{% verbatim %}{% if %}{% endverbatim %}").is_empty());
        assert!(found("{%- block a -%}{%- endblock -%}").is_empty());
    }

    #[test]
    fn reports_missing_names_and_duplicate_routes() {
        let host = MockHost::install();
        let plugin = activated(&host);
        let index = plugin.index_for("/project/x").unwrap();

        let twig = "{{ include('nope.html.twig') }}{{ 'app.title'|trans }}{{ 'app.gone'|trans }}{{ path('unknown') }}";
        let found: Vec<_> = problems(index, "/project/templates/a.html.twig", "twig", twig)
            .into_iter()
            .map(|d| (d.code.unwrap(), d.range.start.character))
            .collect();
        // No compiled routes: unknown routes may come from bundles, so they are not reported.
        assert_eq!(found, [("missing-template".into(), 12), ("missing-translation".into(), 58)]);

        let php = "<?php\nclass A {\n    #[Route('/a', name: 'same')]\n    public function a() {}\n    #[Route('/b', name: 'same')]\n    public function b() {}\n}\n";
        let found = problems(index, "/project/src/Controller/A.php", "php", php);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start, Position::new(4, 25));
    }

    #[test]
    fn publishes_problems_of_opened_files() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/broken.html.twig", "{% extends 'missing.html.twig' %}");
        plugin.handle_events(&[Event::FileOpened("/project/templates/broken.html.twig".into())], &host.invocation());
        assert_eq!(host.diagnostics()["/project/templates/broken.html.twig"].len(), 1);
    }

    #[test]
    fn marks_controllers_templates_blocks_and_services() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let template = host_text(&host, "templates/blog/index.html.twig");
        let markers = plugin.line_markers(&doc("/project/templates/blog/index.html.twig", "twig", &template));
        let tooltips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(tooltips, [(0, "Rendered by BlogController::index"), (1, "Overrides block \"body\" of base.html.twig")]);

        let controller = host_text(&host, "src/Controller/BlogController.php");
        let markers = plugin.line_markers(&doc("/project/src/Controller/BlogController.php", "php", &controller));
        assert_eq!(markers[0].line, 7);
        assert_eq!(markers[0].target.as_ref().unwrap().path, "/project/templates/blog/index.html.twig");

        let mailer = host_text(&host, "src/Mailer.php");
        let markers = plugin.line_markers(&doc("/project/src/Mailer.php", "php", &mailer));
        assert_eq!(markers[0].tooltip, "Service app.mailer");

        let services = host_text(&host, "config/services.yaml");
        let markers = plugin.line_markers(&doc("/project/config/services.yaml", "yaml", &services));
        assert_eq!(markers[0].target.as_ref().unwrap().path, "/project/src/Mailer.php");
    }

    fn host_text(host: &MockHost, path: &str) -> String {
        host.invocation().project().unwrap().read_file(path).unwrap()
    }

    #[test]
    fn adds_missing_translation_keys() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let twig = doc("/project/templates/a.html.twig", "twig", "{{ 'app.new'|trans }}");

        let actions = plugin.code_actions(&twig, Range::new(Position::new(0, 5), Position::new(0, 5)));
        assert_eq!(actions[0].title, "Add translation key \"app.new\" to messages.en.yaml");

        let resolved = plugin.resolve_code_action(&twig, actions[0].clone()).unwrap();
        let (path, edits) = &resolved.edits[0];
        assert_eq!(path, "/project/translations/messages.en.yaml");
        assert_eq!(edits[0].text, "'app.new': 'app.new'\n");
        assert_eq!(edits[0].range.start, Position::new(2, 0));
    }

    #[test]
    fn shows_and_opens_names_in_the_tool_window() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        plugin.handle(VIEW, ViewEvent::Expand("group:route".into()), &ctx).unwrap();
        let View { body: Body::Tree(items), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a tree") };
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Routes (1)", "blog_index", "Templates (2)", "Services (1)", "Translations (2)"]);
        let icons: Vec<_> = items.iter().map(|i| i.icon.as_deref().unwrap_or_default()).collect();
        assert_eq!(icons, ["web", "web", "file-html", "class", "text"]);

        plugin.handle(VIEW, ViewEvent::Expand("group:translation".into()), &ctx).unwrap();
        plugin.handle(VIEW, ViewEvent::Expand("domain:admin".into()), &ctx).unwrap();
        let View { body: Body::Tree(items), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a tree") };
        let icon = |key: &str| items.iter().find(|i| i.key == key).and_then(|i| i.icon.clone());
        assert_eq!(icon("domain:admin").as_deref(), Some("folder"));
        assert_eq!(icon("translation:admin:menu").as_deref(), Some("property"));

        plugin.handle(VIEW, ViewEvent::Activate("route:blog_index".into()), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/Controller/BlogController.php".into(), 4, 28)]);
    }

    #[test]
    fn runs_the_console_then_reindexes() {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
        let mut plugin = activated(&host);
        host.on_process("php", |args, _| {
            assert_eq!(args, ["bin/console", "cache:warmup"]);
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
        });
        assert_eq!(plugin.run_command("cache-warmup", &host.invocation(), None).unwrap(), None);
        let notes: Vec<_> = host.notifications().into_iter().map(|n| (n.title, n.body)).collect();
        assert_eq!(notes, [("Symfony".into(), "Ran bin/console cache:warmup and reindexed the project.".into())]);
    }

    #[test]
    fn names_the_failed_console_command() {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php"]);
        let mut plugin = activated(&host);
        host.on_process("php", |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(1), stdout: String::new(), stderr: "boom\n".into() })
        });
        let error = plugin.run_command("cache-clear", &host.invocation(), None).unwrap_err();
        assert_eq!(error.to_string(), "bin/console cache:clear failed:\nboom");
    }

    #[test]
    fn ignores_projects_without_symfony() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        assert_eq!(plugin.run_command("reindex", &host.invocation(), None).unwrap(), None);
        assert_eq!(host.notifications()[0].body, "No Symfony project is open.");
    }

    #[test]
    fn twig_paths_namespace_resolves() {
        let host = MockHost::install();
        host.add_file(
            "config/packages/twig.yaml",
            "twig:\n    paths:\n        'templates/emails': 'emails'\n        '%kernel.project_dir%/templates/pdf': pdf\n        templates/plain:\n",
        );
        host.add_file("templates/emails/welcome.html.twig", "hi");
        host.add_file("templates/pdf/invoice.html.twig", "pdf");
        host.add_file("templates/plain/p.html.twig", "plain");
        let mut plugin = activated(&host);

        let twig = doc("/project/templates/a.html.twig", "twig", "{% include '@emails/welcome.html.twig' %}");
        let found = plugin.definition(&twig, Position::new(0, 16));
        assert_eq!(found[0].path, "/project/templates/emails/welcome.html.twig");

        let typing = doc("/project/templates/a.html.twig", "twig", "{% include '@' %}");
        let offered = labels(plugin.completion(&typing, Position::new(0, 13), None));
        assert!(offered.contains(&"@emails/welcome.html.twig".to_owned()));
        assert!(offered.contains(&"@pdf/invoice.html.twig".to_owned()));
        assert!(offered.contains(&"p.html.twig".to_owned()));
        assert!(offered.contains(&"base.html.twig".to_owned()));
    }

    #[test]
    fn profiler_view_lists_requests_latest_first() {
        let host = MockHost::install();
        host.add_file(profiler::INDEX_FILE, "a,1.1.1.1,GET,/old,1700000000,,200\nb,1.1.1.1,POST,/new,1700000500,,500\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        let View { body: Body::Table(table), .. } = plugin.render(PROFILER_VIEW, None, &ctx).unwrap() else { panic!("not a table") };
        assert_eq!(table.rows[0], ["2023-11-14 22:21:40", "POST", "500", "/new", "1.1.1.1", "b"]);
        assert_eq!(table.rows.len(), 2);

        plugin.run_command("show-profiler", &ctx, None).unwrap();
        assert!(host.refreshed_views().contains(&PROFILER_VIEW.to_owned()));
    }

    #[test]
    fn profiler_view_without_index_explains() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let View { body, .. } = plugin.render(PROFILER_VIEW, None, &host.invocation()).unwrap();
        assert!(matches!(body, Body::Message(_)));
    }

    #[test]
    fn duplicate_service_key_warns() {
        let host = MockHost::install();
        let plugin = activated(&host);
        let index = plugin.index_for("/project/x").unwrap();
        let found = |text: &str| -> Vec<(String, Position)> {
            problems(index, "/project/config/services.xml", "xml", text)
                .into_iter()
                .filter(|d| d.code.as_deref() == Some("duplicate-service"))
                .map(|d| (d.message, d.range.start))
                .collect()
        };
        let wrap = |body: &str| format!("<container>\n  <services>\n{body}  </services>\n</container>\n");

        let twice = wrap("    <service id=\"app.a\" class=\"A\"/>\n    <service id=\"app.b\"/>\n    <service id=\"app.a\" class=\"B\"/>\n");
        assert_eq!(found(&twice), [("Service \"app.a\" is already defined in this file.".to_owned(), Position::new(4, 17))]);

        let distinct = wrap("    <service id=\"app.a\"/>\n    <service id=\"app.b\"/>\n");
        assert!(found(&distinct).is_empty());

        let defaults = wrap("    <defaults autowire=\"true\"/>\n    <prototype namespace=\"App\\\" resource=\"../src\"/>\n    <service id=\"app.a\"><argument type=\"service\" id=\"app.a\"/></service>\n");
        assert!(found(&defaults).is_empty());
    }

    #[test]
    fn quotes_unquoted_yaml_scalar_with_quick_fix() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let text = "services:\n    app.a:\n        arguments: [@logger, '@ok']\n";
        let yaml = doc("/project/config/services.yaml", "yaml", text);

        let index = plugin.index_for("/project/x").unwrap();
        let read = |_: &str| None;
        let found = extra_problems(index, Groups::all_on(), &read, &yaml.path, "yaml", text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("unquoted-yaml-scalar"));
        assert_eq!(found[0].range, Range::new(Position::new(2, 20), Position::new(2, 27)));

        let at = Range::new(Position::new(2, 22), Position::new(2, 22));
        let actions = plugin.code_actions(&yaml, at);
        assert_eq!(actions[0].title, "Quote '@logger'");
        assert_eq!(actions[0].edits[0].1[0].text, "'@logger'");
        assert_eq!(actions[0].edits[0].1[0].range, found[0].range);

        let other = doc("/project/docker/compose.yaml", "yaml", "a: @x\n");
        assert!(plugin.code_actions(&other, at).is_empty());
        assert!(extra_problems(plugin.index_for("/project/x").unwrap(), Groups::all_on(), &read, &other.path, "yaml", &other.text).is_empty());
    }

    #[test]
    fn quick_fixes_honour_the_group_settings() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let text = "a: @x\nb: \"App\\Foo\"\n";
        let yaml = doc("/project/config/services.yaml", "yaml", text);
        let all = Range::new(Position::new(0, 0), Position::new(2, 0));

        let titles: Vec<String> = plugin.code_actions(&yaml, all).into_iter().map(|a| a.title).collect();
        assert_eq!(titles, ["Quote '@x'", "Replace with \\\\"]);

        host.set_setting("yamlSyntaxChecks", "false");
        assert!(plugin.code_actions(&yaml, all).is_empty());
    }

    #[test]
    fn every_template_is_registered_and_exists() {
        let manifest = include_str!("../plugin.toml");
        let value = |line: &str, key: &str| line.strip_prefix(key).and_then(|rest| rest.trim_start().strip_prefix("= \"")).map(|rest| rest.trim_end_matches('"').to_owned());
        let (mut ids, mut names, mut paths) = (Vec::new(), Vec::new(), Vec::new());
        let mut in_template = false;
        for line in manifest.lines() {
            if line.starts_with('[') {
                in_template = line == "[[contributes.fileTemplates]]";
            }
            if !in_template {
                continue;
            }
            ids.extend(value(line, "id"));
            names.extend(value(line, "name"));
            paths.extend(value(line, "path"));
        }

        assert_eq!(ids.len(), 12);
        assert_eq!((names.len(), paths.len()), (12, 12));
        for list in [&ids, &names, &paths] {
            let unique: BTreeSet<_> = list.iter().collect();
            assert_eq!(unique.len(), list.len(), "{list:?}");
        }

        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/");
        for path in &paths {
            let text = std::fs::read_to_string(format!("{root}{path}")).unwrap_or_else(|_| panic!("{path} is missing"));
            if !path.contains("Services") {
                assert!(text.contains("namespace App\\"), "{path}");
                assert!(text.contains("$NAME$"), "{path}");
            }

            let mut rest = text.as_str();
            while let Some(at) = rest.find('$') {
                rest = &rest[at + 1..];
                let Some(close) = rest.find('$') else { break };
                let word = &rest[..close];
                if !word.is_empty() && word.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                    assert!(matches!(word, "NAME" | "END"), "{path}: undefined placeholder ${word}$");
                    rest = &rest[close + 1..];
                }
            }
        }
    }

    #[test]
    fn service_templates_use_the_documented_defaults() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/");
        for file in ["ServicesYaml.yaml", "ServicesXml.xml", "ServicesPhp.php"] {
            let text = std::fs::read_to_string(format!("{root}{file}")).unwrap();
            assert!(text.contains("autowire"), "{file}");
            assert!(text.contains("autoconfigure"), "{file}");
            assert!(text.contains("App\\"), "{file}");
            assert!(text.contains("$END$"), "{file}");
        }

        let yaml = std::fs::read_to_string(format!("{root}ServicesYaml.yaml")).unwrap();
        let keys = index::yaml_keys(&yaml);
        assert!(keys.iter().any(|key| key.key == "services" && key.parents.is_empty()));
        assert!(keys.iter().any(|key| key.key == "_defaults"));
    }

    #[test]
    fn file_icon_files_exist_and_use_the_theme_colour() {
        let manifest = include_str!("../plugin.toml");
        assert!(manifest.contains("[[contributes.fileIcons]]\nid = \"symfony\""));
        assert!(manifest.contains("icons/symfony.svg") && manifest.contains("icons/symfony_dark.svg"));
        for name in ["services.yaml", "services.yml", "routes.yaml", "routes.yml", "bundles.php", "symfony.lock"] {
            assert!(manifest.contains(&format!("\"{name}\"")), "{name}");
        }

        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/icons/");
        let light = std::fs::read_to_string(format!("{root}symfony.svg")).unwrap();
        assert!(light.contains("#6C707E"));
        assert!(std::fs::read_to_string(format!("{root}symfony_dark.svg")).is_ok());
    }

    #[test]
    fn status_item_shows_framework_version() {
        let host = MockHost::install();
        symfony_project(&host);
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.1"}}"#);
        let _plugin = {
            let mut plugin = Main::default();
            plugin.activate(&host.context("aritti.symfony")).unwrap();
            plugin
        };

        let item = &host.status_items()["symfony"];
        assert_eq!(item.text, "Symfony 7.1");
        assert_eq!(item.command.as_deref(), Some("reindex"));
        assert!(item.tooltip.as_deref().unwrap().contains("routes"));
    }

    #[test]
    fn status_item_is_removed_for_non_symfony_projects() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        assert!(host.status_items().contains_key("symfony"));

        host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
        plugin.run_command("reindex", &host.invocation(), None).unwrap();
        assert!(!host.status_items().contains_key("symfony"));
    }
}
