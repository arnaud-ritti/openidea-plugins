//! Laravel: completion, navigation, problems, gutter markers and a tool
//! window for routes, views, config, translations, env keys, middleware,
//! controllers and assets in PHP and Blade files.
//!
//! The features the free Laravel plugin for IntelliJ (Daniel Espendiller,
//! MIT) has are ported from its public source; the rest follows Laravel's own
//! documentation. See README.md for what was and wasn't ported.

mod actions;
mod blade;
mod bulk;
mod class_actions;
mod components;
mod controllers;
mod directives;
mod eloquent_checks;
mod function_checks;
mod generate;
mod index;
mod indexer;
mod js;
mod lang_actions;
mod migrations;
mod php;
mod refs;
mod routes;
mod unused;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Diagnostic, Event, EventKind, Location, Position, Range, Severity, TextEdit};
use openidea_plugin::{Error, Invocation, LogLevel, NotificationLevel, Plugin, Project, host, text};
use ui_kit::routes::{Handler, Route, RouteEvent, RouteList, copy_as_curl, create_http_file};
use ui_kit::sections::{SectionDef, Switcher};

pub use index::Index;
use index::{AddName, Entry, Lines, Names};
use refs::{Kind, Reference};

const VIEW: &str = "laravel";
const PLUGIN_ID: &str = "aritti.laravel";
const DEFAULT_BASE_URL: &str = "http://localhost:8000";
const SECTIONS: &[SectionDef] = &[SectionDef { id: "routes", label: "Routes" }, SectionDef { id: "index", label: "Index" }];
const SOURCE: &str = "Laravel";
const CONTROLLERS_DIRECTORY: &str = "app/Http/Controllers/";

/// Whether a saved file (project-relative) feeds the index.
fn affects_index(relative: &str) -> bool {
    if matches!(relative, "ide-blade.json" | ".idea/ide-blade.json") {
        return true;
    }

    let directory = [
        "routes/",
        "config/",
        "lang/",
        "resources/lang/",
        "resources/views/",
        "app/Http/Controllers/",
        "app/Mail/",
        "app/View/Components/",
        "app/Livewire/",
        "app/Http/Livewire/",
        "app/Policies/",
        "app/Models/",
        "database/migrations/",
        "app/Providers/",
        "database/factories/",
        "resources/js/Pages/",
        "resources/js/pages/",
    ];
    directory.iter().any(|d| relative.starts_with(d))
        || matches!(relative, "composer.json" | ".env" | ".env.example" | "bootstrap/app.php" | "app/Http/Kernel.php" | "app/Http/routes.php")
}

fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn project_for(path: &str) -> Option<Project> {
    host::projects().ok()?.into_iter().filter(|p| p.root().is_ok_and(|root| under(path, &root))).max_by_key(|p| p.root().map_or(0, |r| r.len()))
}

/// The language the plugin reads a file as: Blade files are PHP files to the
/// IDE when the Blade plugin isn't installed.
fn language_of(path: &str, language: &str) -> Option<&'static str> {
    if path.ends_with(".blade.php") || language == "blade" {
        return Some("blade");
    }
    path.ends_with(".php").then_some("php")
}

fn range_of(text: &str, start: usize, end: usize) -> Range {
    Range::new(text::position(text, start), text::position(text, end))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn location(entry: &Entry) -> Option<Location> {
    if entry.path.is_empty() {
        return None;
    }

    let end = Position::new(entry.at.line, entry.at.character + entry.width);
    Some(Location { path: entry.path.clone(), range: Range::new(entry.at, end) })
}

/// Sections, yields and stacks that the layouts above a view define.
fn layout_names(index: &Index, project: &Project, doc: &Document, kind: Kind) -> Names {
    let mut names = Names::new();
    let Some(parent) = blade::facts(&doc.text).extends else { return names };

    for view in std::iter::once(parent.as_str()).chain(index.parents(&parent)) {
        let (Some(facts), Some(entry)) = (index.blade.get(view), index.views.get(view)) else { continue };
        let Ok(text) = project.read_file(&entry.path) else { continue };
        let lines = Lines::new(&text);

        let found: Vec<&blade::Named> = match kind {
            Kind::Stack => facts.stacks.iter().collect(),
            _ => facts.sections.iter().chain(&facts.yields).collect(),
        };
        for (name, offset) in found {
            names.add(name, Some(view.to_owned()), &entry.path, lines.position(*offset), name.len());
        }
    }

    names
}

/// `Class@method` of every controller.
fn actions(index: &Index) -> Names {
    let mut names = Names::new();
    for (class, methods) in &index.methods {
        for (method, entry) in methods {
            names.insert(format!("{class}@{method}"), entry.clone());
        }
    }
    names
}

/// Names a reference can name, for completion and navigation.
fn candidates<'a>(index: &'a Index, doc: &Document, reference: &Reference) -> Cow<'a, Names> {
    match reference.kind {
        Kind::Route => Cow::Borrowed(&index.routes),
        Kind::View => Cow::Borrowed(&index.views),
        Kind::Config => Cow::Borrowed(&index.config),
        Kind::Translation => Cow::Borrowed(&index.translations),
        Kind::Env => Cow::Borrowed(&index.env),
        Kind::Asset => Cow::Borrowed(&index.assets),
        Kind::Middleware => Cow::Borrowed(&index.middleware),
        Kind::Controller => Cow::Borrowed(&index.controllers),
        Kind::Component => Cow::Borrowed(&index.components),
        Kind::Livewire => Cow::Borrowed(&index.livewire),
        Kind::Vite => Cow::Borrowed(&index.vite),
        Kind::Inertia => Cow::Borrowed(&index.inertia_pages),
        Kind::Rule => Cow::Borrowed(&index.rules),
        Kind::Ability => Cow::Borrowed(&index.abilities),
        Kind::Column => {
            let table = match reference.class.as_deref() {
                Some(table) if table.starts_with("table:") => Some(table["table:".len()..].to_owned()),
                Some(spec) => resolve_model(index, spec, &doc.path).map(|class| index.models.get(&class).cloned().unwrap_or_else(|| index::table_name(&class))),
                None => index.model_files.get(&doc.path).cloned(),
            };
            Cow::Owned(table.and_then(|t| index.columns.get(&t)).cloned().unwrap_or_default())
        }
        Kind::Binding => Cow::Borrowed(&index.bindings),
        Kind::Action => Cow::Owned(actions(index)),
        Kind::Method => {
            let methods = reference.class.as_deref().and_then(|class| index.controller_methods(class));
            Cow::Owned(methods.cloned().unwrap_or_default())
        }
        Kind::Section | Kind::Stack => match project_for(&doc.path) {
            Some(project) => Cow::Owned(layout_names(index, &project, doc, reference.kind)),
            None => Cow::Owned(Names::new()),
        },
    }
}

