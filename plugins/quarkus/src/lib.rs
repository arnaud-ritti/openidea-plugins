//! Quarkus for OpenIDEA: completion and checks for Quarkus configuration
//! files.

pub mod checks;
mod build_source;
mod console;
mod source_features;
mod indexer;
mod jar_archive;
mod jar_metadata;
mod java;
mod java_checks;
mod jaxrs;
mod live;
mod navigation;
mod qute;
mod starter;

use java::JavaInfo;
use java_checks::{Context, Fix};

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::config::{self, is_config_file};
use spring_text::index::{Indexes, PROVIDER_BUDGET, project_of};
use spring_text::metadata::{Catalog, Lookup, parse_tsv, properties_completion, strip_profile, yaml_completion};
use spring_text::range;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// Quarkus 3.40.1 and MicroProfile properties.
pub fn bundled() -> &'static Catalog {
    static BUNDLED: OnceLock<Catalog> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        let mut c = Catalog::default();
        c.add(&parse_tsv(include_str!("../data/quarkus-properties.tsv")));
        c
    })
}

pub(crate) fn bundled_origin() -> &'static str {
    include_str!("../data/quarkus-properties.tsv")
        .lines().next().and_then(|line| line.strip_prefix("# "))
        .unwrap_or("Bundled metadata dataset (version unknown)")
}

const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const PROFILES: [&str; 3] = ["%dev.", "%test.", "%prod."];

pub fn is_quarkus_config(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name == "microprofile-config.properties" || (is_config_file(path) && name.starts_with("application"))
}

pub struct QuarkusTools {
    sources: Indexes<JavaInfo>,
    configs: Indexes<live::ConfigInfo>,
    live: live::State,
    jars: jar_metadata::State,
    qute: qute::State,
    starter: starter::State,
    /// Whether a directory is inside a Quarkus module, by absolute path.
    modules: BTreeMap<(String, String), live::ModuleProof>,
}

impl Default for QuarkusTools {
    fn default() -> Self {
        Self {
            sources: Indexes::new(&["**/*.java"], java::java_info),
            configs: Indexes::new(
                &["**/application*.properties", "**/application*.yaml", "**/application*.yml", "**/microprofile-config.properties"],
                squashed_keys,
            ),
            live: live::State::default(),
            jars: jar_metadata::State::default(),
            qute: qute::State::default(),
            starter: starter::State::default(),
            modules: BTreeMap::new(),
        }
    }
}

fn squashed_keys(path: &str, text: &str) -> Option<live::ConfigInfo> {
    if text.len() > live::MAX_TEXT {
        return Some(live::ConfigInfo { keys: Vec::new(), locations: Vec::new(), complete: false });
    }
    let lookup = Lookup(vec![bundled()]);
    let keys: Vec<String> =
        config::keys(path, text).iter().map(|k| checks::squash(&checks::normalize(&lookup, strip_profile(&k.key)))).collect();
    let complete = keys.len() <= 512;
    Some(live::ConfigInfo { keys: if complete { keys } else { Vec::new() }, locations: navigation::config_targets(path, text), complete })
}

pub(crate) fn source_owned(root: &str, path: &str) -> bool {
    path.len() <= 4096 && !path.chars().any(char::is_control)
        && path.starts_with(&format!("{}/", root.trim_end_matches('/')))
        && !path.split('/').any(|part| matches!(part, "." | ".."))
        && project_of(path).and_then(|project| project.root().ok()).as_deref() == Some(root)
}
fn env_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| line.trim_start().split_once('=').map(|(key, _)| checks::squash(key.trim_start_matches("export "))))
}

impl QuarkusTools {
    /// Every key the project's other config files and `.env` set, `None` while the project is not fully read.
    fn java_problems(&mut self, doc: &Document) -> Vec<java_checks::Problem> {
        let unassigned = host::setting_bool("quarkus.checkUnassigned").unwrap_or(false);
        let annotations = host::setting_bool("quarkus.checkAnnotations").unwrap_or(true);
        if !(unassigned || annotations || host::setting_bool("quarkus.checkHealthMetrics").unwrap_or(true)) || !self.uses_quarkus(&doc.path)
        {
            return Vec::new();
        }
        let owner = project_of(&doc.path).and_then(|project| project.root().ok()).unwrap_or_default();
        let rest_clients = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)
            .filter(|index| index.complete() && index.iter().all(|(path, _)| source_owned(&owner, path))).map(|index| {
            let mut clients: BTreeMap<String, Option<bool>> = BTreeMap::new();
            for (_, info) in index.iter() { for (name, registered) in &info.interfaces {
                let name = if info.package.is_empty() { name.clone() } else { format!("{}.{name}", info.package) };
                clients.entry(name).and_modify(|known| *known = None).or_insert(Some(*registered));
            } }
            clients.into_iter().filter_map(|(name, registered)| registered.map(|value| (name, value))).collect()
        });
        let module = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)
            .filter(|index| index.complete() && index.iter().all(|(path, info)| source_owned(&owner, path) && info.complete))
            .map(|index| jaxrs::merge(index.iter().filter(|(path, _)| *path != doc.path).map(|(_, info)| &info.jaxrs)));
        let (project, _) = self.project(doc);
        let configured = self.keys_elsewhere(doc);
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&project, &jars, bundled()]);
        let health_metrics = host::setting_bool("quarkus.checkHealthMetrics").unwrap_or(true);
        let ctx = Context {
            lookup: &lookup,
            configured: configured.as_ref(),
            rest_clients: rest_clients.as_ref(),
            qualified_rest_clients: true,
            unassigned,
            annotations,
            health_metrics,
            module: module.as_ref(),
        };
        java_checks::check(&doc.text, &ctx)
    }

    fn unregistered_interface(&mut self, doc: &Document, name: &str) -> Option<(String, String)> {
        let owner = project_of(&doc.path)?.root().ok()?;
        let index = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        let matches: Vec<_> = index.iter().filter(|(path, _)| source_owned(&owner, path)).flat_map(|(path, info)| info.interfaces.iter().filter_map(move |(simple, registered)| {
            let qualified = if info.package.is_empty() { simple.clone() } else { format!("{}.{simple}", info.package) };
            (qualified == name).then_some((path.to_owned(), *registered))
        })).collect();
        let [(file, false)] = matches.as_slice() else { return None; };
        let file = file.clone();
        let project = project_of(&doc.path)?;
        let content = project.read_file(&file).ok()?;
        self.sources.update_existing(&project, &file, Some(&content));
        let current = java::java_info(&file, &content)?;
        if !current.complete || current.interfaces.iter().filter(|(simple, registered)| !registered
            && (if current.package.is_empty() { simple.clone() } else { format!("{}.{simple}", current.package) }) == name).count() != 1 {
            return None;
        }
        Some((file, content))
    }

    /// A literal properties target in the same conventional source module.
    fn properties_file_for(&mut self, doc: &Document) -> Option<String> {
        let owner = project_of(&doc.path)?.root().ok()?;
        let module = navigation::module(&doc.path)?;
        let index = self.configs.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        let shared = |p: &str| p.bytes().zip(doc.path.bytes()).take_while(|(a, b)| a == b).count();
        index.iter().map(|(p, _)| p).filter(|path| source_owned(&owner, path) && navigation::module(path).as_deref() == Some(module.as_str())).filter(|p| p.ends_with("/application.properties")).max_by_key(|p| shared(p)).map(str::to_owned)
    }

    fn keys_elsewhere(&mut self, doc: &Document) -> Option<BTreeSet<String>> {
        let root = project_of(&doc.path)?.root().ok()?;
        let index = self.configs.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        if !index.complete() || index.iter().any(|(path, _)| !source_owned(&root, path)) {
            return None;
        }
        let mut keys: BTreeSet<String> = index.iter().flat_map(|(_, info)| info.keys.iter().cloned()).collect();
        let root = project_of(&doc.path)?.root().ok()?;
        let env_path = format!("{}/.env", root.trim_end_matches('/'));
        if source_owned(&root, &env_path) && let Ok(env) = project_of(&doc.path)?.read_file(&env_path) {
            keys.extend(env_keys(&env));
        }
        Some(keys)
    }

    /// The project's own properties, and whether a whole pass over the project was read.
    fn project(&mut self, doc: &Document) -> (Catalog, bool) {
        let mut catalog = Catalog::default();
        let Some(owner) = project_of(&doc.path).and_then(|project| project.root().ok()) else { return (catalog, false); };
        let Some(index) = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else {
            return (catalog, false);
        };
        let owned = index.iter().all(|(path, _)| source_owned(&owner, path));
        for meta in index.iter().filter(|(path, _)| source_owned(&owner, path)).filter_map(|(_, info)| info.meta.as_ref()) {
            catalog.add(meta);
        }
        (catalog, index.complete() && owned)
    }

    /// Looks for literal Quarkus declarations in the build files of the file's directory and
    /// its parents, up to the project root: in a multi-module build only the
    /// root (or a parent) may name Quarkus.
    pub(crate) fn uses_quarkus(&mut self, path: &str) -> bool {
        let Some(project) = project_of(path) else {
            return false;
        };
        let Ok(root) = project.root() else {
            return false;
        };
        let root = root.trim_end_matches('/');
        let start = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        // File events invalidate cached build proofs; diagnostics only read the cache.
        if self.live.build_events_lost {
            return false;
        }
        if let Some(known) = self.modules.get(&(root.to_owned(), start.to_owned())).filter(|proof| proof.at.elapsed() < live::MODULE_TTL) {
            return known.found;
        }
        let mut dir = start;
        let found = loop {
            let named = BUILD_FILES.iter().any(|file| {
                let path = format!("{dir}/{file}");
                !self.live.removed_builds.contains(&path) && project.read_file(&path).is_ok_and(|text| {
                    if build_source::declares(file, &text) { return true; }
                    let catalog_path = format!("{dir}/gradle/libs.versions.toml");
                    if self.live.removed_builds.contains(&catalog_path) || !matches!(*file, "build.gradle" | "build.gradle.kts") { return false; }
                    // Custom catalogue declarations can replace libs or rename its accessor.
                    for settings in ["settings.gradle", "settings.gradle.kts"] {
                        let settings_path = format!("{dir}/{settings}");
                        match project.read_file(&settings_path) {
                            Ok(settings) => if settings.len() > 128 << 10 || spring_text::java::tokens(&settings).iter().any(|t| matches!(t.s, "versionCatalogs" | "defaultLibrariesExtensionName")) { return false; },
                            Err(Error::NotFound(_)) => {},
                            Err(_) => return false,
                        }
                    }
                    project.read_file(&catalog_path).is_ok_and(|catalog| build_source::declares_catalog(file, &text, &catalog))
                })
            });
            if named {
                break true;
            }
            if dir.len() <= root.len() {
                break false;
            }
            let Some((parent, _)) = dir.rsplit_once('/') else {
                break false;
            };
            dir = parent;
        };
        if self.modules.len() >= 64 {
            self.modules.clear();
        }
        self.modules.insert((root.to_owned(), start.to_owned()), live::ModuleProof { found, at: std::time::Instant::now() });
        found
    }
}

