//! Laravel: completion, navigation, problems, gutter markers and a tool
//! window for routes, views, config, translations, env keys, middleware,
//! controllers and assets in PHP and Blade files.
//!
//! The features follow Laravel's own documentation.

pub(crate) const VIEW_MARKER: &str = "icons/marker-view.svg";
pub(crate) const CONTROLLER_MARKER: &str = "icons/marker-controller.svg";
pub(crate) const CONFIG_DEFAULTS_MARKER: &str = "icons/marker-config-defaults.svg";
pub(crate) const USED_BY_MARKER: &str = "icons/marker-used-by.svg";
pub(crate) const RENDERED_BY_MARKER: &str = "icons/marker-rendered-by.svg";
pub(crate) const SECTION_PARENT_MARKER: &str = "icons/marker-section-parent.svg";
pub(crate) const SECTION_CHILD_MARKER: &str = "icons/marker-section-child.svg";
pub(crate) const STACK_PARENT_MARKER: &str = "icons/marker-stack-parent.svg";
pub(crate) const STACK_PUSH_MARKER: &str = "icons/marker-stack-push.svg";
pub(crate) const MIGRATION_MARKER: &str = "icons/marker-migration.svg";
pub(crate) const MODEL_MARKER: &str = "icons/marker-model.svg";
pub(crate) const MODEL_SUPPORT_MARKER: &str = "icons/marker-model-support.svg";
pub(crate) const LISTENER_MARKER: &str = "icons/marker-listener.svg";
pub(crate) const EVENT_MARKER: &str = "icons/marker-event.svg";
pub(crate) const ROUTE_MARKER: &str = "icons/marker-route.svg";
pub(crate) const RELATED_MARKER: &str = "icons/laravel.svg";

mod actions;
mod about;
mod artisan_metadata;
mod artisan_arguments;
mod artisan_workspace;
mod artisan_execution;
mod artisan_output;
mod artisan_docker;
mod artisan_runner;
mod blade;
mod blade_php;
mod bulk;
mod class_actions;
mod components;
mod config_arrays;
mod enum_config_keys;
mod config_inspections;
mod controllers;
mod console_editor;
mod console_rules;
mod directives;
mod eloquent_checks;
mod enum_where_values;
mod eloquent_members;
mod feature_test;
mod function_checks;
mod security_checks;
mod pest_completion;
mod php_snippets;
mod socialite_completion;
mod tool_config;
mod upgrade_checks;
mod generate;
mod generator_plan;
mod generator_presets;
mod generator_preset_actions;
mod generator_workspace;
mod existing_model_fields;
mod vite;
mod hover;
mod view_creation;
mod view_extract;
mod request_creation;
mod request_promotion;
mod source_matches;
mod semantic_refs;
mod strict_json;
mod index;
mod index_queue;
mod indexer;
mod js;
mod lang_actions;
mod live;
mod migrations;
mod model_overview;
mod model_actions;
pub(crate) use model_actions::parameters as model_helper_parameters;
mod helper_automation;
mod native_module_scaffold;
mod native_catalog;
mod class_component_rename;
mod surround_actions;
mod package_generators;
mod ui_text;
mod wayfinder_destinations;
mod request_dto;
mod model_from_schema;
mod vendor_overrides;
mod metadata;
mod validation_productivity;
mod validation_requests;
mod route_completion;
mod route_attributes;
mod route_targets;
mod route_inspections;
mod managed_processes;
mod database_workspace;
mod schema_compare;
mod package_facts;
mod package_integrations;
mod member_refactor;
mod project_wizard;
mod php;
mod refs;
mod routes;
mod route_runtime;
mod route_request;
mod route_search;
mod view_rename;
mod route_editor;
mod runtime_inspection;
mod mcp;
mod unused;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Diagnostic, Event, EventKind, Location, Position, Range, Severity, TextEdit};
use openidea_plugin::{Error, Invocation, NotificationLevel, Plugin, Project, host, text};
use ui_kit::routes::{Handler, Route, RouteEvent, RouteList, copy_as_curl, create_http_file_for_project};

pub use index::Index;
use index::{AddName, Entry, Lines, Names};
use refs::{Kind, Reference};

const VIEW: &str = "laravel";
const PLUGIN_ID: &str = "aritti.laravel";
const DEFAULT_BASE_URL: &str = "http://localhost:8000";
const SOURCE: &str = "Laravel";
const CONTROLLERS_DIRECTORY: &str = "app/Http/Controllers/";

pub(crate) fn still_indexing() -> Error {
    Error::Invalid(ui_text::t("native.lib.stillIndexing", "Laravel is still indexing. Try again in a moment."))
}

/// Whether a saved file (project-relative) feeds the index.
fn affects_index(relative: &str) -> bool {
    if matches!(relative, "ide-blade.json" | ".idea/ide-blade.json" | "ide.json" | ".idea/ide.json")
        || relative.starts_with(".openidea/laravel-")
        || relative.starts_with("database/schema/")
        || relative.starts_with("public/build/")
        || relative.starts_with("vite.config.") {
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
    (relative.starts_with("app/") && relative.ends_with(".php"))
        || (relative.starts_with("database/seeders/") && relative.ends_with(".php"))
        || directory.iter().any(|d| relative.starts_with(d))
        || matches!(
            relative,
            "composer.json" | "composer.lock" | ".env" | ".env.example" | "bootstrap/app.php" | "app/Http/Kernel.php" | "app/Http/routes.php"
        )
}

fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn project_for(path: &str) -> Option<Project> {
    host::projects()
        .ok()?
        .into_iter()
        .filter(|p| p.root().is_ok_and(|root| under(path, &root)))
        .max_by_key(|p| p.root().map_or(0, |r| r.len()))
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

/// Current bounded source facts, not saved offsets projected onto a dirty file.
/// Retain current innermost ownership even when a cached view path still exists.
fn current_blade_view<'a>(index: &'a Index, project: &Project, view: &str) -> Option<(&'a Entry, String, blade::Facts)> {
    let entry = index.views.get(view)?;
    let root = project.root().ok()?;
    if host::project_of(&entry.path).and_then(|owner| owner.root().ok()).as_deref() != Some(root.as_str()) { return None; }
    let metadata = project.stat(&entry.path).ok()?;
    if metadata.kind != openidea_plugin::FileKind::File || metadata.size > 128 * 1024 { return None; }
    let source = project.read_file(&entry.path).ok().filter(|source| source.len() <= 128 * 1024)?;
    let facts = blade::facts(&source);
    Some((entry, source, facts))
}

fn owns_current_path(project: &Project, path: &str) -> bool {
    let Ok(root) = project.root() else { return false; };
    host::project_of(path).and_then(|owner| owner.root().ok()).as_deref() == Some(root.as_str())
}
fn scoped_location(project: &Project, entry: &Entry) -> Option<Location> {
    if !owns_current_path(project, &entry.path) { return None; }
    location(entry)
}

/// Sections, yields and stacks that the layouts above a view define.
fn layout_names(index: &Index, project: &Project, doc: &Document, kind: Kind) -> Names {
    let mut names = Names::new();
    let facts=blade::facts(&doc.text);
    let Some(parent) = index.layout_parent(&facts) else {
        return names;
    };

    for view in std::iter::once(parent).chain(index.parents(parent)) {
        let Some((entry, text, facts)) = current_blade_view(index, project, view) else { continue; };
        let lines = Lines::new(&text);

        let found: Vec<&blade::Named> = match kind {
            Kind::Stack => facts.stacks.iter().collect(),
            _ => facts.sections.iter().chain(&facts.yields).collect(),
        };
        for (name, offset) in found {
            names.add(name, Some(view.to_owned()), &entry.path, lines.position(*offset), lines.width(*offset, name.len()));
        }
    }

    names
}

/// `Class@method` of every controller.
fn actions(index: &Index) -> Names {
    let mut names = Names::new();
    for (class, methods) in &index.methods {
        for (method, entry) in methods {
            if method == "__invoke" {names.insert(class.clone(),entry.clone());}
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
            if let Some(project)=project_for(&doc.path) {
                if let Some(names)=eloquent_members::semantic::column_names(index,doc,reference.start,&|path|owns_current_path(&project,path)) {
                    return Cow::Owned(names);
                }
            }
            let table = match reference.class.as_deref() {
                Some(table) if table.starts_with("table:") => Some(table["table:".len()..].to_owned()),
                Some(spec) => resolve_model(index, spec, &doc.path)
                    .map(|class| index.models.get(&class).cloned().unwrap_or_else(|| index::table_name(&class))),
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
    let mut model = if first.is_empty() { file_name(path).strip_suffix(".php")?.to_owned() } else { first.to_owned() };

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

    let references=refs::references(language,text);
    let config_bindings=config_inspections::prepare(index,language,text,references.len());
    for reference in references.into_iter().filter(|r|r.closed) {
        let name = &text[reference.start..reference.end];
        if is_dynamic(name) {
            continue;
        }

        match reference.kind {
            Kind::View if !name.contains("::") && !index.views.is_empty() && !index.views.contains_key(name) && !refs::optional_view_fallback(language,text,reference.start) => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "missing-view",
                    Severity::Warning,
                    crate::ui_text::format("root.formatted.9efde13429", "View \"{name}\" does not exist.", &[("name", &format!("{}", name))]),
                ));
            }
            Kind::Config if missing_config(index, name)&&config_bindings.as_ref().is_some_and(|context|context.safe(index,&reference)) => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "missing-config",
                    Severity::Info,
                    crate::ui_text::format("root.formatted.12ba96c93a", "Config key \"{name}\" is not defined.", &[("name", &format!("{}", name))]),
                ));
            }
            Kind::Translation if missing_translation(index, name) => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "missing-translation",
                    Severity::Info,
                    crate::ui_text::format("root.formatted.4e520e6ec5", "Translation key \"{name}\" is missing.", &[("name", &format!("{}", name))]),
                ));
            }
            Kind::Translation if enabled("missingTranslationInspection") && !incomplete_locales(index, name).is_empty() => {
                let locales = incomplete_locales(index, name).join(", ");
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "translation-missing-in-locale",
                    Severity::Info,
                    crate::ui_text::format("root.formatted.missingInLocale", "Translation key \"{name}\" is missing in: {locales}.", &[("name", name), ("locales", &locales)]),
                ));
            }
            Kind::Route if check_routes && !index.routes.contains_key(name) => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "missing-route",
                    Severity::Warning,
                    crate::ui_text::format("root.formatted.eb896af3a9", "Route \"{name}\" is not defined.", &[("name", &format!("{}", name))]),
                ));
            }
            Kind::Inertia if sure && unknown_inertia_page(index, name) && enabled("reportUnknownInertiaPages") => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "laravel.unknown-inertia-page",
                    Severity::Warning,
                    crate::ui_text::format("root.formatted.d56867e612", "Inertia page \"{name}\" was not found in configured page roots.", &[("name", &format!("{}", name))]),
                ));
            }
            Kind::Livewire if sure && unknown_livewire(index, name) && enabled("reportUnknownLivewireDirectives") => {
                found.push(problem(
                    text,
                    reference.start..reference.end,
                    "laravel.unknown-livewire-directive",
                    Severity::Warning,
                    crate::ui_text::format("root.formatted.bb6eb44ee8", "Livewire component \"{name}\" does not exist.", &[("name", &format!("{}", name))]),
                ));
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
                    found.push(problem(
                        text,
                        range,
                        "laravel.unknown-component",
                        Severity::Warning,
                        crate::ui_text::format("root.formatted.d3688f4de8", "Blade component \"{name}\" does not exist.", &[("name", &format!("{}", name))]),
                    ));
                }
                Kind::Livewire if unknown_livewire(index, name) && enabled("reportUnknownLivewire") => {
                    found.push(problem(
                        text,
                        range,
                        "laravel.unknown-livewire",
                        Severity::Warning,
                        crate::ui_text::format("root.formatted.bb6eb44ee8", "Livewire component \"{name}\" does not exist.", &[("name", &format!("{}", name))]),
                    ));
                }
                _ => {}
            }
        }
    }

    if language == "php" && sure && enabled("reportUnknownFactories") {
        for range in refs::factory_classes(text) {
            let class = text[range.clone()].rsplit('\\').next().unwrap_or_default();
            if unknown_factory(index, class) {
                found.push(problem(text, range, "laravel.unknown-factory", Severity::Warning, crate::ui_text::format("root.formatted.1bb16b8baf", "Model {class} has no factory.", &[("class", &format!("{}", class))])));
            }
        }
    }

    found.sort_by_key(|d| (d.range.start.line, d.range.start.character));
    found
}

/// `db:seed` runs inside `Model::unguarded()`, so mass assignment there is not discarded.
const UNGUARDED_DIRECTORIES: &[&str] = &["/database/seeders/", "/database/seeds/", "/database/factories/"];

/// Mass assignment, prunable and `orderBy` problems of a PHP file.
fn eloquent_problems(project: &Project, index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if language != "php" || !enabled("checkEloquent") {
        return Vec::new();
    }

    let Ok(root) = project.root() else {
        return Vec::new();
    };
    let read = |relative: &str| project.read_file(&format!("{root}/{relative}")).ok();

    let mut found: Vec<_> = eloquent_checks::source_findings(text, &read)
        .into_iter()
        .map(|finding| {let message=eloquent_checks::localized_message(&finding);problem(text, finding.start..finding.end, finding.code, finding.severity, message)})
        .collect();
    let doc = Document { path: path.into(), language: language.into(), version: 0, text: text.into() };
    found.extend(eloquent_mass_problems(project, index, &doc));
    found.extend(enum_value_problems(project, index, &doc));
    found
}

fn enum_value_problems(project: &Project, index: &Index, doc: &Document) -> Vec<Diagnostic> {
    if !enabled("enumValueInspection") {
        return Vec::new();
    }
    enum_where_values::findings(index, doc, &|path| owns_current_path(project, path))
        .into_iter()
        .map(|finding| problem(&doc.text, finding.start..finding.end, finding.code, finding.severity, finding.message))
        .collect()
}

fn eloquent_mass_problems(project: &Project, index: &Index, doc: &Document) -> Vec<Diagnostic> {
    if doc.language != "php" || !enabled("checkEloquent") || UNGUARDED_DIRECTORIES.iter().any(|part| doc.path.contains(part)) { return Vec::new(); }
    eloquent_checks::mass_assignment_checked(index, doc, &|path| owns_current_path(project,path),
        &|path| { let metadata=project.stat(path).ok()?; if metadata.kind != openidea_plugin::FileKind::File || metadata.size > 128*1024 {return None;} project.read_file(path).ok().filter(|source| source.len() <= 128*1024) }, &host::setting_string("massAssignmentExceptions").or_else(||host::setting("massAssignmentExceptions")).unwrap_or_else(|| "{}".into()))
        .into_iter().map(|finding| problem(&doc.text, finding.start..finding.end, finding.code, finding.severity, finding.message)).collect()
}

/// `env()` calls outside the configuration files.
fn function_problems(project: &Project, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if !enabled("checkFunctions") {
        return Vec::new();
    }

    let Ok(root) = project.root() else {
        return Vec::new();
    };
    let relative = path.strip_prefix(&format!("{root}/")).unwrap_or(path);

    let security = if enabled("checkSecurity") { security_checks::findings(relative, language, text) } else { Vec::new() };

    function_checks::env_outside_config(relative, language, text)
        .into_iter()
        .chain(security)
        .map(|finding|{let message=if finding.code=="env-outside-config"{crate::ui_text::t("editor.final.envOutsideConfig",&finding.message)}else{finding.message};problem(text,finding.start..finding.end,finding.code,finding.severity,message)})
        .collect()
}

fn pest_items(doc: &Document, offset: usize) -> Option<Vec<CompletionItem>> {
    let (kind, start, end) = pest_completion::context(&doc.text, offset)?;
    let project = project_for(&doc.path)?;
    let replace = range_of(&doc.text, start, end);

    Some(pest_completion::names(&project, kind).into_iter().map(|name| {
        let mut item = CompletionItem::new(&name, CompletionKind::Reference);
        item.replace = Some(replace);
        item
    }).collect())
}