/// The model a chain such as `Post>comments>published` ends on: `Post` followed by relation or scope calls.
/// An empty start is the model of the current file. `None` when a call is neither.
fn resolve_model(index: &Index, spec: &str, path: &str) -> Option<String> {
    let mut parts = spec.split('>');
    let first = parts.next()?;
    let mut model = if first.is_empty() {
        file_name(path).strip_suffix(".php")?.to_owned()
    } else {
        first.to_owned()
    };

    for hop in parts {
        if index.scopes.get(&model).is_some_and(|scopes| scopes.contains(hop)) {
            continue;
        }

        model = index.relations.get(&model)?.get(hop)?.clone();
    }

    Some(model)
}

fn completion_kind(kind: Kind) -> CompletionKind {
    match kind {
        Kind::Route => CompletionKind::Reference,
        Kind::View | Kind::Asset => CompletionKind::File,
        Kind::Config => CompletionKind::Property,
        Kind::Translation => CompletionKind::Text,
        Kind::Env | Kind::Section | Kind::Stack => CompletionKind::Constant,
        Kind::Middleware => CompletionKind::Module,
        Kind::Controller | Kind::Component | Kind::Livewire => CompletionKind::Class,
        Kind::Vite => CompletionKind::File,
        Kind::Inertia => CompletionKind::File,
        Kind::Column => CompletionKind::Field,
        Kind::Rule => CompletionKind::Keyword,
        Kind::Ability => CompletionKind::Value,
        Kind::Action | Kind::Method => CompletionKind::Method,
        Kind::Binding => CompletionKind::Class,
    }
}

/// A name that is built at run time (`'posts.' . $x`), a wildcard (`'admin.*'`)
/// or still being typed (`'app.'`): not checked.
fn is_dynamic(name: &str) -> bool {
    name.is_empty() || name.contains(['{', '$', '*']) || name.ends_with('.')
}

fn problem(text: &str, range: std::ops::Range<usize>, code: &str, severity: Severity, message: String) -> Diagnostic {
    Diagnostic {
        range: range_of(text, range.start, range.end),
        severity,
        message,
        code: Some(code.into()),
        source: Some(SOURCE.into()),
        unnecessary: false,
        deprecated: false,
        related: Vec::new(),
    }
}

/// Problems of a file: views, config keys and translation keys that don't
/// exist and, when asked for, routes that don't.
pub fn problems(index: &Index, language: &str, text: &str, check_routes: bool) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    let sure = !index.partial;

    for reference in refs::references(language, text).into_iter().filter(|r| r.closed) {
        let name = &text[reference.start..reference.end];
        if is_dynamic(name) {
            continue;
        }

        match reference.kind {
            Kind::View if !name.contains("::") && !index.views.is_empty() && !index.views.contains_key(name) => {
                found.push(problem(text, reference.start..reference.end, "missing-view", Severity::Warning, format!("View \"{name}\" does not exist.")));
            }
            Kind::Config if missing_config(index, name) => {
                found.push(problem(text, reference.start..reference.end, "missing-config", Severity::Info, format!("Config key \"{name}\" is not defined.")));
            }
            Kind::Translation if missing_translation(index, name) => {
                found.push(problem(text, reference.start..reference.end, "missing-translation", Severity::Info, format!("Translation key \"{name}\" is missing.")));
            }
            Kind::Route if check_routes && !index.routes.contains_key(name) => {
                found.push(problem(text, reference.start..reference.end, "missing-route", Severity::Warning, format!("Route \"{name}\" is not defined.")));
            }
            Kind::Inertia if sure && unknown_inertia_page(index, name) && enabled("reportUnknownInertiaPages") => {
                found.push(problem(text, reference.start..reference.end, "laravel.unknown-inertia-page", Severity::Warning, format!("Inertia page \"{name}\" does not exist.")));
            }
            Kind::Livewire if sure && unknown_livewire(index, name) && enabled("reportUnknownLivewireDirectives") => {
                found.push(problem(text, reference.start..reference.end, "laravel.unknown-livewire-directive", Severity::Warning, format!("Livewire component \"{name}\" does not exist.")));
            }
            _ => {}
        }
    }

    if language == "blade" && sure {
        for reference in refs::tag_references(text) {
            let name = &text[reference.start..reference.end];
            let range = reference.start..reference.end;

            match reference.kind {
                Kind::Component if unknown_component(index, name) && enabled("reportUnknownComponents") => {
                    found.push(problem(text, range, "laravel.unknown-component", Severity::Warning, format!("Blade component \"{name}\" does not exist.")));
                }
                Kind::Livewire if unknown_livewire(index, name) && enabled("reportUnknownLivewire") => {
                    found.push(problem(text, range, "laravel.unknown-livewire", Severity::Warning, format!("Livewire component \"{name}\" does not exist.")));
                }
                _ => {}
            }
        }
    }

    if language == "php" && sure && enabled("reportUnknownFactories") {
        for range in refs::factory_classes(text) {
            let class = text[range.clone()].rsplit('\\').next().unwrap_or_default();
            if unknown_factory(index, class) {
                found.push(problem(text, range, "laravel.unknown-factory", Severity::Warning, format!("Model {class} has no factory.")));
            }
        }
    }

    found.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    found
}

/// `db:seed` runs inside `Model::unguarded()`, so mass assignment there is not discarded.
const UNGUARDED_DIRECTORIES: &[&str] = &["/database/seeders/", "/database/seeds/", "/database/factories/"];