/// `%dev.`, `%test.` and `%prod.` at the start of a properties line.
fn profile_items(text: &str, at: usize) -> Vec<CompletionItem> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let typed = text[line_start..at].trim_start();
    if !typed.is_empty() && (!typed.starts_with('%') || typed.contains(['.', '=', ':'])) {
        return Vec::new();
    }
    let from = at - typed.len();
    PROFILES
        .iter()
        .map(|profile| {
            let mut item = CompletionItem::new(profile, CompletionKind::Keyword);
            item.replace = Some(range(text, from, at));
            item
        })
        .collect()
}

/// Completes a line that names an instance (`quarkus.datasource."users".jdbc.`) as the line without
/// the name, then writes the name back into the items.
fn named_items(text: &str, at: usize, lookup: &Lookup) -> Option<Vec<CompletionItem>> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let line = &text[line_start..line_end];
    let open = line.find(".\"")?;
    let close = open + 2 + line[open + 2..].find('"')?;
    let root = strip_profile(line[..open].trim_start());
    if !line[close + 1..].starts_with('.') || at <= line_start + close || !lookup.values("!named").iter().any(|r| r == root) {
        return None;
    }
    let (cut, name) = (line_start + open, &line[open..=close]);
    let stripped = format!("{}{}", &text[..cut], &text[cut + name.len()..]);
    let shifted = |position: Position| {
        let o = offset(&stripped, position)?;
        Some(range_point(text, if o >= cut { o + name.len() } else { o }))
    };
    let mut items = properties_completion(&stripped, at - name.len(), lookup);
    for item in &mut items {
        if let Some(replace) = item.replace {
            item.replace = Some(Range { start: shifted(replace.start)?, end: shifted(replace.end)? });
        }
        if item.label.starts_with(&format!("{root}.")) {
            for field in [&mut item.label, &mut item.insert_text] {
                *field = field.replacen(root, &format!("{root}{name}"), 1);
            }
            item.filter_text = item.filter_text.take().map(|f| f.replacen(root, &format!("{root}{name}"), 1));
        }
    }
    Some(items)
}

fn range_point(text: &str, at: usize) -> Position {
    openidea_plugin::text::position(text, at)
}

/// Expressions are checked only in `application.properties|yaml|yml`: profile files and
/// `microprofile-config.properties` lean on the base file, `.env`, the environment and `-D`.
fn is_base_file(path: &str) -> bool {
    matches!(path.rsplit(['/', '\\']).next(), Some("application.properties" | "application.yaml" | "application.yml"))
}

fn problems(path: &str, text: &str, lookup: &Lookup, complete: bool, elsewhere: Option<&BTreeSet<String>>) -> Vec<Diagnostic> {
    let keys = config::keys(path, text);
    let mut out = Vec::new();
    for k in &keys {
        let written = strip_profile(&k.key);
        if written.is_empty() {
            continue;
        }
        let base = &*checks::normalize(lookup, written);
        let range = range(text, k.at, k.end);
        if let Some(dep) = lookup.prop(base).and_then(|p| p.deprecated.as_ref()) {
            let message = match dep.replacement.as_str() {
                "" => format!("`{written}` is deprecated."),
                r => format!("`{written}` is deprecated. Use `{r}` instead."),
            };
            let mut d = Diagnostic::new(range, Severity::Warning, message).code("deprecated-property");
            d.deprecated = true;
            out.push(d);
        } else if complete
            // A YAML parent (`http:` with keys below) is checked through its children.
            && !keys.iter().any(|o| o.key.len() > k.key.len() && o.key.starts_with(&k.key) && matches!(o.key.as_bytes()[k.key.len()], b'.' | b'['))
            && let Some(group) = lookup.unknown(base)
        {
            let message = format!("`{written}` is not a known property of `{group}`.");
            out.push(Diagnostic::new(range, Severity::Warning, message).code("unknown-property"));
        }
    }
    let groups = checks::Groups {
        syntax: host::setting_bool("quarkus.checkSyntax").unwrap_or(true),
        values: host::setting_bool("quarkus.checkValues").unwrap_or(true),
        expressions: host::setting_bool("quarkus.checkExpressions").unwrap_or(true) && is_base_file(path),
    };
    let yaml = !path.ends_with(".properties");
    out.extend(checks::check(text, yaml, lookup, groups).iter().map(|f| f.diagnostic(text)));
    if let Some(elsewhere) = elsewhere {
        out.extend(checks::missing_required(&keys, elsewhere, lookup).iter().map(|f| f.diagnostic(text)));
    }
    out
}

impl Providers for QuarkusTools {
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<openidea_plugin::Location> {
        let qute=self.qute_definition(doc,at);
        if !qute.is_empty() {return qute;}
        self.source_definition(doc, at)
    }
    /// Editor diagnostics have their own versioned provider; markers link exact config keys from cached facts only.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.source_markers(doc)
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !doc.path.ends_with(".java") {
            return Vec::new();
        }
        let mut actions = Vec::new();
        for problem in self.java_problems(doc) {
            let finding = &problem.finding;
            let lines = range(&doc.text, finding.start, finding.end);
            if lines.start.line > selection.end.line || lines.end.line < selection.start.line {
                continue;
            }
            for fix in problem.fixes {
                let (title, edits) = match fix {
                    Fix::DefaultValue { at, value } => {
                        let edit = TextEdit { range: range(&doc.text, at, at), text: format!(", defaultValue = \"{value}\"") };
                        ("Add defaultValue".to_owned(), vec![(doc.path.clone(), vec![edit])])
                    }
                    Fix::Edits { title, edits } => {
                        let edits = edits.into_iter().map(|(a, b, text)| TextEdit { range: range(&doc.text, a, b), text }).collect();
                        (title, vec![(doc.path.clone(), edits)])
                    }
                    Fix::RegisterRestClient { name } => {
                        let Some((file, current)) = self.unregistered_interface(doc, &name) else {
                            continue;
                        };
                        let Some(start) = java::parse_interface_start(&current, &name) else {
                            continue;
                        };
                        let edits = java_checks::annotate_edits(&current, start, java_checks::REGISTER_REST_CLIENT, java_checks::REGISTER_REST_CLIENT)
                            .into_iter()
                            .map(|(a, b, text)| TextEdit { range: range(&current, a, b), text })
                            .collect();
                        (format!("Add @RegisterRestClient to {name}"), vec![(file, edits)])
                    }
                    Fix::AddKey { key } => {
                        if !navigation::literal_key(&key) { continue; }
                        let Some(file) = self.properties_file_for(doc) else {
                            continue;
                        };
                        let Ok(current) = project_of(&doc.path).map_or(Err(Error::NotFound(file.clone())), |p| p.read_file(&file)) else {
                            continue;
                        };
                        let Some(project) = project_of(&doc.path) else { continue; };
                        self.configs.update_existing(&project, &file, Some(&current));
                        let Some(fresh) = squashed_keys(&file, &current) else { continue; };
                        let normalized = checks::squash(&checks::normalize(&Lookup(vec![bundled()]), &key));
                        if !fresh.complete || fresh.keys.contains(&normalized) { continue; }
                        let separator = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
                        let edit = TextEdit { range: range(&current, current.len(), current.len()), text: format!("{separator}{key}=\n") };
                        (format!("Add `{key}` to application.properties"), vec![(file, vec![edit])])
                    }
                };
                actions.push(CodeAction { title, kind: Some("quickfix".into()), preferred: false, edits, data: None });
            }
        }
        actions
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let qute = self.qute_completion(doc, at);
        if !qute.is_empty() { return qute; }
        if doc.path.ends_with(".java") {
            let Some(at) = offset(&doc.text, at) else {
                return Vec::new();
            };
            if !self.uses_quarkus(&doc.path) || java::completion_slot(&doc.text, at).is_none() {
                return Vec::new();
            }
            let (project, _) = self.project(doc);
            let jars = self.jars.catalog(&doc.path);
            return java::key_items(&doc.text, at, &Lookup(vec![&project, &jars, bundled()]));
        }
        if !is_quarkus_config(&doc.path) || !self.uses_quarkus(&doc.path) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else {
            return Vec::new();
        };
        let (project, _) = self.project(doc);
        let jars = self.jars.catalog(&doc.path);
        let lookup = Lookup(vec![&project, &jars, bundled()]);
        if !doc.path.ends_with(".properties") {
            return yaml_completion(&doc.text, at, &lookup);
        }
        let mut items = profile_items(&doc.text, at);
        items.extend(named_items(&doc.text, at, &lookup).unwrap_or_else(|| properties_completion(&doc.text, at, &lookup)));
        items
    }
}

impl Plugin for QuarkusTools {
    fn activate(&mut self, _ctx: &openidea_plugin::Context) -> Result<(), Error> {
        console::apply();
        self.activate_live();
        Ok(())
    }
    fn handle_events(&mut self, events: &[openidea_plugin::Event], _ctx: &Invocation) {
        self.live_events(events);
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        if id==source_features::COMMAND { source_features::inspect(ctx,arg)?;return Ok(None); }
        if id == "prepare-local-starter" { return self.prepare_starter(ctx); }
        if id == "open-reviewed-starter" { return self.open_starter(ctx); }
        if id == "read-dependency-jar-metadata" {
            return self.jars.import(ctx);
        }
        if id != "open-code-quarkus" {
            return Err(Error::NotFound(format!("command {id}")));
        }

        host::open_url("https://code.quarkus.io")?;

        Ok(None)
    }
}

