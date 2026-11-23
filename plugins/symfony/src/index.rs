//! A text index of a Symfony project: route names, Twig templates, service
//! ids, translation keys, classes, assets, entries and roles, each with where
//! it is defined, and the controllers' render calls.
//!
//! File indexes (routing, templating, dic, translation, assets, security),
//! built from what can be read from the files without PHP/YAML/Twig PSI.

use std::collections::{BTreeMap, BTreeSet};
use std::cell::Cell;
use std::time::{Duration, Instant};

use openidea_plugin::Position;
use openidea_plugin::host::{self, Project};

use crate::refs::TEMPLATE_CALLS;
use crate::route_defs::{self, RouteDef};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub detail: Option<String>,
    /// Absolute path; empty for built-in names.
    pub path: String,
    pub at: Position,
}

pub type Names = BTreeMap<String, Entry>;

/// A UX Twig component with the blocks its template defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComponentInfo {
    pub component: crate::components::Component,
    pub blocks: Vec<String>,
}

/// A controller rendering a template: `$this->render('x.html.twig', ['a' => …])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Render {
    pub template: String,
    /// Keys of the literal parameter array: the template's variables.
    pub variables: Vec<String>,
    /// What the controller's source says about the types of those variables.
    pub variable_types: std::collections::BTreeMap<String, crate::php_types::TypeHint>,
    /// `BlogController::index`.
    pub caller: String,
    pub path: String,
    pub at: Position,
}

/// `{% include 'target' with {…} %}` in the `caller` template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateInclude {
    pub target: String,
    pub variables: Vec<String>,
    pub caller: String,
}

#[derive(Default, Debug)]
pub struct Index {
    /// Captured global settings of a real build; None is an unbuilt synthetic index.
    pub(crate) cache_settings: Option<Vec<Option<String>>>,
    pub routes: Names,
    /// Routes with their path, methods and controller, for the flat list.
    pub route_defs: Vec<RouteDef>,
    pub templates: Names,
    /// Bounded build observations passed; not opaque host-index completeness proof.
    pub template_scan_observed: bool,
    pub template_directories: Vec<String>,
    pub ambiguous_templates: std::collections::BTreeSet<String>,
    pub template_paths_unknown: bool,
    pub services: Names,
    /// Services autowiring can reach without a literal definition: resource-loaded FQCN ids and `#[AsAlias]` ids.
    pub autowired: Names,
    /// Every route name is literal in scanned files (no compiled routes needed).
    pub routes_complete: bool,
    /// Keys by domain.
    pub translations: BTreeMap<String, Names>,
    /// YAML translation files by domain, where missing keys can be added.
    pub translation_files: BTreeMap<String, Vec<String>>,
    /// Project classes (PSR-4 autoload) and their files.
    pub classes: Names,
    /// PSR-4 namespace prefixes of the project's own code.
    pub psr4_prefixes: Vec<String>,
    /// The exact source-reader pairs matched strict literal production Composer mappings.
    pub psr4_source_verified: bool,
    pub assets: Names,
    /// Webpack Encore and AssetMapper entry points.
    pub entries: Names,
    /// Stimulus controllers of `assets/controllers/`, by their `data-controller` identifier.
    pub stimulus: Names,
    /// UX Twig components by name (`Alert`, `Button:Primary`); the entry points at the class, else the template.
    pub components: Names,
    pub component_info: std::collections::BTreeMap<String, ComponentInfo>,
    /// Doctrine entities by class, from `#[ORM\Entity]` attributes.
    pub entities: std::collections::BTreeMap<String, crate::entities::Entity>,
    /// Variable names of the `.env*` files; values are never kept.
    pub env: Names,
    /// Masked display values of env variables, last defining file wins.
    pub env_values: std::collections::BTreeMap<String, String>,
    pub roles: Names,
    /// Container parameters of the project's config and of the compiled container.
    pub parameters: Names,
    pub commands: Names,
    /// Service tags used in `config/**`, at their first use.
    pub tags: Names,
    pub renders: Vec<Render>,
    /// `twig.globals` of config/packages/twig.yaml.
    pub twig_globals: Names,
    /// Templates included with variables.
    pub includes: Vec<TemplateInclude>,
    /// `symfony/framework-bundle` constraint of composer.json as (major, minor).
    pub framework_version: Option<(u32, u32)>,
    /// Strict cached Composer requirement evidence for advisory live deprecations only.
    pub deprecation_version: Option<VersionEvidence>,
    /// The compiled routes / container were read: bundles' names are known, so
    /// a missing name is an error.
    pub compiled_routes: bool,
    pub compiled_container: bool,
    /// Some files were skipped to stay within the call's time budget.
    pub partial: bool,
    /// `src/` files that build a Twig Lexer: custom delimiters, which Symfony's own Twig checks do not read.
    pub lexer_files: BTreeSet<String>,
    /// Event listeners and subscribers, by event then highest priority.
    pub events: Vec<crate::events::Listener>,
}

/// Compiled files of `bin/console cache:warmup`, which also hold what bundles
/// define (Settings.java and ServiceContainerUtil.java defaults).
const COMPILED_ROUTES: &[&str] = &["var/cache/dev/url_generating_routes.php", "var/cache/dev/UrlGenerator.php"];
const COMPILED_CONTAINERS: &[&str] = &["var/cache/dev/App_KernelDevDebugContainer.xml", "var/cache/dev/srcApp_KernelDevDebugContainer.xml"];
/// Only cache inputs this index actually reads, plus their directory ancestors.
pub(crate) fn affects_compiled_index(relative: &str) -> bool {
    !relative.is_empty() && COMPILED_ROUTES.iter().chain(COMPILED_CONTAINERS).any(|path|
        *path == relative || path.strip_prefix(relative).is_some_and(|tail| tail.starts_with('/')))
}


/// Security attributes that are not roles (AuthenticatedVoter, Symfony's ExpressionLanguage).
const BUILT_IN_ROLES: &[&str] =
    &["IS_AUTHENTICATED", "IS_AUTHENTICATED_FULLY", "IS_AUTHENTICATED_REMEMBERED", "IS_IMPERSONATOR", "IS_REMEMBERED", "PUBLIC_ACCESS"];

// ponytail: 3 s of reading per build, then the index is partial; incremental per-file updates if projects outgrow it.
const BUDGET: Duration = Duration::from_secs(3);
const MAX_FILES: u32 = 5000;

/// The first `major.minor` of the `symfony/framework-bundle` constraint (`^6.4`, `7.*`).
pub fn framework_version(composer: &str) -> Option<(u32, u32)> {
    let after = &composer[composer.find("\"symfony/framework-bundle\"")? + "\"symfony/framework-bundle\"".len()..];
    let constraint = after.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?.split('"').next()?;
    let mut numbers = constraint.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).map(|n| n.parse::<u32>().ok());
    let major = numbers.next()??;
    Some((major, numbers.next().flatten().unwrap_or(0)))
}

/// A conservative lower bound from a single supported requirement, not installed/runtime truth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionEvidence { pub constraint: String, pub floor: (u32,u32) }
impl VersionEvidence {
    pub fn from_composer(composer: &str) -> Option<Self> {
        if composer.len() > 128 << 10 { return None; }
        let json: serde_json::Value = serde_json::from_str(composer).ok()?;
        let constraint = json.get("require")?.get("symfony/framework-bundle")?.as_str()?.trim();
        if constraint.is_empty() || constraint.len()>128 || constraint.chars().any(char::is_whitespace) { return None; }
        let (body,operator) = [">=", "^", "~"].into_iter().find_map(|op|constraint.strip_prefix(op).map(|s|(s,true))).unwrap_or((constraint,false));
        let parts: Vec<_> = body.split('.').collect();
        if parts.is_empty() || parts.len()>3 { return None; }
        let mut numbers = Vec::new();
        for (i,part) in parts.iter().enumerate() {
            if matches!(*part,"*"|"x"|"X") && i+1==parts.len() && i>0 && !operator { break; }
            if part.is_empty() || !part.bytes().all(|b|b.is_ascii_digit()) || part.len()>1 && part.starts_with('0') { return None; }
            numbers.push(part.parse::<u32>().ok()?);
        }
        let floor = (*numbers.first()?,numbers.get(1).copied().unwrap_or(0));
        Some(Self { constraint: constraint.to_owned(), floor })
    }
}

/// Whether the project is a Symfony application (the plugin's auto-configuration check).
pub fn is_symfony(project: &Project) -> bool {
    if project.read_file("symfony.lock").is_ok() {
        return true;
    }

    project
        .read_file("composer.json")
        .is_ok_and(|composer| composer.contains("\"symfony/framework-bundle\"") || composer.contains("\"symfony/symfony\""))
}

/// Project-relative paths of a comma-separated setting.
fn setting_paths(key: &str) -> Vec<String> {
    let value = host::setting_string(key).unwrap_or_default();
    if value.len() > 32 * 1024 { return Vec::new(); }
    value.split(',').map(str::trim).filter(|p| {
        !p.is_empty() && p.len() <= 4096 && !p.contains(['\\', ':'])
            && !p.chars().any(char::is_control)
            && p.split('/').all(|part| !matches!(part, "" | "." | ".."))
    }).take(128).map(str::to_owned).collect()
}

/// Bounded inputs shared with explicit profiler source discovery; no IO.
pub(crate) fn profiler_container_paths() -> Vec<String> {
    COMPILED_CONTAINERS.iter().map(|p| (*p).to_owned()).chain(setting_paths("compiledContainers")).collect::<std::collections::BTreeSet<_>>().into_iter().collect()
}

/// The same project-relative inputs accepted by the reader, collected once per event batch.
pub(crate) fn configured_input_paths() -> Vec<String> {
    ["compiledRoutes", "compiledContainers", "extraServiceFiles", "extraRoutingFiles"]
        .into_iter().flat_map(setting_paths).collect()
}

pub(crate) fn affects_configured_index(relative: &str, paths: &[String]) -> bool {
    !relative.is_empty() && paths.iter().any(|path| path == relative
        || path.strip_prefix(relative).is_some_and(|tail| tail.starts_with('/')))
}

/// Error/cap observations only; the SDK does not expose host walk/index completeness.
fn discover(project: &Project, globs: &[&str], limit: u32) -> (Vec<String>, bool) {
    let mut paths=Vec::new();let mut partial=false;
    for glob in globs {
        match project.find_files(glob,limit) {
            Ok(found) => {partial |= found.len()>=limit as usize;paths.extend(found);}
            Err(_) => partial=true,
        }
    }
    paths.sort();paths.dedup();(paths,partial)
}
fn optional_text(project: &Project, path: &str, index: &mut Index) -> String {
    match project.read_file(path) {
        Ok(text)=>text,
        Err(openidea_plugin::Error::NotFound(_))=>String::new(),
        Err(_)=>{index.partial=true;String::new()}
    }
}
fn production_psr4_matches(composer: &str, mappings: &[(String,String)]) -> bool {
    let Ok(literal)=crate::psr4::mappings(composer) else {return false;};
    mappings.len()==literal.len() && mappings.iter().all(|(prefix,directory)|literal.iter().any(|mapping| {
        prefix==&format!("{}\\",mapping.namespace) && directory.trim_end_matches('/')==mapping.directory
    }))
}

fn cache_settings() -> Option<Vec<Option<String>>> {
    let values: Vec<_> = ["compiledRoutes","compiledContainers","extraServiceFiles","extraRoutingFiles","publicDirectory","twigNamespaces"]
        .into_iter().map(host::setting_string).collect();
    values.iter().all(|value|value.as_ref().is_none_or(|value|value.len()<=32<<10)).then_some(values)
}
/// A Twig Lexer, not Doctrine's or ExpressionLanguage's.
fn declares_twig_lexer(text: &str) -> bool {
    (text.contains("Twig\\Lexer") && text.contains("Lexer("))
        || (text.contains("setLexer(") && text.contains("Twig"))
}

