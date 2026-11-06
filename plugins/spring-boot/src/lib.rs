//! Spring Boot Tools for OpenIDEA: completion of keys and values in
//! `application*.properties|yml`, deprecated and unknown key warnings (from
//! Spring Boot's metadata, the project's metadata files and its
//! `@ConfigurationProperties` classes) and cron expression checks in
//! `@Scheduled(cron = …)` and `*.cron` keys. Text-based, Java only.

mod build;
mod checks;
mod cron;
mod facts;
mod fileset;
mod local_meta;
mod meta;
mod nav;
mod values;

use meta::bundled;
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::Location;
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, NotificationLevel, PickItem, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::config::{self, Key, canonical, is_config_file};
use facts::Summary;
use spring_text::metadata::{Catalog, Lookup, Meta, properties_completion, yaml_completion};
use spring_text::index::{COMMAND_BUDGET, Index, Indexes, PROVIDER_BUDGET, project_of};
use spring_text::{contains, java, range};

const SOURCES: &[&str] = &["**/META-INF/*spring-configuration-metadata.json", "**/*.java"];

fn parse_source(path: &str, text: &str) -> Option<Meta> {
    if path.ends_with(".json") { meta::parse_json(text) } else { meta::parse_java(text) }
}

fn placeholder(s: &str) -> bool {
    s.contains("${") || s.contains("#{")
}

const AUTOWIRE_MODE_GLOBS: &[&str] = &[
    "**/spring.properties",
    "**/junit-platform.properties",
    "**/application*.properties",
    "**/application*.yml",
    "**/application*.yaml",
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/gradle.properties",
];

pub struct BootTools {
    sources: Indexes<Meta>,
    autowire_mode: Indexes<bool>,
    build: Indexes<build::Facts>,
    facts: Indexes<facts::Facts>,
    locations: Indexes<fileset::Locations>,
}

impl Default for BootTools {
    fn default() -> Self {
        Self {
            sources: Indexes::new(SOURCES, parse_source),
            autowire_mode: Indexes::new(AUTOWIRE_MODE_GLOBS, checks::sets_autowire_mode),
            build: Indexes::new(build::BUILD_GLOBS, build::read),
            facts: Indexes::new(nav::SOURCE_GLOBS, facts::read),
            locations: Indexes::new(fileset::GLOBS, fileset::read),
        }
    }
}

impl BootTools {
    /// The project's own properties, and whether a whole pass over the project was read.
    fn project(&mut self, doc: &Document) -> (Catalog, bool) {
        match self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
            Some(index) => catalog_of(index),
            None => (Catalog::default(), false),
        }
    }

    /// The project's classes, enums and packages, as far as they were read.
    fn summary(&mut self, doc: &Document) -> Summary {
        match self.facts.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
            Some(index) => Summary::merge(index.iter().map(|(_, f)| f), index.complete()),
            None => Summary::default(),
        }
    }

    /// Whether Spring Boot reads the document as configuration: by name, or because a standard file points at it.
    fn is_config(&mut self, doc: &Document) -> bool {
        if is_config_file(&doc.path) {
            return true;
        }
        let Some(index) = self.locations.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return false };
        fileset::is_configuration(&doc.path, index.iter().map(|(_, l)| l))
    }

    /// What the build files declare, once all of them were read and there is at least one.
    fn build_summary(&mut self, doc: &Document) -> Option<build::Summary> {
        let index = self.build.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        (index.complete() && index.iter().next().is_some()).then(|| build::Summary::merge(index.iter().map(|(_, f)| f)))
    }

    /// The text checks over a Java document; they need the whole project read to tell whether the autowire mode is set.
    fn checked(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let groups = checks::Groups::from_settings();
        let mode_set = match self.autowire_mode.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
            Some(index) if groups.test_constructors && index.complete() => Some(index.iter().next().is_some()),
            _ => None,
        };
        checks::check_java(&java::parse(&doc.text), mode_set, groups).iter().map(|f| f.diagnostic(&doc.text)).collect()
    }
}

fn catalog_of(index: &Index<Meta>) -> (Catalog, bool) {
    let mut catalog = Catalog::default();
    for (_, meta) in index.iter() {
        catalog.add(meta);
    }
    (catalog, index.complete())
}

fn removal_actions(doc: &Document, selection: Range) -> Vec<CodeAction> {
    checks::removal_fixes(&java::parse(&doc.text), &doc.text)
        .into_iter()
        .map(|(from, to, title)| (range(&doc.text, from, to), title))
        .filter(|(covered, _)| contains(*covered, selection.start))
        .map(|(covered, title)| CodeAction {
            title,
            kind: Some("quickfix".into()),
            preferred: true,
            edits: vec![(doc.path.clone(), vec![TextEdit { range: covered, text: String::new() }])],
            data: None,
        })
        .collect()
}

#[derive(Clone)]
struct Problem {
    diagnostic: Diagnostic,
    fix: Option<(String, TextEdit)>,
}

fn problem(text: &str, from: usize, to: usize, severity: Severity, code: &str, message: String) -> Problem {
    Problem { diagnostic: Diagnostic::new(range(text, from, to), severity, message).code(code), fix: None }
}