impl openidea_plugin::language_features::LanguageFeatures for QuarkusTools {
    fn folding(&mut self, doc: &Document) -> Vec<openidea_plugin::language_features::FoldRange> { self.qute_folding(doc) }
    fn code_lenses(&mut self,doc:&Document)->Vec<openidea_plugin::language_features::CodeLens> {
        if self.cached_module(doc).is_none() {return Vec::new();}
        let prefix = source_features::rest_prefix(&doc.path);
        source_features::lenses_with(doc, prefix.as_deref())
    }
    fn document_symbols(&mut self,doc:&Document)->Vec<openidea_plugin::language_features::DocumentSymbol> {
        if self.cached_module(doc).is_none() {return Vec::new();} source_features::symbols(doc)
    }
}
register_plugin!(QuarkusTools, providers, indexing, diagnostics, hover, inlay_hints, file_actions, language_features);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Range;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{CompletionItem, Document, Providers};
    use openidea_plugin::text::position;
    use spring_text::metadata::Lookup;
    use spring_text::range;

    const PROPS: &str = "/project/src/main/resources/application.properties";
    const YAML: &str = "/project/src/main/resources/application.yaml";
    const QUARKUS_POM: &str = "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>";

    fn quarkus_project() -> MockHost {
        let host = MockHost::install();
        host.add_file("pom.xml", QUARKUS_POM);
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        let language = if path.ends_with(".java") {
            "java"
        } else if path.ends_with(".properties") {
            "properties"
        } else {
            "yaml"
        };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn diagnose(host: &MockHost, tools: &mut QuarkusTools, doc: &Document) -> Vec<LineMarker> {
        tools.activate_live();
        tools.warm_live(&doc.path, Some(&doc.text), std::time::Instant::now() + std::time::Duration::from_secs(1));
        host.open_editor(&doc.path, &doc.language, &doc.text, &[]);
        host.refresh_diagnostics(tools);
        assert!(host.diagnostics().is_empty(), "providers must not manually publish duplicate diagnostics");
        Vec::new()
    }

    /// Completion at the end of `text`.
    fn complete(path: &str, text: &str) -> Vec<CompletionItem> {
        QuarkusTools::default().completion(&doc(path, text), position(text, text.len()), None)
    }

    fn find<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
        items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"))
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn completes_quarkus_keys_and_values() {
        let _host = quarkus_project();
        let items = complete(PROPS, "quarkus.http.po");
        let port = find(&items, "quarkus.http.port");
        assert_eq!(port.insert_text, "quarkus.http.port=");
        assert_eq!(port.detail.as_deref(), Some("int"));
        assert_eq!(port.documentation.as_deref(), Some("The HTTP port\n\nDefault: `8080`"));
        assert_eq!(labels(&complete(PROPS, "quarkus.http.ssl.client-auth=")), ["none", "request", "required"]);
        assert_eq!(labels(&complete(PROPS, "quarkus.http.cors.enabled=")), ["true", "false"]);
        assert_eq!(find(&complete(YAML, "quarkus:\n  http:\n    po"), "port").insert_text, "port: ");
        assert!(complete(PROPS, "# quarkus.").is_empty());
    }

    #[test]
    fn completes_project_config_keys() {
        let host = quarkus_project();
        host.add_file(
            "src/main/java/org/acme/GreetingResource.java",
            "import org.eclipse.microprofile.config.inject.ConfigProperty; class G { @ConfigProperty(name = \"greeting.message\", defaultValue = \"hello\") String message; }",
        );
        let items = complete(PROPS, "greeting.");
        let message = find(&items, "greeting.message");
        assert_eq!(message.detail.as_deref(), Some("String"));
        assert_eq!(message.documentation.as_deref(), Some("Default: `hello`"));
    }

    #[test]
    fn completes_profiles() {
        let _host = quarkus_project();
        assert_eq!(labels(&complete(PROPS, "%")), ["%dev.", "%test.", "%prod."]);
        assert_eq!(find(&complete(PROPS, ""), "%dev.").insert_text, "%dev.");
        let text = "server.x=1\n%dev.quarkus.http.po";
        assert_eq!(find(&complete(PROPS, text), "quarkus.http.port").replace, Some(range(text, 16, text.len())));
        assert_eq!(labels(&complete(PROPS, "%test.quarkus.http.ssl.client-auth=")), ["none", "request", "required"]);
        assert!(profile_items("%dev=", 5).is_empty());
        assert!(profile_items("%dev:", 5).is_empty());
    }

    #[test]
    fn sees_java_edits_without_waiting_for_the_index_ttl() {
        let host = quarkus_project();
        let java = "/project/src/main/java/org/acme/Mail.java";
        let before = "import org.eclipse.microprofile.config.inject.ConfigProperty; class M { @ConfigProperty(name = \"app.mail.from\") String from; }";
        let after = "import org.eclipse.microprofile.config.inject.ConfigProperty; class M { @ConfigProperty(name = \"app.mail.from\") String from; @ConfigProperty(name = \"app.mail.to\") String to; }";
        host.add_file("src/main/java/org/acme/Mail.java", before);
        let mut tools = QuarkusTools::default();
        let typed = "app.mail.";
        let items = tools.completion(&doc(PROPS, typed), position(typed, typed.len()), None);
        assert!(labels(&items).contains(&"app.mail.from"));
        assert!(!labels(&items).contains(&"app.mail.to"));
        assert!(diagnose(&host, &mut tools, &doc(java, after)).is_empty());
        let items = tools.completion(&doc(PROPS, typed), position(typed, typed.len()), None);
        assert!(labels(&items).contains(&"app.mail.to"));
        let text = "app.mail.to=x\n";
        diagnose(&host, &mut tools, &doc(PROPS, text));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        assert!(host.provider_diagnostics().get(java).is_none());
    }

    #[test]
    fn stays_quiet_outside_quarkus_modules() {
        let host = MockHost::install();
        host.add_file("pom.xml", "<project><parent><artifactId>spring-boot-starter-parent</artifactId></parent></project>");
        assert!(complete(PROPS, "quarkus.http.po").is_empty());
        let _host = quarkus_project();
        assert!(complete("/project/src/main/resources/messages.properties", "quarkus.").is_empty());
    }

    #[test]
    fn detects_quarkus_from_any_build_file_up_to_the_root() {
        let host = MockHost::install();
        host.add_file("pom.xml", QUARKUS_POM);
        host.add_file("service/pom.xml", "<project><artifactId>service</artifactId></project>");
        assert!(!complete("/project/service/src/main/resources/application.properties", "quarkus.http.po").is_empty());
        let host = MockHost::install();
        host.add_file("app/build.gradle.kts", "plugins { id(\"io.quarkus\") }");
        assert!(!complete("/project/app/src/main/resources/application.yaml", "quarkus:\n  http:\n    po").is_empty());
    }

    #[test]
    fn recognises_quarkus_config_files() {
        for p in [
            "/p/src/main/resources/application.properties",
            "/p/application-dev.properties",
            "/p/config/application.yml",
            "/p/src/main/resources/application.yaml",
            "/p/src/main/resources/META-INF/microprofile-config.properties",
        ] {
            assert!(is_quarkus_config(p), "{p}");
        }
        for p in ["/p/bootstrap.properties", "/p/messages.properties", "/p/applications.yml"] {
            assert!(!is_quarkus_config(p), "{p}");
        }
    }

    /// (code, text under the squiggle) of each problem.
    fn codes<'t, 'p>(text: &'t str, problems: &'p [Diagnostic]) -> Vec<(&'p str, &'t str)> {
        problems
            .iter()
            .map(|d| {
                let (a, b) = (offset(text, d.range.start).unwrap(), offset(text, d.range.end).unwrap());
                (d.code.as_deref().unwrap_or_default(), &text[a..b])
            })
            .collect()
    }

    #[test]
    fn warns_about_deprecated_and_unknown_keys() {
        let host = quarkus_project();
        let text = "quarkus.hibernate-orm.database.generation=update\nquarkus.http.portt=8080\n%dev.quarkus.http.prot=1\nquarkus.acme-extension.x=1\napp.greeting=hi\nquarkus.log.category.\"io.acme\".level=DEBUG\n";
        diagnose(&host, &mut QuarkusTools::default(), &doc(PROPS, text));
        let diagnostics = host.provider_diagnostics();
        let problems = &diagnostics[PROPS];
        assert_eq!(
            codes(text, problems),
            [
                ("deprecated-property", "quarkus.hibernate-orm.database.generation"),
                ("unknown-property", "quarkus.http.portt"),
                ("unknown-property", "%dev.quarkus.http.prot"),
            ]
        );
        assert!(problems[0].deprecated);
        assert_eq!(problems[0].message, "`quarkus.hibernate-orm.database.generation` is deprecated.");
        assert_eq!(problems[2].message, "`quarkus.http.prot` is not a known property of `quarkus.http`.");
    }

    #[test]
    fn checks_profiled_yaml_keys_as_their_base_key() {
        let host = quarkus_project();
        let text = "\"%dev\":\n  quarkus:\n    http:\n      portt: 1\n      port: 8081\n";
        diagnose(&host, &mut QuarkusTools::default(), &doc(YAML, text));
        assert_eq!(codes(text, &host.provider_diagnostics()[YAML]), [("unknown-property", "portt")]);
    }

    #[test]
    fn publishes_value_checks_with_the_deprecated_ones_and_clears_when_fixed() {
        let host = quarkus_project();
        let mut tools = QuarkusTools::default();
        let text = "quarkus.http.port=abc\nquarkus.http.portt=1\nquarkus.http.port=1\n";
        diagnose(&host, &mut tools, &doc(PROPS, text));
        assert_eq!(
            codes(text, &host.provider_diagnostics()[PROPS]).iter().map(|c| c.0).collect::<Vec<_>>(),
            ["unknown-property", "duplicate-key", "invalid-value"]
        );
        host.set_setting("quarkus.checkValues", "false");
        host.set_setting("quarkus.checkSyntax", "false");
        diagnose(&host, &mut tools, &doc(PROPS, text));
        assert_eq!(host.provider_diagnostics()[PROPS].len(), 1);
        diagnose(&host, &mut tools, &doc(PROPS, "quarkus.http.port=8080\n"));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        let yaml = "quarkus:\n  http:\n    port: abc\n";
        host.set_setting("quarkus.checkValues", "true");
        diagnose(&host, &mut tools, &doc(YAML, yaml));
        assert_eq!(codes(yaml, &host.provider_diagnostics()[YAML]), [("invalid-value", "abc")]);
    }

    #[test]
    fn expressions_are_checked_only_in_the_base_file() {
        let host = quarkus_project();
        let text = "app.greeting=hi\napp.a=${app.greting}\n";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc("/project/src/main/resources/application-dev.properties", text));
        assert!(host.provider_diagnostics().get("/project/src/main/resources/application-dev.properties").is_none_or(|d| d.is_empty()));
        diagnose(&host, &mut tools, &doc(PROPS, text));
        assert_eq!(codes(text, &host.provider_diagnostics()[PROPS]), [("undefined-expression", "${app.greting}")]);
    }

    #[test]
    fn publishes_nothing_outside_quarkus_modules() {
        let host = MockHost::install();
        diagnose(&host, &mut QuarkusTools::default(), &doc(PROPS, "quarkus.http.portt=1\n"));
        assert!(host.provider_diagnostics().is_empty());
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn reads_the_bundled_metadata() {
        let lookup = Lookup(vec![bundled()]);
        let port = lookup.prop("quarkus.http.port").unwrap();
        assert_eq!((port.ty.as_str(), port.default.as_str(), port.description.as_str()), ("int", "8080", "The HTTP port"));
        assert!(lookup.prop("quarkus.hibernate-orm.database.generation").unwrap().deprecated.is_some());
        assert_eq!(lookup.values("quarkus.http.ssl.client-auth"), ["none", "request", "required"]);
        assert_eq!(lookup.values("quarkus.http.cors.enabled"), ["true", "false"]);
        assert!(lookup.values("quarkus.log.level").contains(&"DEBUG".to_owned()));
        assert!(lookup.prop("mp.jwt.verify.publickey").is_some());
        assert!(lookup.prop("quarkus.datasource.\"datasource-name\".db-kind").is_none());
    }

    #[test]
    fn bundled_catalog_covers_top_extensions() {
        let lookup = Lookup(vec![bundled()]);
        let extensions = [
            ("quarkus-core", "quarkus.analytics.disabled"),
            ("quarkus-arc", "quarkus.arc.auto-inject-fields"),
            ("quarkus-vertx", "quarkus.vertx.cache-directory"),
            ("quarkus-vertx-http", "quarkus.http.accept-backlog"),
            ("quarkus-vertx-graphql", "quarkus.vertx-graphql.ui.always-include"),
            ("quarkus-undertow", "quarkus.servlet.buffer-size"),
            ("quarkus-web-dependency-locator", "quarkus.web-dependency-locator.app-root"),
            ("quarkus-info", "quarkus.info.build"),
            ("quarkus-tls-registry", "quarkus.tls.alpn"),
            ("quarkus-rest", "quarkus.rest.multipart.input-part.default-charset"),
            ("quarkus-rest-jackson", "quarkus.rest.jackson.optimization.enable-reflection-free-serializers"),
            ("quarkus-rest-client", "quarkus.rest-client.disable-smart-produces"),
            ("quarkus-rest-client-oidc-filter", "quarkus.rest-client-oidc-filter.client-name"),
            ("quarkus-rest-client-oidc-token-propagation", "quarkus.rest-client-oidc-token-propagation.client-name"),
            ("quarkus-rest-qute", "quarkus.rest.qute.template-instance-non-blocking-type"),
            ("quarkus-resteasy", "quarkus.resteasy.vertx.response-buffer-size"),
            ("quarkus-websockets-next", "quarkus.websockets-next.client.auto-ping-interval"),
            ("quarkus-jackson", "quarkus.jackson.accept-case-insensitive-enums"),
            ("quarkus-jaxb", "quarkus.jaxb.exclude-classes"),
            ("quarkus-grpc", "quarkus.generate-code.grpc.kotlin.generate"),
            ("quarkus-smallrye-graphql", "quarkus.smallrye-graphql.authorization-client-init-payload-name"),
            ("quarkus-smallrye-graphql-client", "quarkus.smallrye-graphql-client.enable-build-time-scanning"),
            ("quarkus-hibernate-orm", "quarkus.hibernate-orm.active"),
            ("quarkus-hibernate-envers", "quarkus.hibernate-envers.active"),
            ("quarkus-hibernate-search-orm-elasticsearch", "quarkus.hibernate-search-orm.active"),
            ("quarkus-hibernate-validator", "quarkus.hibernate-validator.expression-language.constraint-expression-feature-level"),
            ("quarkus-narayana-jta", "quarkus.transaction-manager.default-transaction-timeout"),
            ("quarkus-narayana-lra", "quarkus.lra.base-uri"),
            ("quarkus-datasource", "quarkus.datasource.active"),
            ("quarkus-agroal", "quarkus.datasource.dev-ui.allow-sql"),
            ("quarkus-reactive-datasource", "quarkus.datasource.reactive"),
            ("quarkus-reactive-pg-client", "quarkus.datasource.reactive.postgresql.pipelining-limit"),
            ("quarkus-reactive-mysql-client", "quarkus.datasource.reactive.mysql.authentication-plugin"),
            ("quarkus-reactive-mssql-client", "quarkus.datasource.reactive.mssql.packet-size"),
            ("quarkus-reactive-db2-client", "quarkus.datasource.reactive.db2.ssl"),
            ("quarkus-flyway", "quarkus.flyway.active"),
            ("quarkus-liquibase", "quarkus.liquibase.allow-duplicated-changeset-identifiers"),
            ("quarkus-liquibase-mongodb", "quarkus.liquibase-mongodb.change-log"),
            ("quarkus-mongodb-client", "quarkus.mongodb.active"),
            ("quarkus-redis-client", "quarkus.redis.active"),
            ("quarkus-infinispan-client", "quarkus.infinispan-client.auth-password"),
            ("quarkus-elasticsearch-rest-client", "quarkus.elasticsearch.api-key"),
            ("quarkus-cache", "quarkus.cache.caffeine.expire-after-access"),
            ("quarkus-smallrye-health", "quarkus.smallrye-health.additional.property"),
            ("quarkus-smallrye-openapi", "quarkus.smallrye-openapi.enable"),
            ("quarkus-smallrye-fault-tolerance", "quarkus.fault-tolerance.enabled"),
            ("quarkus-smallrye-jwt", "quarkus.smallrye-jwt.blocking-authentication"),
            ("quarkus-micrometer", "quarkus.micrometer.binder-enabled-default"),
            ("quarkus-opentelemetry", "quarkus.otel.attribute.count.limit"),
            ("quarkus-observability-devservices", "quarkus.observability.dev-resources"),
            ("quarkus-oidc", "quarkus.oidc.allow-token-introspection-cache"),
            ("quarkus-oidc-client", "quarkus.oidc-client.absolute-expires-in"),
            ("quarkus-oidc-db-token-state-manager", "quarkus.oidc.db-token-state-manager.access-token-column-size"),
            ("quarkus-oidc-redis-token-state-manager", "quarkus.oidc.redis-token-state-manager.enabled"),
            ("quarkus-keycloak-authorization", "quarkus.keycloak.connection-pool-size"),
            ("quarkus-security", "quarkus.security.auth.enabled-in-dev-mode"),
            ("quarkus-security-jpa", "quarkus.security-jpa.persistence-unit-name"),
            ("quarkus-security-webauthn", "quarkus.webauthn.attestation"),
            ("quarkus-elytron-security-properties-file", "quarkus.security.users.embedded.algorithm"),
            ("quarkus-elytron-security-jdbc", "quarkus.security.jdbc.enabled"),
            ("quarkus-elytron-security-oauth2", "quarkus.oauth2.ca-cert-file"),
            ("quarkus-messaging-kafka", "quarkus.messaging.kafka.enable-graceful-shutdown-in-dev-and-test-mode"),
            ("quarkus-kafka-client", "quarkus.kafka.devservices.container-env"),
            ("quarkus-kafka-streams", "quarkus.kafka-streams.application-id"),
            ("quarkus-messaging-amqp", "quarkus.amqp.devservices.container-env"),
            ("quarkus-messaging-rabbitmq", "quarkus.rabbitmq.credentials-provider"),
            ("quarkus-messaging-pulsar", "quarkus.messaging.pulsar.schema-autodetection.enabled"),
            ("quarkus-scheduler", "quarkus.scheduler.cron-type"),
            ("quarkus-quartz", "quarkus.quartz.batch-trigger-acquisition-fire-ahead-time-window"),
            ("quarkus-mailer", "quarkus.mailer.allow-rcpt-errors"),
            ("quarkus-qute", "quarkus.qute.alt-expr-syntax"),
            ("quarkus-amazon-lambda", "quarkus.lambda.expected-exceptions"),
            ("quarkus-google-cloud-functions", "quarkus.google-cloud-functions.function"),
            ("quarkus-container-image-jib", "quarkus.jib.always-cache-base-image"),
            ("quarkus-container-image-docker", "quarkus.docker.additional-args"),
            ("quarkus-container-image-buildpack", "quarkus.buildpack.builder-env"),
            ("quarkus-container-image-openshift", "quarkus.openshift.base-jvm-image"),
            ("quarkus-container-image-podman", "quarkus.podman.additional-args"),
            ("quarkus-kubernetes", "quarkus.knative.add-build-timestamp"),
            ("quarkus-kubernetes-config", "quarkus.kubernetes-config.config-maps"),
            ("quarkus-kubernetes-service-binding", "quarkus.kubernetes-service-binding.bind-as-files"),
            ("quarkus-logging-json", "quarkus.log.console.json"),
            ("quarkus-logging-gelf", "quarkus.log.handler.gelf.dynamic-mdc-field-types"),
            ("quarkus-picocli", "quarkus.picocli.top-command"),
            ("quarkus-jacoco", "quarkus.jacoco.aggregate-report-data"),
            ("quarkus-cyclonedx", "quarkus.cyclonedx.embedded.compress"),
        ];
        assert!(extensions.len() >= 80);
        let missing: Vec<&str> = extensions.iter().filter(|(_, key)| lookup.prop(key).is_none()).map(|(extension, _)| *extension).collect();
        assert!(missing.is_empty(), "no metadata for {missing:?}");
    }

    #[test]
    fn completes_below_a_named_instance_and_writes_the_name_back() {
        let _host = quarkus_project();
        let text = "%dev.quarkus.datasource.\"users\".jdbc.ur";
        let items = complete(PROPS, text);
        let url = find(&items, "quarkus.datasource.\"users\".jdbc.url");
        assert_eq!(url.insert_text, "quarkus.datasource.\"users\".jdbc.url=");
        assert_eq!(url.replace, Some(range(text, 5, text.len())));
        assert!(!labels(&items).iter().any(|l| l.starts_with("quarkus.http")));
        let text = "quarkus.datasource.\"users\".jdbc.enable-keep-alive=";
        let items = complete(PROPS, text);
        assert_eq!(labels(&items), ["true", "false"]);
        assert_eq!(items[0].replace, Some(range(text, text.len(), text.len())));
        assert!(complete(PROPS, "quarkus.log.category.\"io.acme\".le").is_empty());
    }

    #[test]
    fn checks_keys_of_a_named_instance_like_the_unnamed_ones() {
        let host = quarkus_project();
        let text = "quarkus.datasource.\"users\".jdbc.url=jdbc:h2:mem\nquarkus.datasource.\"users\".jdbc.urll=x\nquarkus.datasource.\"users\".jdbc.max-size=abc\nquarkus.log.category.\"io.acme\".level=DEBUG\nquarkus.datasource.\"a\".jdbc.url=\n";
        diagnose(&host, &mut QuarkusTools::default(), &doc(PROPS, text));
        assert_eq!(
            codes(text, &host.provider_diagnostics()[PROPS]),
            [("unknown-property", "quarkus.datasource.\"users\".jdbc.urll"), ("invalid-value", "abc"),]
        );
    }

    #[test]
    fn required_keys_are_reported_only_when_asked_and_when_nothing_else_sets_them() {
        let host = quarkus_project();
        let text = "quarkus.tls.key-store.jks.password=secret\n";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc(PROPS, text));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        host.set_setting("quarkus.checkRequired", "true");
        diagnose(&host, &mut tools, &doc(PROPS, text));
        let found = &host.provider_diagnostics()[PROPS];
        assert_eq!(codes(text, found), [("missing-required", "quarkus.tls.key-store.jks.password")]);
        assert_eq!(found[0].severity, Severity::Info);
        diagnose(&host, &mut tools, &doc(PROPS, "quarkus.http.port=1\n"));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn required_keys_set_in_another_file_or_the_environment_are_quiet() {
        let host = quarkus_project();
        host.set_setting("quarkus.checkRequired", "true");
        let text = "quarkus.tls.key-store.jks.password=secret\n";
        host.add_file("src/main/resources/application-prod.properties", "%prod.quarkus.tls.key-store.jks.path=/k.jks\n");
        diagnose(&host, &mut QuarkusTools::default(), &doc(PROPS, text));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        let host = quarkus_project();
        host.set_setting("quarkus.checkRequired", "true");
        host.add_file(".env", "QUARKUS_TLS_KEY_STORE_JKS_PATH=/k.jks\n");
        diagnose(&host, &mut QuarkusTools::default(), &doc(PROPS, text));
        assert!(host.provider_diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn reads_the_bundled_named_roots_and_required_keys() {
        let lookup = Lookup(vec![bundled()]);
        assert!(lookup.values("!named").contains(&"quarkus.datasource".to_owned()));
        assert!(lookup.values("!required").contains(&"quarkus.tls.key-store.jks.path".to_owned()));
        assert!(!lookup.values("!required").contains(&"quarkus.http.host".to_owned()));
    }

    const JAVA: &str = "/project/src/main/java/org/acme/Greeting.java";

    #[test]
    fn completes_config_property_names_and_mapping_prefixes() {
        let host = quarkus_project();
        host.add_file("src/main/java/org/acme/Other.java", "import org.eclipse.microprofile.config.inject.ConfigProperty; class O { @ConfigProperty(name = \"greeting.message\") String m; }");
        let text = "import org.eclipse.microprofile.config.inject.ConfigProperty; class G {\n  @ConfigProperty(name = \"quarkus.http.po";
        let items = complete(JAVA, text);
        let port = find(&items, "quarkus.http.port");
        assert_eq!((port.insert_text.as_str(), port.detail.as_deref()), ("quarkus.http.port", Some("int")));
        assert_eq!(port.replace, Some(range(text, text.len() - 15, text.len())));
        assert!(!labels(&items).contains(&"quarkus.http"));
        let text = "import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix = \"quarkus.http.s";
        assert!(labels(&complete(JAVA, text)).contains(&"quarkus.http.ssl"));
        assert!(complete(JAVA, "@ConfigProperty(name = \"quarkus.http.port\", defaultValue = \"qu").is_empty());
        assert!(complete(JAVA, "String s = \"quarkus.ht").is_empty());
        assert!(complete(JAVA, "@Named(name = \"quarkus.ht").is_empty());
        assert!(labels(&complete(JAVA, "import org.eclipse.microprofile.config.inject.ConfigProperty; class G { @ConfigProperty(name = \"greeting.")).contains(&"greeting.message"));
    }

    fn java_host() -> MockHost {
        let host = quarkus_project();
        host.add_file("src/main/resources/application.properties", "app.d=1\n");
        host.set_setting("quarkus.checkUnassigned", "true");
        host
    }

    #[test]
    fn publishes_unassigned_keys_and_scheduler_problems_for_java() {
        let host = java_host();
        let text = "import org.eclipse.microprofile.config.inject.ConfigProperty; import io.quarkus.scheduler.Scheduled; class G {\n  @ConfigProperty(name = \"app.a\") String a;\n  @ConfigProperty(name = \"app.d\") String d;\n  @Scheduled(cron = \"* * * * *\") void tick() {}\n}\n";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc(JAVA, text));
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert_eq!(codes(text, &host.provider_diagnostics()[JAVA]), [("unassigned-property", "app.a"), ("invalid-cron", "* * * * *")]);
        diagnose(&host, &mut tools, &doc(JAVA, "class G {}\n"));
        assert!(host.provider_diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn unassigned_check_is_off_by_default_and_quiet_without_config_files() {
        let host = quarkus_project();
        let text = "import org.eclipse.microprofile.config.inject.ConfigProperty; class G { @ConfigProperty(name = \"app.a\") String a; }";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert!(host.provider_diagnostics().get(JAVA).is_none());
        host.set_setting("quarkus.checkUnassigned", "true");
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert!(host.provider_diagnostics().get(JAVA).is_none());
    }

    #[test]
    fn reports_a_rest_client_interface_without_register_rest_client() {
        let host = quarkus_project();
        host.add_file("src/main/java/org/acme/Api.java", "import jakarta.ws.rs.Path; @Path(\"/x\") public interface Api { @GET String get(); }");
        host.add_file("src/main/java/org/acme/Ok.java", "import jakarta.ws.rs.Path; import org.eclipse.microprofile.rest.client.inject.RegisterRestClient; @RegisterRestClient @Path(\"/y\") public interface Ok { }");
        let text = "import org.eclipse.microprofile.rest.client.inject.RestClient; class G { @Inject @RestClient Api api; @Inject @RestClient Ok ok; @Inject @RestClient Unknown u; }";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc(JAVA, text));
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert_eq!(codes(text, &host.provider_diagnostics()[JAVA]), [("rest-client-not-registered", "api")]);
        host.set_setting("quarkus.checkAnnotations", "false");
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert!(host.provider_diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn registers_an_unregistered_rest_client_interface_from_the_injection_site() {
        let host = quarkus_project();
        let api = "package org.acme;\n\nimport jakarta.ws.rs.Path;\n\n@Path(\"/x\")\npublic interface Api { }\n";
        host.add_file("src/main/java/org/acme/Api.java", api);
        let text = "package org.acme;\nimport org.eclipse.microprofile.rest.client.inject.RestClient;\nimport jakarta.inject.Inject;\nclass G { @Inject @RestClient Api api; }";
        let mut tools = QuarkusTools::default();
        let at = text.rfind("api").unwrap();
        let selection = range(text, at, at+3);
        tools.code_actions(&doc(JAVA, text), selection);
        let actions = tools.code_actions(&doc(JAVA, text), selection);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Add @RegisterRestClient to org.acme.Api");
        let (file, edits) = &actions[0].edits[0];
        assert_eq!(file, "/project/src/main/java/org/acme/Api.java");
        assert_eq!(edits.len(), 1);
        assert!(edits.iter().any(|e| e.text == "@org.eclipse.microprofile.rest.client.inject.RegisterRestClient\n" && e.range.start == Position::new(4, 0)));
        assert!(!edits.iter().any(|e| e.text.starts_with("import ")));
    }

    #[test]
    fn health_and_metrics_checks_follow_their_setting() {
        let host = quarkus_project();
        let text = "import org.eclipse.microprofile.health.Liveness;\n@Liveness\nclass Ping { }\n";
        let mut tools = QuarkusTools::default();
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert_eq!(host.provider_diagnostics()[JAVA][0].code.as_deref(), Some("health-not-implemented"));
        host.set_setting("quarkus.checkHealthMetrics", "false");
        diagnose(&host, &mut tools, &doc(JAVA, text));
        assert!(host.provider_diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn quick_fixes_add_a_default_and_a_key_to_the_existing_properties_file() {
        let _host = java_host();
        let text = "import org.eclipse.microprofile.config.inject.ConfigProperty; class G {\n  @ConfigProperty(name = \"app.a\") int a;\n}\n";
        let mut tools = QuarkusTools::default();
        let selection = Range::new(Position::new(1, 5), Position::new(1, 5));
        tools.code_actions(&doc(JAVA, text), selection);
        let actions = tools.code_actions(&doc(JAVA, text), selection);
        assert_eq!(
            actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(),
            ["Add defaultValue", "Add `app.a` to application.properties"]
        );
        let default = &actions[0].edits[0].1[0];
        assert_eq!(default.text, ", defaultValue = \"0\"");
        let insertion = text.find(')').unwrap();
        assert_eq!(default.range, range(text, insertion, insertion));
        let (file, edits) = &actions[1].edits[0];
        assert_eq!(file, "/project/src/main/resources/application.properties");
        assert_eq!((edits[0].text.as_str(), edits[0].range.start), ("app.a=\n", Position::new(1, 0)));
        assert!(tools.code_actions(&doc(JAVA, text), Range::new(Position::new(0, 0), Position::new(0, 0))).is_empty());
    }

    #[test]
    fn opens_code_quarkus_io() {
        let host = MockHost::install();
        QuarkusTools::default().run_command("open-code-quarkus", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_urls(), ["https://code.quarkus.io"]);
        assert!(matches!(QuarkusTools::default().run_command("nope", &host.invocation(), None), Err(Error::NotFound(_))));
    }
}

#[cfg(test)]
mod build_source_receipts {
    use super::*;
    #[test]
    fn literal_maven_and_gradle_source_declarations_preserve_config_completion() {
        for (file,build) in [("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>custom-extension</artifactId></dependency></dependencies></project>"),
            ("build.gradle.kts","plugins { id(\"io.quarkus\") }"),
            ("build.gradle","plugins { id 'io.quarkus' }"),
            ("build.gradle.kts","dependencies { implementation(\"io.quarkus:quarkus-rest:3.40.1\") }")] {
            let host=MockHost::install();host.add_file(file,build);let text="quarkus.http.po";
            let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:text.into()};
            let items=QuarkusTools::default().completion(&doc,Position::new(0,text.len() as u32),None);
            assert!(items.iter().any(|item|item.label=="quarkus.http.port"),"{file}: {build}");
        }
    }

    use openidea_plugin::{mock::MockHost,hover::HoverProvider,diagnostics::DiagnosticsProvider};
    #[test]
    fn comments_and_unrelated_strings_do_not_establish_quarkus_module_facts() {
        for (file,build) in [("pom.xml","<project><!-- io.quarkus --><description>io.quarkus</description></project>"),
            ("build.gradle.kts","// plugins { id(\"io.quarkus\") }\nval description = \"io.quarkus\"\n"),
            ("build.gradle","/* io.quarkus */\ndef description = 'io.quarkus'\n"),
            ("build.gradle","def docs = /plugins { id(\"io.quarkus\") }/\n"),
            ("build.gradle","def docs = ''' ignored ' plugins { id(\"io.quarkus\") } '''\n")] {
            let host=MockHost::install();host.add_file(file,build);
            let text="quarkus.http.port=wrong\n";let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:text.into()};
            let mut tools=QuarkusTools::default();
            assert!(tools.completion(&doc,Position::new(0,7),None).is_empty(),"{file}");
            tools.activate_live();tools.warm_live(&doc.path,Some(text),std::time::Instant::now()+std::time::Duration::from_secs(1));
            assert!(tools.hover(&doc,Position::new(0,3)).is_none(),"{file}");assert!(tools.diagnostics(&doc).is_empty(),"{file}");
        }
    }
}

#[cfg(test)]
mod nested_owner_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider};
    const PATH:&str="/project/src/main/resources/application.properties";
    fn setup()->(MockHost,QuarkusTools,Document) {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/private/Foreign.java","package secret; import org.eclipse.microprofile.config.inject.ConfigProperty; class Foreign { @ConfigProperty(name=\"private.flag\",defaultValue=\"foreign-default\") boolean flag; }");
        host.add_project("/project/src/main/java/private");
        let doc=Document{path:PATH.into(),language:"properties".into(),version:1,text:"private.flag=true\n".into()};
        let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(PATH,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));(host,tools,doc)
    }
    #[test]
    fn nested_project_source_metadata_cannot_enter_outer_project_providers() {
        let (_host,mut tools,doc)=setup();assert!(tools.hover(&doc,Position::new(0,3)).is_none());
        let mut partial=doc.clone();partial.text="private.fl".into();assert!(!tools.completion(&partial,Position::new(0,10),None).iter().any(|item|item.label=="private.flag"));
    }
    #[test]
    fn ownership_change_after_cache_warm_withholds_previously_known_source() {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/private/Foreign.java","import org.eclipse.microprofile.config.inject.ConfigProperty; class Foreign { @ConfigProperty(name=\"private.flag\") boolean flag; }");
        let doc=Document{path:PATH.into(),language:"properties".into(),version:1,text:"private.flag=true\n".into()};
        let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(PATH,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));
        assert!(tools.hover(&doc,Position::new(0,3)).is_some());assert_eq!(tools.definition(&doc,Position::new(0,3)).len(),1);
        host.add_project("/project/src/main/java/private");
        assert!(tools.hover(&doc,Position::new(0,3)).is_none());assert!(tools.definition(&doc,Position::new(0,3)).is_empty());
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
    #[test]
    fn nested_project_source_is_not_a_registered_config_definition_target() {
        let (_host,mut tools,doc)=setup();assert!(tools.definition(&doc,Position::new(0,3)).is_empty());
    }
}

#[cfg(test)]
mod annotation_owner_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider};
    fn tools(source:&str)->(MockHost,QuarkusTools) {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/Foreign.java",source);let mut tools=QuarkusTools::default();tools.activate_live();(host,tools)
    }
    #[test]
    fn explicitly_foreign_annotation_names_do_not_supply_config_metadata() {
        for source in ["import other.ConfigProperty; class Foreign { @ConfigProperty(name=\"foreign.flag\") boolean flag; }",
            "import other.ConfigMapping; @ConfigMapping(prefix=\"foreign\") interface Foreign {boolean flag();}"] {
            let (_host,mut tools)=tools(source);let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:"foreign.flag=true\n".into()};
            tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));
            assert!(tools.hover(&doc,Position::new(0,3)).is_none(),"{source}");
        }
    }
    #[test]
    fn literal_framework_imports_and_fully_qualified_annotations_supply_metadata() {
        for source in ["import org.eclipse.microprofile.config.inject.ConfigProperty; class Owned { @ConfigProperty(name=\"owned.flag\") boolean flag; }",
            "class Owned { @org.eclipse.microprofile.config.inject.ConfigProperty(name=\"owned.flag\") boolean flag; }",
            "import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"owned\") interface Owned {boolean flag();}",
            "@io.smallrye.config.ConfigMapping(prefix=\"owned\") interface Owned {boolean flag();}"] {
            let (_host,mut tools)=tools(source);let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:"owned.flag=true\n".into()};
            tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));assert!(tools.hover(&doc,Position::new(0,3)).is_some(),"{source}");
        }
        for source in ["class Unknown { @ConfigProperty(name=\"unknown.flag\") boolean flag; }",
            "class ConfigProperty {} class Shadow { @ConfigProperty(name=\"unknown.flag\") boolean flag; }"] {
            let (_host,mut tools)=tools(source);let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:"unknown.flag=true\n".into()};
            tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));assert!(tools.hover(&doc,Position::new(0,3)).is_none());
        }
    }
    #[test]
    fn foreign_config_annotation_does_not_receive_framework_key_completion() {
        let source="import other.ConfigProperty; class Foreign { @ConfigProperty(name=\"quarkus.http.po\") int port; }";
        let (_host,mut tools)=tools(source);let doc=Document{path:"/project/src/main/java/Foreign.java".into(),language:"java".into(),version:1,text:source.into()};
        let at=openidea_plugin::text::position(source,source.find("quarkus.http.po").unwrap()+"quarkus.http.po".len());
        assert!(tools.completion(&doc,at,None).is_empty());
    }
    #[test]
    fn current_foreign_config_annotation_does_not_receive_framework_hover() {
        let source="import other.ConfigProperty; class Foreign { @ConfigProperty(name=\"quarkus.http.port\") int port; }";
        let (_host,mut tools)=tools(source);let doc=Document{path:"/project/src/main/java/Foreign.java".into(),language:"java".into(),version:1,text:source.into()};
        tools.warm_live(&doc.path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        assert!(tools.hover(&doc,openidea_plugin::text::position(source,source.find("quarkus.http.port").unwrap()+2)).is_none());
    }
}