impl Index {
    pub fn custom_twig_lexer(&self) -> bool {
        !self.lexer_files.is_empty()
    }

    pub(crate) fn note_lexer(&mut self, path: &str, text: &str) {
        match declares_twig_lexer(text) {
            true => self.lexer_files.insert(path.to_owned()),
            false => self.lexer_files.remove(path),
        };
    }

    pub(crate) fn settings_current(&self) -> bool {
        self.cache_settings.as_ref().is_none_or(|expected|cache_settings().as_ref()==Some(expected))
    }
    pub fn build(project: &Project) -> Index {
        // A call gets 5×10⁹ fuel, which BUDGET of parsing can use up on a large project; one
        // that began a task gets 12 times more, so the build can't trap the plugin.
        let _task = host::begin_task("Reading the Symfony project", false);
        let started = Instant::now();
        let mut index = Index::default();
        index.cache_settings=Some(cache_settings().unwrap_or_default());
        let root = project.root().unwrap_or_default();
        let relative = |path: &str| path.strip_prefix(&format!("{root}/")).unwrap_or(path).to_owned();

        let discovery_partial=Cell::new(root.is_empty());
        let nested = crate::nested_roots(&root);
        let files = |globs: &[&str]| -> Vec<String> {
            let (mut paths,partial)=discover(project,globs,MAX_FILES);
            if partial {discovery_partial.set(true);}
            paths.retain(|path| !nested.iter().any(|inner| crate::under(path, inner)));
            paths
        };

        for path in files(&["templates/**/*.twig"]) {
            let name = template_name(relative(&path).trim_start_matches("templates/"));
            index.templates.entry(name).or_insert(Entry { detail: None, path, at: Position::new(0, 0) });
        }

        let twig_config = optional_text(project,"config/packages/twig.yaml",&mut index);
        let configured_paths = twig_paths(&twig_config);
        index.template_directories.extend(configured_paths.iter().map(|(directory, _)| directory.clone()));
        for (directory, namespace) in configured_paths {
            for path in files(&[&format!("{directory}/**/*.twig")]) {
                let Some(file) = relative(&path).strip_prefix(&format!("{directory}/")).map(str::to_owned) else { continue };
                let name = if namespace.is_empty() { file } else { format!("@{namespace}/{file}") };
                index.templates.entry(name).or_insert(Entry { detail: None, path, at: Position::new(0, 0) });
            }
        }

        let lines = Lines::new(&twig_config);
        let (hints, unknown) = crate::namespace_paths::configured(project);
        index.template_paths_unknown = unknown || !hints.is_empty();
        let hint_started = Instant::now();
        let mut hint_files = 0usize;
        index.template_directories.extend(hints.iter().map(|hint| hint.directory.clone()));
        for hint in hints {
            if hint_started.elapsed() > Duration::from_millis(250) || hint_files >= 1024 {
                index.template_paths_unknown = true;
                break;
            }
            if crate::namespace_paths::directory(project, &root, &hint).is_err() {
                index.template_paths_unknown = true;
                continue;
            }
            let mut paths = match project.find_files(&format!("{}/**/*.twig", hint.directory), 129) {
                Ok(paths) => paths,
                Err(_) => {
                    index.template_paths_unknown = true;
                    continue;
                }
            };
            if paths.len() > 128 {
                index.template_paths_unknown = true;
                paths.truncate(128);
            }
            for path in paths {
                if hint_files >= 1024 {
                    index.template_paths_unknown = true;
                    break;
                }
                hint_files += 1;
                let prefix = format!("{root}/{}/", hint.directory);
                let Some(file) = path.strip_prefix(&prefix).filter(|file| !file.is_empty()) else {
                    index.template_paths_unknown = true;
                    continue;
                };
                let name = if hint.namespace.is_empty() { file.to_string() } else { format!("@{}/{file}", hint.namespace) };
                if index.ambiguous_templates.contains(&name) {
                    continue;
                }
                if let Some(existing) = index.templates.get(&name) {
                    if existing.path != path {
                        index.templates.remove(&name);
                        index.ambiguous_templates.insert(name);
                    }
                    continue;
                }
                index.templates.insert(
                    name,
                    Entry { detail: Some("Plugin path hint · may differ at runtime".into()), path, at: Position::new(0, 0) },
                );
            }
        }
        index.template_directories.sort();
        index.template_directories.dedup();

        for key in yaml_keys(&twig_config).into_iter().filter(|key| key.parents == ["twig", "globals"]) {
            let at = lines.position(key.offset);
            index.twig_globals.add(key.key.trim_matches(['\'', '"']), Some("Twig global".into()), "config/packages/twig.yaml", at);
        }

        let public = host::setting_string("publicDirectory").filter(|d| !d.is_empty()).unwrap_or("public".into());
        let public = public.trim_end_matches('/');
        for path in files(&[&format!("{public}/**/*")]) {
            let name = relative(&path).trim_start_matches(&format!("{public}/")).to_owned();
            if !name.ends_with(".php") {
                index.assets.entry(name).or_insert(Entry { detail: None, path, at: Position::new(0, 0) });
            }
        }

        // AssetMapper's logical paths (`asset('images/logo.png')`) and Stimulus controllers live under `assets/`.
        for path in files(&["assets/**/*"]) {
            let Some(name) = relative(&path).strip_prefix("assets/").map(str::to_owned) else { continue };
            if name.is_empty() || name.ends_with(".php") {
                continue;
            }
            if let Some(identifier) = stimulus_identifier(&name) {
                index.stimulus.entry(identifier).or_insert(Entry { detail: Some(name.clone()), path: path.clone(), at: Position::new(0, 0) });
            }
            index.assets.entry(name).or_insert(Entry { detail: Some("AssetMapper path".into()), path, at: Position::new(0, 0) });
        }

        let composer = optional_text(project,"composer.json",&mut index);
        index.framework_version = framework_version(&composer);
        index.deprecation_version = VersionEvidence::from_composer(&composer);
        let class_mappings=psr4(&composer);
        index.psr4_source_verified=production_psr4_matches(&composer,&class_mappings);
        for (namespace, directory) in class_mappings {
            index.psr4_prefixes.push(namespace.clone());
            let directory = format!("{}/", directory.trim_end_matches('/'));
            for path in files(&[&format!("{directory}**/*.php")]) {
                let Some(class) = relative(&path).strip_prefix(&directory).and_then(|p| p.strip_suffix(".php")).map(str::to_owned) else {
                    continue;
                };
                let entry = Entry { detail: None, path, at: Position::new(0, 0) };
                index.classes.insert(format!("{namespace}{}", class.replace('/', "\\")), entry);
            }
        }

        let template_paths: Vec<(String, String)> =
            index.templates.iter().map(|(name, entry)| (name.clone(), entry.path.clone())).collect();
        for (caller, path) in template_paths {
            if started.elapsed() > BUDGET / 2 {
                index.partial=true;
                break;
            }

            let Ok(text) = project.read_file(&path) else { index.partial=true;continue };
            for (target, variables) in crate::twig::includes(&text).into_iter().filter(|(_, variables)| !variables.is_empty()) {
                index.includes.push(TemplateInclude { target, variables, caller: caller.clone() });
            }
        }

        let config = files(&["config/**/*.yaml", "config/**/*.yml", "config/**/*.xml"]);
        let controllers = files(&["src/**/*Controller.php", "src/Controller/**/*.php"]);
        let commands = files(&["src/Command/**/*.php", "src/**/*Command.php"]);
        let translations = files(&["translations/**/*.yaml", "translations/**/*.yml", "translations/**/*.xlf", "translations/**/*.xliff"]);

        let required_files: BTreeSet<_> = config.iter().chain(&controllers).chain(&commands).chain(&translations).cloned()
            .chain(configured_input_paths().into_iter().map(|path|format!("{root}/{path}"))).collect();
        let read = |path: &str, index: &mut Index| -> Option<String> {
            if started.elapsed() > BUDGET {
                index.partial = true;
                return None;
            }
            match project.read_file(path) {
                Ok(text)=>Some(text),
                Err(openidea_plugin::Error::NotFound(_)) if !required_files.contains(path)=>None,
                Err(_)=>{index.partial=true;None}
            }
        };

        let mut resource_defs = Vec::new();
        let mut event_tags = Vec::new();
        let mut routes_literal = true;
        let mut route_files_seen = false;
        let route_php = files(&["config/routes.php", "config/routes/**/*.php"]);
        routes_literal &= route_php.is_empty();
        let kernel = optional_text(project, "src/Kernel.php", &mut index);
        routes_literal &= kernel.find("outes").is_none();
        index.note_lexer(&format!("{root}/src/Kernel.php"), &kernel);

        for path in &config {
            let Some(text) = read(path, &mut index) else { continue };
            let relative = relative(path);

            if relative.contains("security") {
                roles(path, &text, &mut index.roles);
            }

            if relative.ends_with(".xml") {
                routes_literal &= !(text.contains("<routes") && text.contains("<import"));
                xml_services(path, &text, &mut index.services);
                xml_params(path, &text, &mut index.parameters);
                xml_routes(path, &text, &mut index.routes);
                xml_service_tags(path, &text, &mut index.tags);
                event_tags.extend(crate::events::xml_tags(path, &text));
                continue;
            }

            if relative.starts_with("config/routes") {
                route_files_seen = true;
                routes_literal &= yaml_routes_literal(&root, path, &text);
                yaml_routes(path, &text, &mut index.routes);
                index.route_defs.extend(route_defs::yaml_route_defs(path, &text));
                continue;
            }

            routes_literal &= !yaml_keys(&text).iter().any(|key| key.key == "resource" && key.parents == ["framework", "router"]);
            resource_defs.extend(crate::reference_model::resource_defs(path, &text));
            yaml_services(path, &text, &mut index.services);
            yaml_params(path, &text, &mut index.parameters);
            yaml_service_tags(path, &text, &mut index.tags);
            event_tags.extend(crate::events::yaml_tags(path, &text));
        }

        for (key, routes) in [("extraServiceFiles", false), ("extraRoutingFiles", true)] {
            for relative in setting_paths(key) {
                let path = format!("{root}/{relative}");
                let Some(text) = read(&path, &mut index) else { continue };

                match (relative.ends_with(".xml"), routes) {
                    (true, true) => xml_routes(&path, &text, &mut index.routes),
                    (false, true) => {
                        routes_literal &= yaml_routes_literal(&root, &path, &text);
                        yaml_routes(&path, &text, &mut index.routes);
                        index.route_defs.extend(route_defs::yaml_route_defs(&path, &text));
                    }
                    (true, false) => {
                        xml_services(&path, &text, &mut index.services);
                        xml_service_tags(&path, &text, &mut index.tags);
                        event_tags.extend(crate::events::xml_tags(&path, &text));
                    }
                    (false, false) => {
                        resource_defs.extend(crate::reference_model::resource_defs(&path, &text));
                        yaml_services(&path, &text, &mut index.services);
                        yaml_service_tags(&path, &text, &mut index.tags);
                        event_tags.extend(crate::events::yaml_tags(&path, &text));
                    }
                }
            }
        }

        for path in &controllers {
            let Some(text) = read(path, &mut index) else { continue };
            index.note_lexer(path, &text);
            routes_literal &= php_routes_literal(&text);
            php_routes(path, &text, &mut index.routes);
            index.route_defs.extend(route_defs::php_route_defs(path, &text));
            roles(path, &text, &mut index.roles);
            index.renders.extend(php_renders(path, &text));
        }

        for path in &commands {
            let Some(text) = read(path, &mut index) else { continue };
            index.note_lexer(path, &text);
            php_commands(path, &text, &mut index.commands);
        }

        for path in files(&["src/Twig/**/*.php"]) {
            let Some(text) = read(&path, &mut index) else { continue };
            index.note_lexer(&path, &text);
        }

        for path in &translations {
            let Some(text) = read(path, &mut index) else { continue };
            let domain = translation_domain(path).to_owned();
            translation_keys(path, &text, index.translations.entry(domain.clone()).or_default());
            if path.ends_with(".yaml") || path.ends_with(".yml") {
                index.translation_files.entry(domain).or_default().push(path.clone());
            }
        }

        if let Some(text) = read(&format!("{root}/webpack.config.js"), &mut index) {
            encore_entries(&format!("{root}/webpack.config.js"), &text, &mut index.entries);
        }

        if let Some(text) = read(&format!("{root}/importmap.php"), &mut index) {
            importmap_entries(&format!("{root}/importmap.php"), &text, &mut index.entries);
        }

        for config in ["vite.config.js", "vite.config.ts", "vite.config.mjs", "vite.config.mts"] {
            if let Some(text) = read(&format!("{root}/{config}"), &mut index) {
                vite_entries(&format!("{root}/{config}"), &text, &mut index.entries);
            }
        }
        let mut found_components: Vec<(crate::components::Component, String)> = Vec::new();
        for path in files(&["src/**/Components/**/*.php"]) {
            let Some(text) = read(&path, &mut index) else { continue };
            index.note_lexer(&path, &text);
            if !text.contains("AsTwigComponent") && !text.contains("AsLiveComponent") {
                continue;
            }
            if let Some(component) = crate::components::class_component(&text) {
                found_components.push((component, path));
            }
        }
        for path in files(&["templates/components/**/*.twig"]) {
            let relative_path = relative(&path);
            let Some(file) = relative_path.strip_prefix("templates/components/") else { continue };
            let Some(name) = crate::components::anonymous_name(file) else { continue };
            if !found_components.iter().any(|(component, _)| component.name == name) {
                found_components.push((crate::components::Component { name, class: None, template: format!("components/{file}"), props: Vec::new(), live: false }, path));
            }
        }
        for (mut component, path) in found_components {
            let mut blocks = Vec::new();
            let template_path = index.templates.get(&component.template).map(|entry| entry.path.clone());
            if let Some(text) = template_path.and_then(|template_path| read(&template_path, &mut index)) {
                blocks = crate::components::template_blocks(&text);
                for prop in crate::components::template_props(&text) {
                    if !component.props.contains(&prop) {
                        component.props.push(prop);
                    }
                }
            }
            let detail = match &component.class {
                Some(class) => format!("component · {class}"),
                None => "anonymous component".to_owned(),
            };
            index.components.insert(component.name.clone(), Entry { detail: Some(detail), path, at: Position::new(0, 0) });
            index.component_info.insert(component.name.clone(), ComponentInfo { component, blocks });
        }

        let doctrine = optional_text(project, "config/packages/doctrine.yaml", &mut index);
        let entity_globs = entity_globs(&doctrine);
        let entity_globs: Vec<&str> = entity_globs.iter().map(String::as_str).collect();
        for path in files(&entity_globs) {
            let Some(text) = read(&path, &mut index) else { continue };
            index.note_lexer(&path, &text);
            if !text.contains("Entity") {
                continue;
            }
            if let Some(entity) = crate::entities::parse(&path, &text) {
                index.entities.insert(entity.class.clone(), entity);
            }
        }

        let event_files: BTreeSet<String> = files(&crate::events::GLOBS).into_iter().collect();
        for path in &event_files {
            let Some(text) = read(path, &mut index) else { continue };
            index.events.extend(crate::events::php(path, &text));
        }
        let mut class_texts: BTreeMap<String, Option<String>> = BTreeMap::new();
        for tag in event_tags.iter().take(400) {
            let class_path = index.classes.get(&tag.class).map(|entry| entry.path.clone());
            if tag.subscriber && class_path.as_ref().is_some_and(|path| event_files.contains(path)) {
                continue;
            }
            if let Some(path) = class_path.as_ref().filter(|path| !class_texts.contains_key(*path)) {
                let text = read(path, &mut index);
                class_texts.insert(path.clone(), text);
            }
            let text = class_path.as_ref().and_then(|path| class_texts.get(path)?.as_deref());
            index.events.extend(crate::events::tag_listeners(tag, class_path.as_deref().zip(text)));
        }
        crate::events::sort(&mut index.events);

        let mut env_variables: std::collections::BTreeMap<String, (Entry, Vec<&str>)> = std::collections::BTreeMap::new();
        for file in [".env", ".env.local", ".env.dev", ".env.test", ".env.prod", ".env.dev.local", ".env.test.local", ".env.prod.local"] {
            let path = format!("{root}/{file}");
            let Some(text) = read(&path, &mut index) else { continue };
            for (number, line) in text.lines().enumerate() {
                let line = line.trim_start();
                let line = line.strip_prefix("export ").unwrap_or(line);
                let Some((name, raw_value)) = line.split_once('=') else { continue };
                let name = name.trim();
                let valid = name.bytes().next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_') && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
                if valid {
                    index.env_values.insert(name.to_owned(), ui_kit::env_value::display_value(name, raw_value));
                    let slot = env_variables.entry(name.to_owned()).or_insert_with(|| (Entry { detail: None, path: path.clone(), at: Position::new(number as u32, 0) }, Vec::new()));
                    if !slot.1.contains(&file) {
                        slot.1.push(file);
                    }
                }
            }
        }
        for (name, (mut entry, files_defining)) in env_variables {
            entry.detail = Some(format!("defined in {}", files_defining.join(", ")));
            index.env.insert(name, entry);
        }

        for path in files(&["assets/entrypoints/*"]) {
            if let Some(name) = relative(&path).strip_prefix("assets/entrypoints/").map(str::to_owned) {
                index.entries.entry(name).or_insert(Entry { detail: Some("Vite entrypoint".into()), path, at: Position::new(0, 0) });
            }
        }

        let route_files: Vec<String> = COMPILED_ROUTES.iter().map(|p| (*p).to_owned()).chain(setting_paths("compiledRoutes")).collect();
        for relative in route_files {
            let path = format!("{root}/{relative}");
            if let Some(text) = read(&path, &mut index) {
                compiled_routes(&path, &text, &mut index.routes);
                index.compiled_routes = true;
            }
        }

        let container_files: Vec<String> =
            COMPILED_CONTAINERS.iter().map(|p| (*p).to_owned()).chain(setting_paths("compiledContainers")).collect();
        for relative in container_files {
            let path = format!("{root}/{relative}");
            if let Some(text) = read(&path, &mut index) {
                xml_services(&path, &text, &mut index.services);
                xml_params(&path, &text, &mut index.parameters);
                index.compiled_container = true;
            }
        }

        index.routes_complete = routes_literal && route_files_seen && !index.compiled_routes;

        if !index.compiled_container {
            crate::reference_model::resource_services(&resource_defs, &index.classes, &mut index.autowired);
            let candidates: Vec<(String, String)> = index.autowired.keys().filter_map(|class| Some((class.clone(), index.classes.get(class)?.path.clone()))).collect();
            for (class, path) in candidates.into_iter().take(400) {
                if started.elapsed() > BUDGET * 3 / 4 {
                    break;
                }
                let Ok(text) = project.read_file(&path) else { continue };
                for (alias, offset) in crate::reference_model::as_aliases(&text) {
                    let entry = Entry { detail: Some(format!("#[AsAlias] on {class}")), path: path.clone(), at: Lines::new(&text).position(offset) };
                    index.autowired.insert(alias, entry);
                }
            }
        }

        for role in BUILT_IN_ROLES {
            index.roles.add(role, Some("built-in".into()), "", Position::new(0, 0));
        }

        index.partial |= discovery_partial.get() || started.elapsed()>BUDGET;
        index.template_scan_observed=!index.partial;
        index
    }

