//! Symfony: completion, navigation, problems, gutter markers and a tool
//! window for routes, Twig templates, services, translations, classes,
//! assets, entries and roles in PHP, Twig, YAML and XML files.


mod attr_args;
mod checks;
mod config_checks;
mod console_job;
mod console_rules;
mod search_tabs;
mod console_runner;
mod console_workspace;
mod database;
mod database_sources;
mod db_schema;
mod docker_target;
mod doctrine;
mod orm_links;
mod file_actions;
mod functional_test;
mod twig_extract;
mod translation_extract;
mod container_inject;
mod generators;
mod index;
mod indexer;
mod literal_rename;
mod hover;
mod docs_links;
mod ownership;
mod intentions;
mod make_commands;
mod create_missing;
mod unused_templates;
mod command_from_class;
mod new_project;

const PHP_DENIED: &str = "Running PHP needs the process:php permission. Allow it in Settings › Plugins › Symfony, and make sure php is on your PATH.";
mod mcp;
mod live;
mod namespace_paths;
mod php_checks;
mod about;
mod components;
mod entities;
mod forms;
mod php_text;
mod php_types;
mod processes;
mod practice_checks;
mod profiler;
mod profiler_details;
mod query_builder_checks;
mod refs;
mod related;
mod request_dto;
mod reference_model;
mod rewrites;
mod route_defs;
mod route_duplicates;
mod route_lenses;
mod route_request;
mod run_configs;
mod runtime;
mod runtime_logs;
mod source_xml;
mod source_occurrences;
mod service_gen;
mod service_file_actions;
mod service_edit_actions;
mod config_file_actions;
mod suggest;
mod strict_json;
mod psr4;
mod composer_compiler;
mod twig;
mod twig_checks;
mod validation_constraints;
mod wiring;
mod events;
mod browse_tabs;
mod yaml_quotes;
#[cfg(test)]
mod panic_regressions;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, Table, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Diagnostic, Event, EventKind, Location, PickItem, Position, Range, Severity, TextEdit};
use openidea_plugin::{Error, Invocation, LogLevel, NotificationLevel, Plugin, Project, StatusItem, StatusMenuEntry, host, text};

pub use index::Index;
use index::{Entry, Names, Sink};
use refs::{Kind, Reference};
use ui_kit::routes::{self, Handler, Route, RouteEvent, RouteList};

const VIEW: &str = "symfony";
/// Main source index only; profiler/native editors keep their own protocols.
fn refresh_profiler_view() {
    ui_kit::web::refresh_view(PROFILER_VIEW, include_str!("ui_config.json"));
}
fn refresh_main_view() {
    for view in MAIN_VIEWS {
        ui_kit::web::refresh_view(view, include_str!("ui_config.json"));
    }
}

const STATUS_ITEM: &str = "symfony";
const ROUTES_KEY: &str = "symfony.routes";
const DEFAULT_BASE_URL: &str = "http://localhost:8000";
const ABOUT_VIEW: &str = "symfony-about";
const ENTITIES_VIEW: &str = "symfony-entities";
const TEMPLATES_VIEW: &str = "symfony-templates";
const SERVICES_VIEW: &str = "symfony-services";
const CONFIG_VIEW: &str = "symfony-config";
const ENV_VIEW: &str = "symfony-env";
const TRANSLATIONS_VIEW: &str = "symfony-translations";
const MAIN_VIEWS: [&str; 10] = [ABOUT_VIEW, VIEW, ENTITIES_VIEW, TEMPLATES_VIEW, SERVICES_VIEW, CONFIG_VIEW, ENV_VIEW, TRANSLATIONS_VIEW, browse_tabs::COMPONENTS_VIEW, browse_tabs::EVENTS_VIEW];
const PROFILER_VIEW: &str = "symfony-profiler";
const SOURCE: &str = "Symfony";
const DEFAULT_DOMAIN: &str = "messages";
const CHECKED_LANGUAGES: &[(&str, &str)] = &[(".php", "php"), (".twig", "twig"), (".yaml", "yaml"), (".yml", "yaml"), (".xml", "xml")];
/// Whether a saved file (project-relative) feeds the index.
fn affects_index(relative: &str) -> bool {
    let directory = ["config", "templates", "translations", "src"]
        .iter()
        .any(|directory| relative == *directory || relative.starts_with(&format!("{directory}/")));
    directory || index::affects_compiled_index(relative) || matches!(relative, "composer.json" | "symfony.lock" | "webpack.config.js" | "importmap.php")
}
/// Parent templates followed for blocks (`extends` chains are short; this stops cycles).
const MAX_PARENTS: usize = 8;

fn under(path: &str, root: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

/// Roots of the open projects nested inside `root`, read once per request.
pub(crate) fn nested_roots(root: &str) -> Vec<String> {
    host::projects()
        .unwrap_or_default()
        .iter()
        .filter_map(|project| project.root().ok())
        .filter(|inner| inner != root && under(inner, root))
        .collect()
}

fn project_for(path: &str) -> Option<Project> {
    host::projects()
        .ok()?
        .into_iter()
        .filter(|p| p.root().is_ok_and(|root| under(path, &root)))
        .max_by_key(|p| p.root().map_or(0, |r| r.len()))
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

/// Autowiring target of a class or interface id. A resource-loaded class counts
/// only when its file declares a concrete class.
fn autowired_entry<'a>(index: &'a Index, id: &str) -> Option<&'a Entry> {
    let entry = index.autowired.get(id)?;
    if entry.detail.as_deref() != Some(reference_model::RESOURCE_DETAIL) {
        return Some(entry);
    }

    let class_path = &index.classes.get(id)?.path;
    let text = host::project_of(class_path)?.read_file(class_path).ok()?;
    reference_model::is_instantiable(&text).then_some(entry)
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
        let Some(entry) = index.templates.get(&name) else {
            break;
        };
        let Ok(parent_text) = project.read_file(&entry.path) else {
            break;
        };

        for (block, offset) in refs::twig_blocks(&parent_text) {
            blocks.add(block, Some(name.clone()), &entry.path, text::position(&parent_text, offset));
        }
        parent = refs::twig_parent(&parent_text).map(str::to_owned);
    }

    blocks
}

/// A known literal domain, or the default only when no domain was supplied.
fn domain_of(doc_language: &str, text: &str, reference: &Reference) -> Option<String> {
    if doc_language=="twig" { return twig_checks::reference_domain(text,reference); }
    if doc_language=="php" {
        let before=text.get(..reference.start.checked_sub(1)?)?.trim_end();
        if before.strip_suffix(':').is_some_and(|name|name.ends_with("message") || name.ends_with("Message")) {return Some("validators".into());}
        return ownership::source_domain(doc_language,text,reference);
    }
    Some(refs::translation_domain(doc_language, text, reference).unwrap_or_else(|| DEFAULT_DOMAIN.into()))
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
        Kind::Stimulus => Cow::Borrowed(&index.stimulus),
        Kind::Component => Cow::Borrowed(&index.components),
        Kind::Role => Cow::Borrowed(&index.roles),
        Kind::Tag => {
            let mut all: Names = CORE_TAGS
                .iter()
                .map(|tag| {
                    ((*tag).to_owned(), Entry { detail: Some("Symfony core tag".into()), path: String::new(), at: Position::new(0, 0) })
                })
                .collect();
            for (tag, entry) in &index.tags {
                all.entry(tag.clone()).or_insert_with(|| entry.clone());
            }
            Cow::Owned(all)
        }
        Kind::Translation => {
            if let Some(names) = domain_of(language,text,reference).and_then(|domain|index.translations.get(&domain)) {
                return Cow::Borrowed(names);
            }

            // A known key in another catalogue does not establish ownership in
            // the requested domain. Never merge unrelated translation scopes.
            Cow::Owned(Names::new())
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
        Kind::Entry | Kind::Stimulus | Kind::Component => CompletionKind::Module,
        Kind::Role => CompletionKind::Constant,
        Kind::Block => CompletionKind::Field,
        Kind::Tag => CompletionKind::Keyword,
    }
}

/// Twig built-ins that are variables rather than functions.
const TWIG_BUILT_IN_VARIABLES: [(&str, &str); 4] =
    [("app", "Symfony global"), ("_self", "Twig"), ("_context", "Twig"), ("_charset", "Twig")];

/// Variables at the caret: the template's own (`set`, `for`, macro arguments, `@var`),
/// those a controller renders it with or another template includes it with,
/// `twig.globals` and Twig's built-ins.
fn twig_variables(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    let Some((start, end)) = refs::twig_variable_at(&doc.text, offset) else {
        return Vec::new();
    };
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

/// Members of `var.` / `var.a.b.` in Twig, typed from `{# @var #}` comments and from what
/// controllers render the template with.
fn twig_members(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    let Some((chain, start, end)) = refs::twig_member_chain_at(&doc.text, offset) else {
        return Vec::new();
    };
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };
    let load = |fqcn: &str| -> Option<String> {
        let entry = index.classes.get(fqcn.trim_start_matches('\\'))?;
        project.read_file(&entry.path).ok()
    };

    let template = index.template_of(&doc.path);
    let Some(value) = twig_chain_value(index, doc, template, &load, &chain, offset, 0) else {
        return Vec::new();
    };

    member_items(php_types::value_members(&load, &value), range_of(&doc.text, start, end))
}

/// The class a service id stands for: its declared class, following aliases, or the id when it is a project class.
fn service_class(index: &Index, id: &str, depth: u8) -> Option<String> {
    if depth > 5 {
        return None;
    }
    let is_class_name = |name: &str| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'\\'));
    if let Some(detail) = index.services.get(id).and_then(|entry| entry.detail.as_deref()) {
        if let Some(target) = detail.strip_prefix("alias of ") {
            return service_class(index, target.trim(), depth + 1);
        }
        if is_class_name(detail) {
            return Some(detail.trim_start_matches('\\').to_owned());
        }
    }
    (index.classes.contains_key(id) && is_class_name(id)).then(|| id.to_owned())
}

/// What a Twig variable is at `offset`: a loop variable over a typed list, a `{# @var #}` comment, or a controller's argument.
fn twig_value(index: &Index, doc: &Document, template: Option<&str>, load: php_types::Loader, name: &str, offset: usize, depth: u8) -> Option<php_types::Value> {
    if depth > 4 {
        return None;
    }

    if let Some((expression, tag_at)) = twig::loop_source(&doc.text, offset, name) {
        let chain = twig::iterated_chain(&expression)?;
        return match twig_chain_value(index, doc, template, load, &chain, tag_at, depth + 1)? {
            php_types::Value::List(item) => Some(php_types::Value::Object(item)),
            _ => None,
        };
    }

    if let Some(value) = php_types::var_comment_values(&doc.text, offset).remove(name) {
        return Some(value);
    }

    index.renders.iter().filter(|render| Some(render.template.as_str()) == template).find_map(|render| php_types::hint_value(render.variable_types.get(name)?, load, &|id| service_class(index, id, 0)))
}

fn twig_chain_value(index: &Index, doc: &Document, template: Option<&str>, load: php_types::Loader, chain: &[String], offset: usize, depth: u8) -> Option<php_types::Value> {
    let mut value = twig_value(index, doc, template, load, chain.first()?, offset, depth)?;
    for segment in &chain[1..] {
        value = php_types::value_member(load, &value, segment)?;
    }
    Some(value)
}

/// The member under the caret in `var.member` (Twig) or `$var->member` / `$this->member` (PHP), for typed variables,
/// and the byte range of its name.
fn member_at(index: &Index, doc: &Document, offset: usize, load: &dyn Fn(&str) -> Option<String>) -> Option<(php_types::Member, std::ops::Range<usize>)> {
    let (name, members, range) = match doc.language.as_str() {
        "twig" => {
            let (chain, start, end) = refs::twig_member_chain_at(&doc.text, offset)?;
            let template = index.template_of(&doc.path);
            let value = twig_chain_value(index, doc, template, load, &chain, offset, 0)?;
            (doc.text[start..end].to_owned(), php_types::value_members(load, &value), start..end)
        }
        "php" => {
            let masked = php_text::mask(&doc.text);
            let (receiver, start, end) = php_types::member_access_at(&masked, offset)?;
            let members = if receiver == "$this" {
                php_types::this_members(load, &doc.text)
            } else {
                let class = php_types::receiver_hint_at(&doc.text, offset, &receiver).and_then(|hint| php_types::hint_class(&hint, load, &|id| service_class(index, id, 0)))?;
                php_types::instance_members(load, &class)
            };
            (doc.text[start..end].to_owned(), members, start..end)
        }
        _ => return None,
    };
    if name.is_empty() {
        return None;
    }
    members.into_iter().find(|member| member.name == name).map(|member| (member, range))
}

/// Go to the declaration of a typed member, in Twig and PHP.
fn member_definition(index: &Index, doc: &Document, offset: usize, root: &str) -> Vec<Location> {
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };
    let load = |fqcn: &str| -> Option<String> {
        let entry = index.classes.get(fqcn.trim_start_matches('\\'))?;
        project.read_file(&entry.path).ok()
    };
    let Some((member, _)) = member_at(index, doc, offset, &load) else {
        return Vec::new();
    };
    if doc.language == "php" && php_text::class_model(&doc.text).is_some_and(|model| model.fqcn == member.owner) {
        return vec![Location { path: doc.path.clone(), range: Range::new(text::position(&doc.text, member.range.start), text::position(&doc.text, member.range.end)) }];
    }
    located(index, &member, root, &load)
}

/// Hover with the type of a typed member or variable, in Twig and PHP.
fn type_hover(index: &Index, doc: &Document, offset: usize) -> Option<openidea_plugin::hover::Hover> {
    let project = project_for(&doc.path)?;
    let load = |fqcn: &str| -> Option<String> {
        let entry = index.classes.get(fqcn.trim_start_matches('\\'))?;
        project.read_file(&entry.path).ok()
    };
    let escaped = |value: &str| value.replace('\\', "\\\\").replace(['`', '*', '_', '<', '>'], "");
    let label = |value: &php_types::Value| match value {
        php_types::Value::Object(class) => class.clone(),
        php_types::Value::List(class) => format!("{class}[]"),
        php_types::Value::Form(form) => format!("form view of {form}"),
        php_types::Value::Field => "form field view".to_owned(),
        php_types::Value::Vars => "form.vars".to_owned(),
    };

    if let Some((member, range)) = member_at(index, doc, offset, &load).filter(|(member, _)| !member.owner.is_empty()) {
        let signature = match (&member.type_name, &member.element_type) {
            (_, Some(element)) => format!("{} : {element}[]", member.detail),
            (Some(type_name), None) => format!("{} : {type_name}", member.detail),
            (None, None) => member.detail.clone(),
        };
        let body = format!("**{}**\n\n`{}`\n\nInferred from project sources; the runtime type may differ.", escaped(&member.owner), escaped(&signature));
        return Some(openidea_plugin::hover::Hover::new(body).range(range_of(&doc.text, range.start, range.end)));
    }

    let (name, start, end, value) = match doc.language.as_str() {
        "twig" => {
            let (start, end) = refs::twig_variable_at(&doc.text, offset)?;
            let name = doc.text[start..end].to_owned();
            let template = index.template_of(&doc.path);
            let value = twig_value(index, doc, template, &load, &name, offset, 0)?;
            (name, start, end, value)
        }
        "php" => {
            let masked = php_text::mask(&doc.text);
            let bytes = masked.as_bytes();
            let (mut start, mut end) = (offset, offset);
            while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_') {
                start -= 1;
            }
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if start == 0 || bytes[start - 1] != b'$' || start == end {
                return None;
            }
            let name = doc.text[start..end].to_owned();
            let hint = php_types::receiver_hint_at(&doc.text, offset, &format!("${name}"))?;
            (name, start, end, php_types::hint_value(&hint, &load, &|id| service_class(index, id, 0))?)
        }
        _ => return None,
    };
    let body = format!("**{}**: `{}`\n\nInferred from project sources; the runtime type may differ.", escaped(&name), escaped(&label(&value)));
    Some(openidea_plugin::hover::Hover::new(body).range(range_of(&doc.text, start, end)))
}

/// The declaration of a member in its owning class's file, when the file belongs to `root`.
fn located(index: &Index, member: &php_types::Member, root: &str, load: &dyn Fn(&str) -> Option<String>) -> Vec<Location> {
    let Some(entry) = index.classes.get(member.owner.trim_start_matches('\\')) else {
        return Vec::new();
    };
    if entry.path.is_empty() || host::project_of(&entry.path).and_then(|project| project.root().ok()).as_deref() != Some(root) {
        return Vec::new();
    }
    let Some(source) = load(&member.owner) else {
        return Vec::new();
    };
    vec![Location { path: entry.path.clone(), range: Range::new(text::position(&source, member.range.start), text::position(&source, member.range.end)) }]
}

/// The innermost `->add(` call whose array argument holds `offset`: the field type class named by
/// its second argument (when written as `X::class`), found in masked PHP.
fn form_options_context(text: &str, masked: &str, offset: usize) -> Option<Option<String>> {
    let bytes = masked.as_bytes();
    let (mut depth, mut array, mut at) = (0usize, false, offset);
    while at > 0 {
        at -= 1;
        match bytes[at] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'[' => array = true,
            b'(' => {
                if !array || !masked[..at].trim_end().ends_with("->add") {
                    return None;
                }
                let arguments = &text[at + 1..];
                let mut top = 0usize;
                let mut commas = Vec::new();
                for (position, byte) in masked[at + 1..offset].bytes().enumerate() {
                    match byte {
                        b'(' | b'[' | b'{' => top += 1,
                        b')' | b']' | b'}' => top = top.saturating_sub(1),
                        b',' if top == 0 => commas.push(position),
                        _ => {}
                    }
                }
                if commas.len() != 2 {
                    return None;
                }
                let second = arguments[commas[0] + 1..commas[1]].trim();
                let class = second.strip_suffix("::class").map(|name| php_text::resolver(text)(name));
                return Some(class);
            }
            b'{' => return None,
            _ => {}
        }
    }
    None
}

/// Form field names in `$form->get('|')` / `has` / `remove`, and option names in `$builder->add('x', Type::class, ['|'])`.
fn form_completions(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "php" {
        return Vec::new();
    }

    let line_start = doc.text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let prefix = &doc.text[line_start..offset];
    let Some(quote) = prefix.rfind(['\'', '"']) else { return Vec::new() };
    let partial = &prefix[quote + 1..];
    if !partial.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) || prefix[..quote].matches(prefix[quote..].chars().next().unwrap_or('\'')).count() % 2 == 1 {
        return Vec::new();
    }
    let start = offset - partial.len();
    let replace = range_of(&doc.text, start, offset);
    let before = prefix[..quote].trim_end();

    if let Some(options_type) = form_options_context(&doc.text, &php_text::mask(&doc.text), offset) {
        if before.ends_with(['[', ',']) {
            return forms::options(options_type.as_deref().unwrap_or_default())
                .into_iter()
                .map(|name| {
                    let mut item = CompletionItem::new(name, CompletionKind::Property);
                    item.detail = Some("form option".into());
                    item.replace = Some(replace);
                    item
                })
                .collect();
        }
        return Vec::new();
    }

    let Some(call) = ["->get(", "->has(", "->remove("].iter().find_map(|call| before.strip_suffix(call)) else {
        return Vec::new();
    };
    let length = call.bytes().rev().take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'>' | b'-')).count();
    let receiver = &call[call.len() - length..];
    let Some(php_types::TypeHint::Form(form)) = php_types::receiver_hint_at(&doc.text, offset, receiver) else {
        return Vec::new();
    };
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };
    let Some(source) = index.classes.get(form.trim_start_matches('\\')).and_then(|entry| project.read_file(&entry.path).ok()) else {
        return Vec::new();
    };

    forms::fields(&source)
        .into_iter()
        .map(|field| {
            let mut item = CompletionItem::new(&field.name, CompletionKind::Field);
            item.detail = field.type_name.as_deref().map(|name| php_text::short_name(name).to_owned());
            item.replace = Some(replace);
            item
        })
        .collect()
}

/// The `{placeholder}` names of a route path: `/blog/{slug}/{page<\\d+>?1}` has `slug` and `page`.
fn route_placeholders(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    for (open, _) in path.match_indices('{') {
        let rest = path[open + 1..].trim_start_matches('!');
        let name = &rest[..rest.find(['}', '<', '?', '>']).unwrap_or(rest.len())];
        if !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') && !names.contains(&name.to_owned()) {
            names.push(name.to_owned());
        }
    }
    names
}

/// Route parameter names in the argument array of `path()`, `url()`, `generateUrl()`, `redirectToRoute()` and `->generate()`.
fn route_parameter_completions(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    let Some((route, start, end)) = refs::route_parameter_at(&doc.language, &doc.text, offset) else {
        return Vec::new();
    };
    let replace = range_of(&doc.text, start, end);
    let mut seen = BTreeSet::new();
    index
        .route_defs
        .iter()
        .filter(|def| def.name.as_deref() == Some(route.as_str()))
        .flat_map(|def| route_placeholders(&def.path))
        .filter(|name| seen.insert(name.clone()))
        .map(|name| {
            let mut item = CompletionItem::new(&name, CompletionKind::Property);
            item.detail = Some(format!("route parameter · {route}"));
            item.replace = Some(replace);
            item
        })
        .collect()
}

/// UX component props (attribute names of `<twig:Name …>`) and block names (`<twig:block name="…">`).
fn component_completions(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }
    let Some(context) = refs::component_context(&doc.text, offset) else {
        return Vec::new();
    };

    let (names, detail, start, end): (Vec<&String>, &str, usize, usize) = match &context {
        refs::ComponentContext::Prop { component, start, end } => match index.component_info.get(component) {
            Some(info) => (info.component.props.iter().collect(), "component prop", *start, *end),
            None => return Vec::new(),
        },
        refs::ComponentContext::Block { component, start, end } => match index.component_info.get(component) {
            Some(info) => (info.blocks.iter().collect(), "component block", *start, *end),
            None => return Vec::new(),
        },
    };
    let replace = range_of(&doc.text, start, end);
    names
        .into_iter()
        .map(|name| {
            let mut item = CompletionItem::new(name, CompletionKind::Property);
            item.detail = Some(detail.into());
            item.replace = Some(replace);
            item
        })
        .collect()
}

fn member_items(members: Vec<php_types::Member>, replace: Range) -> Vec<CompletionItem> {
    members
        .into_iter()
        .map(|member| {
            let mut item = CompletionItem::new(&member.name, if member.detail.ends_with("()") { CompletionKind::Method } else { CompletionKind::Property });
            item.detail = Some(match &member.type_name {
                Some(type_name) => format!("{} \u{b7} {}", member.detail, php_text::short_name(type_name)),
                None => member.detail.clone(),
            });
            item.replace = Some(replace);
            item
        })
        .collect()
}

/// Members of `$object->` and `$this->property->` in PHP, for variables the source types
/// (typed parameters, `new`, repository and entity-manager finders, `container->get(X::class)`).
fn php_members(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "php" {
        return Vec::new();
    }

    let masked = php_text::mask(&doc.text);
    let Some((receiver, start, end)) = php_types::member_access_at(&masked, offset) else {
        return Vec::new();
    };
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };
    let load = |fqcn: &str| -> Option<String> {
        let entry = index.classes.get(fqcn.trim_start_matches('\\'))?;
        project.read_file(&entry.path).ok()
    };
    if receiver == "$this" {
        return member_items(php_types::this_members(&load, &doc.text), range_of(&doc.text, start, end));
    }
    let Some(class) = php_types::receiver_hint_at(&doc.text, offset, &receiver).and_then(|hint| php_types::hint_class(&hint, &load, &|id| service_class(index, id, 0))) else {
        return Vec::new();
    };

    member_items(php_types::instance_members(&load, &class), range_of(&doc.text, start, end))
}