fn upgrade_problems(index: &Index, language: &str, text: &str) -> Vec<Diagnostic> {
    if !enabled("upgradeInspection") {
        return Vec::new();
    }

    let installed = index.metadata.framework_version.map(|(major, _, _)| major);

    upgrade_checks::findings(installed, language, text)
        .into_iter()
        .map(|finding| problem(text, finding.start..finding.end, finding.code, finding.severity, finding.message))
        .collect()
}

/// `wire:click="method"` naming a method the view's own component class does not have.
fn livewire_method_problems(project: &Project, index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if language != "blade" || index.partial || !enabled("reportUnknownLivewireMethods") {
        return Vec::new();
    }

    let Some(methods)=package_integrations::livewire_methods_for_view(index,project,path,text)else{return Vec::new();};

    blade::wire_calls(text)
        .into_iter()
        .filter(|call| !methods.iter().any(|method|method.eq_ignore_ascii_case(&call.name)))
        .map(|call| {
            let message = crate::ui_text::format("root.formatted.24a37877b0", "Component class has no method \"{method}\".", &[("method", &format!("{}", call.name))]);
            problem(text, call.start..call.end, "laravel.unknown-livewire-method", Severity::Warning, message)
        })
        .collect()
}

fn controllers_namespace() -> String {
    host::setting_string("controllersNamespace")
        .filter(|namespace| !namespace.is_empty())
        .unwrap_or_else(|| "App\\Http\\Controllers".into())
}

/// Controllers and methods that `routes/*.php` name and the project lacks.
fn route_controller_problems(_project:&Project,index:&Index,path:&str,language:&str,text:&str)->Vec<Diagnostic>{route_inspections::current(index,&Document{path:path.into(),language:language.into(),text:text.into(),version:0})}

fn enabled(setting: &str) -> bool {
    host::setting_bool(setting).unwrap_or(true)
}

fn unknown_component(index: &Index, name: &str) -> bool {
    !index.open_components && !name.contains("::") && !matches!(name, "slot" | "dynamic-component") && !index.components.contains_key(name)
}

fn unknown_livewire(index: &Index, name: &str) -> bool {
    !index.open_livewire
        && !name.contains("::")
        && !matches!(name, "is" | "styles" | "scripts" | "dynamic-component")
        && !index.livewire.contains_key(name)
        && !index.views.contains_key(&format!("livewire.{name}"))
}

fn unknown_inertia_page(index: &Index, name: &str) -> bool {
    (!index.inertia_pages.is_empty() || !index.metadata.inertia_paths.is_empty()) && !name.contains("::") && !name.ends_with('/') && !index.inertia_pages.contains_key(name)
}

fn unknown_factory(index: &Index, class: &str) -> bool {
    !matches!(class, "static" | "self" | "parent")
        && index.models.contains_key(class)
        && !index.factories.contains(&format!("{class}Factory"))
        && !index.factory_overrides.contains(class)
}

/// A key below an array (or config file) of the project that doesn't have it.
fn missing_config(index:&Index,name:&str)->bool {config_inspections::missing(index,name)}

/// `file.key` where the project has a lang file for `file` but not the key.
fn missing_translation(index: &Index, name: &str) -> bool {
    if name.contains("::") || index.translations.contains_key(name) {
        return false;
    }

    let Some((file, _)) = name.split_once('.') else {
        return false;
    };
    index.translation_files.contains_key(file)
}

const SEARCH_LIMIT: usize = 5000;

/// The Models row key a model command was given, or one the user picks when it runs from the palette.
fn model_key_or_pick(index: &Index, project: &Project, arg: Option<&str>, title: &str) -> Result<Option<String>, Error> {
    if let Some(key) = arg { return Ok(Some(key.to_owned())); }
    let choices: Vec<_> = model_overview::rows(index, project).into_iter().filter(|row|row["cells"][1] == "Model").filter_map(|row|Some(openidea_plugin::PickItem::new(row["key"].as_str()?, row["cells"][0].as_str()?))).collect();
    if choices.is_empty() { return Err(Error::Invalid(crate::ui_text::t("native.lib.noModels", "No Eloquent models found in this project."))); }
    host::pick(title, &choices)
}

/// One picker over every route name, view, component, config key, translation and env key.
fn search_names(index: &Index) -> Result<(), Error> {
    let sections: [(&str, &Names); 6] = [
        ("route", &index.routes),
        ("view", &index.views),
        ("component", &index.components),
        ("config", &index.config),
        ("translation", &index.translations),
        ("env", &index.env),
    ];

    let choices: Vec<_> = sections
        .iter()
        .flat_map(|(kind, names)| names.iter().map(move |(name, entry)| (kind, name, entry)))
        .filter(|(_, _, entry)| !entry.path.is_empty())
        .take(SEARCH_LIMIT)
        .map(|(kind, name, _)| openidea_plugin::PickItem::new(&format!("{kind}\u{1f}{name}"), name).description(kind))
        .collect();

    let Some(chosen) = host::pick(crate::ui_text::text("search.names", "Search routes, views, config, translations and env keys"), &choices)? else {
        return Ok(());
    };
    let Some((kind, name)) = chosen.split_once('\u{1f}') else {
        return Ok(());
    };
    let Some((_, names)) = sections.iter().find(|(section, _)| *section == kind) else {
        return Ok(());
    };

    if let Some(entry) = names.get(name) {
        host::open_location(&entry.path, entry.at.line, entry.at.character)?;
    }
    Ok(())
}

/// Locales that lack `name` although another locale has it.
fn incomplete_locales(index: &Index, name: &str) -> Vec<String> {
    if name.contains("::") || !index.translations.contains_key(name) || index.translations_by_locale.len() < 2 {
        return Vec::new();
    }

    index.translations_by_locale.iter().filter(|(_, names)| !names.contains_key(name)).map(|(locale, _)| locale.clone()).collect()
}

#[derive(Default)]
pub struct Main {
    artisan_terminals: generate::Terminals,
    application_mcp: mcp::application::State,
    helper_automation: helper_automation::State,
    view_extractions:view_extract::State,
    request_creations:request_creation::State,
    request_promotions:request_promotion::State,
    /// By project root.
    indexes: BTreeMap<String, Index>,
    /// `php` version by project root; `None` when `php` could not run.
    php_versions: std::cell::RefCell<BTreeMap<String, Option<String>>>,
    pending_indexes: BTreeSet<String>,
    index_builders: BTreeMap<String, index::Builder>,
    index_progress: BTreeMap<String, (usize, usize)>,
    index_rechecks: BTreeMap<String, std::collections::VecDeque<String>>,
    /// Files with published problems, checked again when the index changes.
    checked: BTreeSet<String>,
    route_list: Option<RouteList>,
    runtime_routes: BTreeMap<String, route_runtime::Snapshot>,
    artisan_workspace: artisan_workspace::State,
    generator_workspace: generator_workspace::State,
    project_wizard: project_wizard::State,
    managed_processes: managed_processes::State,
    database_workspace: database_workspace::State,
    route_requests: route_request::State,
    model_tree_expanded: BTreeMap<String, BTreeMap<String, bool>>,
    runtime_inspection: runtime_inspection::State,
    index_ticks: u32,
    model_rows: std::cell::RefCell<BTreeMap<String, Vec<serde_json::Value>>>,
    /// Whether the Run console colours are set.
    console_rules: bool,
}

/// First `KEY=value` of the project's `.env`, unquoted; `None` when unset or unreadable.
fn dotenv_value(project: &Project, key: &str) -> Option<String> {
    let text = project.read_file(".env").ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
        .map(|value| value.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|value| !value.is_empty())
}

/// Confirmation body for a command that changes the project's database.
fn database_command_prompt(project: &Project, body_key: &str, body: &str) -> Result<String, Error> {
    let root = project.root()?;
    let mut prompt = crate::ui_text::format(body_key, body, &[("root", &root)]);
    let connection = dotenv_value(project, "DB_CONNECTION");
    let environment = dotenv_value(project, "APP_ENV");
    if connection.is_some() || environment.is_some() {
        let unknown = crate::ui_text::text("native.lib.unknownEnvValue", "not set");
        prompt.push_str(&crate::ui_text::format(
            "native.lib.databaseTarget",
            "\nDatabase connection: {connection}. Environment: {environment}.",
            &[("connection", connection.as_deref().unwrap_or(unknown)), ("environment", environment.as_deref().unwrap_or(unknown))],
        ));
    }
    Ok(prompt)
}

fn confirm_database_command(ctx: &Invocation, title: &str, body_key: &str, body: &str) -> Result<bool, Error> {
    let project = ctx.project().ok_or(crate::ui_text::text("native.lib.6d0169b3b4", "No project is open."))?;
    host::confirm(title, &database_command_prompt(&project, body_key, body)?)
}