#[cfg(test)]
mod literal_argument_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider,diagnostics::DiagnosticsProvider};
    #[test]
    fn partial_scalar_model_keeps_known_facts_without_unknown_key_absence_claims() {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/Model.java","import org.eclipse.microprofile.config.inject.ConfigProperty; class Model { static final String PREFIX=\"actual.\"; @ConfigProperty(name=\"known.flag\") boolean known; @ConfigProperty(name=PREFIX+\"unknown\") String other; }");
        let mut tools=QuarkusTools::default();tools.activate_live();let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:"known.flag=true\nquarkus.http.not-real=x\n".into()};
        tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));
        assert!(tools.hover(&doc,Position::new(0,2)).is_some());assert!(!tools.diagnostics(&doc).iter().any(|problem|problem.code.as_deref()==Some("unknown-property")));
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }

    fn hover(source:&str,key:&str)->Option<openidea_plugin::hover::Hover> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/Model.java",source);let mut tools=QuarkusTools::default();tools.activate_live();
        let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:format!("{key}=true\n")};
        tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));tools.hover(&doc,Position::new(0,1))
    }
    #[test]
    fn compound_config_property_name_does_not_claim_its_first_string_fragment() {
        assert!(hover("import org.eclipse.microprofile.config.inject.ConfigProperty; class Model { static final String PREFIX=\"actual.\"; @ConfigProperty(name=PREFIX+\"wrong.flag\") boolean flag; }","wrong.flag").is_none());
    }
    #[test]
    fn compound_mapping_prefix_does_not_claim_its_first_string_fragment() {
        assert!(hover("import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=Model.PREFIX+\"wrong\") interface Model {String PREFIX=\"actual.\"; boolean flag();}","wrong.flag").is_none());
    }
    #[test]
    fn compound_default_does_not_claim_its_first_string_fragment() {
        assert!(hover("import org.eclipse.microprofile.config.inject.ConfigProperty; class Model { static final String PREFIX=\"actual.\"; @ConfigProperty(name=\"model.flag\",defaultValue=PREFIX+\"false\") boolean flag; }","model.flag").is_none());
    }
}

