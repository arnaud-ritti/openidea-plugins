//! Quarkus Tools for OpenIDEA: completion and checks for Quarkus configuration
//! files, ported from Quarkus Tools for IntelliJ 2.6.1 (Red Hat, EPL-2.0).

pub mod checks;
mod indexer;
mod java;
mod java_checks;

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::config::{self, is_config_file};
use spring_text::index::{Indexes, PROVIDER_BUDGET, project_of};
use spring_text::metadata::{Catalog, Lookup, parse_tsv, properties_completion, strip_profile, yaml_completion};
use spring_text::range;
use java::JavaInfo;
use java_checks::{Context, Fix};
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

const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const PROFILES: [&str; 3] = ["%dev.", "%test.", "%prod."];

pub fn is_quarkus_config(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name == "microprofile-config.properties" || (is_config_file(path) && name.starts_with("application"))
}

pub struct QuarkusTools {
    sources: Indexes<JavaInfo>,
    configs: Indexes<Vec<String>>,
    published_java: BTreeSet<String>,
    /// Whether a directory is inside a Quarkus module, by absolute path.
    modules: BTreeMap<String, bool>,
}

impl Default for QuarkusTools {
    fn default() -> Self {
        Self {
            sources: Indexes::new(&["**/*.java"], java::java_info),
            configs: Indexes::new(&["**/application*.properties", "**/application*.yaml", "**/application*.yml", "**/microprofile-config.properties"], squashed_keys),
            published_java: BTreeSet::new(),
            modules: BTreeMap::new(),
        }
    }
}

fn squashed_keys(path: &str, text: &str) -> Option<Vec<String>> {
    let lookup = Lookup(vec![bundled()]);
    let keys: Vec<String> = config::keys(path, text).iter().map(|k| checks::squash(&checks::normalize(&lookup, strip_profile(&k.key)))).collect();
    Some(keys)
}

fn env_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().filter_map(|line| line.trim_start().split_once('=').map(|(key, _)| checks::squash(key.trim_start_matches("export "))))
}

impl QuarkusTools {
    /// Every key the project's other config files and `.env` set, `None` while the project is not fully read.
    fn java_problems(&mut self, doc: &Document) -> Vec<java_checks::Problem> {
        let unassigned = host::setting_bool("quarkus.checkUnassigned").unwrap_or(false);
        let annotations = host::setting_bool("quarkus.checkAnnotations").unwrap_or(true);
        if !(unassigned || annotations || host::setting_bool("quarkus.checkHealthMetrics").unwrap_or(true)) || !self.uses_quarkus(&doc.path) {
            return Vec::new();
        }
        let rest_clients = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET).filter(|index| index.complete()).map(|index| {
            let mut clients: BTreeMap<String, bool> = BTreeMap::new();
            for (name, registered) in index.iter().flat_map(|(_, info)| info.interfaces.iter()) {
                *clients.entry(name.clone()).or_default() |= *registered;
            }
            clients
        });
        let (project, _) = self.project(doc);
        let configured = self.keys_elsewhere(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        let health_metrics = host::setting_bool("quarkus.checkHealthMetrics").unwrap_or(true);
        let ctx = Context { lookup: &lookup, configured: configured.as_ref(), rest_clients: rest_clients.as_ref(), unassigned, annotations, health_metrics };
        java_checks::check(&doc.text, &ctx)
    }

    fn unregistered_interface(&mut self, doc: &Document, name: &str) -> Option<(String, String)> {
        let index = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        let file = index.iter().find(|(_, info)| info.interfaces.iter().any(|(n, registered)| n == name && !registered))?.0.to_owned();
        let content = project_of(&doc.path)?.read_file(&file).ok()?;
        Some((file, content))
    }

