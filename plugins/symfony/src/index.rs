//! A text index of a Symfony project: route names, Twig templates, service
//! ids, translation keys, classes, assets, entries and roles, each with where
//! it is defined, and the controllers' render calls.
//!
//! Port of the plugin's file indexes (routing, templating, dic, translation,
//! assets, security), reduced to what can be read from the files without
//! PHP/YAML/Twig PSI.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use openidea_plugin::host::{self, Project};
use openidea_plugin::Position;

use crate::refs::TEMPLATE_CALLS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub detail: Option<String>,
    /// Absolute path; empty for built-in names.
    pub path: String,
    pub at: Position,
}

pub type Names = BTreeMap<String, Entry>;

/// A controller rendering a template: `$this->render('x.html.twig', ['a' => …])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Render {
    pub template: String,
    /// Keys of the literal parameter array: the template's variables.
    pub variables: Vec<String>,
    /// `BlogController::index`.
    pub caller: String,
    pub path: String,
    pub at: Position,
}

#[derive(Default, Debug)]
pub struct Index {
    pub routes: Names,
    pub templates: Names,
    pub services: Names,
    /// Keys by domain.
    pub translations: BTreeMap<String, Names>,
    /// YAML translation files by domain, where missing keys can be added.
    pub translation_files: BTreeMap<String, Vec<String>>,
    /// Project classes (PSR-4 autoload) and their files.
    pub classes: Names,
    pub assets: Names,
    /// Webpack Encore and AssetMapper entry points.
    pub entries: Names,
    pub roles: Names,
    pub renders: Vec<Render>,
    /// The compiled routes / container were read: bundles' names are known, so
    /// a missing name is an error.
    pub compiled_routes: bool,
    pub compiled_container: bool,
    /// Some files were skipped to stay within the call's time budget.
    pub partial: bool,
}

/// Compiled files of `bin/console cache:warmup`, which also hold what bundles
/// define (Settings.java and ServiceContainerUtil.java defaults).
const COMPILED_ROUTES: &[&str] = &["var/cache/dev/url_generating_routes.php", "var/cache/dev/UrlGenerator.php"];
const COMPILED_CONTAINERS: &[&str] =
    &["var/cache/dev/App_KernelDevDebugContainer.xml", "var/cache/dev/srcApp_KernelDevDebugContainer.xml"];

/// Security attributes that are not roles (AuthenticatedVoter, Symfony's ExpressionLanguage).
const BUILT_IN_ROLES: &[&str] = &[
    "IS_AUTHENTICATED",
    "IS_AUTHENTICATED_FULLY",
    "IS_AUTHENTICATED_REMEMBERED",
    "IS_IMPERSONATOR",
    "IS_REMEMBERED",
    "PUBLIC_ACCESS",
];

// ponytail: 3 s of reading per build, then the index is partial; incremental per-file updates if projects outgrow it.
const BUDGET: Duration = Duration::from_secs(3);
const MAX_FILES: u32 = 5000;

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
    value.split(',').map(str::trim).filter(|p| !p.is_empty()).map(str::to_owned).collect()
}

impl Index {
    pub fn build(project: &Project) -> Index {
        let started = Instant::now();
        let mut index = Index::default();
        let root = project.root().unwrap_or_default();
        let relative = |path: &str| path.strip_prefix(&format!("{root}/")).unwrap_or(path).to_owned();

        let files = |globs: &[&str]| -> Vec<String> {
            let mut paths: Vec<String> =
                globs.iter().flat_map(|glob| project.find_files(glob, MAX_FILES).unwrap_or_default()).collect();
            paths.sort();
            paths.dedup();
            paths
        };

        for path in files(&["templates/**/*.twig"]) {
            let name = template_name(relative(&path).trim_start_matches("templates/"));
            index.templates.entry(name).or_insert(Entry { detail: None, path, at: Position::new(0, 0) });
        }

        let public = host::setting_string("publicDirectory").filter(|d| !d.is_empty()).unwrap_or("public".into());
        let public = public.trim_end_matches('/');
        for path in files(&[&format!("{public}/**/*")]) {
            let name = relative(&path).trim_start_matches(&format!("{public}/")).to_owned();
            if !name.ends_with(".php") {
                index.assets.entry(name).or_insert(Entry { detail: None, path, at: Position::new(0, 0) });
            }
        }

        let composer = project.read_file("composer.json").unwrap_or_default();
        for (namespace, directory) in psr4(&composer) {
            let directory = format!("{}/", directory.trim_end_matches('/'));
            for path in files(&[&format!("{directory}**/*.php")]) {
                let Some(class) = relative(&path).strip_prefix(&directory).and_then(|p| p.strip_suffix(".php")).map(str::to_owned)
                else {
                    continue;
                };
                let entry = Entry { detail: None, path, at: Position::new(0, 0) };
                index.classes.insert(format!("{namespace}{}", class.replace('/', "\\")), entry);
            }
        }

        let config = files(&["config/**/*.yaml", "config/**/*.yml", "config/**/*.xml"]);
        let controllers = files(&["src/**/*Controller.php", "src/Controller/**/*.php"]);
        let translations =
            files(&["translations/**/*.yaml", "translations/**/*.yml", "translations/**/*.xlf", "translations/**/*.xliff"]);

        let read = |path: &str, index: &mut Index| -> Option<String> {
            if started.elapsed() > BUDGET {
                index.partial = true;
                return None;
            }
            project.read_file(path).ok()
        };

        for path in &config {
            let Some(text) = read(path, &mut index) else { continue };
            let relative = relative(path);

            if relative.contains("security") {
                roles(path, &text, &mut index.roles);
            }

            if relative.ends_with(".xml") {
                xml_services(path, &text, &mut index.services);
                xml_routes(path, &text, &mut index.routes);
                continue;
            }

            if relative.starts_with("config/routes") {
                yaml_routes(path, &text, &mut index.routes);
                continue;
            }

            yaml_services(path, &text, &mut index.services);
        }

        for path in &controllers {
            let Some(text) = read(path, &mut index) else { continue };
            php_routes(path, &text, &mut index.routes);
            roles(path, &text, &mut index.roles);
            index.renders.extend(php_renders(path, &text));
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
                index.compiled_container = true;
            }
        }