/// The text replacing key `k` as written so that it reads `replacement`; `None` when unsure.
fn renamed(text: &str, k: &Key, replacement: &str, yaml: bool) -> Option<String> {
    if !yaml {
        return Some(replacement.to_owned());
    }
    let written = &text[k.at..k.end];
    let parent = k.key.strip_suffix(written)?.trim_end_matches('.');
    if parent.is_empty() {
        return Some(replacement.to_owned());
    }
    let n = parent.split('.').count();
    let segments: Vec<&str> = replacement.split('.').collect();
    let same_parent = !parent.contains('[') && segments.len() > n && canonical(&segments[..n].join(".")) == canonical(parent);
    same_parent.then(|| segments[n..].join("."))
}

/// The problem with a key's value, read as a list when the key's type is one (comma separated, or one item of a YAML sequence).
fn value_problem(lookup: &Lookup, k: &Key, facts: &Summary) -> Option<String> {
    if let Some(p) = lookup.prop(&k.key) {
        return match values::element_type(&p.ty) {
            Some(element) => k.value.split(',').find_map(|v| values::problem(element, v, facts)),
            None => values::problem(&p.ty, &k.value, facts),
        };
    }
    let (base, index) = k.key.strip_suffix(']')?.rsplit_once('[')?;
    let element = values::element_type(&lookup.prop(base)?.ty)?;
    index.bytes().all(|b| b.is_ascii_digit()).then(|| values::problem(element, &k.value, facts)).flatten()
}

fn config_problems(path: &str, text: &str, lookup: &Lookup, complete: bool, drivers: Option<&build::Summary>, facts: &Summary) -> Vec<Problem> {
    let check_values = host::setting_bool("springBoot.checkPropertyValues").unwrap_or(true);
    let keys = config::keys(path, text);
    let yaml = !path.ends_with(".properties");
    let mut out = Vec::new();
    for k in &keys {
        if let Some(dep) = lookup.prop(&k.key).and_then(|p| p.deprecated.as_ref()) {
            let state = if dep.removed { "is no longer supported" } else { "is deprecated" };
            let message = match dep.replacement.as_str() {
                "" => format!("`{}` {state}.", k.key),
                r => format!("`{}` {state}. Use `{r}` instead.", k.key),
            };
            let mut p = problem(text, k.at, k.end, Severity::Warning, "deprecated-property", message);
            p.diagnostic.deprecated = true;
            p.fix = (!dep.replacement.is_empty())
                .then(|| renamed(text, k, &dep.replacement, yaml))
                .flatten()
                .map(|new| (format!("Replace with `{}`", dep.replacement), TextEdit { range: p.diagnostic.range, text: new }));
            out.push(p);
        } else if complete
            // A YAML parent (`datasource:` with keys below) is checked through its children.
            && !keys.iter().any(|o| o.key.len() > k.key.len() && o.key.starts_with(&k.key) && matches!(o.key.as_bytes()[k.key.len()], b'.' | b'['))
            && let Some(group) = lookup.unknown(&k.key)
        {
            let message = format!("`{}` is not a known property of `{group}`.", k.key);
            out.push(problem(text, k.at, k.end, Severity::Warning, "unknown-property", message));
        }
        if check_values
            && let Some(error) = value_problem(lookup, k, facts)
        {
            out.push(problem(text, k.value_at, k.value_end, Severity::Warning, "invalid-property-value", error));
        }
        if let Some(summary) = drivers
            && canonical(&k.key).ends_with("datasource.url")
            && let Some(message) = build::missing_driver(&k.value, summary)
        {
            out.push(problem(text, k.value_at, k.value_end, Severity::Info, "missing-jdbc-driver", message));
        }
        if !k.value.is_empty()
            && canonical(&k.key).ends_with("cron")
            && !placeholder(&k.value)
            && let Some(error) = cron::check(&k.value)
        {
            out.push(problem(text, k.value_at, k.value_end, Severity::Error, "invalid-cron", error));
        }
    }
    out
}

/// `@Scheduled(cron = "…")` literals that Spring would refuse.
fn cron_problems(text: &str) -> Vec<Diagnostic> {
    let file = java::parse(text);
    file.types
        .iter()
        .flat_map(|t| t.methods())
        .filter_map(|m| m.annotation("Scheduled")?.arg("cron"))
        .filter_map(|arg| {
            let [s] = arg.strings.as_slice() else { return None };
            let literal = arg.text.starts_with('"') && arg.text.ends_with('"');
            if !literal || s.value.trim().is_empty() || placeholder(&s.value) {
                return None;
            }
            let error = cron::check(&s.value)?;
            Some(Diagnostic::new(range(text, s.at, s.end), Severity::Error, error).code("invalid-cron"))
        })
        .collect()
}

fn driver_check_on() -> bool {
    host::setting_bool("springBoot.checkDriverDependency").unwrap_or(false)
}