fn refresh_laravel() {
    host::refresh_view(VIEW);
    let _ = host::post_view_message(VIEW, r#"{"channel":"ui-kit-refresh"}"#);
}

impl Main {
    /// The hub's background poll wraps the tick in an opaque-message envelope that the
    /// grouped-view guard can reject; indexing must not depend on that envelope's tab state.
    fn is_index_tick_envelope(request: &serde_json::Value) -> bool {
        request["op"] == "opaque-message"
            && request["message"]
                .as_str()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .is_some_and(|inner| inner["op"] == "index-tick")
    }
    fn index_project(&mut self, project: &Project) {
        self.queue_index(project);
        self.advance_index(project);
    }

    fn index_all(&mut self) {
        self.queue_indexes();
    }

    fn index_for(&self, path: &str) -> Option<&Index> {
        // Cached ancestry is not current ownership. A newly opened nested app
        // may have no cache yet; a closed/denied scope must not expose its old
        // inventory. Unknown ownership withholds results, never deletes caches.
        let root = project_for(path)?.root().ok()?;
        self.indexes.get(&root)
    }

    /// Publishes the problems of a file (none when they are switched off).
    fn check(&mut self, project: &Project, path: &str) {
        let Some(language) = language_of(path, "") else {
            return;
        };
        let Some(index) = self.index_for(path) else {
            return;
        };

        let enabled = host::setting_bool("diagnostics").unwrap_or(true);
        let found = match project.read_file(path) {
            Ok(text) if enabled && !index.partial => {
                // Cheap cached-inventory checks belong exclusively to DiagnosticsProvider.
                let mut found = Vec::new();
                found.extend(eloquent_problems(project, index, path, language, &text));
                found.extend(function_problems(project, path, language, &text));
                found.extend(upgrade_problems(index, language, &text));
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
        let project = ctx.project().ok_or(crate::ui_text::text("native.lib.6d0169b3b4", "No project is open."))?;
        let task = host::begin_task(&crate::ui_text::format("root.formatted.ae05baa7cc", "artisan {command}", &[("command", &format!("{}", command))]), false);
        let args = ["artisan".to_owned(), command.to_owned(), "--no-interaction".to_owned(), "--no-ansi".to_owned()];

        let output = match project.run_process("php", &args, None) {
            Err(Error::Denied(_)) => {
                return Err("Running PHP needs the process:php permission. Allow it in Settings › Plugins › Laravel, and make sure php is on your PATH.".into());
            }
            result => result?,
        };

        if output.status != Some(0) {
            return Err(artisan_output::failure(command, output.status, &output.stdout, &output.stderr).into());
        }

        task.report(1, Some(2), Some("Indexing"));
        if let Ok(root)=project.root(){self.runtime_routes.remove(&root);}
        self.index_project(&project);
        host::notify(NotificationLevel::Info, SOURCE, &crate::ui_text::format("index.artisanQueued", "Ran artisan {command}. Project indexing is queued.", &[("command", &format!("{}", command))]));
        Ok(None)
    }

    fn generator(&mut self, id: &str, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or(crate::ui_text::text("native.lib.6d0169b3b4", "No project is open."))?;
        let root = project.root()?;
        if id == "new-model" {
            ui_kit::web::reveal_view(generator_workspace::VIEW, include_str!("ui_config.json"))?;
            return Ok(None);
        }
        if id == "make" && self.artisan_workspace.open_generator(ctx, None)? {
            return Ok(None);
        }
        let index = self.indexes.get(&root);

        match id {
            "make" => {
                let mut composer = project.read_file("composer.json").unwrap_or_default();
                if index.is_some_and(|i| !i.livewire.is_empty()) {
                    composer.push_str("\"livewire/livewire\"");
                }
                generate::make(&self.artisan_terminals, ctx, index, None, &composer)
            }
            "new-model" => generate::make(&self.artisan_terminals, ctx, index, Some("model"), ""),
            "generate-for-model" => {
                let runner=self.artisan_workspace.runner(ctx)?;
                let runtime=self.runtime_inspection.schema_for_generation(&project,runner).ok().map(|snapshot|(runner,snapshot));
                let before=bulk::pending_generation(&self.artisan_terminals.model_plans,&root);
                let result=generate::for_model(&self.artisan_terminals, ctx, index, runtime)?;
                let Some((key,anchor))=bulk::pending_generation(&self.artisan_terminals.model_plans,&root).filter(|after|before.as_ref()!=Some(after)) else {return Ok(result)};
                if let Some(count)=self.apply_editor_action(&anchor,&format!("bulk-generate:{key}:"))? {
                    host::notify(NotificationLevel::Info,SOURCE,&crate::ui_text::format("native.lib.filesWritten","Created or updated {count} files.",&[("count",&count.to_string())]));
                }
                Ok(None)
            },
            _ => generate::create_missing(&self.artisan_terminals, ctx, index),
        }
    }

    fn unused_views(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or(crate::ui_text::text("native.lib.6d0169b3b4", "No project is open."))?;
        let root = project.root()?;
        let index = self.indexes.get(&root).ok_or(crate::ui_text::text("native.lib.46db8a1248", "This project is not a Laravel application."))?;
        let _task = host::begin_task(crate::ui_text::text("root.findingUnused", "Finding unused views"), false);

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
            host::notify(NotificationLevel::Info, SOURCE, crate::ui_text::text("native.lib.6fe44f17e2", "Every view is used."));
            return Ok(None);
        }

        let choices: Vec<openidea_plugin::PickItem> = unused
            .iter()
            .map(|view| {
                let item = openidea_plugin::PickItem::new(&view.name, &view.name);
                if view.maybe { item.description(crate::ui_text::text("final.views.maybeUsed","maybe used: the project builds view names at run time")) } else { item }
            })
            .collect();
        let title = crate::ui_text::format("root.formatted.f688338d77", "{count} unused views (choose one to open)", &[("count", &format!("{}", unused.len()))]);
        let Some(name) = host::pick(&title, &choices)? else {
            return Ok(None);
        };

        if let Some(entry) = index.views.get(&name) {
            host::open_location(&entry.path, 0, 0)?;
        }
        Ok(None)
    }

    fn summary(&self) -> String {
        if !self.pending_indexes.is_empty() { return crate::ui_text::text("index.building", "Indexing Laravel project…").into(); }
        if self.indexes.is_empty() {
            return crate::ui_text::text("root.formatted.f34af16af5", "No Laravel project is open.").into();
        }

        let lines: Vec<String> = self
            .indexes
            .iter()
            .map(|(root, i)| {
                let partial = if i.partial { crate::ui_text::text("root.formatted.159f2f9c2e", " (index incomplete)") } else { "" };
                crate::ui_text::format("root.formatted.8f788d0935", "{root}: {routes} routes, {views} views, {config} config keys, {translations} translations{partial}", &[("root", &format!("{}", root)), ("routes", &format!("{}", i.route_defs.len())), ("views", &format!("{}", i.views.len())), ("config", &format!("{}", i.config.len())), ("translations", &format!("{}", i.translations.len())), ("partial", &format!("{}", partial))])
            })
            .collect();
        lines.join("\n")
    }
}

impl Main {
    /// The Laravel version of the project that owns `path`, and the PHP that runs it, in the status bar.
    fn show_version_status(&self, path: &str) {
        let project = project_for(path);
        let root = project.as_ref().and_then(|project| project.root().ok());
        let version = root.as_ref().and_then(|root| self.indexes.get(root)).and_then(|index| index.metadata.framework_version);

        let (Some((major, minor, _)), Some(project), Some(root)) = (version, project, root) else {
            host::remove_status_item("laravel-version");
            return;
        };

        let php = self.php_versions.borrow_mut().entry(root).or_insert_with(|| {
            let output = project.run_process("php", &["-r".to_owned(), "echo PHP_VERSION;".to_owned()], None).ok()?;
            (output.status == Some(0)).then(|| output.stdout.trim().to_owned()).filter(|version| !version.is_empty())
        }).clone();
        let text = match php {
            Some(php) => format!("Laravel {major}.{minor} · PHP {php}"),
            None => format!("Laravel {major}.{minor}"),
        };

        let item = openidea_plugin::StatusItem { text, tooltip: None, command: None };
        if host::set_status_item_menu("laravel-version", &item, &status_menu()).is_err() {
            host::set_status_item("laravel-version", &item);
        }
    }
}

/// The Laravel status bar item's menu: the commands used most while working on the project.
fn status_menu() -> Vec<openidea_plugin::StatusMenuEntry> {
    use openidea_plugin::StatusMenuEntry;
    let action = |key: &str, label: &'static str, command: &str| StatusMenuEntry::action(crate::ui_text::text(key, label), command);
    vec![
        action("statusMenu.artisanRun", "Run Artisan Command…", "artisan-run"),
        action("statusMenu.optimizeClear", "Clear All Caches", "optimize-clear"),
        StatusMenuEntry::Separator,
        action("statusMenu.routeSearch", "Search Routes…", "route-search"),
        action("statusMenu.sendRequest", "Send Request…", "route-send-request"),
        StatusMenuEntry::Separator,
        action("statusMenu.reveal", "Show Laravel Tool Window", "reveal"),
        action("statusMenu.reindex", "Reindex Project", "reindex"),
    ]
}

impl Plugin for Main {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileSaved, EventKind::FileOpened, EventKind::TerminalExited, EventKind::Scheduled, EventKind::SettingsChanged, EventKind::VcsChanged] {
            host::subscribe(kind)?;
        }
        host::subscribe(EventKind::EditorFocused)?;
        host::subscribe_paths("files-changed", &["**/*.php", "**/*.json", "**/*.sql", "**/*.js", "**/*.ts", "**/*.mjs", "**/*.mts", ".env*", "resources/**/*"])?;
        self.index_all();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "new-project" => self.project_wizard.run(ctx),
            "project-installation-recovery" => self.project_wizard.recover(ctx),
            "runtime-schema" => {
                self.runtime_inspection.capture_selected_schema(ctx,self.artisan_workspace.runner(ctx)?)?;
                refresh_laravel();
                Ok(None)
            }
            "runtime-register-database" => {
                self.runtime_inspection.register_database_source(ctx, self.artisan_workspace.runner(ctx)?)?;
                Ok(None)
            }
            "helper-source-automation" => {model_actions::parameters::configure_advanced(ctx)?;refresh_laravel();Ok(None)}
            "helper-automation-state" | "helper-automation-poll" | "helper-automation-stop" => {
                let runner=self.artisan_workspace.runner(ctx)?;
                let value=self.helper_automation.handle(ctx,&serde_json::json!({"op":id}).to_string(),runner)?;
                host::output_clear("Laravel Helper Automation")?;
                host::output_append("Laravel Helper Automation",&helper_automation::summary(&value))?;
                host::output_reveal("Laravel Helper Automation")?;Ok(None)
            }
            "index-issues" => {
                let index=self.view_index(ctx).ok_or(Error::Stale)?;
                host::output_clear("Laravel Index Issues")?;
                host::output_append("Laravel Index Issues",&index.metadata.issues.iter().map(|issue|metadata::presentation_issue(index,issue)).collect::<Vec<_>>().join("\n"))?;
                host::output_reveal("Laravel Index Issues")?;Ok(None)
            }
            "runtime-application-mcp" => {
                let extra=self.view_index(ctx).map(|index|index.metadata.route_files.clone()).unwrap_or_default();let value=self.application_mcp.connect(ctx,self.artisan_workspace.runner(ctx)?,&extra)?;
                host::notify(NotificationLevel::Info,SOURCE,&mcp::application::summary(&value));refresh_laravel();Ok(None)
            }
            "runtime-model-connections" => {
                let project=ctx.project().ok_or(Error::Stale)?;
                self.advance_index(&project);
                let index=self.view_index(ctx).ok_or_else(still_indexing)?;
                if !index::index_sources_current(&project,index){return Err(still_indexing());}
                let models=eloquent_members::model_identities(&index.eloquent_models).into_iter().filter(|(_,path)|owns_current_path(&project,path)).collect::<Vec<_>>();
                let items=models.iter().map(|(full,_)|openidea_plugin::PickItem::new(full,full)).collect::<Vec<_>>();
                let Some(selected)=host::pick_many(&crate::ui_text::t("native.lib.modelConnectionsPick", "Models to read the database connection and table of (up to 32)"),&items)?else{return Ok(None)};
                if selected.is_empty()||selected.len()>32||selected.iter().any(|name|!models.iter().any(|(full,_)|full==name)){return Err(Error::Invalid(crate::ui_text::t("native.lib.pickModels", "Select between 1 and 32 models of this project.")));}
                let models=models.into_iter().filter(|(full,_)|selected.contains(full)).collect::<Vec<_>>();
                self.runtime_inspection.capture_model_connections(ctx,self.artisan_workspace.runner(ctx)?,&models)?;refresh_laravel();Ok(None)
            }
            "helper-parameters" => {
                model_actions::configure(ctx)?;
                refresh_laravel();
                Ok(None)
            }
            "regenerate-model-helpers" => {
                self.advance_context_index(ctx);
                let index=self.view_index(ctx).ok_or_else(still_indexing)?;
                let anchor=model_actions::helper_anchor(ctx,index)?;
                if let Some(count)=self.apply_editor_action(&anchor,"model-native:helpers-all:")? {
                    host::notify(NotificationLevel::Info,SOURCE,&crate::ui_text::format("native.lib.helpersWritten","Updated {count} model helper files.",&[("count",&count.to_string())]));
                }
                Ok(None)
            }
            "model-from-database" => {
                let schema = self.runtime_inspection.schema_snapshot(ctx, self.artisan_workspace.runner(ctx)?)?;
                if let Some(draft) = model_from_schema::run(ctx, &schema)? {
                    self.generator_workspace.seed_model(ctx, draft)?;
                }
                Ok(None)
            }
            "model-open-source" => {
                let project = ctx.project().ok_or(Error::Stale)?;
                let index = self.view_index(ctx).ok_or_else(still_indexing)?;
                let Some(key) = model_key_or_pick(index, &project, arg, &crate::ui_text::t("native.lib.openModelPick", "Open Model Source"))? else { return Ok(None) };
                model_overview::open(index, &project, &key)?;
                Ok(None)
            }
            "model-create-fields" => {
                let project = ctx.project().ok_or(Error::Stale)?;
                let root = project.root()?;
                if self.view_index(ctx).is_none() { return Err(Error::Stale); }
                let index = self.indexes.get(&root).ok_or(Error::Stale)?;
                let Some(key) = model_key_or_pick(index, &project, arg, &crate::ui_text::t("native.lib.createFieldsPick", "Create Model Fields"))? else { return Ok(None) };
                let path = model_overview::source_path(index, &project, &key)?;
                self.generator_workspace.seed_existing(ctx, index, &path)?;
                Ok(None)
            }
            "runtime-open-frame" => {
                let runner = self.artisan_workspace.runner(ctx)?;
                let Some(key) = self.runtime_row_or_pick(ctx, runner, arg, |row| row.get("error").is_some(), &crate::ui_text::t("native.lib.errorPick", "Open Error Stack Frame"), &crate::ui_text::t("native.lib.noErrors", "No errors have been read yet. Click Logs in the Runtime tab and pick a log first."))? else { return Ok(None) };
                self.runtime_inspection.open_log_frame(ctx, runner, &key)?;
                Ok(None)
            }
            "console-command-open" => {
                let Some(target)=arg.map(str::to_owned).or_else(||console_editor::editor_target(ctx)) else {
                    ui_kit::web::reveal_view(artisan_workspace::VIEW,include_str!("ui_config.json"))?;
                    return Ok(None);
                };
                let name=console_editor::resolve(ctx,&target)?;
                if !self.artisan_workspace.run_entry(ctx,&name,&self.artisan_terminals,false)? {
                    ui_kit::web::reveal_view(artisan_workspace::VIEW,include_str!("ui_config.json"))?;
                    host::notify(NotificationLevel::Info,SOURCE,&crate::ui_text::format("root.formatted.79812948ba", "Refresh Artisan command metadata to configure {name}.", &[("name", &format!("{}", name))]));
                }
                Ok(None)
            }
            "search-names" => {
                let project = ctx.project().ok_or(Error::Stale)?;
                self.advance_index(&project);
                let index = self.view_index(ctx).ok_or_else(still_indexing)?;
                search_names(index)?;
                Ok(None)
            }
            "route-search" => self.search_routes(ctx),
            "runtime-database-list" => {
                let root = ctx.project().ok_or(Error::Stale)?.root()?;
                let ids = self.runtime_inspection.database_sources(ctx)?;
                if ids.is_empty() { host::notify(NotificationLevel::Info, SOURCE, crate::ui_text::text("native.lib.113cede793", "No registered Laravel database sources. Add a Database Connection from Models first.")); return Ok(None); }
                let choices: Vec<_> = ids.iter().map(|id|openidea_plugin::PickItem::new(id,id)).collect();
                let Some(id) = host::pick(crate::ui_text::text("native.lib.d03a057646", "Laravel database connections"), &choices)? else { return Ok(None) };
                if !ids.contains(&id) {return Err(Error::Stale);}
                let Some(action) = host::pick(crate::ui_text::text("native.lib.c23b1b3b55", "Database connection action"), &[openidea_plugin::PickItem::new("connect", crate::ui_text::text("native.lib.c2138e9f13", "Connect / enter credentials…")),openidea_plugin::PickItem::new("remove", crate::ui_text::text("native.lib.37a3027038", "Remove connection and saved credentials…"))])? else { return Ok(None) };
                if ctx.project().ok_or(Error::Stale)?.root()? != root || !host::projects()?.iter().any(|p|p.root().ok().as_deref()==Some(&root)) {return Err(Error::Stale);}
                self.runtime_inspection.database_source_action(ctx,&action,Some(&id))?;
                refresh_laravel(); Ok(None)
            }
            "runtime-database-connect" | "runtime-database-remove" => {
                let action = if id == "runtime-database-connect" {"connect"} else {"remove"};
                if let Some(key) = arg.filter(|key|key.starts_with("runtime:database:")) {
                    self.runtime_inspection.database_source_row_action(ctx,action,key)?;
                } else { self.runtime_inspection.database_source_action(ctx,action,arg)?; }
                refresh_laravel(); Ok(None)
            }
            "route-editor-open" => {
                let Some(raw) = arg else { return self.search_routes(ctx) };
                let index = self.view_index(ctx).ok_or(Error::Stale)?;
                route_editor::open(index, ctx, raw)?;
                Ok(None)
            }
            "route-editor-action" => {
                let Some(raw) = arg else { return self.search_routes(ctx) };
                let index = self.view_index(ctx).ok_or(Error::Stale)?;
                route_editor::action(index, ctx, raw, DEFAULT_BASE_URL)?;
                Ok(None)
            }
            "find-literal-source-matches" => crate::source_matches::run(ctx),
            "routes-runtime-refresh" => {
                let project=ctx.project().ok_or(crate::ui_text::text("process.error0", "Open a Laravel project first."))?;
                let root=project.root()?;
                if self.runtime_routes.len() >= 8 && !self.runtime_routes.contains_key(&root) { return Err(Error::Invalid(crate::ui_text::text("native.lib.e2aad7065c", "Too many runtime route snapshots; reindex to clear them.").into())); }
                if !self.index_pending(&root) && self.completed_index_for(&root).is_none_or(|index|!index::index_sources_current(&project,index)) { self.queue_index(&project); }
                self.advance_index(&project);
                let index=self.view_index(ctx).ok_or(crate::ui_text::text("native.lib.3d4beff9da", "This project is not indexed as Laravel."))?;
                let partial=index.partial;
                let snapshot=route_runtime::capture(ctx,index)?;
                let count=snapshot.routes.len();
                let open: BTreeSet<_>=host::projects()?.iter().filter_map(|p|p.root().ok()).collect();
                self.runtime_routes.retain(|root,_|open.contains(root));
                self.runtime_routes.insert(root,snapshot);
                refresh_laravel();
                host::notify(NotificationLevel::Info,SOURCE,&crate::ui_text::format("root.formatted.4a93214e02", "Rebuilt the source index{partial} and loaded {count} registered routes from Artisan.", &[("partial", &format!("{}", if partial {crate::ui_text::text("native.lib.a1fe49246a", " (incomplete)")} else {""})), ("count", &format!("{}", count))]));
                Ok(None)
            }
            "reindex" => {
                self.runtime_routes.clear();
                let _task = host::begin_task(crate::ui_text::text("root.indexing", "Indexing Laravel projects"), false);
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
            "db-seed" => {
                if !confirm_database_command(ctx, crate::ui_text::text("native.lib.seedTitle", "Seed Database"), "native.lib.seedBody", "Run the database seeders for {root}?")? {
                    return Ok(None);
                }
                self.artisan(ctx, "db:seed")
            }
            "route-send-request" => {
                let Some(key) = self.route_key_or_pick(ctx, arg)? else { return Ok(None) };
                let Some(route) = self.route_at(&key, ctx) else {
                    return Ok(None);
                };
                let project = ctx.project().ok_or(Error::Stale)?;
                let root = project.root()?;
                let template = route_request::body_template(&project, &route);
                self.route_requests.select(&root, &route, template)?;
                ui_kit::web::reveal_view(route_request::VIEW, include_str!("ui_config.json"))?;
                refresh_laravel();
                Ok(None)
            }
            "route-copy-curl" | "route-create-http" => {
                let Some(key) = self.route_key_or_pick(ctx, arg)? else { return Ok(None) };
                let Some(mut route) = self.route_at(&key, ctx) else {
                    return Ok(None);
                };
                if route.method.contains('|') {
                    let choices:Vec<_>=route.method.split('|').map(|method|openidea_plugin::PickItem::new(method,method)).collect();
                    let Some(method)=host::pick(crate::ui_text::text("native.lib.c009049312", "HTTP method"),&choices)? else {return Ok(None);};
                    if !route.method.split('|').any(|known|known==method) {return Err(Error::Invalid(crate::ui_text::text("native.lib.0cdb3a94ab", "Unknown HTTP method.").into()));}
                    route.method=method;
                }
                match id {
                    "route-copy-curl" => copy_as_curl(&route, DEFAULT_BASE_URL, "Laravel")?,
                    _ => create_http_file_for_project(&route, DEFAULT_BASE_URL, None, &ctx.project().ok_or(Error::Stale)?, "Laravel")?,
                }
                Ok(None)
            }
            "migrate-fresh" => {
                if !confirm_database_command(ctx, crate::ui_text::text("native.lib.freshTitle", "Fresh Migration"), "native.lib.freshBody", "Drop every table in the database of {root} and run all migrations again?")? {
                    return Ok(None);
                }
                self.artisan(ctx, "migrate:fresh")
            }
            "runtime-about" | "runtime-events" | "runtime-schedule" | "runtime-queues" | "runtime-queue-status" | "runtime-workers" => {
                let section=id.strip_prefix("runtime-").ok_or(Error::Stale)?;
                self.runtime_inspection.capture(ctx,self.artisan_workspace.runner(ctx)?,section)?;
                refresh_laravel();
                Ok(None)
            },
            "runtime-inspect-queue" => {
                let Some(connection)=host::input(crate::ui_text::text("native.lib.f768be8c36", "Queue connection (blank for all configured connections)"),None)? else{return Ok(None)};
                let Some(queue)=host::input(crate::ui_text::text("native.lib.51dcb2b03d", "Queue name (blank for configured defaults)"),None)? else{return Ok(None)};
                self.runtime_inspection.capture_queue_status(ctx,self.artisan_workspace.runner(ctx)?,Some(connection.trim()).filter(|value|!value.is_empty()),Some(queue.trim()).filter(|value|!value.is_empty()))?;
                refresh_laravel();
                Ok(None)
            },
            "runtime-logs" => {
                let files=self.runtime_inspection.log_files(ctx)?;
                if files.is_empty(){host::notify(NotificationLevel::Info,SOURCE,crate::ui_text::text("native.lib.noLogFiles", "No log files found under storage/logs. They appear once the application writes a log."));return Ok(None);}
                let choices:Vec<_>=files.iter().map(|file|openidea_plugin::PickItem::new(file,file)).collect();
                if let Some(file)=host::pick(crate::ui_text::text("native.lib.21923a348b", "Read Laravel log"),&choices)? {
                    self.runtime_inspection.capture_log(ctx,self.artisan_workspace.runner(ctx)?,&file)?;
                    refresh_laravel();
                } Ok(None)
            },
            "runtime-retry-failed" => {
                let runner=self.artisan_workspace.runner(ctx)?;
                let Some(key)=self.runtime_row_or_pick(ctx,runner,arg,|row|row["key"].as_str().is_some_and(|key|key.starts_with("failedjob:")),&crate::ui_text::t("native.lib.failedJobPick","Retry Failed Job"),&crate::ui_text::t("native.lib.noFailedJobs","No failed jobs have been read yet. Click Failed Jobs in the Runtime tab first."))? else {return Ok(None)};
                let result=self.runtime_inspection.retry_failed(ctx,runner,&key)?;
                host::notify(NotificationLevel::Info,SOURCE,&result);
                refresh_laravel();
                Ok(None)
            },
            "runtime-runner" => {
                if let Some(runner)=host::pick(crate::ui_text::text("native.lib.0f9782a9d2", "Runtime PHP runner"),&[openidea_plugin::PickItem::new("php",crate::ui_text::text("command.localPhp", "Local PHP")),openidea_plugin::PickItem::new("docker","Docker")])? {
                    let runner:artisan_runner::Runner=serde_json::from_value(serde_json::json!(runner)).map_err(|_|Error::Stale)?;
                    self.artisan_workspace.handle(ctx,&serde_json::json!({"op":"runner","runner":runner}).to_string(),&self.artisan_terminals)?;
                    refresh_laravel();
                } Ok(None)
            },
            "unused-views" => self.unused_views(ctx),
            "artisan-run" => { ui_kit::web::reveal_view(artisan_workspace::VIEW, include_str!("ui_config.json"))?; Ok(None) },
            "artisan-run-terminal" => {if self.artisan_workspace.run_terminal_entry(ctx,&self.artisan_terminals)? {Ok(None)} else {generate::run_artisan(&self.artisan_terminals,ctx)}},
            "artisan-refresh-metadata" => {let result=artisan_metadata::refresh(ctx)?;if let Some(project)=ctx.project(){if let Ok(root)=project.root(){if let Some(index)=self.indexes.get_mut(&root){metadata::refresh_artisan(&project,&mut index.metadata);}}}Ok(result)},
            "artisan-refresh-docker-metadata" => artisan_docker::refresh(ctx),
            "artisan-choose-docker-target" => {let result=artisan_docker::discover(ctx)?;refresh_laravel();Ok(result)},
            "artisan-clear-docker-target" => {let result=artisan_docker::clear_discovered_target(ctx)?;refresh_laravel();Ok(result)},
            "artisan-browse-docker-metadata" => artisan_docker::browse(ctx),
            "make" | "new-model" | "generate-for-model" | "create-missing" => self.generator(id, ctx),
            "remove-phpdocs" => {
                let project = ctx.project().ok_or(Error::Stale)?;
                self.advance_index(&project);
                let index = self.view_index(ctx).filter(|index| !index.partial).ok_or_else(still_indexing)?;
                let Some(anchor) = bulk::phpdoc_anchor(index, &project) else {
                    host::notify(NotificationLevel::Info, SOURCE, &crate::ui_text::t("native.lib.noHelperPhpdoc", "No model has IDE helper PHPDoc to remove."));
                    return Ok(None);
                };
                if let Some(count) = self.apply_editor_action(&anchor, "bulk-phpdocs:")? {
                    host::notify(NotificationLevel::Info, SOURCE, &crate::ui_text::format("native.lib.phpdocRemoved", "Removed IDE helper PHPDoc from {count} models.", &[("count", &count.to_string())]));
                }
                Ok(None)
            }
            "reveal" => {
                host::reveal_view(VIEW)?;
                Ok(None)
            }
            "route-list" => {
                generate::run(&self.artisan_terminals, ctx, "route:list", Vec::new())?;
                Ok(None)
            }
            _ => Err(Error::NotFound(crate::ui_text::format("root.formatted.b0a5d84e95", "command {id}", &[("id", &format!("{}", id))]))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        for event in events {
            if let Event::VcsChanged(_)=event {
                refresh_laravel();
            }
            if let Event::Other{kind,data}=event {
                if kind=="settings-changed" {
                    let changed=serde_json::from_str::<Vec<String>>(data).ok();
                    if changed.as_ref().is_none_or(|keys|keys.iter().any(|key|matches!(key.as_str(),"viewPaths"|"modelDirectories"|"publicDirectory"|"locale"))) { self.queue_indexes(); }
                    refresh_laravel();
                }
            }
        }
        if events.iter().any(|event|matches!(event,Event::Scheduled)) {
            for project in host::projects().unwrap_or_default(){if let Ok(runner)=self.artisan_workspace.runner_for_project(&project){if let Err(error)=self.helper_automation.tick(&project,runner){host::notify(NotificationLevel::Warning,SOURCE,&crate::ui_text::format("root.formatted.4c5bd9748b", "Helper commands: {error}", &[("error", &format!("{}", error))]));}}}
        }

        for event in events {
            if let Some(exit) = event.terminal_exited() {
                if let Err(error) = self.project_wizard.complete(&exit) {
                    host::notify(NotificationLevel::Error, SOURCE, &error.to_string());
                }
            }
            if let Some(exit) = event.terminal_exited()
                && let Some(root) = self.artisan_terminals.complete(&exit, ctx)
                && let Some(project) = host::projects().unwrap_or_default().into_iter().find(|p|p.root().ok().as_deref()==Some(&root)) {
                self.runtime_routes.remove(&root);
                self.queue_index(&project);
                refresh_laravel();
            }
        }
        let mut saved: BTreeSet<&str> = BTreeSet::new();
        let mut opened: BTreeSet<&str> = BTreeSet::new();

        for event in events {
            match event {
                Event::FilesChanged(batch) => {
                    saved.extend(batch.created.iter().chain(&batch.modified).chain(&batch.removed).map(String::as_str));
                }
                Event::FileSaved(path) => {
                    saved.insert(path);
                }
                Event::FileOpened(path) => {
                    self.show_version_status(&path);
                    opened.insert(path);
                }
                Event::EditorFocused(Some(path)) => self.show_version_status(&path),
                _ => {}
            }
        }

        for project in host::projects().unwrap_or_default() {
            let Ok(root) = project.root() else { continue };
            let relative = |path: &&str| path.strip_prefix(&format!("{root}/")).map(str::to_owned);

            let changed = saved.iter().filter_map(relative).any(|r| affects_index(&r) || self.indexes.get(&root).is_some_and(|index| index.affects(&r)));
            let unknown = !self.indexes.contains_key(&root) && opened.iter().any(|path| under(path, &root));
            if saved.iter().any(|path|under(path,&root) && (path.ends_with(".php") || path.rsplit('/').next().is_some_and(|name|name.starts_with(".env") || matches!(name,"composer.json"|"composer.lock")))) { self.runtime_routes.remove(&root); self.runtime_inspection.invalidate_source_views(&root); }
            if changed || unknown {
                self.queue_index(&project);
            }

            for path in saved.iter().chain(&opened).filter(|path| under(path, &root)) {
                self.check(&project, path);
            }
            let paths=saved.iter().filter(|path|under(path,&root)).map(|path|(*path).to_owned()).collect::<Vec<_>>();
            if !paths.is_empty(){let result=self.artisan_workspace.runner_for_project(&project).and_then(|runner|self.helper_automation.on_changes(self,&project,&paths,runner));if let Err(error)=result{host::notify(NotificationLevel::Warning,SOURCE,&crate::ui_text::format("root.formatted.7ecedee4e5", "Helper automation: {error}", &[("error", &format!("{}", error))]));}}

        }
        self.advance_context_index(ctx);
    }
}

fn component_view_text(index: &Index, project: &Project, component: &str) -> Option<String> {
    let anonymous = components::metadata_view(index,component)
        .or_else(||index.components.get(component).filter(|entry|entry.path.ends_with(".blade.php")))
        .or_else(||index.views.get(&format!("components.{component}")))
        .or_else(||index.views.get(&format!("components.{component}.index")))?;
    if !owns_current_path(project, &anonymous.path) { return None; }
    project.read_file(&anonymous.path).ok()
}

/// Attributes and slots of the component named by the tag around the caret.
fn component_items(index: &Index, doc: &Document, offset: usize, context: &components::Context) -> Vec<CompletionItem> {
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };

    let (component, kind, start, slots) = match context {
        components::Context::Attribute { kind, component, start } => (component, *kind, *start, false),
        components::Context::Slot { component, start } => (component, Kind::Component, *start, true),
    };
    let replace = range_of(&doc.text, start, offset);

    let (props, view) = if kind == Kind::Livewire {
        let class =
            index.livewire.get(component).filter(|entry| !entry.path.is_empty() && owns_current_path(&project, &entry.path)).and_then(|entry| project.read_file(&entry.path).ok());
        let mut props=class.map(|class| components::livewire_props(&class)).unwrap_or_default();
        if let Some(names)=index.packages.volt_props.get(component){for name in names {if !props.iter().any(|prop|prop.name==*name){props.push(components::Prop{name:name.clone(),default:None});}}}
        (props, None)
    } else {
        let view = component_view_text(index, &project, component);
        let class = index
            .components
            .get(component)
            .filter(|entry| entry.path.ends_with(".php") && !entry.path.ends_with(".blade.php") && owns_current_path(&project, &entry.path))
            .and_then(|entry| project.read_file(&entry.path).ok());
        let props = package_integrations::native_component_props(index,&project,component).unwrap_or_else(||match &class {
            Some(class) => components::constructor_props(class),
            None => view.as_deref().map(components::view_props).unwrap_or_default(),
        });
        let view =
            view.or_else(|| index.views.get(&format!("components.{component}")).filter(|entry| owns_current_path(&project, &entry.path)).and_then(|entry| project.read_file(&entry.path).ok()));
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
            item.detail = prop.default.map(|default| crate::ui_text::format("root.formatted.3737caaa23", "default {default}", &[("default", &format!("{}", default))]));
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

fn directive_items(index: Option<&Index>, replace: Range, ranking: &directives::Ranking) -> Vec<CompletionItem> {
    let livewire = index.is_none_or(|index| index.metadata.package_enabled("livewire/livewire", true));
    let core = directives::CORE.iter().filter(|name| livewire || !matches!(**name, "livewire" | "livewireStyles" | "livewireScripts" | "livewireScriptConfig")).map(|name| (*name, None, true));
    let custom = index.into_iter().flat_map(|index| index.directives.iter()).map(|(name, entry)| (name.as_str(), entry.detail.clone(), false));

    let mut seen = BTreeSet::new();
    let mut items: Vec<_> = custom
        .chain(core)
        .filter(|(name, _, _)| seen.insert(*name))
        .map(|(name, detail, is_core)| {
            let mut item = CompletionItem::new(name, CompletionKind::Keyword);
            item.detail = detail;
            item.sort_text = Some(ranking.key(name, is_core));
            item.replace = Some(replace);
            if is_core && let Some((_, body)) = directives::SNIPPETS.iter().find(|(n, _)| *n == name) {
                item.snippet = true;
                item.insert_text = if ranking.repairs_opening(name) {
                    format!("{}$0", body.lines().next().unwrap_or(name))
                } else {
                    (*body).to_owned()
                };
            }
            item
        })
        .collect();
    items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
    items
}

impl Main {
    fn script_completion(&self, doc: &Document, at: Position) -> Vec<CompletionItem> {
        if !js::mentions_route(&doc.text) {
            return Vec::new();
        }

        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        let Some(name) = js::route_name_at(&doc.text, offset) else {
            return Vec::new();
        };
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };

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
        if let (Some(project), Some(index)) = (project_for(&doc.path), self.index_for(&doc.path)) {
            let direct=wayfinder_destinations::definition(&project,index,doc,at);if !direct.is_empty(){return direct;}
            let wayfinder = package_facts::wayfinder_definition(&project, index, doc, at);
            if !wayfinder.is_empty() { return wayfinder; }
        }
        if !js::mentions_route(&doc.text) {
            return Vec::new();
        }

        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        let Some(name) = js::route_name_at(&doc.text, offset) else {
            return Vec::new();
        };
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };

        let Some(project) = project_for(&doc.path) else { return Vec::new(); };
        index.routes.get(&doc.text[name]).and_then(|entry| scoped_location(&project, entry)).into_iter().collect()
    }
}

impl Providers for Main {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if let Some(offset) = text::offset(&doc.text, at) && let Some(found) = tool_config::completion(&doc.path, &doc.text, offset) {
            let replace = range_of(&doc.text, found.start, found.end);
            return found.names.iter().map(|name| {
                let mut item = CompletionItem::new(name, CompletionKind::Property);
                item.replace = Some(replace);
                item
            }).collect();
        }
        self.advance_document_index(&doc.path);
        if let Some(index)=self.index_for(&doc.path){let items=package_integrations::completion(index,doc,at);if !items.is_empty(){return items;}}
        if js::is_script(&doc.path, &doc.language) {
            return self.script_completion(doc, at);
        }

        let Some(language) = language_of(&doc.path, &doc.language) else {
            return Vec::new();
        };
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        if language == "blade" && blade_php::opaque_at(&doc.text, offset) { return Vec::new(); }
        if language == "blade" && let Some((start, end)) = directives::name_at(&doc.text, offset) {
            let custom = self.index_for(&doc.path).into_iter().flat_map(|index| index.directives.keys().map(String::as_str)).collect();
            let Some(ranking) = directives::ranking(&doc.text, offset, start, end, &custom) else {
                return Vec::new();
            };
            return directive_items(self.index_for(&doc.path), range_of(&doc.text, start, end), &ranking);
        }
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };
        if language=="blade" {let bare=components::bare_completion(index,doc,at);if !bare.is_empty(){return bare;}}
        if language=="blade" {let values=components::value_completion(index,doc,at);if !values.is_empty(){return values;}}
        if language == "php" && let Some(items) = pest_items(doc, offset) { return items; }
        if language == "php" && let Some((start, end)) = socialite_completion::context(&doc.text, offset) {
            let replace = range_of(&doc.text, start, end);
            return socialite_completion::providers().iter().map(|name| {
                let mut item = CompletionItem::new(name, CompletionKind::Reference);
                item.replace = Some(replace);
                item
            }).collect();
        }
        if language == "php" && let Some((start, prefix)) = php_snippets::context(&doc.text, offset) {
            let replace = range_of(&doc.text, start, offset);
            let snippets: Vec<_> = php_snippets::matching(prefix).map(|(label, detail, body, imports)| {
                let mut item = CompletionItem::new(label, CompletionKind::Snippet);
                item.detail = Some((*detail).into());
                item.insert_text = (*body).into();
                item.snippet = true;
                item.replace = Some(replace);
                item.additional_edits = imports.iter().filter_map(|fqcn| php::insert_use(&doc.text, fqcn)).filter(|edit| !edit.text.is_empty()).map(|edit| {
                    let at = text::position(&doc.text, edit.offset);
                    TextEdit { range: Range::new(at, at), text: edit.text }
                }).collect();
                item
            }).collect();
            if !snippets.is_empty() { return snippets; }
        }
        if language=="php" {let items=route_attributes::completion(index,doc,at);if !items.is_empty(){return items;}}
        if language=="php"{if let Some(items)=route_completion::short_controllers(index,doc,at){return items;}}
        if language=="php" {let items=route_completion::completion(index,doc,at);if !items.is_empty(){return items;}}
        if language=="php" {let items=validation_productivity::completion(index,doc,at);if !items.is_empty(){return items;}}
        let scopes = package_facts::passport_completion(index, doc, at);
        if !scopes.is_empty() { return scopes; }
        if language == "php" {
            let custom = metadata::completion_document(index, doc, offset);
            if !custom.is_empty() { return custom; }
            let headers=metadata::response_completion(index,doc,offset);if !headers.is_empty(){return headers;}
        }
        if language == "php" && enabled("enumValueInspection") && let Some(project) = project_for(&doc.path) {
            let items = enum_where_values::completion(index, doc, at, &|path| owns_current_path(&project, path));
            if !items.is_empty() { return items; }
        }
        if language == "php" {
            let root = project_for(&doc.path).and_then(|p| p.root().ok());
            let mut items = eloquent_members::completion_checked(index, doc, at, &|path| root.is_some() && project_for(path).and_then(|p|p.root().ok()) == root);
            eloquent_members::localize_completions(&mut items);
            if !items.is_empty() { return items; }
        }
        if language == "php" && let Some(token) = provider_token(&doc.path, &doc.text, offset) {
            return provider_items(index, doc, token);
        }
        if language == "blade"
            && let Some(context) = components::context_with_metadata(index,&doc.text, offset)
        {
            return component_items(index, doc, offset, &context);
        }

        let Some(reference) = refs::reference_at(language, &doc.text, offset).or_else(||(language=="php").then(||metadata::route_reference(&index.metadata,&doc.text,offset)).flatten()).or_else(||(language=="php").then(||metadata::pipe_reference(index,doc,offset)).flatten()) else {
            return Vec::new();
        };

        let replace = range_of(&doc.text, reference.start, reference.end);
        candidates(index, doc, &reference)
            .iter()
            .map(|(name, entry)| {
                let mut item = CompletionItem::new(name, completion_kind(reference.kind));
                item.detail = if matches!(reference.kind, refs::Kind::Component | refs::Kind::View | refs::Kind::Livewire) {
                    package_integrations::presentation_detail(entry.detail.clone())
                } else { entry.detail.clone() };
                item.replace = Some(replace);
                item
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        self.advance_document_index(&doc.path);
        if let Some(index)=self.index_for(&doc.path){let items=package_integrations::definition(index,doc,at);if !items.is_empty(){return items;}}
        if js::is_script(&doc.path, &doc.language) {
            return self.script_definition(doc, at);
        }

        let Some(language) = language_of(&doc.path, &doc.language) else {
            return Vec::new();
        };
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        if language=="blade" {let values=components::value_definition(index,doc,at);if !values.is_empty(){return values;}}
        if language=="blade" {let bare=components::bare_definition(index,doc,at);if !bare.is_empty(){return bare;}}
        if language=="php" {let items=route_attributes::definition(index,doc,at);if !items.is_empty(){return items;}}
        if language=="php"&&route_targets::recognized(index,doc,at){return route_targets::definition(index,doc,at);}
        let navigation = indexer::contextual_navigation(index,&doc.path,&doc.text);
        let resolver = ui_kit::navigation::Resolver::new(
            index
                .navigation
                .iter()
                .filter(|(path, _)| **path != doc.path)
                .map(|(path, g)| (path.as_str(), g))
                .chain(std::iter::once((doc.path.as_str(), &navigation))),
        );
        let Some(project) = project_for(&doc.path) else { return Vec::new() };
        let scopes = package_facts::passport_definition(index, doc, at);
        if !scopes.is_empty() { return scopes; }
        if language == "php" {
            let parents=metadata::config_parent_definition(index,&project,doc,offset);if !parents.is_empty(){return parents;}
            let custom = metadata::definition_document(index, doc, offset);
            let owned: Vec<_> = custom.into_iter().filter(|location| owns_current_path(&project, &location.path)).collect();
            if !owned.is_empty() { return owned; }
        }
        if language == "php" && let Some(entry) = eloquent_members::definition_checked(index, doc, at, &|path| project_for(path).and_then(|p|p.root().ok()) == project.root().ok()) {
            return scoped_location(&project, &entry).into_iter().collect();
        }
        let related = resolver
            .checked(|path| {
                if path == doc.path {
                    return Some(navigation.clone());
                }
                if !owns_current_path(&project, path) { return None; }
                project.read_file(path).ok().filter(|text| text.len() <= 512 * 1024).map(|text| indexer::contextual_navigation(index,path,&text))
            })
            .definition(&navigation, at);
        if !related.is_empty() {
            return related.into_iter().filter(|target| owns_current_path(&project, &target.path)).collect();
        }

        if language == "php"
            && let Some(token) = provider_token(&doc.path, &doc.text, offset)
        {
            let start = text::offset(&doc.text, token.start).unwrap_or(0);
            let end = text::offset(&doc.text, token.end).unwrap_or(0);
            return index.bindings.get(doc.text[start..end].trim_start_matches('\\')).and_then(|entry| scoped_location(&project, entry)).into_iter().collect();
        }
        let Some(reference) = refs::reference_at(language, &doc.text, offset).or_else(||(language=="php").then(||metadata::route_reference(&index.metadata,&doc.text,offset)).flatten()).or_else(||(language=="php").then(||metadata::pipe_reference(index,doc,offset)).flatten()) else {
            return Vec::new();
        };

        let name = &doc.text[reference.start..reference.end];
        let names = candidates(index, doc, &reference);
        let name = if reference.kind == Kind::Binding { name.replace("\\\\", "\\") } else { name.to_owned() };
        names.get(&name).and_then(|entry| scoped_location(&project, entry)).into_iter().collect()
    }

    /// Controller ↔ route, `view()` → view file, view → controllers and views
    /// using it, section ↔ parent section, push ↔ stack.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.advance_document_index(&doc.path);
        let Some(language) = language_of(&doc.path, &doc.language) else {
            return Vec::new();
        };
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };
        let Some(project) = project_for(&doc.path) else {
            return Vec::new();
        };

        let markers = match language {
            "php" => {
                let navigation = indexer::contextual_navigation(index,&doc.path,&doc.text);
                let resolver = ui_kit::navigation::Resolver::new(
                    index
                        .navigation
                        .iter()
                        .filter(|(path, _)| **path != doc.path)
                        .map(|(path, g)| (path.as_str(), g))
                        .chain(std::iter::once((doc.path.as_str(), &navigation))),
                );
                let mut markers = php_markers(index, &project, doc);
                markers.extend(metadata::config_parent_markers(index,&project,doc));
                markers.extend(
                    resolver
                        .checked(|path| {
                            if path == doc.path {
                                return Some(navigation.clone());
                            }
                            if !owns_current_path(&project, path) { return None; }
                            project
                                .read_file(path)
                                .ok()
                                .filter(|text| text.len() <= 512 * 1024)
                                .map(|text| indexer::contextual_navigation(index,path,&text))
                        })
                        .markers(&navigation),
                );
                markers
            }
            _ => blade_markers(index, &project, doc),
        };
        markers.into_iter().filter(|marker| marker.target.as_ref().is_none_or(|target| owns_current_path(&project, &target.path))).map(|mut marker|{indexer::localize_route_marker(index,&mut marker);marker}).collect()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        self.advance_document_index(&doc.path);
        let Some(language) = language_of(&doc.path, &doc.language) else {
            return Vec::new();
        };
        let Some(offset) = text::offset(&doc.text, selection.start) else {
            return Vec::new();
        };

        let mut found = Vec::new();
        if let Some(index) = self.index_for(&doc.path) {
            found.extend(missing_key_actions(index, doc, language, offset));
        }

        if language == "php" && enabled("checkEloquent") {
            let masked = php::mask_comments(&doc.text);
            found.extend(eloquent_checks::order_by(&masked).into_iter().filter(|f| f.start <= offset && offset <= f.end).filter_map(
                |finding| {
                    Some(CodeAction {
                        title: crate::ui_text::format("root.formatted.acc1548281", "Replace with {replacement}", &[("replacement", &format!("{}", finding.replacement.as_deref()?))]),
                        kind: Some("quickfix".into()),
                        preferred: true,
                        edits: vec![(
                            doc.path.clone(),
                            vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: finding.replacement? }],
                        )],
                        data: None,
                    })
                },
            ));
        }

        if language == "php" && enabled("checkSecurity") && let Ok(root) = project_for(&doc.path).map_or(Err(()), |p| p.root().map_err(|_| ())) {
            let relative = doc.path.strip_prefix(&format!("{root}/")).unwrap_or(&doc.path);
            found.extend(security_checks::findings(relative, &doc.language, &doc.text).into_iter().filter(|f| f.start <= offset && offset <= f.end).filter_map(|finding| {
                let replacement = finding.replacement?;
                Some(CodeAction {
                    title: crate::ui_text::format("root.formatted.acc1548281", "Replace with {replacement}", &[("replacement", &replacement)]),
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: replacement }])],
                    data: None,
                })
            }));
        }

        if language == "php" && enabled("enumValueInspection") && let (Some(index), Some(project)) = (self.index_for(&doc.path), project_for(&doc.path)) {
            found.extend(enum_where_values::findings(index, doc, &|path| owns_current_path(&project, path)).into_iter().filter(|f| f.start <= offset && offset <= f.end).filter_map(|finding| {
                let replacement = finding.replacement?;
                Some(CodeAction {
                    title: format!("Change to '{replacement}'"),
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: replacement }])],
                    data: None,
                })
            }));
        }

        if language == "php" && enabled("upgradeInspection") && let Some(index) = self.index_for(&doc.path) {
            let installed = index.metadata.framework_version.map(|(major, _, _)| major);
            found.extend(upgrade_checks::findings(installed, &doc.language, &doc.text).into_iter().filter(|f| f.start <= offset && offset <= f.end).filter_map(|finding| {
                let replacement = finding.replacement?;
                Some(CodeAction {
                    title: crate::ui_text::format("root.formatted.acc1548281", "Replace with {replacement}", &[("replacement", &replacement)]),
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: replacement }])],
                    data: None,
                })
            }));
        }

        if language == "php"
            && let Some((range, text)) = unused::remove_model_phpdocs(&doc.text, offset)
        {
            found.push(CodeAction {
                title: crate::ui_text::text("root.removeModelPhpdoc", "Remove IDE helper PHPDoc from model").into(),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text }])],
                data: None,
            });
        }

        let controllers_namespace = host::setting_string("controllersNamespace")
            .filter(|namespace| !namespace.is_empty())
            .unwrap_or_else(|| "App\\Http\\Controllers".into());
        let version = self.index_for(&doc.path).and_then(|index| index.metadata.framework_version);
        let features = actions::FrameworkFeatures {
            rule_array: version.is_some_and(|version| version >= (12,0,0)),
            scope_attribute: version.is_some_and(|version| version >= (12,6,0)) && host::setting_bool("scopeAttributeIntentions").unwrap_or(true),
            scope_model: self.index_for(&doc.path).is_some_and(|index|eloquent_members::scope_model_source(index,doc)),
            blade_class: version.is_some_and(|version|version >= (9,0,0)),
        };
        let mut rewrites = actions::rewrites_with_framework_features(&doc.path, language, &doc.text, offset, &controllers_namespace, features);
        if language=="php"{if let Some(contextual)=self.index_for(&doc.path).and_then(|index|actions::contextual_controllers(index,doc,offset,&controllers_namespace)){rewrites.retain(|rewrite|!actions::controller_rewrite(rewrite));rewrites.extend(contextual);}}
        rewrites.extend(surround_actions::rewrites(doc,selection));
        if language == "php"
            && let Some(index) = self.index_for(&doc.path)
        {
            rewrites.extend(lang_actions::column_rewrites(index, &doc.path, &doc.text, offset));
        }
        found.extend(rewrites.into_iter().map(|rewrite| CodeAction {
            title: lang_actions::localized_title(&actions::localized_title(&rewrite.title)),
            kind: Some(rewrite.kind.into()),
            preferred: false,
            edits: vec![(
                doc.path.clone(),
                rewrite.edits.into_iter().map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text,
                        }).collect(),
            )],
            data: None,
        }));

        if let (Some(index), Some(project)) = (self.index_for(&doc.path), project_for(&doc.path)) {
            found.extend(lang_actions::extract_string(index, &project, doc, language, selection).into_iter().map(|mut action| { action.title=lang_actions::localized_title(&action.title); action }));
        }

        found
    }

    fn resolve_code_action(&mut self, _doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let Some(data) = action.data.clone() else {
            return Ok(action);
        };
        let (file, key) = data.split_once('\n').ok_or(crate::ui_text::text("native.lib.cbe56cab0b", "Malformed code action."))?;
        let project = project_for(file).ok_or(crate::ui_text::text("native.lib.6f7c2bbe0d", "The file is not in an open project."))?;
        let text = project.read_file(file)?;

        let edit = lang_actions::entry_edit(&text, key, key).ok_or(crate::ui_text::text("native.lib.df9a210973", "The lang file doesn't end its array."))?;

        action.edits = vec![(file.to_owned(), vec![edit])];
        Ok(action)
    }
}