/// Mass assignment, prunable and `orderBy` problems of a PHP file.
fn eloquent_problems(project: &Project, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if language != "php" || !enabled("checkEloquent") {
        return Vec::new();
    }

    let Ok(root) = project.root() else { return Vec::new() };
    let read = |relative: &str| project.read_file(&format!("{root}/{relative}")).ok();

    let unguarded_run = UNGUARDED_DIRECTORIES.iter().any(|directory| path.contains(directory));

    eloquent_checks::findings(text, &read)
        .into_iter()
        .filter(|finding| !(unguarded_run && finding.code == "guarded-attribute-assignment"))
        .map(|finding| problem(text, finding.start..finding.end, finding.code, finding.severity, finding.message))
        .collect()
}

/// `env()` calls outside the configuration files.
fn function_problems(project: &Project, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if !enabled("checkFunctions") {
        return Vec::new();
    }

    let Ok(root) = project.root() else { return Vec::new() };
    let relative = path.strip_prefix(&format!("{root}/")).unwrap_or(path);

    function_checks::env_outside_config(relative, language, text)
        .into_iter()
        .map(|finding| problem(text, finding.start..finding.end, finding.code, finding.severity, finding.message))
        .collect()
}

/// `wire:click="method"` naming a method the view's own component class does not have.
fn livewire_method_problems(project: &Project, index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if language != "blade" || index.partial || !enabled("reportUnknownLivewireMethods") {
        return Vec::new();
    }

    let Some(component) = index.view_of(path).and_then(|view| view.strip_prefix("livewire.")) else { return Vec::new() };
    let Some(entry) = index.livewire.get(component).filter(|entry| !entry.path.is_empty()) else { return Vec::new() };
    let Some(methods) = project.read_file(&entry.path).ok().and_then(|class| blade::livewire_methods(&class)) else { return Vec::new() };

    blade::wire_calls(text)
        .into_iter()
        .filter(|call| !methods.contains(&call.name))
        .map(|call| {
            let message = format!("Component class has no method \"{}\".", call.name);
            problem(text, call.start..call.end, "laravel.unknown-livewire-method", Severity::Warning, message)
        })
        .collect()
}

fn controllers_namespace() -> String {
    host::setting_string("controllersNamespace").filter(|namespace| !namespace.is_empty()).unwrap_or_else(|| "App\\Http\\Controllers".into())
}

/// Controllers and methods that `routes/*.php` name and the project lacks.
fn route_controller_problems(project: &Project, index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    let Ok(root) = project.root() else { return Vec::new() };
    let inside_routes = path.strip_prefix(&format!("{root}/")).is_some_and(|relative| relative.starts_with("routes/"));
    if language != "php" || !inside_routes || index.partial || !enabled("reportUnknownRouteControllers") {
        return Vec::new();
    }

    let masked = php::mask_comments(text);
    if masked.contains("::controller(") || masked.contains("->controller(") {
        return Vec::new();
    }

    let namespace = controllers_namespace();
    let imports = routes::imports(&masked);
    let mut declared: BTreeMap<String, Option<Vec<String>>> = BTreeMap::new();
    let mut missing: BTreeMap<usize, (std::ops::Range<usize>, String, Vec<String>)> = BTreeMap::new();
    let mut found = Vec::new();

    for def in routes::parse(&masked) {
        let Some((class, method)) = &def.controller else { continue };
        if method == "__invoke" {
            continue;
        }

        let candidates: Vec<&String> = index.methods.keys().filter(|full| full.rsplit('\\').next() == Some(class.as_str())).collect();
        let range = def.offset..def.offset + def.width;

        match candidates.as_slice() {
            [] => {
                let imported_from_project = imports.iter().any(|(name, from)| name == class && from.starts_with(&namespace));
                if imported_from_project && !index.partial {
                    found.push(problem(text, range, "laravel.unknown-route-controller", Severity::Warning, format!("Controller \"{class}\" does not exist.")));
                }
            }
            [full] => {
                let methods = declared.entry((*full).clone()).or_insert_with(|| {
                    let file = &index.controllers.get(*full)?.path;
                    controllers::declared_methods(&php::mask_comments(&project.read_file(file).ok()?))
                });
                if methods.as_ref().is_some_and(|methods| !methods.contains(method)) {
                    missing.entry(def.offset).or_insert_with(|| (range, class.clone(), Vec::new())).2.push(method.clone());
                }
            }
            _ => {}
        }
    }

    for (range, class, methods) in missing.into_values() {
        match methods.as_slice() {
            [_, _, ..] if !enabled("reportMissingResourceMethods") => {}
            [method] => found.push(problem(text, range, "laravel.unknown-route-method", Severity::Warning, format!("{class} has no method \"{method}\"."))),
            _ => {
                let message = format!("{class} lacks the resource methods {}.", methods.join(", "));
                found.push(problem(text, range, "laravel.missing-resource-methods", Severity::Warning, message));
            }
        }
    }
    found.sort_by_key(|diagnostic| diagnostic.range.start.line);

    found
}

fn enabled(setting: &str) -> bool {
    host::setting_bool(setting).unwrap_or(true)
}

fn unknown_component(index: &Index, name: &str) -> bool {
    !index.open_components && !matches!(name, "slot" | "dynamic-component") && !index.components.contains_key(name)
}

fn unknown_livewire(index: &Index, name: &str) -> bool {
    !index.open_livewire && !matches!(name, "is" | "styles" | "scripts" | "dynamic-component") && !index.livewire.contains_key(name) && !index.views.contains_key(&format!("livewire.{name}"))
}

fn unknown_inertia_page(index: &Index, name: &str) -> bool {
    !index.inertia_pages.is_empty() && !name.contains("::") && !name.ends_with('/') && !index.inertia_pages.contains_key(name)
}

fn unknown_factory(index: &Index, class: &str) -> bool {
    !matches!(class, "static" | "self" | "parent")
        && index.models.contains_key(class)
        && !index.factories.contains(&format!("{class}Factory"))
        && !index.factory_overrides.contains(class)
}

/// A key below an array (or config file) of the project that doesn't have it.
fn missing_config(index: &Index, name: &str) -> bool {
    if index.config.contains_key(name) {
        return false;
    }

    let Some((parent, _)) = name.rsplit_once('.') else { return false };
    index.config.get(parent).is_some_and(|entry| matches!(entry.detail.as_deref(), Some("array" | "config file")))
}