    /// Re-reads one modified file into the index. False when the change needs a full build.
    pub fn update_file(&mut self, project: &Project, root: &str, relative: &str) -> bool {
        let path = format!("{root}/{relative}");
        let php = relative.starts_with("src/") && relative.ends_with(".php");
        let template = relative.starts_with("templates/") && relative.ends_with(".twig");
        let translation = relative.starts_with("translations/")
            && [".yaml", ".yml", ".xlf", ".xliff"].iter().any(|extension| relative.ends_with(extension));
        if !(php || template || translation) {
            return false;
        }
        let Ok(text) = project.read_file(&path) else { return false };
        let drop_path = |names: &mut Names| names.retain(|_, entry| entry.path != path);

        if template {
            let owns_component = self.component_info.values().any(|info| self.templates.get(&info.component.template).is_some_and(|entry| entry.path == path));
            if owns_component {
                return false;
            }
            self.includes.retain(|include| self.templates.get(&include.caller).is_none_or(|entry| entry.path != path));
            let Some(caller) = self.templates.iter().find(|(_, entry)| entry.path == path).map(|(name, _)| name.clone()) else { return false };
            for (target, variables) in crate::twig::includes(&text).into_iter().filter(|(_, variables)| !variables.is_empty()) {
                self.includes.push(TemplateInclude { target, variables, caller: caller.clone() });
            }
            return true;
        }

        if translation {
            let domain = translation_domain(&path).to_owned();
            drop_path(self.translations.entry(domain.clone()).or_default());
            translation_keys(&path, &text, self.translations.entry(domain).or_default());
            return true;
        }

        let declares_component = text.contains("AsTwigComponent") || text.contains("AsLiveComponent");
        if declares_component || text.contains("AsAlias") || self.components.values().any(|entry| entry.path == path) {
            return false;
        }
        self.note_lexer(&path, &text);
        self.events.retain(|listener| listener.source != path);
        if crate::events::candidate(relative) {
            self.events.extend(crate::events::php(&path, &text));
            crate::events::sort(&mut self.events);
        }

        drop_path(&mut self.routes);
        drop_path(&mut self.roles);
        drop_path(&mut self.commands);
        self.route_defs.retain(|route| route.file != path);
        self.renders.retain(|render| render.path != path);
        self.entities.retain(|_, entity| entity.entry.path != path);

        if relative.ends_with("Controller.php") || relative.starts_with("src/Controller/") {
            self.routes_complete &= php_routes_literal(&text);
            php_routes(&path, &text, &mut self.routes);
            self.route_defs.extend(route_defs::php_route_defs(&path, &text));
            roles(&path, &text, &mut self.roles);
            self.renders.extend(php_renders(&path, &text));
        }
        if relative.starts_with("src/Command/") || relative.ends_with("Command.php") {
            php_commands(&path, &text, &mut self.commands);
        }
        if text.contains("Entity") {
            if let Some(entity) = crate::entities::parse(&path, &text) {
                self.entities.insert(entity.class.clone(), entity);
            }
        }
        true
    }

    /// The Twig name of a template file.
    pub fn template_of(&self, path: &str) -> Option<&str> {
        self.templates.iter().find(|(_, entry)| entry.path == path).map(|(name, _)| name.as_str())
    }
}

/// `autoload.psr-4` of composer.json: (namespace prefix, directory) pairs;
/// `App\` → `src/` when there are none.
pub fn psr4(composer: &str) -> Vec<(String, String)> {
    let body = composer
        .find("\"psr-4\"")
        .and_then(|at| {
            let open = at + composer[at..].find('{')?;
            let close = open + composer[open..].find('}')?;
            Some(&composer[open + 1..close])
        })
        .unwrap_or_default();

    let strings: Vec<&str> = body.split('"').collect();
    let mut pairs = Vec::new();
    let mut i = 1;
    while i + 2 < strings.len() {
        if strings[i + 1].trim() == ":" {
            pairs.push((strings[i].replace("\\\\", "\\"), strings[i + 2].to_owned()));
            i += 4;
            continue;
        }
        i += 2;
    }

    if pairs.is_empty() {
        pairs.push(("App\\".into(), "src/".into()));
    }
    pairs
}