/// Twig tags, filters, functions and tests, plus variables where an expression can start.
fn twig_names(index: &Index, doc: &Document, offset: usize) -> Vec<CompletionItem> {
    if doc.language != "twig" {
        return Vec::new();
    }

    if let Some((template, typed)) = twig::macro_import_at(&doc.text, offset) {
        return imported_macros(index, doc, &template, typed);
    }

    let Some((slot, start, end)) = twig::slot_at(&doc.text, offset) else {
        return Vec::new();
    };
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

fn imported_macros(index: &Index, doc: &Document, template: &str, typed: std::ops::Range<usize>) -> Vec<CompletionItem> {
    let source = match template {
        "_self" => Some(doc.text.clone()),
        name => index.templates.get(name).and_then(|entry| project_for(&doc.path)?.read_file(&entry.path).ok()),
    };
    let replace = range_of(&doc.text, typed.start, typed.end);

    twig::macro_definitions(&source.unwrap_or_default())
        .into_iter()
        .map(|(name, arguments)| {
            let mut item = CompletionItem::new(&name, CompletionKind::Function);
            item.detail = Some(format!("macro {name}({arguments})"));
            item.replace = Some(replace);
            item
        })
        .collect()
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
fn source_service_duplicates(path: &str, language: &str, text: &str) -> Option<Vec<checks::Finding>> {
    if language != "xml" || path.contains("/var/cache/") { return None; }
    let services = source_xml::service_ids(text)?;
    let mut seen = BTreeSet::new();
    Some(services.into_iter().filter_map(|(id, range)| {
        (!seen.insert(id.clone())).then(||checks::Finding::warn(range,"duplicate-service",format!("Service \"{id}\" is already defined in this file.")))
    }).collect())
}

/// Compiled routes are authoritative. Otherwise every route must be literal in the
/// scanned sources, the name a plain one (not `_` internal, FQCN or `Class::method`
/// alias) and the call a URL generator's.
fn route_is_missing(index: &Index, text: &str, start: usize, name: &str) -> bool {
    if index.compiled_routes {
        return true;
    }
    if !index.routes_complete || name.starts_with('_') || name.contains(['\\', ':']) {
        return false;
    }

    let before = &text[..start];
    let call = before.trim_end_matches(['\'', '"']).trim_end_matches('(');
    call.ends_with("generateUrl") || call.ends_with("redirectToRoute") || {
        let receiver = call.strip_suffix("->generate").unwrap_or("").to_ascii_lowercase();
        receiver.ends_with("router") || receiver.ends_with("urlgenerator")
    }
}

pub fn problems(index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
    if language == "twig" && index.custom_twig_lexer() {
        return Vec::new();
    }
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
        if language=="twig" && matches!(reference.kind,Kind::Template|Kind::Translation) {continue;}
        let name = &text[reference.start..reference.end];
        if is_dynamic(name) {
            continue;
        }

        match reference.kind {
            Kind::Route if language != "twig" && !index.routes.contains_key(name) && route_is_missing(index, text, reference.start, name) => {
                problem(reference.start, reference.end, "missing-route", format!("Route \"{name}\" is not defined."));
            }
            Kind::Template
                if !name.starts_with('@')
                    && !index.template_paths_unknown
                    && !index.ambiguous_templates.contains(name)
                    && !index.templates.contains_key(name) =>
            {
                problem(reference.start, reference.end, "missing-template", format!("Template \"{name}\" does not exist in templates/."));
            }
            Kind::Service if index.compiled_container && !index.services.contains_key(name) => {
                problem(reference.start, reference.end, "missing-service", format!("Service \"{name}\" is not defined."));
            }
            Kind::Translation => {
                let Some(domain) = domain_of(language, text, &reference) else {continue;};
                if index.translations.get(&domain).is_some_and(|keys| !keys.contains_key(name)) {
                    let message = format!("Translation key \"{name}\" is missing from domain \"{domain}\".");
                    problem(reference.start, reference.end, "missing-translation", message);
                }
            }
            _ => {}
        }
    }

    if language=="twig" {
        for finding in twig_checks::source_references(index,text) {problem(finding.start,finding.end,finding.code,finding.message);}
    }
    if let Some(findings) = source_service_duplicates(path, language, text) {
        for finding in findings { problem(finding.start, finding.end, finding.code, finding.message); }
    } else if language == "xml" && !path.contains("/var/cache/") {
        let mut services: Vec<(String, Position)> = Vec::new();
        index::xml_services(path, text, &mut services);
        for (i, (id, at)) in services.iter().enumerate() {
            if services[..i].iter().any(|(earlier, _)| earlier == id) {
                let start = text::offset(text, *at).unwrap_or(0);
                problem(start, start + id.len(), "duplicate-service", format!("Service \"{id}\" is already defined in this file."));
            }
        }
    }

    if let Some(findings) = route_duplicates::check(path, language, text) {
        for finding in findings { problem(finding.start, finding.end, finding.code, finding.message); }
    } else {
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
    let groups = if language == "twig" && index.custom_twig_lexer() { Groups { twig: false, twig_syntax: false, ..groups } } else { groups };

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
        findings.extend(wiring::autowire_service(&ctx, language, text));
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
        findings.extend(orm_links::check(&ctx, language, text));
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

    if groups.twig && language == "twig" && !index.custom_twig_lexer() {
        let read = |_: &str| None;
        found.extend(twig_checks::names(&checks::Context::new(index, &read), text));
    }

    if groups.deprecations && language == "php" {
        let read = |_: &str| None;
        found.extend(php_checks::deprecations(&checks::Context::new(index, &read), text));
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

struct Window {
    routes: RouteList,
}

impl Window {
    fn new() -> Window {
        Window { routes: RouteList::new(ROUTES_KEY, "aritti.symfony") }
    }
}

#[derive(Default)]
pub struct Main {
    profiler_snapshot: profiler::Snapshot,
    processes: processes::Processes,
    namespace_editor: namespace_paths::Editor,
    template_creation: file_actions::Creation,
    service_registration: service_file_actions::Registration,
    service_edits: service_edit_actions::Edits,
    config_creation: config_file_actions::Creation,
    compiler_creation: composer_compiler::Creation,
    window: Option<Window>,
    /// Key of the last selected row of the tool window.
    selected: Option<String>,
    route_owner: Option<String>,
    route_selection: Option<(String, Route)>,
    route_requests: route_request::State,
    console: console_workspace::State,
    runtime: runtime::Runtime,
    database: database::State,
    /// By project root.
    indexes: BTreeMap<String, Index>,
    /// Files with published problems, checked again when the index changes.
    checked: BTreeSet<String>,
    /// The Events tab shows a flat list instead of grouping by event.
    events_flat: bool,
    /// Whether the Run console colours are set.
    console_rules: bool,
}

fn concrete_method(route: &Route) -> Route {
    let mut route = route.clone();
    if route.method == "ANY" {
        route.method = "GET".into();
    }
    route
}

/// A route with one method: a lens route that lists several asks which one; `None` when dismissed.
fn one_method(route: Route) -> Result<Option<Route>, Error> {
    if !route.method.contains('|') {
        return Ok(Some(concrete_method(&route)));
    }
    let items: Vec<PickItem> = route.method.split('|').map(|method| PickItem::new(method, method)).collect();
    let Some(method) = host::pick("HTTP method", &items)? else {
        return Ok(None);
    };
    if !route.method.split('|').any(|known| known == method) {
        return Err(Error::Stale);
    }
    Ok(Some(Route { method, ..route }))
}

/// The Symfony status bar item's menu: the commands used most while working on the project.
fn status_menu() -> Vec<StatusMenuEntry> {
    vec![
        StatusMenuEntry::action("Run Console Command…", "run-console-command"),
        StatusMenuEntry::action("Show Console", "show-console"),
        StatusMenuEntry::action("Clear Cache", "cache-clear"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Go to Route…", "go-to-route"),
        StatusMenuEntry::action("Go to Service…", "go-to-service"),
        StatusMenuEntry::action("Open Route in Request Tab…", "route-send-request"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Show Profiler", "show-profiler"),
        StatusMenuEntry::action("Reindex Project", "reindex"),
    ]
}

/// The title of the route picker of route command `command`.
fn route_pick_title(command: &str) -> &'static str {
    match command {
        "route-copy-url" => "Copy URL of which route?",
        "route-copy-curl" => "Copy cURL of which route?",
        "route-create-http" => "Create a request file for which route?",
        _ => "Open which route in the Request tab?",
    }
}

/// Asks for the base URL and copies it with the route path.
fn copy_url(route: &Route) -> Result<(), Error> {
    let Some(base) = host::input("Base URL", Some(DEFAULT_BASE_URL))?.map(|base| base.trim().to_owned()).filter(|base| !base.is_empty()) else {
        return Ok(());
    };
    match host::copy_to_clipboard(&format!("{}{}", base.trim_end_matches('/'), route.path)) {
        Ok(()) => host::notify(NotificationLevel::Info, SOURCE, "Copied the URL."),
        Err(Error::Denied(_)) => host::notify(NotificationLevel::Warning, SOURCE, &ui_kit::denied::message("Copy URL", "clipboard", "Symfony")),
        Err(error) => return Err(error),
    }
    Ok(())
}

impl Main {
    fn index_project(&mut self, project: &Project) {
        let Ok(root) = project.root() else { return };

        if !index::is_symfony(project) {
            let removed = self.indexes.remove(&root).is_some();
            self.update_status_item(&root);
            if removed { refresh_main_view(); }
            return;
        }

        let index = Index::build(project);
        if index.partial {
            host::log(LogLevel::Warn, &format!("index of {root} is partial: discovery, read or time limit"));
        }
        self.indexes.insert(root.clone(), index);
        self.update_status_item(&root);

        let checked: Vec<String> = self.checked.iter().filter(|path| under(path, &root)).cloned().collect();
        for path in checked {
            self.check(project, &path);
        }
        refresh_main_view();
    }

    fn update_status_item(&mut self, root: &str) {
        self.show_console_rules(!self.indexes.is_empty());
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
        if host::set_status_item_menu(STATUS_ITEM, &item, &status_menu()).is_err() {
            host::set_status_item(STATUS_ITEM, &item);
        }
    }

    fn index_all(&mut self) {
        let projects = host::projects().unwrap_or_default();
        let roots: BTreeSet<_> = projects.iter().filter_map(|project| project.root().ok()).collect();
        let before = self.indexes.len();
        self.indexes.retain(|root, _| roots.contains(root));
        if self.indexes.len() != before { refresh_main_view(); }
        self.checked.retain(|path| roots.iter().any(|root| under(path, root)));
        for project in projects {
            self.index_project(&project);
        }
    }

    fn index_for(&self, path: &str) -> Option<&Index> {
        let root = host::project_of(path)?.root().ok()?;
        self.indexes.get(&root).filter(|index|index.settings_current())
    }

    /// Publishes the problems of a file (none when they are switched off).
    fn check(&mut self, project: &Project, path: &str) {
        let Some(language) = language_of(path) else {
            return;
        };
        let Some(index) = self.index_for(path) else {
            return;
        };

        let enabled = host::setting_bool("diagnostics").unwrap_or(true);
        let found = match project.read_file(path) {
            Ok(text) if enabled && !index.partial => {
                let read = |file: &str| project.read_file(file).ok();
                let source_xml_owned = source_service_duplicates(path, language, &text).is_some();
                let source_factory_owned=config_checks::source_factory_settings(path,language,&text).is_some();
                let source_route_settings_owned=checks::source_route_deprecations(path,language,&text,None).is_some();
                let source_routes_owned = route_duplicates::check(path, language, &text).is_some();
                let source_classes_owned = index.psr4_source_verified && checks::source_service_classes(path, language, &text).is_some();
                let mut groups = Groups::from_settings();
                let mut found: Vec<_> = problems(index, path, language, &text).into_iter()
                    .filter(|finding| match finding.code.as_deref() {
                        Some("duplicate-service") => !source_xml_owned && groups.services,
                        Some("duplicate-route") => !source_routes_owned && groups.controllers,
                        Some("missing-template"|"missing-translation") if language=="twig" => false,
                        _ => true,
                    }).collect();
                // These source-local groups are owned by the versioned current-document provider.
                groups.twig_syntax = false;
                groups.yaml_syntax = false;
                groups.commands = false;
                groups.practices = false;
                groups.deprecations = false;
                groups.doctrine = false;
                if language == "php" { groups.controllers = false; }
                // Keep source-reading checks here; migrated codes/scopes have one current-document owner.
                found.extend(extra_problems(index, groups, &read, path, language, &text).into_iter()
                    .filter(|finding| {
                        !(source_classes_owned && finding.code.as_deref()==Some("service-class-missing"))
                            && !(source_factory_owned && finding.code.as_deref()==Some("deprecated-factory-setting"))
                            && !(source_route_settings_owned && finding.code.as_deref()==Some("deprecated-route-setting"))
                            && !matches!(finding.code.as_deref(), Some(
                                "unknown-parameter" | "duplicate-parameter-key"
                                | "form-type-string" | "missing-asset"
                                | "missing-translation-domain" | "twig-missing-route" | "twig-missing-asset"
                                | "twig-route-compare" | "twig-missing-translation-domain"
                                | "twig-assets-tag-missing" | "twig-path-replacement"
                            ))
                    }));
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

        let output = console_runner::run(&project, &args)?;

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
        let listed = {
            let _task = host::begin_task("Listing console commands", false);
            console_runner::run(&project, &args).ok().filter(|o| o.status == Some(0))
        };

        let mut descriptions: BTreeMap<String, Option<String>> = BTreeMap::new();
        for line in listed.iter().flat_map(|o| o.stdout.lines()) {
            let mut parts = line.trim().splitn(2, char::is_whitespace);
            let Some(name) = parts.next().filter(|n| !n.is_empty()) else {
                continue;
            };
            descriptions.insert(name.to_owned(), parts.next().map(|d| d.trim().to_owned()));
        }
        if let Some(index) = self.view_index(ctx) {
            for name in index.commands.keys() {
                descriptions.entry(name.clone()).or_default();
            }
        }
        if descriptions.is_empty() {
            return Err("No console commands found. Allow process:php in Settings › Plugins › Symfony or define a command in the project.".into());
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
        let Some(name) = host::pick("Run console command", &items)? else {
            return Ok(None);
        };
        let Some(arguments) = host::input(&format!("Arguments for bin/console {name}"), None)? else {
            return Ok(None);
        };

        let mut args = vec!["bin/console".to_owned(), name.clone()];
        args.extend(shell_words::split(&arguments).map_err(|_| Error::Invalid("Unbalanced quote in the arguments.".into()))?);
        console_runner::open_terminal(&project, &format!("console {name}"), &args).map(|()| None)
    }

    fn go_to(&self, ctx: &Invocation, what: &str, select: impl Fn(&Index) -> &Names) -> Result<Option<String>, Error> {
        let root = ctx.project().and_then(|project| project.root().ok()).ok_or(Error::Stale)?;
        let index = self.indexes.get(&root).filter(|index|index.settings_current()).ok_or("No Symfony project is indexed.")?;
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
        let Some(key) = host::pick(&format!("Go to {what}"), &items)? else {
            return Ok(None);
        };
        let entry = names.get(&key).ok_or_else(|| format!("{key} is no longer indexed."))?;
        if entry.path.is_empty() {
            return Err(format!("{key} is defined by the framework, not by a project file.").into());
        }

        if !self.current_navigation_target(ctx, &root, &entry.path) { return Err(Error::Stale); }
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
        let Some(parent) = host::pick("Extend template", &items)? else {
            return Ok(None);
        };

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
        if !i.settings_current() {return format!("{root}: source settings changed; Reindex to apply them.");}
        let partial = match (i.partial, i.template_directories.is_empty()) {
            (true, _) => " (index incomplete)",
            (false, false) => " (templates in gitignored folders may not be listed)",
            _ => "",
        };
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
        self.profiler_snapshot = profiler::Snapshot::default();
        for kind in [EventKind::FileSaved, EventKind::FileOpened, EventKind::FilesChanged] {
            match host::subscribe(kind) {
                Err(error) => host::log(LogLevel::Warn, &format!("Symfony problems need the {} event: {error}", kind.name())),
                Ok(()) if kind == EventKind::FilesChanged => self.database.cache_schemas(true),
                Ok(()) => {}
            }
        }
        let _ = host::subscribe(EventKind::TerminalExited);
        self.index_all();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        self.reap_overdue();
        let lens = arg.filter(|arg| route_lenses::is_target(arg));
        match id {
            "find-source-occurrences" => source_occurrences::run(self,ctx),
            "twig-paths" => {
                ui_kit::web::reveal_view(namespace_paths::VIEW, include_str!("ui_config.json"))?;
                Ok(None)
            }
            "reindex" => {
                let _task = host::begin_task("Indexing Symfony projects", false);
                self.index_all();
                host::notify(NotificationLevel::Info, SOURCE, &self.summary());
                Ok(None)
            }
            "runtime-open-frame" | "runtime-retry-failed" | "runtime-remove-failed" | "runtime-failed-messages" => {
                let result = match id {
                    "runtime-open-frame" => self.runtime.open_frame(ctx, arg),
                    "runtime-retry-failed" => self.runtime.retry(ctx, arg),
                    "runtime-remove-failed" => self.runtime.remove(ctx, arg),
                    _ => self.runtime.inspect_failed(ctx),
                };
                if id == "runtime-failed-messages" {
                    ui_kit::web::reveal_view(runtime::VIEW, include_str!("ui_config.json"))?;
                } else {
                    ui_kit::web::refresh_view(runtime::VIEW, include_str!("ui_config.json"));
                }
                result
            }
            "show-profiler" => {
                self.profiler_snapshot.refresh(ctx);
                ui_kit::web::reveal_view(PROFILER_VIEW, include_str!("ui_config.json"))?;
                Ok(None)
            }
            "cache-warmup" => self.console(ctx, "cache:warmup"),
            "cache-clear" => self.console(ctx, "cache:clear"),
            "run-console-command" => if self.console.pick_into(ctx, console_workspace::VIEW, "Run console command")? { Ok(None) } else { self.run_console_command(ctx) },
            "show-console" => {
                ui_kit::web::reveal_view(console_workspace::VIEW, include_str!("ui_config.json"))?;
                Ok(None)
            }
            "database-migrate" | "database-reset" | "database-fixtures" => {
                let result = self.database.change(ctx, id);
                ui_kit::web::refresh_view(database::VIEW, include_str!("ui_config.json"));
                result.map(|()| None)
            }
            "database-refresh" => {
                self.database.compare(ctx)?;
                ui_kit::web::reveal_view(database::VIEW, include_str!("ui_config.json"))?;
                Ok(None)
            }
            "database-add-source" => database_sources::add(ctx).map(|()| None),
            "database-sources" => database_sources::manage(ctx).map(|()| None),
            "docker-choose-target" | "docker-clear-target" => {
                let result = if id == "docker-choose-target" { docker_target::choose(ctx) } else { docker_target::clear(ctx) };
                ui_kit::web::refresh_view(processes::VIEW, include_str!("ui_config.json"));
                result
            }
            "route-copy-curl" => {
                if let Some(route) = self.command_route(ctx, lens, id)?.map(one_method).transpose()?.flatten() {
                    routes::copy_as_curl(&route, DEFAULT_BASE_URL, "Symfony")?;
                }
                Ok(None)
            }
            "route-create-http" => {
                if let Some(route) = self.command_route(ctx, lens, id)?.map(one_method).transpose()?.flatten() {
                    routes::create_http_file(&route, DEFAULT_BASE_URL, None, "Symfony")?;
                }
                Ok(None)
            }
            "route-copy-url" => {
                if let Some(route) = self.command_route(ctx, lens, id)? {
                    copy_url(&route)?;
                }
                Ok(None)
            }
            "route-send-request" => {
                let Some(route) = self.command_route(ctx, lens, id)? else { return Ok(None) };
                self.open_in_request_tab(ctx, route)?;
                Ok(None)
            }
            "route-actions" => {
                if let Some(raw) = lens {
                    route_lenses::resolve(ctx, raw)?;
                }
                let items: Vec<PickItem> = route_lenses::ROUTE_ACTIONS.iter().map(|(command, label)| PickItem::new(command, label)).collect();
                let Some(choice) = host::pick("Run which route action?", &items)? else { return Ok(None) };
                let (command, _) = route_lenses::ROUTE_ACTIONS.iter().find(|(command, _)| *command == choice).ok_or(Error::Stale)?;
                self.run_command(command, ctx, lens)
            }
            "go-to-route" if lens.is_some() => {
                let route = route_lenses::resolve(ctx, lens.unwrap_or_default())?;
                host::open_location(&route.file, route.at.line, route.at.character)?;
                Ok(None)
            }
            "go-to-route" => self.go_to(ctx, "route", |i| &i.routes),
            "go-to-service" => self.go_to(ctx, "service", |i| &i.services),
            "go-to-command" => self.go_to(ctx, "console command", |i| &i.commands),
            "go-to-template" => self.go_to(ctx, "template", |i| &i.templates),
            "twig-extends" => self.twig_extends(ctx),
            "make" => if self.console.pick_into(ctx, console_workspace::GENERATORS_VIEW, "Symfony: Make")? { Ok(None) } else { self.make(ctx) },
            "create-template" => self.create_template(ctx),
            "unused-templates" => unused_templates::run(self, ctx),
            "run-command-from-class" => command_from_class::run(self, ctx),
            "new-project" => new_project::run(),
            "twig-form-fields" => self.twig_form_fields(ctx),
            "component-open-template" | "component-open-class" | "component-copy-tag" => browse_tabs::component_command(id, self.view_index(ctx), arg),
            browse_tabs::EVENTS_MODE => {
                self.events_flat = !self.events_flat;
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.reap_overdue();
        new_project::handle_events(events);
        let projects = host::projects().unwrap_or_default();
        let roots: BTreeSet<_> = projects.iter().filter_map(|project| project.root().ok()).collect();
        // Do not retain stale facts after an oversized batch or perform unbounded callbacks.
        let mut changed = BTreeSet::new();
        let mut added_or_removed = BTreeSet::new();
        let mut opened = BTreeSet::new();
        let mut count = 0usize;
        for event in events.iter().take(1025) {
            let (paths, is_open): (Box<dyn Iterator<Item = &str>>, bool) = match event {
                Event::FileSaved(path) => (Box::new(std::iter::once(path.as_str())), false),
                Event::FileOpened(path) => (Box::new(std::iter::once(path.as_str())), true),
                Event::FilesChanged(files) => {
                    added_or_removed.extend(files.created.iter().chain(&files.removed).map(String::as_str));
                    (Box::new(files.created.iter().chain(&files.modified).chain(&files.removed).map(String::as_str)), false)
                }
                _ => continue,
            };
            for path in paths.take(1025) {
                count += 1;
                if count > 1024 || path.len() > 4096 {
                    let changed = !self.indexes.is_empty();
                    self.indexes.clear();
                    self.checked.clear();
                    self.database.forget_schemas();
                    if changed { refresh_main_view(); }
                    if self.profiler_snapshot.invalidate_changes(&BTreeSet::new(), &roots, true) { refresh_profiler_view(); }
                    return;
                }
                if is_open {
                    opened.insert(path);
                } else {
                    changed.insert(path);
                }
            }
        }
        if events.len() > 1024 {
            let changed = !self.indexes.is_empty();
            self.indexes.clear();
            self.checked.clear();
            self.database.forget_schemas();
            if changed { refresh_main_view(); }
            if self.profiler_snapshot.invalidate_changes(&BTreeSet::new(), &roots, true) { refresh_profiler_view(); }
            return;
        }
        self.database.files_changed(changed.iter().copied());
        let configured_inputs = index::configured_input_paths();
        if self.profiler_snapshot.invalidate_changes(&changed, &roots, false) { refresh_profiler_view(); }
        let before = self.indexes.len();
        self.indexes.retain(|root, _| roots.contains(root));
        if self.indexes.len() != before { refresh_main_view(); }
        self.checked.retain(|path| roots.iter().any(|root| under(path, root)));
        for project in projects {
            let Ok(root) = project.root() else {
                continue;
            };
            let directories = self.indexes.get(&root).filter(|index|index.settings_current()).map(|index| index.template_directories.clone()).unwrap_or_default();
            let relevant: Vec<&str> = changed
                .iter()
                .filter(|path| {
                    path.strip_prefix(&format!("{root}/")).is_some_and(|relative| affects_index(relative) || index::affects_configured_index(relative, &configured_inputs)
                        || namespace_paths::affects(relative, &directories))
                })
                .copied()
                .collect();
            let unknown = !self.indexes.contains_key(&root) && opened.iter().any(|path| under(path, &root));
            let settings_changed=self.indexes.get(&root).is_some_and(|index|!index.settings_current());
            let mut full_build = unknown || settings_changed;
            let mut updated = false;
            if !full_build && !relevant.is_empty() {
                match self.indexes.get_mut(&root) {
                    None => full_build = true,
                    Some(index) => {
                        for path in &relevant {
                            let relative = &path[root.len() + 1..];
                            if added_or_removed.contains(path) || !index.update_file(&project, &root, relative) {
                                full_build = true;
                                break;
                            }
                            updated = true;
                        }
                    }
                }
            }
            if full_build {
                self.index_project(&project);
            } else if updated {
                refresh_main_view();
            }
            for path in changed.iter().chain(&opened).filter(|path| under(path, &root)) {
                self.check(&project, path);
            }
        }
    }
}

impl Providers for Main {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(root)=host::project_of(&doc.path).and_then(|project|project.root().ok()) else {return Vec::new();};
        let Some(index) = self.indexes.get(&root).filter(|index|index.settings_current()) else {
            return Vec::new();
        };
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };

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

        let form_items = form_completions(index, doc, offset);
        if !form_items.is_empty() {
            return form_items;
        }

        let parameters = route_parameter_completions(index, doc, offset);
        if !parameters.is_empty() {
            return parameters;
        }

        let component_items = component_completions(index, doc, offset);
        if !component_items.is_empty() {
            return component_items;
        }

        let php_members = php_members(index, doc, offset);
        if !php_members.is_empty() {
            return php_members;
        }

        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else {
            let members = twig_members(index, doc, offset);
            if !members.is_empty() {
                return members;
            }
            return twig_names(index, doc, offset);
        };
        let replace = range_of(&doc.text, reference.start, reference.end);

        let nested = nested_roots(&root);
        let items=candidates(index, &doc.path, &doc.language, &doc.text, &reference)
            .iter()
            .filter(|(_,entry)|entry.path.is_empty() || (under(&entry.path, &root) && !nested.iter().any(|inner| under(&entry.path, inner))))
            .map(|(name, entry)| {
                let mut item = CompletionItem::new(name, completion_kind(reference.kind));
                item.detail = entry.detail.clone();
                item.replace = Some(replace);
                item
            })
            .collect();
        if host::project_of(&doc.path).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str()) {return Vec::new();}
        items
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(index) = self.index_for(&doc.path) else { return Vec::new() };
        related::markers(index, &doc.path, &doc.language, &doc.text)
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(root) = host::project_of(&doc.path).and_then(|project|project.root().ok()) else {return Vec::new();};
        let Some(index) = self.indexes.get(&root).filter(|index|index.settings_current()) else {return Vec::new();};
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        let members = member_definition(index, doc, offset, &root);
        if !members.is_empty() {
            return members;
        }

        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else {
            if doc.language != "php" {
                return Vec::new();
            }
            let entry = reference_model::constructor_type_at(&doc.text, offset)
                .and_then(|class| index.services.get(&class).or_else(|| autowired_entry(index, &class)));
            return entry
                .filter(|entry| host::project_of(&entry.path).and_then(|project| project.root().ok()).as_deref() == Some(root.as_str()))
                .and_then(|entry| location(entry, 0))
                .into_iter()
                .collect();
        };
        if reference.kind == Kind::Tag {
            return Vec::new();
        }

        let name = &doc.text[reference.start..reference.end];
        let names = candidates(index, &doc.path, &doc.language, &doc.text, &reference);
        let Some(entry) = names.get(name).or_else(|| (reference.kind == Kind::Service).then(|| autowired_entry(index, name)).flatten()) else {
            return Vec::new();
        };

        // Recheck selected logical ownership without discovery or declaration reads.
        if entry.path.is_empty()
            || host::project_of(&doc.path).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str())
            || host::project_of(&entry.path).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str()) {return Vec::new();}
        let is_file = matches!(reference.kind, Kind::Template | Kind::Class | Kind::Asset | Kind::Stimulus | Kind::Component);
        let width = if is_file { 0 } else { name.encode_utf16().count() as u32 };
        location(entry, width).into_iter().collect()
    }

    /// Quick fixes (quote YAML scalars, apply check replacements, add a missing translation key) and refactorings (`intentions.rs`).
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(index) = self.index_for(&doc.path) else {
            return Vec::new();
        };
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

        let Some(offset) = text::offset(&doc.text, selection.start) else {
            return actions;
        };
        let selection_end = text::offset(&doc.text, selection.end).unwrap_or(offset);

        let intentions = match doc.language.as_str() {
            "php" => {
                let mut php = intentions::php_actions(&doc.text, offset, index.framework_version);
                php.extend(rewrites::php_rewrites(&doc.text, offset, &|class| index.classes.contains_key(class)));
                php.extend(validation_constraints::actions(&doc.text, offset));
                php
            }
            "twig" if intentions::caret_on_extends(&doc.text, offset) => {
                let blocks: Vec<String> = project_for(&doc.path)
                    .map(|project| parent_blocks(index, &project, &doc.text).into_keys().collect())
                    .unwrap_or_default();
                intentions::twig_actions(&doc.text, offset, &blocks)
            }
            _ => Vec::new(),
        };
        actions.extend(intentions.into_iter().map(|action| CodeAction {
            title: action.title,
            kind: Some(action.kind.into()),
            preferred: false,
            edits: vec![(
                    doc.path.clone(),
                    action
                        .edits
                        .into_iter()
                        .map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text })
                        .collect(),
                )],
            data: None,
        }));

        actions.extend(generator_actions(index, doc, offset, selection_end));

        for finding in fixes(index, &doc.path, &doc.language, &doc.text) {
            let Some(replacement) = finding.replacement else {
                continue;
            };
            if finding.start > selection_end || finding.end < offset {
                continue;
            }

            actions.push(CodeAction {
                title: format!("Replace with {replacement}"),
                kind: Some("quickfix".into()),
                preferred: true,
                edits: vec![(
                    doc.path.clone(),
                    vec![TextEdit { range: range_of(&doc.text, finding.start, finding.end), text: replacement }],
                )],
                data: None,
            });
        }

        actions.extend(typo_fixes(index, doc, offset, selection_end));

        let Some(reference) = refs::reference_at(&doc.language, &doc.text, offset) else {
            return actions;
        };

        let key = &doc.text[reference.start..reference.end];
        let Some(domain) = domain_of(&doc.language, &doc.text, &reference) else {return actions;};
        let missing = twig_checks::cached_translation_key(index,&domain,key)==Some(false);
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
            let mut parts=rest.splitn(4, '\n');
            let (Some(file),Some(start),Some(end),Some(key))=(parts.next(),parts.next(),parts.next(),parts.next()) else {return Err(Error::Stale);};
            let (Ok(start),Ok(end))=(start.parse::<usize>(),end.parse::<usize>()) else {return Err(Error::Stale);};
            let index=self.index_for(&doc.path).ok_or(Error::Stale)?;
            let expected=generator_actions(index,doc,start,end).into_iter().any(|candidate|candidate.data.as_deref()==Some(data.as_str()));
            if !expected {return Err(Error::Stale);}
            let existing=self.translation_target(doc,file,key)?;
            action.edits = translation_edits(doc, rest, &existing)?;
            return Ok(action);
        }

        if data.starts_with("service\n") { return Err(Error::Stale); }

        let (file, key) = data.split_once('\n').ok_or("Malformed code action.")?;
        let index=self.index_for(&doc.path).ok_or(Error::Stale)?;
        let domain=index::translation_domain(file);
        let expected=refs::references(&doc.language,&doc.text).into_iter().any(|reference| reference.kind==Kind::Translation
            && doc.text.get(reference.start..reference.end)==Some(key)
            && domain_of(&doc.language,&doc.text,&reference).as_deref()==Some(domain)
            && twig_checks::cached_translation_key(index,domain,key)==Some(false));
        if !expected {return Err(Error::Stale);}
        let text = self.translation_target(doc,file,key)?;

        let end = text::position(&text, text.len());
        let separator = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        let quoted = format!("'{}'", key.replace('\'', "''"));
        let edit = TextEdit { range: Range::new(end, end), text: format!("{separator}{quoted}: {quoted}\n") };

        action.edits = vec![(file.to_owned(), vec![edit])];
        Ok(action)
    }
}

/// Replaces the selected Twig text with a `trans` call and adds the key to the domain's translation file.
fn translation_edits(doc: &Document, data: &str, existing: &str) -> Result<Vec<(String, Vec<TextEdit>)>, Error> {
    let mut parts = data.splitn(4, '\n');
    let (file, start, end, key) = (parts.next(), parts.next(), parts.next(), parts.next());
    let (Some(file), Some(Ok(start)), Some(Ok(end)), Some(key)) = (file, start.map(str::parse::<usize>), end.map(str::parse::<usize>), key)
    else {
        return Err("Malformed code action.".into());
    };
    let value = doc.text.get(start..end).ok_or("The selection changed.")?;
    let (at, line) = generators::translation_line(existing, key, value);
    let replacement = TextEdit { range: range_of(&doc.text, start, end), text: format!("{{{{ '{key}'|trans }}}}") };
    let addition = TextEdit { range: range_of(&existing, at, at), text: line };

    Ok(vec![(doc.path.clone(), vec![replacement]), (file.to_owned(), vec![addition])])
}

/// "Add form fields" in a form type and "Extract translation" for selected Twig text.
fn generator_actions(index: &Index, doc: &Document, offset: usize, selection_end: usize) -> Vec<CodeAction> {
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };

    if doc.language == "php" {
        let read = |file: &str| project.read_file(file).ok();
        let ctx = checks::Context::new(index, &read);
        return generators::form_fields(&doc.text, offset, &ctx)
            .into_iter()
            .map(|action| CodeAction {
                title: action.title,
                kind: Some(action.kind.into()),
                preferred: false,
                edits: vec![(
                    doc.path.clone(),
                    action
                        .edits
                        .into_iter()
                        .map(|(range, text)| TextEdit { range: range_of(&doc.text, range.start, range.end), text })
                        .collect(),
                )],
                data: None,
            })
            .collect();
    }

    if doc.language != "twig" || selection_end <= offset {
        return Vec::new();
    }

    let Some(range) = generators::translatable(&doc.text, offset, selection_end) else {
        return Vec::new();
    };
    let Some(domain) = generators::default_domain(&doc.text) else {return Vec::new();};
    let Some(file) =
        index.translation_files.get(&domain).and_then(|files| files.iter().find(|file| file.contains(".en.")).or(files.first()))
    else {
        return Vec::new();
    };
    let Some(base) = generators::translation_key(index.template_of(&doc.path), &doc.text[range.clone()]) else {
        return Vec::new();
    };

    let taken = |key: &str| index.translations.get(&domain).is_some_and(|keys| keys.contains_key(key));
    let Some(key) = std::iter::once(base.clone()).chain((2..10).map(|n| format!("{base}_{n}"))).find(|key| !taken(key)) else {
        return Vec::new();
    };

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

/// Service definition generators: a new definition for the class at the caret
/// (in the project's service file) and, in a YAML service file, constructor
/// arguments and implied tags for the service at the caret.
fn service_source_actions(index: &Index, doc: &Document, offset: usize, read: checks::Reader) -> Vec<CodeAction> {
    let ctx = checks::Context::new(index, read);
    let mut actions = Vec::new();
    if doc.language == "yaml" && checks::is_service_file(&doc.path) {
        let edit = |range: std::ops::Range<usize>, text: String| {
            vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text }])]
        };

        if let Some((range, text)) = service_gen::resync_arguments(&ctx, &doc.text, offset) {
            actions.push(CodeAction {
                title: "Resync arguments with the constructor".into(),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: edit(range, text),
                data: None,
            });
        }

        if let Some((tags, (range, text))) = service_gen::add_implied_tags(&ctx, &doc.text, offset) {
            actions.push(CodeAction {
                title: format!("Add tags {}", tags.join(", ")),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: edit(range, text),
                data: None,
            });
        }
    }

    if doc.language == "xml" && checks::is_service_file(&doc.path) && doc.text.contains("<container") {
        let edit = |range: std::ops::Range<usize>, text: String| {
            vec![(doc.path.clone(), vec![TextEdit { range: range_of(&doc.text, range.start, range.end), text }])]
        };

        if let Some((range, text)) = service_gen::add_arguments_xml(&ctx, &doc.text, offset) {
            actions.push(CodeAction {
                title: "Add arguments from the constructor".into(),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: edit(range, text),
                data: None,
            });
        }

        if let Some((tags, (range, text))) = service_gen::add_implied_tags_xml(&ctx, &doc.text, offset) {
            actions.push(CodeAction {
                title: format!("Add tags {}", tags.join(", ")),
                kind: Some("refactor.rewrite".into()),
                preferred: false,
                edits: edit(range, text),
                data: None,
            });
        }
    }

    actions
}