/// `file.key` where the project has a lang file for `file` but not the key.
fn missing_translation(index: &Index, name: &str) -> bool {
    if name.contains("::") || index.translations.contains_key(name) {
        return false;
    }

    let Some((file, _)) = name.split_once('.') else { return false };
    index.translation_files.contains_key(file)
}

#[derive(Default)]
pub struct Main {
    /// By project root.
    indexes: BTreeMap<String, Index>,
    /// Files with published problems, checked again when the index changes.
    checked: BTreeSet<String>,
    /// Expanded nodes of the tool window.
    expanded: BTreeSet<String>,
    /// Project root and time `migrate-fresh` was first asked for.
    fresh_asked: Option<(String, Instant)>,
    sections: Option<Switcher>,
    route_list: Option<RouteList>,
}

impl Main {
    fn index_project(&mut self, project: &Project) {
        let Ok(root) = project.root() else { return };

        if !index::is_laravel(project) {
            self.indexes.remove(&root);
            return;
        }

        let index = Index::build(project);
        if index.partial {
            host::log(LogLevel::Warn, &format!("index of {root} is partial: out of time"));
        }
        self.indexes.insert(root.clone(), index);
        let _ = host::set_context("indexed", true);

        let checked: Vec<String> = self.checked.iter().filter(|path| under(path, &root)).cloned().collect();
        for path in checked {
            self.check(project, &path);
        }
        host::refresh_view(VIEW);
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
        let Some(language) = language_of(path, "") else { return };
        let Some(index) = self.index_for(path) else { return };

        let enabled = host::setting_bool("diagnostics").unwrap_or(true);
        let check_routes = host::setting_bool("reportMissingRoutes").unwrap_or(false);
        let found = match project.read_file(path) {
            Ok(text) if enabled && !index.partial => {
                let mut found = problems(index, language, &text, check_routes);
                found.extend(eloquent_problems(project, path, language, &text));
                found.extend(function_problems(project, path, language, &text));
                found.extend(livewire_method_problems(project, index, path, language, &text));
                found.extend(route_controller_problems(project, index, path, language, &text));
                found.sort_by_key(|d| (d.range.start.line, d.range.start.character));
                found
            }
            _ => Vec::new(),
        };

        if host::publish_diagnostics(path, &found).is_ok() {
            self.checked.insert(path.to_owned());
        }
    }

    fn artisan(&mut self, ctx: &Invocation, command: &str) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let task = host::begin_task(&format!("artisan {command}"), false);
        let args = ["artisan".to_owned(), command.to_owned(), "--no-interaction".to_owned()];

        let output = match project.run_process("php", &args, None) {
            Err(Error::Denied(_)) => return Err("Allow the process:php capability in Settings › Plugins.".into()),
            result => result?,
        };

        if output.status != Some(0) {
            let error = if output.stderr.trim().is_empty() { &output.stdout } else { &output.stderr };
            let last: Vec<&str> = error.trim().lines().rev().take(5).collect();
            let tail = last.into_iter().rev().collect::<Vec<_>>().join("\n");
            return Err(format!("artisan {command} failed:\n{tail}").into());
        }

        task.report(1, Some(2), Some("Indexing"));
        self.index_project(&project);
        host::notify(NotificationLevel::Info, SOURCE, &format!("Ran artisan {command} and reindexed the project."));
        Ok(None)
    }

    fn generator(&self, id: &str, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let root = project.root()?;
        let index = self.indexes.get(&root);

        match id {
            "make" => {
                let mut composer = project.read_file("composer.json").unwrap_or_default();
                if index.is_some_and(|i| !i.livewire.is_empty()) {
                    composer.push_str("\"livewire/livewire\"");
                }
                generate::make(ctx, index, None, &composer)
            }
            "new-model" => generate::make(ctx, index, Some("model"), ""),
            "generate-for-model" => generate::for_model(ctx, index),
            _ => generate::create_missing(ctx, index),
        }
    }

    fn unused_views(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let root = project.root()?;
        let index = self.indexes.get(&root).ok_or("This project is not a Laravel application.")?;
        let _task = host::begin_task("Finding unused views", false);

        let mut sources: Vec<(&str, String)> = Vec::new();
        for glob in ["app/**/*.php", "routes/**/*.php", "resources/views/**/*.php", "database/**/*.php", "src/**/*.php"] {
            for path in project.find_files(glob, 5000).unwrap_or_default() {
                if let Ok(text) = project.read_file(&path) {
                    sources.push((if path.ends_with(".blade.php") { "blade" } else { "php" }, text));
                }
            }
        }

        let views: BTreeSet<&str> = index.views.keys().map(String::as_str).collect();
        let unused = unused::unused_views(&views, &sources);
        if unused.is_empty() {
            host::notify(NotificationLevel::Info, SOURCE, "Every view is used.");
            return Ok(None);
        }

        let choices: Vec<openidea_plugin::PickItem> = unused
            .iter()
            .map(|view| {
                let item = openidea_plugin::PickItem::new(&view.name, &view.name);
                if view.maybe { item.description("maybe used: the project builds view names at run time") } else { item }
            })
            .collect();
        let title = format!("{} unused views (choose one to open)", unused.len());
        let Some(name) = host::pick(&title, &choices)? else { return Ok(None) };

        if let Some(entry) = index.views.get(&name) {
            host::open_location(&entry.path, 0, 0)?;
        }
        Ok(None)
    }

    fn summary(&self) -> String {
        if self.indexes.is_empty() {
            return "No Laravel project is open.".into();
        }

        let lines: Vec<String> = self
            .indexes
            .iter()
            .map(|(root, i)| {
                let partial = if i.partial { " (index incomplete)" } else { "" };
                format!(
                    "{root}: {} routes, {} views, {} config keys, {} translations{partial}",
                    i.route_defs.len(),
                    i.views.len(),
                    i.config.len(),
                    i.translations.len(),
                )
            })
            .collect();
        lines.join("\n")
    }
}

