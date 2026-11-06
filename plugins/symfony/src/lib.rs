//! Symfony: completion, navigation, problems, gutter markers and a tool
//! window for routes, Twig templates, services, translations, classes,
//! assets, entries and roles in PHP, Twig, YAML and XML files.
//!
//! Partial port of the Symfony Plugin for IntelliJ (Daniel Espendiller, Adrien
//! Brault; MIT), see README.md for what was and wasn't ported.

mod attr_args;
mod checks;
mod config_checks;
mod doctrine;
mod generators;
mod index;
mod indexer;
mod intentions;
mod php_checks;
mod php_text;
mod profiler;
mod refs;
mod rewrites;
mod service_gen;
mod suggest;
mod twig;
mod twig_checks;
mod wiring;
mod yaml_quotes;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::views::{Body, Button, Table, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Diagnostic, Event, EventKind, Location, PickItem, Position, Range, Severity, TextEdit};
use openidea_plugin::{Error, Invocation, LogLevel, NotificationLevel, Plugin, Project, StatusItem, host, text};

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

/// The tags of Symfony's "Built-in Symfony Service Tags" page.
const CORE_TAGS: &[&str] = &[
    "asset_mapper.compiler",
    "assets.package",
    "auto_alias",
    "cache.pool",
    "cache.pool.clearer",
    "cache.taggable",
    "chatter.transport_factory",
    "config_cache.resource_checker",
    "console.command",
    "container.do_not_inline",
    "container.env_var_loader",
    "container.env_var_processor",
    "container.excluded",
    "container.hot_path",
    "container.no_preload",
    "container.preload",
    "container.service_locator",
    "container.service_subscriber",
    "container.stack",
    "controller.argument_value_resolver",
    "controller.service_arguments",
    "data_collector",
    "doctrine.event_listener",
    "doctrine.event_subscriber",
    "doctrine.orm.entity_listener",
    "doctrine.repository_service",
    "form.type",
    "form.type_extension",
    "form.type_guesser",
    "http_client.client",
    "kernel.cache_clearer",
    "kernel.cache_warmer",
    "kernel.event_listener",
    "kernel.event_subscriber",
    "kernel.fragment_renderer",
    "kernel.locale_aware",
    "kernel.reset",
    "mailer.transport_factory",
    "messenger.message_handler",
    "messenger.receiver",
    "messenger.transport_factory",
    "mime.mime_type_guesser",
    "monolog.logger",
    "monolog.processor",
    "notifier.channel",
    "property_info.access_extractor",
    "property_info.constructor_extractor",
    "property_info.initializable_extractor",
    "property_info.list_extractor",
    "property_info.type_extractor",
    "proxy",
    "routing.expression_language_provider",
    "routing.loader",
    "routing.route_loader",
    "scheduler.schedule_provider",
    "security.expression_language_provider",
    "security.remember_me_handler",
    "security.voter",
    "serializer.encoder",
    "serializer.normalizer",
    "texter.transport_factory",
    "translation.dumper",
    "translation.extractor",
    "translation.extractor.visitor",
    "translation.loader",
    "twig.extension",
    "twig.loader",
    "twig.runtime",
    "validator.auto_mapper",
    "validator.constraint_validator",
    "validator.initializer",
    "workflow.definition",
];

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
        Kind::Tag => {
            let mut all: Names = CORE_TAGS
                .iter()
                .map(|tag| ((*tag).to_owned(), Entry { detail: Some("Symfony core tag".into()), path: String::new(), at: Position::new(0, 0) }))
                .collect();
            for (tag, entry) in &index.tags {
                all.entry(tag.clone()).or_insert_with(|| entry.clone());
            }
            Cow::Owned(all)
        }
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
        Kind::Tag => CompletionKind::Keyword,
    }
}

/// Twig built-ins that are variables rather than functions.
const TWIG_BUILT_IN_VARIABLES: [(&str, &str); 4] = [("app", "Symfony global"), ("_self", "Twig"), ("_context", "Twig"), ("_charset", "Twig")];

/// Variables at the caret: the template's own (`set`, `for`, macro arguments, `@var`),
/// those a controller renders it with or another template includes it with,
/// `twig.globals` and Twig's built-ins.
fn twig_variables(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    let Some((start, end)) = refs::twig_variable_at(&doc.text, offset) else { return Vec::new() };
    let template = index.template_of(&doc.path);
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    let mut add = |name: &str, detail: String| {
        if seen.insert(name.to_owned()) {
            let mut item = CompletionItem::new(name, CompletionKind::Variable);
            item.detail = Some(detail);
            item.replace = Some(range_of(&doc.text, start, end));
            items.push(item);
        }
    };

    for local in twig::local_variables(&doc.text, offset) {
        add(&local.name, local.detail.to_owned());
    }

    for include in index.includes.iter().filter(|include| Some(include.target.as_str()) == template) {
        for variable in &include.variables {
            add(variable, format!("included from {}", include.caller));
        }
    }

    for render in index.renders.iter().filter(|render| Some(render.template.as_str()) == template) {
        for variable in &render.variables {
            add(variable, render.caller.clone());
        }
    }

    for name in index.twig_globals.keys() {
        add(name, "Twig global".into());
    }

    for (name, detail) in TWIG_BUILT_IN_VARIABLES {
        add(name, detail.into());
    }

    items
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
    practices: bool,
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
            practices: true,
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
            practices: on("practiceChecks"),
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
        findings.extend(config_checks::unknown_parameters(&ctx, path, language, text));
        findings.extend(wiring::check(&ctx, path, language, text));
        findings.extend(wiring::php_constants(&ctx, path, language, text));
    }

    if groups.yaml_syntax {
        findings.extend(config_checks::yaml_scalars(path, language, text));
    }

    if groups.commands && language == "php" {
        findings.extend(php_checks::commands(text));
    }

    if groups.deprecations && language == "php" {
        findings.extend(php_checks::deprecations(&ctx, text));
    }

    if groups.practices && language == "php" {
        findings.extend(php_checks::practices(text));
    }

    if groups.doctrine {
        findings.extend(doctrine::check(path, language, text));
    }

    findings
}