fn service_actions(index: &Index, doc: &Document, offset: usize) -> Vec<CodeAction> {
    let Some(project) = project_for(&doc.path) else {
        return Vec::new();
    };
    let read = |file: &str| project.read_file(file).ok();
    let mut actions = service_source_actions(index,doc,offset,&read);

    if doc.language != "php" {
        return actions;
    }

    let Some(model) = php_text::class_model(&doc.text) else {
        return actions;
    };
    let masked = php_text::mask(&doc.text);
    let line_start = masked[..model.name_range.start].rfind('\n').map_or(0, |at| at + 1);
    let line_end = masked[model.name_range.start..].find('\n').map_or(masked.len(), |at| model.name_range.start + at);
    let on_declaration = (line_start..=line_end).contains(&offset);
    let declaration = &masked[line_start..model.name_range.start];
    let concrete = declaration.contains("class ") && !declaration.contains("abstract");
    if !on_declaration || !concrete || !index.classes.contains_key(&model.fqcn) || index.services.contains_key(&model.fqcn) {
        return actions;
    }

    let Some((services_path, services)) = services_file(&project) else {
        return actions;
    };
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
            Kind::Translation => domain.as_deref().filter(|domain|twig_checks::cached_translation_key(index,domain,name)==Some(false)).and_then(|domain|index.translations.get(domain)),
            _ => None,
        };

        let near = known
            .filter(|_| !is_dynamic(name))
            .and_then(|names| suggest::closest(name, names.keys().map(String::as_str).filter(|candidate| !candidate.starts_with('_'))));
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

/// Tool window row keys: `<kind>:<name>`, `translation:<domain>:<key>`; the icon is the rows'.
const GROUPS: &[(&str, &str, &str)] = &[
    ("route", "Routes", "web"),
    ("template", "Templates", "file-html"),
    ("service", "Services", "class"),
    ("translation", "Translations", "text"),
    ("entity", "Entities", "database"),
    ("parameter", "Parameters", "settings"),
    ("env", "Environment", "variable"),
];

fn group_names<'a>(index: &'a Index, group: &str) -> Option<&'a Names> {
    match group {
        "route" => Some(&index.routes),
        "template" => Some(&index.templates),
        "service" => Some(&index.services),
        "parameter" => Some(&index.parameters),
        "env" => Some(&index.env),
        _ => None,
    }
}

fn entry_for<'a>(index: &'a Index, key: &str) -> Option<&'a Entry> {
    let (group, name) = key.split_once(':')?;
    if group == "translation" {
        let (domain, name) = name.split_once(':')?;
        return index.translations.get(domain)?.get(name);
    }
    if group == "entity" {
        return index.entities.get(name).map(|entity| &entity.entry);
    }
    if let Some(entry) = browse_tabs::entry(index, group, name) {
        return Some(entry);
    }
    group_names(index, group)?.get(name)
}

impl Main {
    fn web_config(&self, ctx: &Invocation) -> String {
        let mut config: serde_json::Value = serde_json::from_str(include_str!("ui_config.json")).expect("packaged Symfony UI config");
        if let Some(index) = self.view_index(ctx) {
            for tab in config["group"]["tabs"].as_array_mut().expect("Symfony tabs") {
                let count = match tab["id"].as_str().unwrap_or_default() {
                    VIEW => Some(self.routes(index).len()),
                    ENTITIES_VIEW => Some(index.entities.len()),
                    TEMPLATES_VIEW => Some(index.templates.len()),
                    SERVICES_VIEW => Some(index.services.len()),
                    CONFIG_VIEW => Some(index.parameters.keys().filter(|name| !name.starts_with('.')).count()),
                    ENV_VIEW => Some(index.env.len()),
                    TRANSLATIONS_VIEW => Some(index.translations.values().map(Names::len).sum()),
                    view => browse_tabs::count(index, view),
                };
                if let Some(count) = count {
                    tab["count"] = count.into();
                }
            }
        }
        config.to_string()
    }

    /// Resource table of a non-route tab: one row per fact, keyed as `entry_for` expects.
    fn inventory_table(index: &Index, group: &str, root: &str) -> Body {
        use serde_json::json;
        let icon = GROUPS.iter().find(|(id, _, _)| *id == group).map_or("file", |(_, _, icon)| *icon);
        let file = |path: &str| path.strip_prefix(root).unwrap_or(path).trim_start_matches(['/', '\\']).to_owned();
        let (items, data): (Vec<serde_json::Value>, serde_json::Value) = match group {
            "env" => (
                index.env.iter().map(|(name, entry)| {
                    let value = index.env_values.get(name).map_or("", String::as_str);
                    let detail = entry.detail.as_deref().unwrap_or_default();
                    json!({"key":format!("env:{name}"),"cells":[name,value,detail.strip_prefix("defined in ").unwrap_or(detail)],"icon":icon})
                }).collect(),
                json!({"columns":["Variable","Value","File"],"columnStyles":["code","code","muted"],"searchPlaceholder":"Filter environment variables…"}),
            ),
            "parameter" => (
                index.parameters.iter().filter(|(name, _)| !name.starts_with('.')).map(|(name, entry)| {
                    json!({"key":format!("parameter:{name}"),"cells":[name,entry.detail.as_deref().unwrap_or_default(),file(&entry.path)],"icon":icon})
                }).collect(),
                json!({"columns":["Key","Value","File"],"columnStyles":["code","code","muted"],"searchPlaceholder":"Filter parameters…","facetLabels":{"2":"All files"},"facets":[2]}),
            ),
            "service" => (
                index.services.iter().map(|(id, entry)| {
                    let detail = entry.detail.as_deref().unwrap_or_default();
                    let (class, kind) = match detail.strip_prefix("alias of ") {
                        Some(target) => (format!("@{target}"), "Alias"),
                        None if detail.is_empty() && id.contains('\\') => (id.clone(), "Service"),
                        None => (detail.to_owned(), "Service"),
                    };
                    json!({"key":format!("service:{id}"),"cells":[id,class,kind,file(&entry.path)],"icon":icon})
                }).collect(),
                json!({"columns":["Service","Class","Kind","File"],"columnStyles":["code","code","text","muted"],"columnWeights":[3,3,1,3],
                    "searchPlaceholder":"Filter services…","facetLabels":{"2":"All kinds","3":"All files"},"facets":[2,3]}),
            ),
            "template" => (
                index.templates.iter().map(|(name, entry)| {
                    let namespace = name.strip_prefix('@').and_then(|rest| rest.split_once('/')).map_or("(default)".to_owned(), |(namespace, _)| format!("@{namespace}"));
                    let rendered_by = index.renders.iter().filter(|render| render.template == *name).count();
                    let details: Vec<_> = entry.detail.iter().map(|detail| json!({"label":"Source","value":detail})).collect();
                    json!({"key":format!("template:{name}"),"cells":[name,namespace,file(&entry.path),rendered_by.to_string()],"icon":icon,"details":details})
                }).collect(),
                json!({"columns":["Template","Namespace","File","Rendered by"],"columnStyles":["code","locale","muted","locale"],"columnWeights":[4,1.5,4,1],
                    "searchPlaceholder":"Filter templates…","facetLabels":{"1":"All namespaces"},"facets":[1]}),
            ),
            "entity" => (
                index.entities.iter().map(|(class, entity)| {
                    let mut details = vec![json!({"label":"Class","value":class})];
                    details.extend(entity.fields.iter().map(|field| {
                        let value = format!("{}{}{}", field.type_name, if field.nullable { ", nullable" } else { "" }, if field.id { ", id" } else { "" });
                        json!({"label":format!("${}", field.name),"value":value})
                    }));
                    details.extend(entity.relations.iter().map(|relation| {
                        json!({"label":format!("${}", relation.name),"value":format!("{} \u{2192} {}", relation.kind, relation.target.as_deref().unwrap_or("?"))})
                    }));
                    let members: Vec<&str> = entity.fields.iter().map(|field| field.name.as_str()).chain(entity.relations.iter().map(|relation| relation.name.as_str())).collect();
                    let cells = [
                        php_text::short_name(class).to_owned(),
                        entity.table.clone(),
                        entity.fields.len().to_string(),
                        entity.relations.len().to_string(),
                        entity.repository.as_deref().map(php_text::short_name).unwrap_or_default().to_owned(),
                        file(&entity.entry.path),
                    ];
                    json!({"key":format!("entity:{class}"),"cells":cells,"icon":"class","details":details,"searchText":format!("{class} {}", members.join(" "))})
                }).collect(),
                json!({"columns":["Entity","Table","Fields","Relations","Repository","File"],"columnStyles":["code","code","locale","locale","code","muted"],
                    "columnWeights":[2.5,2,1,1,2.5,3],"searchPlaceholder":"Filter entities, tables or fields…"}),
            ),
            _ => (
                index.translations.iter().flat_map(|(domain, names)| names.iter().map(move |(key, entry)| (domain, key, entry))).map(|(domain, key, entry)| {
                    json!({"key":format!("translation:{domain}:{key}"),"cells":[key,domain,entry.detail.as_deref().unwrap_or_default(),file(&entry.path)],"icon":icon})
                }).collect(),
                json!({"columns":["Key","Domain","Value","File"],"columnStyles":["code","locale","text","muted"],"columnWeights":[3,1,3,2],
                    "searchPlaceholder":"Filter translations…","facetLabels":{"1":"All domains"},"facets":[1]}),
            ),
        };
        let mut data = data;
        data["showRowIcons"] = true.into();
        data["items"] = items.into();
        Body::Other { kind: "resources".into(), data: data.to_string() }
    }

    fn routes(&self, index: &Index) -> Vec<Route> {
        let mut routes = Vec::new();
        let mut handled: BTreeSet<(&str, Option<&str>)> = BTreeSet::new();
        let mut named: BTreeSet<&str> = BTreeSet::new();

        for def in &index.route_defs {
            handled.insert((&def.class, def.member.as_deref()));
            named.extend(def.name.as_deref());
            let class = if def.class.is_empty() { "Closure" } else { def.class.as_str() };
            let handler = Handler { class: class.to_owned(), member: def.member.clone() };
            let width = def.path.len() as u32;
            let location =
                Some(Location { path: def.file.clone(), range: Range::new(def.at, Position::new(def.at.line, def.at.character + width)) });
            let methods: Vec<&str> = if def.methods.is_empty() { vec!["ANY"] } else { def.methods.iter().map(String::as_str).collect() };
            for method in methods {
                routes.push(Route {
                    method: method.to_owned(),
                    path: def.path.clone(),
                    handler: handler.clone(),
                    name: def.name.clone(),
                    location: location.clone(),
                });
            }
        }

        for (name, entry) in &index.routes {
            if named.contains(name.as_str()) {
                continue;
            }

            let controller = entry.detail.as_deref().filter(|detail| !detail.starts_with('/'));
            let (class, member) = match controller.and_then(|controller| controller.split_once("::")) {
                Some((class, member)) => (class.rsplit('\\').next().unwrap_or(class), Some(member)),
                None => (controller.map_or("Closure", |class| class.rsplit('\\').next().unwrap_or(class)), None),
            };
            if handled.contains(&(class, member)) {
                continue;
            }

            let path = entry.detail.as_deref().filter(|detail| detail.starts_with('/')).unwrap_or(name);
            routes.push(Route {
                method: "ANY".into(),
                path: path.to_owned(),
                handler: Handler { class: class.to_owned(), member: member.map(str::to_owned) },
                name: Some(name.clone()),
                location: location(entry, name.len() as u32),
            });
        }

        routes
    }