impl Plugin for Main {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileSaved, EventKind::FileOpened] {
            host::subscribe(kind)?;
        }
        self.index_all();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "reindex" => {
                let _task = host::begin_task("Indexing Laravel projects", false);
                self.index_all();
                host::notify(NotificationLevel::Info, SOURCE, &self.summary());
                Ok(None)
            }
            "optimize-clear" => self.artisan(ctx, "optimize:clear"),
            "cache-clear" => self.artisan(ctx, "cache:clear"),
            "config-clear" => self.artisan(ctx, "config:clear"),
            "route-clear" => self.artisan(ctx, "route:clear"),
            "view-clear" => self.artisan(ctx, "view:clear"),
            "migrate" => self.artisan(ctx, "migrate"),
            "storage-link" => self.artisan(ctx, "storage:link"),
            "db-seed" => self.artisan(ctx, "db:seed"),
            "route-copy-curl" | "route-create-http" => {
                let Some(route) = arg.and_then(|key| self.route_at(key, ctx)) else {
                    return Ok(None);
                };
                match id {
                    "route-copy-curl" => copy_as_curl(&route, DEFAULT_BASE_URL)?,
                    _ => create_http_file(&route, DEFAULT_BASE_URL, None)?,
                }
                Ok(None)
            }
            "migrate-fresh" => {
                let root = ctx.project().and_then(|project| project.root().ok()).unwrap_or_default();
                let confirmed = self
                    .fresh_asked
                    .take()
                    .is_some_and(|(asked_root, asked)| asked_root == root && asked.elapsed() < Duration::from_secs(30));
                if !confirmed {
                    self.fresh_asked = Some((root, Instant::now()));
                    host::notify(
                        NotificationLevel::Warning,
                        SOURCE,
                        "Fresh migration drops all tables. Run Laravel: Fresh Migration (drops all tables) again within 30 seconds to confirm.",
                    );
                    return Ok(None);
                }
                self.artisan(ctx, "migrate:fresh")
            }
            "unused-views" => self.unused_views(ctx),
            "artisan-run" => generate::run_artisan(ctx),
            "make" | "new-model" | "generate-for-model" | "create-missing" => self.generator(id, ctx),
            "remove-phpdocs" => {
                let root = ctx.project().and_then(|project| project.root().ok()).unwrap_or_default();
                bulk::remove_phpdocs(ctx, self.indexes.get(&root))
            }
            "reveal" => {
                host::reveal_view(VIEW)?;
                Ok(None)
            }
            "route-list" => {
                host::open_terminal("artisan route:list", "php", &["artisan".to_owned(), "route:list".to_owned()])?;
                Ok(None)
            }
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

fn component_view_text(index: &Index, project: &Project, component: &str) -> Option<String> {
    let anonymous = index.views.get(&format!("components.{component}")).or_else(|| index.views.get(&format!("components.{component}.index")))?;
    project.read_file(&anonymous.path).ok()
}

/// Attributes and slots of the component named by the tag around the caret.
fn component_items(index: &Index, doc: &Document, offset: usize, context: &components::Context) -> Vec<CompletionItem> {
    let Some(project) = project_for(&doc.path) else { return Vec::new() };

    let (component, kind, start, slots) = match context {
        components::Context::Attribute { kind, component, start } => (component, *kind, *start, false),
        components::Context::Slot { component, start } => (component, Kind::Component, *start, true),
    };
    let replace = range_of(&doc.text, start, offset);

    let (props, view) = if kind == Kind::Livewire {
        let class = index.livewire.get(component).filter(|entry| !entry.path.is_empty()).and_then(|entry| project.read_file(&entry.path).ok());
        (class.map(|class| components::livewire_props(&class)).unwrap_or_default(), None)
    } else {
        let view = component_view_text(index, &project, component);
        let class = index.components.get(component).filter(|entry| entry.path.ends_with(".php") && !entry.path.ends_with(".blade.php")).and_then(|entry| project.read_file(&entry.path).ok());
        let props = match &class {
            Some(class) => components::constructor_props(class),
            None => view.as_deref().map(components::view_props).unwrap_or_default(),
        };
        let view = view.or_else(|| index.views.get(&format!("components.{component}")).and_then(|entry| project.read_file(&entry.path).ok()));
        (props, view)
    };

    if slots {
        return view
            .map(|view| components::slot_names(&view, &props))
            .unwrap_or_default()
            .into_iter()
            .map(|name| {
                let mut item = CompletionItem::new(&components::attribute_name(&name), CompletionKind::Property);
                item.detail = Some("slot".into());
                item.replace = Some(replace);
                item
            })
            .collect();
    }

    props
        .into_iter()
        .map(|prop| {
            let mut item = CompletionItem::new(&components::attribute_name(&prop.name), CompletionKind::Property);
            item.detail = prop.default.map(|default| format!("default {default}"));
            item.replace = Some(replace);
            item
        })
        .collect()
}

/// The class name under the caret in the providers of `config/app.php` or `bootstrap/providers.php`.
fn provider_token(path: &str, text: &str, offset: usize) -> Option<Range> {
    let whole_file = path.ends_with("bootstrap/providers.php");
    if !whole_file && !path.ends_with("config/app.php") {
        return None;
    }

    if !whole_file {
        let key = text.find("'providers'")?;
        let open = text[key..].find('[').map(|i| key + i)?;
        let close = php::closing(text, open)?;
        if offset <= open || offset > close {
            return None;
        }
    }

    let bytes = text.as_bytes();
    let class_byte = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'\\');
    let mut start = offset.min(bytes.len());
    while start > 0 && class_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset.min(bytes.len());
    while end < bytes.len() && class_byte(bytes[end]) {
        end += 1;
    }

    let before = text[..start].trim_end();
    let valid = start < end && bytes[start] != b'\\' || start + 1 < end;
    (valid && (before.ends_with('[') || before.ends_with(',')) && !text[..start].ends_with('\'')).then(|| range_of(text, start, end))
}

fn provider_items(index: &Index, doc: &Document, token: Range) -> Vec<CompletionItem> {
    let end = text::offset(&doc.text, token.end).unwrap_or(0);
    let followed = doc.text[end..].starts_with("::class");

    index
        .providers
        .keys()
        .map(|name| {
            let mut item = CompletionItem::new(name, CompletionKind::Class);
            item.replace = Some(token);
            if !followed {
                item.insert_text = format!("{name}::class");
            }
            item
        })
        .collect()
}