/// Findings that carry a replacement, for quick fixes: YAML scalars and hard-coded Twig paths.
fn fixes(index: &Index, path: &str, language: &str, text: &str) -> Vec<checks::Finding> {
    let published = host::setting_bool("diagnostics").unwrap_or(true) && !index.partial;
    if !published {
        return Vec::new();
    }

    let groups = Groups::from_settings();
    let mut found = Vec::new();
    if groups.yaml_syntax {
        found.extend(config_checks::yaml_scalars(path, language, text));
    }

    if groups.twig && language == "twig" {
        let read = |_: &str| None;
        found.extend(twig_checks::names(&checks::Context::new(index, &read), text));
    }

    found.into_iter().filter(|finding| finding.replacement.is_some()).collect()
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
        let remaining = self.indexes.iter().next();
        let Some((root, index)) = self.indexes.get_key_value(root).or(remaining) else {
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

    fn run_console_command(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let args = ["bin/console".to_owned(), "list".to_owned(), "--raw".to_owned()];
        let listed = project.run_process("php", &args, None).ok().filter(|o| o.status == Some(0));

        let mut descriptions: BTreeMap<String, Option<String>> = BTreeMap::new();
        for line in listed.iter().flat_map(|o| o.stdout.lines()) {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let Some(name) = parts.next().filter(|n| !n.is_empty()) else { continue };
            descriptions.insert(name.to_owned(), parts.next().map(|d| d.trim().to_owned()));
        }
        if let Some(index) = self.view_index(ctx) {
            for name in index.commands.keys() {
                descriptions.entry(name.clone()).or_default();
            }
        }
        if descriptions.is_empty() {
            return Err("No console commands found. Allow process:php in Settings › Plugins or define a command in the project.".into());
        }

        let items: Vec<PickItem> = descriptions
            .iter()
            .map(|(name, description)| {
                let item = PickItem::new(name, name);
                match description {
                    Some(description) => item.description(description),
                    None => item,
                }
            })
            .collect();
        let Some(name) = host::pick("Run console command", &items)? else { return Ok(None) };
        let Some(arguments) = host::input(&format!("Arguments for bin/console {name}"), None)? else { return Ok(None) };

        let mut args = vec!["bin/console".to_owned(), name.clone()];
        args.extend(arguments.split_whitespace().map(str::to_owned));
        match host::open_terminal(&format!("console {name}"), "php", &args) {
            Err(Error::Denied(_)) => Err("Allow the process:php capability in Settings › Plugins.".into()),
            result => result.map(|()| None),
        }
    }

    fn go_to(&self, ctx: &Invocation, what: &str, select: impl Fn(&Index) -> &Names) -> Result<Option<String>, Error> {
        let index = self.view_index(ctx).ok_or("No Symfony project is indexed.")?;
        let names = select(index);
        if names.is_empty() {
            return Err(format!("No {what}s are indexed.").into());
        }

        let items: Vec<PickItem> = names
            .iter()
            .map(|(name, entry)| {
                let item = PickItem::new(name, name);
                match &entry.detail {
                    Some(detail) => item.description(detail),
                    None => item,
                }
            })
            .collect();
        let Some(key) = host::pick(&format!("Go to {what}"), &items)? else { return Ok(None) };
        let entry = names.get(&key).ok_or_else(|| format!("{key} is no longer indexed."))?;
        if entry.path.is_empty() {
            return Err(format!("{key} is defined by the framework, not by a project file.").into());
        }

        host::open_location(&entry.path, entry.at.line, entry.at.character)?;
        Ok(None)
    }

    fn twig_extends(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("Open a Twig template first.")?;
        let path = editor.path();
        if !path.ends_with(".twig") {
            return Err("The open file is not a Twig template.".into());
        }

        let document = editor.document()?;
        if refs::twig_parent(&document.text).is_some() {
            return Err("This template already extends another.".into());
        }

        let index = self.index_for(&path).ok_or("The file is not in an indexed Symfony project.")?;
        let own = index.template_of(&path);
        let items: Vec<PickItem> =
            index.templates.keys().filter(|name| Some(name.as_str()) != own).map(|name| PickItem::new(name, name)).collect();
        let Some(parent) = host::pick("Extend template", &items)? else { return Ok(None) };

        let start = Position::new(0, 0);
        let edit = TextEdit { range: Range::new(start, start), text: format!("{{% extends '{parent}' %}}\n") };
        editor.apply_edits(document.version, &[edit])?;
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
            "run-console-command" => self.run_console_command(ctx),
            "go-to-route" => self.go_to(ctx, "route", |i| &i.routes),
            "go-to-service" => self.go_to(ctx, "service", |i| &i.services),
            "go-to-command" => self.go_to(ctx, "console command", |i| &i.commands),
            "go-to-template" => self.go_to(ctx, "template", |i| &i.templates),
            "twig-extends" => self.twig_extends(ctx),
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

        if doc.language == "php" {
            if let Some((range, items)) = attr_args::completions(&doc.text, offset) {
                let replace = range_of(&doc.text, range.start, range.end);
                return items
                    .into_iter()
                    .map(|completion| {
                        let mut item = CompletionItem::new(&completion.label, CompletionKind::Property);
                        item.insert_text = completion.insert;
                        item.detail = Some(completion.detail.to_owned());
                        item.replace = Some(replace);
                        item
                    })
                    .collect();
            }
        }

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
        if reference.kind == Kind::Tag {
            return Vec::new();
        }

        let name = &doc.text[reference.start..reference.end];
        let names = candidates(index, &doc.path, &doc.language, &doc.text, &reference);
        let Some(entry) = names.get(name) else { return Vec::new() };

        let is_file = matches!(reference.kind, Kind::Template | Kind::Class | Kind::Asset);
        let width = if is_file { 0 } else { name.encode_utf16().count() as u32 };
        location(entry, width).into_iter().collect()
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
            "php" => {
                let mut php = intentions::php_actions(&doc.text, offset, index.framework_version);
                php.extend(rewrites::php_rewrites(&doc.text, offset, &|class| index.classes.contains_key(class)));
                php
            }
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

        actions.extend(service_actions(index, doc, offset));
        actions.extend(generator_actions(index, doc, offset, selection_end));

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

        actions.extend(typo_fixes(index, doc, offset, selection_end));

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

    fn resolve_code_action(&mut self, doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let data = action.data.clone().ok_or("Nothing to resolve.")?;
        if let Some(rest) = data.strip_prefix("translate\n") {
            action.edits = translation_edits(doc, rest)?;
            return Ok(action);
        }

        if let Some(rest) = data.strip_prefix("service\n") {
            let (services_file, class_file) = rest.split_once('\n').ok_or("Malformed code action.")?;
            let index = self.index_for(services_file).ok_or("The file is not in a Symfony project.")?;
            action.edits = vec![service_definition_edit(index, services_file, class_file)?];
            return Ok(action);
        }

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

/// Replaces the selected Twig text with a `trans` call and adds the key to the domain's translation file.
fn translation_edits(doc: &Document, data: &str) -> Result<Vec<(String, Vec<TextEdit>)>, Error> {
    let mut parts = data.splitn(4, '\n');
    let (file, start, end, key) = (parts.next(), parts.next(), parts.next(), parts.next());
    let (Some(file), Some(Ok(start)), Some(Ok(end)), Some(key)) = (file, start.map(str::parse::<usize>), end.map(str::parse::<usize>), key) else { return Err("Malformed code action.".into()) };
    let value = doc.text.get(start..end).ok_or("The selection changed.")?;
    let project = project_for(file).ok_or("The file is not in an open project.")?;
    let existing = project.read_file(file)?;

    let (at, line) = generators::translation_line(&existing, key, value);
    let replacement = TextEdit { range: range_of(&doc.text, start, end), text: format!("{{{{ '{key}'|trans }}}}") };
    let addition = TextEdit { range: range_of(&existing, at, at), text: line };

    Ok(vec![(doc.path.clone(), vec![replacement]), (file.to_owned(), vec![addition])])
}

/// "Add form fields" in a form type and "Extract translation" for selected Twig text.
fn generator_actions(index: &Index, doc: &Document, offset: usize, selection_end: usize) -> Vec<CodeAction> {
    let Some(project) = project_for(&doc.path) else { return Vec::new() };

    if doc.language == "php" {
        let read = |file: &str| project.read_file(file).ok();
        let ctx = checks::Context::new(index, &read);
        return generators::form_fields(&doc.text, offset, &ctx)
            .into_iter()
            .map(|action| CodeAction {
                title: action.title,
                kind: Some(action.kind.into()),
                preferred: false,
                edits: vec![(doc.path.clone(), action.edits.into_iter().map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text }).collect())],
                data: None,
            })
            .collect();
    }

    if doc.language != "twig" || selection_end <= offset {
        return Vec::new();
    }

    let Some(range) = generators::translatable(&doc.text, offset, selection_end) else { return Vec::new() };
    let domain = generators::default_domain(&doc.text);
    let Some(file) = index.translation_files.get(&domain).and_then(|files| files.iter().find(|file| file.contains(".en.")).or(files.first())) else { return Vec::new() };
    let Some(base) = generators::translation_key(index.template_of(&doc.path), &doc.text[range.clone()]) else { return Vec::new() };

    let taken = |key: &str| index.translations.get(&domain).is_some_and(|keys| keys.contains_key(key));
    let Some(key) = std::iter::once(base.clone()).chain((2..10).map(|n| format!("{base}_{n}"))).find(|key| !taken(key)) else { return Vec::new() };

    vec![CodeAction {
        title: format!("Extract translation \"{key}\" to {}", file_name(file)),
        kind: Some("refactor.extract".into()),
        preferred: false,
        edits: Vec::new(),
        data: Some(format!("translate\n{file}\n{}\n{}\n{key}", range.start, range.end)),
    }]
}

const SERVICE_FILES: [&str; 3] = ["config/services.yaml", "config/services.yml", "config/services.xml"];

/// The first existing service definition file of the project and its text.
fn services_file(project: &Project) -> Option<(String, String)> {
    let root = project.root().ok()?;
    SERVICE_FILES.iter().find_map(|relative| {
        let path = format!("{root}/{relative}");
        project.read_file(&path).ok().map(|text| (path, text))
    })
}

/// Appends the definition of the class in `class_file` to the service file.
fn service_definition_edit(index: &Index, services_file: &str, class_file: &str) -> Result<(String, Vec<TextEdit>), Error> {
    let project = project_for(services_file).ok_or("The file is not in an open project.")?;
    let services = project.read_file(services_file)?;
    let source = project.read_file(class_file)?;
    let model = php_text::class_model(&source).ok_or("No class found.")?;
    let read = |file: &str| project.read_file(file).ok();
    let ctx = checks::Context::new(index, &read);

    let plan = service_gen::plan(&ctx, &model, &[], !service_gen::has_autoconfigure(&services));
    let (range, text) = match services_file.ends_with(".xml") {
        true => service_gen::xml_insertion(&services, &plan).ok_or("The service file has no <services> element.")?,
        false => service_gen::yaml_insertion(&services, &plan),
    };

    Ok((services_file.to_owned(), vec![TextEdit { range: range_of(&services, range.start, range.end), text }]))
}

/// Service definition generators: a new definition for the class at the caret
/// (in the project's service file) and, in a YAML service file, constructor
/// arguments and implied tags for the service at the caret.
fn service_actions(index: &Index, doc: &Document, offset: usize) -> Vec<CodeAction> {
    let Some(project) = project_for(&doc.path) else { return Vec::new() };
    let read = |file: &str| project.read_file(file).ok();
    let ctx = checks::Context::new(index, &read);
    let mut actions = Vec::new();

    if doc.language == "yaml" && checks::is_service_file(&doc.path) {
        let edit = |range: std::ops::Range<usize>, text: String| vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text }])];

        if let Some((range, text)) = service_gen::resync_arguments(&ctx, &doc.text, offset) {
            actions.push(CodeAction { title: "Resync arguments with the constructor".into(), kind: Some("refactor.rewrite".into()), preferred: false, edits: edit(range, text), data: None });
        }

        if let Some((tags, (range, text))) = service_gen::add_implied_tags(&ctx, &doc.text, offset) {
            actions.push(CodeAction { title: format!("Add tags {}", tags.join(", ")), kind: Some("refactor.rewrite".into()), preferred: false, edits: edit(range, text), data: None });
        }
    }

    if doc.language != "php" {
        return actions;
    }

    let Some(model) = php_text::class_model(&doc.text) else { return actions };
    let masked = php_text::mask(&doc.text);
    let line_start = masked[..model.name_range.start].rfind('\n').map_or(0, |at| at + 1);
    let line_end = masked[model.name_range.start..].find('\n').map_or(masked.len(), |at| model.name_range.start + at);
    let on_declaration = (line_start..=line_end).contains(&offset);
    let declaration = &masked[line_start..model.name_range.start];
    let concrete = declaration.contains("class ") && !declaration.contains("abstract");
    if !on_declaration || !concrete || !index.classes.contains_key(&model.fqcn) || index.services.contains_key(&model.fqcn) {
        return actions;
    }

    let Some((services_path, services)) = services_file(&project) else { return actions };
    if service_gen::registered_by_prototype(&services, &model.fqcn) {
        return actions;
    }

    actions.push(CodeAction {
        title: format!("Add service definition to {}", file_name(&services_path)),
        kind: Some("refactor.rewrite".into()),
        preferred: false,
        edits: Vec::new(),
        data: Some(format!("service\n{services_path}\n{}", doc.path)),
    });
    actions
}