impl BootTools {
    fn profiles(doc: &Document) -> Vec<String> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let files = project.find_files("**/application-*.*", 200).unwrap_or_default();
        let names: std::collections::BTreeSet<String> = files.iter().filter_map(|f| nav::profile_of(f)).collect();
        names.into_iter().collect()
    }

    /// The files under `src/main/resources`, as `classpath:` locations.
    fn resources(doc: &Document) -> Vec<String> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let files = project.find_files("**/src/main/resources/**", 500).unwrap_or_default();
        files.iter().filter_map(|f| f.split_once("/src/main/resources/").map(|(_, rest)| rest.to_owned())).collect()
    }

    fn defining_actions(&mut self, doc: &Document, selection: Range, problems: &[Problem]) -> Vec<CodeAction> {
        let Some(unknown) = problems.iter().find(|p| p.diagnostic.code.as_deref() == Some("unknown-property") && contains(p.diagnostic.range, selection.start)) else {
            return Vec::new();
        };
        let Some(key) = config::keys(&doc.path, &doc.text).into_iter().find(|k| range(&doc.text, k.at, k.end) == unknown.diagnostic.range) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let glob = format!("**/META-INF/{}", local_meta::FILE_NAME);
        let Some(file) = project.find_files(&glob, 5).ok().and_then(|f| f.into_iter().next()) else { return Vec::new() };
        let Ok(text) = project.read_file(&file) else { return Vec::new() };
        let Some((from, to, insert)) = local_meta::define_key(&text, &key.key) else { return Vec::new() };
        vec![CodeAction {
            title: format!("Define `{}` in {}", key.key, local_meta::FILE_NAME),
            kind: Some("quickfix".into()),
            preferred: false,
            edits: vec![(file, vec![TextEdit { range: range(&text, from, to), text: insert }])],
            data: None,
        }]
    }
}