/// Template render calls of a controller, with the keys of their parameter
/// array and the calling method.
pub fn php_renders(path: &str, text: &str) -> Vec<Render> {
    let lines = Lines::new(text);
    let class = path.rsplit('/').next().unwrap_or(path).trim_end_matches(".php");
    let mut renders = Vec::new();

    for call in TEMPLATE_CALLS {
        for (at, _) in text.match_indices(call) {
            let Some((template, offset)) = quoted_at(text, at + call.len()) else { continue };
            let after_literal = offset + template.len() + 1;
            let rest = text[after_literal..].trim_start();
            let entries = rest
                .strip_prefix(',')
                .map(str::trim_start)
                .filter(|r| r.starts_with('['))
                .map(|r| array_entries(text, text.len() - r.len() + 1))
                .unwrap_or_default();
            let variables: Vec<String> = entries.iter().map(|(key, _)| key.clone()).collect();
            let variable_types = crate::php_types::render_variable_types(text, at, &entries);

            let method = text[..at]
                .rfind("function ")
                .map(|f| {
                    let name = &text[f + "function ".len()..];
                    let end = name.bytes().position(|b| !crate::refs::is_identifier(b)).unwrap_or(name.len());
                    &name[..end]
                })
                .unwrap_or("?");

            renders.push(Render {
                template: template.to_owned(),
                variables,
                variable_types,
                caller: format!("{class}::{method}"),
                path: path.to_owned(),
                at: lines.position(offset),
            });
        }
    }

    renders.sort_by_key(|r| (r.at.line, r.at.character));
    renders
}

/// `'key' => expression` pairs at the top level of the array literal starting after `[` at `from`.
fn array_entries(text: &str, from: usize) -> Vec<(String, String)> {
    let end = closing_paren(text, from);
    let bytes = text.as_bytes();
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut depth = 0;
    let mut i = from;
    let mut value_from = None;

    while i < end {
        match bytes[i] {
            b'[' | b'(' | b'{' => depth += 1,
            b']' | b')' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                if let (Some(start), Some(last)) = (value_from.take(), entries.last_mut()) {
                    last.1 = text[start..i].trim().to_owned();
                }
            }
            b'\'' | b'"' => {
                let Some((value, offset)) = quoted_at(text, i) else { break };
                i = offset + value.len();
                if depth == 0 && value_from.is_none() && text[i + 1..].trim_start().starts_with("=>") {
                    entries.push((value.to_owned(), String::new()));
                    value_from = Some(i + 1 + text[i + 1..].find("=>").unwrap_or(0) + 2);
                }
            }
            _ => {}
        }
        i += 1;
    }

    if let (Some(start), Some(last)) = (value_from, entries.last_mut()) {
        last.1 = text[start..end.min(text.len())].trim().trim_end_matches(']').trim().to_owned();
    }
    entries
}

/// `'key' =>` keys at the top level of the array literal starting after `[` at `from`.
#[allow(dead_code)]
fn array_keys(text: &str, from: usize) -> Vec<String> {
    let end = closing_paren(text, from);
    let mut keys = Vec::new();
    let mut depth = 0;
    let mut i = from;
    let bytes = text.as_bytes();

    while i < end {
        match bytes[i] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth -= 1,
            b'\'' | b'"' => {
                let Some((value, offset)) = quoted_at(text, i) else { break };
                i = offset + value.len();
                if depth == 0 && text[i + 1..].trim_start().starts_with("=>") {
                    keys.push(value.to_owned());
                }
            }
            _ => {}
        }
        i += 1;
    }

    keys
}

/// `ROLE_*` names, wherever they appear (security.yaml, `#[IsGranted]`, voters).
pub fn roles(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let bytes = text.as_bytes();

    for (at, _) in text.match_indices("ROLE_") {
        if at > 0 && crate::refs::is_identifier(bytes[at - 1]) {
            continue;
        }

        let end = (at..bytes.len()).find(|&i| !(bytes[i].is_ascii_uppercase() || bytes[i].is_ascii_digit() || bytes[i] == b'_'));
        let role = &text[at..end.unwrap_or(text.len())];
        if role.len() > "ROLE_".len() {
            names.add(role, None, path, lines.position(at));
        }
    }
}

/// `.addEntry('app', …)` / `.addStyleEntry('app', …)` of webpack.config.js.
pub fn encore_entries(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for call in [".addEntry(", ".addStyleEntry("] {
        for (at, _) in text.match_indices(call) {
            let Some((name, offset)) = quoted_at(text, at + call.len()) else { continue };
            let file = text[offset + name.len() + 1..].split_once(',').and_then(|(_, rest)| quoted_at(rest, 0)).map(|(f, _)| f.to_owned());
            names.add(name, file, path, lines.position(offset));
        }
    }
}

/// The `data-controller` identifier of a file under `assets/` (`controllers/users/list_item_controller.js`
/// is `users--list-item`), when it is a Stimulus controller.
pub fn stimulus_identifier(relative: &str) -> Option<String> {
    let rest = relative.strip_prefix("controllers/")?;
    let (stem, extension) = rest.rsplit_once("_controller.")?;
    (!stem.is_empty() && matches!(extension, "js" | "ts" | "jsx" | "tsx" | "mjs")).then(|| stem.replace('/', "--").replace('_', "-"))
}

/// Entries of the `input` option of a Vite config: object keys, or the file paths of an array.
pub fn vite_entries(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let Some(at) = text.find("input:") else { return };
    let after = &text[at + "input:".len()..];
    let Some(open) = after.find(['[', '{']) else { return };
    if after[..open].trim().len() > 0 {
        return;
    }
    let (opener, closer) = if after.as_bytes()[open] == b'[' { ('[', ']') } else { ('{', '}') };
    let body_start = at + "input:".len() + open + 1;
    let Some(length) = text[body_start..].find(closer) else { return };
    let body = &text[body_start..body_start + length];
    let mut offset = 0;
    for item in body.split(',') {
        let item_offset = body_start + offset;
        offset += item.len() + 1;
        let item = item.trim();
        let quoted = |value: &str| -> Option<String> { value.trim().trim_matches(['\'', '"', '`']).to_owned().into() };
        let (name, value) = match (opener, item.split_once(':')) {
            ('{', Some((key, value))) => (quoted(key), quoted(value)),
            ('[', _) => (quoted(item).map(|value| value.trim_start_matches("./").trim_start_matches("assets/").to_owned()), quoted(item)),
            _ => continue,
        };
        let (Some(name), Some(value)) = (name, value) else { continue };
        if !name.is_empty() && !name.contains(['(', ')', ' ', '$']) {
            names.add(&name, Some(format!("Vite entry · {value}")), path, lines.position(item_offset));
        }
    }
}

/// Top-level `'app' => [` entries of AssetMapper's importmap.php.
pub fn importmap_entries(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_offset = offset;
        offset += line.len();

        let indent = line.len() - line.trim_start().len();
        let Some(rest) = line.trim_start().strip_prefix('\'') else { continue };
        let Some(end) = rest.find('\'') else { continue };

        if indent == 4 && rest[end + 1..].trim_start().starts_with("=> [") {
            names.add(&rest[..end], None, path, lines.position(line_offset + indent + 1));
        }
    }
}

/// `templates/` relative path to the Twig name; `bundles/AcmeBundle/x.html.twig`
/// overrides are `@Acme/x.html.twig`.
pub fn template_name(relative: &str) -> String {
    if let Some(rest) = relative.strip_prefix("bundles/")
        && let Some((bundle, file)) = rest.split_once('/')
        && let Some(name) = bundle.strip_suffix("Bundle")
    {
        return format!("@{name}/{file}");
    }

    relative.to_owned()
}

/// `twig: paths:` of config/packages/twig.yaml: (project-relative directory,
/// namespace) pairs, as a mapping `dir: namespace` or a list of directories
/// (an empty namespace is Twig's main one).
pub fn twig_paths(text: &str) -> Vec<(String, String)> {
    let keys = yaml_keys(text);
    let Some(paths) = keys.iter().find(|k| k.key == "paths" && k.parents == ["twig"]) else { return Vec::new() };

    let unquote = |value: &str| value.trim().trim_matches(['\'', '"']).to_owned();
    let mut found = Vec::new();
    let mut paths_indent = None;

    for line in text[paths.offset..].lines().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - trimmed.len();
        let paths_indent = *paths_indent.get_or_insert(indent);
        if indent < paths_indent {
            break;
        }

        let (directory, namespace) = match trimmed.strip_prefix("- ") {
            Some(item) => (unquote(item), String::new()),
            None => match split_yaml_key(trimmed) {
                Some((key, value)) => (key.to_owned(), unquote(value)),
                None => continue,
            },
        };

        let directory = directory.replace("%kernel.project_dir%", "");
        let directory = directory.trim_start_matches("./").trim_matches('/').to_owned();
        let namespace = if namespace == "~" || namespace == "null" { String::new() } else { namespace.trim_start_matches('!').to_owned() };
        if !directory.is_empty() && !directory.contains("..") {
            found.push((directory, namespace));
        }
    }

    found
}

/// Byte offset → editor position, for many offsets of one file.
pub(crate) struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Self { text, starts }
    }

    pub(crate) fn position(&self, at: usize) -> Position {
        let line = self.starts.partition_point(|&start| start <= at) - 1;
        let start = self.starts[line];
        let character = self.text[start..at].chars().map(|c| c.len_utf16() as u32).sum();
        Position::new(line as u32, character)
    }
}

/// Where parsers put the names they find: an index (the first definition
/// wins) or a list of every definition (to find duplicates).
pub trait Sink {
    fn add(&mut self, name: &str, detail: Option<String>, path: &str, at: Position);
}

impl Sink for Names {
    fn add(&mut self, name: &str, detail: Option<String>, path: &str, at: Position) {
        if name.is_empty() {
            return;
        }

        self.entry(name.to_owned()).or_insert(Entry { detail, path: path.to_owned(), at });
    }
}