        for role in BUILT_IN_ROLES {
            index.roles.add(role, Some("built-in".into()), "", Position::new(0, 0));
        }

        index
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
            let variables = rest
                .strip_prefix(',')
                .map(str::trim_start)
                .filter(|r| r.starts_with('['))
                .map(|r| array_keys(text, text.len() - r.len() + 1))
                .unwrap_or_default();

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
                caller: format!("{class}::{method}"),
                path: path.to_owned(),
                at: lines.position(offset),
            });
        }
    }

    renders.sort_by_key(|r| (r.at.line, r.at.character));
    renders
}

/// `'key' =>` keys at the top level of the array literal starting after `[` at `from`.
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

/// Byte offset → editor position, for many offsets of one file.
struct Lines<'a> {
    text: &'a str,
    starts: Vec<usize>,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let starts = std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
        Self { text, starts }
    }

    fn position(&self, at: usize) -> Position {
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

#[derive(Debug, PartialEq, Eq)]
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

fn split_yaml_key(line: &str) -> Option<(&str, &str)> {
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
        let alias = key.value.trim_matches(['\'', '"']).strip_prefix('@').map(|target| format!("alias of {target}"));
        names.add(&key.key, class.or(alias), path, lines.position(key.offset));
    }
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
fn xml_attribute<'a>(tag: &'a str, name: &str) -> Option<(&'a str, usize)> {
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
fn xml_tags<'a>(text: &'a str, name: &str) -> Vec<(&'a str, usize)> {
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
fn closing_paren(text: &str, from: usize) -> usize {
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

/// The fully qualified name of a PHP file's class and the offset of its name.
pub fn php_class(text: &str) -> Option<(String, usize)> {
    let at = class_declaration(text)? + "class ".len();
    let name_len = text[at..].bytes().position(|b| !crate::refs::is_identifier(b))?;
    let namespace = text
        .find("namespace ")
        .and_then(|n| text[n + "namespace ".len()..].split_once(';'))
        .map(|(namespace, _)| format!("{}\\", namespace.trim()))
        .unwrap_or_default();
    Some((format!("{namespace}{}", &text[at..at + name_len]), at))
}

fn class_declaration(text: &str) -> Option<usize> {
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
mod tests {
    use super::*;

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
            [
                ("".into(), "a".into(), 0),
                ("a".into(), "b".into(), 5),
                ("a".into(), "d e".into(), 23),
                ("".into(), "f".into(), 35),
            ]
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

        let importmap = "<?php\nreturn [\n    'app' => [\n        'path' => './assets/app.js',\n        'entrypoint' => true,\n    ],\n];\n";
        assert_eq!(names(importmap_entries, "/p/importmap.php", importmap), [name("app", None, 2, 5)]);
    }

    #[test]
    fn names_templates() {
        assert_eq!(template_name("blog/index.html.twig"), "blog/index.html.twig");
        assert_eq!(template_name("bundles/TwigBundle/Exception/error.html.twig"), "@Twig/Exception/error.html.twig");
        assert_eq!(translation_domain("/p/translations/messages+intl-icu.en.yaml"), "messages");
    }
}