#[cfg(test)]
mod mapping_modifier_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider};
    fn hover(source:&str,key:&str)->Option<openidea_plugin::hover::Hover> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");host.add_file("src/main/java/Model.java",source);
        let mut tools=QuarkusTools::default();tools.activate_live();let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:format!("{key}=true\n")};
        tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));tools.hover(&doc,Position::new(0,1))
    }
    #[test]
    fn foreign_with_name_does_not_rename_a_mapping_source_property() {
        let source="import io.smallrye.config.ConfigMapping; import other.WithName; @ConfigMapping(prefix=\"model\") interface Model {@WithName(\"invented\") boolean flag();}";
        assert!(hover(source,"model.invented").is_none());assert!(hover(source,"model.flag").is_some());
    }
    #[test]
    fn foreign_with_default_does_not_supply_a_mapping_default() {
        let source="import io.smallrye.config.ConfigMapping; import other.WithDefault; @ConfigMapping(prefix=\"model\") interface Model {@WithDefault(\"foreign-default\") boolean flag();}";
        assert!(!hover(source,"model.flag").unwrap().markdown.contains("foreign-default"));
    }
    #[test]
    fn foreign_parent_name_does_not_drop_mapping_member_name() {
        let source="import io.smallrye.config.ConfigMapping; import other.WithParentName; @ConfigMapping(prefix=\"model\") interface Model {@WithParentName boolean flag();}";
        assert!(hover(source,"model").is_none());assert!(hover(source,"model.flag").is_some());
    }
    #[test]
    fn known_modifier_namespaces_and_literal_static_enum_owner_remain_available() {
        let source="import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"owned\") interface Model {@io.smallrye.config.WithName(\"switch\") @io.smallrye.config.WithDefault(\"false\") boolean flag();}";
        assert!(hover(source,"owned.switch").unwrap().markdown.contains("false"));
        let source="import io.smallrye.config.ConfigMapping; import static io.smallrye.config.ConfigMapping.NamingStrategy.VERBATIM; @ConfigMapping(prefix=\"owned\",namingStrategy=VERBATIM) interface Model {boolean someName();}";
        assert!(hover(source,"owned.someName").is_some());
        let unknown="import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"unknown\") interface Model {@WithName(\"invented\") boolean flag();}";
        assert!(hover(unknown,"unknown.invented").is_none());assert!(hover(unknown,"unknown.flag").is_none());
    }
    #[test]
    fn unknown_naming_strategy_owner_does_not_select_a_known_strategy() {
        let source="import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"model\",namingStrategy=other.NamingStrategy.VERBATIM) interface Model {boolean someName();}";
        assert!(hover(source,"model.someName").is_none());
    }
}