    /// The `application.properties` nearest to `path`.
    fn properties_file_for(&mut self, doc: &Document) -> Option<String> {
        let index = self.configs.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        let shared = |p: &str| p.bytes().zip(doc.path.bytes()).take_while(|(a, b)| a == b).count();
        index.iter().map(|(p, _)| p).filter(|p| p.ends_with("/application.properties")).max_by_key(|p| shared(p)).map(str::to_owned)
    }

    fn keys_elsewhere(&mut self, doc: &Document) -> Option<BTreeSet<String>> {
        let index = self.configs.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        if !index.complete() {
            return None;
        }
        let mut keys: BTreeSet<String> = index.iter().flat_map(|(_, keys)| keys.iter().cloned()).collect();
        let root = project_of(&doc.path)?.root().ok()?;
        if let Ok(env) = project_of(&doc.path)?.read_file(&format!("{}/.env", root.trim_end_matches('/'))) {
            keys.extend(env_keys(&env));
        }
        Some(keys)
    }

    /// The project's own properties, and whether a whole pass over the project was read.
    fn project(&mut self, doc: &Document) -> (Catalog, bool) {
        let mut catalog = Catalog::default();
        let Some(index) = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else {
            return (catalog, false);
        };
        for meta in index.iter().filter_map(|(_, info)| info.meta.as_ref()) {
            catalog.add(meta);
        }
        (catalog, index.complete())
    }

    /// Looks for `io.quarkus` in the build files of the file's directory and
    /// its parents, up to the project root: in a multi-module build only the
    /// root (or a parent) may name Quarkus.
    pub(crate) fn uses_quarkus(&mut self, path: &str) -> bool {
        let Some(project) = project_of(path) else { return false };
        let Ok(root) = project.root() else { return false };
        let root = root.trim_end_matches('/');
        let start = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        // ponytail: answers (negatives too) are kept for the plugin's lifetime; invalidate on build-file save events if users hit it
        if let Some(&known) = self.modules.get(start) {
            return known;
        }
        let mut dir = start;
        let found = loop {
            let named = BUILD_FILES.iter().any(|file| {
                project.read_file(&format!("{dir}/{file}")).is_ok_and(|text| text.contains("io.quarkus"))
            });
            if named {
                break true;
            }
            if dir.len() <= root.len() {
                break false;
            }
            let Some((parent, _)) = dir.rsplit_once('/') else { break false };
            dir = parent;
        };
        self.modules.insert(start.to_owned(), found);
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
    /// Publishes the file's problems; there are no gutter markers. Java documents only refresh the index.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if doc.path.ends_with(".java") {
            if self.uses_quarkus(&doc.path) {
                self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET);
            }
            let found: Vec<Diagnostic> = self.java_problems(doc).iter().map(|p| p.finding.diagnostic(&doc.text)).collect();
            if !found.is_empty() || self.published_java.remove(&doc.path) {
                let _ = host::publish_diagnostics(&doc.path, &found);
            }
            if !found.is_empty() {
                self.published_java.insert(doc.path.clone());
            }
            return Vec::new();
        }
        if !is_quarkus_config(&doc.path) {
            return Vec::new();
        }
        let found = if self.uses_quarkus(&doc.path) {
            let (project, complete) = self.project(doc);
            let elsewhere = if host::setting_bool("quarkus.checkRequired").unwrap_or(false) { self.keys_elsewhere(doc) } else { None };
            problems(&doc.path, &doc.text, &Lookup(vec![&project, bundled()]), complete, elsewhere.as_ref())
        } else {
            Vec::new()
        };
        let _ = host::publish_diagnostics(&doc.path, &found);
        Vec::new()
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
                        let Some((file, current)) = self.unregistered_interface(doc, &name) else { continue };
                        let Some(start) = java::parse_interface_start(&current, &name) else { continue };
                        let edits = java_checks::annotate_edits(&current, start, "RegisterRestClient", java_checks::REGISTER_REST_CLIENT)
                            .into_iter()
                            .map(|(a, b, text)| TextEdit { range: range(&current, a, b), text })
                            .collect();
                        (format!("Add @RegisterRestClient to {name}"), vec![(file, edits)])
                    }
                    Fix::AddKey { key } => {
                        let Some(file) = self.properties_file_for(doc) else { continue };
                        let Ok(current) = project_of(&doc.path).map_or(Err(Error::NotFound(file.clone())), |p| p.read_file(&file)) else { continue };
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
        if doc.path.ends_with(".java") {
            let Some(at) = offset(&doc.text, at) else { return Vec::new() };
            if !self.uses_quarkus(&doc.path) || java::completion_slot(&doc.text, at).is_none() {
                return Vec::new();
            }
            let (project, _) = self.project(doc);
            return java::key_items(&doc.text, at, &Lookup(vec![&project, bundled()]));
        }
        if !is_quarkus_config(&doc.path) || !self.uses_quarkus(&doc.path) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let (project, _) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        if !doc.path.ends_with(".properties") {
            return yaml_completion(&doc.text, at, &lookup);
        }
        let mut items = profile_items(&doc.text, at);
        items.extend(named_items(&doc.text, at, &lookup).unwrap_or_else(|| properties_completion(&doc.text, at, &lookup)));
        items
    }
}