/// Adds a missing translation key to the main locale's lang file.
fn missing_key_actions(index: &Index, doc: &Document, language: &str, offset: usize) -> Vec<CodeAction> {
    let Some(reference) = refs::reference_at(language, &doc.text, offset) else {
        return Vec::new();
    };

    let name = &doc.text[reference.start..reference.end];
    if reference.kind != Kind::Translation || is_dynamic(name) || !missing_translation(index, name) {
        return Vec::new();
    }

    let Some((file_key, key)) = name.split_once('.') else {
        return Vec::new();
    };
    if key.contains('.') {
        return Vec::new();
    }

    let files = index.translation_files.get(file_key).map(Vec::as_slice).unwrap_or_default();
    files
        .iter()
        .take(1)
        .map(|file| CodeAction {
            title: crate::ui_text::format("root.formatted.30de26590d", "Add translation key \"{name}\" to {locale}/{file}", &[("name", &format!("{}", name)), ("locale", &format!("{}", file.rsplit('/').nth(1).unwrap_or_default())), ("file", &format!("{}", file_name(file)))]),
            kind: Some("quickfix".into()),
            preferred: false,
            edits: Vec::new(),
            data: Some(format!("{file}\n{key}")),
        })
        .collect()
}

fn model_gutter(index: &Index, doc: &Document) -> Option<openidea_plugin::language_features::GutterAction> {
    let full = eloquent_members::declaration(&doc.text)?;
    let is_model = eloquent_members::model_inventory(&index.eloquent_models).iter().any(|(model, path, _)| model.eq_ignore_ascii_case(&full) && *path == doc.path);
    if !is_model {
        return None;
    }
    let short = full.rsplit('\\').next()?;
    let offset = doc.text.find(&format!("class {short}"))?;
    Some(openidea_plugin::language_features::GutterAction {
        line: text::position(&doc.text, offset).line,
        icon: Some("navigate".into()),
        tooltip: format!("Eloquent model {short}: open Laravel hub"),
        command: "reveal".into(),
        argument: None,
    })
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
            markers.push(LineMarker {
                line: line_of(reference.start),
                icon: Some(if matches!(reference.kind, Kind::View) { VIEW_MARKER } else { CONTROLLER_MARKER }.into()),
                tooltip: format!("{label} {name}"),
                target: Some(target),
            });
        }
    }

    let Ok(root) = project.root() else {
        return markers;
    };
    let Some(inside) = doc.path.strip_prefix(&format!("{root}/{CONTROLLERS_DIRECTORY}")) else {
        return markers;
    };
    let masked = php::mask_comments(&doc.text);
    let Some(controller) = controllers::parse(inside, &masked) else {
        return markers;
    };

    let Some(full)=eloquent_members::declaration(&doc.text)else{return markers;};
    // Every route of a method shares one icon: the host lists several targets in
    // its anchored chooser and opens a single one directly.
    for (method, offset) in &controller.methods {
        let routes = index.route_defs.iter().filter(|r|r.width>0&&index.route_controller_full(r).is_some_and(|target|target.eq_ignore_ascii_case(&full))&&r.def.controller.as_ref().is_some_and(|(_,m)|m.eq_ignore_ascii_case(method)));
        for route in routes {
            let end = Position::new(route.at.line, route.at.character + route.width);
            let tooltip = crate::ui_text::format("root.formatted.c960019874", "Route {methods} {uri}", &[("methods", &route.def.methods), ("uri", &route.def.uri)]);
            let target = Location { path: route.path.clone(), range: Range::new(route.at, end) };
            markers.push(LineMarker { line: line_of(*offset), icon: Some(RELATED_MARKER.into()), tooltip, target: Some(target) });
        }
    }

    markers
}