#[cfg(test)]
mod group_owner_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider};
    fn hover(source:&str,key:&str)->Option<openidea_plugin::hover::Hover> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");host.add_file("src/main/java/Models.java",source);
        let mut tools=QuarkusTools::default();tools.activate_live();let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:format!("{key}=true\n")};
        tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));tools.hover(&doc,Position::new(0,1))
    }
    #[test]
    fn same_simple_nested_groups_do_not_share_the_first_type_owner() {
        let source="import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"a\") interface A {Group nested(); interface Group {boolean fromA();}} @ConfigMapping(prefix=\"b\") interface B {Group nested(); interface Group {boolean fromB();}}";
        assert!(hover(source,"b.nested.from-a").is_none());assert!(hover(source,"b.nested.from-b").is_some());assert!(hover(source,"a.nested.from-a").is_some());
    }
    #[test]
    fn qualified_foreign_group_is_not_replaced_by_a_local_simple_name() {
        let source="import io.smallrye.config.ConfigMapping; @ConfigMapping(prefix=\"model\") interface Model {other.Group nested(); interface Group {boolean local();}}";
        assert!(hover(source,"model.nested.local").is_none());
    }
}

#[cfg(test)]
mod rest_fix_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    const APP:&str="package app; import org.eclipse.microprofile.rest.client.inject.RestClient; import b.Api; class App { @RestClient Api api; }";
    fn setup(duplicate:bool)->(MockHost,QuarkusTools,Document) {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/a/Api.java",if duplicate {"package b; import jakarta.ws.rs.Path; @Path(\"/a\") public interface Api {}"} else {"package a; import jakarta.ws.rs.Path; @Path(\"/a\") public interface Api {}"});
        host.add_file("src/main/java/b/Api.java","package b; import jakarta.ws.rs.Path; @Path(\"/b\") public interface Api {}");host.add_file("src/main/java/app/App.java",APP);
        let doc=Document{path:"/project/src/main/java/app/App.java".into(),language:"java".into(),version:1,text:APP.into()};let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(&doc.path,Some(APP),std::time::Instant::now()+std::time::Duration::from_secs(1));(host,tools,doc)
    }
    fn actions(tools:&mut QuarkusTools,doc:&Document)->Vec<CodeAction> {
        let at=APP.rfind("api").unwrap();tools.code_actions(doc,range(APP,at,at+3)).into_iter().filter(|action|action.title.contains("RegisterRestClient")).collect()
    }
    #[test]
    fn registration_fix_targets_the_qualified_injected_interface_not_first_simple_name() {
        let (host,mut tools,doc)=setup(false);let fixes=actions(&mut tools,&doc);assert_eq!(fixes.len(),1);
        assert_eq!(fixes[0].edits[0].0,"/project/src/main/java/b/Api.java");assert!(host.process_runs().is_empty());assert!(host.requests().is_empty());
    }
    #[test]
    fn registration_proposal_preserves_compact_package_and_annotation_placement() {
        let (host,mut tools,doc)=setup(false);let removed="/project/src/main/java/a/Api.java";
        host.add_file(removed,"package a; class Gone {}");tools.handle_events(&[openidea_plugin::Event::FileSaved(removed.into())],&host.invocation());
        let fixes=actions(&mut tools,&doc);assert_eq!(fixes.len(),1);let (path,edits)=&fixes[0].edits[0];
        let original=host.invocation().project().unwrap().read_file(path).unwrap();let mut changed=original.clone();
        let mut edits:Vec<_>=edits.iter().map(|edit|(openidea_plugin::text::offset(&original,edit.range.start).unwrap(),openidea_plugin::text::offset(&original,edit.range.end).unwrap(),edit.text.clone())).collect();edits.sort_by_key(|edit|std::cmp::Reverse(edit.0));
        for (start,end,text) in edits {changed.replace_range(start..end,&text);}
        assert!(changed.starts_with("package b;"),"{changed}");assert!(changed.contains("@org.eclipse.microprofile.rest.client.inject.RegisterRestClient"));
        assert_eq!(host.invocation().project().unwrap().read_file(path).unwrap(),original);
    }
    #[test]
    fn duplicate_qualified_interface_declarations_do_not_offer_arbitrary_registration_fix() {
        let (_host,mut tools,doc)=setup(true);assert!(actions(&mut tools,&doc).is_empty());
    }
    #[test]
    fn registration_fix_rechecks_target_package_after_cached_discovery() {
        let (host,mut tools,doc)=setup(false);host.add_file("src/main/java/b/Api.java","package changed; import jakarta.ws.rs.Path; @Path(\"/changed\") public interface Api {}");
        assert!(actions(&mut tools,&doc).is_empty());
    }
}