fn directive_items(index: &Index, replace: Range) -> Vec<CompletionItem> {
    let core = directives::CORE.iter().map(|name| (*name, None, true));
    let custom = index.directives.iter().map(|(name, entry)| (name.as_str(), entry.detail.clone(), false));

    let mut seen = BTreeSet::new();
    custom
        .chain(core)
        .filter(|(name, _, _)| seen.insert(*name))
        .map(|(name, detail, is_core)| {
            let mut item = CompletionItem::new(name, CompletionKind::Keyword);
            item.detail = detail;
            item.replace = Some(replace);
            if is_core && let Some((_, body)) = directives::SNIPPETS.iter().find(|(n, _)| *n == name) {
                item.snippet = true;
                item.insert_text = (*body).to_owned();
            }
            item
        })
        .collect()
}

impl Main {
    fn script_completion(&self, doc: &Document, at: Position) -> Vec<CompletionItem> {
        if !js::mentions_route(&doc.text) {
            return Vec::new();
        }

        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let Some(name) = js::route_name_at(&doc.text, offset) else { return Vec::new() };
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };

        let replace = range_of(&doc.text, name.start, name.end);
        index
            .routes
            .iter()
            .map(|(name, entry)| {
                let mut item = CompletionItem::new(name, CompletionKind::Reference);
                item.detail = entry.detail.clone();
                item.replace = Some(replace);
                item
            })
            .collect()
    }

    fn script_definition(&self, doc: &Document, at: Position) -> Vec<Location> {
        if !js::mentions_route(&doc.text) {
            return Vec::new();
        }

        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let Some(name) = js::route_name_at(&doc.text, offset) else { return Vec::new() };
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };

        index.routes.get(&doc.text[name]).and_then(location).into_iter().collect()
    }
}

impl Providers for Main {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if js::is_script(&doc.path, &doc.language) {
            return self.script_completion(doc, at);
        }

        let Some(language) = language_of(&doc.path, &doc.language) else { return Vec::new() };
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        if language == "php" && let Some(token) = provider_token(&doc.path, &doc.text, offset) {
            return provider_items(index, doc, token);
        }
        if language == "blade" && let Some((start, end)) = directives::name_at(&doc.text, offset) {
            return directive_items(index, range_of(&doc.text, start, end));
        }

        if language == "blade" && let Some(context) = components::context_at(&doc.text, offset) {
            return component_items(index, doc, offset, &context);
        }

        let Some(reference) = refs::reference_at(language, &doc.text, offset) else { return Vec::new() };

        let replace = range_of(&doc.text, reference.start, reference.end);
        candidates(index, doc, &reference)
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
        if js::is_script(&doc.path, &doc.language) {
            return self.script_definition(doc, at);
        }

        let Some(language) = language_of(&doc.path, &doc.language) else { return Vec::new() };
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        if language == "php" && let Some(token) = provider_token(&doc.path, &doc.text, offset) {
            let start = text::offset(&doc.text, token.start).unwrap_or(0);
            let end = text::offset(&doc.text, token.end).unwrap_or(0);
            return index.bindings.get(doc.text[start..end].trim_start_matches('\\')).and_then(location).into_iter().collect();
        }
        let Some(reference) = refs::reference_at(language, &doc.text, offset) else { return Vec::new() };

        let name = &doc.text[reference.start..reference.end];
        let names = candidates(index, doc, &reference);
        let name = if reference.kind == Kind::Binding { name.replace("\\\\", "\\") } else { name.to_owned() };
        names.get(&name).and_then(location).into_iter().collect()
    }

    /// Controller ↔ route, `view()` → view file, view → controllers and views
    /// using it, section ↔ parent section, push ↔ stack.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(language) = language_of(&doc.path, &doc.language) else { return Vec::new() };
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(project) = project_for(&doc.path) else { return Vec::new() };

        match language {
            "php" => php_markers(index, &project, doc),
            _ => blade_markers(index, &project, doc),
        }
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(language) = language_of(&doc.path, &doc.language) else { return Vec::new() };
        let Some(offset) = text::offset(&doc.text, selection.start) else { return Vec::new() };

        let mut found = Vec::new();
        if let Some(index) = self.index_for(&doc.path) {
            found.extend(missing_key_actions(index, doc, language, offset));
        }

        if language == "php" && enabled("checkEloquent") {
            let masked = php::mask_comments(&doc.text);
            found.extend(eloquent_checks::order_by(&masked).into_iter().filter(|f| f.start <= offset && offset <= f.end).filter_map(|finding| {
                Some(CodeAction {
                    title: format!("Replace with {}", finding.replacement.as_deref()?),
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: finding.replacement? }])],
                    data: None,
                })
            }));
        }

        if language == "php" && let Some((range, text)) = unused::remove_model_phpdocs(&doc.text, offset) {
            found.push(CodeAction {
                title: "Remove IDE helper PHPDoc from model".into(),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text }])],
                data: None,
            });
        }

        let controllers_namespace = host::setting_string("controllersNamespace").filter(|namespace| !namespace.is_empty()).unwrap_or_else(|| "App\\Http\\Controllers".into());
        let mut rewrites = actions::rewrites(&doc.path, language, &doc.text, offset, &controllers_namespace);
        if language == "php" && let Some(index) = self.index_for(&doc.path) {
            rewrites.extend(lang_actions::column_rewrites(index, &doc.path, &doc.text, offset));
        }
        found.extend(rewrites.into_iter().map(|rewrite| CodeAction {
            title: rewrite.title,
            kind: Some(rewrite.kind.into()),
            preferred: false,
            edits: vec![(
                doc.path.clone(),
                rewrite.edits.into_iter().map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text }).collect(),
            )],
            data: None,
        }));

        if let (Some(index), Some(project)) = (self.index_for(&doc.path), project_for(&doc.path)) {
            found.extend(lang_actions::extract_string(index, &project, doc, language, selection));
        }

        found
    }

    fn resolve_code_action(&mut self, _doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let Some(data) = action.data.clone() else { return Ok(action) };
        let (file, key) = data.split_once('\n').ok_or("Malformed code action.")?;
        let project = project_for(file).ok_or("The file is not in an open project.")?;
        let text = project.read_file(file)?;

        let edit = lang_actions::entry_edit(&text, key, key).ok_or("The lang file doesn't end its array.")?;

        action.edits = vec![(file.to_owned(), vec![edit])];
        Ok(action)
    }
}