/// Where `name` is defined by `pick` in the view's file.
fn locate(index: &Index, project: &Project, view: &str, pick: fn(&blade::Facts) -> Vec<&blade::Named>, name: &str) -> Option<Location> {
    let (entry, source, facts) = current_blade_view(index, project, view)?;
    let (_, offset) = pick(&facts).into_iter().find(|(n, _)| n == name)?;
    let lines = Lines::new(&source);
    let start = lines.position(*offset);
    let width = lines.width(*offset, name.len()) as u32;
    Some(Location { path: entry.path.clone(), range: Range::new(start, Position::new(start.line, start.character + width)) })
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
            marker(0, USED_BY_MARKER, crate::ui_text::format("root.formatted.c7a29e7f7e", "Used by {users}", &[("users", &format!("{}", users.join(", ")))]), Some(target));
        }

        let renders: Vec<&index::Render> = index.renders.iter().filter(|r| r.view == view).collect();
        if let Some(first) = renders.first() {
            let callers: Vec<&str> = renders.iter().map(|r| r.caller.as_str()).collect();
            let target = Location { path: first.path.clone(), range: Range::new(first.at, first.at) };
            marker(0, RENDERED_BY_MARKER, crate::ui_text::format("root.formatted.80621090a6", "Rendered by {callers}", &[("callers", &format!("{}", callers.join(", ")))]), Some(target));
        }
    }

    let parents: Vec<&str> = index.layout_parent(&facts).map(|p| std::iter::once(p).chain(index.parents(p)).collect()).unwrap_or_default();
    let descendants: Vec<&str> = match view {
        Some(view) => index.blade.keys().map(String::as_str).filter(|v| index.parents(v).contains(&view)).collect(),
        None => Vec::new(),
    };

    for (name, offset) in &facts.sections {
        let line = line_of(*offset);
        let parent = parents.iter().find_map(|p| locate(index, project, p, sections_and_yields, name));
        if let Some(parent) = parent {
            marker(line, SECTION_PARENT_MARKER, crate::ui_text::format("root.formatted.dbd29333be", "Parent section \"{name}\"", &[("name", &format!("{}", name))]), Some(parent));
        }
    }

    for (name, offset) in facts.sections.iter().chain(&facts.yields) {
        let child = descendants.iter().find_map(|d| locate(index, project, d, only_sections, name));
        if let Some(child) = child {
            marker(line_of(*offset), SECTION_CHILD_MARKER, crate::ui_text::format("root.formatted.f06d126755", "Section \"{name}\" is filled by another view", &[("name", &format!("{}", name))]), Some(child));
        }
    }

    for (name, offset) in &facts.pushes {
        let stack = parents.iter().find_map(|p| locate(index, project, p, only_stacks, name));
        if let Some(stack) = stack {
            marker(line_of(*offset), STACK_PARENT_MARKER, crate::ui_text::format("root.formatted.f72ffdea96", "Stack \"{name}\"", &[("name", &format!("{}", name))]), Some(stack));
        }
    }

    for (name, offset) in &facts.stacks {
        let push = descendants.iter().find_map(|d| locate(index, project, d, only_pushes, name));
        if let Some(push) = push {
            marker(line_of(*offset), STACK_PUSH_MARKER, crate::ui_text::format("root.formatted.c0d995cb9b", "Stack \"{name}\" is pushed to by another view", &[("name", &format!("{}", name))]), Some(push));
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
        "component" => Some(&index.components),
        "livewire" => Some(&index.livewire),
        "config" => Some(&index.config),
        "translation" => Some(&index.translations),
        "middleware" => Some(&index.middleware),
        "env" => Some(&index.env),
        _ => None,
    }
}