impl Sink for Vec<(String, Position)> {
    fn add(&mut self, name: &str, _detail: Option<String>, _path: &str, at: Position) {
        if !name.is_empty() {
            self.push((name.to_owned(), at));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YamlKey {
    /// Parent keys, outermost first, without `when@<env>`.
    pub parents: Vec<String>,
    pub key: String,
    /// What follows the colon, trimmed (empty for a mapping).
    pub value: String,
    /// Byte offset of the key.
    pub offset: usize,
}

/// Every mapping key of a YAML document with its parents, from indentation
/// alone. Sequences, flow mappings and block scalar contents are skipped.
pub fn yaml_keys(text: &str) -> Vec<YamlKey> {
    let mut keys = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut block_scalar_indent: Option<usize> = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_offset = offset;
        offset += line.len();

        let content = line.trim_end();
        let trimmed = content.trim_start();
        let indent = content.len() - trimmed.len();

        if trimmed.is_empty() {
            continue;
        }

        if let Some(scalar_indent) = block_scalar_indent {
            if indent > scalar_indent {
                continue;
            }
            block_scalar_indent = None;
        }

        if trimmed.starts_with('#') || trimmed.starts_with('-') || trimmed.starts_with("---") {
            continue;
        }

        let Some((key, value)) = split_yaml_key(trimmed) else { continue };

        while stack.last().is_some_and(|(parent_indent, _)| *parent_indent >= indent) {
            stack.pop();
        }

        if value.starts_with('|') || value.starts_with('>') {
            block_scalar_indent = Some(indent);
        }

        let key_offset = line_offset + indent + usize::from(trimmed.starts_with(['\'', '"']));
        let parents = stack.iter().map(|(_, k)| k.clone()).skip_while(|k| k.starts_with("when@")).collect();
        keys.push(YamlKey { parents, key: key.to_owned(), value: value.to_owned(), offset: key_offset });
        stack.push((indent, key.to_owned()));
    }

    keys
}

pub(crate) fn split_yaml_key(line: &str) -> Option<(&str, &str)> {
    if let Some(quote) = line.chars().next().filter(|c| *c == '\'' || *c == '"') {
        let close = line[1..].find(quote)? + 1;
        let rest = line[close + 1..].trim_start().strip_prefix(':')?;
        return Some((&line[1..close], rest.trim()));
    }

    let bytes = line.as_bytes();
    let colon = (0..bytes.len()).find(|&i| bytes[i] == b':' && bytes.get(i + 1).is_none_or(|b| *b == b' ' || *b == b'\t'))?;
    let key = line[..colon].trim_end();

    if key.is_empty() || key.contains(['{', '[', '"', '\'']) {
        return None;
    }

    Some((key, line[colon + 1..].trim()))
}

/// `services:` keys of a service definition file: ids and FQCNs, not
/// `_defaults`, `_instanceof` or `App\:` resource prefixes.
pub fn yaml_services(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let keys = yaml_keys(text);

    for key in &keys {
        if key.parents != ["services"] || key.key.starts_with('_') || key.key.ends_with('\\') {
            continue;
        }

        let class = keys
            .iter()
            .find(|child| child.key == "class" && child.parents.len() == 2 && child.parents[1] == key.key)
            .map(|child| child.value.trim_matches(['\'', '"']).to_owned());
        let alias = key.value.trim_matches(['\'', '"']).strip_prefix('@').map(|target| format!("alias of {target}")).or_else(|| {
            keys.iter()
                .find(|child| child.key == "alias" && child.parents.len() == 2 && child.parents[1] == key.key)
                .map(|child| format!("alias of {}", child.value.trim_matches(['\'', '"']).trim_start_matches('@')))
        });
        names.add(&key.key, class.or(alias), path, lines.position(key.offset));
    }
}

/// Byte ranges of the tag names in the `tags:` of YAML services: `- { name: x }`,
/// `- name: x`, `tags: ['x']` and `- x`. An empty name is an empty range.
pub(crate) fn yaml_tag_slots(text: &str) -> Vec<(usize, usize)> {
    let mut slots = Vec::new();

    for key in yaml_keys(text) {
        if key.key != "tags" || key.parents.len() != 2 || key.parents[0] != "services" {
            continue;
        }

        let line_start = text[..key.offset].rfind('\n').map_or(0, |i| i + 1);
        let line_end = text[key.offset..].find('\n').map_or(text.len(), |i| key.offset + i);
        let line = text[line_start..line_end].trim_end_matches('\r');
        let Some(colon) = line[key.offset - line_start..].find(':').map(|i| key.offset - line_start + i) else { continue };

        let inline = without_comment(&line[colon + 1..]);
        if !inline.trim().is_empty() {
            flow_tag_slots(inline, line_start + colon + 1, &mut slots);
            continue;
        }

        let tags_indent = line.len() - line.trim_start().len();
        block_tag_slots(text, line_end, tags_indent, &mut slots);
    }

    slots
}

pub(crate) fn without_comment(value: &str) -> &str {
    value.find(" #").map_or(value, |i| &value[..i])
}

fn block_tag_slots(text: &str, after: usize, tags_indent: usize, slots: &mut Vec<(usize, usize)>) {
    let mut offset = after;

    for line in text[after..].split_inclusive('\n') {
        let line_offset = offset;
        offset += line.len();
        let line = line.trim_end_matches(['\n', '\r']);
        let trimmed = line.trim_start();
        if trimmed.is_empty() && line_offset == after {
            continue;
        }

        if trimmed.trim().is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - trimmed.len();
        let is_item = trimmed.starts_with("- ") || trimmed == "-";
        if indent < tags_indent || (indent == tags_indent && !is_item) {
            return;
        }

        let (item, item_offset) = match trimmed.strip_prefix("- ") {
            Some(item) => (item.trim_start(), line_offset + line.len() - item.trim_start().len()),
            None if is_item => continue,
            None => (trimmed, line_offset + indent),
        };
        let item = without_comment(item);

        if item.starts_with('{') {
            mapping_tag_slots(item, item_offset, slots);
            continue;
        }

        if let Some(value) = item.strip_prefix("name:") {
            slots.push(scalar_slot(value, item_offset + 5));
            continue;
        }

        if is_item && !item.contains(": ") && !item.ends_with(':') {
            slots.push(scalar_slot(item, item_offset));
        }
    }
}

/// `[a, { name: b }]`.
fn flow_tag_slots(value: &str, base: usize, slots: &mut Vec<(usize, usize)>) {
    let leading = value.len() - value.trim_start().len();
    let Some(inner) = value.trim_start().strip_prefix('[') else { return };
    let inner = inner.trim_end().strip_suffix(']').unwrap_or(inner);

    for (offset, part) in split_flow(inner, base + leading + 1) {
        let trimmed = part.trim_start();
        if trimmed.starts_with('{') {
            mapping_tag_slots(trimmed, offset + part.len() - trimmed.len(), slots);
            continue;
        }

        if !trimmed.contains(": ") {
            slots.push(scalar_slot(part, offset));
        }
    }
}

/// `{ name: x, priority: 1 }`.
fn mapping_tag_slots(mapping: &str, base: usize, slots: &mut Vec<(usize, usize)>) {
    let Some(inner) = mapping.trim_end().strip_prefix('{') else { return };
    let inner = inner.strip_suffix('}').unwrap_or(inner);

    for (offset, part) in split_flow(inner, base + 1) {
        let trimmed = part.trim_start();
        if let Some(value) = trimmed.strip_prefix("name:") {
            slots.push(scalar_slot(value, offset + part.len() - trimmed.len() + 5));
        }
    }
}

/// `text` split at the commas outside quotes and brackets, with the offsets.
pub(crate) fn split_flow(text: &str, base: usize) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut from) = (0_u32, None, 0);

    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '{' | '[') => depth += 1,
            (None, '}' | ']') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => {
                parts.push((base + from, &text[from..i]));
                from = i + 1;
            }
            _ => {}
        }
    }

    parts.push((base + from, &text[from..]));
    parts
}

/// The unquoted name in a scalar at `base`; an empty one sits at the end.
pub(crate) fn scalar_slot(value: &str, base: usize) -> (usize, usize) {
    let trimmed = value.trim();
    let leading = value.len() - value.trim_start().len();
    if trimmed.is_empty() {
        return (base + value.len().min(1), base + value.len());
    }

    let start = base + leading;
    let Some(quote) = trimmed.chars().next().filter(|c| *c == '\'' || *c == '"') else { return (start, start + trimmed.len()) };
    let inner = &trimmed[1..];
    (start + 1, start + 1 + inner.find(quote).unwrap_or(inner.len()))
}

/// Byte ranges of `name="…"` of the `<tag>` elements of service XML.
pub(crate) fn xml_tag_slots(text: &str) -> Vec<(usize, usize)> {
    let visible = blank_xml_comments(text);

    xml_tags(&visible, "tag")
        .into_iter()
        .filter_map(|(tag, at)| xml_attribute(tag, "name").map(|(value, offset)| (at + offset, at + offset + value.len())))
        .collect()
}

/// The text with the XML comments replaced by spaces, so offsets stay the same.
pub(crate) fn blank_xml_comments(text: &str) -> String {
    let mut visible = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find("<!--") {
        let close = rest[open..].find("-->").map_or(rest.len(), |i| open + i + 3);
        visible.push_str(&rest[..open]);
        visible.extend(rest[open..close].chars().flat_map(|c| {
            let blank = if c == '\n' || c == '\r' { c } else { ' ' };
            std::iter::repeat_n(blank, c.len_utf8())
        }));
        rest = &rest[close..];
    }

    visible.push_str(rest);
    visible
}

/// The tag name slot containing `at`, in a YAML or XML service file.
pub(crate) fn tag_slot_at(language: &str, text: &str, at: usize) -> Option<(usize, usize)> {
    let slots = match language {
        "yaml" => yaml_tag_slots(text),
        "xml" => xml_tag_slots(text),
        _ => return None,
    };

    slots.into_iter().find(|&(start, end)| start <= at && at <= end)
}

fn add_tags(path: &str, text: &str, slots: Vec<(usize, usize)>, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for (start, end) in slots {
        names.add(text[start..end].trim(), None, path, lines.position(start));
    }
}

pub fn yaml_service_tags(path: &str, text: &str, names: &mut impl Sink) {
    add_tags(path, text, yaml_tag_slots(text), names);
}

pub fn xml_service_tags(path: &str, text: &str, names: &mut impl Sink) {
    add_tags(path, text, xml_tag_slots(text), names);
}

/// `name=directory` pairs of the `twigNamespaces` setting as (project-relative
/// directory, namespace); an empty name is Twig's main namespace.
pub fn twig_namespaces(value: &str) -> Vec<(String, String)> {
    value
        .split(',')
        .filter_map(|pair| {
            let (namespace, directory) = pair.split_once('=')?;
            let namespace = namespace.trim().trim_start_matches('@');
            let raw = directory.trim();
            let raw = raw.strip_prefix("%kernel.project_dir%/").unwrap_or(raw);
            if raw.starts_with('/') || raw.starts_with('\\') {
                return None;
            }
            let directory = raw.trim_start_matches("./").trim_end_matches('/').to_string();
            let mapping = crate::namespace_paths::Mapping { namespace: namespace.into(), directory: directory.clone(), enabled: true };
            crate::namespace_paths::valid(&mapping).then(|| (directory, namespace.into()))
        })
        .collect()
}

/// Top-level keys of a routing file, except imports (`resource:`).
pub fn yaml_routes(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let keys = yaml_keys(text);

    for key in keys.iter().filter(|key| key.parents.is_empty()) {
        if key.key.starts_with("when@") {
            continue;
        }

        let is_import = keys.iter().any(|child| child.parents == [key.key.as_str()] && child.key == "resource");
        if is_import {
            continue;
        }

        let route_path = keys
            .iter()
            .find(|child| child.parents == [key.key.as_str()] && child.key == "path")
            .map(|child| child.value.trim_matches(['\'', '"']).to_owned());

        names.add(&key.key, route_path, path, lines.position(key.offset));
    }
}

/// Value of `name="…"` (or `name='…'`) in an XML tag, with its offset.
pub(crate) fn xml_attribute<'a>(tag: &'a str, name: &str) -> Option<(&'a str, usize)> {
    let mut from = 0;

    while let Some(found) = tag[from..].find(name) {
        let at = from + found;
        from = at + name.len();

        let boundary = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[from..];
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };

        if !boundary {
            continue;
        }

        let start = from + 2;
        let end = start + tag[start..].find(quote)?;
        return Some((&tag[start..end], start));
    }

    None
}

/// Start tags `<name …>` of an XML document with their offsets.
pub(crate) fn xml_tags<'a>(text: &'a str, name: &str) -> Vec<(&'a str, usize)> {
    let open = format!("<{name}");

    text.match_indices(open.as_str())
        .filter_map(|(at, _)| {
            let after = text.as_bytes().get(at + open.len())?;
            if !after.is_ascii_whitespace() && *after != b'>' && *after != b'/' {
                return None;
            }

            let end = text[at..].find('>').map_or(text.len(), |i| at + i);
            Some((&text[at..end], at))
        })
        .collect()
}