impl Providers for BootTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !self.is_config(doc) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let (project, _) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        let mut items = if doc.path.ends_with(".properties") {
            properties_completion(&doc.text, at, &lookup)
        } else {
            yaml_completion(&doc.text, at, &lookup)
        };
        let facts = self.summary(doc);
        let packages: Vec<String> = facts.packages.iter().cloned().collect();
        let profiles = Self::profiles(doc);
        let resources = || Self::resources(doc);
        let cx = nav::Context { path: &doc.path, text: &doc.text, at, lookup: &lookup, packages: &packages, profiles: &profiles, facts: &facts, resources: &resources };
        items.extend(nav::completion(&cx));
        items
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if !self.is_config(doc) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let (project, _) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        let Some(target) = nav::target(&doc.path, &doc.text, at, &lookup) else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        match target {
            nav::Target::File(glob) => project
                .find_files(&glob, 10)
                .unwrap_or_default()
                .into_iter()
                .map(|path| Location { path, range: Range::new(Position::new(0, 0), Position::new(0, 0)) })
                .collect(),
            nav::Target::Class(name) => {
                let simple = name.rsplit('.').next().unwrap_or(&name).to_owned();
                let tail = format!("{}", name.replace('.', "/"));
                ["java", "kt"]
                    .iter()
                    .flat_map(|ext| project.find_files(&format!("**/{simple}.{ext}"), 20).unwrap_or_default())
                    .filter(|path| path.contains(&tail) || !name.contains('.'))
                    .map(|path| {
                        let range = project.read_file(&path).map(|t| nav::declaration(&t, &simple)).unwrap_or_default();
                        Location { path, range }
                    })
                    .collect()
            }
        }
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if doc.path.ends_with(".java") {
            if !checks::Groups::from_settings().application_setup {
                return Vec::new();
            }

            return removal_actions(doc, selection);
        }
        if !self.is_config(doc) {
            return Vec::new();
        }
        let (project, complete) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        let facts = self.summary(doc);
        let problems = config_problems(&doc.path, &doc.text, &lookup, complete, None, &facts);
        let mut actions: Vec<CodeAction> = problems
            .iter()
            .filter(|p| contains(p.diagnostic.range, selection.start))
            .filter_map(|p| {
                let (title, edit) = p.fix.clone()?;
                Some(CodeAction {
                    title,
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![edit])],
                    data: None,
                })
            })
            .collect();
        actions.extend(self.defining_actions(doc, selection, &problems));
        actions
    }

    /// Publishes the file's problems; there are no gutter markers.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let problems = if doc.path.ends_with(".java") {
            self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET);
            let mut problems = cron_problems(&doc.text);
            problems.extend(self.checked(doc));
            if doc.text.contains("Hooks.onOperatorDebug()")
                && let Some(summary) = self.build_summary(doc)
            {
                problems.extend(build::operator_debug(&doc.text, &summary).iter().map(|f| f.diagnostic(&doc.text)));
            }
            problems
        } else if local_meta::is_metadata_file(&doc.path) {
            if !host::setting_bool("springBoot.checkMetadataFiles").unwrap_or(true) {
                return Vec::new();
            }
            let (project, _) = self.project(doc);
            let lookup = Lookup(vec![&project, bundled()]);
            let facts = self.summary(doc);
            let context = local_meta::Context { defined: &|name| lookup.prop(name).is_some(), facts: &facts };
            local_meta::check(&doc.text, &context).iter().map(|f| f.diagnostic(&doc.text)).collect()
        } else if self.is_config(doc) {
            let (project, complete) = self.project(doc);
            let lookup = Lookup(vec![&project, bundled()]);
            let summary = if driver_check_on() { self.build_summary(doc) } else { None };
            let facts = self.summary(doc);
            config_problems(&doc.path, &doc.text, &lookup, complete, summary.as_ref(), &facts).into_iter().map(|p| p.diagnostic).collect()
        } else {
            return Vec::new();
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

const MAX_LISTED_PROBLEMS: usize = 400;

struct Listed {
    path: String,
    line: u32,
    character: u32,
    severity: Severity,
    code: String,
    message: String,
}

impl BootTools {
    /// Every configuration file of the project: the standard names and what `spring.config.*` points at.
    fn configuration_files(&mut self, project: &openidea_plugin::Project) -> Vec<String> {
        let mut files: Vec<String> = fileset::GLOBS.iter().flat_map(|g| project.find_files(g, 500).unwrap_or_default()).collect();
        let sets: Vec<fileset::Locations> = self.locations.get(project, COMMAND_BUDGET).iter().map(|(_, l)| l.clone()).collect();
        for set in &sets {
            for tail in &set.tails {
                files.extend(project.find_files(&format!("**/{tail}"), 20).unwrap_or_default());
            }
            for name in &set.names {
                for extension in ["properties", "yml", "yaml"] {
                    files.extend(project.find_files(&format!("**/{name}*.{extension}"), 20).unwrap_or_default());
                }
            }
        }
        files.sort();
        files.dedup();
        files
    }

    fn configuration_problems(&mut self, project: &openidea_plugin::Project) -> (usize, Vec<Listed>) {
        let (catalog, complete) = catalog_of(self.sources.get(project, COMMAND_BUDGET));
        let lookup = Lookup(vec![&catalog, bundled()]);
        let facts = {
            let index = self.facts.get(project, COMMAND_BUDGET);
            Summary::merge(index.iter().map(|(_, f)| f), index.complete())
        };
        let files = self.configuration_files(project);
        let metadata = project.find_files("**/META-INF/*spring-configuration-metadata.json", 20).unwrap_or_default();
        let mut listed = Vec::new();
        for path in files.iter().chain(&metadata) {
            let Ok(text) = project.read_file(path) else { continue };
            let found: Vec<Diagnostic> = if local_meta::is_metadata_file(path) {
                let context = local_meta::Context { defined: &|name| lookup.prop(name).is_some(), facts: &facts };
                local_meta::check(&text, &context).iter().map(|f| f.diagnostic(&text)).collect()
            } else {
                config_problems(path, &text, &lookup, complete, None, &facts).into_iter().map(|p| p.diagnostic).collect()
            };
            listed.extend(found.into_iter().map(|d| Listed {
                path: path.clone(),
                line: d.range.start.line,
                character: d.range.start.character,
                severity: d.severity,
                code: d.code.unwrap_or_default(),
                message: d.message,
            }));
        }
        (files.len() + metadata.len(), listed)
    }

    fn show_diagnostics(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("Open a project first.")?;
        let (checked, mut listed) = self.configuration_problems(&project);
        if listed.is_empty() {
            host::notify(NotificationLevel::Info, "Spring Boot", &format!("No configuration problems in {checked} files."));
            return Ok(None);
        }
        let total = listed.len();
        listed.sort_by(|a, b| (severity_rank(a.severity), &a.path, a.line).cmp(&(severity_rank(b.severity), &b.path, b.line)));
        listed.truncate(MAX_LISTED_PROBLEMS);
        let items: Vec<PickItem> = listed
            .iter()
            .map(|l| {
                let file = l.path.rsplit('/').next().unwrap_or(&l.path);
                PickItem::new(&format!("{}\t{}\t{}", l.path, l.line, l.character), &format!("{:?} {}: {}", l.severity, l.code, l.message))
                    .description(&format!("{file}:{}", l.line + 1))
            })
            .collect();
        let title = format!("Spring Boot configuration: {total} problems in {checked} files");
        let chosen = match host::pick(&title, &items) {
            Err(Error::Invalid(_)) => return Ok(None),
            other => other?,
        };
        let Some(key) = chosen else { return Ok(None) };
        let mut parts = key.rsplitn(3, '\t');
        let (character, line, path) = (parts.next().unwrap_or("0"), parts.next().unwrap_or("0"), parts.next().unwrap_or_default());
        host::open_location(path, line.parse().unwrap_or(0), character.parse().unwrap_or(0))?;
        Ok(None)
    }
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

impl Plugin for BootTools {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "show-diagnostics" => self.show_diagnostics(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

register_plugin!(BootTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const PROPS: &str = "/project/src/main/resources/application.properties";
    const YAML: &str = "/project/src/main/resources/application.yml";

    const MAIL: &str = r#"package com.acme;
@ConfigurationProperties(prefix = "acme.mail")
public class MailProperties {
    private String host = "localhost";
    private boolean ssl;
}
"#;
    const EXTRA: &str = r#"{"properties": [
        {"name": "acme.mode", "type": "java.lang.String", "description": "How the app runs."},
        {"name": "acme.legacy", "type": "java.lang.String", "deprecation": {"replacement": "acme.mode"}}
    ], "hints": [{"name": "acme.mode", "values": [{"value": "fast"}, {"value": "safe"}]}]}"#;

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/MailProperties.java", MAIL);
        host.add_file("src/main/resources/META-INF/additional-spring-configuration-metadata.json", EXTRA);
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        let language = if path.ends_with(".java") { "java" } else if path.ends_with(".properties") { "properties" } else { "yaml" };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    /// Completion at the end of `text`.
    fn complete(path: &str, text: &str) -> Vec<CompletionItem> {
        BootTools::default().completion(&doc(path, text), position(text, text.len()), None)
    }

    fn find<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
        items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"))
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn completes_properties_keys_and_values() {
        let _host = setup();
        let text = "server.port=8080\nspring.datasource.u";
        let items = complete(PROPS, text);
        let url = find(&items, "spring.datasource.url");
        assert_eq!(url.insert_text, "spring.datasource.url=");
        assert_eq!(url.detail.as_deref(), Some("String"));
        assert_eq!(url.replace, Some(range(text, 17, text.len())));
        assert!(items.iter().all(|i| i.label.starts_with("spring.datasource.")));
        assert!(!labels(&items).contains(&"spring.datasource.schema"), "removed keys are not offered");

        let items = complete(PROPS, "logging.le");
        assert!(labels(&items).contains(&"logging.level.root"));
        assert_eq!(find(&items, "logging.level.").insert_text, "logging.level.");

        assert_eq!(labels(&complete(PROPS, "spring.jpa.show-sql=")), ["true", "false"]);
        assert_eq!(labels(&complete(PROPS, "logging.level.com.acme = WA")).len(), 7);
        assert_eq!(labels(&complete(PROPS, "acme.mode=")), ["fast", "safe"]);

        let items = complete(PROPS, "acme.");
        assert_eq!(find(&items, "acme.mail.host").documentation.as_deref(), Some("Default: `localhost`"));
        assert_eq!(find(&items, "acme.mode").documentation.as_deref(), Some("How the app runs"));
        assert_eq!(labels(&complete(PROPS, "acme.mail.ssl=")), ["true", "false"]);
        assert!(complete(PROPS, "# spring.").is_empty());
        assert!(complete("/project/messages.properties", "spring.").is_empty());
    }

    #[test]
    fn completes_yaml_segments_and_values() {
        let _host = setup();
        let text = "spring:\n  datasource:\n    url: x\n    pass";
        let items = complete(YAML, text);
        let password = find(&items, "password");
        assert_eq!(password.insert_text, "password: ");
        assert_eq!(password.replace, Some(range(text, text.len() - 4, text.len())));
        assert!(labels(&items).contains(&"hikari"));

        let items = complete(YAML, "spring:\n  dat");
        assert_eq!(find(&items, "datasource").insert_text, "datasource:\n    ");
        assert!(find(&items, "datasource").detail.as_deref().unwrap().ends_with("properties"));
        let items = complete(YAML, "spring:\n  datasource.u");
        assert_eq!(find(&items, "url").insert_text, "datasource.url: ");

        let items = complete(YAML, "logging:\n  ");
        assert_eq!(find(&items, "level").insert_text, "level:\n    ");
        let items = complete(YAML, "logging:\n  level:\n    ");
        assert_eq!(find(&items, "root").insert_text, "root: ");
        assert_eq!(labels(&complete(YAML, "logging:\n  level:\n    com.acme: ")).len(), 7);
        assert!(labels(&complete(YAML, "spring:\n  jpa:\n    hibernate:\n      ddl-auto: ")).contains(&"update"));
        assert!(labels(&complete(YAML, "acme:\n  ")).contains(&"mail"));
        assert!(labels(&complete(YAML, "")).contains(&"server"));
    }

    fn problems(path: &str, text: &str) -> Vec<Diagnostic> {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/MailProperties.java", MAIL);
        BootTools::default().line_markers(&doc(path, text));
        host.diagnostics().get(path).cloned().unwrap_or_default()
    }

    fn codes(problems: &[Diagnostic]) -> Vec<(&str, &str)> {
        problems.iter().map(|d| (d.code.as_deref().unwrap_or_default(), d.message.as_str())).collect()
    }

    #[test]
    fn warns_about_deprecated_keys_and_fixes_them() {
        let _host = setup();
        let text = "spring.redis.host=localhost\nspring.jpa.show-sql=true\n";
        let mut tools = BootTools::default();
        tools.line_markers(&doc(PROPS, text));
        let found = &_host.diagnostics()[PROPS];
        assert_eq!(codes(found), [(
            "deprecated-property",
            "`spring.redis.host` is no longer supported. Use `spring.data.redis.host` instead."
        )]);
        assert!(found[0].deprecated);
        let actions = tools.code_actions(&doc(PROPS, text), Range::new(Position::new(0, 3), Position::new(0, 3)));
        assert_eq!(actions[0].edits[0].1, [TextEdit { range: range(text, 0, 17), text: "spring.data.redis.host".into() }]);
        assert!(tools.code_actions(&doc(PROPS, text), Range::new(Position::new(1, 3), Position::new(1, 3))).is_empty());

        let text = "logging:\n  file: app.log\nspring:\n  redis:\n    host: x\n";
        let actions = tools.code_actions(&doc(YAML, text), Range::new(Position::new(1, 3), Position::new(1, 3)));
        assert_eq!(actions[0].edits[0].1[0].text, "file.name");
        let other_parent = tools.code_actions(&doc(YAML, text), Range::new(Position::new(4, 5), Position::new(4, 5)));
        assert!(other_parent.is_empty(), "spring.redis.host → spring.data.redis.host moves to another parent");

        let text = "acme.legacy=x\n";
        let actions = tools.code_actions(&doc(PROPS, text), Range::new(Position::new(0, 1), Position::new(0, 1)));
        assert_eq!(actions[0].title, "Replace with `acme.mode`");
    }

    #[test]
    fn warns_about_unknown_keys_of_known_groups_only() {
        let text = "spring.datasource.urll=x\nspring.datasource.druid.initial-size=5\nacme.anything=1\nacme.mail.hots=x\n\
                    logging.level.com.acme=DEBUG\nspring.cloud.config.uri=x\nserver.servlet.context-path=/\n";
        let found = problems(PROPS, text);
        assert_eq!(codes(&found), [
            ("unknown-property", "`spring.datasource.urll` is not a known property of `spring.datasource`."),
            ("unknown-property", "`acme.mail.hots` is not a known property of `acme.mail`."),
        ]);
        assert_eq!(found[0].range, range(text, 0, 22));

        let text = "spring:\n  datasource:\n    hikari:\n      maximum-pool-size: 5\n    urll: x\nserver:\n  servlet:\n    contextPath: /\n";
        assert_eq!(codes(&problems(YAML, text)), [(
            "unknown-property",
            "`spring.datasource.urll` is not a known property of `spring.datasource`."
        )]);
    }

    #[test]
    fn spring_cloud_key_completes() {
        let _host = setup();
        let items = complete(PROPS, "spring.cloud.config.u");
        assert_eq!(find(&items, "spring.cloud.config.uri").insert_text, "spring.cloud.config.uri=");
        assert!(labels(&complete(PROPS, "eureka.client.service-u")).contains(&"eureka.client.service-url."));
        assert!(labels(&complete(PROPS, "spring.cloud.gateway.server.webflux.rou"))
            .contains(&"spring.cloud.gateway.server.webflux.routes"));

        let items = complete(YAML, "spring:\n  cloud:\n    config:\n      ur");
        assert_eq!(find(&items, "uri").insert_text, "uri: ");
    }

    #[test]
    fn unknown_key_inside_known_cloud_group_is_reported() {
        let text = "spring.cloud.config.ur=x\nspring.cloud.config.uri=x\nspring.cloud.unheard.of=1\n";
        let found = problems(PROPS, text);
        assert_eq!(codes(&found), [(
            "unknown-property",
            "`spring.cloud.config.ur` is not a known property of `spring.cloud.config`."
        )]);
    }

    #[test]
    fn checks_cron_values_in_config_and_java() {
        let text = "app.cleanup.cron=0 0 * * *\napp.report-cron=0 0 6 * * MON-FRI\napp.other.cron=${CRON:-}\n";
        let found = problems(PROPS, text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("invalid-cron"));
        assert!(found[0].message.contains("6 fields"));
        assert_eq!(found[0].range, range(text, 17, 26));

        let text = "jobs:\n  nightly:\n    cron: \"0 0 25 * * *\"\n";
        let found = problems(YAML, text);
        assert_eq!(codes(&found), [("invalid-cron", "Cron hour `25`: 25 is outside 0-23.")]);

        let java = r#"class Jobs {
    @Scheduled(cron = "0 0 25 * * *")
    void bad() {}
    @Scheduled(cron = "0 */5 * * * *", zone = "UTC")
    void good() {}
    @Scheduled(cron = "${jobs.cron}")
    void configured() {}
    @Scheduled(cron = PREFIX + " * * *")
    void computed() {}
}
"#;
        let path = "/project/src/main/java/Jobs.java";
        let found = problems(path, java);
        assert_eq!(codes(&found), [("invalid-cron", "Cron hour `25`: 25 is outside 0-23.")]);
        let at = java.find("0 0 25").unwrap();
        assert_eq!(found[0].range, range(java, at, at + 12));
    }

    #[test]
    fn redundant_annotation_gets_a_removal_action() {
        let _host = MockHost::install();
        let text = "package a;\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n@SpringBootApplication\n@ComponentScan\nclass App {}\n";
        let path = "/project/src/main/java/App.java";
        let mut tools = BootTools::default();
        let actions = tools.code_actions(&doc(path, text), Range::new(Position::new(3, 3), Position::new(3, 3)));
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Remove redundant @ComponentScan");
        assert_eq!(actions[0].edits[0].1, [TextEdit { range: range(text, text.find("@ComponentScan").unwrap(), text.find("class").unwrap()), text: String::new() }]);
        assert!(tools.code_actions(&doc(path, text), Range::new(Position::new(4, 1), Position::new(4, 1))).is_empty());
    }

    #[test]
    fn removal_action_is_not_offered_when_application_setup_checks_are_off() {
        let host = MockHost::install();
        host.set_setting("springBoot.checkApplicationSetup", "false");
        let text = "package a;\nimport org.springframework.boot.autoconfigure.SpringBootApplication;\n@SpringBootApplication\n@ComponentScan\nclass App {}\n";
        let mut tools = BootTools::default();
        let actions = tools.code_actions(&doc("/project/src/main/java/App.java", text), Range::new(Position::new(3, 3), Position::new(3, 3)));
        assert!(actions.is_empty());
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        host.add_file("src/test/resources/spring.properties", "other=1\n");
        let path = "/project/src/test/java/OwnerTests.java";
        let broken = "import org.springframework.context.event.EventListener;\n@SpringBootTest\nclass OwnerTests {\n    OwnerTests(Service s) {}\n    @EventListener\n    void é😀(A a, B b) {}\n}\n";
        let mut tools = BootTools::default();
        tools.line_markers(&doc(path, broken));
        let found = &host.diagnostics()[path];
        let codes: Vec<_> = found.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, ["test-constructor-autowire", "event-listener-signature"]);
        assert_eq!(found[1].range, range(broken, broken.find("é😀").unwrap(), broken.find("(A").unwrap()));
        assert_eq!(found[1].range.end.character - found[1].range.start.character, 3);

        let fixed = broken.replace("    OwnerTests(", "    @Autowired OwnerTests(").replace("(A a, B b)", "(A a)");
        tools.line_markers(&doc(path, &fixed));
        assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn junit_platform_properties_sets_autowire_mode() {
        let host = MockHost::install();
        host.add_file("src/test/resources/junit-platform.properties", "spring.test.constructor.autowire.mode=all\n");
        let path = "/project/src/test/java/OwnerTests.java";
        BootTools::default().line_markers(&doc(path, "@SpringBootTest\nclass OwnerTests {\n    OwnerTests(Service s) {}\n}\n"));
        assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn test_constructor_check_is_quiet_when_mode_is_configured() {
        let host = MockHost::install();
        host.add_file("src/test/resources/spring.properties", "spring.test.constructor.autowire.mode=all\n");
        let path = "/project/src/test/java/OwnerTests.java";
        BootTools::default().line_markers(&doc(path, "@SpringBootTest\nclass OwnerTests {\n    OwnerTests(Service s) {}\n}\n"));
        assert!(host.diagnostics().get(path).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn reports_values_that_do_not_convert() {
        let text = "server.port=80x\nspring.jpa.show-sql=maybe\nserver.tomcat.max-http-form-post-size=2XB\n\
                    spring.mvc.async.request-timeout=10 s\nserver.port=${PORT}\nacme.mail.ssl=true\nspring.application.name=any\n";
        let found = problems(PROPS, text);
        let codes: Vec<_> = found.iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(codes, ["invalid-property-value", "invalid-property-value", "invalid-property-value", "invalid-property-value"]);
        assert_eq!(found[0].range, range(text, 12, 15));
        assert!(problems(YAML, "server:\n  port: 8080\n  address: whatever\n").is_empty());
        assert_eq!(problems(YAML, "server:\n  port: abc\n").len(), 1);
    }

    #[test]
    fn property_value_check_can_be_switched_off() {
        let host = MockHost::install();
        host.set_setting("springBoot.checkPropertyValues", "false");
        BootTools::default().line_markers(&doc(PROPS, "server.port=x\n"));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn checks_a_local_metadata_file() {
        let host = MockHost::install();
        let path = "/project/src/main/resources/META-INF/additional-spring-configuration-metadata.json";
        let text = r#"{"properties": [{"name": "acme.x", "type": "java.lang.String", "colour": 1}]}"#;
        BootTools::default().line_markers(&doc(path, text));
        let codes: Vec<_> = host.diagnostics()[path].iter().map(|d| d.code.clone().unwrap()).collect();
        assert_eq!(codes, ["unknown-metadata-field", "missing-metadata-description"]);
    }

    #[test]
    fn offers_to_define_an_unknown_key_in_the_metadata_file() {
        let host = setup();
        let text = "spring.datasource.urll=x\n";
        let actions = BootTools::default().code_actions(&doc(PROPS, text), Range::new(Position::new(0, 3), Position::new(0, 3)));
        let define = actions.iter().find(|a| a.title.starts_with("Define")).expect("define action");
        assert!(define.edits[0].0.ends_with("additional-spring-configuration-metadata.json"));
        assert!(define.edits[0].1[0].text.contains("spring.datasource.urll"));
        drop(host);
        let _host = MockHost::install();
        let none = BootTools::default().code_actions(&doc(PROPS, text), Range::new(Position::new(0, 3), Position::new(0, 3)));
        assert!(none.iter().all(|a| !a.title.starts_with("Define")), "no metadata file to edit");
    }

    #[test]
    fn config_import_and_class_values_navigate() {
        let host = setup();
        host.add_file("src/main/resources/extra.yml", "a: 1\n");
        host.add_file("src/main/java/com/acme/Handler.java", "package com.acme;\npublic class Handler {}\n");
        let text = "spring.config.import=optional:classpath:extra.yml\n";
        let found = BootTools::default().definition(&doc(PROPS, text), Position::new(0, 40));
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("extra.yml"));
        assert!(BootTools::default().definition(&doc(PROPS, "spring.config.import=vault://x\n"), Position::new(0, 25)).is_empty());
    }

    #[test]
    fn flags_a_driverless_datasource_url_only_when_asked() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<artifactId>postgresql</artifactId>");
        let text = "spring.datasource.url=jdbc:mysql://db/app\n";
        BootTools::default().line_markers(&doc(PROPS, text));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        host.set_setting("springBoot.checkDriverDependency", "true");
        BootTools::default().line_markers(&doc(PROPS, text));
        assert_eq!(host.diagnostics()[PROPS][0].code.as_deref(), Some("missing-jdbc-driver"));
    }

    #[test]
    fn suggests_the_reactor_agent_over_operator_debug() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<artifactId>reactor-tools</artifactId>");
        let path = "/project/src/main/java/A.java";
        BootTools::default().line_markers(&doc(path, "class A { void f() { Hooks.onOperatorDebug(); } }"));
        assert_eq!(host.diagnostics()[path][0].code.as_deref(), Some("reactor-debug-agent"));
    }

    #[test]
    fn checks_enum_class_and_list_values() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Props.java", "package com.acme;\n@ConfigurationProperties(\"acme.x\")\nclass Props { Mode mode; Class<?> handler; List<Integer> ports; }\nenum Mode { FAST, SAFE_MODE }\nclass Handler {}\n");
        host.add_file("src/main/resources/META-INF/additional-spring-configuration-metadata.json", r#"{"properties": [
            {"name": "acme.mode", "type": "com.acme.Mode", "description": "d"},
            {"name": "acme.handler", "type": "java.lang.Class<?>", "description": "d"},
            {"name": "acme.ports", "type": "java.util.List<java.lang.Integer>", "description": "d"}]}"#);
        let text = "acme.mode=slow\nacme.mode=safe-mode\nacme.handler=com.acme.Gone\nacme.handler=com.acme.Handler\nacme.handler=org.lib.Any\nacme.ports=80,x\nacme.ports=80,90\nmanagement.endpoint.health.show-details=sometimes\nmanagement.endpoint.health.show-details=always\n";
        BootTools::default().line_markers(&doc(PROPS, text));
        let lines: Vec<u32> = host.diagnostics()[PROPS].iter().filter(|d| d.code.as_deref() == Some("invalid-property-value")).map(|d| d.range.start.line).collect();
        assert_eq!(lines, [0, 2, 5, 7]);
        let yaml = "acme:\n  ports:\n    - 1\n    - two\n";
        BootTools::default().line_markers(&doc(YAML, yaml));
        assert_eq!(host.diagnostics()[YAML].iter().filter(|d| d.code.as_deref() == Some("invalid-property-value")).count(), 1);
    }

    #[test]
    fn files_pointed_at_by_the_standard_files_are_configuration() {
        let host = setup();
        host.add_file("src/main/resources/application.properties", "spring.config.import=optional:classpath:custom/extra.yml\n");
        let extra = "/project/src/main/resources/custom/extra.yml";
        BootTools::default().line_markers(&doc(extra, "server:\n  port: abc\n"));
        assert_eq!(host.diagnostics()[extra].len(), 1);
        let other = "/project/src/main/resources/custom/other.yml";
        BootTools::default().line_markers(&doc(other, "server:\n  port: abc\n"));
        assert!(host.diagnostics().get(other).is_none());
        assert!(!BootTools::default().completion(&doc(extra, "server:\n  po"), position("server:\n  po", 13), None).is_empty());
    }

    #[test]
    fn diagnostics_command_lists_problems_and_opens_the_chosen_one() {
        let host = setup();
        host.add_file("src/main/resources/application.properties", "server.port=abc\nspring.datasource.urll=x\n");
        host.add_file("src/main/resources/application-dev.yml", "server:\n  port: 80\n");
        host.answer_next_prompt(Some(&["/project/src/main/resources/application.properties\t0\t12"]));
        let _ = BootTools::default().run_command("show-diagnostics", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/main/resources/application.properties".to_owned(), 0, 12)]);
        let clean = MockHost::install();
        clean.add_file("src/main/resources/application.properties", "server.port=1\n");
        BootTools::default().run_command("show-diagnostics", &clean.invocation(), None).unwrap();
        assert!(clean.opened_locations().is_empty());
    }
}