/// Replaces a route, template, translation key or translation domain that is
/// not defined with the single known name that differs by a typo.
fn typo_fixes(index: &Index, doc: &Document, offset: usize, selection_end: usize) -> Vec<CodeAction> {
    let mut wanted: Vec<(std::ops::Range<usize>, String)> = Vec::new();

    if let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) {
        let name = &doc.text[reference.start..reference.end];
        let domain = domain_of(&doc.language, &doc.text, &reference);
        let known = match reference.kind {
            Kind::Route if index.compiled_routes && !name.starts_with('_') && !index.routes.contains_key(name) => Some(&index.routes),
            Kind::Template if !name.starts_with('@') && !index.templates.contains_key(name) => Some(&index.templates),
            Kind::Translation => index.translations.get(&domain).filter(|keys| !keys.contains_key(name)),
            _ => None,
        };

        let near = known.filter(|_| !is_dynamic(name)).and_then(|names| suggest::closest(name, names.keys().map(String::as_str).filter(|candidate| !candidate.starts_with('_'))));
        wanted.extend(near.map(|candidate| (reference.start..reference.end, candidate.to_owned())));
    }

    let read = |_: &str| None;
    let domain_codes = ["missing-translation-domain", "twig-missing-translation-domain"];
    for finding in findings(index, Groups::from_settings(), &read, &doc.path, &doc.language, &doc.text) {
        let overlaps = finding.start <= selection_end && finding.end >= offset;
        if !overlaps || !domain_codes.contains(&finding.code) {
            continue;
        }

        let domains = index.translations.keys().map(String::as_str);
        let near = suggest::closest(&doc.text[finding.start..finding.end], domains);
        wanted.extend(near.map(|candidate| (finding.start..finding.end, candidate.to_owned())));
    }

    wanted
        .into_iter()
        .map(|(range, candidate)| CodeAction {
            title: format!("Change to \"{candidate}\""),
            kind: Some("quickfix".into()),
            preferred: true,
            edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text: candidate }])],
            data: None,
        })
        .collect()
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