/// `<service id="…">` of XML definitions and the compiled container. Ids
/// starting with `.` are the container's own.
pub fn xml_services(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for (tag, at) in xml_tags(text, "service") {
        let Some((id, offset)) = xml_attribute(tag, "id") else { continue };
        if id.starts_with('.') {
            continue;
        }

        let detail = xml_attribute(tag, "class")
            .map(|(class, _)| class.to_owned())
            .or_else(|| xml_attribute(tag, "alias").map(|(target, _)| format!("alias of {target}")));
        names.add(id, detail, path, lines.position(at + offset));
    }
}

pub fn xml_routes(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for (tag, at) in xml_tags(text, "route") {
        let Some((id, offset)) = xml_attribute(tag, "id") else { continue };
        let route_path = xml_attribute(tag, "path").map(|(p, _)| p.to_owned());
        names.add(id, route_path, path, lines.position(at + offset));
    }
}

/// End of a parenthesised argument list starting after `(` at `from`,
/// skipping strings.
pub(crate) fn closing_paren(text: &str, from: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 1;
    let mut quote: Option<u8> = None;
    let mut i = from;

    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(_) if b == b'\\' => i += 1,
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'\'' || b == b'"' => quote = Some(b),
            None if b == b'(' || b == b'[' => depth += 1,
            None if b == b')' || b == b']' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            None => {}
        }
        i += 1;
    }

    bytes.len()
}

/// A quoted string starting at `at` (after whitespace): its contents and offset.
fn quoted_at(text: &str, at: usize) -> Option<(&str, usize)> {
    let rest = &text[at..];
    let skipped = rest.len() - rest.trim_start().len();
    let start = at + skipped;
    let quote = text[start..].chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let end = start + 1 + text[start + 1..].find(quote)?;
    Some((&text[start + 1..end], start + 1))
}

/// The `name` argument of a route attribute or annotation (`name: 'x'`, `name="x"`).
fn route_name_argument(text: &str, args: std::ops::Range<usize>) -> Option<(&str, usize)> {
    let inside = &text[args.clone()];
    let mut from = 0;

    while let Some(found) = inside[from..].find("name") {
        let at = from + found;
        from = at + 4;

        let before_ok = at == 0 || !inside.as_bytes()[at - 1].is_ascii_alphanumeric() && inside.as_bytes()[at - 1] != b'_';
        let rest = inside[from..].trim_start();
        let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else { continue };

        if !before_ok {
            continue;
        }

        let value_at = args.start + inside.len() - rest.len();
        return quoted_at(text, value_at);
    }

    None
}

/// Imports of a routing file that only load attribute routes of `src/Controller`
/// (or a `Kernel.php` checked elsewhere) without renaming them. Entries named
/// `_…` (profiler, error pages) only define `_` routes.
fn yaml_routes_literal(root: &str, path: &str, text: &str) -> bool {
    let keys = yaml_keys(text);
    let base = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let controllers = format!("{root}/src/Controller");

    keys.iter().filter(|key| key.parents.is_empty() && !key.key.starts_with("when@") && !key.key.starts_with('_')).all(|entry| {
        let child = |parents: &[&str], name: &str| keys.iter().find(|key| key.key == name && key.parents == parents);
        let Some(resource) = child(&[entry.key.as_str()], "resource") else { return true };
        if child(&[entry.key.as_str()], "name_prefix").is_some() {
            return false;
        }
        let value = if resource.value.is_empty() {
            child(&[entry.key.as_str(), "resource"], "path").map(|key| key.value.clone()).unwrap_or_default()
        } else {
            resource.value.clone()
        };
        let value = without_comment(&value).trim().trim_matches(['\'', '"']).to_owned();
        let resolved = crate::reference_model::normalize(base, &value);
        let local = resolved == format!("{root}/src/Kernel.php") || resolved.trim_end_matches('/') == controllers || resolved.starts_with(&format!("{controllers}/"));
        let kind = child(&[entry.key.as_str()], "type").map(|key| key.value.trim_matches(['\'', '"']).to_owned());
        local && !value.contains(['*', '{', '@']) && kind.is_none_or(|kind| kind == "attribute" || kind == "annotation")
    })
}

/// Globs of the entity folders: `src/Entity`, plus each relative `dir` under `doctrine.orm.mappings`.
pub(crate) fn entity_globs(doctrine_yaml: &str) -> Vec<String> {
    let mut globs = vec!["src/Entity/**/*.php".to_owned()];
    for key in yaml_keys(doctrine_yaml) {
        if key.key == "dir" && key.parents.len() == 4 && key.parents[2] == "mappings" {
            let directory = key.value.trim_matches(['\'', '"']).trim_start_matches("%kernel.project_dir%/").trim_end_matches('/').to_owned();
            let glob = format!("{directory}/**/*.php");
            if !directory.is_empty() && !directory.starts_with('/') && !directory.contains("..") && !globs.contains(&glob) {
                globs.push(glob);
            }
        }
    }
    globs
}

/// Every `Route(` of a controller (`#[Route]`, `@Route`) names its route with a
/// string literal: unnamed routes get generated names the index cannot know.
fn php_routes_literal(text: &str) -> bool {
    let class_at = class_declaration(text).unwrap_or(0);

    text.match_indices("Route(")
        .filter(|(at, _)| {
            let before = &text[..*at];
            before.ends_with("#[") || before.ends_with('@') || before.ends_with('\\') || before.ends_with(", ")
        })
        .all(|(at, matched)| {
            let args_start = at + matched.len();
            let args_end = closing_paren(text, args_start);
            let named = route_name_argument(text, args_start..args_end).is_some();
            let declares_name = text[args_start..args_end].contains("name");
            named || (at < class_at && !declares_name)
        })
}

/// Named routes of `#[Route]` attributes and `@Route` annotations. A route on
/// the class prefixes the names of its methods' routes.
pub fn php_routes(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let class_at = class_declaration(text).unwrap_or(0);
    let mut prefix = String::new();

    let attributes = text.match_indices("Route(").filter(|(at, _)| {
        let before = &text[..*at];
        before.ends_with("#[") || before.ends_with('@') || before.ends_with('\\') || before.ends_with(", ")
    });

    for (at, matched) in attributes {
        let args_start = at + matched.len();
        let args_end = closing_paren(text, args_start);
        let Some((name, offset)) = route_name_argument(text, args_start..args_end) else { continue };

        if at < class_at {
            prefix = name.to_owned();
            continue;
        }

        let full = format!("{prefix}{name}");
        names.add(&full, None, path, lines.position(offset));
    }
}

/// The name an attribute or `new X(` call is given: its first string, or `name:`.
fn named_argument(text: &str, args_start: usize) -> Option<(&str, usize)> {
    if text[args_start..].trim_start().starts_with(['\'', '"']) {
        return quoted_at(text, args_start);
    }

    route_name_argument(text, args_start..closing_paren(text, args_start))
}

/// Console command names: `#[AsCommand('x')]`, `#[AsCommand(name: 'x')]`, `$defaultName = 'x'`.
pub fn php_commands(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for (at, matched) in text.match_indices("AsCommand(") {
        if let Some((name, offset)) = named_argument(text, at + matched.len()) {
            names.add(name, None, path, lines.position(offset));
        }
    }

    for (at, matched) in text.match_indices("$defaultName") {
        let after = at + matched.len();
        let Some(equals) = text[after..].find('=').filter(|e| !text[after..after + e].contains(';')) else { continue };
        if let Some((name, offset)) = quoted_at(text, after + equals + 1) {
            names.add(name, None, path, lines.position(offset));
        }
    }
}

const TWIG_EXTENSION_CALLS: &[(&str, &str)] = &[
    ("new TwigFilter(", "filter"),
    ("AsTwigFilter(", "filter"),
    ("new TwigFunction(", "function"),
    ("AsTwigFunction(", "function"),
    ("new TwigTest(", "test"),
    ("AsTwigTest(", "test"),
];

/// Filters, functions and tests a project's Twig extensions define: (kind, name, position).
pub fn php_twig_extensions(text: &str) -> Vec<(&'static str, String, Position)> {
    let lines = Lines::new(text);
    let mut found = Vec::new();

    for (call, kind) in TWIG_EXTENSION_CALLS {
        for (at, _) in text.match_indices(call) {
            if let Some((name, offset)) = named_argument(text, at + call.len()) {
                found.push((*kind, name.to_owned(), lines.position(offset)));
            }
        }
    }

    found
}

/// `#[AsTwigComponent]` classes: the `name` argument, else the class name.
pub fn php_components(text: &str) -> Vec<(String, Position)> {
    let lines = Lines::new(text);
    let mut found = Vec::new();

    for (at, matched) in text.match_indices("AsTwigComponent") {
        let args_start = at + matched.len() + 1;
        let has_arguments = text[at + matched.len()..].starts_with('(');
        if let Some((name, offset)) = has_arguments.then(|| named_argument(text, args_start)).flatten() {
            found.push((name.to_owned(), lines.position(offset)));
            continue;
        }

        if let Some((class, offset)) = php_class(text) {
            let short = class.rsplit('\\').next().unwrap_or(&class).to_owned();
            found.push((short, lines.position(offset)));
        }
    }

    found
}

/// `parameters:` keys of a YAML file.
pub fn yaml_params(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    for key in yaml_keys(text) {
        if key.parents != ["parameters"] {
            continue;
        }

        let name = key.key.trim_matches(['\'', '"']);
        let quote = usize::from(name.len() != key.key.len());
        let after_key = &text[key.offset + key.key.len()..];
        let raw = after_key.lines().next().unwrap_or_default().trim_start().strip_prefix(':').unwrap_or_default();
        let value = ui_kit::env_value::display_value(name, raw);
        names.add(name, Some(value).filter(|value| !value.is_empty()), path, lines.position(key.offset + quote));
    }
}

/// Top-level `<parameter key="…">` of an XML file with a display value; collections show their item count
/// and nested keys are not parameters.
pub fn xml_params(path: &str, text: &str, names: &mut impl Sink) {
    struct Open<'a> {
        key: &'a str,
        at: usize,
        collection: bool,
        children: usize,
        value_from: usize,
    }

    let lines = Lines::new(text);
    let mut events: Vec<(usize, Option<&str>)> = xml_tags(text, "parameter").into_iter().map(|(tag, at)| (at, Some(tag))).collect();
    events.extend(text.match_indices("</parameter>").map(|(at, _)| (at, None)));
    events.sort_by_key(|(at, _)| *at);

    let mut stack: Vec<Open> = Vec::new();
    let mut add = |key: &str, value: String, at: usize| names.add(key, Some(value).filter(|value| !value.is_empty()), path, lines.position(at));

    for (at, tag) in events {
        let Some(tag) = tag else {
            let Some(open) = stack.pop() else { continue };
            if stack.is_empty() {
                let value = if open.collection {
                    format!("[{} items]", open.children)
                } else {
                    let raw = text.get(open.value_from..at).unwrap_or_default();
                    ui_kit::env_value::mask_value(open.key, &xml_unescape(raw.trim()))
                };
                add(open.key, value, open.at);
            }
            continue;
        };

        if let Some(parent) = stack.last_mut() {
            parent.children += 1;
        }
        let Some((key, offset)) = xml_attribute(tag, "key") else { continue };
        if tag.ends_with('/') {
            if stack.is_empty() {
                add(key, String::new(), at + offset);
            }
            continue;
        }
        stack.push(Open {
            key,
            at: at + offset,
            collection: xml_attribute(tag, "type").is_some_and(|(kind, _)| kind == "collection"),
            children: 0,
            value_from: at + tag.len() + 1,
        });
    }
}

fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The fully qualified name of a PHP file's class and the offset of its name.
pub fn php_class(text: &str) -> Option<(String, usize)> {
    let masked = crate::php_text::mask(text);
    let declaration = class_declaration(&masked)?;
    let at = declaration + "class ".len();
    let name_len = text[at..].bytes().position(|b| !crate::refs::is_identifier(b))?;
    let namespace = masked[..declaration]
        .find("namespace ")
        .and_then(|n| masked[n + "namespace ".len()..declaration].split_once(';'))
        .map(|(namespace, _)| format!("{}\\", namespace.trim()))
        .unwrap_or_default();
    Some((format!("{namespace}{}", &text[at..at + name_len]), at))
}

pub(crate) fn class_declaration(text: &str) -> Option<usize> {
    let masked = crate::php_text::mask(text);
    let text = masked.as_str();
    text.match_indices("class ").map(|(at, _)| at).find(|&at| {
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let before = text[line_start..at].trim();
        before.split_whitespace().all(|word| matches!(word, "final" | "abstract" | "readonly"))
    })
}

/// Routes of the compiled URL generator: `'name' => [[…], ['_controller' => '…'], …]`.
pub fn compiled_routes(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_offset = offset;
        offset += line.len();

        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('\'') else { continue };
        let Some(end) = rest.find('\'') else { continue };

        if !rest[end + 1..].trim_start().starts_with("=> [[") {
            continue;
        }

        let controller = line
            .split_once("'_controller' => '")
            .and_then(|(_, after)| after.split_once('\''))
            .map(|(controller, _)| controller.replace("\\\\", "\\"));
        let key_offset = line_offset + (line.len() - trimmed.len()) + 1;
        names.add(&rest[..end], controller, path, lines.position(key_offset));
    }
}

/// Translation domain of `messages+intl-icu.en.yaml`: `messages`.
pub fn translation_domain(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    let domain = file.split('.').next().unwrap_or(file);
    domain.split('+').next().unwrap_or(domain)
}

/// Keys of a YAML (nested keys joined with `.`; the text as detail) or XLIFF
/// translation file.
pub fn translation_keys(path: &str, text: &str, names: &mut impl Sink) {
    let lines = Lines::new(text);

    if path.ends_with(".yaml") || path.ends_with(".yml") {
        for key in yaml_keys(text).into_iter().filter(|key| !key.value.is_empty()) {
            let mut full = key.parents.join(".");
            if !full.is_empty() {
                full.push('.');
            }
            full.push_str(&key.key);
            let value = key.value.trim_matches(['\'', '"']).to_owned();
            names.add(&full, Some(value), path, lines.position(key.offset));
        }
        return;
    }

    for (tag_name, key_attribute) in [("trans-unit", "resname"), ("unit", "name")] {
        for (tag, at) in xml_tags(text, tag_name) {
            if let Some((key, offset)) = xml_attribute(tag, key_attribute) {
                names.add(key, None, path, lines.position(at + offset));
                continue;
            }

            let after = at + tag.len();
            let Some(source) = text[after..].find("<source>").map(|i| after + i + "<source>".len()) else { continue };
            let Some(end) = text[source..].find("</source>") else { continue };
            names.add(&text[source..source + end], None, path, lines.position(source));
        }
    }
}

#[cfg(test)]
mod cache_observation_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn built_cache_tracks_each_global_source_setting_without_reader_io() {
        let host=MockHost::install();let project=host.invocation().project().unwrap();
        let index=Index::build(&project);
        assert!(index.settings_current());
        host.set_setting("diagnostics","false");assert!(index.settings_current(),"diagnostic toggles are evaluated separately");
        for key in ["compiledRoutes","compiledContainers","extraServiceFiles","extraRoutingFiles","publicDirectory","twigNamespaces"] {
            host.set_setting(key,r#""changed""#);assert!(!index.settings_current(),"{key}");
            host.set_setting(key,"null");assert!(index.settings_current(),"restored {key}");
        }
        host.set_setting("compiledRoutes",&serde_json::to_string(&"x".repeat(32*1024+1)).unwrap());
        assert!(!index.settings_current());
    }

