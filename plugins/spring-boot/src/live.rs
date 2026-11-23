//! Cached source/metadata facts for live providers; discovery runs only in lifecycle events.
use super::*;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::inlay_hints::{InlayHint, InlayHints, InlayKind};
use openidea_plugin::{Event, EventKind};
use std::collections::{BTreeMap, BTreeSet};
use spring_text::index::{Index, Indexes, project_of};
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};
const MAX_TEXT: usize = 128 << 10;
const GLOBS: &[&str] = &[
    "**/*.java",
    "**/*.kt",
    "**/META-INF/*spring-configuration-metadata.json",
    "**/spring.properties",
    "**/junit-platform.properties",
    "**/application*.properties",
    "**/application*.yml",
    "**/application*.yaml",
    "**/bootstrap*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/gradle.properties",
    "**/libs.versions.toml",
];
#[derive(Clone)]
pub(super) struct File {
    meta: Option<Meta>,
    facts: Option<facts::Facts>,
    build: Option<build::Facts>,
    mode: bool,
    locations: Option<fileset::Locations>,
    complete: bool,
}
fn read(path: &str, text: &str) -> Option<File> {
    let complete =
        text.len() <= MAX_TEXT && (!is_config_file(path) || config::keys(path, text).len() <= 512);
    if !complete {
        return Some(File {
            meta: None,
            facts: None,
            build: None,
            mode: false,
            locations: None,
            complete: false,
        });
    }
    let metadata = path.ends_with(".java") || local_meta::is_metadata_file(path);
    let meta = metadata.then(|| parse_source(path, text)).flatten();
    let source = path.ends_with(".java") || path.ends_with(".kt");
    let build = build::BUILD_GLOBS
        .iter()
        .any(|glob| spring_text::index::glob_matches(glob, path))
        .then(|| build::read(path, text))
        .flatten();
    let mode = AUTOWIRE_MODE_GLOBS
        .iter()
        .any(|glob| spring_text::index::glob_matches(glob, path))
        && checks::sets_autowire_mode(path, text).is_some();
    Some(File {
        complete: (!local_meta::is_metadata_file(path) || meta.is_some())
            && !(source
                && text.contains("ConfigurationProperties")
                && meta.is_none()),
        meta,
        facts: source.then(|| facts::read(path, text)).flatten(),
        build,
        mode,
        locations: fileset::read(path, text),
    })
}
#[derive(PartialEq)]
struct Key {
    generation: u64,
    roots: Vec<String>,
    fresh: bool,
    watching: bool,
}
pub(crate) struct State {
    pub(super) files: Indexes<File>,
    watching: bool,
    snapshots: BTreeMap<String, (Key, Arc<Shared>)>,
    applied: BTreeMap<String, (u64, u64)>,
    events_notice: std::sync::atomic::AtomicBool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            files: Indexes::new(GLOBS, read),
            watching: false,
            snapshots: BTreeMap::new(),
            applied: BTreeMap::new(),
            events_notice: Default::default(),
        }
    }
}
impl BootTools {
    pub(crate) fn activate_live(&mut self) {
        self.live.watching = host::subscribe_paths("files-changed", GLOBS).is_ok();
        if matches!(host::subscribe(EventKind::FileOpened), Err(Error::Denied(_))) {
            ui_kit::denied::notify_once(&self.live.events_notice, "Checking the whole project (unknown keys, test constructors)", "events", "Spring Boot Tools");
        }
        let _ = host::subscribe(EventKind::FileSaved);
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let budget = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250));
            if budget.is_zero() {
                break;
            }
            self.live.files.get(&project, budget);
        }
    }
    #[cfg(test)]
    pub(crate) fn warm_live(&mut self, doc: &Document) {
        let Some(project) = project_of(&doc.path) else {
            return;
        };
        self.live.files.get(&project, Duration::from_millis(250));
        self.live
            .files
            .update_existing(&project, &doc.path, Some(&doc.text));
    }
    pub(crate) fn live_events(&mut self, events: &[Event]) {
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for (path, removed) in event_paths(events) {
            self.jars.source_changed(path, removed);
        }
        self.live.files.on_events(events, deadline);
        let mut warmed = BTreeSet::new();
        for (path, _) in event_paths(events) {
            if !GLOBS
                .iter()
                .any(|glob| spring_text::index::glob_matches(glob, path))
            {
                continue;
            }
            let Some(project) = project_of(path) else {
                continue;
            };
            let Ok(root) = project.root() else {
                continue;
            };
            let budget = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250));
            if budget.is_zero() {
                return;
            }
            if warmed.insert(root) {
                self.live.files.get(&project, budget);
            }
        }
    }
    pub(super) fn live_snapshot(&mut self, doc: &Document) -> Option<Snapshot> {
        if doc.text.len() > MAX_TEXT {
            return None;
        }
        let root = project_of(&doc.path)?.root().ok()?;
        let roots = current_roots()?;
        if spring_text::scope::current_owner(&doc.path, &roots).as_deref() != Some(root.as_str()) {
            return None;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        doc.text.hash(&mut hasher);
        let hash = hasher.finish();
        let live = &mut self.live;
        let applied = live.applied.get(&doc.path).copied();
        let unchanged = live
            .files
            .cached(&doc.path)
            .is_some_and(|index| applied == Some((hash, index.generation())));
        let index = if unchanged {
            live.files.cached(&doc.path)?
        } else {
            let index = live.files.for_cached_document(&doc.path, &doc.text)?;
            if live.applied.len() >= 64 {
                live.applied.clear();
            }
            live.applied
                .insert(doc.path.clone(), (hash, index.generation()));
            &*index
        };
        let key = Key {
            generation: index.generation(),
            roots,
            fresh: index.fresh(),
            watching: live.watching,
        };
        let shared = match live.snapshots.get(&root) {
            Some((known, shared)) if *known == key => shared.clone(),
            _ => {
                let shared = Arc::new(shared_of(index, &key.roots, &root, live.watching));
                live.snapshots
                    .retain(|known, _| key.roots.contains(known));
                live.snapshots
                    .insert(root, (key, shared.clone()));
                shared
            }
        };
        let is_config = is_config_file(&doc.path)
            || fileset::is_configuration(
                &doc.path,
                shared.locations.iter().map(|(path, l)| (path.as_str(), l)),
            );
        Some(Snapshot { shared, is_config })
    }
}
fn event_paths(events: &[Event]) -> Vec<(&str, bool)> {
    let mut paths = Vec::new();
    for event in events {
        match event {
            Event::FileOpened(path) | Event::FileSaved(path) => paths.push((path.as_str(), false)),
            Event::FilesChanged(files) => {
                paths.extend(files.removed.iter().map(|path| (path.as_str(), true)));
                paths.extend(
                    files
                        .created
                        .iter()
                        .chain(&files.modified)
                        .map(|path| (path.as_str(), false)),
                );
            }
            _ => {}
        }
    }
    paths
}
// Bounded current ownership inventory; unknown or malformed scope fails closed.
pub(super) fn current_roots() -> Option<Vec<String>> {
    let projects = host::projects().ok()?;
    if projects.len() > 64 {
        return None;
    }
    projects
        .into_iter()
        .map(|project| {
            let root = project.root().ok()?;
            spring_text::scope::scope_path(&root).map(|_| root)
        })
        .collect()
}
pub(super) struct Shared {
    pub(super) catalog: Catalog,
    pub(super) complete: bool,
    pub(super) ambiguous: BTreeSet<String>,
    pub(super) facts: Summary,
    pub(super) build: Option<build::Summary>,
    pub(super) mode: Option<bool>,
    pub(super) profiles: Vec<String>,
    pub(super) locations: Vec<(String, fileset::Locations)>,
}
pub(super) struct Snapshot {
    shared: Arc<Shared>,
    pub(super) is_config: bool,
}
impl Deref for Snapshot {
    type Target = Shared;
    fn deref(&self) -> &Shared {
        &self.shared
    }
}
pub(super) fn shared_of(index: &Index<File>, roots: &[String], owner: &str, watching: bool) -> Shared {
    let owned: Vec<_> = index
        .iter()
        .filter(|(path, _)| spring_text::scope::current_owner(path, roots).as_deref() == Some(owner))
        .collect();
    let files: Vec<_> = owned.iter().map(|(_, file)| *file).collect();
    let complete = index.fresh()
        && files.len() == index.files().len()
        && files.iter().all(|file| file.complete)
        && watching;
    let mut properties: BTreeMap<String, spring_text::metadata::Prop> = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    let mut hints = Vec::new();
    for meta in files.iter().filter_map(|file| file.meta.as_ref()) {
        for prop in &meta.props {
            let key = canonical(&prop.name);
            if properties.get(&key).is_some_and(|known| known != prop) {
                ambiguous.insert(key.clone());
            }
            properties.entry(key).or_insert_with(|| prop.clone());
        }
        hints.extend(meta.hints.clone());
    }
    let mut catalog = Catalog::default();
    catalog.add(&Meta {
        props: properties
            .into_iter()
            .filter(|(key, _)| !ambiguous.contains(key))
            .map(|(_, prop)| prop)
            .collect(),
        hints,
    });
    let facts = Summary::merge(
        files.iter().filter_map(|file| file.facts.as_ref()),
        complete,
    );
    let build = (complete && files.iter().any(|file| file.build.is_some()))
        .then(|| build::Summary::merge(files.iter().filter_map(|file| file.build.as_ref())));
    let mode = complete.then(|| files.iter().any(|file| file.mode));
    let profiles: BTreeSet<String> = owned
        .iter()
        .filter(|(path, _)| {
            path.rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with("application-"))
        })
        .filter_map(|(path, _)| nav::profile_of(path))
        .collect();
    let locations = owned
        .iter()
        .filter_map(|(path, file)| Some(((*path).to_owned(), file.locations.clone()?)))
        .collect();
    Shared {
        catalog,
        complete: complete && ambiguous.is_empty(),
        ambiguous,
        facts,
        build,
        mode,
        profiles: profiles.into_iter().collect(),
        locations,
    }
}
impl DiagnosticsProvider for BootTools {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let Some(cached) = self.live_snapshot(doc) else {
            return Vec::new();
        };
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&cached.catalog, &jars, self.jars.bundled(&doc.path)]);
        let found = if doc.language == "java" && doc.path.ends_with(".java") {
            let mut problems = cron_problems(&doc.text);
            problems.extend(
                checks::check_java(
                    &java::parse(&doc.text),
                    cached.mode,
                    checks::Groups::from_settings(),
                )
                .iter()
                .map(|finding| finding.diagnostic(&doc.text)),
            );
            if let Some(build) = &cached.build {
                problems.extend(
                    build::operator_debug(&doc.text, build)
                        .iter()
                        .map(|finding| finding.diagnostic(&doc.text)),
                );
            }
            problems
        } else if local_meta::is_metadata_file(&doc.path) && doc.language == "json" {
            if !host::setting_bool("springBoot.checkMetadataFiles").unwrap_or(true) {
                return Vec::new();
            }
            let context = local_meta::Context {
                defined: &|name| !cached.complete || self.jars.version_scoped(&doc.path) || lookup.prop(name).is_some(),
                facts: &cached.facts,
            };
            local_meta::check(&doc.text, &context)
                .iter()
                .map(|finding| finding.diagnostic(&doc.text))
                .collect()
        } else if cached.is_config
            && matches!(doc.language.as_str(), "properties" | "yaml")
            && config::keys(&doc.path, &doc.text).len() <= 512
        {
            config_problems(
                &doc.path,
                &doc.text,
                &lookup,
                cached.complete && !self.jars.version_scoped(&doc.path),
                if driver_check_on() {
                    cached.build.as_ref()
                } else {
                    None
                },
                &cached.facts,
            )
            .into_iter()
            .map(|problem| problem.diagnostic)
            .collect()
        } else {
            Vec::new()
        };
        found.into_iter().take(500).collect()
    }
}
fn markdown(key: &str, property: &spring_text::metadata::Prop) -> String {
    fn safe(text: &str, limit: usize) -> String {
        text.chars()
            .take(limit)
            .collect::<String>()
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('`', "\\`")
    }
    let mut text = format!(
        "**{}**\n\nDeclared type: `{}`",
        safe(key, 256),
        safe(&property.ty, 256)
    );
    if !property.description.is_empty() {
        text.push_str(&format!("\n\n{}", safe(&property.description, 2000)));
    }
    if !property.default.is_empty() {
        text.push_str(&format!(
            "\n\nDeclared default: `{}`",
            safe(&property.default, 256)
        ));
    }
    if let Some(deprecated) = &property.deprecated {
        text.push_str("\n\nDeprecated.");
        if !deprecated.replacement.is_empty() {
            text.push_str(&format!(
                " Replacement: `{}`",
                safe(&deprecated.replacement, 256)
            ));
        }
    }
    text.push_str("\n\nDeclaration metadata; no effective runtime value is resolved.");
    text
}
impl HoverProvider for BootTools {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if let Some(hover) = crate::placeholder_nav::hover(doc, at) {
            return Some(hover);
        }
        if !matches!(doc.language.as_str(), "properties" | "yaml") {
            return None;
        }
        let cached = self.live_snapshot(doc)?;
        if !cached.is_config {
            return None;
        }
        let off = offset(&doc.text, at)?;
        let key = config::keys(&doc.path, &doc.text)
            .into_iter()
            .find(|key| key.at <= off && off <= key.end)?;
        if placeholder(&key.key) || cached.ambiguous.contains(&canonical(&key.key)) {
            return None;
        }
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&cached.catalog, &jars, self.jars.bundled(&doc.path)]);
        let prop = lookup.prop(&key.key)?;
        if prop.open() {
            return None;
        }
        let mut content = markdown(&key.key, prop);
        if Lookup(vec![&cached.catalog]).prop(&key.key).is_some() {
            content.push_str("\n\nMetadata origin: project-local current/cached declarations.");
        } else if Lookup(vec![&jars]).prop(&key.key).is_some() {
            content.push_str("\n\nMetadata origin: selected dependency JAR snapshot.");
            if let Some(provenance) = self.jars.provenance(&doc.path) {
                content.push_str("\n\n");
                content.push_str(
                    &provenance
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                        .replace('`', "\\`"),
                );
            }
        } else {
            content.push_str("\n\nMetadata origin: bundled fallback snapshot. ");
            content.push_str(
                &meta::bundled_origin()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('`', "\\`"),
            );
            content.push_str(
                ". Packaged dataset version, not an inferred project dependency version.",
            );
        }
        Some(Hover::new(content).range(range(&doc.text, key.at, key.end)))
    }
}
impl InlayHints for BootTools {
    fn inlay_hints(&mut self, doc: &Document, requested: Range) -> Vec<InlayHint> {
        if !matches!(doc.language.as_str(), "properties" | "yaml") {
            return Vec::new();
        }
        let Some(cached) = self.live_snapshot(doc) else {
            return Vec::new();
        };
        if !cached.is_config {
            return Vec::new();
        }
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&cached.catalog, &jars, self.jars.bundled(&doc.path)]);
        config::keys(&doc.path, &doc.text)
            .into_iter()
            .filter_map(|key| {
                let location = range(&doc.text, key.at, key.end);
                if location.end < requested.start
                    || location.end > requested.end
                    || placeholder(&key.key)
                    || cached.ambiguous.contains(&canonical(&key.key))
                {
                    return None;
                }
                let prop = lookup.prop(&key.key)?;
                if prop.open() || prop.ty.is_empty() || prop.ty.len() > 128 || prop.ty.chars().any(char::is_control) {
                    return None;
                }
                Some(
                    InlayHint::new(location.end, format!(": {}", prop.ty))
                        .kind(InlayKind::Type)
                        .padding_left()
                        .tooltip("Declared type from bundled/cached Spring Boot metadata; not an effective runtime value."),
                )
            })
            .take(200)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{mock::MockHost, text::position};
    const PROPS: &str = "/project/src/main/resources/application.properties";
    const META: &str =
        "/project/src/main/resources/META-INF/additional-spring-configuration-metadata.json";
    const JSON: &str = r#"{"properties":[{"name":"app.greeting","type":"java.lang.String","defaultValue":"hello","description":"Greeting"}]}"#;
    fn doc(path: &str, text: &str) -> Document {
        Document {
            path: path.into(),
            language: if path.ends_with(".java") {
                "java"
            } else if path.ends_with(".json") {
                "json"
            } else if path.ends_with(".properties") {
                "properties"
            } else {
                "yaml"
            }
            .into(),
            text: text.into(),
            version: 1,
        }
    }
    fn setup() -> (MockHost, BootTools) {
        let host = MockHost::install();
        host.add_file(
            "src/main/resources/META-INF/additional-spring-configuration-metadata.json",
            JSON,
        );
        let mut tools = BootTools::default();
        tools.activate_live();
        (host, tools)
    }
    fn all() -> Range {
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
    }
    #[test]
    fn missing_events_permission_is_explained_once() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut tools = BootTools::default();
        tools.activate_live();
        tools.activate_live();
        let notes = host.notifications();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "Checking the whole project (unknown keys, test constructors) needs the events permission. Allow it in Settings › Plugins › Spring Boot Tools.");
    }
    #[test]
    fn registered_metadata_hover_does_not_expand_foreign_qualified_field_by_simple_name() {
        let h = MockHost::install();
        h.add_file("src/main/java/App.java","import org.springframework.boot.context.properties.ConfigurationProperties; @ConfigurationProperties(prefix=\"app\") class App {foreign.Pool pool;static class Pool {String wrong;}}");
        let mut tools = BootTools::default();
        tools.activate_live();
        let d = doc(PROPS, "app.pool.wrong=secret\n");
        assert!(tools.hover(&d, Position::new(0, 6)).is_none());
        assert!(tools.inlay_hints(&d, all()).is_empty());
        h.add_file("src/main/java/App.java","import org.springframework.boot.context.properties.ConfigurationProperties; class Other {static class Pool {String wrong;}} @ConfigurationProperties(prefix=\"app\") class App {Pool pool;static class Pool {String right;}}");
        tools.live_events(&[Event::FileSaved("/project/src/main/java/App.java".into())]);
        assert!(tools.hover(&d, Position::new(0, 6)).is_none());
        assert!(
            tools
                .hover(&doc(PROPS, "app.pool.right=secret\n"), Position::new(0, 6))
                .is_some()
        );
        assert!(h.requests().is_empty() && h.process_runs().is_empty());
    }
    #[test]
    fn registered_catalogue_accepts_unique_exact_import_or_fully_qualified_owner() {
        for source in [
            "import org.springframework.boot.context.properties.ConfigurationProperties; import java.util.List; @ConfigurationProperties(prefix=\"app\") class A<T> {String name;}",
            "@org.springframework.boot.context.properties.ConfigurationProperties(prefix=\"app\") class A {String name;}",
        ] {
            let h = MockHost::install();
            h.add_file("src/main/java/A.java", source);
            let mut tools = BootTools::default();
            tools.activate_live();
            let d = doc(PROPS, "app.name=secret\n");
            let hover = tools
                .hover(&d, Position::new(0, 4))
                .expect("explicit source declaration owner");
            assert!(
                hover.markdown.contains("Declared type: `String`")
                    && !hover.markdown.contains("secret")
            );
            assert_eq!(tools.inlay_hints(&d, all()).len(), 1);
            assert!(h.requests().is_empty() && h.process_runs().is_empty());
        }
    }
    #[test]
    fn registered_catalogue_requires_explicit_annotation_owner() {
        for source in [
            "@ConfigurationProperties(prefix=\"app\") class A {String name;}",
            "import org.springframework.boot.context.properties.*; @ConfigurationProperties(prefix=\"app\") class A {String name;}",
            "import org.springframework.boot.context.properties.ConfigurationProperties; class ConfigurationProperties {} @ConfigurationProperties(prefix=\"app\") class A {String name;}",
            "import org.springframework.boot.context.properties.ConfigurationProperties; @ConfigurationProperties(prefix=\"app\") class A<ConfigurationProperties> {String name;}",
        ] {
            let h = MockHost::install();
            h.add_file("src/main/java/A.java", source);
            let mut tools = BootTools::default();
            tools.activate_live();
            let d = doc(PROPS, "app.name=secret\n");
            assert!(tools.hover(&d, Position::new(0, 4)).is_none(), "{source}");
            assert!(tools.inlay_hints(&d, all()).is_empty());
            assert!(h.requests().is_empty() && h.process_runs().is_empty());
        }
    }
    #[test]
    fn registered_metadata_hover_does_not_invent_prefix_from_expression_fragment() {
        let h = MockHost::install();
        h.add_file("src/main/java/A.java","import org.springframework.boot.context.properties.ConfigurationProperties; @ConfigurationProperties(prefix=\"app\" + SUFFIX) class A {String name;}");
        let mut tools = BootTools::default();
        tools.activate_live();
        let d = doc(PROPS, "app.name=secret\n");
        assert!(tools.hover(&d, Position::new(0, 4)).is_none());
        assert!(tools.inlay_hints(&d, all()).is_empty());
        assert!(h.requests().is_empty() && h.process_runs().is_empty());
    }
    #[test]
    fn registered_hover_never_presents_explicit_foreign_annotation_as_boot_declaration() {
        let h = MockHost::install();
        h.add_file("src/main/java/A.java","import foreign.ConfigurationProperties; @ConfigurationProperties(prefix=\"app\") class A {String name;}");
        let mut tools = BootTools::default();
        tools.activate_live();
        let d = doc(PROPS, "app.name=secret\n");
        assert!(tools.hover(&d, Position::new(0, 4)).is_none());
        assert!(tools.inlay_hints(&d, all()).is_empty());
        assert!(h.requests().is_empty() && h.process_runs().is_empty());
    }
    #[test]
    fn newly_opened_nested_project_retires_cached_metadata_positive_facts() {
        let host = MockHost::install();
        host.add_file(
            "nested/src/main/resources/META-INF/additional-spring-configuration-metadata.json",
            JSON,
        );
        let mut tools = BootTools::default();
        tools.activate_live();
        let current = doc(PROPS, "app.greeting=hello\n");
        assert!(tools.hover(&current, Position::new(0, 4)).is_some());
        assert!(!tools.inlay_hints(&current, all()).is_empty());
        host.add_project("/project/nested");
        assert!(
            tools.hover(&current, Position::new(0, 4)).is_none(),
            "nested metadata must not remain an outer positive fact"
        );
        assert!(tools.inlay_hints(&current, all()).is_empty());
        assert!(host.process_runs().is_empty() && host.requests().is_empty());
    }
    #[test]
    fn current_ownership_preserves_outer_metadata_and_bounds_unknown_scope() {
        let (host, mut tools) = setup();
        host.add_file(
            "nested/src/main/resources/META-INF/additional-spring-configuration-metadata.json",
            &JSON.replace("app.greeting", "nested.only"),
        );
        tools.live.files.reset();
        tools.activate_live();
        host.add_project("/project/nested");
        let current = doc(PROPS, "app.greeting=hello\nnested.only=x\n");
        assert!(tools.hover(&current, Position::new(0, 4)).is_some());
        assert!(tools.hover(&current, Position::new(1, 4)).is_none());
        assert_eq!(tools.inlay_hints(&current, all()).len(), 1);
        for path in [
            "/project/../other/application.properties",
            "/project//application.properties",
        ] {
            let bad = doc(path, "app.greeting=x\n");
            assert!(tools.hover(&bad, Position::new(0, 4)).is_none());
            assert!(tools.inlay_hints(&bad, all()).is_empty());
        }
        for i in 0..65 {
            host.add_project(&format!("/other-{i}"));
        }
        assert!(tools.hover(&current, Position::new(0, 4)).is_none());
        assert!(tools.inlay_hints(&current, all()).is_empty());
        assert!(host.process_runs().is_empty() && host.requests().is_empty());
    }
    #[test]
    fn current_unsaved_config_and_java_fixes_clear_results_without_marker_publication() {
        let (host, mut tools) = setup();
        host.open_editor(PROPS, "properties", "server.port=oops\n", &[]);
        host.refresh_diagnostics(&mut tools);
        assert_eq!(
            host.provider_diagnostics()[PROPS][0].code.as_deref(),
            Some("invalid-property-value")
        );
        host.open_editor(PROPS, "properties", "server.port=8080\n", &[]);
        host.type_in_editor("# unsaved version two\n");
        host.refresh_diagnostics(&mut tools);
        assert_eq!(host.editor_version(), 2);
        assert!(host.provider_diagnostics().is_empty());
        tools.line_markers(&doc(PROPS, "server.port=oops\n"));
        assert!(host.diagnostics().is_empty());
        let java = doc(
            "/project/src/main/java/App.java",
            "class App { @Scheduled(cron=\"* * * * *\") void go() {} }",
        );
        assert!(!tools.diagnostics(&java).is_empty());
        assert!(
            tools
                .diagnostics(&Document {
                    text: java.text.replace("* * * * *", "0 * * * * *"),
                    version: 2,
                    ..java
                })
                .is_empty()
        );
    }
    #[test]
    fn cold_requests_use_local_metadata_without_discovering_project_files() {
        let host = MockHost::install();
        host.add_file("src/main/java/Custom.java", "import org.springframework.boot.context.properties.ConfigurationProperties; @ConfigurationProperties(prefix=\"app\") class Custom {String mode;}");
        let mut tools = BootTools::default();
        let current = doc(PROPS, "server.port=oops\nspring.datasource.urll=x\n");
        let found = tools.diagnostics(&current);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("invalid-property-value"));
        let index = tools
            .live
            .files
            .for_cached_document(PROPS, &current.text)
            .unwrap();
        assert!(!index.complete());
        assert_eq!(index.files().len(), 1);
        assert!(index.files().contains_key(PROPS));
        assert!(
            host.requests().is_empty()
                && host.process_runs().is_empty()
                && host.diagnostics().is_empty()
        );
    }
    #[test]
    fn hover_and_range_limited_inlays_show_declared_metadata_without_runtime_values() {
        let (_host, mut tools) = setup();
        let current = doc(
            PROPS,
            "server.port=99991\napp.greeting=private-value\nunknown.key=x\n",
        );
        let hover = tools.hover(&current, Position::new(0, 3)).unwrap();
        assert!(hover.markdown.contains("8080"));
        assert!(!hover.markdown.contains("99991"));
        let greeting = tools.hover(&current, Position::new(1, 5)).unwrap();
        assert!(greeting.markdown.contains("Greeting") && greeting.markdown.contains("hello"));
        assert!(!greeting.markdown.contains("private-value"));
        let hints = tools.inlay_hints(
            &current,
            Range::new(Position::new(1, 0), Position::new(1, 100)),
        );
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, ": String");
        assert!(tools.hover(&current, Position::new(2, 3)).is_none());
    }
    #[test]
    fn unsaved_metadata_snapshots_and_save_events_refresh_established_facts() {
        let (_host, mut tools) = setup();
        let changed = JSON.replace("app.greeting", "app.updated");
        tools.diagnostics(&doc(META, &changed));
        assert!(
            tools
                .hover(&doc(PROPS, "app.updated=x\n"), Position::new(0, 4))
                .is_some()
        );
        assert!(
            tools
                .hover(&doc(PROPS, "app.greeting=x\n"), Position::new(0, 4))
                .is_none()
        );
        tools.live_events(&[Event::FileSaved(META.into())]);
        assert!(
            tools
                .hover(&doc(PROPS, "app.updated=x\n"), Position::new(0, 4))
                .is_none()
        );
        assert!(
            tools
                .hover(&doc(PROPS, "app.greeting=x\n"), Position::new(0, 4))
                .is_some()
        );
    }
    #[test]
    fn ambiguous_metadata_and_dynamic_unknown_keys_have_no_hints() {
        let (host, mut tools) = setup();
        host.add_file(
            "src/main/resources/META-INF/spring-configuration-metadata.json",
            &JSON.replace("java.lang.String", "java.lang.Integer"),
        );
        tools.live.files.reset();
        tools.activate_live();
        let current = doc(PROPS, "app.greeting=x\n${dynamic}=x\n");
        assert!(tools.hover(&current, Position::new(0, 3)).is_none());
        assert!(tools.inlay_hints(&current, all()).is_empty());
        assert!(
            !tools
                .diagnostics(&doc(PROPS, "app.greetingz=x\n"))
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("unknown-property"))
        );
    }
    #[test]
    fn project_switch_oversized_inputs_and_denied_events_do_not_leak_absence_claims() {
        let (host, mut tools) = setup();
        host.set_project_root(Some("/another"));
        assert!(
            tools
                .hover(
                    &doc("/another/application.properties", "app.greeting=x\n"),
                    Position::new(0, 3)
                )
                .is_none()
        );
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut tools = BootTools::default();
        tools.activate_live();
        assert!(!tools.live.watching);
        assert!(
            !tools
                .diagnostics(&doc(PROPS, "spring.datasource.urll=x\n"))
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("unknown-property"))
        );
        assert!(
            !tools
                .diagnostics(&doc(PROPS, "server.port=oops\n"))
                .is_empty()
        );
        host.add_file("src/main/java/Large.java", &"x".repeat(MAX_TEXT + 1));
        tools.live.files.reset();
        tools.activate_live();
        assert!(
            tools
                .diagnostics(&doc(PROPS, &"x".repeat(MAX_TEXT + 1)))
                .is_empty()
        );
        let many = (0..513)
            .map(|n| format!("app.key{n}=x\n"))
            .collect::<String>();
        assert!(tools.diagnostics(&doc(PROPS, &many)).is_empty());
    }
    #[test]
    fn imported_config_recognition_changes_with_current_standard_file_snapshot() {
        let (host, mut tools) = setup();
        host.add_file(
            "src/main/resources/application.properties",
            "spring.config.import=optional:classpath:extra.yml\n",
        );
        tools.live.files.reset();
        tools.activate_live();
        let extra = doc(
            "/project/src/main/resources/extra.yml",
            "server:\n  port: oops\n",
        );
        assert!(!tools.diagnostics(&extra).is_empty());
        tools.diagnostics(&doc(
            PROPS,
            "spring.config.import=optional:classpath:other.yml\n",
        ));
        assert!(tools.diagnostics(&extra).is_empty());
        assert!(
            !tools
                .diagnostics(&doc(
                    "/project/src/main/resources/other.yml",
                    "server:\n  port: oops\n"
                ))
                .is_empty()
        );
    }
    #[test]
    fn metadata_json_checks_remain_current_text_and_unicode_ranges_use_utf16() {
        let (_host, mut tools) = setup();
        let invalid = doc(
            META,
            r#"{"properties":[{"name":"app.bad","type":"java.lang.Boolean","description":"Boolean setting","defaultValue":"not-boolean"}]}"#,
        );
        assert!(!tools.diagnostics(&invalid).is_empty());
        assert!(
            tools
                .diagnostics(&Document {
                    text: invalid.text.replace("not-boolean", "true"),
                    version: 2,
                    ..invalid
                })
                .is_empty()
        );
        let json = JSON.replace("app.greeting", "app.🦀");
        tools.diagnostics(&doc(META, &json));
        let current = doc(PROPS, "app.🦀=x\n");
        let hints = tools.inlay_hints(&current, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].position, position(&current.text, "app.🦀".len()));
        assert_eq!(hints[0].position.character, 6);
    }

    #[test]
    fn incomplete_metadata_does_not_claim_other_declarations_are_missing() {
        let host = MockHost::install();
        let mut tools = BootTools::default();
        let metadata = doc(
            META,
            r#"{"hints":[{"name":"app.from-unread-file","values":[]}],"properties":[{"name":"app.old","type":"String","description":"old","deprecation":{"replacement":"app.from-unread-file"}}]}"#,
        );
        let found = tools.diagnostics(&metadata);
        assert!(!found.iter().any(|finding| matches!(
            finding.code.as_deref(),
            Some("unknown-hint-property" | "unknown-metadata-replacement")
        )));
        host.add_file(
            "src/main/java/Dynamic.java",
            r#"import org.springframework.boot.context.properties.ConfigurationProperties; @ConfigurationProperties(prefix="${dynamic.prefix}") class Dynamic {String value;}"#,
        );
        tools.activate_live();
        assert!(
            !tools
                .diagnostics(&doc(PROPS, "spring.datasource.urll=x\n"))
                .iter()
                .any(|finding| finding.code.as_deref() == Some("unknown-property"))
        );
    }
    #[test]
    fn read_error_does_not_reset_live_cache() {
        let (host, mut tools) = setup();
        let current = doc(PROPS, "app.greeting=hello\n");
        assert!(tools.hover(&current, Position::new(0, 4)).is_some());
        host.add_file_bytes("src/main/java/Binary.java", &[0xff, 0xfe, 0x00]);
        tools.live_events(&[Event::FileSaved("/project/src/main/java/Binary.java".into())]);
        assert!(tools.hover(&current, Position::new(0, 4)).is_some());
    }
    #[test]
    fn snapshot_is_reused_until_the_index_changes() {
        let (host, mut tools) = setup();
        let current = doc(PROPS, "app.greeting=hello\n");
        let first = tools.live_snapshot(&current).unwrap();
        let second = tools.live_snapshot(&current).unwrap();
        assert!(Arc::ptr_eq(&first.shared, &second.shared));
        host.add_file("src/main/resources/META-INF/other-spring-configuration-metadata.json", JSON);
        tools.live_events(&[Event::FileSaved(
            "/project/src/main/resources/META-INF/other-spring-configuration-metadata.json".into(),
        )]);
        let third = tools.live_snapshot(&current).unwrap();
        assert!(!Arc::ptr_eq(&first.shared, &third.shared));
    }
    #[test]
    fn kotlin_config_props_suppresses_unknown_key() {
        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "app.name=x\n");
        let mut control = BootTools::default();
        control.activate_live();
        assert!(control.live_snapshot(&doc(PROPS, "app.name=x\n")).unwrap().complete);
        host.add_file(
            "src/main/kotlin/App.kt",
            "import org.springframework.boot.context.properties.ConfigurationProperties\n@ConfigurationProperties(prefix = \"app\")\nclass App(val name: String)\n",
        );
        let mut tools = BootTools::default();
        tools.activate_live();
        let current = doc(PROPS, "app.name=x\n");
        let shared = tools.live_snapshot(&current).unwrap();
        assert!(!shared.complete);
    }
    #[test]
    fn drive_letter_roots_are_inventoried() {
        let host = MockHost::install();
        host.add_project("C:/work/app");
        let roots = current_roots().unwrap();
        assert!(roots.iter().any(|root| root == "C:/work/app"));
        assert_eq!(
            spring_text::scope::current_owner("c:/work/app/src/A.java", &["C:/work/app".to_owned()]).as_deref(),
            Some("C:/work/app")
        );
    }
}