#[cfg(not(target_arch = "wasm32"))]
const _: fn() -> Main = <Main as Default>::default;
#[cfg(target_arch = "wasm32")]
const _: () = {
    struct Component;
    openidea_plugin::__plugin_exports!(Main, Component, openidea_plugin::bindings);
    openidea_plugin::__providers_exports!(Main, Component);
    openidea_plugin::__views_exports!(Main, Component);
    openidea_plugin::__indexing_exports!(Main, Component);
    openidea_plugin::bindings::__export_plugin_world!(Component with_types_in openidea_plugin::bindings);
};

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

    fn console_host() -> (MockHost, Main) {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
        let mut plugin = activated(&host);
        host.add_file("src/Command/SyncCommand.php", "<?php\n#[AsCommand(name: 'app:sync')]\nclass SyncCommand {}\n");
        plugin.index_all();
        host.on_process("php", |args, _| {
            assert_eq!(args, ["bin/console", "list", "--raw"]);
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "about  Display info\ncache:clear  Clear\n".into(), stderr: String::new() })
        });
        (host, plugin)
    }

    #[test]
    fn runs_a_picked_console_command_in_a_terminal() {
        let (host, mut plugin) = console_host();
        host.answer_next_prompt(Some(&["app:sync"]));
        host.answer_next_prompt(Some(&["--env=dev -v"]));
        plugin.run_command("run-console-command", &host.invocation(), None).unwrap();
        let terminal = &host.terminals()[0];
        assert_eq!((terminal.title.as_str(), terminal.program.as_str()), ("console app:sync", "php"));
        assert_eq!(terminal.args, ["bin/console", "app:sync", "--env=dev", "-v"]);
    }

    #[test]
    fn dismissed_console_prompts_run_nothing() {
        let (host, mut plugin) = console_host();
        plugin.run_command("run-console-command", &host.invocation(), None).unwrap();
        host.answer_next_prompt(Some(&["about"]));
        plugin.run_command("run-console-command", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn go_to_commands_open_the_picked_name() {
        let (host, mut plugin) = console_host();
        let ctx = host.invocation();
        host.answer_next_prompt(Some(&["blog_index"]));
        plugin.run_command("go-to-route", &ctx, None).unwrap();
        host.answer_next_prompt(Some(&["app:sync"]));
        plugin.run_command("go-to-command", &ctx, None).unwrap();
        host.answer_next_prompt(None);
        plugin.run_command("go-to-service", &ctx, None).unwrap();
        let opened = host.opened_locations();
        assert_eq!(opened.len(), 2);
        assert_eq!(opened[0].0, "/project/src/Controller/BlogController.php");
        assert_eq!(opened[1], ("/project/src/Command/SyncCommand.php".into(), 1, 19));
    }

    #[test]
    fn extends_generator_inserts_the_picked_template() {
        let (host, mut plugin) = console_host();
        host.open_editor("/project/templates/page.html.twig", "twig", "{% block body %}{% endblock %}", &[(0, 0, 0, 0)]);
        host.answer_next_prompt(Some(&["base.html.twig"]));
        plugin.run_command("twig-extends", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "{% extends 'base.html.twig' %}\n{% block body %}{% endblock %}");

        host.answer_next_prompt(None);
        assert!(plugin.run_command("twig-extends", &host.invocation(), None).is_err());
        assert_eq!(host.editor_version(), 2);
    }

    #[test]
    fn dismissed_extends_generator_leaves_the_template_alone() {
        let (host, mut plugin) = console_host();
        host.open_editor("/project/templates/page.html.twig", "twig", "x", &[(0, 0, 0, 0)]);
        host.answer_next_prompt(None);
        plugin.run_command("twig-extends", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "x");
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

    fn complete_marked(plugin: &mut Main, path: &str, language: &str, marked: &str) -> Vec<String> {
        let at = marked.find('|').unwrap();
        let text = marked.replace('|', "");
        let before = &marked[..at];
        let line = before.matches('\n').count() as u32;
        let column = before.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
        labels(plugin.completion(&doc(path, language, &text), Position::new(line, column), None))
    }

    fn tagged_project(host: &MockHost) {
        host.add_file(
            "config/services/tags.yaml",
            "services:\n    a:\n        tags:\n            - { name: app.flow_tag, priority: 1 }\n            - name: app.block_tag\n            - app.bare_tag\n    b:\n        tags: ['app.list_tag']\n",
        );
        host.add_file("config/services/tags.xml", "<services><service id=\"c\"><tag name=\"app.xml_tag\"/></service></services>");
        host.add_file("src/legacy.yaml", "services:\n    z:\n        tags: [src.tag]\n");
        host.add_file("translations/t.yaml", "services:\n    z:\n        tags: [translation.tag]\n");
        host.add_file(
            "config/services/commented.yaml",
            "services:\n    a:\n        # tags: [commented.list]\n        tags:\n            # - { name: commented.item }\n            - app.real\n",
        );
        host.add_file("config/services/commented.xml", "<services><!-- <tag name=\"commented.xml\"/> --></services>");
    }

    const SERVICES_FILE: &str = "/project/config/services.yaml";

    #[test]
    fn completes_core_and_project_tags_in_yaml_service_tags() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        for marked in [
            "services:\n    a:\n        tags:\n            - { name: | }\n",
            "services:\n    a:\n        tags:\n            - name: |\n",
            "services:\n    a:\n        tags: ['|']\n",
            "services:\n    a:\n        tags: [{ name: '|' }]\n",
            "services:\n    a:\n        tags:\n            - |\n",
            "services:\n    a:\n        tags:\n            - priority: 3\n              name: app.fl|\n",
        ] {
            let offered = complete_marked(&mut plugin, SERVICES_FILE, "yaml", marked);
            for expected in ["kernel.event_listener", "console.command", "twig.extension", "app.flow_tag", "app.block_tag", "app.bare_tag", "app.list_tag", "app.xml_tag", "app.real"] {
                assert!(offered.contains(&expected.to_owned()), "{expected} in {marked:?}");
            }
        }
    }

    #[test]
    fn completes_tags_in_xml_tag_elements() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, "/project/config/services.xml", "xml", "<service id=\"a\"><tag name=\"|\"/></service>");
        assert!(offered.contains(&"kernel.event_subscriber".to_owned()));
        assert!(offered.contains(&"app.flow_tag".to_owned()));
        assert!(offered.contains(&"app.xml_tag".to_owned()));
    }

    #[test]
    fn tag_completion_is_quiet_outside_tags_and_in_other_files() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        let quiet = |plugin: &mut Main, path: &str, language: &str, marked: &str| {
            let offered = complete_marked(plugin, path, language, marked);
            assert!(!offered.contains(&"kernel.event_listener".to_owned()), "{marked:?}");
        };
        quiet(&mut plugin, SERVICES_FILE, "yaml", "parameters:\n    tags: ['|']\n");
        quiet(&mut plugin, SERVICES_FILE, "yaml", "parameters:\n    tags:\n        - |\n");
        quiet(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        arguments:\n            - name: |\n");
        quiet(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        arguments: [{ name: '|' }]\n");
        quiet(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags:\n            - { priority: |, name: x }\n");
        quiet(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags:\n            - { name: x }\n        calls:\n            - |\n");
        quiet(&mut plugin, "/project/src/Controller/HomeController.php", "php", "<?php\n$x->tags('|');\n");
        quiet(&mut plugin, "/project/templates/a.html.twig", "twig", "{{ include('|') }}");
        quiet(&mut plugin, "/project/config/services.xml", "xml", "<service id=\"a\"><argument name=\"|\"/></service>");
        quiet(&mut plugin, "/project/config/services.xml", "xml", "<service id=\"a\"><tags name=\"|\"/></service>");
    }

    #[test]
    fn project_tags_come_from_config_files_only() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags: ['|']\n");
        assert!(offered.contains(&"app.list_tag".to_owned()));
        assert!(!offered.contains(&"src.tag".to_owned()));
        assert!(!offered.contains(&"translation.tag".to_owned()));
    }

    #[test]
    fn tag_completion_ignores_commented_lines() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags: ['|']\n");
        assert!(offered.contains(&"app.real".to_owned()));
        for hidden in ["commented.list", "commented.item", "commented.xml"] {
            assert!(!offered.contains(&hidden.to_owned()), "{hidden}");
        }

        let commented = complete_marked(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags:\n            # - { name: |\n");
        assert!(!commented.contains(&"kernel.event_listener".to_owned()));
    }

    #[test]
    fn tag_names_have_no_definition_and_no_problem() {
        let host = MockHost::install();
        tagged_project(&host);
        let mut plugin = activated(&host);

        let yaml = doc(SERVICES_FILE, "yaml", "services:\n    a:\n        tags: ['app.list_tag', 'unknown.tag']\n");
        assert!(plugin.definition(&yaml, Position::new(2, 20)).is_empty());
        let index = plugin.index_for(SERVICES_FILE).unwrap();
        assert!(problems(index, SERVICES_FILE, "yaml", &yaml.text).is_empty());
    }

    #[test]
    fn core_tags_are_documented_symfony_tags() {
        assert!(!CORE_TAGS.contains(&"runtime.reset"));
        assert!(!CORE_TAGS.contains(&"security.authenticator.login_linker"));
        assert!(CORE_TAGS.contains(&"kernel.reset"));
    }

    #[test]
    fn project_tags_keep_the_core_tag_detail() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let yaml = doc(SERVICES_FILE, "yaml", "services:\n    a:\n        tags: ['kernel.reset']\n");
        let project_entry = Entry { detail: Some("project".into()), path: "x".into(), at: Position::new(0, 0) };
        plugin.indexes.values_mut().for_each(|index| {
            index.tags.insert("kernel.reset".into(), project_entry.clone());
        });
        let reference = refs::reference_at("yaml", &yaml.text, yaml.text.find("kernel.reset").unwrap() + 2).unwrap();
        let index = plugin.index_for(SERVICES_FILE).unwrap();
        let names = candidates(index, SERVICES_FILE, "yaml", &yaml.text, &reference);
        assert_eq!(names["kernel.reset"].detail.as_deref(), Some("Symfony core tag"));
    }

    #[test]
    fn twig_namespaces_setting_resolves_templates() {
        let host = MockHost::install();
        host.set_setting("twigNamespaces", r#""emails=templates/mail""#);
        host.add_file("templates/mail/welcome.html.twig", "hi");
        let mut plugin = activated(&host);

        let twig = doc("/project/templates/a.html.twig", "twig", "{% include '@emails/welcome.html.twig' %}");
        assert_eq!(plugin.definition(&twig, Position::new(0, 16))[0].path, "/project/templates/mail/welcome.html.twig");

        let offered = complete_marked(&mut plugin, "/project/templates/a.html.twig", "twig", "{% include '@|' %}");
        assert!(offered.contains(&"@emails/welcome.html.twig".to_owned()));
    }

    #[test]
    fn twig_namespaces_setting_merges_with_twig_yaml_paths() {
        let host = MockHost::install();
        host.set_setting("twigNamespaces", r#""emails=templates/mail""#);
        host.add_file("config/packages/twig.yaml", "twig:\n    paths:\n        'templates/pdf': pdf\n");
        host.add_file("templates/mail/welcome.html.twig", "hi");
        host.add_file("templates/pdf/invoice.html.twig", "pdf");
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, "/project/templates/a.html.twig", "twig", "{% include '@|' %}");
        assert!(offered.contains(&"@emails/welcome.html.twig".to_owned()));
        assert!(offered.contains(&"@pdf/invoice.html.twig".to_owned()));
    }

    #[test]
    fn twig_namespaces_setting_ignores_malformed_pairs() {
        let host = MockHost::install();
        host.set_setting("twigNamespaces", r#""nodelimiter, a=, b=../mail, c=templates/ok""#);
        host.add_file("mail/welcome.html.twig", "hi");
        host.add_file("templates/ok/fine.html.twig", "ok");
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, "/project/templates/a.html.twig", "twig", "{% include '|' %}");
        assert!(offered.contains(&"@c/fine.html.twig".to_owned()));
        assert!(!offered.iter().any(|name| name.contains("welcome") || name.starts_with("@a/") || name.starts_with("@b/")), "{offered:?}");
    }

    #[test]
    fn extra_service_and_routing_files_settings_feed_the_index() {
        let host = MockHost::install();
        host.set_setting("extraServiceFiles", r#""legacy/services.yml, legacy/other.xml, gone.yml""#);
        host.set_setting("extraRoutingFiles", r#""legacy/routes.yml""#);
        host.add_file("legacy/services.yml", "services:\n    legacy.mailer:\n        class: App\\M\n");
        host.add_file("legacy/other.xml", "<container><services><service id=\"legacy.xml\" class=\"App\\X\"/></services></container>");
        host.add_file("legacy/routes.yml", "legacy_home:\n    path: /\n    controller: App\\C::i\n");
        let plugin = activated(&host);

        let index = plugin.index_for(SERVICES_FILE).unwrap();
        assert!(index.services.contains_key("legacy.mailer"));
        assert!(index.services.contains_key("legacy.xml"));
        assert!(index.routes.contains_key("legacy_home"));
    }

    #[test]
    fn missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        host.set_setting("twigNamespaces", r#""gone=templates/gone""#);
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags: ['|']\n");
        assert!(offered.contains(&"kernel.event_listener".to_owned()));
        assert!(plugin.index_for(SERVICES_FILE).unwrap().tags.is_empty());
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
    fn replacement_fixes_follow_diagnostics_setting_and_partial_index() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let yaml = doc("/project/config/services.yaml", "yaml", "b: \"App\\Foo\"\n");
        let all = Range::new(Position::new(0, 0), Position::new(1, 0));
        let replaces = |plugin: &mut Main| plugin.code_actions(&yaml, all).iter().any(|a| a.title.starts_with("Replace with"));
        assert!(replaces(&mut plugin));

        host.set_setting("diagnostics", "false");
        assert!(!replaces(&mut plugin));
        host.set_setting("diagnostics", "true");
        assert!(replaces(&mut plugin));

        plugin.indexes.values_mut().for_each(|index| index.partial = true);
        assert!(!replaces(&mut plugin));
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

        assert_eq!(ids.len(), 17);
        assert_eq!((names.len(), paths.len()), (17, 17));
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
            let namespace = match file {
                "ServicesYaml.yaml" => "App\\:",
                "ServicesPhp.php" => "'App\\\\'",
                _ => "namespace=\"App\\\"",
            };
            assert!(text.contains(namespace), "{file}");
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
        let dark = std::fs::read_to_string(format!("{root}symfony_dark.svg")).unwrap();
        assert!(!dark.contains("#6C707E"));
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

    #[test]
    fn status_item_stays_while_another_symfony_project_is_indexed() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        plugin.update_status_item("/not-symfony");
        assert!(host.status_items().contains_key("symfony"));

        plugin.indexes.clear();
        plugin.update_status_item("/not-symfony");
        assert!(!host.status_items().contains_key("symfony"));
    }

    fn change_titles(plugin: &mut Main, document: &Document, marked_at: usize) -> Vec<(String, String)> {
        let before = &document.text[..marked_at];
        let line = before.matches('\n').count() as u32;
        let column = before.rsplit('\n').next().unwrap().encode_utf16().count() as u32;
        let at = Position::new(line, column);
        plugin
            .code_actions(document, Range::new(at, at))
            .into_iter()
            .filter(|action| action.title.starts_with("Change to"))
            .map(|action| (action.title, action.edits[0].1[0].text.clone()))
            .collect()
    }

    #[test]
    fn typo_in_a_route_template_key_or_domain_offers_the_known_name() {
        let host = MockHost::install();
        host.add_file("var/cache/dev/url_generating_routes.php", "<?php\n\nreturn [\n    'app_home' => [[], ['_controller' => 'x'], [], []],\n    'app_about' => [[], ['_controller' => 'x'], [], []],\n];\n");
        let mut plugin = activated(&host);
        let path = "/project/src/Controller/BlogController.php";

        let route = doc(path, "php", "<?php\n$this->generateUrl('app_hom');\n");
        assert_eq!(change_titles(&mut plugin, &route, route.text.find("app_hom").unwrap() + 2), [("Change to \"app_home\"".to_owned(), "app_home".to_owned())]);

        let template = doc(path, "php", "<?php\n$this->render('blog/indx.html.twig');\n");
        assert_eq!(change_titles(&mut plugin, &template, template.text.find("indx").unwrap()), [("Change to \"blog/index.html.twig\"".to_owned(), "blog/index.html.twig".to_owned())]);

        let domain = doc(path, "php", "<?php\n$t->trans('menu', [], 'admn');\n");
        assert_eq!(change_titles(&mut plugin, &domain, domain.text.find("admn").unwrap()), [("Change to \"admin\"".to_owned(), "admin".to_owned())]);

        let twig_path = "/project/templates/a.html.twig";
        let in_twig = doc(twig_path, "twig", "{{ path('app_abot') }}{{ 'app.titl'|trans({}, 'messages') }}");
        assert_eq!(change_titles(&mut plugin, &in_twig, in_twig.text.find("app_abot").unwrap()), [("Change to \"app_about\"".to_owned(), "app_about".to_owned())]);
        assert_eq!(change_titles(&mut plugin, &in_twig, in_twig.text.find("app.titl").unwrap()), [("Change to \"app.title\"".to_owned(), "app.title".to_owned())]);
    }

    #[test]
    fn typo_fix_is_quiet_for_ties_dynamic_names_and_defined_names() {
        let host = MockHost::install();
        host.add_file("var/cache/dev/url_generating_routes.php", "<?php\n\nreturn [\n    'app_hom1' => [[], [], [], []],\n    'app_hom2' => [[], [], [], []],\n    'app_home' => [[], [], [], []],\n];\n");
        let mut plugin = activated(&host);
        let path = "/project/src/Controller/BlogController.php";

        for text in ["<?php\n$this->generateUrl('app_hom');\n", "<?php\n$this->generateUrl('app_home');\n", "<?php\n$this->generateUrl('app_' . $x);\n", "<?php\n$this->generateUrl('zzzzzz');\n"] {
            let document = doc(path, "php", text);
            let at = text.find("generateUrl").unwrap() + 14;
            assert!(change_titles(&mut plugin, &document, at).is_empty(), "{text}");
        }
    }

    #[test]
    fn twig_path_replacement_hints_at_exact_route_paths_only() {
        let host = MockHost::install();
        host.add_file("config/routes.yaml", "blog:\n    path: /blog\n    controller: App\\C::i\nshow:\n    path: /blog/{id}\nx:\n    path: /dup\ny:\n    path: /dup\n");
        let mut plugin = activated(&host);
        let text = "<a href=\"/blog\">a</a><a href='/blog/{id}'>b</a><a href=\"/dup\">c</a><a href=\"https://x/blog\">d</a><form action=\"/blog\"></form><a data-href=\"/blog\"></a>{# <a href=\"/blog\"> #}";
        let template = doc("/project/templates/a.html.twig", "twig", text);

        let index = plugin.index_for("/project/x").unwrap();
        let read = |_: &str| None;
        let found = extra_problems(index, Groups::all_on(), &read, &template.path, "twig", text);
        let found: Vec<_> = found.iter().filter(|d| d.code.as_deref() == Some("twig-path-replacement")).collect();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].severity, Severity::Hint);
        assert_eq!(found[0].message, "Use path('blog') instead of the hard-coded URL \"/blog\".");

        let at = Position::new(0, 10);
        let actions = plugin.code_actions(&template, Range::new(at, at));
        let fix = actions.iter().find(|action| action.title == "Replace with {{ path('blog') }}").unwrap();
        assert_eq!(fix.edits[0].1[0].range, Range::new(Position::new(0, 9), Position::new(0, 14)));
    }

    const MAILER: &str = "<?php\nnamespace App\\Service;\n\nuse App\\Logger;\n\nclass Mailer\n{\n    public function __construct(private Logger $logger, private string $dsn, private int $retries = 3, private array $options = [], private ?Missing $x = null)\n    {\n    }\n}\n";

    const VOTER: &str = "<?php\nnamespace App\\Security;\n\nuse Symfony\\Component\\Security\\Core\\Authorization\\Voter\\Voter;\n\nclass PostVoter extends Voter\n{\n}\n";

    fn generator_project(host: &MockHost, services: &str) {
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        host.add_file("src/Service/Mailer.php", MAILER);
        host.add_file("src/Logger.php", "<?php\nnamespace App;\n\nclass Logger {}\n");
        host.add_file("src/Security/PostVoter.php", "<?php\nnamespace App\\Security;\n\nuse Symfony\\Component\\Security\\Core\\Authorization\\Voter\\Voter;\n\nclass PostVoter extends Voter\n{\n}\n");
        if !services.is_empty() {
            host.add_file("config/services.yaml", services);
        }
    }

    fn service_action(plugin: &mut Main, path: &str, text: &str, marker: &str) -> Option<CodeAction> {
        let document = doc(path, "php", text);
        let before = &text[..text.find(marker).unwrap()];
        let at = Position::new(before.matches('\n').count() as u32, before.rsplit('\n').next().unwrap().len() as u32);
        let action = plugin.code_actions(&document, Range::new(at, at)).into_iter().find(|action| action.title.starts_with("Add service definition"))?;
        Some(plugin.resolve_code_action(&document, action).unwrap())
    }

    fn applied(text: &str, action: &CodeAction) -> String {
        let (_, edits) = &action.edits[0];
        let mut out = text.to_owned();
        for edit in edits {
            let start = text::offset(text, edit.range.start).unwrap();
            let end = text::offset(text, edit.range.end).unwrap();
            out.replace_range(start..end, &edit.text);
        }
        out
    }

    #[test]
    fn service_definition_is_appended_to_the_existing_yaml_file() {
        let host = MockHost::install();
        let services = "parameters:\n    a: 1\n\nservices:\n    _defaults:\n        autowire: true\n\n    App\\Other: ~\n\nwhen@dev:\n    services:\n        x: ~\n";
        generator_project(&host, services);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let action = service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").unwrap();
        assert_eq!(action.title, "Add service definition to services.yaml");
        assert_eq!(action.edits[0].0, "/project/config/services.yaml");
        let out = applied(services, &action);
        assert!(out.contains("    App\\Other: ~\n\n    App\\Service\\Mailer:\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: ''\n\nwhen@dev:"), "{out}");
        assert!(!out.contains("$retries") && !out.contains("$options") && !out.contains("$x:"), "{out}");
    }

    #[test]
    fn service_definition_adds_implied_tags_unless_autoconfigure_is_on() {
        let host = MockHost::install();
        generator_project(&host, "services:\n    _defaults:\n        autowire: true\n");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        let voter = VOTER.to_owned();

        let action = service_action(&mut plugin, "/project/src/Security/PostVoter.php", &voter, "class PostVoter").unwrap();
        assert!(applied("services:\n    _defaults:\n        autowire: true\n", &action).ends_with("    App\\Security\\PostVoter:\n        tags: ['security.voter']\n"));

        let auto = "services:\n    _defaults:\n        autowire: true\n        autoconfigure: true\n";
        host.add_file("config/services.yaml", auto);
        let action = service_action(&mut plugin, "/project/src/Security/PostVoter.php", &voter, "class PostVoter").unwrap();
        assert!(applied(auto, &action).ends_with("    App\\Security\\PostVoter: ~\n"));
    }

    #[test]
    fn service_definition_goes_into_xml_when_there_is_no_yaml_file() {
        let host = MockHost::install();
        generator_project(&host, "");
        let xml = "<?xml version=\"1.0\"?>\n<container>\n    <services>\n        <service id=\"App\\Other\"/>\n    </services>\n</container>\n";
        host.add_file("config/services.xml", xml);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let action = service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").unwrap();
        let out = applied(xml, &action);
        assert!(out.contains("        <service id=\"App\\Other\"/>\n        <service id=\"App\\Service\\Mailer\">\n            <argument key=\"$logger\" type=\"service\" id=\"App\\Logger\"/>\n            <argument key=\"$dsn\" type=\"string\"></argument>\n        </service>\n    </services>"), "{out}");
    }

    #[test]
    fn service_definition_is_not_offered_for_registered_or_other_classes() {
        let host = MockHost::install();
        generator_project(&host, "services:\n    App\\:\n        resource: '../src/'\n");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").is_none());

        host.add_file("config/services.yaml", "services:\n    App\\Service\\Mailer: ~\n");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").is_none());

        host.add_file("config/services.yaml", "services: {}\n");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "namespace").is_none());
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").is_some());
        let abstract_text = MAILER.replace("class Mailer", "abstract class Mailer");
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", &abstract_text, "abstract class").is_none());
    }

    fn yaml_actions(plugin: &mut Main, text: &str, marker: &str) -> Vec<CodeAction> {
        let document = doc("/project/config/services.yaml", "yaml", text);
        let before = &text[..text.find(marker).unwrap()];
        let at = Position::new(before.matches('\n').count() as u32, before.rsplit('\n').next().unwrap().len() as u32);
        plugin.code_actions(&document, Range::new(at, at))
    }

    fn yaml_applied(text: &str, action: &CodeAction) -> String {
        let mut out = text.to_owned();
        for edit in &action.edits[0].1 {
            let start = text::offset(text, edit.range.start).unwrap();
            let end = text::offset(text, edit.range.end).unwrap();
            out.replace_range(start..end, &edit.text);
        }
        out
    }

    #[test]
    fn arguments_are_resynced_with_the_constructor() {
        let host = MockHost::install();
        generator_project(&host, "");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let stale = "services:\n    App\\Service\\Mailer:\n        arguments:\n            $dsn: 'smtp://x'\n            $gone: 1\n\n    App\\Other: ~\n";
        let actions = yaml_actions(&mut plugin, stale, "$dsn");
        let action = actions.iter().find(|action| action.title == "Resync arguments with the constructor").unwrap();
        assert_eq!(yaml_applied(stale, action), "services:\n    App\\Service\\Mailer:\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: 'smtp://x'\n\n    App\\Other: ~\n");

        let bare = "services:\n    App\\Service\\Mailer:\n        public: true\n\n    App\\Other: ~\n";
        let actions = yaml_actions(&mut plugin, bare, "public");
        let action = actions.iter().find(|action| action.title == "Resync arguments with the constructor").unwrap();
        assert_eq!(yaml_applied(bare, action), "services:\n    App\\Service\\Mailer:\n        public: true\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: ''\n\n    App\\Other: ~\n");
    }

    #[test]
    fn arguments_resync_is_quiet_for_positional_lists_in_sync_blocks_and_unknown_classes() {
        let host = MockHost::install();
        generator_project(&host, "");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        for text in [
            "services:\n    App\\Service\\Mailer:\n        arguments:\n            - '@App\\Logger'\n",
            "services:\n    App\\Service\\Mailer:\n        arguments: ['@App\\Logger']\n",
            "services:\n    App\\Service\\Mailer:\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: 'x'\n",
            "services:\n    app.x:\n        class: Vendor\\Thing\n",
        ] {
            let marker = if text.contains("app.x") { "app.x" } else { "App\\Service\\Mailer" };
            let actions = yaml_actions(&mut plugin, text, marker).into_iter().filter(|action| action.title.starts_with("Resync")).count();
            assert_eq!(actions, 0, "{text}");
        }
    }

    #[test]
    fn implied_tags_are_added_to_a_service_without_tags() {
        let host = MockHost::install();
        generator_project(&host, "");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let text = "services:\n    App\\Security\\PostVoter:\n        public: true\n\n    App\\Other: ~\n";
        let actions = yaml_actions(&mut plugin, text, "public");
        let action = actions.iter().find(|action| action.title == "Add tags security.voter").unwrap();
        assert_eq!(yaml_applied(text, action), "services:\n    App\\Security\\PostVoter:\n        public: true\n        tags: ['security.voter']\n\n    App\\Other: ~\n");

        let tagged = text.replace("public: true", "tags: ['x']");
        assert!(yaml_actions(&mut plugin, &tagged, "tags").iter().all(|action| !action.title.starts_with("Add tags")));

        let auto = format!("services:\n    _defaults:\n        autoconfigure: true\n{}", &text["services:\n".len()..]);
        assert!(yaml_actions(&mut plugin, &auto, "public").iter().all(|action| !action.title.starts_with("Add tags")));
    }

    #[test]
    fn selected_twig_text_is_extracted_into_the_translation_file() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let text = "<p>Read more</p>";
        let template = doc("/project/templates/blog/index.html.twig", "twig", text);
        let start = Position::new(0, 3);
        let end = Position::new(0, 12);

        let actions = plugin.code_actions(&template, Range::new(start, end));
        let action = actions.into_iter().find(|action| action.title.starts_with("Extract translation")).unwrap();
        assert_eq!(action.title, "Extract translation \"blog.index.read_more\" to messages.en.yaml");

        let resolved = plugin.resolve_code_action(&template, action).unwrap();
        assert_eq!(resolved.edits[0].0, "/project/templates/blog/index.html.twig");
        assert_eq!(resolved.edits[0].1[0].text, "{{ 'blog.index.read_more'|trans }}");
        assert_eq!(resolved.edits[0].1[0].range, Range::new(start, end));
        assert_eq!(resolved.edits[1].0, "/project/translations/messages.en.yaml");
        assert_eq!(resolved.edits[1].1[0].text, "'blog.index.read_more': 'Read more'\n");

        let at_tag = Range::new(Position::new(0, 0), Position::new(0, 6));
        assert!(plugin.code_actions(&template, at_tag).iter().all(|action| !action.title.starts_with("Extract translation")));
        let empty = Range::new(start, start);
        assert!(plugin.code_actions(&template, empty).iter().all(|action| !action.title.starts_with("Extract translation")));
    }

    #[test]
    fn form_fields_are_offered_in_a_form_type_with_a_project_entity() {
        let host = MockHost::install();
        host.add_file("src/Entity/Product.php", "<?php\nnamespace App\\Entity;\n\nclass Product\n{\n    private string $name;\n    private int $stock;\n}\n");
        let mut plugin = activated(&host);
        let form = "<?php\nnamespace App\\Form;\n\nuse App\\Entity\\Product;\nuse Symfony\\Component\\Form\\AbstractType;\n\nclass ProductType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options): void\n    {\n        $builder->add('name', TextType::class);\n    }\n\n    public function configureOptions($resolver): void\n    {\n        $resolver->setDefaults(['data_class' => Product::class]);\n    }\n}\n";
        let document = doc("/project/src/Form/ProductType.php", "php", form);
        let at = Position::new(10, 10);

        let action = plugin.code_actions(&document, Range::new(at, at)).into_iter().find(|action| action.title == "Add form fields for Product").unwrap();
        let text = &action.edits[0].1.iter().map(|edit| edit.text.as_str()).collect::<String>();
        assert!(text.contains("$builder->add('stock', IntegerType::class);") && !text.contains("'name', TextType"), "{text}");
    }

    #[test]
    fn twig_variables_come_from_the_template_includes_globals_and_built_ins() {
        let host = MockHost::install();
        host.add_file("config/packages/twig.yaml", "twig:\n    globals:\n        site_name: 'x'\n        'ga_id': y\n    paths: {}\n");
        host.add_file("templates/blog/_row.html.twig", "{% set row_local = 1 %}{{ x }}");
        host.add_file("templates/blog/list.html.twig", "{% include 'blog/_row.html.twig' with {post: p, index: 1} %}");
        let mut plugin = activated(&host);

        let offered = complete_marked(&mut plugin, "/project/templates/blog/_row.html.twig", "twig", "{% set row_local = 1 %}{{ |");
        for name in ["row_local", "post", "index", "site_name", "ga_id", "app", "_self"] {
            assert!(offered.contains(&name.to_owned()), "{name} in {offered:?}");
        }

        let elsewhere = complete_marked(&mut plugin, "/project/templates/base.html.twig", "twig", "{{ |");
        assert!(!elsewhere.contains(&"post".to_owned()) && elsewhere.contains(&"site_name".to_owned()));

        let in_loop = complete_marked(&mut plugin, "/project/templates/base.html.twig", "twig", "{% for item in items %}{{ it| }}{% endfor %}");
        assert!(in_loop.contains(&"item".to_owned()) && in_loop.contains(&"loop".to_owned()));
    }

    #[test]
    fn attribute_arguments_complete_in_php_files() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let path = "/project/src/Controller/BlogController.php";
        let header = "<?php\nuse Symfony\\Component\\Routing\\Attribute\\Route;\n\n";

        let names = complete_marked(&mut plugin, path, "php", &format!("{header}#[Route('/x', me|)]\nclass A {{}}"));
        assert!(names.contains(&"methods".to_owned()) && names.contains(&"name".to_owned()), "{names:?}");

        let values = complete_marked(&mut plugin, path, "php", &format!("{header}#[Route('/x', methods: ['|'])]\nclass A {{}}"));
        assert!(values.contains(&"GET".to_owned()), "{values:?}");

        let items = plugin.completion(&doc(path, "php", &format!("{header}#[Route('/x', me)]\nclass A {{}}")), Position::new(3, 16), None);
        let item = items.iter().find(|item| item.label == "methods").unwrap();
        assert_eq!(item.insert_text, "methods: ");
        assert_eq!(item.replace, Some(Range::new(Position::new(3, 14), Position::new(3, 16))));
    }
}