#[cfg(test)]
mod add_key_target_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn setup(sibling:bool)->(MockHost,QuarkusTools,Document,String) {
        let host=MockHost::install();host.set_setting("quarkus.checkUnassigned","true");host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");
        let target=if sibling {"/project/b/src/main/resources/application.properties"}else{"/project/src/main/resources/application.properties"};host.add_file(target,"other.value=x\n");
        let path=if sibling {"/project/a/src/main/java/App.java"}else{"/project/src/main/java/App.java"};let source="import org.eclipse.microprofile.config.inject.ConfigProperty; class App {@ConfigProperty(name=\"app.missing\") int missing;}";host.add_file(path,source);
        let doc=Document{path:path.into(),language:"java".into(),version:1,text:source.into()};let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));(host,tools,doc,target.into())
    }
    fn actions(tools:&mut QuarkusTools,doc:&Document)->Vec<CodeAction> {
        let at=doc.text.rfind("missing").unwrap();tools.code_actions(doc,range(&doc.text,at,at+7))
    }
    #[test]
    fn add_key_does_not_choose_an_unproved_sibling_module_configuration() {
        let (host,mut tools,doc,target)=setup(true);let fixes=actions(&mut tools,&doc);
        assert!(!fixes.iter().any(|fix|fix.title.starts_with("Add `app.missing`")));assert!(fixes.iter().any(|fix|fix.title=="Add defaultValue"));
        assert_eq!(host.invocation().project().unwrap().read_file(&target).unwrap(),"other.value=x\n");
    }
    #[test]
    fn add_key_does_not_serialize_a_colon_name_as_a_different_properties_key() {
        let (_host,mut tools,mut doc,_target)=setup(false);doc.text=doc.text.replace("app.missing","app:missing");
        assert!(!actions(&mut tools,&doc).iter().any(|fix|fix.title.starts_with("Add `app:missing`")));
    }
    #[test]
    fn add_key_rechecks_current_target_keys_after_cached_discovery() {
        let (host,mut tools,doc,target)=setup(false);host.add_file(&target,"other.value=x\napp.missing=42\n");
        assert!(!actions(&mut tools,&doc).iter().any(|fix|fix.title.starts_with("Add `app.missing`")));
        assert_eq!(host.invocation().project().unwrap().read_file(&target).unwrap(),"other.value=x\napp.missing=42\n");
    }
}

#[cfg(test)]
mod scheduler_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn diagnostics(source:&str)->Vec<Diagnostic> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-scheduler</artifactId></dependency></dependencies></project>");
        let path="/project/src/main/java/App.java";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        host.open_editor(path,"java",source,&[]);host.refresh_diagnostics(&mut tools);host.provider_diagnostics().get(path).cloned().unwrap_or_default()
    }
    #[test]
    fn foreign_or_unresolved_scheduled_annotation_has_no_quarkus_diagnostics() {
        for source in ["import other.Scheduled; class App {@Scheduled(every=\"soon\") void run(){}}","class App {@Scheduled(every=\"soon\") void run(){}}","import io.quarkus.scheduler.Scheduled; @interface Scheduled {} class App {@Scheduled(every=\"soon\") void run(){}}"] {
            assert!(diagnostics(source).is_empty(),"{source}");
        }
    }
    #[test]
    fn qualified_scheduler_annotation_preserves_owned_diagnostics() {
        for source in ["import io.quarkus.scheduler.Scheduled; class App {@Scheduled(every=\"soon\") void run(){}}","class App {@io.quarkus.scheduler.Scheduled(every=\"soon\") void run(){}}"] {
            assert!(diagnostics(source).iter().any(|d|d.code.as_deref()==Some("invalid-every")),"{source}");
        }
    }
}

#[cfg(test)]
mod rest_injection_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn diagnostics(source:&str)->Vec<Diagnostic> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest-client</artifactId></dependency></dependencies></project>");
        host.add_file("src/main/java/b/Api.java","package b; import jakarta.ws.rs.Path; @Path(\"/api\") public interface Api {}");
        let path="/project/src/main/java/App.java";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        host.open_editor(path,"java",source,&[]);host.refresh_diagnostics(&mut tools);host.provider_diagnostics().get(path).cloned().unwrap_or_default()
    }
    #[test]
    fn foreign_or_unresolved_rest_client_does_not_claim_registration_failure() {
        for source in ["import b.Api; import other.RestClient; class App {@RestClient Api api;}","import b.Api; class App {@RestClient Api api;}"] {
            assert!(diagnostics(source).is_empty(),"{source}");
        }
    }
    #[test]
    fn literal_qualified_rest_client_preserves_owned_diagnostic() {
        let source="import b.Api; class App {@org.eclipse.microprofile.rest.client.inject.RestClient Api api;}";
        assert!(diagnostics(source).iter().any(|d|d.code.as_deref()==Some("rest-client-not-registered")));
    }
}

#[cfg(test)]
mod rest_catalog_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn diagnostics(target:&str)->Vec<Diagnostic> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest-client</artifactId></dependency></dependencies></project>");host.add_file("src/main/java/b/Api.java",target);
        let path="/project/src/main/java/App.java";let source="import b.Api; import org.eclipse.microprofile.rest.client.inject.RestClient; class App {@RestClient Api api;}";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        host.open_editor(path,"java",source,&[]);host.refresh_diagnostics(&mut tools);host.provider_diagnostics().get(path).cloned().unwrap_or_default()
    }
    #[test]
    fn foreign_path_does_not_manufacture_a_rest_candidate() {
        assert!(diagnostics("package b; import other.Path; @Path(\"/x\") public interface Api {}").is_empty());
    }
    #[test]
    fn another_type_path_does_not_manufacture_a_rest_candidate() {
        assert!(diagnostics("package b; import jakarta.ws.rs.Path; @Path(\"/x\") class Other {} public interface Api {}").is_empty());
    }
    #[test]
    fn foreign_registration_does_not_register_a_known_rest_interface() {
        assert!(diagnostics("package b; import jakarta.ws.rs.Path; import other.RegisterRestClient; @RegisterRestClient @Path(\"/x\") public interface Api {}").iter().any(|d|d.code.as_deref()==Some("rest-client-not-registered")));
    }
    #[test]
    fn unresolved_registration_annotation_keeps_absence_claim_unknown() {
        assert!(diagnostics("package b; import jakarta.ws.rs.Path; @RegisterRestClient @Path(\"/x\") public interface Api {}").is_empty());
    }
    #[test]
    fn literal_qualified_path_preserves_rest_candidate() {
        assert!(diagnostics("package b; @jakarta.ws.rs.Path(\"/x\") public interface Api {}").iter().any(|d|d.code.as_deref()==Some("rest-client-not-registered")));
    }
}