fn route_list_of(index: &Index) -> Vec<Route> {
    let identities=eloquent_members::class_inventory(&index.eloquent_models);
    let label=|entry:&crate::index::RouteEntry,raw:&str|->String{
        let Some(full)=index.route_controller_full(entry)else{return raw.to_owned();};
        let mut matches=index.controllers.iter().filter(|(_,candidate)|identities.iter().any(|identity|!identity.ambiguous&&identity.path==candidate.path&&identity.full.eq_ignore_ascii_case(full)));
        let first=matches.next();if matches.next().is_none(){if let Some((name,_))=first{return name.clone();}}
        full.to_owned()
    };
    index
        .route_defs
        .iter()
        .map(|entry| {
            let def = &entry.def;
            let handler = match &def.controller {
                Some((class, method)) => Handler { class: label(entry,class), member: Some(method.clone()) },
                None => Handler { class: def.action.clone(), member: None },
            };
            let end = Position::new(entry.at.line, entry.at.character + entry.width);
            Route {
                method: def.methods.split('|').filter(|method|!(*method=="HEAD" && def.methods.split('|').any(|m|m=="GET"))).collect::<Vec<_>>().join("|"),
                path: def.uri.clone(),
                handler,
                name: def.name.clone(),
                location: Some(Location { path: entry.path.clone(), range: Range::new(entry.at, end) }),
            }
        })
        .collect()
}

fn entry_for<'a>(index: &'a Index, key: &str) -> Option<(&'a str, Position, u32)> {
    if let Some(encoded) = key.strip_prefix("translation-locale:") {
        let (locale, name): (String, String) = serde_json::from_str(encoded).ok()?;
        let entry = index.translations_by_locale.get(&locale)?.get(&name)?;
        return Some((&entry.path, entry.at, entry.width));
    }
    let (group, name) = key.split_once(':')?;
    if group == "route" {
        let route = index.route_defs.get(name.parse::<usize>().ok()?)?;
        return Some((&route.path, route.at, route.width));
    }

    let entry = group_names(index, group)?.get(name)?;
    Some((&entry.path, entry.at, entry.width))
}

fn view_inventory(index: &Index, project: &Project) -> Body {
    let root=project.root().unwrap_or_default();
    let items: Vec<serde_json::Value> = index.views.iter().filter(|(_,entry)|owns_current_path(project,&entry.path)).map(|(name, entry)| {
        let namespace=name.split_once("::").map_or("app",|(namespace,_)|namespace);
        let file=entry.path.strip_prefix(&root).unwrap_or(&entry.path).trim_start_matches(['/', '\\']);
        let folder=index.view_folders.get(name).map(String::as_str).unwrap_or("(root)");
        serde_json::json!({"key":format!("view:{name}"),"cells":[name,file,folder,namespace],"icon":"file-html"})
    }).collect();
    Body::Other { kind:"resources".into(),data:serde_json::json!({"columns":[crate::ui_text::text("column.view", "View"),crate::ui_text::text("column.file", "File"),crate::ui_text::text("column.folder", "Folder"),crate::ui_text::text("column.namespace", "Namespace")],"columnStyles":["code","code","muted","locale"],"columnWeights":[3,4,2,1.5],"showRowIcons":true,"searchPlaceholder":crate::ui_text::text("search.laravel-views", "Filter views…"),"facetLabels":{"2":"All folders","3":"All namespaces"},"facets":[2,3],"items":items}).to_string() }
}

fn component_inventory(index: &Index, project: &Project) -> Body {
    let root = project.root().unwrap_or_default();
    let items: Vec<serde_json::Value> = [("component", &index.components), ("livewire", &index.livewire)]
        .into_iter().flat_map(|(kind, names)| names.iter().map(move |(name, entry)| (kind, name, entry)))
        .filter(|(_, _, entry)| !entry.path.is_empty() && owns_current_path(project, &entry.path))
        .map(|(kind, name, entry)| {
            let file = entry.path.strip_prefix(&root).unwrap_or(&entry.path).trim_start_matches(['/', '\\']);
            let (label, category, icon) = if kind == "livewire" {
                (format!("<livewire:{name}>"), "Livewire", "class")
            } else if entry.detail.as_deref() == Some("Blade::component") {
                (format!("<x-{name}>"), "Blade alias", "file-html")
            } else if entry.path.ends_with(".blade.php") {
                (format!("<x-{name}>"), "Blade anonymous", "file-html")
            } else {
                (format!("<x-{name}>"), "Blade class", "class")
            };
            serde_json::json!({"key":format!("{kind}:{name}"),"cells":[label,category,file],"icon":icon})
        }).collect();
    Body::Other { kind: "resources".into(), data: serde_json::json!({
        "columns":[crate::ui_text::text("column.component", "Component"),crate::ui_text::text("column.type", "Type"),crate::ui_text::text("column.file", "File")],"columnStyles":["code","text","muted"],
        "columnWeights":[3,1.5,5],"showRowIcons":true,"searchPlaceholder":crate::ui_text::text("search.laravel-components", "Filter components…"),
        "facets":[1],"facetLabels":{"1":"All types"},"items":items
    }).to_string() }
}

fn env_inventory(index: &Index, root: &str) -> Body {
    let items: Vec<serde_json::Value> = index.env.iter().map(|(name, entry)| {
        let value = index.env_values.get(name).map_or("", String::as_str);
        let file = entry.path.strip_prefix(root).unwrap_or(&entry.path).trim_start_matches(['/', '\\']);
        serde_json::json!({"key":format!("env:{name}"),"cells":[name,value,file],"icon":"file-config"})
    }).collect();
    Body::Other { kind:"resources".into(), data:serde_json::json!({"columns":[crate::ui_text::text("column.name", "Name"),crate::ui_text::text("column.value", "Value"),crate::ui_text::text("column.file", "File")],"columnStyles":["code","code","muted"],"showRowIcons":true,"searchPlaceholder":crate::ui_text::text("search.laravel-env", "Filter environment variables…"),"facetLabels":{"2":"All files"},"facets":[2],"items":items}).to_string() }
}

fn config_inventory(index: &Index, root: &str) -> Body {
    let items: Vec<serde_json::Value> = index.config.iter().map(|(name, entry)| {
        let value = index.config_expressions.get(name).map(String::as_str).unwrap_or_else(|| entry.detail.as_deref().unwrap_or("Expression unavailable"));
        serde_json::json!({"key":format!("config:{name}"),"cells":[name,value,entry.path.strip_prefix(root).unwrap_or(&entry.path).trim_start_matches(['/', '\\'])],"icon":"file-config"})
    }).collect();
    Body::Other { kind:"resources".into(), data:serde_json::json!({"columns":[crate::ui_text::text("column.key", "Key"),crate::ui_text::text("column.value", "Value"),crate::ui_text::text("column.file", "File")],"columnStyles":["code","code","muted"],"showRowIcons":true,"searchPlaceholder":crate::ui_text::text("search.laravel-config", "Filter config…"),"facetLabels":{"2":"All files"},"facets":[2],"items":items}).to_string() }
}

fn translation_inventory(index: &Index, root: &str) -> Body {
    let items: Vec<serde_json::Value> = index.translations_by_locale.iter().flat_map(|(locale, names)| {
        names.iter().map(move |(name, entry)| serde_json::json!({
            "key": format!("translation-locale:{}", serde_json::json!([locale, name])),
            "cells": [name, locale, entry.detail.as_deref().unwrap_or(""), entry.path.strip_prefix(root).unwrap_or(&entry.path).trim_start_matches(['/', '\\'])],
            "icon": "text"
        }))
    }).collect();
    Body::Other { kind: "resources".into(), data: serde_json::json!({"columns":[crate::ui_text::text("column.key", "Key"),crate::ui_text::text("column.locale", "Locale"),crate::ui_text::text("column.value", "Value"),crate::ui_text::text("column.file", "File")],"columnStyles":["code","locale","text","muted"],"showRowIcons":true,"searchPlaceholder":crate::ui_text::text("search.laravel-translations", "Filter translations…"),"facetLabels":{"1":"All locales"},"facets":[1],"items":items}).to_string() }
}

impl Main {
    fn tree(&self, index: &Index, group: &str) -> Vec<TreeItem> {
        let icon = GROUPS.iter().find(|(id, _, _)| *id == group).map_or("file", |(_, _, icon)| *icon);
        group_names(index, group).into_iter().flat_map(|names| names.iter()).map(|(name, entry)| {
            let leaf = TreeItem::new(&format!("{group}:{name}"), name).icon(icon);
            match &entry.detail {
                Some(detail) if !detail.is_empty() => leaf.detail(detail),
                _ => leaf,
            }
        }).collect()
    }

    fn route_list(&mut self) -> &mut RouteList {
        self.route_list.get_or_insert_with(|| RouteList::new("laravel.routes", PLUGIN_ID))
    }

    fn active_routes(&self, ctx: &Invocation) -> Vec<Route> {
        let Some(index)=self.view_index(ctx) else{return vec![];};
        let project=ctx.project();
        if let Some(snapshot)=project.as_ref().and_then(|p|p.root().ok()).and_then(|root|self.runtime_routes.get(&root)) {
            return snapshot.routes.iter().cloned().map(|mut route|{if route.location.as_ref().is_some_and(|l|!project.as_ref().is_some_and(|p|owns_current_path(p,&l.path))){route.location=None;}route}).collect();
        }
        route_list_of(index)
    }
    fn search_routes_from_index(&self, ctx: &Invocation, index:&Index) -> Vec<Route> {
        let Some(project) = ctx.project() else {return vec![]};
        if index.partial {return vec![]}
        if let Some(snapshot) = project.root().ok().and_then(|root|self.runtime_routes.get(&root)) {
            return route_runtime::endpoint_routes(snapshot,index,&project);
        }
        route_list_of(index).into_iter().zip(&index.route_defs).filter_map(|(mut route,entry)| {
            if !owns_current_path(&project,&entry.path) {return None}
            if let Some(uri) = &entry.endpoint_uri {route.path=uri.clone();}
            Some(route)
        }).collect()
    }
    fn handle_route_request(&mut self, ctx: &Invocation, event: ViewEvent) -> Result<(), Error> {
        let project = ctx.project().ok_or(Error::Stale)?;
        let root = project.root()?;
        if !host::projects()?.iter().any(|open| open.root().ok().as_deref() == Some(&root)) {
            return Err(Error::Stale);
        }
        match event {
            ViewEvent::Field(key, value) => self.route_requests.set_field(&root, &key, &value)?,
            ViewEvent::Button(id) if id == "request-reset-body" => {
                self.route_requests.reset_body(&root)?;
                refresh_laravel();
            }
            ViewEvent::Button(id) if id == "request-copy-curl" => {
                let request = self.route_requests.request(&root)?;
                host::copy_to_clipboard(&request.curl(DEFAULT_BASE_URL))?;
                host::notify(NotificationLevel::Info, SOURCE, crate::ui_text::text("request.curlCopied", "Copied the request as curl. Act as user is not part of a curl request, and web routes still need their CSRF token against a real server."));
            }
            ViewEvent::Button(id) if id == "request-send" => {
                let runner = self.artisan_workspace.runner(ctx)?;
                let sent = self.route_requests.send(&root, |args| route_request::execute(&project, runner, args));
                refresh_laravel();
                sent?;
            }
            _ => {}
        }
        Ok(())
    }

    fn search_routes(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or(Error::Stale)?;
        self.advance_index(&project);
        let index=self.view_index(ctx).ok_or_else(still_indexing)?;
        if index.partial {return Err(Error::Failed(crate::ui_text::text("native.lib.3b27ad675b", "Laravel route index is incomplete; reindex before searching.").into()));}
        if !index::index_sources_current(&project,index){return Err(still_indexing());}
        let initial = self.search_routes_from_index(ctx,index);
        route_search::run(ctx, initial, || { if !index::index_sources_current(&project,index) {return Err(Error::Stale);} Ok(self.search_routes_from_index(ctx,index)) })?;
        Ok(None)
    }

