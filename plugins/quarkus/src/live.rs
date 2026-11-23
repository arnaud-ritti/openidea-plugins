//! Current editor requests use cached facts only. Discovery stays in activation/events.
use super::*;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::inlay_hints::{InlayHint, InlayHints, InlayKind};
use openidea_plugin::{Event, EventKind, LogLevel};
use spring_text::metadata::Meta;
use std::time::{Duration, Instant};

pub(crate) const MODULE_TTL: Duration = Duration::from_secs(30);
#[derive(Clone)]
pub(crate) struct ModuleProof {
    pub(super) found: bool,
    pub(super) at: Instant,
}
pub(crate) const MAX_TEXT: usize = 128 << 10;
const WATCH: &[&str] = &[
    "**/*.java",
    "**/application*.properties",
    "**/application*.yaml",
    "**/application*.yml",
    "**/microprofile-config.properties",
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/.env",
    "**/gradle/libs.versions.toml",
    "**/settings.gradle",
    "**/settings.gradle.kts",
    "**/src/main/resources/templates/**",
];
#[derive(Clone)]
pub(crate) struct ConfigInfo {
    pub locations: Vec<crate::navigation::Target>,
    pub keys: Vec<String>,
    pub complete: bool,
}
#[derive(Default)]
pub(crate) struct State {
    pub(super) watching: bool,
    pub(super) removed_builds: BTreeSet<String>,
    pub(super) build_events_lost: bool,
    env: BTreeMap<String, Option<BTreeSet<String>>>,
}

fn is_build_input(path: &str) -> bool {
    BUILD_FILES.iter().chain(["gradle/libs.versions.toml", "settings.gradle", "settings.gradle.kts"].iter()).any(|file| path.ends_with(&format!("/{file}")))
}