    fn translation_target(&self, doc: &Document, file: &str, key: &str) -> Result<String, Error> {
        if doc.text.len()>128<<10 || key.is_empty() || key.len()>512 || key.contains(['\n','\r'])
            || !file_actions::normalized(file) {return Err(Error::Stale);}
        let project=host::project_of(&doc.path).ok_or(Error::Stale)?;
        let root=project.root()?;
        let index=self.indexes.get(&root).filter(|index|index.settings_current()).ok_or(Error::Stale)?;
        let domain=index::translation_domain(file);
        if !under(file,&root) || !index.translation_files.get(domain).is_some_and(|files|files.iter().any(|path|path==file))
            || host::project_of(file).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str()) {return Err(Error::Stale);}
        let text=project.read_file(file)?;
        if text.len()>128<<10 {return Err(Error::Stale);}
        let mut names=Names::new();index::translation_keys(file,&text,&mut names);
        let parsed_key=index::yaml_keys(&text).into_iter().any(|entry| {
            let mut parts=entry.parents;parts.push(entry.key);parts.join(".")==key
        });
        if names.contains_key(key) || parsed_key
            || host::project_of(&doc.path).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str())
            || host::project_of(file).and_then(|project|project.root().ok()).as_deref()!=Some(root.as_str()) {return Err(Error::Stale);}
        Ok(text)
    }

    fn current_navigation_target(&self, ctx: &Invocation, root: &str, path: &str) -> bool {
        !path.is_empty() && self.indexes.get(root).is_some_and(|index|index.settings_current())
            && ctx.project().and_then(|project|project.root().ok()).as_deref() == Some(root)
            && host::project_of(path).and_then(|project|project.root().ok()).as_deref() == Some(root)
    }

    fn current_route_member(&self, ctx: &Invocation, route: &Route) -> bool {
        let root = ctx.project().and_then(|p| p.root().ok());
        root.is_some() && root == self.route_owner
            && self.view_index(ctx).is_some_and(|index| self.routes(index).contains(route))
    }

    fn selected_route(&self, ctx: &Invocation) -> Option<&Route> {
        let (owner, expected) = self.route_selection.as_ref()?;
        if ctx.project()?.root().ok()?.as_str() != owner { return None; }
        let route = self.window.as_ref()?.routes.route(self.selected.as_deref()?)?;
        (route == expected && self.current_route_member(ctx, route)).then_some(route)
    }

    /// The route selected in the tool window, or one the user picks when the command runs from the palette.
    fn selected_route_or_pick(&self, ctx: &Invocation, title: &str) -> Result<Option<Route>, Error> {
        if let Some(route) = self.selected_route(ctx) {
            return Ok(Some(route.clone()));
        }
        self.pick_route(ctx, title)
    }

    /// A route the user picks from this project's routes; `None` when dismissed.
    fn pick_route(&self, ctx: &Invocation, title: &str) -> Result<Option<Route>, Error> {
        let index = self.view_index(ctx).ok_or("No Symfony project is indexed.")?;
        let routes = self.routes(index);
        if routes.is_empty() {
            return Err("No routes are indexed.".into());
        }
        let items: Vec<PickItem> = routes
            .iter()
            .enumerate()
            .map(|(i, route)| PickItem::new(&i.to_string(), &format!("{} {}", route.method, route.path)).description(route.name.as_deref().unwrap_or(&route.handler.class)))
            .collect();
        let Some(choice) = host::pick(title, &items)? else {
            return Ok(None);
        };
        routes.into_iter().nth(choice.parse().map_err(|_| Error::Stale)?).map(Some).ok_or(Error::Stale)
    }

    /// The route a route command acts on: a code lens's route (read again from its file), else the
    /// selected Routes row, else one picked from the palette.
    fn command_route(&self, ctx: &Invocation, lens: Option<&str>, command: &str) -> Result<Option<Route>, Error> {
        match lens {
            Some(raw) => Ok(Some(route_lenses::route_of(&route_lenses::resolve(ctx, raw)?))),
            None => self.selected_route_or_pick(ctx, route_pick_title(command)),
        }
    }

    /// Opens `route`, with every method it answers to, in the Request tab.
    fn open_in_request_tab(&mut self, ctx: &Invocation, route: Route) -> Result<(), Error> {
        let root = ctx.project().ok_or("Open a project first.")?.root()?;
        let route = self.with_sibling_methods(ctx, route);
        self.route_requests.select(&root, &route)?;
        ui_kit::web::reveal_view(route_request::VIEW, include_str!("ui_config.json"))
    }

    /// A Routes row has one method per row; the Request tab offers every method of that route.
    fn with_sibling_methods(&self, ctx: &Invocation, mut route: Route) -> Route {
        if route.method.contains('|') {
            return route;
        }
        let Some(index) = self.view_index(ctx) else { return route };
        let siblings: Vec<String> = self
            .routes(index)
            .into_iter()
            .filter(|other| other.path == route.path && other.name == route.name && other.handler == route.handler)
            .map(|other| other.method)
            .collect();
        if siblings.len() > 1 {
            route.method = siblings.join("|");
        }
        route
    }

    fn view_index(&self, ctx: &Invocation) -> Option<&Index> {
        let root = ctx.project()?.root().ok()?;
        self.indexes.get(&root).filter(|index|index.settings_current())
    }
}


impl Views for Main {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        self.reap_overdue();
        (|| -> Result<View, Error> {
            if view == namespace_paths::VIEW {
                return Ok(self.namespace_editor.render(ctx));
            }
            if view == PROFILER_VIEW {
                return Ok(self.profiler_snapshot.render(ctx));
            }
            if view == runtime::VIEW {
                return Ok(self.runtime.render(ctx));
            }
            if view == processes::VIEW {
                return Ok(self.processes.render(ctx));
            }
            if view == console_workspace::VIEW || view == console_workspace::GENERATORS_VIEW {
                return Ok(self.render_console(view, ctx));
            }
            if view == database::VIEW {
                let Some(project) = ctx.project() else {
                    return Ok(View::message("Open a project first."));
                };
                return Ok(database::view(&project));
            }
            if view == route_request::VIEW {
                let Some(root) = ctx.project().and_then(|project| project.root().ok()) else {
                    return Ok(View::message("Open a project first."));
                };
                return Ok(self.route_requests.render(&root));
            }
            if view == ABOUT_VIEW {
                let Some(project) = ctx.project() else {
                    return Ok(View::message("Open a project first."));
                };
                let reindex = Button::new("reindex", "Reindex").icon("refresh");
                let Some(index) = self.view_index(ctx) else {
                    return Ok(View::message("This project is not a Symfony application.").button(reindex));
                };
                return Ok(View::new(about::body(&project, Some(index))).button(Button::new("about-console", "Run bin/console about").icon("terminal")).button(reindex));
            }

            let group = match view {
                VIEW => "route",
                ENTITIES_VIEW => "entity",
                TEMPLATES_VIEW => "template",
                SERVICES_VIEW => "service",
                CONFIG_VIEW => "parameter",
                ENV_VIEW => "env",
                TRANSLATIONS_VIEW => "translation",
                browse_tabs::COMPONENTS_VIEW | browse_tabs::EVENTS_VIEW => "browse",
                _ => return Err(Error::NotFound(view.into())),
            };
            let routes_active = group == "route";
            let reindex = Button::new("reindex", "Reindex").icon("refresh");
            let stale_settings=ctx.project().and_then(|project|project.root().ok()).and_then(|root|self.indexes.get(&root)).is_some_and(|index|!index.settings_current());
            let Some(index) = self.view_index(ctx) else {
                self.route_owner = None; self.route_selection = None; self.selected = None;
                let message=if stale_settings {"Symfony source settings changed. Reindex to apply them."} else {"This project is not a Symfony application."};
                return Ok(View::message(message).button(reindex));
            };
            if group == "browse" {
                return Ok(browse_tabs::render(view, index, ctx, !self.events_flat));
            }

            let mut status = if index.compiled_container {
                String::from("Compiled container found")
            } else {
                String::from("No compiled container: run Warm Up Cache to index bundle services and routes")
            };
            if index.template_paths_unknown {
                status.push_str(" · some Twig template paths may be missing");
            }
            if !index.ambiguous_templates.is_empty() {
                status.push_str(" · templates with clashing names are skipped");
            }
            if index.partial {
                status.push_str(" · Some files weren't indexed (too many or too large). Reindex to retry.");
            }

            let base = View::new(Body::Tree(Vec::new()))
                .button(reindex)
                .button(Button::new("cache-warmup", "Warm Up Cache").icon("build-project"))
                .status(&status);
            let routes = if routes_active { self.routes(index) } else { Vec::new() };
            let root = ctx.project().and_then(|p| p.root().ok());
            let table = (!routes_active).then(|| Self::inventory_table(index, group, root.as_deref().unwrap_or_default()));
            if self.route_owner != root { self.selected = None; self.route_selection = None; }
            self.route_owner = root;
            let window = self.window.get_or_insert_with(Window::new);
            let view = match table {
                Some(body) => View { body, ..base },
                None => {
                    window.routes.set_routes(routes);
                    window.routes.render(base)
                }
            };
            Ok(view)
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        self.reap_overdue();
        let config = self.web_config(ctx);
        if ui_kit::web::handle(self, view, &event, ctx, &config)? {
            return Ok(());
        }

        if view == runtime::VIEW {
            return self.runtime.handle(&event, ctx);
        }

        if view == processes::VIEW {
            return self.processes.handle(&event, ctx);
        }

        if view == console_workspace::VIEW || view == console_workspace::GENERATORS_VIEW {
            return self.handle_console(view, event, ctx);
        }

        if view == route_request::VIEW {
            return self.handle_route_request(ctx, event);
        }

        if view == database::VIEW {
            match event {
                ViewEvent::Message(message) => {
                    let reply = self.database.handle(ctx, &message).unwrap_or_else(|error| serde_json::json!({"error": error.to_string()}));
                    if !reply.is_null() {
                        ui_kit::web::post_view_message(view, &reply.to_string(), &config)?;
                    }
                }
                ViewEvent::Button(id) => {
                    let result = self.run_command(&id, ctx, None);
                    ui_kit::web::refresh_view(view, include_str!("ui_config.json"));
                    result?;
                }
                _ => {}
            }
            return Ok(());
        }

        if view == ABOUT_VIEW && matches!(&event, ViewEvent::Button(id) if id == "about-console") {
            return processes::console_about(ctx);
        }

        if view == PROFILER_VIEW {
            match event {
                ViewEvent::Button(id) if id == "discover-profiler-source" => {
                    if profiler::discover_source(ctx)? { refresh_profiler_view(); }
                }
                ViewEvent::Button(id) if id == "choose-profiler-source" => {
                    if profiler::choose_source(ctx)? { refresh_profiler_view(); }
                }
                ViewEvent::Button(id) if id == "inspect-profile-details" => {
                    self.profiler_snapshot.inspect(ctx)?;
                    refresh_profiler_view();
                }
                ViewEvent::Button(id) if id == "close-profile-details" => {
                    self.profiler_snapshot.close_details();
                    refresh_profiler_view();
                }
                ViewEvent::Button(id) if id == "profiler-previous-page" || id == "profiler-next-page" => {
                    self.profiler_snapshot.turn_page(id == "profiler-next-page");
                    refresh_profiler_view();
                }
                ViewEvent::Button(id) if id == "refresh-profiler" => {
                    self.profiler_snapshot.refresh(ctx);
                    ui_kit::web::refresh_view(PROFILER_VIEW, include_str!("ui_config.json"));
                }
                ViewEvent::Select(key) => self.profiler_snapshot.select(&key, ctx),
                ViewEvent::Activate(key) => self.profiler_snapshot.open(&key, ctx)?,
                _ => {}
            }
            return Ok(());
        }
        if view == namespace_paths::VIEW {
            let changed = self.namespace_editor.handle(&event, ctx)?;
            if changed {
                if let Some(project) = ctx.project() {
                    self.index_project(&project);
                }
            }
            ui_kit::web::refresh_view(view, include_str!("ui_config.json"));
            return Ok(());
        }
        if MAIN_VIEWS.contains(&view) {
            if view == VIEW {
                let root = ctx.project().and_then(|p| p.root().ok());
                if root.is_none() || root != self.route_owner || self.view_index(ctx).is_none() { return Err(Error::Stale); }
                if let ViewEvent::Select(key) = &event {
                    let route = self.window.as_ref().and_then(|w| w.routes.route(key)).cloned();
                    self.route_selection = route.filter(|r| self.current_route_member(ctx, r)).map(|r| (root.clone().unwrap(), r));
                    self.selected = Some(key.clone());
                }
                let outcome = self.window.as_mut().unwrap().routes.handle(&event);
                match outcome {
                    RouteEvent::Changed => { refresh_main_view(); return Ok(()); }
                    RouteEvent::Open(route) => {
                        if !self.current_route_member(ctx, &route) { return Err(Error::Stale); }
                        if let Some(location) = route.location {
                            let root = ctx.project().and_then(|project|project.root().ok()).ok_or(Error::Stale)?;
                            if !self.current_navigation_target(ctx, &root, &location.path) { return Err(Error::Stale); }
                            host::open_location(&location.path, location.range.start.line, location.range.start.character)?;
                        }
                        return Ok(());
                    }
                    RouteEvent::Ignored => {}
                }
            } else if let ViewEvent::Select(key) = &event {
                self.selected = Some(key.clone());
                self.route_selection = None;
            }
        }

        match event {
            ViewEvent::Activate(key) => {
                let entry = self.view_index(ctx).and_then(|index| entry_for(index, &key));
                if let Some(entry) = entry.filter(|e| !e.path.is_empty()) {
                    let root = ctx.project().and_then(|project|project.root().ok()).ok_or(Error::Stale)?;
                    if !self.current_navigation_target(ctx, &root, &entry.path) { return Err(Error::Stale); }
                    host::open_location(&entry.path, entry.at.line, entry.at.character)?;
                }
            }
            ViewEvent::Button(id) => {
                self.run_command(&id, ctx, None)?;
                refresh_main_view();
            }
            _ => {}
        }
        Ok(())
    }
}

impl Main {
    /// Stops every Send Request, database probe and console run past its time limit: a hidden or
    /// closed tab sends no polls. Runs on every command, view request and event batch; no host call
    /// unless a run is overdue. Not for editor providers.
    fn reap_overdue(&mut self) {
        self.route_requests.reap_overdue();
        self.database.reap_overdue();
        self.console.reap();
    }