    #[test]
    fn discovery_caps_and_errors_are_partial_without_discarding_retained_paths() {
        let host=MockHost::install();host.add_file("templates/a.twig","");host.add_file("templates/b.twig","");
        let project=host.invocation().project().unwrap();
        let (files,partial)=discover(&project,&["templates/**/*.twig"],2);assert_eq!(files.len(),2);assert!(partial,"a cap-sized result cannot establish exhaustiveness");
        let (files,partial)=discover(&project,&["templates/**/*.twig","templates/a.twig"],3);assert_eq!(files.len(),2);assert!(!partial,"overlapping successful lists deduplicate");
        let denied=MockHost::install().with_capabilities(&[]);let project=denied.invocation().project().unwrap();
        assert!(discover(&project,&["templates/**/*.twig"],3).1);
        let mut index=Index::default();assert!(optional_text(&project,"optional.php",&mut index).is_empty());assert!(index.partial,"permission error is not optional-file absence");
    }
    #[test]
    fn build_distinguishes_optional_defaults_from_declared_missing_inputs() {
        let host=MockHost::install();host.add_file("composer.json",r#"{"require":{"symfony/framework-bundle":"^7.0"},"autoload":{"psr-4":{"App\\":"src/"}}}"#);
        let project=host.invocation().project().unwrap();
        let index=Index::build(&project);assert!(!index.partial);assert!(index.template_scan_observed);assert!(index.psr4_source_verified);
        host.set_setting("compiledContainers",r#""custom/container.xml""#);
        let index=Index::build(&project);assert!(index.partial);assert!(!index.template_scan_observed);
        host.add_file("custom/container.xml","<container/>");
        let index=Index::build(&project);assert!(!index.partial);assert!(index.compiled_container);
    }
    #[test]
    fn class_mapping_provenance_requires_strict_production_pairs_used_by_reader() {
        let good=r#"{"autoload":{"psr-4":{"App\\":"src/","Domain\\":"lib/domain"}}}"#;
        assert!(production_psr4_matches(good,&psr4(good)));
        for text in [
            "{}",
            r#"{"autoload":{"psr-4":{"App\\":["src/","other/"]}}}"#,
            r#"{"autoload-dev":{"psr-4":{"Tests\\":"tests/"}},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
            r#"{"autoload":{"psr-4":{"App\\":"src/","App\\":"other/"}}}"#,
            r#"{"autoload":{"psr-4":{"App\\":"../outside"}}}"#,
        ] {assert!(!production_psr4_matches(text,&psr4(text)),"{text}");}
    }
}

#[cfg(test)]
mod php_literal_discovery_tests {
    use super::*;
    #[test]
    fn class_and_namespace_discovery_ignore_php_looking_literal_bodies() {
        let text="<?php\n$literal = <<<'TEXT'\nnamespace Ghost;\nclass Fake {}\nTEXT;\nclass Actual {}\n";
        assert_eq!(php_class(text),Some(("Actual".into(),text.find("Actual").unwrap())));
        let text="<?php\nnamespace App;\n$literal = <<<TEXT\nclass Fake {}\nTEXT;\nclass Actual {}\n";
        assert_eq!(php_class(text),Some(("App\\Actual".into(),text.find("Actual").unwrap())));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn xml_params_keep_top_level_keys_with_values_and_summarize_collections() {
        let xml = r#"<container><parameters>
            <parameter key="kernel.debug">true</parameter>
            <parameter key="kernel.secret">abc</parameter>
            <parameter key="app.url">http://u:p@host/x?a=1&amp;b=2</parameter>
            <parameter key="kernel.bundles" type="collection">
              <parameter key="FrameworkBundle">Symfony\FrameworkBundle</parameter>
              <parameter key="TwigBundle">Symfony\TwigBundle</parameter>
            </parameter>
            <parameter key="empty"/>
        </parameters></container>"#;
        let mut names = Names::new();
        xml_params("/p/c.xml", xml, &mut names);
        let shown: Vec<_> = names.iter().map(|(name, entry)| (name.as_str(), entry.detail.as_deref())).collect();
        assert_eq!(
            shown,
            [
                ("app.url", Some("http://***@host/x?a=1&b=2")),
                ("empty", None),
                ("kernel.bundles", Some("[2 items]")),
                ("kernel.debug", Some("true")),
                ("kernel.secret", Some("••••••")),
            ]
        );
    }

    use super::*;

    #[test]
    fn live_deprecation_version_requires_supported_unambiguous_production_requirement() {
        let composer=|constraint:&str|serde_json::json!({"require":{"symfony/framework-bundle":constraint}}).to_string();
        for (value,floor) in [("^8.1",(8,1)),("~6.4.2",(6,4)),(">=7.3",(7,3)),("8.1.2",(8,1)),("7.*",(7,0)),("6.4.x",(6,4)),("8",(8,0))] {
            let evidence=VersionEvidence::from_composer(&composer(value)).unwrap();
            assert_eq!(evidence.floor,floor,"{value}");assert_eq!(evidence.constraint,value);
        }
        for value in ["*","dev-main","<8.1","<=8.1","!=8.1","^6.4 || ^8.1",">=6.4 <8.0","8.1-beta1","8.1@dev","^8.*","08.1","8..1","8.1.2.3",">= 8.1",""] {
            assert!(VersionEvidence::from_composer(&composer(value)).is_none(),"{value}");
        }
        for json in [r#"{"require-dev":{"symfony/framework-bundle":"^8.1"}}"#,r#"{"description":"symfony/framework-bundle ^8.1"}"#,r#"{"require":{"symfony/framework-bundle":8.1}}"#,"{broken"] {
            assert!(VersionEvidence::from_composer(json).is_none(),"{json}");
        }
    }

    #[test]
    fn configured_inputs_share_reader_validation_and_exact_directory_boundaries() {
        let host = openidea_plugin::mock::MockHost::install();
        host.set_setting("compiledRoutes", &serde_json::to_string("build/cache/routes.php,../outside.php,/absolute.php,./dot.php,C:/bad.php,build//bad.php,build/../bad.php,build/\\bad.php").unwrap());
        host.set_setting("extraRoutingFiles", &serde_json::to_string(" custom/routes.yaml ").unwrap());
        let paths = configured_input_paths();
        assert_eq!(paths, ["build/cache/routes.php", "custom/routes.yaml"]);
        for path in ["build", "build/cache", "build/cache/routes.php", "custom", "custom/routes.yaml"] {
            assert!(affects_configured_index(path, &paths), "{path}");
        }
        for path in ["", "build/cach", "build/cache/routes.php.old", "custom/routes.yaml/child"] {
            assert!(!affects_configured_index(path, &paths), "{path}");
        }
        host.set_setting("compiledRoutes", &serde_json::to_string(&"x".repeat(32 * 1024 + 1)).unwrap());
        assert!(setting_paths("compiledRoutes").is_empty());
        host.set_setting("compiledRoutes", &serde_json::to_string(&(0..130).map(|n|format!("build/{n}.php")).collect::<Vec<_>>().join(",")).unwrap());
        assert_eq!(setting_paths("compiledRoutes").len(), 128);
    }

    fn names(parse: fn(&str, &str, &mut Names), path: &str, text: &str) -> Vec<(String, Option<String>, u32, u32)> {
        let mut names = Names::new();
        parse(path, text, &mut names);
        names.into_iter().map(|(name, e)| (name, e.detail, e.at.line, e.at.character)).collect()
    }

    fn name(n: &str, detail: Option<&str>, line: u32, column: u32) -> (String, Option<String>, u32, u32) {
        (n.into(), detail.map(Into::into), line, column)
    }

    #[test]
    fn yaml_keys_follow_indentation() {
        let text = "a:\n  b: |\n    c: no\n  'd e': 1\n# x\nf: ~\n";
        let keys: Vec<_> = yaml_keys(text).into_iter().map(|k| (k.parents.join("."), k.key, k.offset)).collect();
        assert_eq!(
            keys,
            [("".into(), "a".into(), 0), ("a".into(), "b".into(), 5), ("a".into(), "d e".into(), 23), ("".into(), "f".into(), 35),]
        );
    }

    #[test]
    fn indexes_yaml_services() {
        let text = "services:\n    _defaults:\n        autowire: true\n    App\\:\n        resource: '../src/'\n    app.mailer:\n        class: App\\Mailer\n    App\\Mailer: '@app.mailer'\nwhen@dev:\n    services:\n        dev.only: ~\n";
        assert_eq!(
            names(yaml_services, "/p/config/services.yaml", text),
            [
                name("App\\Mailer", Some("alias of app.mailer"), 7, 4),
                name("app.mailer", Some("App\\Mailer"), 5, 4),
                name("dev.only", None, 10, 8)
            ]
        );
    }

    #[test]
    fn indexes_yaml_routes_but_not_imports() {
        let text = "controllers:\n    resource: ../src/Controller/\n    type: attribute\nblog_list:\n    path: /blog\n    controller: App\\Controller\\BlogController::list\n";
        assert_eq!(names(yaml_routes, "/p/config/routes.yaml", text), [name("blog_list", Some("/blog"), 3, 0)]);
    }

    #[test]
    fn indexes_attribute_and_annotation_routes() {
        let text = r#"<?php
namespace App\Controller;

#[Route('/blog', name: 'blog_')]
final class BlogController extends AbstractController
{
    #[Route('/', name: 'index', methods: ['GET'])]
    public function index(): Response {}

    /**
     * @Route("/old", name="old")
     */
    public function old(): Response {}

    #[Route(path: '/unnamed')]
    public function unnamed(): Response {}
}
"#;
        assert_eq!(
            names(php_routes, "/p/src/Controller/BlogController.php", text),
            [name("blog_index", None, 6, 24), name("blog_old", None, 10, 28)]
        );
    }

    #[test]
    fn indexes_compiled_routes() {
        let text = "<?php\n\nreturn [\n    '_preview_error' => [['code', '_format'], ['_controller' => 'error_controller::preview'], [], []],\n    'app_home' => [[], ['_controller' => 'App\\\\Controller\\\\HomeController::index'], [], [['text', '/']], [], [], []],\n];\n";
        assert_eq!(
            names(compiled_routes, "/p/var/cache/dev/url_generating_routes.php", text),
            [
                name("_preview_error", Some("error_controller::preview"), 3, 5),
                name("app_home", Some("App\\Controller\\HomeController::index"), 4, 5)
            ]
        );
    }

    #[test]
    fn indexes_xml_services_and_routes() {
        let text = "<container>\n  <services>\n    <service id=\"router\" class=\"Symfony\\Bundle\\Router\" public=\"true\"/>\n    <service id=\".private\" class=\"X\"/>\n    <service id=\"url_generator\" alias=\"router\"/>\n  </services>\n  <route id=\"home\" path=\"/\"/>\n</container>\n";
        assert_eq!(
            names(xml_services, "/p/c.xml", text),
            [name("router", Some("Symfony\\Bundle\\Router"), 2, 17), name("url_generator", Some("alias of router"), 4, 17)]
        );
        assert_eq!(names(xml_routes, "/p/c.xml", text), [name("home", Some("/"), 6, 13)]);
    }

    #[test]
    fn indexes_translations() {
        let yaml = "app:\n  title: Hello\n  nav:\n    home: Home\nplain.key: Plain\n";
        assert_eq!(
            names(translation_keys, "/p/translations/messages+intl-icu.en.yaml", yaml),
            [name("app.nav.home", Some("Home"), 3, 4), name("app.title", Some("Hello"), 1, 2), name("plain.key", Some("Plain"), 4, 0)]
        );

        let xliff = "<xliff>\n<trans-unit id=\"a\" resname=\"form.save\">\n<source>form.save</source>\n</trans-unit>\n<trans-unit id=\"b\">\n<source>Bye</source>\n</trans-unit>\n</xliff>\n";
        assert_eq!(
            names(translation_keys, "/p/translations/validators.fr.xlf", xliff),
            [name("Bye", None, 5, 8), name("form.save", None, 1, 28)]
        );
    }

    #[test]
    fn finds_the_class_of_a_php_file() {
        let text = "<?php\n\nnamespace App\\Mail;\n\nfinal class Mailer\n{\n}\n";
        assert_eq!(php_class(text), Some(("App\\Mail\\Mailer".into(), 40)));
    }

    #[test]
    fn reads_psr4_autoload() {
        let composer = r#"{"autoload": {"psr-4": {"App\\": "src/", "Lib\\": "lib"}}, "autoload-dev": {"psr-4": {"T\\": "tests/"}}}"#;
        assert_eq!(psr4(composer), [("App\\".into(), "src/".into()), ("Lib\\".into(), "lib".into())]);
        assert_eq!(psr4("{}"), [("App\\".into(), "src/".into())]);
    }

    #[test]
    fn indexes_render_calls() {
        let text = "<?php\nclass BlogController {\n    public function show(): Response\n    {\n        return $this->render('blog/show.html.twig', [\n            'post' => $post,\n            'tags' => ['a' => 1],\n        ]);\n    }\n}\n";
        let renders = php_renders("/p/src/Controller/BlogController.php", text);
        assert_eq!(renders.len(), 1);
        assert_eq!(renders[0].template, "blog/show.html.twig");
        assert_eq!(renders[0].variables, ["post", "tags"]);
        assert_eq!(renders[0].caller, "BlogController::show");
        assert_eq!(renders[0].at, Position::new(4, 30));
    }

    #[test]
    fn indexes_roles_and_entries() {
        let security = "security:\n    role_hierarchy:\n        ROLE_ADMIN: [ROLE_USER, ROLE_EDITOR]\n";
        let found: Vec<_> = names(roles, "/p/config/packages/security.yaml", security).into_iter().map(|n| n.0).collect();
        assert_eq!(found, ["ROLE_ADMIN", "ROLE_EDITOR", "ROLE_USER"]);

        let webpack = "Encore\n    .addEntry('app', './assets/app.js')\n    .addStyleEntry('mail', './assets/mail.scss')\n";
        assert_eq!(
            names(encore_entries, "/p/webpack.config.js", webpack),
            [name("app", Some("./assets/app.js"), 1, 15), name("mail", Some("./assets/mail.scss"), 2, 20)]
        );

        let importmap =
            "<?php\nreturn [\n    'app' => [\n        'path' => './assets/app.js',\n        'entrypoint' => true,\n    ],\n];\n";
        assert_eq!(names(importmap_entries, "/p/importmap.php", importmap), [name("app", None, 2, 5)]);
    }

    #[test]
    fn indexes_service_tags_from_yaml_and_xml() {
        let yaml = "services:\n    a:\n        tags:\n            - { name: one.tag, priority: 1 }\n            - name: two.tag\n            - three.tag\n    b:\n        tags: ['four.tag', { name: five.tag }]\nparameters:\n    tags: [nope.tag]\n";
        let found: Vec<_> = names(yaml_service_tags, "/p/config/s.yaml", yaml).into_iter().map(|n| (n.0, n.2)).collect();
        assert_eq!(
            found,
            [("five.tag".into(), 7), ("four.tag".into(), 7), ("one.tag".into(), 3), ("three.tag".into(), 5), ("two.tag".into(), 4)]
        );

        let xml =
            "<services>\n  <service id=\"a\">\n    <tag name=\"x.tag\"/>\n    <!-- <tag name=\"old.tag\"/> -->\n  </service>\n</services>";
        assert_eq!(names(xml_service_tags, "/p/config/s.xml", xml), [name("x.tag", None, 2, 15)]);
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let yaml = "services:\n    a:\n        tags: [{ name: 'é𝄞.tag' }]\n";
        assert_eq!(names(yaml_service_tags, "/p/config/s.yaml", yaml), [name("é𝄞.tag", None, 2, 24)]);

        let xml = "<!-- é𝄞 --><service id=\"é𝄞\"><tag name=\"x.tag\"/></service>";
        assert_eq!(names(xml_service_tags, "/p/config/s.xml", xml), [name("x.tag", None, 0, 41)]);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let yaml = "services:\r\n    a:\r\n        tags:\r\n            - name: one.tag\r\n            - { name: two.tag }\r\n";
        assert_eq!(names(yaml_service_tags, "/p/config/s.yaml", yaml), [name("one.tag", None, 3, 20), name("two.tag", None, 4, 22)]);

        let xml = "<service id=\"a\">\r\n<tag name=\"x.tag\"/>\r\n</service>";
        assert_eq!(names(xml_service_tags, "/p/config/s.xml", xml), [name("x.tag", None, 1, 11)]);
    }

    #[test]
    fn twig_namespaces_setting_parses_pairs() {
        assert_eq!(
            twig_namespaces("emails=templates/mail, =templates/main ,pdf=./pdf/"),
            [("templates/mail".into(), "emails".into()), ("templates/main".into(), String::new()), ("pdf".into(), "pdf".into())]
        );
    }

    #[test]
    fn twig_namespaces_setting_strips_a_leading_at_sign() {
        assert_eq!(twig_namespaces("@emails=templates/mail"), [("templates/mail".into(), "emails".into())]);
    }

    #[test]
    fn twig_namespaces_setting_ignores_malformed_pairs() {
        assert!(twig_namespaces("nodelimiter, a=, b=../up, ,=, c=/").is_empty());
    }

    #[test]
    fn names_templates() {
        assert_eq!(template_name("blog/index.html.twig"), "blog/index.html.twig");
        assert_eq!(template_name("bundles/TwigBundle/Exception/error.html.twig"), "@Twig/Exception/error.html.twig");
        assert_eq!(translation_domain("/p/translations/messages+intl-icu.en.yaml"), "messages");
    }

    #[test]
    fn stimulus_controllers_get_their_data_controller_identifier() {
        assert_eq!(stimulus_identifier("controllers/hello_controller.js").as_deref(), Some("hello"));
        assert_eq!(stimulus_identifier("controllers/users/list_item_controller.ts").as_deref(), Some("users--list-item"));
        assert_eq!(stimulus_identifier("controllers/csrf-protection_controller.js").as_deref(), Some("csrf-protection"));
        assert_eq!(stimulus_identifier("controllers/helper.js"), None);
        assert_eq!(stimulus_identifier("styles/app_controller.css"), None);
        assert_eq!(stimulus_identifier("other/hello_controller.js"), None);
    }

    #[test]
    fn vite_entries_come_from_the_input_option() {
        let names = |text: &str| {
            let mut found = Names::new();
            vite_entries("/p/vite.config.js", text, &mut found);
            found.into_iter().map(|(name, entry)| (name, entry.detail.unwrap_or_default())).collect::<Vec<_>>()
        };
        assert_eq!(
            names("export default { build: { rollupOptions: { input: { app: 'assets/app.js', admin: \"./assets/admin.ts\" } } } }"),
            [("admin".to_owned(), "Vite entry · ./assets/admin.ts".to_owned()), ("app".to_owned(), "Vite entry · assets/app.js".to_owned())]
        );
        assert_eq!(names("export default { build: { input: ['./assets/app.js', 'assets/theme.css'] } }").len(), 2);
        assert!(names("export default { input: resolve(__dirname, 'x') }").is_empty());
        assert!(names("export default {}").is_empty());
    }
}