    /// The row key a route command was given, or one the user picks when it runs from the palette.
    fn route_key_or_pick(&self, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        if let Some(key) = arg { return Ok(Some(key.to_owned())); }
        let routes = self.active_routes(ctx);
        if routes.is_empty() { return Err(Error::Invalid(crate::ui_text::t("native.lib.noRoutesToPick", "No routes found. Open a Laravel project and let indexing finish, then try again."))); }
        let choices: Vec<_> = routes.iter().enumerate().map(|(i, route)| openidea_plugin::PickItem::new(&format!("route:{i}"), &route_search::label(route))).collect();
        host::pick(crate::ui_text::text("routeSearch.matches", "Matching Laravel routes"), &choices)
    }

    /// The row key a Runtime command was given, or one the user picks from the read rows when it runs from the palette.
    fn runtime_row_or_pick(&self, ctx: &Invocation, runner: artisan_runner::Runner, arg: Option<&str>, keep: impl Fn(&serde_json::Value) -> bool, title: &str, empty: &str) -> Result<Option<String>, Error> {
        if let Some(key) = arg { return Ok(Some(key.to_owned())); }
        let snapshot = self.runtime_inspection.cached(ctx, runner)?;
        let choices: Vec<_> = snapshot["snapshots"].as_array().into_iter().flatten()
            .flat_map(|snapshot| snapshot["rows"].as_array().into_iter().flatten())
            .filter(|row| keep(row))
            .filter_map(|row| {
                let label = row["cells"].as_array()?.iter().filter_map(|cell| cell.as_str()).filter(|cell| !cell.is_empty()).collect::<Vec<_>>().join(" · ");
                Some(openidea_plugin::PickItem::new(row["key"].as_str()?, &label.chars().take(160).collect::<String>()))
            })
            .collect();
        if choices.is_empty() { return Err(Error::Invalid(empty.into())); }
        host::pick(title, &choices)
    }