/// Adds a missing translation key to the main locale's lang file.
fn missing_key_actions(index: &Index, doc: &Document, language: &str, offset: usize) -> Vec<CodeAction> {
    let Some(reference) = refs::reference_at(language, &doc.text, offset) else { return Vec::new() };

    let name = &doc.text[reference.start..reference.end];
    if reference.kind != Kind::Translation || is_dynamic(name) || !missing_translation(index, name) {
        return Vec::new();
    }

    let Some((file_key, key)) = name.split_once('.') else { return Vec::new() };
    if key.contains('.') {
        return Vec::new();
    }

    let files = index.translation_files.get(file_key).map(Vec::as_slice).unwrap_or_default();
    files
        .iter()
        .take(1)
        .map(|file| CodeAction {
            title: format!("Add translation key \"{name}\" to {}/{}", file.rsplit('/').nth(1).unwrap_or_default(), file_name(file)),
            kind: Some("quickfix".into()),
            preferred: false,
            edits: Vec::new(),
            data: Some(format!("{file}\n{key}")),
        })
        .collect()
}

fn php_markers(index: &Index, project: &Project, doc: &Document) -> Vec<LineMarker> {
    let mut markers = Vec::new();
    let line_of = |offset: usize| text::position(&doc.text, offset).line;

    for reference in refs::references("php", &doc.text).into_iter().filter(|r| r.closed) {
        let name = &doc.text[reference.start..reference.end];
        let (target, label) = match reference.kind {
            Kind::View => (index.views.get(name), "View"),
            Kind::Controller => (index.controllers.get(name), "Controller"),
            _ => continue,
        };

        if let Some(target) = target.and_then(location) {
            markers.push(LineMarker { line: line_of(reference.start), icon: None, tooltip: format!("{label} {name}"), target: Some(target) });
        }
    }

    let Ok(root) = project.root() else { return markers };
    let Some(inside) = doc.path.strip_prefix(&format!("{root}/{CONTROLLERS_DIRECTORY}")) else { return markers };
    let masked = php::mask_comments(&doc.text);
    let Some(controller) = controllers::parse(inside, &masked) else { return markers };

    let short = controller.class.rsplit('\\').next().unwrap_or_default();
    for (method, offset) in &controller.methods {
        let route = index.route_defs.iter().find(|r| r.def.controller.as_ref().is_some_and(|(class, m)| class == short && m == method));
        let Some(route) = route else { continue };

        let end = Position::new(route.at.line, route.at.character + route.def.width as u32);
        let tooltip = format!("Route {} {}", route.def.methods, route.def.uri);
        let target = Location { path: route.path.clone(), range: Range::new(route.at, end) };
        markers.push(LineMarker { line: line_of(*offset), icon: Some("run".into()), tooltip, target: Some(target) });
    }

    markers
}

/// Where `name` is defined by `pick` in the view's file.
fn locate(index: &Index, project: &Project, view: &str, pick: fn(&blade::Facts) -> Vec<&blade::Named>, name: &str) -> Option<Location> {
    let entry = index.views.get(view)?;
    let facts = index.blade.get(view)?;
    let (_, offset) = pick(facts).into_iter().find(|(n, _)| n == name)?;

    let text = project.read_file(&entry.path).ok()?;
    let start = text::position(&text, *offset);
    Some(Location { path: entry.path.clone(), range: Range::new(start, Position::new(start.line, start.character + name.len() as u32)) })
}

fn sections_and_yields(facts: &blade::Facts) -> Vec<&blade::Named> {
    facts.sections.iter().chain(&facts.yields).collect()
}

fn only_sections(facts: &blade::Facts) -> Vec<&blade::Named> {
    facts.sections.iter().collect()
}

fn only_stacks(facts: &blade::Facts) -> Vec<&blade::Named> {
    facts.stacks.iter().collect()
}

fn only_pushes(facts: &blade::Facts) -> Vec<&blade::Named> {
    facts.pushes.iter().collect()
}

fn blade_markers(index: &Index, project: &Project, doc: &Document) -> Vec<LineMarker> {
    let mut markers = Vec::new();
    let line_of = |offset: usize| text::position(&doc.text, offset).line;
    let mut marker = |line: u32, icon: &str, tooltip: String, target: Option<Location>| {
        markers.push(LineMarker { line, icon: Some(icon.into()), tooltip, target });
    };

    let facts = blade::facts(&doc.text);
    let view = index.view_of(&doc.path);

    if let Some(view) = view {
        let users = index.users_of(view);
        if let Some(first) = users.first().and_then(|v| index.views.get(*v)) {
            let target = Location { path: first.path.clone(), range: Range::new(Position::new(0, 0), Position::new(0, 0)) };
            marker(0, "implemented", format!("Used by {}", users.join(", ")), Some(target));
        }

        let renders: Vec<&index::Render> = index.renders.iter().filter(|r| r.view == view).collect();
        if let Some(first) = renders.first() {
            let callers: Vec<&str> = renders.iter().map(|r| r.caller.as_str()).collect();
            let target = Location { path: first.path.clone(), range: Range::new(first.at, first.at) };
            marker(0, "navigate", format!("Rendered by {}", callers.join(", ")), Some(target));
        }
    }

    let parents: Vec<&str> = facts.extends.as_deref().map(|p| std::iter::once(p).chain(index.parents(p)).collect()).unwrap_or_default();
    let descendants: Vec<&str> = match view {
        Some(view) => index.blade.keys().map(String::as_str).filter(|v| index.parents(v).contains(&view)).collect(),
        None => Vec::new(),
    };

    for (name, offset) in &facts.sections {
        let line = line_of(*offset);
        let parent = parents.iter().find_map(|p| locate(index, project, p, sections_and_yields, name));
        if let Some(parent) = parent {
            marker(line, "overriding", format!("Parent section \"{name}\""), Some(parent));
        }
    }

    for (name, offset) in facts.sections.iter().chain(&facts.yields) {
        let child = descendants.iter().find_map(|d| locate(index, project, d, only_sections, name));
        if let Some(child) = child {
            marker(line_of(*offset), "overridden", format!("Section \"{name}\" is filled by another view"), Some(child));
        }
    }

    for (name, offset) in &facts.pushes {
        let stack = parents.iter().find_map(|p| locate(index, project, p, only_stacks, name));
        if let Some(stack) = stack {
            marker(line_of(*offset), "implementing", format!("Stack \"{name}\""), Some(stack));
        }
    }

    for (name, offset) in &facts.stacks {
        let push = descendants.iter().find_map(|d| locate(index, project, d, only_pushes, name));
        if let Some(push) = push {
            marker(line_of(*offset), "implemented", format!("Stack \"{name}\" is pushed to by another view"), Some(push));
        }
    }

    markers
}