    fn handle_route_request(&mut self, ctx: &Invocation, event: ViewEvent) -> Result<(), Error> {
        let project = ctx.project().ok_or("Open a project first.")?;
        let root = project.root()?;
        match event {
            ViewEvent::Field(key, value) => self.route_requests.set_field(&root, &key, &value)?,
            ViewEvent::Button(id) if id == route_request::CHOOSE_ROUTE => {
                if let Some(route) = self.pick_route(ctx, route_pick_title("route-send-request"))? {
                    self.open_in_request_tab(ctx, route)?;
                }
            }
            ViewEvent::Button(id) if id == "request-copy-curl" => {
                let curl = self.route_requests.request(&root)?.curl(DEFAULT_BASE_URL);
                match host::copy_to_clipboard(&curl) {
                    Ok(()) => host::notify(
                        NotificationLevel::Info,
                        SOURCE,
                        "Copied the request as cURL. Act as user is not part of it, and form posts still need their CSRF token against a real server.",
                    ),
                    Err(Error::Denied(_)) => host::notify(NotificationLevel::Warning, SOURCE, &ui_kit::denied::message("Copy as cURL", "clipboard", "Symfony")),
                    Err(error) => return Err(error),
                }
            }
            ViewEvent::Button(id) if id == "request-send" => {
                let sent = self.route_requests.send(&root, |args| route_request::start(&project, args));
                ui_kit::web::refresh_view(route_request::VIEW, include_str!("ui_config.json"));
                sent?;
            }
            ViewEvent::Button(id) if id == route_request::CANCEL => {
                self.route_requests.cancel(&root);
                ui_kit::web::refresh_view(route_request::VIEW, include_str!("ui_config.json"));
            }
            ViewEvent::Message(message) => {
                let tick: serde_json::Value = serde_json::from_str(&message).map_err(|_| Error::Stale)?;
                if tick["op"] != route_request::TICK {
                    return Err(Error::Stale);
                }
                if self.route_requests.poll(&root) {
                    ui_kit::web::refresh_view(route_request::VIEW, include_str!("ui_config.json"));
                }
                let answer = serde_json::json!({"_backgroundPoll": tick["_backgroundPoll"]});
                ui_kit::web::post_view_message(route_request::VIEW, &answer.to_string(), include_str!("ui_config.json"))?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl openidea_plugin::language_features::LanguageFeatures for Main {
    fn code_lenses(&mut self, doc: &Document) -> Vec<openidea_plugin::language_features::CodeLens> {
        if !host::setting_bool("routeLenses").unwrap_or(true) {
            return Vec::new();
        }
        self.index_for(&doc.path).map(|index| route_lenses::lenses(index, doc)).unwrap_or_default()
    }
}

openidea_plugin::register_plugin!(Main, providers, views, indexing, endpoints, search, file_actions, hover, diagnostics, mcp, language_features, run_configurations);

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

    pub(crate) fn activated(host: &MockHost) -> Main {
        symfony_project(host);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin
    }

    pub(crate) fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn labels(items: Vec<CompletionItem>) -> Vec<String> {
        items.into_iter().map(|item| item.label).collect()
    }

    fn main_notices(host:&MockHost,after:usize)->Vec<String>{
        host.view_messages()[after..].iter().filter_map(|(view,text)| {
            let value:serde_json::Value=serde_json::from_str(text).ok()?;
            (value["channel"]=="ui-kit-refresh").then(||value["targetView"].as_str().unwrap_or(view).to_owned())
        }).collect()
    }

    #[test]
    fn compiled_cache_input_events_rebuild_and_notify_only_for_owned_known_inputs() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let path="var/cache/dev/url_generating_routes.php";
        host.add_file(path,"<?php\nreturn [\n'owned_cache_route' => [[], ['_controller' => 'App\\\\Controller\\\\HomeController::index'], [], []]];");
        let before=host.view_messages().len();plugin.handle_events(&[Event::FileSaved(format!("/project/{path}"))],&host.invocation());
        assert!(plugin.indexes["/project"].routes.contains_key("owned_cache_route"));assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));
        let before=host.view_messages().len();plugin.handle_events(&[Event::FileSaved("/other/var/cache/dev/url_generating_routes.php".into()),Event::FileSaved("/project/var/cache/dev/profiler/index.csv".into())],&host.invocation());
        assert!(main_notices(&host,before).is_empty());
        host.add_file(path,"<?php return [];");let before=host.view_messages().len();
        plugin.handle_events(&[Event::FileSaved("/project/var/cache/dev".into())],&host.invocation());
        assert!(!plugin.indexes["/project"].routes.contains_key("owned_cache_route"));assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));
    }
    #[test]
    fn custom_cache_and_extra_source_events_invalidate_exact_inputs_and_ancestors() {
        let host = MockHost::install();
        host.set_setting("compiledRoutes", &serde_json::to_string("build/cache/routes.php").unwrap());
        host.set_setting("compiledContainers", &serde_json::to_string("build/cache/container.xml").unwrap());
        host.set_setting("extraServiceFiles", &serde_json::to_string("custom/services.yaml").unwrap());
        let mut plugin = activated(&host);
        host.add_file("build/cache/routes.php", "<?php\nreturn [\n'custom_cache_route' => [[], [], [], []]];");
        host.add_file("build/cache/container.xml", "<container><services><service id=\"custom_compiled\" class=\"App\\Custom\" /></services></container>");
        host.add_file("custom/services.yaml", "services:\n  custom_source:\n    class: App\\Custom\n");
        let before = host.view_messages().len();
        plugin.handle_events(&[Event::FileSaved("/project/build/cache/routes.php".into()), Event::FileSaved("/project/custom/services.yaml".into())], &host.invocation());
        assert!(plugin.indexes["/project"].routes.contains_key("custom_cache_route"));
        assert!(plugin.indexes["/project"].services.contains_key("custom_compiled"));
        assert!(plugin.indexes["/project"].services.contains_key("custom_source"));
        assert_eq!(main_notices(&host, before), MAIN_VIEWS.map(String::from));
        host.add_file("build/cache/routes.php", "<?php\nreturn [];" );
        host.add_file("build/cache/container.xml", "<container/>" );
        let before = host.view_messages().len();
        plugin.handle_events(&[Event::FileSaved("/project/build".into())], &host.invocation());
        assert!(!plugin.indexes["/project"].routes.contains_key("custom_cache_route"));
        assert!(!plugin.indexes["/project"].services.contains_key("custom_compiled"));
        assert_eq!(main_notices(&host, before), MAIN_VIEWS.map(String::from));
        let before = host.view_messages().len();
        plugin.handle_events(&[Event::FileSaved("/project/build/cache/routes.php.old".into()), Event::FileSaved("/other/build/cache/routes.php".into())], &host.invocation());
        assert!(main_notices(&host, before).is_empty());
    }
    #[test]
    fn compiled_cache_filter_uses_shared_reader_inputs_and_directory_boundaries() {
        for path in ["var","var/cache","var/cache/dev","var/cache/dev/UrlGenerator.php","var/cache/dev/srcApp_KernelDevDebugContainer.xml"] {assert!(affects_index(path),"{path}");}
        for path in ["","var/cache/development","var/cache/dev/profiler/index.csv","var/cache/dev/OtherContainer.xml","var/cache/prod/UrlGenerator.php","var/cache/dev/UrlGenerator.php.old"]{assert!(!affects_index(path),"{path}");}
    }
    #[test]
    fn main_commit_and_framework_removal_notify_only_main_view(){
        let host=MockHost::install();let mut plugin=activated(&host);let before=host.view_messages().len();
        plugin.index_project(&host.invocation().project().unwrap());assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));
        host.add_file("composer.json","{}");let before=host.view_messages().len();plugin.index_project(&host.invocation().project().unwrap());
        assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));assert!(plugin.view_index(&host.invocation()).is_none());
        let before=host.view_messages().len();plugin.index_project(&host.invocation().project().unwrap());assert!(main_notices(&host,before).is_empty());
    }
    #[test]
    fn overflow_clear_and_closed_root_prune_notify_but_do_not_rebuild_from_render(){
        let host=MockHost::install();let mut plugin=activated(&host);let before=host.view_messages().len();
        let events=vec![Event::FileSaved("/project/a.php".into());1025];plugin.handle_events(&events,&host.invocation());
        assert!(plugin.indexes.is_empty());assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));
        let before=host.view_messages().len();plugin.render(VIEW,None,&host.invocation()).unwrap();assert_eq!(host.view_messages().len(),before);
        plugin.index_project(&host.invocation().project().unwrap());let before=host.view_messages().len();host.set_project_root(None);
        plugin.handle_events(&[],&host.invocation());assert!(plugin.indexes.is_empty());assert_eq!(main_notices(&host,before),MAIN_VIEWS.map(String::from));
    }
    #[test]
    fn stable_main_ui_kit_render_replies_without_index_invalidation_loop(){
        let host=MockHost::install();let mut plugin=activated(&host);
        for sequence in [1,2]{
            let before=host.view_messages().len();let request=serde_json::json!({"channel":"ui-kit","frameId":"0123456789abcdef0123456789abcdef","sequence":sequence,"project":"/project","op":"render"});
            plugin.handle(VIEW,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();let messages=host.view_messages();assert_eq!(messages.len(),before+1);
            let reply:serde_json::Value=serde_json::from_str(&messages[before].1).unwrap();assert_eq!(reply["channel"],"ui-kit");assert!(reply.get("model").is_some());
        }
    }
    #[test]
    fn route_copy_curl_without_selection_asks_and_copies_nothing_when_dismissed(){
        let host=MockHost::install().with_capabilities(&["project.read","clipboard"]);let mut plugin=activated(&host);
        plugin.render(VIEW,None,&host.invocation()).unwrap();
        host.answer_next_prompt(None);
        plugin.run_command("route-copy-curl",&host.invocation(),None).unwrap();
        assert!(host.clipboard().is_none());
    }
    #[test]
    fn route_actions_reject_last_other_root_and_no_index_cached_rows(){
        let host=MockHost::install();let mut plugin=activated(&host);let ctx=host.invocation();
        let Body::List(rows)=plugin.render(VIEW,None,&ctx).unwrap().body else {panic!("routes expected")};let key=rows[0].key.clone();
        plugin.handle(VIEW,ViewEvent::Select(key.clone()),&ctx).unwrap();assert!(plugin.selected_route(&ctx).is_some());
        host.set_project_root(Some("/other"));let other=host.invocation();assert!(plugin.run_command("route-copy-curl",&other,None).is_err());assert!(host.clipboard().is_none());
        assert!(matches!(plugin.handle(VIEW,ViewEvent::Activate(key),&other),Err(Error::Stale)));assert!(host.opened_locations().is_empty());
        host.set_project_root(Some("/project"));plugin.indexes.clear();assert!(plugin.run_command("route-copy-curl",&host.invocation(),None).is_err());assert!(host.clipboard().is_none());
    }
    #[test]
    fn cached_route_disappearance_refuses_old_selected_fact_without_io(){
        let host=MockHost::install();let mut plugin=activated(&host);let ctx=host.invocation();let Body::List(rows)=plugin.render(VIEW,None,&ctx).unwrap().body else {panic!("routes expected")};
        plugin.handle(VIEW,ViewEvent::Select(rows[0].key.clone()),&ctx).unwrap();
        plugin.indexes.get_mut("/project").unwrap().routes.clear();
        let before=host.requests().len();plugin.run_command("route-copy-curl",&ctx,None).unwrap();assert!(host.clipboard().is_none());assert_eq!(host.requests().len(),before);assert!(host.process_runs().is_empty());
    }

    #[test]
    fn code_actions_combine_quote_fix_and_intentions() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let yaml = doc(
            "/project/config/services.yaml",
            "yaml",
            "services:\n    app.mailer:\n        arguments: [@foo]\n        class: App\\Mailer\n        factory: @bar\n",
        );
        let titles: Vec<String> = plugin
            .code_actions(&yaml, Range::new(Position::new(4, 17), Position::new(4, 17)))
            .into_iter()
            .map(|action| action.title)
            .collect();
        assert!(titles.iter().any(|title| title.starts_with("Quote ")), "{titles:?}");

        let php = doc(
            "/project/src/Controller/PostController.php",
            "php",
            "<?php\nnamespace App\\Controller;\n\nclass PostController extends AbstractController\n{\n    public function show()\n    {\n    }\n}\n",
        );
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
        let keys = [
            "controllerChecks",
            "serviceChecks",
            "phpCallChecks",
            "twigChecks",
            "twigSyntaxChecks",
            "yamlSyntaxChecks",
            "commandChecks",
            "deprecationChecks",
            "doctrineChecks",
        ];
        let states = |groups: Groups| {
            [
                groups.controllers,
                groups.services,
                groups.php_calls,
                groups.twig,
                groups.twig_syntax,
                groups.yaml_syntax,
                groups.commands,
                groups.deprecations,
                groups.doctrine,
            ]
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
    fn twig_members_are_typed_from_controllers_var_comments_and_parent_classes() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/post/show.html.twig", "{{ post }}");
        host.add_file("src/Entity/Base.php", "<?php\nnamespace App\\Entity;\nabstract class Base\n{\n    public function getId(): ?int { return 1; }\n}\n");
        host.add_file("src/Entity/User.php", "<?php\nnamespace App\\Entity;\nclass User\n{\n    public function getEmail(): string { return ''; }\n}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nclass Post extends Base\n{\n    public function getTitle(): string { return ''; }\n    public function getAuthor(): ?User { return null; }\n    public function isPublished(): bool { return true; }\n    private function internal(): void {}\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository {}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Entity\\Post;\nuse App\\Repository\\PostRepository;\nclass PostController\n{\n    public function show(PostRepository $repo, int $id)\n    {\n        $post = $repo->find($id);\n        return $this->render('post/show.html.twig', ['post' => $post, 'draft' => new Post(), 'count' => 3]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let complete = |plugin: &mut Main, text: &str, line: u32, character: u32| {
            let twig = doc("/project/templates/post/show.html.twig", "twig", text);
            labels(plugin.completion(&twig, Position::new(line, character), None))
        };

        assert_eq!(complete(&mut plugin, "{{ post. }}", 0, 8), ["author", "id", "published", "title"]);
        assert_eq!(complete(&mut plugin, "{{ draft.ti }}", 0, 11), ["author", "id", "published", "title"]);
        assert_eq!(complete(&mut plugin, "{{ post.author. }}", 0, 15), ["email"]);
        assert!(complete(&mut plugin, "{{ count. }}", 0, 9).is_empty());
        assert!(!complete(&mut plugin, "{{ unknown. }}", 0, 11).contains(&"title".to_owned()));
        assert!(!complete(&mut plugin, "{{ post|upper. }}", 0, 14).contains(&"title".to_owned()));

        let commented = "{# @var item \\App\\Entity\\User #}\n{{ item. }}";
        assert_eq!(complete(&mut plugin, commented, 1, 8), ["email"]);
    }

    #[test]
    fn php_members_follow_typed_variables_and_stay_quiet_otherwise() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("src/Entity/Base.php", "<?php\nnamespace App\\Entity;\nabstract class Base\n{\n    public function getId(): ?int { return 1; }\n}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nclass Post extends Base\n{\n    public string $slug = '';\n    public function getTitle(): string { return ''; }\n    public function rename(string $title): static { return $this; }\n    protected function internal(): void {}\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository\n{\n    public function latest(int $limit = 5): array { return []; }\n}\n");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let source = |statement: &str| {
            format!("<?php\nnamespace App\\Controller;\nuse App\\Entity\\Post;\nuse App\\Repository\\PostRepository;\nclass X\n{{\n    public function __construct(private PostRepository $posts) {{}}\n    public function a(PostRepository $repo, int $id, Post $typed)\n    {{\n        $found = $repo->find($id);\n        $fresh = new Post();\n        $text = 'x';\n{statement}\n    }}\n}}\n")
        };
        let complete = |plugin: &mut Main, statement: &str| {
            let text = source(statement);
            let line = text.lines().position(|line| line == statement).unwrap() as u32;
            let php = doc("/project/src/Controller/X.php", "php", &text);
            labels(plugin.completion(&php, Position::new(line, statement.len() as u32), None))
        };

        let post = ["getId", "getTitle", "rename", "slug"];
        assert_eq!(complete(&mut plugin, "        $found->"), post);
        assert_eq!(complete(&mut plugin, "        $fresh->ge"), post);
        assert_eq!(complete(&mut plugin, "        $typed->"), post);
        assert_eq!(complete(&mut plugin, "        $repo->"), ["latest"]);
        assert_eq!(complete(&mut plugin, "        $this->posts->"), ["latest"]);
        assert!(complete(&mut plugin, "        $text->").is_empty());
        assert!(complete(&mut plugin, "        $unknown->").is_empty());
        assert!(complete(&mut plugin, "        $x = '$found->").is_empty());
    }

    #[test]
    fn twig_loops_follow_typed_collections_and_stay_scoped() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/post/list.html.twig", "{{ posts }}");
        host.add_file("src/Entity/Comment.php", "<?php\nnamespace App\\Entity;\nclass Comment\n{\n    public function getBody(): string { return ''; }\n}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nuse Doctrine\\Common\\Collections\\Collection;\nclass Post\n{\n    /**\n     * @return Collection<int, Comment>\n     */\n    public function getComments(): Collection { return new Collection(); }\n    public function getTitle(): string { return ''; }\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository {}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Repository\\PostRepository;\nclass PostController\n{\n    public function list(PostRepository $repo)\n    {\n        $posts = $repo->findAll();\n        return $this->render('post/list.html.twig', ['posts' => $posts, 'latest' => $repo->findBy([], null, 3)]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let complete = |plugin: &mut Main, text: &str, line: u32, character: u32| {
            let twig = doc("/project/templates/post/list.html.twig", "twig", text);
            labels(plugin.completion(&twig, Position::new(line, character), None))
        };

        let looped = "{% for post in posts %}\n{{ post. }}\n{% endfor %}";
        assert_eq!(complete(&mut plugin, looped, 1, 8), ["comments", "title"]);
        let keyed = "{% for index, post in latest|slice(0, 2) %}\n{{ post. }}\n{% endfor %}";
        assert_eq!(complete(&mut plugin, keyed, 1, 8), ["comments", "title"]);
        let nested = "{% for post in posts %}{% for comment in post.comments %}\n{{ comment. }}\n{% endfor %}{% endfor %}";
        assert_eq!(complete(&mut plugin, nested, 1, 11), ["body"]);
        let after = "{% for post in posts %}{% endfor %}\n{{ post. }}";
        assert!(complete(&mut plugin, after, 1, 8).is_empty());
        let collection = "{{ posts. }}";
        assert!(complete(&mut plugin, collection, 0, 9).is_empty());
        let commented = "{# @var items \\App\\Entity\\Comment[] #}\n{% for item in items %}\n{{ item. }}\n{% endfor %}";
        assert_eq!(complete(&mut plugin, commented, 2, 8), ["body"]);
    }

    #[test]
    fn member_definitions_jump_to_the_declaring_class_including_parents() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/post/show.html.twig", "{{ post }}");
        host.add_file("src/Entity/Base.php", "<?php\nnamespace App\\Entity;\nabstract class Base\n{\n    public function getId(): ?int { return 1; }\n}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nclass Post extends Base\n{\n    public string $slug = '';\n    public function getTitle(): string { return ''; }\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository {}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Repository\\PostRepository;\nclass PostController\n{\n    public function show(PostRepository $repo, int $id)\n    {\n        $post = $repo->find($id);\n        return $this->render('post/show.html.twig', ['post' => $post]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/post/show.html.twig", "twig", text);
        let at = |plugin: &mut Main, document: &Document, line: u32, character: u32| plugin.definition(document, Position::new(line, character));
        let title = at(&mut plugin, &twig("{{ post.title }}"), 0, 10);
        assert_eq!(title.len(), 1);
        assert_eq!(title[0].path, "/project/src/Entity/Post.php");
        assert_eq!((title[0].range.start.line, title[0].range.start.character), (5, 20));
        let inherited = at(&mut plugin, &twig("{{ post.id }}"), 0, 9);
        assert_eq!(inherited[0].path, "/project/src/Entity/Base.php");
        let slug = at(&mut plugin, &twig("{{ post.slug }}"), 0, 10);
        assert_eq!((slug[0].path.as_str(), slug[0].range.start.line), ("/project/src/Entity/Post.php", 4));
        assert!(at(&mut plugin, &twig("{{ post.missing }}"), 0, 11).is_empty());

        let php = "<?php\nnamespace App\\Controller;\nuse App\\Entity\\Post;\nclass X\n{\n    public function a(Post $post)\n    {\n        $post->getTitle();\n    }\n}\n";
        let found = at(&mut plugin, &doc("/project/src/Controller/X.php", "php", php), 7, 17);
        assert_eq!((found[0].path.as_str(), found[0].range.start.line), ("/project/src/Entity/Post.php", 5));
    }

    #[test]
    fn service_ids_and_query_results_type_twig_and_php_members() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/home.html.twig", "{{ mailer }}");
        host.add_file("config/services.yaml", "services:\n    app.mailer:\n        class: App\\Service\\Mailer\n    app.mail_alias:\n        alias: app.mailer\n");
        host.add_file("src/Service/Mailer.php", "<?php\nnamespace App\\Service;\nclass Mailer\n{\n    public function send(string $to): void {}\n    public function getTransport(): string { return ''; }\n}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nclass Post\n{\n    public function getTitle(): string { return ''; }\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository {}\n");
        host.add_file(
            "src/Controller/HomeController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Repository\\PostRepository;\nuse Psr\\Container\\ContainerInterface;\nclass HomeController\n{\n    public function index(ContainerInterface $locator, PostRepository $repo)\n    {\n        $mailer = $locator->get('app.mail_alias');\n        $posts = $repo->createQueryBuilder('p')->orderBy('p.id')->getQuery()->getResult();\n        return $this->render('home.html.twig', ['mailer' => $mailer, 'posts' => $posts]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/home.html.twig", "twig", text);
        assert_eq!(labels(plugin.completion(&twig("{{ mailer. }}"), Position::new(0, 10), None)), ["transport"]);
        let looped = "{% for post in posts %}{{ post. }}{% endfor %}";
        assert_eq!(labels(plugin.completion(&twig(looped), Position::new(0, 31), None)), ["title"]);

        let php = "<?php\nnamespace App\\Controller;\nuse Psr\\Container\\ContainerInterface;\nclass X\n{\n    public function a(ContainerInterface $locator)\n    {\n        $m = $locator->get('app.mailer');\n        $m->\n    }\n}\n";
        let items = labels(plugin.completion(&doc("/project/src/Controller/X.php", "php", php), Position::new(8, 12), None));
        assert_eq!(items, ["getTransport", "send"]);
    }

    #[test]
    fn this_members_complete_and_navigate_inside_the_class() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("src/Base.php", "<?php\nnamespace App;\nclass Base\n{\n    protected function boot(): void {}\n}\n");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();
        let php = "<?php\nnamespace App;\nclass Child extends Base\n{\n    private string $label = '';\n    private function helper(): void {}\n    public function run(): void\n    {\n        $this->\n        $this->helper();\n        $this->boot();\n    }\n}\n";
        let document = doc("/project/src/Child.php", "php", php);
        assert_eq!(labels(plugin.completion(&document, Position::new(8, 16), None)), ["boot", "helper", "label", "run"]);
        let own = plugin.definition(&document, Position::new(9, 18));
        assert_eq!((own[0].path.as_str(), own[0].range.start.line), ("/project/src/Child.php", 5));
        let parent = plugin.definition(&document, Position::new(10, 17));
        assert_eq!((parent[0].path.as_str(), parent[0].range.start.line), ("/project/src/Base.php", 4));
    }

    #[test]
    fn hover_shows_the_inferred_type_of_members_and_variables() {
        use openidea_plugin::hover::HoverProvider;
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/post/show.html.twig", "{{ post }}");
        host.add_file("src/Entity/Comment.php", "<?php\nnamespace App\\Entity;\nclass Comment {}\n");
        host.add_file("src/Entity/Post.php", "<?php\nnamespace App\\Entity;\nclass Post\n{\n    public function getTitle(): string { return ''; }\n    /** @return Comment[] */\n    public function getComments(): array { return []; }\n}\n");
        host.add_file("src/Repository/PostRepository.php", "<?php\nnamespace App\\Repository;\nuse App\\Entity\\Post;\n/**\n * @extends ServiceEntityRepository<Post>\n */\nclass PostRepository extends ServiceEntityRepository {}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Repository\\PostRepository;\nclass PostController\n{\n    public function show(PostRepository $repo, int $id)\n    {\n        $post = $repo->find($id);\n        return $this->render('post/show.html.twig', ['post' => $post]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/post/show.html.twig", "twig", text);
        let member = plugin.hover(&twig("{{ post.title }}"), Position::new(0, 10)).unwrap();
        assert!(member.markdown.contains("getTitle() : string") && member.markdown.contains("App"), "{}", member.markdown);
        let list = plugin.hover(&twig("{{ post.comments }}"), Position::new(0, 11)).unwrap();
        assert!(list.markdown.contains("Comment[]"), "{}", list.markdown);
        let variable = plugin.hover(&twig("{{ post.title }}"), Position::new(0, 4)).unwrap();
        assert!(variable.markdown.contains("post") && variable.markdown.contains("Post"), "{}", variable.markdown);
        assert!(plugin.hover(&twig("{{ unknown }}"), Position::new(0, 5)).is_none());

        let php = "<?php\nnamespace App\\Controller;\nuse App\\Entity\\Post;\nclass X\n{\n    public function a(Post $post)\n    {\n        $post->getTitle();\n    }\n}\n";
        let document = doc("/project/src/Controller/X.php", "php", php);
        assert!(plugin.hover(&document, Position::new(7, 17)).unwrap().markdown.contains("getTitle() : string"));
        assert!(plugin.hover(&document, Position::new(7, 10)).unwrap().markdown.contains("Post"));
    }

    #[test]
    fn forms_type_twig_views_php_field_names_and_option_names() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("templates/post/edit.html.twig", "{{ form }}");
        host.add_file(
            "src/Form/PostType.php",
            "<?php\nnamespace App\\Form;\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\TextType;\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\ChoiceType;\nclass PostType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options): void\n    {\n        $builder\n            ->add('title', TextType::class)\n            ->add('address', AddressType::class)\n            ->add('status', ChoiceType::class);\n    }\n}\n",
        );
        host.add_file("src/Form/AddressType.php", "<?php\nnamespace App\\Form;\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\TextType;\nclass AddressType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options): void\n    {\n        $builder->add('street', TextType::class);\n    }\n}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nuse App\\Form\\PostType;\nclass PostController\n{\n    public function edit()\n    {\n        $form = $this->createForm(PostType::class, null);\n        return $this->render('post/edit.html.twig', ['form' => $form->createView()]);\n    }\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/post/edit.html.twig", "twig", text);
        let sorted = |mut items: Vec<String>| {
            items.sort();
            items
        };
        let complete = |plugin: &mut Main, text: &str, character: u32| sorted(labels(plugin.completion(&twig(text), Position::new(0, character), None)));
        assert_eq!(complete(&mut plugin, "{{ form_row(form.) }}", 17), ["address", "children", "methodRendered", "parent", "rendered", "status", "title", "vars"]);
        assert_eq!(complete(&mut plugin, "{{ form.title. }}", 14), ["children", "methodRendered", "parent", "rendered", "vars"]);
        assert!(complete(&mut plugin, "{{ form.vars. }}", 13).contains(&"errors".to_owned()));
        assert!(complete(&mut plugin, "{{ form.address. }}", 16).contains(&"street".to_owned()));
        let jump = plugin.definition(&twig("{{ form.title }}"), Position::new(0, 10));
        assert_eq!((jump[0].path.as_str(), jump[0].range.start.line), ("/project/src/Form/PostType.php", 9));

        let controller = |statement: &str| {
            format!("<?php\nnamespace App\\Controller;\nuse App\\Form\\PostType;\nuse Symfony\\Component\\Form\\Extension\\Core\\Type\\ChoiceType;\nclass X\n{{\n    public function a()\n    {{\n        $form = $this->createForm(PostType::class);\n{statement}\n    }}\n}}\n")
        };
        let php = |plugin: &mut Main, statement: &str| {
            let caret = statement.find('|').unwrap();
            let line_text = statement.replace('|', "");
            let text = controller(&line_text);
            let line = text.lines().position(|line| line == line_text).unwrap() as u32;
            sorted(labels(plugin.completion(&doc("/project/src/Controller/X.php", "php", &text), Position::new(line, caret as u32), None)))
        };
        assert_eq!(php(&mut plugin, "        $form->get('|');"), ["address", "status", "title"]);
        assert_eq!(php(&mut plugin, "        $form->has(\"st|\");"), ["address", "status", "title"]);
        let options = php(&mut plugin, "        $builder->add('status', ChoiceType::class, ['|' => 1]);");
        assert!(options.contains(&"choices".to_owned()) && options.contains(&"label".to_owned()) && !options.contains(&"scale".to_owned()));
        assert!(php(&mut plugin, "        $builder->add('|');").is_empty());
        assert!(php(&mut plugin, "        $form->get('title')->foo('|');").is_empty());
        assert!(php(&mut plugin, "        $builder->add('x', ChoiceType::class, ['a' => '|']);").is_empty());
    }

    #[test]
    fn stimulus_controllers_vite_entries_and_assetmapper_paths_complete_and_navigate() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("assets/controllers/hello_controller.js", "export default class {}");
        host.add_file("assets/controllers/users/list_item_controller.js", "export default class {}");
        host.add_file("assets/controllers/helper.js", "export const x = 1;");
        host.add_file("assets/app.js", "import './styles/app.css';");
        host.add_file("assets/images/logo.png", "png");
        host.add_file("vite.config.js", "export default { build: { rollupOptions: { input: { app: 'assets/app.js', admin: 'assets/admin.js' } } } }");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/page.html.twig", "twig", text);
        let sorted = |mut items: Vec<String>| {
            items.sort();
            items
        };
        let attribute = "<div data-controller=\"hello \">";
        assert_eq!(sorted(labels(plugin.completion(&twig(attribute), Position::new(0, 28), None))), ["hello", "users--list-item"]);
        let function = "{{ stimulus_controller('') }}";
        assert_eq!(sorted(labels(plugin.completion(&twig(function), Position::new(0, 24), None))), ["hello", "users--list-item"]);
        let jump = plugin.definition(&twig("<div data-controller=\"users--list-item\">"), Position::new(0, 24));
        assert_eq!(jump[0].path, "/project/assets/controllers/users/list_item_controller.js");

        let entries = sorted(labels(plugin.completion(&twig("{{ vite_entry_script_tags('') }}"), Position::new(0, 27), None)));
        assert_eq!(entries, ["admin", "app"]);
        let assets = labels(plugin.completion(&twig("{{ asset('') }}"), Position::new(0, 10), None));
        assert!(assets.contains(&"images/logo.png".to_owned()) && assets.contains(&"app.js".to_owned()), "{assets:?}");
    }

    #[test]
    fn route_parameters_types_tag_and_enum_classes_complete() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("src/Enum/Status.php", "<?php\nnamespace App\\Enum;\nenum Status: string { case Active = 'a'; }\n");
        host.add_file("src/Entity/User.php", "<?php\nnamespace App\\Entity;\nclass User {}\n");
        host.add_file(
            "src/Controller/PostController.php",
            "<?php\nnamespace App\\Controller;\nclass PostController\n{\n    #[Route('/posts/{slug}/page/{page<\\d+>?1}', name: 'post_show')]\n    public function show() {}\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/page.html.twig", "twig", text);
        let sorted = |mut items: Vec<String>| {
            items.sort();
            items
        };
        let at_end = |plugin: &mut Main, document: &Document| {
            let line = document.text.lines().count() as u32 - 1;
            let character = document.text.lines().last().unwrap().len() as u32;
            sorted(labels(plugin.completion(document, Position::new(line, character), None)))
        };

        assert_eq!(at_end(&mut plugin, &twig("{{ path('post_show', { ")), ["page", "slug"]);
        assert_eq!(at_end(&mut plugin, &twig("{{ path('post_show', { slug: 1, ")), ["page", "slug"]);
        assert!(!at_end(&mut plugin, &twig("{{ path('unknown', { ")).contains(&"slug".to_owned()));

        let php = doc("/project/src/Controller/X.php", "php", "<?php\nclass X { function a() { return $this->generateUrl('post_show', ['");
        assert_eq!(at_end(&mut plugin, &php), ["page", "slug"]);

        let types = at_end(&mut plugin, &twig("{% types user: 'App"));
        assert!(types.contains(&"App\\Entity\\User".to_owned()) && types.contains(&"App\\Enum\\Status".to_owned()), "{types:?}");
        let enums = at_end(&mut plugin, &twig("{{ enum_cases('App\\En"));
        assert!(enums.contains(&"App\\Enum\\Status".to_owned()), "{enums:?}");
    }

    #[test]
    fn ux_components_complete_names_props_and_blocks_and_navigate() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file(
            "src/Twig/Components/Alert.php",
            "<?php\nnamespace App\\Twig\\Components;\nuse Symfony\\UX\\TwigComponent\\Attribute\\AsTwigComponent;\n#[AsTwigComponent]\nclass Alert\n{\n    public string $type = 'info';\n    public string $message = '';\n}\n",
        );
        host.add_file("templates/components/Alert.html.twig", "<div class=\"{{ type }}\">{% block content %}{{ message }}{% endblock %}{% block footer %}{% endblock %}</div>");
        host.add_file("templates/components/Button/Primary.html.twig", "{% props label, size = 'md' %}\n<button>{{ label }}</button>");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let twig = |text: &str| doc("/project/templates/page.html.twig", "twig", text);
        let sorted = |mut items: Vec<String>| {
            items.sort();
            items
        };
        let at_end = |plugin: &mut Main, text: &str| {
            let document = twig(text);
            let line = text.lines().count() as u32 - 1;
            sorted(labels(plugin.completion(&document, Position::new(line, text.lines().last().unwrap().len() as u32), None)))
        };

        assert_eq!(at_end(&mut plugin, "<twig:"), ["Alert", "Button:Primary"]);
        assert_eq!(at_end(&mut plugin, "{{ component('') }}".trim_end_matches(" }}").trim_end_matches("')")), ["Alert", "Button:Primary"]);
        assert_eq!(at_end(&mut plugin, "<twig:Alert "), ["message", "type"]);
        assert_eq!(at_end(&mut plugin, "<twig:Button:Primary label=\"x\" "), ["label", "size"]);
        assert_eq!(at_end(&mut plugin, "<twig:Alert>\n<twig:block name=\""), ["content", "footer"]);
        assert!(!at_end(&mut plugin, "<twig:Unknown ").contains(&"type".to_owned()));

        let class_jump = plugin.definition(&twig("<twig:Alert type=\"x\" />"), Position::new(0, 8));
        assert_eq!(class_jump[0].path, "/project/src/Twig/Components/Alert.php");
        let template_jump = plugin.definition(&twig("<twig:Button:Primary />"), Position::new(0, 10));
        assert_eq!(template_jump[0].path, "/project/templates/components/Button/Primary.html.twig");
    }

    #[test]
    fn entities_config_and_env_tabs_list_their_inventories_and_navigate() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file(
            "src/Entity/Post.php",
            "<?php\nnamespace App\\Entity;\nuse Doctrine\\ORM\\Mapping as ORM;\n#[ORM\\Entity]\n#[ORM\\Table(name: 'blog_post')]\nclass Post\n{\n    #[ORM\\Id, ORM\\Column]\n    private ?int $id = null;\n    #[ORM\\Column(length: 255, nullable: true)]\n    private ?string $title = null;\n    #[ORM\\ManyToOne(targetEntity: User::class)]\n    private ?User $author = null;\n}\n",
        );
        host.add_file("src/Entity/Helper.php", "<?php\nnamespace App\\Entity;\nclass Helper {}\n");
        host.add_file("config/services.yaml", "parameters:\n    app.items_per_page: 10\n    app.name: Blog\nservices:\n    _defaults:\n        autowire: true\n");
        host.add_file(".env", "# comment\nAPP_ENV=dev\nDATABASE_URL=\"postgresql://user:secret@db/app\"\n");
        host.add_file(".env.local", "export APP_SECRET=abc\nDATABASE_URL=sqlite:///x\n");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();
        let ctx = host.invocation();

        let mut table = |view: &str| {
            let View { body: Body::Other { kind, data }, .. } = plugin.render(view, None, &ctx).unwrap() else { panic!("not a table") };
            assert_eq!(kind, "resources");
            serde_json::from_str::<serde_json::Value>(&data).unwrap()
        };
        let entities = table(ENTITIES_VIEW);
        assert_eq!(entities["columns"], serde_json::json!(["Entity", "Table", "Fields", "Relations", "Repository", "File"]));
        let items = entities["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["key"], "entity:App\\Entity\\Post");
        assert_eq!(items[0]["cells"], serde_json::json!(["Post", "blog_post", "2", "1", "", "src/Entity/Post.php"]));
        assert!(items[0]["searchText"].as_str().unwrap().contains("title"), "{entities}");
        let details: Vec<_> = items[0]["details"].as_array().unwrap().iter().map(|detail| (detail["label"].as_str().unwrap(), detail["value"].as_str().unwrap())).collect();
        assert_eq!(
            details,
            [("Class", "App\\Entity\\Post"), ("$id", "int, id"), ("$title", "string, nullable"), ("$author", "ManyToOne \u{2192} App\\Entity\\User")]
        );

        let config = table(CONFIG_VIEW);
        assert_eq!(config["columns"], serde_json::json!(["Key", "Value", "File"]));
        assert!(config["items"].as_array().unwrap().iter().any(|item| item["cells"][0] == "app.items_per_page"), "{config}");
        let env_table = table(ENV_VIEW);
        let env: Vec<_> = env_table["items"].as_array().unwrap().iter().map(|item| (item["cells"][0].as_str().unwrap(), item["cells"][2].as_str().unwrap().to_owned())).collect();
        assert_eq!(
            env,
            [
                ("APP_ENV", ".env".to_owned()),
                ("APP_SECRET", ".env.local".to_owned()),
                ("DATABASE_URL", ".env, .env.local".to_owned()),
            ]
        );
        assert!(env.iter().all(|(_, detail)| !detail.contains("secret") && !detail.contains("postgresql")));
        let secret = env_table["items"].as_array().unwrap().iter().find(|item| item["cells"][0] == "APP_SECRET").unwrap();
        assert_eq!(secret["cells"][1], "••••••");

        plugin.handle(ENTITIES_VIEW, ViewEvent::Activate("entity:App\\Entity\\Post".into()), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/Entity/Post.php".into(), 5, 6)]);
    }

    #[test]
    fn about_tab_summarises_the_project_and_opens_bin_console_about_in_a_terminal() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:php"]);
        let mut plugin = activated(&host);
        host.add_file("composer.lock", r#"{"packages":[{"name":"symfony/framework-bundle","version":"v7.1.5"}]}"#);
        let ctx = host.invocation();

        let view = plugin.render(ABOUT_VIEW, None, &ctx).unwrap();
        let Body::Html(html) = &view.body else { panic!("not html") };
        assert!(html.contains("v7.1.5") && html.contains("Source index") && html.contains("Routes"), "{html}");
        assert_eq!(toolbar_labels(&view), ["Run bin/console about", "Reindex"]);

        assert!(plugin.handle(ABOUT_VIEW, ViewEvent::Button("about-console".into()), &ctx).is_err());
        host.add_file("bin/console", "#!/usr/bin/env php");
        plugin.handle(ABOUT_VIEW, ViewEvent::Button("about-console".into()), &ctx).unwrap();
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
        assert!(
            ["if", "for", "block", "extends", "endblock", "endif", "verbatim", "types"].iter().all(|t| tags.contains(&(*t).to_owned()))
        );

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
        assert!(host.diagnostics().get("/project/templates/broken.html.twig").is_none_or(Vec::is_empty));
        let document=Document {path:"/project/templates/broken.html.twig".into(),language:"twig".into(),version:1,text:"{% extends 'missing.html.twig' %}".into()};
        assert_eq!(openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut plugin,&document).len(),1);
    }

    fn console_host() -> (MockHost, Main) {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:php", "editor.read", "editor.write"]);
        let mut plugin = activated(&host);
        host.add_file("src/Command/SyncCommand.php", "<?php\n#[AsCommand(name: 'app:sync')]\nclass SyncCommand {}\n");
        plugin.index_all();
        host.on_process("php", |args, _| {
            assert_eq!(args, ["bin/console", "list", "--raw"]);
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: "about  Display info\ncache:clear  Clear\n".into(),
                stderr: String::new(),
            })
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
    fn run_console_command_lists_inside_a_task() {
        let (host, mut plugin) = console_host();
        host.answer_next_prompt(Some(&["app:sync"]));
        host.answer_next_prompt(Some(&[""]));
        plugin.run_command("run-console-command", &host.invocation(), None).unwrap();
        assert!(!host.tasks().is_empty());
    }

    #[test]
    fn make_args_keep_quoted_value() {
        let (host, mut plugin) = generator_host(&["project.read", "process:php"]);
        host.on_process("php", |_, _| Err(Error::Invalid("no console".into())));
        host.answer_next_prompt(Some(&["controller"]));
        host.answer_next_prompt(Some(&["--message=\"a b\" Blog"]));
        plugin.run_command("make", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["bin/console", "make:controller", "--message=a b", "Blog"]);
        assert!(!host.tasks().is_empty());
    }

    #[test]
    fn completion_excludes_nested_project_entries() {
        let host = MockHost::install();
        host.add_project("/project/packages/inner");
        host.add_file("packages/inner/src/Controller/InnerController.php", "<?php\nclass InnerController\n{\n    #[Route('/inner', name: 'inner_route')]\n    public function i() {}\n}\n");
        let mut plugin = activated(&host);
        plugin.index_all();
        assert!(!plugin.indexes["/project"].routes.contains_key("inner_route"));
        assert!(plugin.indexes["/project"].routes.contains_key("blog_index"));
        let doc = doc("/project/templates/a.twig", "twig", "{{ path('') }}");
        let names: Vec<_> = plugin.completion(&doc, Position::new(0, 9), None).into_iter().map(|item| item.label).collect();
        assert!(names.contains(&"blog_index".to_owned()) && !names.contains(&"inner_route".to_owned()));
    }

    #[test]
    fn saving_controller_updates_index_without_full_build() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        plugin.index_all();
        host.add_file("templates/unrelated.html.twig", "x");
        host.add_file(
            "src/Controller/BlogController.php",
            "<?php\nnamespace App\\Controller;\nclass BlogController\n{\n    #[Route('/blog', name: 'blog_renamed')]\n    public function index() {}\n}\n",
        );
        plugin.handle_events(&[Event::FileSaved("/project/src/Controller/BlogController.php".into())], &host.invocation());
        let index = &plugin.indexes["/project"];
        assert!(index.routes.contains_key("blog_renamed") && !index.routes.contains_key("blog_index"));
        assert!(!index.templates.contains_key("unrelated.html.twig"));
    }

    #[test]
    fn saving_unrelated_controller_stays_incremental_with_a_twig_lexer() {
        let host = MockHost::install();
        host.add_file("src/Twig/Custom.php", "<?php\nuse Twig\\Lexer;\n$twig->setLexer(new Lexer($twig));\n");
        let mut plugin = activated(&host);
        plugin.index_all();
        assert!(plugin.indexes["/project"].custom_twig_lexer());
        host.add_file("templates/unrelated.html.twig", "x");
        host.add_file(
            "src/Controller/BlogController.php",
            "<?php\nnamespace App\\Controller;\nclass BlogController\n{\n    #[Route('/blog', name: 'blog_renamed')]\n    public function index() {}\n}\n",
        );
        plugin.handle_events(&[Event::FileSaved("/project/src/Controller/BlogController.php".into())], &host.invocation());
        let index = &plugin.indexes["/project"];
        assert!(index.routes.contains_key("blog_renamed") && !index.templates.contains_key("unrelated.html.twig"));
        assert!(index.custom_twig_lexer());
        host.add_file("src/Twig/Custom.php", "<?php\nclass Custom {}\n");
        plugin.handle_events(&[Event::FileSaved("/project/src/Twig/Custom.php".into())], &host.invocation());
        assert!(!plugin.indexes["/project"].custom_twig_lexer());
        assert!(!plugin.indexes["/project"].templates.contains_key("unrelated.html.twig"));
    }

    #[test]
    fn saving_composer_json_still_rebuilds() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        plugin.index_all();
        host.add_file("templates/unrelated.html.twig", "x");
        plugin.handle_events(&[Event::FileSaved("/project/composer.json".into())], &host.invocation());
        assert!(plugin.indexes["/project"].templates.contains_key("unrelated.html.twig"));
    }

    fn generator_host(capabilities: &[&str]) -> (MockHost, Main) {
        let host = MockHost::install().with_capabilities(capabilities);
        let mut plugin = activated(&host);
        host.add_file("templates/base.html.twig", "{% block body %}{% endblock %}");
        host.add_file("src/Form/PostType.php", "<?php\nnamespace App\\Form;\n\nclass PostType\n{\n    function f($builder) { $builder->add('title')->add('body', TextareaType::class); }\n}\n");
        plugin.index_all();
        (host, plugin)
    }

    fn assert_names_php_permission(error: &str) {
        assert!(error.contains("needs the process:php permission"), "{error}");
        assert!(error.contains("Settings › Plugins › Symfony"), "{error}");
        assert!(error.contains("php is on your PATH"), "{error}");
    }

    #[test]
    fn console_command_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        let mut plugin = activated(&host);
        let error = plugin.run_command("cache-clear", &host.invocation(), None).unwrap_err().to_string();
        assert_names_php_permission(&error);
    }

    #[test]
    fn console_terminal_denied_names_permission() {
        let (host, mut plugin) = console_host();
        host.answer_next_prompt(Some(&["app:sync"]));
        host.answer_next_prompt(Some(&[""]));
        let host = host.with_capabilities(&["project.read", "events"]);
        let error = plugin.run_command("run-console-command", &host.invocation(), None).unwrap_err().to_string();
        assert_names_php_permission(&error);
    }

    #[test]
    fn make_terminal_denied_names_permission() {
        let (host, mut plugin) = generator_host(&["project.read"]);
        host.answer_next_prompt(Some(&["form"]));
        host.answer_next_prompt(Some(&[""]));
        let error = plugin.run_command("make", &host.invocation(), None).unwrap_err().to_string();
        assert_names_php_permission(&error);
    }

    #[test]
    fn make_runs_the_picked_maker_in_a_terminal() {
        let (host, mut plugin) = generator_host(&["project.read", "process:php"]);
        host.on_process("php", |args, _| {
            assert_eq!(args, ["bin/console", "list", "make", "--raw"]);
            Ok(openidea_plugin::ProcessOutput {
                status: Some(0),
                stdout: "make:controller  Create a controller\nmake:entity  Create an entity\ncache:clear  x\n".into(),
                stderr: String::new(),
            })
        });
        host.answer_next_prompt(Some(&["controller"]));
        host.answer_next_prompt(Some(&["Blog --no-template"]));
        plugin.run_command("make", &host.invocation(), None).unwrap();
        let terminal = &host.terminals()[0];
        assert_eq!((terminal.title.as_str(), terminal.program.as_str()), ("make:controller", "php"));
        assert_eq!(terminal.args, ["bin/console", "make:controller", "Blog", "--no-template"]);
    }

    #[test]
    fn make_falls_back_to_the_built_in_makers_and_stops_when_dismissed() {
        let (host, mut plugin) = generator_host(&["project.read", "process:php"]);
        host.on_process("php", |_, _| Err(Error::Invalid("no console".into())));
        host.answer_next_prompt(Some(&["form"]));
        plugin.run_command("make", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());

        host.answer_next_prompt(Some(&["form"]));
        host.answer_next_prompt(Some(&[""]));
        plugin.run_command("make", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["bin/console", "make:form"]);
    }

    #[test]
    fn create_twig_template_command_writes_file() {
        let (host, mut plugin) = generator_host(&["project.read", "project.write", "editor.read"]);
        let source = "{% include 'blog/new.html.twig' %}\n";
        host.add_file("templates/caller.html.twig", source);
        host.open_editor("/project/templates/caller.html.twig", "twig", source, &[(0, 14, 0, 14)]);
        host.answer_next_prompt(Some(&["empty"]));
        host.answer_next_prompt(Some(&[]));

        plugin.run_command("create-template", &host.invocation(), None).unwrap();

        assert_eq!(host.invocation().project().unwrap().read_file("templates/blog/new.html.twig").unwrap(), "");
        assert_eq!(host.opened_locations(), [("/project/templates/blog/new.html.twig".to_owned(), 0, 0)]);
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn create_twig_template_command_prompts_when_editor_read_is_denied() {
        let (host, mut plugin) = generator_host(&["project.read", "project.write"]);
        host.open_editor("/project/templates/caller.html.twig", "twig", "x\n", &[(0, 0, 0, 0)]);
        host.answer_next_prompt(Some(&["blog/show.html.twig"]));
        host.answer_next_prompt(Some(&[]));

        plugin.run_command("create-template", &host.invocation(), None).unwrap();

        assert_eq!(host.invocation().project().unwrap().read_file("templates/blog/show.html.twig").unwrap(), "");
    }

    #[test]
    fn create_twig_template_command_prompts_without_caret_reference() {
        let (host, mut plugin) = generator_host(&["project.read", "project.write"]);
        for bad in ["../x.html.twig", "a\\b.html.twig", "a//b.html.twig", "/a.html.twig", "a.html"] {
            host.answer_next_prompt(Some(&[bad]));
            let error = plugin.run_command("create-template", &host.invocation(), None).unwrap_err().to_string();
            assert!(error.contains(".twig"), "{bad}: {error}");
        }
        assert!(host.process_runs().is_empty());
        assert!(!host.invocation().project().unwrap().exists("templates/a.html").unwrap());
        assert!(!host.invocation().project().unwrap().exists("templates/x.html.twig").unwrap());

        host.answer_next_prompt(Some(&["blog/show.html.twig"]));
        host.answer_next_prompt(Some(&[]));
        plugin.run_command("create-template", &host.invocation(), None).unwrap();

        assert_eq!(host.invocation().project().unwrap().read_file("templates/blog/show.html.twig").unwrap(), "");
    }

    #[test]
    fn twig_form_rows_are_inserted_from_a_form_type() {
        let (host, mut plugin) = generator_host(&["project.read", "editor.read", "editor.write"]);
        host.open_editor("/project/templates/post.html.twig", "twig", "<h1>x</h1>\n", &[(1, 0, 1, 0)]);
        host.answer_next_prompt(Some(&["App\\Form\\PostType"]));
        plugin.run_command("twig-form-fields", &host.invocation(), None).unwrap();
        assert_eq!(
            host.editor_text(),
            "<h1>x</h1>\n{{ form_start(form) }}\n{{ form_row(form.title) }}\n{{ form_row(form.body) }}\n{{ form_end(form) }}\n"
        );
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
    fn cached_picker_and_tree_targets_reject_new_nested_owner() {
        let (host, mut plugin) = console_host();
        let ctx = host.invocation();
        host.add_project("/project/src/Controller");
        host.answer_next_prompt(Some(&["blog_index"]));
        assert!(matches!(plugin.run_command("go-to-route", &ctx, None), Err(Error::Stale)));
        assert!(matches!(plugin.handle(VIEW, ViewEvent::Activate("route:blog_index".into()), &ctx), Err(Error::Stale)));
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn navigation_target_requires_current_root_and_metadata_grant() {
        let host=MockHost::install().with_capabilities(&["project.read"]); let mut plugin=Main::default();
        plugin.indexes.insert("/project".into(), Index::default());
        assert!(plugin.current_navigation_target(&host.invocation(), "/project", "/project/config/routes.yaml"));
        assert!(!plugin.current_navigation_target(&host.invocation(), "/other", "/other/routes.yaml"));
        host.add_project("/project/config");
        assert!(!plugin.current_navigation_target(&host.invocation(), "/project", "/project/config/routes.yaml"));
        assert!(!plugin.current_navigation_target(&host.invocation(), "/project", ""));
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
    fn literal_completion_filters_newly_foreign_declarations_but_keeps_builtins() {
        let host=MockHost::install().with_capabilities(&["project.read"]);let mut plugin=Main::default();let mut index=Index::default();
        index.routes.insert("known".into(),Entry{detail:None,path:"/project/config/routes.yaml".into(),at:Position::new(0,0)});
        index.routes.insert("builtin".into(),Entry{detail:None,path:String::new(),at:Position::new(0,0)});plugin.indexes.insert("/project".into(),index);
        let doc=doc("/project/templates/a.twig","twig","{{ path('') }}");
        let labels=|items:Vec<CompletionItem>|items.into_iter().map(|item|item.label).collect::<Vec<_>>();
        assert_eq!(labels(plugin.completion(&doc,Position::new(0,9),None)),vec!["builtin","known"]);
        host.add_project("/project/config");
        assert_eq!(labels(plugin.completion(&doc,Position::new(0,9),None)),vec!["builtin"]);
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }

    #[test]
    fn changed_settings_rebuild_on_next_event_without_settings_event_kind() {
        let host=MockHost::install();let mut plugin=activated(&host);
        host.set_setting("publicDirectory",r#""web""#);
        assert!(plugin.view_index(&host.invocation()).is_none());
        plugin.handle_events(&[Event::FileOpened("/project/README.md".into())],&host.invocation());
        assert!(plugin.view_index(&host.invocation()).is_some());
    }

    #[test]
    fn changed_index_settings_hide_cached_providers_until_reindex() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let doc=doc("/project/templates/a.twig","twig","{{ path('blog_index') }}");
        assert_eq!(plugin.definition(&doc,Position::new(0,12)).len(),1);
        host.set_setting("compiledRoutes",r#""custom/routes.php""#);
        assert!(plugin.definition(&doc,Position::new(0,12)).is_empty());
        assert!(plugin.completion(&doc,Position::new(0,12),None).is_empty());
        assert!(plugin.view_index(&host.invocation()).is_none());
        plugin.run_command("reindex",&host.invocation(),None).unwrap();
        assert_eq!(plugin.definition(&doc,Position::new(0,12)).len(),1);
        assert!(plugin.view_index(&host.invocation()).is_some());
    }

    #[test]
    fn cached_definition_requires_current_invoking_and_declaration_owner() {
        let host=MockHost::install().with_capabilities(&["project.read"]);let mut plugin=Main::default();let mut index=Index::default();
        index.routes.insert("known".into(),Entry{detail:None,path:"/project/config/routes.yaml".into(),at:Position::new(3,4)});plugin.indexes.insert("/project".into(),index);
        let document=doc("/project/templates/current.twig","twig","{{ path('known') }}");
        assert_eq!(plugin.definition(&document,Position::new(0,10)).len(),1,"no source files are needed for a positive cached location");
        host.add_project("/project/config");
        assert!(plugin.definition(&document,Position::new(0,10)).is_empty(),"cached declaration is newly foreign");
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }

    #[test]
    fn cold_nested_definition_does_not_fall_back_to_parent_or_cross_root_entry() {
        let host=MockHost::install().with_capabilities(&["project.read"]);let mut plugin=Main::default();let mut parent=Index::default();
        parent.routes.insert("known".into(),Entry{detail:None,path:"/project/config/routes.yaml".into(),at:Position::new(0,0)});plugin.indexes.insert("/project".into(),parent);
        host.add_project("/project/templates");
        let document=doc("/project/templates/current.twig","twig","{{ path('known') }}");
        assert!(plugin.definition(&document,Position::new(0,10)).is_empty(),"cold nested owner cannot borrow parent facts");
        let mut child=Index::default();child.routes.insert("known".into(),Entry{detail:None,path:"/project/config/routes.yaml".into(),at:Position::new(0,0)});plugin.indexes.insert("/project/templates".into(),child);
        assert!(plugin.definition(&document,Position::new(0,10)).is_empty(),"child cache cannot navigate to a foreign parent declaration");
    }

    #[test]
    fn php_dynamic_translation_domain_does_not_borrow_messages_catalog() {
        let host=MockHost::install();let mut plugin=activated(&host);
        for suffix in [", [], $domain)", ", domain: $domain)", ", domain : $domain)", ", ...$args)", ", /* note */ domain: $domain)", ", [], 'messages' . $suffix)", ", [],"] {
            let text=format!("<?php $t->trans('app.title'{suffix};");
            let doc=doc("/project/src/A.php","php",&text);
            let at=text.find("app.title").unwrap()+2;
            assert!(plugin.definition(&doc,Position::new(0,at as u32)).is_empty(),"{text}");
            assert!(plugin.completion(&doc,Position::new(0,at as u32),None).is_empty(),"{text}");
        }
        let doc=doc("/project/src/A.php","php","<?php $t->trans('app.new', domain: $domain);");
        assert!(plugin.code_actions(&doc,Range::new(Position::new(0,18),Position::new(0,18))).iter().all(|a|!a.title.starts_with("Add translation key")));
    }

    #[test]
    fn twig_dynamic_domain_does_not_offer_default_catalog_actions_or_navigation() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let document=doc("/project/templates/a.html.twig","twig","{{ 'app.title'|trans({}, domainName) }}");
        assert!(plugin.definition(&document,Position::new(0,7)).is_empty());
        assert!(plugin.completion(&document,Position::new(0,7),None).is_empty());
        let missing=doc("/project/templates/a.html.twig","twig","{{ 'app.new'|trans({}, domainName) }}");
        let actions=plugin.code_actions(&missing,Range::new(Position::new(0,7),Position::new(0,7)));
        assert!(actions.iter().all(|action|!action.title.starts_with("Add translation key") && !action.title.starts_with("Change to")));
        let known=doc("/project/templates/a.html.twig","twig","{{ 'app.title'|trans({}, 'messages') }}");
        assert!(!plugin.definition(&known,Position::new(0,7)).is_empty());
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
    fn translation_resolution_rejects_foreign_catalog_and_current_duplicate() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let doc=doc("/project/templates/a.html.twig","twig","{{ 'app.new'|trans }}");
        let action=plugin.code_actions(&doc,Range::new(Position::new(0,5),Position::new(0,5))).into_iter().find(|a|a.title.starts_with("Add translation key")).unwrap();
        host.add_file("translations/messages.en.yaml", "'app.new': existing\n");
        assert!(matches!(plugin.resolve_code_action(&doc,action.clone()),Err(Error::Stale)));
        host.add_file("translations/messages.en.yaml", "'other': value\n");host.add_project("/project/translations");
        assert!(matches!(plugin.resolve_code_action(&doc,action),Err(Error::Stale)));
    }

    #[test]
    fn translation_extraction_revalidates_selection_and_target_owner() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let doc=doc("/project/templates/blog/index.html.twig","twig","<p>Read more</p>");
        let action=plugin.code_actions(&doc,Range::new(Position::new(0,3),Position::new(0,12))).into_iter().find(|a|a.title.starts_with("Extract translation")).unwrap();
        let changed=Document{text:"<p>Different</p>".into(),..doc.clone()};
        assert!(matches!(plugin.resolve_code_action(&changed,action.clone()),Err(Error::Stale)));
        host.add_project("/project/translations");
        assert!(matches!(plugin.resolve_code_action(&doc,action),Err(Error::Stale)));
    }

    #[test]
    fn translation_resolution_rejects_current_null_and_branch_key_collisions() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let doc=doc("/project/templates/a.html.twig","twig","{{ 'app.new'|trans }}");
        let action=plugin.code_actions(&doc,Range::new(Position::new(0,5),Position::new(0,5))).into_iter().find(|a|a.title.starts_with("Add translation key")).unwrap();
        for current in ["'app.new':\n", "app:\n  new:\n", "app:\n  new:\n    child: value\n"] {
            host.add_file("translations/messages.en.yaml",current);
            assert!(matches!(plugin.resolve_code_action(&doc,action.clone()),Err(Error::Stale)),"{current}");
        }
    }

    #[test]
    fn translation_resolution_rejects_changed_reference_and_unindexed_catalog() {
        let host=MockHost::install();let mut plugin=activated(&host);
        let doc=doc("/project/templates/a.html.twig","twig","{{ 'app.new'|trans }}");
        let action=plugin.code_actions(&doc,Range::new(Position::new(0,5),Position::new(0,5))).into_iter().find(|a|a.title.starts_with("Add translation key")).unwrap();
        let changed=Document{text:"{{ 'app.new'|trans({}, runtimeDomain) }}".into(),..doc.clone()};
        assert!(matches!(plugin.resolve_code_action(&changed,action.clone()),Err(Error::Stale)));
        let mut forged=action;forged.data=Some("/project/unindexed.yaml\napp.new".into());
        assert!(matches!(plugin.resolve_code_action(&doc,forged),Err(Error::Stale)));
    }

    #[test]
    fn shows_and_opens_names_in_the_tool_window() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        let mut table = |view: &str| {
            let View { body: Body::Other { kind, data }, .. } = plugin.render(view, None, &ctx).unwrap() else { panic!("not a table") };
            assert_eq!(kind, "resources");
            serde_json::from_str::<serde_json::Value>(&data).unwrap()
        };
        let rows = |table: &serde_json::Value| -> Vec<(String, serde_json::Value)> {
            table["items"].as_array().unwrap().iter().map(|item| (item["key"].as_str().unwrap().to_owned(), item["cells"].clone())).collect()
        };

        let templates = table(TEMPLATES_VIEW);
        assert_eq!(templates["columns"], serde_json::json!(["Template", "Namespace", "File", "Rendered by"]));
        assert_eq!(templates["facets"], serde_json::json!([1]));
        assert_eq!(
            rows(&templates),
            [
                ("template:base.html.twig".into(), serde_json::json!(["base.html.twig", "(default)", "templates/base.html.twig", "0"])),
                ("template:blog/index.html.twig".into(), serde_json::json!(["blog/index.html.twig", "(default)", "templates/blog/index.html.twig", "1"])),
            ]
        );

        let services = table(SERVICES_VIEW);
        assert_eq!(services["columns"], serde_json::json!(["Service", "Class", "Kind", "File"]));
        assert_eq!(services["facets"], serde_json::json!([2, 3]));
        assert_eq!(rows(&services), [("service:app.mailer".into(), serde_json::json!(["app.mailer", "App\\Mailer", "Service", "config/services.yaml"]))]);

        let translations = table(TRANSLATIONS_VIEW);
        assert_eq!(translations["columns"], serde_json::json!(["Key", "Domain", "Value", "File"]));
        assert_eq!(translations["facets"], serde_json::json!([1]));
        assert_eq!(
            rows(&translations),
            [
                ("translation:admin:menu".into(), serde_json::json!(["menu", "admin", "Menu", "translations/admin.en.yaml"])),
                ("translation:messages:app.title".into(), serde_json::json!(["app.title", "messages", "Blog", "translations/messages.en.yaml"])),
            ]
        );
        assert!(translations["items"].as_array().unwrap().iter().all(|item| item["icon"] == "text"));

        plugin.handle(TRANSLATIONS_VIEW, ViewEvent::Activate("translation:admin:menu".into()), &ctx).unwrap();
        plugin.handle(SERVICES_VIEW, ViewEvent::Activate("service:app.mailer".into()), &ctx).unwrap();
        plugin.render(VIEW, None, &ctx).unwrap();
        plugin.handle(VIEW, ViewEvent::Activate("route:blog_index".into()), &ctx).unwrap();
        assert_eq!(
            host.opened_locations(),
            [
                ("/project/translations/admin.en.yaml".into(), 0, 0),
                ("/project/config/services.yaml".into(), 1, 4),
                ("/project/src/Controller/BlogController.php".into(), 4, 28),
            ]
        );
    }

    fn toolbar_labels(view: &View) -> Vec<String> {
        view.toolbar.iter().map(|button| button.label.clone()).collect()
    }

    #[test]
    fn symfony_window_has_a_tab_per_inventory_with_counts() {
        let host = MockHost::install();
        let plugin = activated(&host);
        let ctx = host.invocation();

        let config: serde_json::Value = serde_json::from_str(&plugin.web_config(&ctx)).unwrap();
        let counts: Vec<_> = config["group"]["tabs"].as_array().unwrap().iter().map(|tab| (tab["id"].as_str().unwrap(), tab["count"].as_u64())).collect();
        assert_eq!(
            counts,
            [
                (ABOUT_VIEW, None),
                (VIEW, Some(1)),
                (ENTITIES_VIEW, Some(0)),
                (database::VIEW, None),
                (TEMPLATES_VIEW, Some(2)),
                (browse_tabs::COMPONENTS_VIEW, Some(0)),
                (SERVICES_VIEW, Some(1)),
                (browse_tabs::EVENTS_VIEW, Some(0)),
                (CONFIG_VIEW, Some(0)),
                (ENV_VIEW, Some(0)),
                (TRANSLATIONS_VIEW, Some(2)),
                (PROFILER_VIEW, None),
                (namespace_paths::VIEW, None),
                (processes::VIEW, None),
                (route_request::VIEW, None),
                (console_workspace::VIEW, None),
                (console_workspace::GENERATORS_VIEW, None),
                (runtime::VIEW, None),
            ]
        );
        let group: serde_json::Value = serde_json::from_str(include_str!("ui_group.json")).unwrap();
        assert_eq!(group["tabs"].as_array().unwrap().len(), counts.len());
    }

    #[test]
    fn routes_section_lists_controller_method_and_route_name() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        let View { body: Body::List(rows), status, .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "/blog");
        assert_eq!(rows[0].detail.as_deref(), Some("[ANY] BlogController@index \u{b7} blog_index"));
        assert_eq!(status.as_deref(), Some("1 endpoints"));

        plugin.handle(VIEW, ViewEvent::Activate(rows[0].key.clone()), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/Controller/BlogController.php".into(), 4, 13)]);
    }

    #[test]
    fn attribute_methods_give_one_row_each_with_the_controller_method() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file(
            "src/Controller/UserController.php",
            "<?php\nnamespace App\\Controller;\n#[Route('/users')]\nclass UserController\n{\n    #[Route('/{id}', name: 'app_user_show', methods: ['GET', 'POST'])]\n    public function show() {}\n}\n",
        );
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &host.invocation()).unwrap() else { panic!("not a list") };
        let details: Vec<_> = rows.iter().map(|row| (row.label.as_str(), row.detail.clone().unwrap())).collect();
        assert!(details.contains(&("/users/{id}", "[GET] UserController@show \u{b7} app_user_show".to_owned())), "{details:?}");
        assert!(details.contains(&("/users/{id}", "[POST] UserController@show \u{b7} app_user_show".to_owned())), "{details:?}");
    }

    #[test]
    fn filter_and_toggle_buttons_show_only_in_the_routes_section() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        let labels = toolbar_labels(&plugin.render(VIEW, None, &ctx).unwrap());
        assert!(labels.iter().any(|label| label.starts_with("Filter")), "{labels:?}");
        assert!(labels.contains(&"Tree".to_owned()), "{labels:?}");

        let labels = toolbar_labels(&plugin.render(TEMPLATES_VIEW, None, &ctx).unwrap());
        assert!(!labels.iter().any(|label| label.starts_with("Filter") || label == "Tree" || label == "Clear"), "{labels:?}");
    }

    #[test]
    fn non_symfony_project_still_shows_the_message() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"laravel/framework": "^11.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Message(text) = view.body else { panic!("not a message") };
        assert_eq!(text, "This project is not a Symfony application.");
    }

    #[test]
    fn route_row_actions_use_the_selected_route() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };

        plugin.handle(VIEW, ViewEvent::Select(rows[0].key.clone()), &ctx).unwrap();
        host.answer_next_prompt(Some(&["http://localhost:8000"]));
        plugin.run_command("route-copy-curl", &ctx, None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:8000/blog'"));
    }

    #[test]
    fn route_commands_from_the_palette_ask_which_route() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        plugin.render(VIEW, None, &ctx).unwrap();

        host.answer_next_prompt(Some(&["0"]));
        host.answer_next_prompt(Some(&["http://localhost:8000"]));
        plugin.run_command("route-copy-curl", &ctx, None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:8000/blog'"));
    }

    #[test]
    fn send_request_from_the_palette_picks_a_route_and_opens_the_request_tab() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard", "process:php"]);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        assert!(matches!(plugin.render(route_request::VIEW, None, &ctx).unwrap().body, Body::Message(_)));

        host.answer_next_prompt(Some(&["0"]));
        plugin.run_command("route-send-request", &ctx, None).unwrap();
        assert!(host.revealed_views().contains(&"symfony".to_owned()));
        let View { body: Body::Form(fields), toolbar, .. } = plugin.render(route_request::VIEW, None, &ctx).unwrap() else { panic!("request form") };
        assert!(matches!(&fields[0].value, openidea_plugin::views::FieldValue::Other { data, .. } if data.contains("/blog") && data.contains("BlogController::index")));
        assert!(toolbar.iter().any(|button| button.id == "request-send"));

        plugin.handle(route_request::VIEW, ViewEvent::Field("query".into(), "page=2".into()), &ctx).unwrap();
        plugin.handle(route_request::VIEW, ViewEvent::Button("request-copy-curl".into()), &ctx).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:8000/blog?page=2' -H 'Accept: text/html,application/xhtml+xml'"));

        plugin.handle(route_request::VIEW, ViewEvent::Field("method".into(), "DELETE".into()), &ctx).unwrap();
        plugin.handle(route_request::VIEW, ViewEvent::Field("rollback".into(), "false".into()), &ctx).unwrap();
        plugin.handle(route_request::VIEW, ViewEvent::Button("request-send".into()), &ctx).unwrap();
        assert!(host.tasks().iter().all(|task| task.title != "Sending Symfony request"), "a cancelled confirm sends nothing");
        host.answer_next_prompt(Some(&["ok"]));
        plugin.handle(route_request::VIEW, ViewEvent::Button("request-send".into()), &ctx).unwrap();
        let sending = plugin.render(route_request::VIEW, None, &ctx).unwrap();
        assert_eq!(sending.status.as_deref(), Some("Sending…"));
        assert!(matches!(&sending.body, Body::Other { kind, data } if kind == "form" && data.contains(route_request::TICK)));

        let config: serde_json::Value = serde_json::from_str(&plugin.web_config(&ctx)).unwrap();
        assert!(config["opaque_views"].as_array().unwrap().iter().any(|view| view == route_request::VIEW));
        let tick = serde_json::json!({"op": route_request::TICK, "_backgroundPoll": "poll-1"}).to_string();
        let envelope = serde_json::json!({"channel":"ui-kit","sequence":0,"frameId":"0123456789abcdef0123456789abcdef","op":"opaque-message","project":"/project","targetView":route_request::VIEW,"message":tick});
        let notified = host.notifications().len();
        plugin.handle(VIEW, ViewEvent::Message(envelope.to_string()), &ctx).unwrap();
        let acknowledged = host.view_messages().iter().filter_map(|(_, raw)| serde_json::from_str::<serde_json::Value>(raw).ok()).any(|reply| {
            reply["targetView"] == route_request::VIEW && reply["message"].as_str().is_some_and(|message| message.contains(r#""_backgroundPoll":"poll-1""#))
        });
        assert!(acknowledged, "{:?}", host.view_messages());
        assert_eq!(host.notifications().len(), notified, "{:?}", host.notifications());
        assert_eq!(plugin.render(route_request::VIEW, None, &ctx).unwrap().status.as_deref(), Some("Sending…"), "the mock process is still running");

        plugin.route_requests.expire("/project");
        plugin.handle_events(&[], &ctx);
        assert!(!plugin.route_requests.is_sending("/project"), "an event batch stops the overdue request of a hidden tab");
        let View { body: Body::Form(fields), .. } = plugin.render(route_request::VIEW, None, &ctx).unwrap() else { panic!("request form") };
        assert!(format!("{fields:?}").contains("The request ran longer than 60 seconds and was stopped."));

        plugin.handle(route_request::VIEW, ViewEvent::Button(route_request::CANCEL.into()), &ctx).unwrap();
        assert!(matches!(plugin.render(route_request::VIEW, None, &ctx).unwrap().body, Body::Form(_)));
    }

    #[test]
    fn choose_route_in_the_request_tab_always_asks_and_keeps_the_form_when_dismissed() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("config/routes.yaml", "news_list:\n    path: /news\n    controller: App\\Controller\\BlogController::index\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };
        let selected = rows.iter().find(|row| plugin.window.as_ref().unwrap().routes.route(&row.key).is_some_and(|route| route.path == "/blog")).unwrap().key.clone();
        plugin.handle(VIEW, ViewEvent::Select(selected), &ctx).unwrap();
        plugin.run_command("route-send-request", &ctx, None).unwrap();
        let route_field = |plugin: &mut Main| {
            let View { body: Body::Form(fields), toolbar, .. } = plugin.render(route_request::VIEW, None, &ctx).unwrap() else { panic!("request form") };
            assert!(toolbar.iter().any(|button| button.id == route_request::CHOOSE_ROUTE && button.label == "Choose Route…"));
            let openidea_plugin::views::FieldValue::Other { data, .. } = &fields[0].value else { panic!("route") };
            data.clone()
        };
        let first = route_field(&mut plugin);
        plugin.handle(route_request::VIEW, ViewEvent::Field("query".into(), "page=2".into()), &ctx).unwrap();

        host.answer_next_prompt(None);
        plugin.handle(route_request::VIEW, ViewEvent::Button(route_request::CHOOSE_ROUTE.into()), &ctx).unwrap();
        assert_eq!(route_field(&mut plugin), first, "a dismissed picker keeps the form");
        assert_eq!(plugin.route_requests.request("/project").unwrap().curl("http://h"), "curl -X GET 'http://h/blog?page=2' -H 'Accept: text/html,application/xhtml+xml'");

        let index = plugin.view_index(&ctx).unwrap();
        let other = plugin.routes(index).into_iter().position(|route| route.path != "/blog").expect("a second route");
        let path = plugin.routes(index)[other].path.clone();
        host.answer_next_prompt(Some(&[&other.to_string()]));
        plugin.handle(route_request::VIEW, ViewEvent::Button(route_request::CHOOSE_ROUTE.into()), &ctx).unwrap();
        assert!(route_field(&mut plugin).contains(&path), "the selected Routes row is not reused");
    }

    #[test]
    fn route_pickers_name_what_they_are_for() {
        assert_eq!(route_pick_title("route-copy-url"), "Copy URL of which route?");
        assert_eq!(route_pick_title("route-copy-curl"), "Copy cURL of which route?");
        assert_eq!(route_pick_title("route-create-http"), "Create a request file for which route?");
        assert_eq!(route_pick_title("route-send-request"), "Open which route in the Request tab?");
    }

    #[test]
    fn send_request_on_a_routes_row_uses_that_route() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };
        plugin.handle(VIEW, ViewEvent::Select(rows[0].key.clone()), &ctx).unwrap();
        plugin.run_command("route-send-request", &ctx, Some(&rows[0].key)).unwrap();
        let View { body: Body::Form(fields), .. } = plugin.render(route_request::VIEW, None, &ctx).unwrap() else { panic!("request form") };
        let openidea_plugin::views::FieldValue::Choice(methods, _) = &fields[1].value else { panic!("methods") };
        assert_eq!(methods.len(), 5, "an ANY route offers the usual methods");
    }

    #[test]
    fn dismissed_base_url_prompt_copies_nothing() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };

        plugin.handle(VIEW, ViewEvent::Select(rows[0].key.clone()), &ctx).unwrap();
        host.answer_next_prompt(None);
        plugin.run_command("route-copy-curl", &ctx, None).unwrap();
        assert_eq!(host.clipboard(), None);
    }

    #[test]
    fn project_without_routes_shows_the_empty_message() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        host.add_file("config/routes.yaml", "");
        host.add_file("src/Controller/BlogController.php", "<?php\nclass BlogController {}\n");
        plugin.run_command("reindex", &host.invocation(), None).unwrap();

        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Message(ref text) if text.contains("No routes")), "{:?}", view.body);
    }

    #[test]
    fn filter_on_a_vanished_controller_falls_back_to_all_routes() {
        let host = MockHost::install();
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        host.add_file(
            "src/Controller/UserController.php",
            "<?php\nnamespace App\\Controller;\nclass UserController\n{\n    #[Route('/users', name: 'users')]\n    public function index() {}\n}\n",
        );
        plugin.run_command("reindex", &ctx, None).unwrap();
        plugin.render(VIEW, None, &ctx).unwrap();
        host.answer_next_prompt(Some(&["UserController"]));
        plugin.handle(VIEW, ViewEvent::Button("routes:filter".into()), &ctx).unwrap();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };
        assert_eq!(rows.len(), 1);

        host.add_file("src/Controller/UserController.php", "<?php\nclass UserController {}\n");
        plugin.run_command("reindex", &ctx, None).unwrap();
        let View { body: Body::List(rows), .. } = plugin.render(VIEW, None, &ctx).unwrap() else { panic!("not a list") };
        assert_eq!(rows.len(), 1);
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
    fn cache_warmup_runs_in_the_docker_target_and_refuses_a_changed_container() {
        use crate::docker_target::tests::{ID, choose_first, raw};
        let host = MockHost::install().with_capabilities(&["project.read", "events", "storage", "process:php", "process:docker"]);
        let mut plugin = activated(&host);
        choose_first(&host, raw());
        host.on_process("docker", |args, _| {
            let stdout = match args[2].as_str() {
                "inspect" => raw().to_string(),
                _ => {
                    assert_eq!(args, ["--host", "unix:///var/run/docker.sock", "exec", "--workdir", "/app", ID, "php", "bin/console", "cache:warmup"]);
                    "ok".into()
                }
            };
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout, stderr: String::new() })
        });
        assert_eq!(plugin.run_command("cache-warmup", &host.invocation(), None).unwrap(), None);
        let programs: Vec<_> = host.process_runs().into_iter().map(|run| (run.program, run.args[2].clone())).collect();
        assert_eq!(programs, [("docker".into(), "inspect".into()), ("docker".into(), "exec".into())]);

        host.on_process("docker", |args, _| {
            assert_eq!(args[2], "inspect", "a changed container must not run PHP");
            let mut restarted = raw();
            restarted["StartedAt"] = serde_json::json!("2026-11-12T00:00:00Z");
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: restarted.to_string(), stderr: String::new() })
        });
        let error = plugin.run_command("cache-warmup", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("was restarted") && error.ends_with("Run Symfony: Choose Docker Target… again."), "{error}");

        plugin.run_command("docker-clear-target", &host.invocation(), None).unwrap();
        host.on_process("php", |args, _| {
            assert_eq!(args, ["bin/console", "cache:warmup"]);
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "ok".into(), stderr: String::new() })
        });
        plugin.run_command("cache-warmup", &host.invocation(), None).unwrap();
        assert_eq!(host.process_runs().last().unwrap().program, "php");
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
            for expected in [
                "kernel.event_listener",
                "console.command",
                "twig.extension",
                "app.flow_tag",
                "app.block_tag",
                "app.bare_tag",
                "app.list_tag",
                "app.xml_tag",
                "app.real",
            ] {
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
        quiet(
            &mut plugin,
            SERVICES_FILE,
            "yaml",
            "services:\n    a:\n        tags:\n            - { name: x }\n        calls:\n            - |\n",
        );
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

        let commented =
            complete_marked(&mut plugin, SERVICES_FILE, "yaml", "services:\n    a:\n        tags:\n            # - { name: |\n");
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
    fn imported_macros_are_completed_after_the_alias() {
        let host = MockHost::install();
        host.add_file("templates/forms.html.twig", "{% macro input(name, type = 'text') %}x{% endmacro %}{% macro row() %}{% endmacro %}");
        let mut plugin = activated(&host);

        let offered =
            complete_marked(&mut plugin, "/project/templates/a.html.twig", "twig", "{% import 'forms.html.twig' as f %}{{ f.in| }}");
        assert_eq!(offered, ["input", "row"]);

        let own = complete_marked(
            &mut plugin,
            "/project/templates/a.html.twig",
            "twig",
            "{% import _self as me %}{% macro mine() %}{% endmacro %}{{ me.| }}",
        );
        assert_eq!(own, ["mine"]);
    }

    #[test]
    fn twig_namespaces_setting_resolves_templates() {
        let host = MockHost::install();
        host.add_dir("templates");
        host.add_dir("templates/mail");
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
        host.add_dir("templates");
        host.add_dir("templates/mail");
        host.add_dir("templates/pdf");
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
        host.add_dir("templates");
        host.add_dir("templates/ok");
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
        host.add_file(profiler::INDEX_FILE, "aaaaaa,1.1.1.1,GET,/old,1700000000,,200\nbbbbbb,1.1.1.1,POST,/new,1700000500,,500\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();

        assert!(host.view_messages().iter().all(|(view,_)|view==VIEW));
        let before=host.view_messages().len();
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        assert_eq!(ui_kit::web::active_view(VIEW,include_str!("ui_config.json")).unwrap(),PROFILER_VIEW);
        assert_eq!(host.revealed_views().last().map(String::as_str),Some(VIEW));
        assert_eq!(&host.view_messages()[before..], &[(VIEW.to_owned(), r#"{"channel":"ui-kit-refresh"}"#.to_owned())]);
        let View { body: Body::Table(table), .. } = plugin.render(PROFILER_VIEW, None, &ctx).unwrap() else { panic!("not a table") };
        assert_eq!(table.rows[0], ["2023-11-14 22:21:40", "POST", "500", "/new", "1.1.1.1", "bbbbbb"]);
        assert_eq!(table.rows.len(), 2);
        assert_eq!(host.view_messages().len(), before + 1, "render must not send another invalidation");

        plugin.run_command("show-profiler", &ctx, None).unwrap();
        assert!(host.refreshed_views().contains(&VIEW.to_owned()));
        assert_eq!(host.view_messages().len(), before + 2);
        assert!(host.view_messages()[before..].iter().all(|(view, message)| view == VIEW && message == r#"{"channel":"ui-kit-refresh"}"#));
    }

    #[test]
    fn profiler_filesystem_events_mark_cached_details_stale_without_adoption() {
        let host = MockHost::install();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,/old,1,,200\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        let key = plugin.profiler_snapshot.row_key("abcdef");
        let messages = host.view_messages().len();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,/new,1,,200\n");
        for path in ["/other/var/cache/dev/profiler/index.csv", "/project/var/cache/dev/profiler/index.csv.old", "/project/var/cache/dev/profiler-other"] {
            plugin.handle_events(&[Event::FileSaved(path.into())], &ctx);
        }
        assert_eq!(host.view_messages().len(), messages);
        plugin.handle_events(&[Event::FileOpened(format!("/project/{}", profiler::INDEX_FILE))], &ctx);
        assert_eq!(host.view_messages().len(), messages, "opening alone does not stale cached source");
        plugin.handle_events(&[Event::FileSaved("/project/var/cache/dev/profiler".into())], &ctx);
        assert_eq!(&host.view_messages()[messages..], &[(VIEW.to_owned(), r#"{"channel":"ui-kit-refresh","targetView":"symfony-profiler"}"#.to_owned())]);
        let view = plugin.render(PROFILER_VIEW, None, &ctx).unwrap();
        assert!(view.status.unwrap().contains("profiler index changed"));
        let Body::Table(table) = view.body else { panic!("cached rows") };
        assert_eq!(table.rows[0][3], "/old", "event does not adopt changed record");
        assert_eq!(plugin.profiler_snapshot.row_key("abcdef"), key);
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(key), &ctx).is_err());
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        let view = plugin.render(PROFILER_VIEW, None, &ctx).unwrap();
        assert!(!view.status.unwrap().contains("profiler index changed"));
        let Body::Table(table) = view.body else { panic!("refreshed rows") };
        assert_eq!(table.rows[0][3], "/new");
        for field in 0..3 {
            plugin.run_command("show-profiler", &ctx, None).unwrap();
            let mut files = openidea_plugin::FilesChanged::default();
            let list = match field { 0 => &mut files.created, 1 => &mut files.modified, _ => &mut files.removed };
            list.push(format!("/project/{}", profiler::INDEX_FILE));
            plugin.handle_events(&[Event::FilesChanged(files)], &ctx);
            assert!(plugin.render(PROFILER_VIEW, None, &ctx).unwrap().status.unwrap().contains("profiler index changed"));
        }
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        plugin.handle_events(&vec![Event::FileSaved("/unrelated".into()); 1025], &ctx);
        assert!(plugin.render(PROFILER_VIEW, None, &ctx).unwrap().status.unwrap().contains("profiler index changed"));
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }

    #[test]
    fn profiler_project_source_button_hides_prior_details_until_explicit_refresh() {
        let host = MockHost::install();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,/default,1,,200\n");
        host.add_file("custom/profiler.csv", "abcdef,ip,GET,/custom,1,,200\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        let key = plugin.profiler_snapshot.row_key("abcdef");
        host.answer_next_prompt(Some(&["custom/profiler.csv"]));
        plugin.handle(PROFILER_VIEW, ViewEvent::Button("choose-profiler-source".into()), &ctx).unwrap();
        assert!(matches!(plugin.render(PROFILER_VIEW, None, &ctx).unwrap().body, Body::Message(_)));
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(key), &ctx).is_err());
        plugin.handle(PROFILER_VIEW, ViewEvent::Button("refresh-profiler".into()), &ctx).unwrap();
        let Body::Table(table) = plugin.render(PROFILER_VIEW, None, &ctx).unwrap().body else { panic!("custom rows") };
        assert_eq!(table.rows[0][3], "/custom");
        plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [("/project/custom/profiler.csv".into(),0,0)]);
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }

    #[test]
    fn profiler_discovery_button_selects_bounded_custom_cache_without_adoption() {
        let host = MockHost::install();
        host.set_setting("compiledContainers", "\"build/cache/container.xml\"");
        host.add_file("build/cache/container.xml", "<container/>");
        host.add_file("build/cache/profiler/index.csv", "abcdef,ip,GET,/discovered,1,,200\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        host.answer_next_prompt(Some(&["0"]));
        plugin.handle(PROFILER_VIEW, ViewEvent::Button("discover-profiler-source".into()), &ctx).unwrap();
        assert!(matches!(plugin.render(PROFILER_VIEW, None, &ctx).unwrap().body, Body::Message(_)));
        plugin.handle(PROFILER_VIEW, ViewEvent::Button("refresh-profiler".into()), &ctx).unwrap();
        let Body::Table(table) = plugin.render(PROFILER_VIEW, None, &ctx).unwrap().body else { panic!("discovered rows") };
        assert_eq!(table.rows[0][3], "/discovered");
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }

    #[test]
    fn profiler_view_without_index_explains() {
        let host = MockHost::install();
        let mut plugin = activated(&host);

        let View { body, .. } = plugin.render(PROFILER_VIEW, None, &host.invocation()).unwrap();
        assert!(matches!(body, Body::Message(_)));
    }

    #[test]
    fn profiler_open_public_handler_opens_only_exact_current_owned_index_line() {
        let host = MockHost::install();
        let text = "123abc,ip,GET,/older,1,,200\nabcdef,127.0.0.1,POST,/café😀?private=one,1700000000,,400\n";
        host.add_file(profiler::INDEX_FILE, text);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [(format!("/project/{}", profiler::INDEX_FILE), 1, 0)]);
    }

    #[test]
    fn profiler_open_public_handler_refuses_unknown_changed_and_ambiguous_sources() {
        let host = MockHost::install();
        let original = "abcdef,ip,GET,/a?secret=old,1,,200\n";
        host.add_file(profiler::INDEX_FILE, original);
        let mut plugin = activated(&host);
        let ctx = host.invocation();
        plugin.run_command("show-profiler", &ctx, None).unwrap();
        for key in ["request:unknown", "abcdef", "service:abcdef", "request:../abcdef"] {
            assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(key.into()), &ctx).is_err());
        }
        for changed in [
            "abcdef,ip,POST,/a?secret=old,1,,200\n",
            "\nabcdef,ip,GET,/a?secret=old,1,,200\n",
            "abcdef,ip,GET,/a?secret=old,1,,200\nABCDEF,ip,GET,/a?secret=old,1,,200\n",
            "malformed",
            "",
        ] {
            host.add_file(profiler::INDEX_FILE, changed);
            assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &ctx).is_err());
        }
        host.add_file(profiler::INDEX_FILE, &"x".repeat(1024*1024+1));
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &ctx).is_err());
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn profiler_open_public_handler_rejects_foreign_nested_and_denied_owners() {
        let host = MockHost::install();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,/a,1,,200\n");
        let mut plugin = activated(&host);
        plugin.run_command("show-profiler", &host.invocation(), None).unwrap();
        host.set_project_root(Some("/foreign"));
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &host.invocation()).is_err());
        host.set_project_root(None);
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &host.invocation()).is_err());
        host.set_project_root(Some("/project"));host.add_project("/project/var/cache");
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &host.invocation()).is_err());
        let host = host.with_capabilities(&[]);
        assert!(plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &host.invocation()).is_err());
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn profiler_public_custom_source_uses_current_path_and_handles_empty_missing_scopes() {
        let host=MockHost::install();host.set_setting("profilerIndexPath",r#""build/profiler/index.csv""#);
        host.add_file("build/profiler/index.csv","abcdef,ip,GET,/custom,1,,200\n");host.add_file(profiler::INDEX_FILE,"123abc,ip,GET,/default,1,,200\n");
        let mut plugin=activated(&host);assert!(matches!(plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        plugin.run_command("show-profiler",&host.invocation(),None).unwrap();
        let key=plugin.profiler_snapshot.row_key("abcdef");plugin.handle(PROFILER_VIEW,ViewEvent::Activate(key.clone()),&host.invocation()).unwrap();
        assert_eq!(host.opened_locations(),[("/project/build/profiler/index.csv".into(),0,0)]);
        assert!(plugin.handle(PROFILER_VIEW,ViewEvent::Activate(plugin.profiler_snapshot.row_key("999999")),&host.invocation()).is_err());
        host.set_setting("profilerIndexPath",r#""build/empty.csv""#);host.add_file("build/empty.csv","");
        plugin.handle(PROFILER_VIEW,ViewEvent::Button("refresh-profiler".into()),&host.invocation()).unwrap();
        let Body::Table(table)=plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body else {panic!("valid empty source")};assert!(table.rows.is_empty());
        assert!(plugin.handle(PROFILER_VIEW,ViewEvent::Activate(key),&host.invocation()).is_err());
        host.set_setting("profilerIndexPath",r#""build/missing.csv""#);plugin.run_command("show-profiler",&host.invocation(),None).unwrap();
        assert!(matches!(plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        assert!(host.requests().is_empty()&&host.process_runs().is_empty());
    }

    #[test]
    fn profiler_public_refresh_and_model_hide_nested_owner_changes() {
        let host=MockHost::install();host.add_file(profiler::INDEX_FILE,"abcdef,ip,GET,/owned,1,,200\n");let mut plugin=activated(&host);
        plugin.run_command("show-profiler",&host.invocation(),None).unwrap();
        assert!(matches!(plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body,Body::Table(_)));
        host.add_project("/project/var/cache");
        assert!(matches!(plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        plugin.handle(PROFILER_VIEW,ViewEvent::Button("refresh-profiler".into()),&host.invocation()).unwrap();
        assert!(matches!(plugin.render(PROFILER_VIEW,None,&host.invocation()).unwrap().body,Body::Message(_)));
        assert!(host.opened_locations().is_empty()&&host.requests().is_empty()&&host.process_runs().is_empty());
    }

    #[test]
    fn profiler_open_hidden_url_only_changes_are_accepted_without_caching_raw_values() {
        let host = MockHost::install();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,https://user:old@host/a?secret=old#old,1,,200\n");
        let mut plugin = activated(&host);
        let ctx = host.invocation();plugin.run_command("show-profiler", &ctx, None).unwrap();
        host.add_file(profiler::INDEX_FILE, "abcdef,ip,GET,https://other:new@host/a?secret=new#new,1,,200\n");
        plugin.handle(PROFILER_VIEW, ViewEvent::Activate(plugin.profiler_snapshot.row_key("abcdef")), &ctx).unwrap();
        assert_eq!(host.opened_locations(), [(format!("/project/{}", profiler::INDEX_FILE), 0, 0)]);
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

        let defaults = wrap(
            "    <defaults autowire=\"true\"/>\n    <prototype namespace=\"App\\\" resource=\"../src\"/>\n    <service id=\"app.a\"><argument type=\"service\" id=\"app.a\"/></service>\n",
        );
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
        assert!(
            extra_problems(plugin.index_for("/project/x").unwrap(), Groups::all_on(), &read, &other.path, "yaml", &other.text).is_empty()
        );
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
    fn every_command_is_scoped_and_twig_only_commands_need_a_twig_editor() {
        let manifest = include_str!("../plugin.toml");
        for block in manifest.split("[[contributes.commands]]").skip(1) {
            let block = block.split("\n[").next().unwrap();
            if block.contains("id = \"new-project\"") {
                assert!(!block.contains("when = "), "New Symfony Project… creates the first file: {block}");
                continue;
            }
            let when = block.lines().find_map(|l| l.strip_prefix("when = ")).unwrap_or_else(|| panic!("no when: {block}"));
            let twig_only = block.contains("id = \"twig-extends\"") || block.contains("id = \"twig-form-fields\"");
            assert_eq!(twig_only, when.contains("editorLangId == twig"), "{block}");
        }
    }

    #[test]
    fn every_template_is_registered_and_exists() {
        let manifest = include_str!("../plugin.toml");
        let value = |line: &str, key: &str| {
            line.strip_prefix(key).and_then(|rest| rest.trim_start().strip_prefix("= \"")).map(|rest| rest.trim_end_matches('"').to_owned())
        };
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

        assert_eq!(ids.len(), 13);
        for retired in ["compiler-pass","services-yaml","services-xml","services-php"] {assert!(!ids.contains(&retired.to_owned()));}
        assert_eq!((names.len(), paths.len()), (13, 13));
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
    fn retired_service_template_reference_assets_keep_the_documented_defaults() {
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
    fn status_item_opens_a_menu_of_declared_commands() {
        let host = MockHost::install();
        let _plugin = activated(&host);

        let commands: Vec<String> = host.status_menu("symfony").into_iter().filter_map(|entry| match entry {
            StatusMenuEntry::Action { command, .. } => Some(command),
            _ => None,
        }).collect();
        assert_eq!(commands, ["run-console-command", "show-console", "cache-clear", "go-to-route", "go-to-service", "route-send-request", "show-profiler", "reindex"]);
        assert!(commands.iter().all(|command| include_str!("../plugin.toml").contains(&format!("id = \"{command}\""))), "{commands:?}");
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
        assert_eq!(
            change_titles(&mut plugin, &route, route.text.find("app_hom").unwrap() + 2),
            [("Change to \"app_home\"".to_owned(), "app_home".to_owned())]
        );

        let template = doc(path, "php", "<?php\n$this->render('blog/indx.html.twig');\n");
        assert_eq!(
            change_titles(&mut plugin, &template, template.text.find("indx").unwrap()),
            [("Change to \"blog/index.html.twig\"".to_owned(), "blog/index.html.twig".to_owned())]
        );

        let domain = doc(path, "php", "<?php\n$t->trans('menu', [], 'admn');\n");
        assert_eq!(
            change_titles(&mut plugin, &domain, domain.text.find("admn").unwrap()),
            [("Change to \"admin\"".to_owned(), "admin".to_owned())]
        );

        let twig_path = "/project/templates/a.html.twig";
        let in_twig = doc(twig_path, "twig", "{{ path('app_abot') }}{{ 'app.titl'|trans({}, 'messages') }}");
        assert_eq!(
            change_titles(&mut plugin, &in_twig, in_twig.text.find("app_abot").unwrap()),
            [("Change to \"app_about\"".to_owned(), "app_about".to_owned())]
        );
        assert_eq!(
            change_titles(&mut plugin, &in_twig, in_twig.text.find("app.titl").unwrap()),
            [("Change to \"app.title\"".to_owned(), "app.title".to_owned())]
        );
    }

    #[test]
    fn typo_fix_is_quiet_for_ties_dynamic_names_and_defined_names() {
        let host = MockHost::install();
        host.add_file("var/cache/dev/url_generating_routes.php", "<?php\n\nreturn [\n    'app_hom1' => [[], [], [], []],\n    'app_hom2' => [[], [], [], []],\n    'app_home' => [[], [], [], []],\n];\n");
        let mut plugin = activated(&host);
        let path = "/project/src/Controller/BlogController.php";

        for text in [
            "<?php\n$this->generateUrl('app_hom');\n",
            "<?php\n$this->generateUrl('app_home');\n",
            "<?php\n$this->generateUrl('app_' . $x);\n",
            "<?php\n$this->generateUrl('zzzzzz');\n",
        ] {
            let document = doc(path, "php", text);
            let at = text.find("generateUrl").unwrap() + 14;
            assert!(change_titles(&mut plugin, &document, at).is_empty(), "{text}");
        }
    }

    #[test]
    fn twig_path_replacement_hints_at_exact_route_paths_only() {
        let host = MockHost::install();
        host.add_file(
            "config/routes.yaml",
            "blog:\n    path: /blog\n    controller: App\\C::i\nshow:\n    path: /blog/{id}\nx:\n    path: /dup\ny:\n    path: /dup\n",
        );
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
        use openidea_plugin::file_actions::FileActions;
        let document = doc(path, "php", text);
        let before = &text[..text.find(marker).unwrap()];
        let at = Position::new(before.matches('\n').count() as u32, before.rsplit('\n').next().unwrap().len() as u32);
        let action = plugin
            .file_actions(&document, Range::new(at, at))
            .into_iter()
            .find(|action| action.title.starts_with("Add service definition"))?;
        // Existing parser-output checks also exercise the concrete review requirement.
        // The host confirmation answer is supplied by each test.
        let resolved = plugin.resolve_file_action(&document, action).unwrap();
        Some(CodeAction { title: resolved.title, kind: resolved.kind, edits: resolved.edits, preferred: false, data: None })
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

        host.answer_next_prompt(Some(&[]));
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

        host.answer_next_prompt(Some(&[]));
        let action = service_action(&mut plugin, "/project/src/Security/PostVoter.php", &voter, "class PostVoter").unwrap();
        assert!(
            applied("services:\n    _defaults:\n        autowire: true\n", &action)
                .ends_with("    App\\Security\\PostVoter:\n        tags: ['security.voter']\n")
        );

        let auto = "services:\n    _defaults:\n        autowire: true\n        autoconfigure: true\n";
        host.add_file("config/services.yaml", auto);
        host.answer_next_prompt(Some(&[]));
        let action = service_action(&mut plugin, "/project/src/Security/PostVoter.php", &voter, "class PostVoter").unwrap();
        assert!(applied(auto, &action).ends_with("    App\\Security\\PostVoter: ~\n"));
    }

    #[test]
    fn service_definition_goes_into_xml_when_there_is_no_yaml_file() {
        let host = MockHost::install();
        generator_project(&host, "");
        let xml =
            "<?xml version=\"1.0\"?>\n<container>\n    <services>\n        <service id=\"App\\Other\"/>\n    </services>\n</container>\n";
        host.add_file("config/services.xml", xml);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        host.answer_next_prompt(Some(&[]));
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
        host.answer_next_prompt(Some(&[]));
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", MAILER, "class Mailer").is_some());
        let abstract_text = MAILER.replace("class Mailer", "abstract class Mailer");
        assert!(service_action(&mut plugin, "/project/src/Service/Mailer.php", &abstract_text, "abstract class").is_none());
    }

    fn yaml_actions(host: &MockHost, plugin: &mut Main, text: &str, marker: &str) -> Vec<CodeAction> {
        let document = doc("/project/config/services.yaml", "yaml", text);
        let before = &text[..text.find(marker).unwrap()];
        let at = Position::new(before.matches('\n').count() as u32, before.rsplit('\n').next().unwrap().len() as u32);
        use openidea_plugin::file_actions::FileActions;
        host.add_file(&document.path,text);
        plugin.file_actions(&document,Range::new(at,at)).into_iter().map(|action| {
            host.answer_next_prompt(Some(&[]));
            let resolved=plugin.resolve_file_action(&document,action).unwrap();
            CodeAction { title:resolved.title,kind:resolved.kind,preferred:false,edits:resolved.edits,data:None }
        }).collect()
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
        let actions = yaml_actions(&host, &mut plugin, stale, "$dsn");
        let action = actions.iter().find(|action| action.title == "Resync arguments with the constructor").unwrap();
        assert_eq!(
            yaml_applied(stale, action),
            "services:\n    App\\Service\\Mailer:\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: 'smtp://x'\n\n    App\\Other: ~\n"
        );

        let bare = "services:\n    App\\Service\\Mailer:\n        public: true\n\n    App\\Other: ~\n";
        let actions = yaml_actions(&host, &mut plugin, bare, "public");
        let action = actions.iter().find(|action| action.title == "Resync arguments with the constructor").unwrap();
        assert_eq!(
            yaml_applied(bare, action),
            "services:\n    App\\Service\\Mailer:\n        public: true\n        arguments:\n            $logger: '@App\\Logger'\n            $dsn: ''\n\n    App\\Other: ~\n"
        );
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
            let actions = yaml_actions(&host, &mut plugin, text, marker).into_iter().filter(|action| action.title.starts_with("Resync")).count();
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
        let actions = yaml_actions(&host, &mut plugin, text, "public");
        let action = actions.iter().find(|action| action.title == "Add tags security.voter").unwrap();
        assert_eq!(
            yaml_applied(text, action),
            "services:\n    App\\Security\\PostVoter:\n        public: true\n        tags: ['security.voter']\n\n    App\\Other: ~\n"
        );

        let tagged = text.replace("public: true", "tags: ['x']");
        assert!(yaml_actions(&host, &mut plugin, &tagged, "tags").iter().all(|action| !action.title.starts_with("Add tags")));

        let auto = format!("services:\n    _defaults:\n        autoconfigure: true\n{}", &text["services:\n".len()..]);
        assert!(yaml_actions(&host, &mut plugin, &auto, "public").iter().all(|action| !action.title.starts_with("Add tags")));
    }

    fn xml_actions(host: &MockHost, plugin: &mut Main, text: &str, marker: &str) -> Vec<CodeAction> {
        let document = doc("/project/config/services.xml", "xml", text);
        let before = &text[..text.find(marker).unwrap()];
        let at = Position::new(before.matches('\n').count() as u32, before.rsplit('\n').next().unwrap().len() as u32);
        use openidea_plugin::file_actions::FileActions;
        host.add_file(&document.path,text);
        plugin.file_actions(&document,Range::new(at,at)).into_iter().map(|action| {
            host.answer_next_prompt(Some(&[]));
            let resolved=plugin.resolve_file_action(&document,action).unwrap();
            CodeAction { title:resolved.title,kind:resolved.kind,preferred:false,edits:resolved.edits,data:None }
        }).collect()
    }

    #[test]
    fn xml_services_get_tags_and_constructor_arguments() {
        let host = MockHost::install();
        generator_project(&host, "");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        let text = "<container>\n    <services>\n        <service id=\"App\\Security\\PostVoter\"/>\n        <service id=\"App\\Service\\Mailer\">\n        </service>\n    </services>\n</container>\n";
        let actions = xml_actions(&host, &mut plugin, text, "PostVoter");
        let action = actions.iter().find(|action| action.title == "Add tags security.voter").unwrap();
        assert_eq!(
            yaml_applied(text, action),
            "<container>\n    <services>\n        <service id=\"App\\Security\\PostVoter\">\n            <tag name=\"security.voter\"/>\n        </service>\n        <service id=\"App\\Service\\Mailer\">\n        </service>\n    </services>\n</container>\n"
        );

        let actions = xml_actions(&host, &mut plugin, text, "Mailer");
        let action = actions.iter().find(|action| action.title == "Add arguments from the constructor").unwrap();
        assert!(yaml_applied(text, action).contains("            <argument key=\"$logger\" type=\"service\" id=\"App\\Logger\"/>\n            <argument key=\"$dsn\" type=\"string\"></argument>\n        </service>"));

        let tagged = text.replace("PostVoter\"/>", "PostVoter\"><tag name=\"x\"/></service>");
        assert!(xml_actions(&host, &mut plugin, &tagged, "PostVoter").iter().all(|action| !action.title.starts_with("Add tags")));
    }

    #[test]
    fn translation_extraction_requires_known_default_catalog_domain() {
        let host=MockHost::install();host.add_file("translations/admin.en.yaml","'existing': value\n");let mut plugin=activated(&host);
        for prefix in ["{% trans_default_domain runtimeDomain %}","{% if flag %}{% trans_default_domain 'admin' %}{% endif %}","{% trans_default_domain 'messages' %}{% trans_default_domain 'admin' %}"] {
            let text=format!("{prefix}<p>Read more</p>");let start=text.find("Read more").unwrap();
            let doc=doc("/project/templates/blog/index.html.twig","twig",&text);
            let selection=Range::new(text::position(&text,start),text::position(&text,start+9));
            assert!(plugin.code_actions(&doc,selection).iter().all(|action|!action.title.starts_with("Extract translation")),"{prefix}");
        }
        let text="{% trans_default_domain 'admin' %}<p>Read more</p>";let start=text.find("Read more").unwrap();
        let doc=doc("/project/templates/blog/index.html.twig","twig",text);
        let action=plugin.code_actions(&doc,Range::new(text::position(text,start),text::position(text,start+9))).into_iter().find(|action|action.title.starts_with("Extract translation")).unwrap();
        assert!(action.title.ends_with("admin.en.yaml"));let resolved=plugin.resolve_code_action(&doc,action).unwrap();
        assert_eq!(resolved.edits[1].0,"/project/translations/admin.en.yaml");
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
        host.add_file(
            "src/Entity/Product.php",
            "<?php\nnamespace App\\Entity;\n\nclass Product\n{\n    private string $name;\n    private int $stock;\n}\n",
        );
        let mut plugin = activated(&host);
        let form = "<?php\nnamespace App\\Form;\n\nuse App\\Entity\\Product;\nuse Symfony\\Component\\Form\\AbstractType;\n\nclass ProductType extends AbstractType\n{\n    public function buildForm(FormBuilderInterface $builder, array $options): void\n    {\n        $builder->add('name', TextType::class);\n    }\n\n    public function configureOptions($resolver): void\n    {\n        $resolver->setDefaults(['data_class' => Product::class]);\n    }\n}\n";
        let document = doc("/project/src/Form/ProductType.php", "php", form);
        let at = Position::new(10, 10);

        let action = plugin
            .code_actions(&document, Range::new(at, at))
            .into_iter()
            .find(|action| action.title == "Add form fields for Product")
            .unwrap();
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

        let in_loop =
            complete_marked(&mut plugin, "/project/templates/base.html.twig", "twig", "{% for item in items %}{{ it| }}{% endfor %}");
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

impl openidea_plugin::endpoints::EndpointsProvider for Main {
    fn endpoints(&mut self, ctx: &Invocation) -> Result<Vec<openidea_plugin::endpoints::Endpoint>, Error> {
        let Some(project) = ctx.project() else {
            return Ok(Vec::new());
        };
        self.index_project(&project);
        let Some(index) = self.view_index(ctx) else {
            return Ok(Vec::new());
        };
        if index.partial {
            return Err(Error::Failed("Symfony route index is incomplete; refresh after indexing.".into()));
        }
        Ok(ui_kit::routes::endpoints(&self.routes(index), "Symfony"))
    }
}


#[cfg(test)]
mod grouped_ui_parity_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use serde_json::{Value, json};

    fn send(plugin:&mut Main,host:&MockHost,active:&str,operation:Value)->Value {
        let mut request=operation;
        request["channel"]=json!("ui-kit");request["sequence"]=json!(1);
        request["frameId"]=json!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        request["project"]=json!("/project");request["targetView"]=json!(active);
        plugin.handle(VIEW,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();
        serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap()
    }
    #[test]
    fn twig_path_draft_survives_custom_tabs_and_hidden_tab_fields_refuse() {
        let host=MockHost::install();let mut plugin=Main::default();
        let answer=send(&mut plugin,&host,VIEW,json!({"op":"select-view","target":namespace_paths::VIEW}));
        assert_eq!(answer["model"]["activeView"],namespace_paths::VIEW);
        assert_eq!(answer["model"]["viewTabs"].as_array().unwrap().len(),18);
        let answer=send(&mut plugin,&host,namespace_paths::VIEW,json!({"op":"event","event":{"kind":"button","key":"twig-path-add"}}));
        let fields=answer["model"]["body"]["fields"].as_array().unwrap();
        let field=fields.iter().find(|field|field["label"]=="Existing project-relative directory").unwrap()["key"].as_str().unwrap().to_owned();
        let answer=send(&mut plugin,&host,namespace_paths::VIEW,json!({"op":"event","event":{"kind":"field","key":field,"value":"templates/kept"}}));
        assert!(answer.get("error").is_none(),"{answer}");
        send(&mut plugin,&host,namespace_paths::VIEW,json!({"op":"select-view","target":PROFILER_VIEW}));
        let stale=send(&mut plugin,&host,namespace_paths::VIEW,json!({"op":"event","event":{"kind":"field","key":field,"value":"templates/stale"}}));
        assert!(stale["error"].is_string());
        let answer=send(&mut plugin,&host,PROFILER_VIEW,json!({"op":"select-view","target":namespace_paths::VIEW}));
        assert!(answer["model"]["body"]["fields"].as_array().unwrap().iter().any(|field|field["value"]["text"]=="templates/kept"));
        assert!(host.view_messages().iter().all(|(view,_)|view==VIEW));
    }
}

#[cfg(test)]
mod marker_icon_guard {
    #[test]
    fn marker_icons_are_declared_in_the_manifest_and_exist() {
        let manifest = include_str!("../plugin.toml");
        let declared: Vec<&str> = manifest
            .lines()
            .filter_map(|line| line.strip_prefix("icon = \"icons/marker-"))
            .collect();
        assert_eq!(declared.len(), 1);
        for name in declared {
            let icon = format!("icons/marker-{}", name.trim_end_matches('"'));
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(&icon).is_file(), "{icon}");
        }
    }
}