    /// Runs an editor action's own review from a command and writes its result, since a command
    /// can't hand the host a workspace edit. `None` when the user cancels the review.
    fn apply_editor_action(&mut self, path: &str, data: &str) -> Result<Option<usize>, Error> {
        use openidea_plugin::file_actions::FileActions;
        let project = host::project_of(path).ok_or(Error::Stale)?;
        let doc = Document { path: path.into(), language: "php".into(), version: 0, text: project.read_file(path)? };
        let action = self.file_actions(&doc, Range::default()).into_iter()
            .find(|action| action.data.as_deref().is_some_and(|key| key.starts_with(data)))
            .ok_or_else(|| Error::Invalid(crate::ui_text::t("native.lib.actionUnavailable", "This can't run right now. Save open files, wait for indexing to finish and try again.")))?;
        match self.resolve_file_action(&doc, action) {
            Ok(resolved) => bulk::write_action(&project, &resolved).map(Some),
            Err(Error::Cancelled) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn route_at(&self, key: &str, ctx: &Invocation) -> Option<Route> {
        let number: usize = key.strip_prefix("route:")?.parse().ok()?;
        self.active_routes(ctx).into_iter().nth(number)
    }

    fn web_config(&self, ctx: &Invocation) -> String {
        let mut config: serde_json::Value = serde_json::from_str(include_str!("ui_config.json")).expect("packaged Laravel UI config");
        config["uiLanguage"] = ui_text::locale().into();
        if ctx.project().and_then(|project| project.root().ok()).is_some_and(|root| self.index_work_pending(&root)) {
            let views = config["views"].as_object().into_iter().flat_map(|views|views.keys().cloned()).collect::<Vec<_>>();
            if let Some(opaque) = config["opaque_views"].as_array_mut() {
                for view in views { if !opaque.iter().any(|id| id.as_str() == Some(&view)) { opaque.push(view.into()); } }
            }
        }
        if let Some(opaque)=config["opaque_views"].as_array_mut() {
            for view in ["laravel-models"] { if !opaque.iter().any(|id|id.as_str()==Some(view)) { opaque.push(view.into()); } }
        }
        let runtime = self.artisan_workspace.runner(ctx).ok().and_then(|runner| self.runtime_inspection.cached(ctx, runner).ok());
        for action in config["actions"].as_array_mut().expect("Laravel actions") {
            let keys: Option<Vec<serde_json::Value>> = match action["id"].as_str() {
                Some("runtime-database-connect" | "runtime-database-remove") => Some(self.runtime_inspection.database_source_rows(ctx).unwrap_or_default().into_iter().filter_map(|row|row.get("key").cloned()).collect()),
                Some("runtime-open-frame") => Some(runtime.as_ref().into_iter().flat_map(|snapshot| snapshot["snapshots"].as_array().into_iter().flatten()).flat_map(|snapshot| snapshot["rows"].as_array().into_iter().flatten()).filter(|row| row.get("error").is_some()).filter_map(|row|row.get("key").cloned()).collect()),
                Some("model-open-source" | "model-create-fields") => Some(ctx.project().and_then(|project| self.view_index(ctx).map(|index| model_overview::source_keys(index, &project))).unwrap_or_default()),
                _ => None,
            };
            if let Some(keys) = keys { action["keys"] = keys.into(); }
        }
        if let Some(index) = self.view_index(ctx) {
            for tab in config["group"]["tabs"].as_array_mut().expect("Laravel tabs") {
                let count = match tab["id"].as_str().unwrap_or_default() {
                    VIEW => Some(self.active_routes(ctx).len()),
                    "laravel-components" => ctx.project().map(|project| [&index.components, &index.livewire].into_iter().flat_map(|names| names.values()).filter(|entry| !entry.path.is_empty() && owns_current_path(&project, &entry.path)).count()),
                    "laravel-models" => ctx.project().map(|project| crate::eloquent_members::model_inventory(&index.eloquent_models).into_iter().filter(|(_, path, _)| owns_current_path(&project, path)).count()),
                    "laravel-views" => Some(index.views.values().filter(|entry|ctx.project().as_ref().is_some_and(|project|owns_current_path(project,&entry.path))).count()),
                    "laravel-config" => Some(index.config.len()),
                    "laravel-translations" => Some(index.translations_by_locale.values().map(Names::len).sum()),
                    "laravel-middleware" => Some(index.middleware.len()),
                    "laravel-env" => Some(index.env.len()),
                    _ => None,
                };
                if let Some(count) = count { tab["count"] = count.into(); }
            }
        }
        config.to_string()
    }

    fn view_index(&self, ctx: &Invocation) -> Option<&Index> {
        let root = ctx.project()?.root().ok()?;
        if root.is_empty() || root.chars().any(char::is_control) || root.split(['/', '\\']).any(|part|matches!(part,"."|".."))
            || !host::projects().ok()?.iter().any(|project|project.root().ok().as_deref()==Some(root.as_str())) {return None;}
        self.indexes.get(&root)
    }
}

impl Views for Main {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if !matches!(view,"laravel-about"|"laravel-artisan"|"laravel-generators"|"laravel-processes"|"laravel-runtime"|"laravel-database"|route_request::VIEW) && self.view_index(ctx).is_none() {
            if let Some(pending)=self.pending_index_view(ctx) { return Ok(pending); }
        }
        (|| -> Result<View, Error> {
            if view == "laravel-about" {
                let project = ctx.project().ok_or(crate::ui_text::text("process.error0", "Open a Laravel project first."))?;
                let runner = self.artisan_workspace.runner(ctx)?;
                let cached = self.runtime_inspection.cached(ctx, runner).ok();
                return Ok(View::new(about::body(&project, cached.as_ref())?)
                    .button(Button::new("runtime-about", crate::ui_text::text("native.lib.08d13322f2", "Refresh App Info")).icon("refresh"))
                    .button(Button::new("runtime-runner", if runner == artisan_runner::Runner::Docker {"Docker"} else {crate::ui_text::text("command.localPhp", "Local PHP")}).icon("terminal")));
            }
            if view == route_request::VIEW {
                return Ok(self.route_requests.render(&ctx.project().ok_or(Error::Stale)?.root()?));
            }
            if view == artisan_workspace::VIEW {
                return Ok(View::new(Body::Html(artisan_workspace::html())));
            }
            if view == generator_workspace::VIEW {
                return Ok(View::new(Body::Html(generator_workspace::html())));
            }
        if view == database_workspace::DATABASE_VIEW {
                return Ok(View::new(Body::Html(database_workspace::html())));
            }
        if view == managed_processes::VIEW {
                return Ok(View::new(Body::Html(managed_processes::html())));
            }
            if view == "laravel-models" {
                let project = ctx.project().ok_or(crate::ui_text::text("process.error0", "Open a Laravel project first."))?;
                let Some(index) = self.view_index(ctx) else { return Ok(View::new(Body::Message(crate::ui_text::text("final.models.indexBeforeBrowse","Index this Laravel project to browse models.").into()))); };
                let schema = self.runtime_inspection.schema_snapshot(ctx, self.artisan_workspace.runner(ctx)?).ok();
                let expanded = self.model_tree_expanded.get(&project.root()?).cloned().unwrap_or_default();
                let rows = self.model_rows.borrow_mut().entry(project.root()?).or_insert_with(|| model_overview::rows(index, &project)).clone();
                let inventory = model_overview::inventory_with_rows(rows, index, &project, schema.as_ref(), &expanded);
                return Ok(View::new(inventory)
                    .button(Button::new("new-model", crate::ui_text::text("native.lib.c52a01384f", "New Model…")).icon("add"))
                    .button(Button::new("helper-parameters", crate::ui_text::text("native.lib.902d026067", "Helper Parameters…")).icon("settings"))
                    .button(Button::new("helper-source-automation", crate::ui_text::text("native.lib.d6933a0cc5", "Helper Source & Automation…")).icon("settings"))
                    .button(Button::new("helper-automation-poll", crate::ui_text::text("action.runPendingHelperCommands", "Run Pending Model Helper Commands")).icon("terminal"))
                    .button(Button::new("helper-automation-stop", crate::ui_text::text("action.stopHelperCommands", "Stop Model Helper Commands")).icon("stop"))
                    .button(Button::new("runtime-model-connections", crate::ui_text::text("native.lib.readModelConnections", "Read Model Database Connections…")).icon("database"))
                    .button(Button::new("regenerate-model-helpers", crate::ui_text::text("native.lib.f1ce4d1777", "Regenerate Helpers…")).icon("refresh"))
                    .button(Button::new("runtime-schema", crate::ui_text::text("native.lib.f1c6a224e5", "Refresh Database Metadata")).icon("database"))
                    .button(Button::new("model-from-database", crate::ui_text::text("native.lib.52b406bd26", "Model from Table…")).icon("class"))
                    .button(Button::new("runtime-register-database", crate::ui_text::text("native.lib.2daae400b6", "Add Database Connection…")).icon("database"))
                    .button(Button::new("runtime-database-list", crate::ui_text::text("native.lib.12abf3f7a3", "Database Connections…")).icon("database"))
                    .button(Button::new("reindex", crate::ui_text::text("action.reindex", "Reindex")).icon("refresh")));
            }
            if view == "laravel-runtime" {
                let runner = self.artisan_workspace.runner(ctx)?;
                return Ok(View::new(self.runtime_inspection.body(ctx,runner)?)
                    .button(Button::new("runtime-database-list",crate::ui_text::text("native.lib.12abf3f7a3", "Database Connections…")).icon("database"))
                    .button(Button::new("runtime-application-mcp",crate::ui_text::text("native.lib.d29378a863", "Connect Application MCP…")).icon("services"))
                    .button(Button::new("runtime-about", crate::ui_text::text("action.runtime-about", "Application")).icon("info"))
                    .button(Button::new("runtime-events", crate::ui_text::text("action.runtime-events", "Events")).icon("text"))
                    .button(Button::new("runtime-schedule", crate::ui_text::text("action.runtime-schedule", "Schedule")).icon("calendar"))
                    .button(Button::new("runtime-queues",crate::ui_text::text("action.runtime-queues", "Failed Jobs")).icon("warning"))
                    .button(Button::new("runtime-queue-status",crate::ui_text::text("action.runtime-queue-status", "Queue Status")).icon("services"))
                    .button(Button::new("runtime-workers", crate::ui_text::text("action.runtime-workers", "Workers")).icon("services"))
                    .button(Button::new("runtime-logs", crate::ui_text::text("action.runtime-logs", "Logs")).icon("text"))
                    .button(Button::new("runtime-runner",if runner==artisan_runner::Runner::Docker{"Docker"}else{crate::ui_text::text("command.localPhp", "Local PHP")}).icon("terminal"))
                    .status(&self.runtime_inspection.status(ctx,runner)?));
            }
            let reindex = Button::new("reindex", crate::ui_text::text("action.reindex", "Reindex")).icon("refresh");
            let group = match view {
                VIEW => "route",
                "laravel-views" => "view",
                "laravel-components" => "components",
                "laravel-config" => "config",
                "laravel-translations" => "translation",
                "laravel-middleware" => "middleware",
                "laravel-env" => "env",
                _ => return Err(Error::NotFound(view.into())),
            };
            let routes_active = group == "route";
            let Some(index) = self.view_index(ctx) else {
                return Ok(View::message("This project is not a Laravel application.").button(reindex));
            };

            let partial = index.partial;
            let has_index_issues=!index.metadata.issues.is_empty();
            let native_components_partial=index.packages.native_partial;
            let config_gaps = index.config_coverage_gaps();
            let config_incomplete = config_gaps.len();
            let config_coverage = config_gaps.iter().take(3).map(String::as_str).collect::<Vec<_>>().join(", ");
            let toolbar = |body| {
                let mut view = View::new(body);
                if routes_active {
                    view = view.button(Button::new("routes-runtime-refresh", crate::ui_text::text("native.lib.refreshRegisteredRoutes", "Refresh Registered Routes")).icon("refresh"));
                    view = view.button(Button::new("route-list", crate::ui_text::text("native.lib.ed2e28eb59", "Route List")).icon("terminal"));
                } else {
                    view = view.button(reindex);
                }
                if has_index_issues{view=view.button(Button::new("index-issues", crate::ui_text::text("action.index-issues","Index Issues")).icon("warning"));}
                let cache_action = match group {
                    "route" => Some(("route-clear", "Clear Route Cache")),
                    "view" => Some(("view-clear", "Clear Compiled Views")),
                    "config" | "env" => Some(("config-clear", "Clear Config Cache")),
                    _ => None,
                };
                if let Some((id, label)) = cache_action {
                    view = view.button(Button::new(id, label).icon("build-project"));
                }
                view
            };
            let mut view = if routes_active {
                let routes = self.active_routes(ctx);
                let list = self.route_list();
                list.set_routes(routes);
                list.render(toolbar(Body::Message(String::new())))
            } else if group == "components" {
                toolbar(component_inventory(index, &ctx.project().ok_or(Error::Stale)?))
            } else if group == "view" {
                toolbar(view_inventory(index, &ctx.project().ok_or(Error::Stale)?))
            } else if group == "config" {
                toolbar(config_inventory(index, &ctx.project().and_then(|p| p.root().ok()).unwrap_or_default()))
            } else if group == "env" {
                toolbar(env_inventory(index, &ctx.project().and_then(|p| p.root().ok()).unwrap_or_default()))
            } else if group == "translation" {
                toolbar(translation_inventory(index, &ctx.project().and_then(|p| p.root().ok()).unwrap_or_default()))
            } else {
                toolbar(Body::Tree(self.tree(index, group)))
            };
            if routes_active {
                let provenance=ctx.project().and_then(|p|p.root().ok()).and_then(|root|self.runtime_routes.get(&root)).map(route_runtime::Snapshot::provenance).unwrap_or_else(||crate::ui_text::text("native.lib.routesFromSource", "Routes read from source files · Refresh Registered Routes to include package routes").into());
                view.status=Some(format!("{} · {provenance}",view.status.as_deref().unwrap_or(crate::ui_text::text("root.formatted.0dbc380315", "No routes"))));
            }
            if group == "config" && config_incomplete > 0 {
                view = view.status(&crate::ui_text::format("root.formatted.de02ed919e", "Config coverage incomplete: {config_coverage}{more} (dynamic arrays, unreadable sources or indexing limits){partial}", &[("config_coverage", &format!("{}", config_coverage)), ("more", &format!("{}", if config_incomplete > 3 {crate::ui_text::format("root.formatted.671692d603", ", +{count} more", &[("count", &format!("{}", config_incomplete-3))])} else {String::new()})), ("partial", &format!("{}", if partial {crate::ui_text::text("native.lib.1455526819", "; project indexing also timed out")} else {""}))]));
            }
            if group=="components" && native_components_partial {
                let warning=crate::ui_text::text("native.components.partial", "Some component registrations could not be resolved. Open Index Issues for details.");
                view.status=Some(match view.status.take(){Some(status)=>format!("{status} · {warning}"),None=>warning.into()});
            }
            if partial && view.status.is_none() {
                view = view.status(crate::ui_text::text("native.lib.8caf65552a", "Index incomplete (time limit reached)"));
            }
            Ok(view)
        })().map(|rendered|self.decorate_index_progress(rendered,ctx)).map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if let ViewEvent::Message(message) = &event {
            if serde_json::from_str::<serde_json::Value>(message).ok().is_some_and(|request|request["channel"] == "ui-kit" && (request["op"] == "render" || Self::is_index_tick_envelope(&request))) {
                self.advance_context_index(ctx);
            }
        }
        let config = self.web_config(ctx);
        if ui_kit::web::handle(self, view, &event, ctx, &config)? {
            return Ok(());
        }
        if let ViewEvent::Message(message) = &event {
            if let Ok(request) = serde_json::from_str::<serde_json::Value>(message) {
                if request["op"] == "index-tick" {
                    let root=ctx.project().and_then(|project|project.root().ok());
                    let was_pending=root.as_ref().is_some_and(|root|self.index_pending(root));
                    self.advance_context_index(ctx);
                    let reply = serde_json::json!({"_backgroundPoll": request["_backgroundPoll"], "pending": ctx.project().and_then(|project|project.root().ok()).is_some_and(|root|self.index_work_pending(&root))});
                    ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
                    if view==generator_workspace::VIEW && was_pending {
                        if let Some(index)=root.as_ref().and_then(|root|self.indexes.get(root)) {
                            self.generator_workspace.set_models(ctx,index)?;
                            let mut state=self.generator_workspace.handle(ctx,r#"{"op":"state"}"#)?;
                            state["_indexReady"]=true.into();
                            ui_kit::web::post_view_message(view,&state.to_string(),&config)?;
                        }
                    }
                    self.index_ticks=self.index_ticks.wrapping_add(1);
                    if self.index_ticks%10==0 { refresh_laravel(); }
                    return Ok(());
                }
            }
        }

            if view == "laravel-models" && matches!(&event,ViewEvent::Message(_)) {
            if let ViewEvent::Message(message)=event {
                let request=serde_json::from_str::<serde_json::Value>(&message).map_err(|_|Error::Stale)?;
                if request["op"]!="helper-automation-tick"{return Err(Error::Stale);}
                let result=self.artisan_workspace.runner(ctx).and_then(|runner|self.helper_automation.handle(ctx,&message,runner));
                let mut reply=match result{Ok(value)=>value,Err(error)=>serde_json::json!({"error":error.to_string()})};
                reply["_backgroundPoll"]=request["_backgroundPoll"].clone();
                ui_kit::web::post_view_message(view,&reply.to_string(),&config)?;
            }
            return Ok(());
        }
        if view == database_workspace::DATABASE_VIEW {
            if let ViewEvent::Message(message) = event {
                let result = self.artisan_workspace.runner(ctx).and_then(|runner| {
                    let index = ctx.project().and_then(|project| project.root().ok()).and_then(|root| self.indexes.get(&root));
                    self.database_workspace.handle(ctx, &message, index, runner)
                });
                let reply = result.unwrap_or_else(|error| serde_json::json!({"error": error.to_string()}));
                ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
            }
            return Ok(());
        }
        if view == managed_processes::VIEW {
            if let ViewEvent::Message(message) = event {
                let reply = match self.managed_processes.handle(ctx, &message) {
                    Ok(value) => value,
                    Err(error) => serde_json::json!({"error": error.to_string()}),
                };
                ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
            }
            return Ok(());
        }
        if view == generator_workspace::VIEW {
            if let ViewEvent::Message(message) = event {
                let result = (|| {
                    let project = ctx.project().ok_or(Error::Stale)?;
                    let root = project.root()?;
                    if !self.indexes.contains_key(&root) {
                        if !self.index_pending(&root) { self.queue_index(&project); }
                        self.advance_index(&project);
                    }
                    if let Some(index) = self.indexes.get(&root) {
                    let requested_model=serde_json::from_str::<serde_json::Value>(&message).ok().and_then(|value|value.get("model").or_else(||value.get("draft").and_then(|draft|draft.get("model"))).or_else(||value.get("request").and_then(|request|request.get("model"))).and_then(|model|model.as_str()).map(str::to_owned));
                    if let Some(model)=requested_model {
                        let schema=eloquent_members::model_identities(&index.eloquent_models).into_iter().find(|(full,_)|full==&model).and_then(|(full,path)|self.artisan_workspace.runner_for_project(&project).ok().and_then(|runner|self.runtime_inspection.helper_schema(&project,runner,&full,&path).ok().flatten()));
                        self.generator_workspace.set_model_schema(ctx,&model,schema)?;
                    }
                    self.generator_workspace.set_models(ctx,index)?;
                    } else if self.index_pending(&root) {
                        self.generator_workspace.set_models_pending(ctx)?;
                    } else {
                        return Err(Error::Failed(crate::ui_text::text("native.lib.3d4beff9da","This project is not indexed as Laravel.").into()));
                    }
                    let before = self.generator_workspace.prepared(&root);
                    let mut reply = self.generator_workspace.handle(ctx, &message)?;
                    let Some((key, anchor)) = self.generator_workspace.prepared(&root).into_iter().find(|entry| !before.contains(entry)) else { return Ok(reply) };
                    reply["prepared"] = false.into();
                    reply["instruction"] = match self.apply_editor_action(&anchor, &key)? {
                        Some(count) => crate::ui_text::format("native.lib.filesWritten", "Created or updated {count} files.", &[("count", &count.to_string())]),
                        None => crate::ui_text::t("native.lib.nothingWritten", "Nothing was written."),
                    }.into();
                    Ok(reply)
                })();
                let reply = match result {
                    Ok(value) => value,
                    Err(error) => serde_json::json!({"error": error.to_string()}),
                };
                ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
            }
            return Ok(());
        }
        if view == artisan_workspace::VIEW {
            if let ViewEvent::Message(message) = event {
                let reply = match self.artisan_workspace.handle(ctx, &message, &self.artisan_terminals) {
                    Ok(value) => value,
                    Err(error) => {
                        let preview = serde_json::from_str::<serde_json::Value>(&message).ok().is_some_and(|request| request["op"] == "preview");
                        if !preview && !matches!(error, Error::Cancelled) {
                            host::notify(NotificationLevel::Error, "Artisan", &error.to_string());
                        }
                        serde_json::json!({"error":error.to_string()})
                    },
                };
                if reply["completed"] == true && let Some(project) = ctx.project() { if let Ok(root)=project.root(){self.runtime_routes.remove(&root);} self.index_project(&project); }
                ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
            }
            return Ok(());
        }

        if view == route_request::VIEW {
            return self.handle_route_request(ctx, event);
        }
        if view == VIEW {
            match self.route_list().handle(&event) {
                RouteEvent::Changed => {
                    host::refresh_view(view);
                    return Ok(());
                }
                RouteEvent::Open(route) => {
                    if !self.active_routes(ctx).iter().any(|current| current == &route) {
                        refresh_laravel();
                        return Err(Error::Stale);
                    }
                    if let Some(location) = route.location {
                        host::open_location(&location.path, location.range.start.line, location.range.start.character)?;
                    }
                    return Ok(());
                }
                RouteEvent::Ignored => {}
            }
        }

        let model_expand = matches!(&event, ViewEvent::Expand(_));
        match event {
            ViewEvent::Expand(key) | ViewEvent::Collapse(key) if view == "laravel-models" => {
                let current = self.render(view, None, ctx)?;
                let Body::Other { data, .. } = current.body else { return Err(Error::Stale) };
                let body: serde_json::Value = serde_json::from_str(&data).map_err(|_| Error::Stale)?;
                if !body["items"].as_array().is_some_and(|items| items.iter().any(|item|item["key"] == key && item["expandable"] == true)) { return Err(Error::Stale); }
                let root = ctx.project().ok_or(Error::Stale)?.root()?;
                self.model_tree_expanded.entry(root).or_default().insert(key, model_expand);
                host::refresh_view(view);
            }
            ViewEvent::Activate(key) => {
                if view == "laravel-models" {
                    let index = self.view_index(ctx).ok_or(Error::Stale)?;
                    let project = ctx.project().ok_or(Error::Stale)?;
                    return model_overview::open(index, &project, &key);
                }
                let target = self.view_index(ctx).and_then(|index| entry_for(index, &key));
                if let Some((path, at, _)) = target.filter(|(path, _, _)| !path.is_empty() && ctx.project().as_ref().is_some_and(|project| owns_current_path(project, path))) {
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

impl openidea_plugin::inlay_hints::InlayHints for Main {
    fn inlay_hints(&mut self, doc: &Document, range: Range) -> Vec<openidea_plugin::inlay_hints::InlayHint> {
        self.advance_document_index(&doc.path);
        if !host::setting_bool("routeHints").unwrap_or(true) { return Vec::new(); }
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        let Some(project) = project_for(&doc.path) else { return Vec::new() };
        route_editor::hints(index, doc, range, &|path| owns_current_path(&project, path))
    }
}

impl openidea_plugin::language_features::LanguageFeatures for Main {
    fn type_definition(&mut self,doc:&Document,at:Position)->Vec<Location>{
        self.advance_document_index(&doc.path);
        let Some(project)=project_for(&doc.path)else{return vec![]};let Some(index)=self.index_for(&doc.path)else{return vec![]};
        if route_targets::recognized(index,doc,at){return route_targets::type_definition(index,doc,at);}
        eloquent_members::type_definition_checked(index,doc,at,&|path|owns_current_path(&project,path)).iter().filter_map(|entry|scoped_location(&project,entry)).collect()
    }

    fn signature_help(&mut self, doc: &Document, at: Position) -> Option<openidea_plugin::language_features::SignatureHelp> {
        self.advance_document_index(&doc.path);
        let index = self.index_for(&doc.path)?;
        let project = project_for(&doc.path)?;
        let mut help=eloquent_members::forwarded_signature_help_checked(index, doc, at, &|path| owns_current_path(&project, path))?;
        for signature in &mut help.signatures {
            if signature.documentation.as_deref()==Some("Declared constructor parameters forwarded through project metadata.") {
                signature.documentation=Some(crate::ui_text::t("final.signature.forwarded","Declared constructor parameters forwarded through project metadata."));
            }
        }
        Some(help)
    }
    fn code_lenses(&mut self, doc: &Document) -> Vec<openidea_plugin::language_features::CodeLens> {
        self.advance_document_index(&doc.path);
        let Some(project) = project_for(&doc.path) else { return Vec::new() };
        let owns=|path:&str| owns_current_path(&project,path);
        let mut lenses=console_editor::lenses(doc,&owns);
        for lens in &mut lenses {
            if let Some(command)=lens.argument.as_deref().and_then(|raw|serde_json::from_str::<serde_json::Value>(raw).ok()).and_then(|value|value["command"].as_str().map(str::to_owned)) {
                lens.title=crate::ui_text::format("final.console.run","Run {command}…",&[("command",&command)]);
            }
        }
        if host::setting_bool("routeHints").unwrap_or(true) && let Some(index)=self.index_for(&doc.path) {lenses.extend(route_editor::lenses(index,doc,&owns));}
        lenses
    }
    fn gutter_actions(&mut self, doc: &Document) -> Vec<openidea_plugin::language_features::GutterAction> {
        self.advance_document_index(&doc.path);
        let Some(project) = project_for(&doc.path) else { return Vec::new() };
        let owns=|path:&str| owns_current_path(&project,path);
        let mut gutters=console_editor::gutters(doc,&owns);
        if let Some(index)=self.index_for(&doc.path) {
            gutters.extend(model_gutter(index,doc));
        }
        gutters
    }
}

openidea_plugin::register_plugin!(Main, providers, views, indexing, endpoints, search, file_actions, hover, diagnostics, mcp, inlay_hints, language_features);

#[cfg(test)]
mod tests;

impl openidea_plugin::search::SearchProvider for Main {
    /// Answers from the finished index only: no indexing step, no process, no prompt.
    fn search(&mut self, category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<openidea_plugin::search::SearchPage, Error> {
        let Some(index) = self.view_index(ctx).filter(|_| category == "routes") else {
            return Ok(Default::default());
        };
        Ok(route_search::page(self.search_routes_from_index(ctx, index), query, limit))
    }
}

impl openidea_plugin::endpoints::EndpointsProvider for Main {
    fn endpoints(&mut self, ctx: &Invocation) -> Result<Vec<openidea_plugin::endpoints::Endpoint>, Error> {
        let Some(project) = ctx.project() else {
            return Ok(Vec::new());
        };
        if !index::is_laravel(&project) {return Ok(Vec::new());}
        self.advance_index(&project);
        let Some(index) = project.root().ok().and_then(|root|self.completed_index_for(&root)) else { return Ok(Vec::new()); };
        if index.partial {
            return Err(Error::Failed(crate::ui_text::text("native.lib.d18609f6eb", "Laravel route index is incomplete; refresh after indexing.").into()));
        }
        if let Some(snapshot) = project.root().ok().and_then(|root|self.runtime_routes.get(&root)) {
            let routes = route_runtime::endpoint_routes(snapshot,index,&project);
            return Ok(ui_kit::routes::endpoints(&routes,"Laravel"));
        }
        let mut routes = route_list_of(index);
        for (route, entry) in routes.iter_mut().zip(&index.route_defs) {
            route.method = entry.def.methods.clone();
        }
        let mut endpoints = Vec::new();
        for (mut route, entry) in routes.into_iter().zip(&index.route_defs) {
            let Some(uri) = &entry.endpoint_uri else { continue };
            route.path = uri.clone();
            let mut published = ui_kit::routes::endpoints(&[route], "Laravel");
            for endpoint in &mut published {
                endpoint.handler = Some(match &entry.def.controller {
                    Some((class, method)) => format!("{}@{method}",index.route_controller_full(entry).unwrap_or(class)),
                    None => entry.def.action.clone(),
                });
            }
            endpoints.extend(published);
        }
        Ok(endpoints)
    }
}

#[cfg(test)]
mod marker_icon_guard {
    #[test]
    fn marker_icons_are_declared_in_the_manifest() {
        let manifest = include_str!("../plugin.toml");
        for icon in [crate::VIEW_MARKER, crate::CONTROLLER_MARKER, crate::CONFIG_DEFAULTS_MARKER, crate::USED_BY_MARKER, crate::RENDERED_BY_MARKER, crate::SECTION_PARENT_MARKER, crate::SECTION_CHILD_MARKER, crate::STACK_PARENT_MARKER, crate::STACK_PUSH_MARKER, crate::MIGRATION_MARKER, crate::MODEL_MARKER, crate::MODEL_SUPPORT_MARKER, crate::LISTENER_MARKER, crate::EVENT_MARKER, crate::ROUTE_MARKER, crate::RELATED_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")), "{icon}");
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file(), "{icon}");
        }
    }
}

#[cfg(test)]
mod incomplete_locale_tests {
    use super::*;

    #[test]
    fn a_key_lacking_in_one_locale_names_it() {
        let mut index = Index::default();
        index.translations.add("app.hi", None, "/p/lang/en/app.php", Position::default(), 0);
        index.translations_by_locale.entry("en".into()).or_default().add("app.hi", None, "/p/lang/en/app.php", Position::default(), 0);
        index.translations_by_locale.entry("fr".into()).or_default();
        assert_eq!(incomplete_locales(&index, "app.hi"), ["fr"]);
        assert!(incomplete_locales(&index, "app.bye").is_empty());
    }
}