impl Plugin for QuarkusTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if id != "open-code-quarkus" {
            return Err(Error::NotFound(format!("command {id}")));
        }

        host::open_url("https://code.quarkus.io")?;

        Ok(None)
    }
}

register_plugin!(QuarkusTools, providers, indexing);

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::metadata::Lookup;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{CompletionItem, Document, Providers};
    use openidea_plugin::text::position;
    use spring_text::range;
    use openidea_plugin::Range;

    const PROPS: &str = "/project/src/main/resources/application.properties";
    const YAML: &str = "/project/src/main/resources/application.yaml";
    const QUARKUS_POM: &str = "<project><dependencies><dependency><groupId>io.quarkus</groupId><artifactId>quarkus-rest</artifactId></dependency></dependencies></project>";

    fn quarkus_project() -> MockHost {
        let host = MockHost::install();
        host.add_file("pom.xml", QUARKUS_POM);
        host
    }

    fn doc(path: &str, text: &str) -> Document {
        let language = if path.ends_with(".java") { "java" } else if path.ends_with(".properties") { "properties" } else { "yaml" };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
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
        host.add_file("src/main/java/org/acme/GreetingResource.java",
            "class G { @ConfigProperty(name = \"greeting.message\", defaultValue = \"hello\") String message; }");
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
        let before = "class M { @ConfigProperty(name = \"app.mail.from\") String from; }";
        let after = "class M { @ConfigProperty(name = \"app.mail.from\") String from; @ConfigProperty(name = \"app.mail.to\") String to; }";
        host.add_file("src/main/java/org/acme/Mail.java", before);
        let mut tools = QuarkusTools::default();
        let typed = "app.mail.";
        let items = tools.completion(&doc(PROPS, typed), position(typed, typed.len()), None);
        assert!(labels(&items).contains(&"app.mail.from"));
        assert!(!labels(&items).contains(&"app.mail.to"));
        assert!(tools.line_markers(&doc(java, after)).is_empty());
        let items = tools.completion(&doc(PROPS, typed), position(typed, typed.len()), None);
        assert!(labels(&items).contains(&"app.mail.to"));
        let text = "app.mail.to=x\n";
        tools.line_markers(&doc(PROPS, text));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        assert!(host.diagnostics().get(java).is_none());
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
        for p in ["/p/src/main/resources/application.properties", "/p/application-dev.properties", "/p/config/application.yml",
                  "/p/src/main/resources/application.yaml", "/p/src/main/resources/META-INF/microprofile-config.properties"] {
            assert!(is_quarkus_config(p), "{p}");
        }
        for p in ["/p/bootstrap.properties", "/p/messages.properties", "/p/applications.yml"] {
            assert!(!is_quarkus_config(p), "{p}");
        }
    }

    /// (code, text under the squiggle) of each problem.
    fn codes<'t, 'p>(text: &'t str, problems: &'p [Diagnostic]) -> Vec<(&'p str, &'t str)> {
        problems.iter().map(|d| {
            let (a, b) = (offset(text, d.range.start).unwrap(), offset(text, d.range.end).unwrap());
            (d.code.as_deref().unwrap_or_default(), &text[a..b])
        }).collect()
    }

    #[test]
    fn warns_about_deprecated_and_unknown_keys() {
        let host = quarkus_project();
        let text = "quarkus.hibernate-orm.database.generation=update\nquarkus.http.portt=8080\n%dev.quarkus.http.prot=1\nquarkus.acme-extension.x=1\napp.greeting=hi\nquarkus.log.category.\"io.acme\".level=DEBUG\n";
        QuarkusTools::default().line_markers(&doc(PROPS, text));
        let diagnostics = host.diagnostics();
        let problems = &diagnostics[PROPS];
        assert_eq!(codes(text, problems), [
            ("deprecated-property", "quarkus.hibernate-orm.database.generation"),
            ("unknown-property", "quarkus.http.portt"),
            ("unknown-property", "%dev.quarkus.http.prot"),
        ]);
        assert!(problems[0].deprecated);
        assert_eq!(problems[0].message, "`quarkus.hibernate-orm.database.generation` is deprecated.");
        assert_eq!(problems[2].message, "`quarkus.http.prot` is not a known property of `quarkus.http`.");
    }

    #[test]
    fn checks_profiled_yaml_keys_as_their_base_key() {
        let host = quarkus_project();
        let text = "\"%dev\":\n  quarkus:\n    http:\n      portt: 1\n      port: 8081\n";
        QuarkusTools::default().line_markers(&doc(YAML, text));
        assert_eq!(codes(text, &host.diagnostics()[YAML]), [("unknown-property", "portt")]);
    }

    #[test]
    fn publishes_value_checks_with_the_deprecated_ones_and_clears_when_fixed() {
        let host = quarkus_project();
        let mut tools = QuarkusTools::default();
        let text = "quarkus.http.port=abc\nquarkus.http.portt=1\nquarkus.http.port=1\n";
        tools.line_markers(&doc(PROPS, text));
        assert_eq!(codes(text, &host.diagnostics()[PROPS]).iter().map(|c| c.0).collect::<Vec<_>>(), ["unknown-property", "duplicate-key", "invalid-value"]);
        host.set_setting("quarkus.checkValues", "false");
        host.set_setting("quarkus.checkSyntax", "false");
        tools.line_markers(&doc(PROPS, text));
        assert_eq!(host.diagnostics()[PROPS].len(), 1);
        tools.line_markers(&doc(PROPS, "quarkus.http.port=8080\n"));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        let yaml = "quarkus:\n  http:\n    port: abc\n";
        host.set_setting("quarkus.checkValues", "true");
        tools.line_markers(&doc(YAML, yaml));
        assert_eq!(codes(yaml, &host.diagnostics()[YAML]), [("invalid-value", "abc")]);
    }

    #[test]
    fn expressions_are_checked_only_in_the_base_file() {
        let host = quarkus_project();
        let text = "app.greeting=hi\napp.a=${app.greting}\n";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc("/project/src/main/resources/application-dev.properties", text));
        assert!(host.diagnostics().get("/project/src/main/resources/application-dev.properties").is_none_or(|d| d.is_empty()));
        tools.line_markers(&doc(PROPS, text));
        assert_eq!(codes(text, &host.diagnostics()[PROPS]), [("undefined-expression", "${app.greting}")]);
    }

    #[test]
    fn publishes_nothing_outside_quarkus_modules() {
        let host = MockHost::install();
        let stale = Diagnostic::new(Range::default(), Severity::Warning, "stale");
        host::publish_diagnostics(PROPS, &[stale]).unwrap();
        assert_eq!(host.diagnostics()[PROPS].len(), 1);
        QuarkusTools::default().line_markers(&doc(PROPS, "quarkus.http.portt=1\n"));
        assert!(host.diagnostics().get(PROPS).is_none());
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
        let missing: Vec<&str> = extensions
            .iter()
            .filter(|(_, key)| lookup.prop(key).is_none())
            .map(|(extension, _)| *extension)
            .collect();
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
        QuarkusTools::default().line_markers(&doc(PROPS, text));
        assert_eq!(codes(text, &host.diagnostics()[PROPS]), [
            ("unknown-property", "quarkus.datasource.\"users\".jdbc.urll"),
            ("invalid-value", "abc"),
        ]);
    }

    #[test]
    fn required_keys_are_reported_only_when_asked_and_when_nothing_else_sets_them() {
        let host = quarkus_project();
        let text = "quarkus.tls.key-store.jks.password=secret\n";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc(PROPS, text));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        host.set_setting("quarkus.checkRequired", "true");
        tools.line_markers(&doc(PROPS, text));
        let found = &host.diagnostics()[PROPS];
        assert_eq!(codes(text, found), [("missing-required", "quarkus.tls.key-store.jks.password")]);
        assert_eq!(found[0].severity, Severity::Info);
        tools.line_markers(&doc(PROPS, "quarkus.http.port=1\n"));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn required_keys_set_in_another_file_or_the_environment_are_quiet() {
        let host = quarkus_project();
        host.set_setting("quarkus.checkRequired", "true");
        let text = "quarkus.tls.key-store.jks.password=secret\n";
        host.add_file("src/main/resources/application-prod.properties", "%prod.quarkus.tls.key-store.jks.path=/k.jks\n");
        QuarkusTools::default().line_markers(&doc(PROPS, text));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
        let host = quarkus_project();
        host.set_setting("quarkus.checkRequired", "true");
        host.add_file(".env", "QUARKUS_TLS_KEY_STORE_JKS_PATH=/k.jks\n");
        QuarkusTools::default().line_markers(&doc(PROPS, text));
        assert!(host.diagnostics().get(PROPS).is_none_or(|d| d.is_empty()));
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
        host.add_file("src/main/java/org/acme/Other.java", "class O { @ConfigProperty(name = \"greeting.message\") String m; }");
        let text = "class G {\n  @ConfigProperty(name = \"quarkus.http.po";
        let items = complete(JAVA, text);
        let port = find(&items, "quarkus.http.port");
        assert_eq!((port.insert_text.as_str(), port.detail.as_deref()), ("quarkus.http.port", Some("int")));
        assert_eq!(port.replace, Some(range(text, text.len() - 15, text.len())));
        assert!(!labels(&items).contains(&"quarkus.http"));
        let text = "@ConfigMapping(prefix = \"quarkus.http.s";
        assert!(labels(&complete(JAVA, text)).contains(&"quarkus.http.ssl"));
        assert!(complete(JAVA, "@ConfigProperty(name = \"quarkus.http.port\", defaultValue = \"qu").is_empty());
        assert!(complete(JAVA, "String s = \"quarkus.ht").is_empty());
        assert!(complete(JAVA, "@Named(name = \"quarkus.ht").is_empty());
        assert!(labels(&complete(JAVA, "class G { @ConfigProperty(name = \"greeting.")).contains(&"greeting.message"));
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
        let text = "class G {\n  @ConfigProperty(name = \"app.a\") String a;\n  @ConfigProperty(name = \"app.d\") String d;\n  @Scheduled(cron = \"* * * * *\") void tick() {}\n}\n";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc(JAVA, text));
        tools.line_markers(&doc(JAVA, text));
        assert_eq!(codes(text, &host.diagnostics()[JAVA]), [("unassigned-property", "app.a"), ("invalid-cron", "* * * * *")]);
        tools.line_markers(&doc(JAVA, "class G {}\n"));
        assert!(host.diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn unassigned_check_is_off_by_default_and_quiet_without_config_files() {
        let host = quarkus_project();
        let text = "class G { @ConfigProperty(name = \"app.a\") String a; }";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc(JAVA, text));
        assert!(host.diagnostics().get(JAVA).is_none());
        host.set_setting("quarkus.checkUnassigned", "true");
        tools.line_markers(&doc(JAVA, text));
        assert!(host.diagnostics().get(JAVA).is_none());
    }

    #[test]
    fn reports_a_rest_client_interface_without_register_rest_client() {
        let host = quarkus_project();
        host.add_file("src/main/java/org/acme/Api.java", "@Path(\"/x\") public interface Api { @GET String get(); }");
        host.add_file("src/main/java/org/acme/Ok.java", "@RegisterRestClient @Path(\"/y\") public interface Ok { }");
        let text = "class G { @Inject @RestClient Api api; @Inject @RestClient Ok ok; @Inject @RestClient Unknown u; }";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc(JAVA, text));
        tools.line_markers(&doc(JAVA, text));
        assert_eq!(codes(text, &host.diagnostics()[JAVA]), [("rest-client-not-registered", "api")]);
        host.set_setting("quarkus.checkAnnotations", "false");
        tools.line_markers(&doc(JAVA, text));
        assert!(host.diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn registers_an_unregistered_rest_client_interface_from_the_injection_site() {
        let host = quarkus_project();
        let api = "package org.acme;\n\nimport jakarta.ws.rs.Path;\n\n@Path(\"/x\")\npublic interface Api { }\n";
        host.add_file("src/main/java/org/acme/Api.java", api);
        let text = "class G { @Inject @RestClient Api api; }";
        let mut tools = QuarkusTools::default();
        let selection = Range::new(Position::new(0, 30), Position::new(0, 30));
        tools.code_actions(&doc(JAVA, text), selection);
        let actions = tools.code_actions(&doc(JAVA, text), selection);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].title, "Add @RegisterRestClient to Api");
        let (file, edits) = &actions[0].edits[0];
        assert_eq!(file, "/project/src/main/java/org/acme/Api.java");
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().any(|e| e.text == "@RegisterRestClient\n" && e.range.start == Position::new(4, 0)));
        assert!(edits.iter().any(|e| e.text.starts_with("import org.eclipse.microprofile.rest.client.inject.RegisterRestClient;")));
    }

    #[test]
    fn health_and_metrics_checks_follow_their_setting() {
        let host = quarkus_project();
        let text = "@Liveness\nclass Ping { }\n";
        let mut tools = QuarkusTools::default();
        tools.line_markers(&doc(JAVA, text));
        assert_eq!(host.diagnostics()[JAVA][0].code.as_deref(), Some("health-not-implemented"));
        host.set_setting("quarkus.checkHealthMetrics", "false");
        tools.line_markers(&doc(JAVA, text));
        assert!(host.diagnostics().get(JAVA).is_none_or(|d| d.is_empty()));
    }

    #[test]
    fn quick_fixes_add_a_default_and_a_key_to_the_existing_properties_file() {
        let _host = java_host();
        let text = "class G {\n  @ConfigProperty(name = \"app.a\") int a;\n}\n";
        let mut tools = QuarkusTools::default();
        let selection = Range::new(Position::new(1, 5), Position::new(1, 5));
        tools.code_actions(&doc(JAVA, text), selection);
        let actions = tools.code_actions(&doc(JAVA, text), selection);
        assert_eq!(actions.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Add defaultValue", "Add `app.a` to application.properties"]);
        let default = &actions[0].edits[0].1[0];
        assert_eq!(default.text, ", defaultValue = \"0\"");
        assert_eq!(default.range, range(text, 42, 42));
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