#[cfg(test)]
mod gauge_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn diagnostics(source:&str)->Vec<Diagnostic> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-smallrye-metrics</artifactId></dependency></dependencies></project>");
        let path="/project/src/main/java/App.java";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        host.open_editor(path,"java",source,&[]);host.refresh_diagnostics(&mut tools);host.provider_diagnostics().get(path).cloned().unwrap_or_default()
    }
    #[test]
    fn foreign_gauge_does_not_claim_framework_return_type_failure() {
        assert!(diagnostics("import other.Gauge; class App {@Gauge String value(){return null;}}").is_empty());
    }
    #[test]
    fn foreign_scope_is_not_a_framework_scope_rewrite_target() {
        assert!(diagnostics("import org.eclipse.microprofile.metrics.annotation.Gauge; import other.RequestScoped; @RequestScoped class App {@Gauge int value(){return 1;}}").is_empty());
    }
    #[test]
    fn literal_qualified_gauge_preserves_owned_diagnostics() {
        assert!(diagnostics("class App {@org.eclipse.microprofile.metrics.annotation.Gauge String value(){return null;}}").iter().any(|d|d.code.as_deref()==Some("gauge-return-type")));
    }
}

#[cfg(test)]
mod health_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn setup(source:&str)->(MockHost,QuarkusTools,Document) {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-smallrye-health</artifactId></dependency></dependencies></project>");
        let path="/project/src/main/java/App.java";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));
        let doc=Document{path:path.into(),language:"java".into(),version:1,text:source.into()};(host,tools,doc)
    }
    fn diagnostics(source:&str)->Vec<Diagnostic> {
        let (host,mut tools,doc)=setup(source);host.open_editor(&doc.path,"java",source,&[]);host.refresh_diagnostics(&mut tools);host.provider_diagnostics().get(&doc.path).cloned().unwrap_or_default()
    }
    #[test]
    fn foreign_health_names_do_not_supply_framework_type_facts() {
        for source in ["import other.Liveness; @Liveness class App {}","import other.HealthCheck; class App implements HealthCheck {}"] {assert!(diagnostics(source).is_empty(),"{source}");}
    }
    #[test]
    fn unknown_implemented_interface_does_not_prove_health_check_absent() {
        assert!(diagnostics("import org.eclipse.microprofile.health.Liveness; import external.Check; @Liveness class App implements Check {}").is_empty());
    }
    #[test]
    fn literal_qualified_health_qualifier_preserves_source_diagnostic() {
        assert!(diagnostics("@org.eclipse.microprofile.health.Liveness class App {}").iter().any(|d|d.code.as_deref()==Some("health-not-implemented")));
    }
    #[test]
    fn source_qualifier_absence_does_not_claim_effective_registration_absent() {
        let found=diagnostics("import org.eclipse.microprofile.health.HealthCheck; class App implements HealthCheck {}");assert!(!found.is_empty());assert!(found.iter().all(|d|!d.message.contains("Quarkus does not register")));
    }
    #[test]
    fn health_implements_proposal_preserves_existing_foreign_type_import() {
        let source="import org.eclipse.microprofile.health.Liveness; import other.HealthCheck; @Liveness class App {}";
        let (_host,mut tools,doc)=setup(source);let at=source.find("@Liveness").unwrap();let fixes=tools.code_actions(&doc,range(source,at,at+9));
        let action=fixes.iter().find(|f|f.title.contains("implement HealthCheck")).unwrap();assert!(action.edits.iter().flat_map(|(_,edits)|edits).any(|e|e.text.contains("implements org.eclipse.microprofile.health.HealthCheck")));assert!(!action.edits.iter().flat_map(|(_,edits)|edits).any(|e|e.text.contains("import org.eclipse.microprofile.health.HealthCheck")));
    }
}

#[cfg(test)]
mod optional_owner_receipts {
    use super::*;
    use openidea_plugin::{mock::MockHost,hover::HoverProvider};
    fn hover(source:&str,key:&str)->Option<openidea_plugin::hover::Hover> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>");host.add_file("src/main/java/Models.java",source);
        let mut tools=QuarkusTools::default();tools.activate_live();let doc=Document{path:"/project/src/main/resources/application.properties".into(),language:"properties".into(),version:1,text:format!("{key}=true\n")};tools.warm_live(&doc.path,Some(&doc.text),std::time::Instant::now()+std::time::Duration::from_secs(1));tools.hover(&doc,Position::new(0,1))
    }
    #[test]
    fn foreign_or_unresolved_optional_does_not_supply_nested_mapping_fields() {
        for import in ["import other.Optional;", "", "import java.util.Optional; class Optional<T>{}"] {
            let source=format!("import io.smallrye.config.ConfigMapping; {import} @ConfigMapping(prefix=\"model\") interface Model {{Optional<Group> nested(); interface Group {{boolean local();}}}}");assert!(hover(&source,"model.nested.local").is_none(),"{source}");
        }
    }
    #[test]
    fn literal_java_optional_preserves_nested_mapping_fields() {
        let source="import io.smallrye.config.ConfigMapping; import java.util.Optional; @ConfigMapping(prefix=\"model\") interface Model {Optional<Group> nested(); interface Group {boolean local();}}";assert!(hover(source,"model.nested.local").is_some());
    }
}

#[cfg(test)]
mod annotation_fix_owner_receipts {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn fixes(source:&str)->Vec<CodeAction> {
        let host=MockHost::install();host.add_file("pom.xml","<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-smallrye-health</artifactId></dependency></dependencies></project>");
        let path="/project/src/main/java/App.java";host.add_file(path,source);let mut tools=QuarkusTools::default();tools.activate_live();tools.warm_live(path,Some(source),std::time::Instant::now()+std::time::Duration::from_secs(1));let doc=Document{path:path.into(),language:"java".into(),version:1,text:source.into()};let result=tools.code_actions(&doc,range(source,0,source.len()));assert_eq!(host.invocation().project().unwrap().read_file(path).unwrap(),source);result
    }
    #[test]
    fn health_qualifier_fix_does_not_bind_to_existing_foreign_import() {
        let actions=fixes("import other.Liveness; import org.eclipse.microprofile.health.HealthCheck; class App implements HealthCheck {}");let action=actions.iter().find(|a|a.title.ends_with("@Liveness")).unwrap();let edits:Vec<_>=action.edits.iter().flat_map(|(_,edits)|edits).collect();assert!(edits.iter().any(|e|e.text.contains("@org.eclipse.microprofile.health.Liveness")));assert!(!edits.iter().any(|e|e.text.contains("import org.eclipse.microprofile.health.Liveness")));
    }
    #[test]
    fn gauge_scope_fix_does_not_bind_to_existing_foreign_import() {
        let actions=fixes("import other.ApplicationScoped; import jakarta.enterprise.context.RequestScoped; import org.eclipse.microprofile.metrics.annotation.Gauge; @RequestScoped class App {@Gauge int value(){return 1;}}");let action=actions.iter().find(|a|a.title=="Use @ApplicationScoped").unwrap();let edits:Vec<_>=action.edits.iter().flat_map(|(_,edits)|edits).collect();assert!(edits.iter().any(|e|e.text=="@jakarta.enterprise.context.ApplicationScoped"));assert!(!edits.iter().any(|e|e.text.contains("import jakarta.enterprise.context.ApplicationScoped")));
    }
}

#[cfg(test)]
mod gradle_catalog_tests {
    use super::*;
    use openidea_plugin::Event;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn conventional_catalog_alias_enables_module_and_changes_invalidate_proof() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.add_file("build.gradle.kts", "plugins { alias(libs.plugins.quarkus.core) }");
        host.add_file("gradle/libs.versions.toml", "[plugins]\nquarkus-core = { id = \"io.quarkus\", version = \"3.40.1\" }\n");
        let mut tools = QuarkusTools::default();
        let path = "/project/src/main/resources/application.properties";
        assert!(tools.uses_quarkus(path));
        host.add_file("gradle/libs.versions.toml", "[plugins]\nquarkus-core = { id = \"other.framework\", version = \"1\" }\n");
        tools.live_events(&[Event::FileSaved("/project/gradle/libs.versions.toml".into())]);
        assert!(!tools.uses_quarkus(path));
        host.add_file("gradle/libs.versions.toml", "[plugins]\nquarkus-core = { id = \"io.quarkus\", version = \"3.40.1\" }\n");
        tools.live_events(&[Event::FileSaved("/project/gradle/libs.versions.toml".into())]);
        assert!(tools.uses_quarkus(path));
        host.add_file("settings.gradle.kts", "dependencyResolutionManagement { versionCatalogs { create(\"libs\") {} } }");
        tools.live_events(&[Event::FileSaved("/project/settings.gradle.kts".into())]);
        assert!(!tools.uses_quarkus(path));
    }
    #[test]
    fn catalog_requires_real_plugin_block_and_exact_alias_identity() {
        let cat = "[plugins]\nquarkus = { id = \"io.quarkus\", version.ref = \"quarkus\" }\n";
        assert!(build_source::declares_catalog("build.gradle.kts", "plugins { alias(libs.plugins.quarkus) }", cat));
        for text in ["// plugins { alias(libs.plugins.quarkus) }", "dependencies { alias(libs.plugins.quarkus) }", "plugins { alias(libs.plugins.other) }", "plugins { alias(libs.plugins.quarkus + foo) }", "plugins { alias(libs.plugins.quarkus) apply false }", "plugins { alias(libs.plugins.quarkus) "] {
            assert!(!build_source::declares_catalog("build.gradle.kts", text, cat), "{text}");
        }
        assert!(!build_source::declares_catalog("build.gradle.kts", "plugins { alias(libs.plugins.quarkus) }", "not valid toml"));
    }
}

#[cfg(test)]
mod unapplied_plugin_tests {
    #[test]
    fn deferred_gradle_plugins_do_not_establish_module_ownership() {
        for (file, text) in [("build.gradle.kts", "plugins { id(\"io.quarkus\") apply false }"), ("build.gradle", "plugins { id 'io.quarkus' apply false }")] {
            assert!(!crate::build_source::declares(file, text));
        }
        assert!(crate::build_source::declares("build.gradle.kts", "plugins { id(\"io.quarkus\") apply true }"));
    }
}