/// Tool window node keys: `group:<kind>`, `<kind>:<name>`, `route:<number>`.
const GROUPS: &[(&str, &str, &str)] = &[
    ("route", "Routes", "web"),
    ("view", "Views", "file-html"),
    ("config", "Config", "file-config"),
    ("translation", "Translations", "text"),
    ("middleware", "Middleware", "class"),
    ("env", "Env", "file-dotenv"),
];

fn group_names<'a>(index: &'a Index, group: &str) -> Option<&'a Names> {
    match group {
        "view" => Some(&index.views),
        "config" => Some(&index.config),
        "translation" => Some(&index.translations),
        "middleware" => Some(&index.middleware),
        "env" => Some(&index.env),
        _ => None,
    }
}

fn route_list_of(index: &Index) -> Vec<Route> {
    index
        .route_defs
        .iter()
        .map(|entry| {
            let def = &entry.def;
            let handler = match &def.controller {
                Some((class, method)) => Handler { class: class.clone(), member: Some(method.clone()) },
                None => Handler { class: def.action.clone(), member: None },
            };
            let end = Position::new(entry.at.line, entry.at.character + def.width as u32);
            Route {
                method: def.methods.split('|').next().unwrap_or_default().to_owned(),
                path: def.uri.clone(),
                handler,
                name: def.name.clone(),
                location: Some(Location { path: entry.path.clone(), range: Range::new(entry.at, end) }),
            }
        })
        .collect()
}

fn entry_for<'a>(index: &'a Index, key: &str) -> Option<(&'a str, Position, u32)> {
    let (group, name) = key.split_once(':')?;
    if group == "route" {
        let route = index.route_defs.get(name.parse::<usize>().ok()?)?;
        return Some((&route.path, route.at, route.def.width as u32));
    }

    let entry = group_names(index, group)?.get(name)?;
    Some((&entry.path, entry.at, entry.width))
}

impl Main {
    fn tree(&self, index: &Index) -> Vec<TreeItem> {
        let mut items = Vec::new();

        for (group, label, icon) in GROUPS {
            let key = format!("group:{group}");
            let count = group_names(index, group).map_or(index.route_defs.len(), Names::len);
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
                for (name, entry) in names {
                    let leaf = TreeItem::new(&format!("{group}:{name}"), name).parent(&key).icon(icon);
                    items.push(match &entry.detail {
                        Some(detail) if !detail.is_empty() => leaf.detail(detail),
                        _ => leaf,
                    });
                }
                continue;
            }

            for (number, route) in index.route_defs.iter().enumerate() {
                let label = format!("{} {}", route.def.methods, route.def.uri);
                let detail = match &route.def.name {
                    Some(name) => format!("{name} · {}", route.def.action),
                    None => route.def.action.clone(),
                };
                items.push(TreeItem::new(&format!("route:{number}"), &label).parent(&key).icon(icon).detail(&detail));
            }
        }

        items
    }

    fn sections(&mut self) -> &mut Switcher {
        self.sections.get_or_insert_with(|| Switcher::new("laravel.section", SECTIONS))
    }

    fn route_list(&mut self) -> &mut RouteList {
        self.route_list.get_or_insert_with(|| RouteList::new("laravel.routes", PLUGIN_ID))
    }

    fn route_at(&self, key: &str, ctx: &Invocation) -> Option<Route> {
        let number: usize = key.strip_prefix("route:")?.parse().ok()?;
        route_list_of(self.view_index(ctx)?).into_iter().nth(number)
    }

    fn view_index(&self, ctx: &Invocation) -> Option<&Index> {
        let root = ctx.project()?.root().ok()?;
        self.indexes.get(&root)
    }
}

impl Views for Main {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let reindex = Button::new("reindex", "Reindex").icon("refresh");
        let routes_active = self.sections().active() == "routes";
        let Some(index) = self.view_index(ctx) else {
            return Ok(View::message("This project is not a Laravel application.").button(reindex));
        };

        let partial = index.partial;
        let toolbar = |body| {
            View::new(body)
                .button(reindex)
                .button(Button::new("route-list", "Route List").icon("terminal"))
                .button(Button::new("optimize-clear", "Clear Caches").icon("build-project"))
        };
        let mut view = if routes_active {
            let routes = route_list_of(index);
            let list = self.route_list();
            list.set_routes(routes);
            list.render(toolbar(Body::Message(String::new())))
        } else {
            toolbar(Body::Tree(self.tree(index)))
        };
        if partial && view.status.is_none() {
            view = view.status("Index incomplete (time limit reached)");
        }
        Ok(self.sections().buttons(view, &[]))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if self.sections().handle(&event) {
            host::refresh_view(view);
            return Ok(());
        }

        if self.sections().active() == "routes" {
            match self.route_list().handle(&event) {
                RouteEvent::Changed => {
                    host::refresh_view(view);
                    return Ok(());
                }
                RouteEvent::Open(route) => {
                    if let Some(location) = route.location {
                        host::open_location(&location.path, location.range.start.line, location.range.start.character)?;
                    }
                    return Ok(());
                }
                RouteEvent::Ignored => {}
            }
        }

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
                let target = self.view_index(ctx).and_then(|index| entry_for(index, &key));
                if let Some((path, at, _)) = target.filter(|(path, _, _)| !path.is_empty()) {
                    host::open_location(path, at.line, at.character)?;
                }
            }
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
mod tests;