impl QuarkusTools {
    pub(crate) fn activate_live(&mut self) {
        self.live.watching = host::subscribe_paths("files-changed", WATCH).is_ok();
        let _ = host::subscribe(EventKind::FileOpened);
        let _ = host::subscribe(EventKind::FileSaved);
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for project in host::projects().unwrap_or_default() {
            if Instant::now() >= deadline {
                break;
            }
            let Ok(root) = project.root() else {
                continue;
            };
            self.warm_live(&format!("{root}/.quarkus-cache"), None, deadline);
        }
    }
    pub(crate) fn warm_live(&mut self, path: &str, text: Option<&str>, deadline: Instant) {
        if Instant::now() >= deadline {
            return;
        }
        let Some(project) = project_of(path) else {
            return;
        };
        let Ok(root) = project.root() else {
            return;
        };
        if !self.uses_quarkus(path) {
            return;
        }
        self.warm_qute(path);
        if let Some(text) = text {
            self.sources.update_existing(&project, path, Some(text));
            self.configs.update_existing(&project, path, Some(text));
        }
        let budget = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(200));
        if !budget.is_zero() {
            self.sources.get(&project, budget);
        }
        let budget = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(200));
        if !budget.is_zero() {
            self.configs.get(&project, budget);
        }
        if let Some(text) = text {
            self.sources.update_existing(&project, path, Some(text));
            self.configs.update_existing(&project, path, Some(text));
        }
        let env_path = format!("{}/.env", root.trim_end_matches('/'));
        let env = if !source_owned(&root, &env_path) { None } else { match project.read_file(&env_path) {
            Ok(text) if text.len() <= MAX_TEXT => Some(env_keys(&text).collect()),
            Err(Error::NotFound(_)) => Some(BTreeSet::new()),
            _ => None,
        } };
        self.live.env.insert(root, env);
    }
    pub(crate) fn live_events(&mut self, events: &[Event]) {
        self.live_events_until(events, Instant::now() + spring_text::index::COMMAND_BUDGET);
    }
    fn live_events_until(&mut self, events: &[Event], deadline: Instant) {
        // Invalidation is mandatory even when later discovery exhausts its budget.
        self.invalidate_qute_navigation(events);
        self.observe_build_changes(events);
        for event in events {
            match event {
                Event::FileOpened(p) | Event::FileSaved(p) => self.jars.invalidate(p),
                Event::FilesChanged(b) => {
                    for p in b.created.iter().chain(&b.modified).chain(&b.removed) {
                        self.jars.invalidate(p);
                    }
                }
                _ => {}
            }
            let paths: Vec<_> = match event {
                Event::FileOpened(path) | Event::FileSaved(path) => vec![(path, false)],
                Event::FilesChanged(files) => {
                    files.removed.iter().map(|p| (p, true)).chain(files.created.iter().chain(&files.modified).map(|p| (p, false))).collect()
                }
                _ => Vec::new(),
            };
            for (path, removed) in paths {
                if Instant::now() >= deadline {
                    return;
                }
                if !WATCH.iter().any(|glob| spring_text::index::glob_matches(glob, path)) {
                    continue;
                }
                if is_build_input(path) {
                    self.modules.clear();
                    self.qute.modules.clear();
                }
                let Some(project) = project_of(path) else {
                    continue;
                };
                if removed {
                    self.sources.update_existing(&project, path, None);
                    self.configs.update_existing(&project, path, None);
                    self.warm_live(path, None, deadline);
                    continue;
                }
                match project.read_file(path) {
                    Ok(text) => self.warm_live(path, Some(&text), deadline),
                    Err(error) => {
                        self.sources.reset();
                        self.configs.reset();
                        self.modules.clear();
                        self.live.env.clear();
                        self.qute.modules.clear();
                        host::log(LogLevel::Debug, &format!("Quarkus cached facts invalidated: {error}"));
                    }
                }
            }
        }
    }
    fn observe_build_changes(&mut self, events: &[Event]) {
        let build = |path: &str| is_build_input(path);
        for event in events {
            let (removed, changed): (Vec<&String>, Vec<&String>) = match event {
                Event::FilesChanged(batch) => (
                    batch.removed.iter().filter(|p| build(p)).collect(),
                    batch.created.iter().chain(&batch.modified).filter(|p| build(p)).collect(),
                ),
                Event::FileSaved(path) if build(path) => (vec![], vec![path]),
                Event::FileOpened(path) if build(path) => {
                    self.modules.clear();
                    self.qute.modules.clear();
                    continue;
                }
                _ => continue,
            };
            if removed.is_empty() && changed.is_empty() {
                continue;
            }
            self.modules.clear();
            self.qute.modules.clear();
            for path in &changed {
                if !removed.contains(path) {
                    self.live.removed_builds.remove(*path);
                }
            }
            for path in removed {
                if path.len() > 4096 || self.live.removed_builds.len() >= 1024 {
                    self.live.build_events_lost = true;
                } else {
                    self.live.removed_builds.insert(path.clone());
                }
            }
        }
    }
    pub(super) fn cached_module(&self, doc: &Document) -> Option<String> {
        if doc.text.len() > MAX_TEXT {
            return None;
        }
        let root = project_of(&doc.path)?.root().ok()?;
        if root.is_empty() || !doc.path.starts_with(&format!("{}/", root.trim_end_matches('/'))) {
            return None;
        }
        let dir = doc.path.rsplit_once('/')?.0;
        self.modules
            .iter()
            .filter(|((owner, path), _)| owner == &root && (path == dir || dir.starts_with(&format!("{path}/"))))
            .max_by_key(|((_, path), _)| path.len())
            .and_then(|(_, proof)| (proof.found && proof.at.elapsed() < MODULE_TTL && !self.live.build_events_lost).then_some(root))
    }
    fn cached_catalog(&mut self, doc: &Document) -> (Catalog, bool, BTreeSet<String>) {
        let mut catalog = Catalog::default();
        let mut complete = false;
        let mut properties: BTreeMap<String, spring_text::metadata::Prop> = BTreeMap::new();
        let mut ambiguous = BTreeSet::new();
        let mut hints = Vec::new();
        let Some(root) = project_of(&doc.path).and_then(|project| project.root().ok()) else { return (catalog, false, ambiguous); };
        if let Some(index) = self.sources.for_cached_document(&doc.path, &doc.text) {
            complete = index.complete() && index.iter().all(|(path, info)| source_owned(&root, path) && info.complete);
            for meta in index.iter().filter(|(path, _)| source_owned(&root, path)).filter_map(|(_, info)| info.meta.as_ref()) {
                for prop in &meta.props {
                    let key = config::canonical(&prop.name);
                    if properties.get(&key).is_some_and(|known| known != prop) {
                        ambiguous.insert(key.clone());
                    }
                    properties.entry(key).or_insert_with(|| prop.clone());
                }
                hints.extend(meta.hints.clone());
            }
        }
        let props = properties.into_iter().filter(|(key, _)| !ambiguous.contains(key)).map(|(_, prop)| prop).collect();
        catalog.add(&Meta { props, hints });
        (catalog, complete && ambiguous.is_empty(), ambiguous)
    }
    fn cached_configured(&mut self, doc: &Document, root: &str) -> Option<BTreeSet<String>> {
        if !self.live.watching {
            return None;
        }
        let index = self.configs.for_cached_document(&doc.path, &doc.text)?;
        if !index.complete() || !index.iter().all(|(path, info)| source_owned(root, path) && info.complete) {
            return None;
        }
        let mut keys: BTreeSet<_> = index.iter().flat_map(|(_, info)| info.keys.iter().cloned()).collect();
        keys.extend(self.live.env.get(root)?.as_ref()?.iter().cloned());
        Some(keys)
    }
    fn cached_jaxrs(&mut self, doc: &Document) -> Option<crate::jaxrs::Facts> {
        if !self.live.watching {
            return None;
        }
        let root = project_of(&doc.path)?.root().ok()?;
        let index = self.sources.for_cached_document(&doc.path, &doc.text)?;
        if !index.complete() || !index.iter().all(|(path, info)| source_owned(&root, path) && info.complete) {
            return None;
        }
        Some(crate::jaxrs::merge(index.iter().filter(|(path, _)| *path != doc.path).map(|(_, info)| &info.jaxrs)))
    }
    fn cached_clients(&mut self, doc: &Document) -> Option<BTreeMap<String, bool>> {
        if !self.live.watching {
            return None;
        }
        let root = project_of(&doc.path)?.root().ok()?;
        let index = self.sources.for_cached_document(&doc.path, &doc.text)?;
        if !index.complete() || !index.iter().all(|(path, info)| source_owned(&root, path) && info.complete) {
            return None;
        }
        let mut clients: BTreeMap<String, Option<bool>> = BTreeMap::new();
        for (_, info) in index.iter() {
            for (name, registered) in &info.interfaces {
                let key = if info.package.is_empty() { name.clone() } else { format!("{}.{name}", info.package) };
                clients.entry(key).and_modify(|known| *known = None).or_insert(Some(*registered));
            }
        }
        Some(clients.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))).collect())
    }
}
impl DiagnosticsProvider for QuarkusTools {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if matches!(doc.language.as_str(), "html" | "plaintext" | "qute-html") { return self.qute_diagnostics(doc); }
        let Some(root) = self.cached_module(doc) else {
            return Vec::new();
        };
        let (project, complete, _) = self.cached_catalog(doc);
        let configured = if complete { self.cached_configured(doc, &root) } else { None };
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&project, &jars, bundled()]);
        let found = if doc.language == "java" && doc.path.ends_with(".java") {
            let clients = self.cached_clients(doc);
            let module = self.cached_jaxrs(doc);
            let ctx = Context {
                lookup: &lookup,
                configured: configured.as_ref(),
                rest_clients: clients.as_ref(),
                qualified_rest_clients: true,
                unassigned: host::setting_bool("quarkus.checkUnassigned").unwrap_or(false),
                annotations: host::setting_bool("quarkus.checkAnnotations").unwrap_or(true),
                health_metrics: host::setting_bool("quarkus.checkHealthMetrics").unwrap_or(true),
                module: module.as_ref(),
            };
            java_checks::check(&doc.text, &ctx).into_iter().map(|problem| problem.finding.diagnostic(&doc.text)).chain(self.qute_java_diagnostics(doc)).collect()
        } else if is_quarkus_config(&doc.path)
            && matches!(doc.language.as_str(), "properties" | "quarkus-properties" | "yaml")
            && config::keys(&doc.path, &doc.text).len() <= 512
        {
            problems(
                &doc.path,
                &doc.text,
                &lookup,
                complete && self.live.watching,
                if host::setting_bool("quarkus.checkRequired").unwrap_or(false) { configured.as_ref() } else { None },
            )
        } else {
            Vec::new()
        };
        found.into_iter().take(500).collect()
    }
}
fn metadata_markdown(key: &str, prop: &spring_text::metadata::Prop) -> String {
    fn safe(value: &str, max: usize) -> String {
        value.chars().take(max).collect::<String>().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('`', "\\`")
    }
    let mut text = format!("**{}**\n\nDeclared type: `{}`", safe(key, 256), safe(&prop.ty, 256));
    if !prop.description.is_empty() {
        text.push_str(&format!("\n\n{}", safe(&prop.description, 2000)));
    }
    if !prop.default.is_empty() {
        text.push_str(&format!("\n\nDeclared default: `{}`", safe(&prop.default, 256)));
    }
    if let Some(deprecated) = &prop.deprecated {
        text.push_str("\n\nDeprecated.");
        if !deprecated.replacement.is_empty() {
            text.push_str(&format!(" Replacement: `{}`", safe(&deprecated.replacement, 256)));
        }
    }
    text.push_str("\n\nDeclaration metadata only; runtime values are not resolved.");
    text
}
impl HoverProvider for QuarkusTools {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        self.cached_module(doc)?;
        let off = offset(&doc.text, at)?;
        let (project, _, ambiguous) = self.cached_catalog(doc);
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&project, &jars, bundled()]);
        let candidates: Vec<_> = if is_quarkus_config(&doc.path) {
            config::keys(&doc.path, &doc.text).into_iter().map(|key| (key.key, key.at, key.end)).collect()
        } else if doc.language == "java" {
            java::injections(&spring_text::java::parse(&doc.text), &doc.text)
                .into_iter()
                .map(|injection| (injection.key, injection.at, injection.end))
                .collect()
        } else {
            Vec::new()
        };
        let (key, start, end) = candidates.into_iter().find(|(_, start, end)| *start <= off && off <= *end)?;
        let normalized = checks::normalize(&lookup, strip_profile(&key));
        if ambiguous.contains(&config::canonical(&normalized)) || key.contains(['{', '}']) {
            return None;
        }
        let prop = lookup.prop(&normalized)?;
        if prop.open() {
            return None;
        }
        let mut content = metadata_markdown(&key, prop);
        if Lookup(vec![&project]).prop(&normalized).is_some() {
            content.push_str("\n\nMetadata origin: project-source current/cached declarations.");
        } else if Lookup(vec![&jars]).prop(&normalized).is_some() {
            content.push_str("\n\nMetadata origin: selected dependency JAR snapshot.");
            if let Some(provenance) = self.jars.provenance(&doc.path) {
                content.push_str("\n\n");
                content.push_str(&provenance.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('`', "\\`"));
            }
        } else {
            content.push_str("\n\nMetadata origin: bundled fallback snapshot. ");
            content.push_str(&bundled_origin().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('`', "\\`"));
            content.push_str(". Packaged dataset version, not an inferred project dependency version.");
        }
        Some(Hover::new(content).range(range(&doc.text, start, end)))
    }
}
impl InlayHints for QuarkusTools {
    fn inlay_hints(&mut self, doc: &Document, requested: Range) -> Vec<InlayHint> {
        if doc.language=="java" {
            if self.cached_module(doc).is_none() {return Vec::new();}
            return source_features::inlays(doc,requested);
        }
        if self.cached_module(doc).is_none() || !is_quarkus_config(&doc.path) || !matches!(doc.language.as_str(), "properties" | "quarkus-properties" | "yaml") {
            return Vec::new();
        }
        let (project, _, ambiguous) = self.cached_catalog(doc);
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&project, &jars, bundled()]);
        config::keys(&doc.path, &doc.text)
            .into_iter()
            .filter_map(|key| {
                let location = range(&doc.text, key.at, key.end);
                if location.end < requested.start || location.end > requested.end {
                    return None;
                }
                if key.key.contains(['{', '}']) {
                    return None;
                }
                let normalized = checks::normalize(&lookup, strip_profile(&key.key));
                if ambiguous.contains(&config::canonical(&normalized)) {
                    return None;
                }
                let prop = lookup.prop(&normalized)?;
                if prop.open() || prop.ty.is_empty() || prop.ty.len() > 128 || prop.ty.chars().any(char::is_control) {
                    return None;
                }
                Some(
                    InlayHint::new(location.end, format!(": {}", prop.ty))
                        .kind(InlayKind::Type)
                        .padding_left()
                        .tooltip("Declared type from bundled/cached Quarkus metadata; not a resolved runtime value."),
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
    const ROOT: &str = "/project";
    const PROPS: &str = "/project/src/main/resources/application.properties";
    const JAVA: &str = "/project/src/main/java/org/acme/App.java";
    fn doc(path: &str, text: &str) -> Document {
        Document {
            path: path.into(),
            language: if path.ends_with(".java") {
                "java"
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
    fn setup() -> (MockHost, QuarkusTools) {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        let mut tools = QuarkusTools::default();
        tools.activate_live();
        (host, tools)
    }
    fn all() -> Range {
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
    }
    #[test]
    fn expired_module_proof_does_not_refresh_from_provider_or_fresh_source_cache() {
        let (_host, mut tools) = setup();
        let current = doc(PROPS, "quarkus.http.port=oops\n");
        assert!(!tools.diagnostics(&current).is_empty());
        for proof in tools.modules.values_mut() {
            proof.at = Instant::now() - MODULE_TTL - Duration::from_secs(1);
        }
        assert!(tools.cached_module(&current).is_none());
        assert!(tools.diagnostics(&current).is_empty());
        assert!(tools.hover(&current, Position::new(0, 2)).is_none());
        assert!(tools.modules.values().all(|proof| proof.at.elapsed() > MODULE_TTL));
    }
    #[test]
    fn build_deletion_invalidates_before_expired_batch_budget_and_retained_source() {
        let (_host, mut tools) = setup();
        let current = doc(PROPS, "quarkus.http.port=oops\n");
        assert!(tools.cached_module(&current).is_some());
        let build = format!("{ROOT}/pom.xml");
        let events =
            [Event::FilesChanged(openidea_plugin::FilesChanged { created: vec![], modified: vec![], removed: vec![build.clone()] })];
        tools.live_events_until(&events, Instant::now() - Duration::from_secs(1));
        assert!(tools.modules.is_empty() && tools.qute.modules.is_empty());
        assert!(tools.cached_module(&current).is_none());
        // Mock disk/editor text deliberately remains readable. An explicit
        // deletion must dominate it until a new creation/save observation.
        tools.warm_live(PROPS, None, Instant::now() + Duration::from_secs(1));
        assert!(!tools.uses_quarkus(PROPS));
        assert!(tools.cached_module(&current).is_none());
        tools.live_events(&[Event::FilesChanged(openidea_plugin::FilesChanged {
            created: vec![build],
            modified: vec![],
            removed: vec![],
        })]);
        tools.warm_live(PROPS, None, Instant::now() + Duration::from_secs(1));
        assert!(tools.uses_quarkus(PROPS));
    }
    #[test]
    fn current_unsaved_fixes_clear_versioned_provider_results_without_marker_publication() {
        let (host, mut tools) = setup();
        host.open_editor(PROPS, "properties", "quarkus.http.port=oops\n", &[]);
        host.refresh_diagnostics(&mut tools);
        assert_eq!(host.provider_diagnostics()[PROPS][0].code.as_deref(), Some("invalid-value"));
        host.open_editor(PROPS, "properties", "quarkus.http.port=8080\n", &[]);
        host.type_in_editor("#unsaved version two\n");
        host.refresh_diagnostics(&mut tools);
        assert_eq!(host.editor_version(), 2);
        assert!(host.provider_diagnostics().is_empty());
        tools.line_markers(&doc(PROPS, "quarkus.http.port=oops\n"));
        assert!(host.diagnostics().is_empty());
        let broken = doc(JAVA, "import io.quarkus.scheduler.Scheduled; class G { @Scheduled(every = \"soon\") void go() {} }");
        assert_eq!(tools.diagnostics(&broken)[0].code.as_deref(), Some("invalid-every"));
        assert!(tools.diagnostics(&Document { text: broken.text.replace("soon", "10s"), version: 2, ..broken }).is_empty());
    }
    #[test]
    fn cold_known_module_diagnostics_do_not_discover_sources_or_config_files() {
        let host = MockHost::install();
        host.add_file("src/main/java/X.java", "import org.eclipse.microprofile.config.inject.ConfigProperty; class X { @ConfigProperty(name=\"app.x\") String x; }");
        let mut tools = QuarkusTools::default();
        tools.modules.insert((ROOT.into(), ROOT.into()), ModuleProof { found: true, at: Instant::now() });
        let current = doc(PROPS, "quarkus.http.port=oops\nquarkus.http.portt=1\n");
        let diagnostics = tools.diagnostics(&current);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.as_deref(), Some("invalid-value"));
        let source = tools.sources.for_cached_document(PROPS, &current.text).unwrap();
        assert!(!source.complete());
        assert!(source.files().is_empty());
        assert!(host.requests().is_empty() && host.process_runs().is_empty() && host.diagnostics().is_empty());
    }
    #[test]
    fn module_build_events_and_project_switch_invalidate_local_proof() {
        let (host, mut tools) = setup();
        let current = doc(PROPS, "quarkus.http.port=oops\n");
        assert!(!tools.diagnostics(&current).is_empty());
        host.add_file("pom.xml", "<project/>");
        tools.live_events(&[Event::FileSaved("/project/pom.xml".into())]);
        assert!(tools.diagnostics(&current).is_empty());
        host.add_file("pom.xml", "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        tools.live_events(&[Event::FileSaved("/project/pom.xml".into())]);
        assert!(!tools.diagnostics(&current).is_empty());
        host.set_project_root(Some("/other"));
        assert!(tools.diagnostics(&doc("/other/application.properties", "quarkus.http.port=oops")).is_empty());
    }
    #[test]
    fn metadata_hover_and_type_inlays_use_declared_facts_not_current_runtime_values() {
        let (_host, mut tools) = setup();
        let current = doc(PROPS, "%dev.quarkus.http.port=99991\nunknown.runtime.key=secret\n");
        let hover = tools.hover(&current, Position::new(0, 10)).unwrap();
        assert!(hover.markdown.contains("8080") && hover.markdown.contains("int"));
        assert!(!hover.markdown.contains("99991"));
        let hints = tools.inlay_hints(&current, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, ": int");
        assert_eq!(hints[0].position, position(&current.text, "%dev.quarkus.http.port".len()));
        assert!(tools.hover(&current, Position::new(1, 5)).is_none());
        assert!(tools.inlay_hints(&current, Range::new(Position::new(1, 0), Position::new(2, 0))).is_empty());
        let java = doc(JAVA, "import org.eclipse.microprofile.config.inject.ConfigProperty; /* 🦀 */ class G { @ConfigProperty(name = \"quarkus.http.port\") int port; }");
        let start = java.text.find("quarkus.http.port").unwrap();
        let hover = tools.hover(&java, position(&java.text, start)).unwrap();
        assert_eq!(hover.range.unwrap().start, position(&java.text, start));
    }
    #[test]
    fn ambiguous_source_metadata_and_dynamic_keys_stay_quiet() {
        let (host, mut tools) = setup();
        host.add_file("src/main/java/A.java", "import org.eclipse.microprofile.config.inject.ConfigProperty; class A { @ConfigProperty(name=\"app.mode\") String mode; }");
        host.add_file("src/main/java/B.java", "import org.eclipse.microprofile.config.inject.ConfigProperty; class B { @ConfigProperty(name=\"app.mode\") int mode; }");
        tools.sources.reset();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        let current = doc(PROPS, "app.mode=value\n");
        assert!(tools.hover(&current, Position::new(0, 3)).is_none());
        assert!(tools.inlay_hints(&current, all()).is_empty());
        let java = doc(JAVA, "import org.eclipse.microprofile.config.inject.ConfigProperty; class G { @ConfigProperty(name = \"${runtime.key}\") String value; }");
        assert!(tools.hover(&java, Position::new(0, 41)).is_none());
        assert!(tools.inlay_hints(&doc(PROPS, "${runtime.key}=value\n"), all()).is_empty());
    }
    #[test]
    fn oversized_or_incomplete_facts_and_denied_events_suppress_absence_claims() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.add_file("pom.xml", "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        let mut tools = QuarkusTools::default();
        tools.activate_live();
        assert!(!tools.live.watching);
        assert!(tools.diagnostics(&doc(PROPS, "quarkus.http.portt=1\n")).is_empty());
        assert_eq!(tools.diagnostics(&doc(PROPS, "quarkus.http.port=oops\n"))[0].code.as_deref(), Some("invalid-value"));
        let (host, mut tools) = setup();
        host.add_file("src/main/java/Large.java", &"x".repeat(MAX_TEXT + 1));
        tools.sources.reset();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        assert!(tools.diagnostics(&doc(PROPS, "quarkus.http.portt=1\n")).is_empty());
        assert!(tools.diagnostics(&doc(PROPS, &"x".repeat(MAX_TEXT + 1))).is_empty());
        let many = (0..513).map(|n| format!("app.key{n}=x\n")).collect::<String>();
        assert!(tools.diagnostics(&doc(PROPS, &many)).is_empty());
    }
    #[test]
    fn cached_rest_client_ownership_respects_namespace_and_duplicate_fq_names() {
        let (host, mut tools) = setup();
        host.add_file("src/main/java/a/Api.java", "package a; import jakarta.ws.rs.Path; @Path(\"/a\") public interface Api {}");
        host.add_file("src/main/java/b/Api.java", "package b; import jakarta.ws.rs.Path; import org.eclipse.microprofile.rest.client.inject.RegisterRestClient; @RegisterRestClient @Path(\"/b\") public interface Api {}");
        tools.sources.reset();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        let current = doc(JAVA, "package owner; import a.Api; import org.eclipse.microprofile.rest.client.inject.RestClient; class G { @RestClient Api api; }");
        assert!(tools.diagnostics(&current).iter().any(|d| d.code.as_deref() == Some("rest-client-not-registered")));
        assert!(tools.diagnostics(&doc(JAVA, &current.text.replace("import a.Api", "import b.Api"))).is_empty());
        assert!(tools.diagnostics(&doc(JAVA, "package owner; class G { @RestClient other.Api api; }")).is_empty());
        host.add_file("src/main/java/another/Api.java", "package a; import jakarta.ws.rs.Path; @Path(\"/duplicate\") public interface Api {}");
        tools.sources.reset();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        assert!(tools.diagnostics(&current).is_empty());
    }
    #[test]
    fn cached_declared_unicode_keys_keep_utf16_inlay_ranges() {
        let (host, mut tools) = setup();
        host.add_file("src/main/java/A.java", "import org.eclipse.microprofile.config.inject.ConfigProperty; class A { @ConfigProperty(name=\"app.🦀\") String value; }");
        tools.sources.reset();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        let current = doc(PROPS, "app.🦀=x\n");
        let hints = tools.inlay_hints(&current, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].position.character, 6);
    }
    #[test]
    fn qute_template_deletion_invalidates_before_expired_discovery_budget() {
        use openidea_plugin::providers::Providers;
        let host=MockHost::install();let pom="<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-qute</artifactId></dependency></dependencies></project>";
        let java="import io.quarkus.qute.Template; import io.quarkus.qute.Location; class View { @Location(\"pages/home.html\") Template home; }";
        host.add_file("pom.xml",pom);host.add_file("src/main/java/View.java",java);host.add_file("src/main/resources/templates/pages/home.html","still readable old bytes");
        let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live("/project/src/main/java/View.java",None,Instant::now()+Duration::from_secs(1));
        let current=Document{path:"/project/src/main/java/View.java".into(),language:"java".into(),version:1,text:java.into()};let at=position(java,java.find("pages/home.html").unwrap()+3);
        assert_eq!(tools.definition(&current,at).len(),1);
        tools.live_events_until(&[Event::FilesChanged(openidea_plugin::FilesChanged{created:vec![],modified:vec![],removed:vec!["/project/src/main/resources/templates/pages/home.html".into()]})],Instant::now()-Duration::from_secs(1));
        assert!(tools.definition(&current,at).is_empty());
        tools.warm_live(&current.path,None,Instant::now()+Duration::from_secs(1));assert!(tools.definition(&current,at).is_empty());
    }

    #[test]
    fn no_warm_without_quarkus_dependency() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project/>");
        host.add_file("src/main/java/App.java", "class App {}");
        let mut tools = QuarkusTools::default();
        tools.activate_live();
        tools.warm_live(JAVA, Some("class App {}"), Instant::now() + Duration::from_secs(1));
        assert!(tools.live.env.is_empty());
    }
    #[test]
    fn env_refreshed_on_save() {
        let (host, mut tools) = setup();
        host.add_file(".env", "A=1\n");
        tools.live_events(&[Event::FileOpened(format!("{ROOT}/.env"))]);
        assert!(tools.live.env[ROOT].as_ref().unwrap().contains("a"));
        host.add_file(".env", "B=1\n");
        tools.live_events(&[Event::FileSaved(format!("{ROOT}/.env"))]);
        let keys = tools.live.env[ROOT].as_ref().unwrap();
        assert!(keys.contains("b") && !keys.contains("a"));
    }
    #[test]
    fn quarkus_dependency_added_later_starts_warming() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project/>");
        host.add_file(".env", "a=1\n");
        let mut tools = QuarkusTools::default();
        tools.activate_live();
        tools.warm_live(JAVA, None, Instant::now() + Duration::from_secs(1));
        assert!(tools.live.env.is_empty());
        host.add_file("pom.xml", "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        tools.live_events(&[Event::FileSaved(format!("{ROOT}/pom.xml"))]);
        assert!(tools.live.env.contains_key(ROOT));
    }
}
