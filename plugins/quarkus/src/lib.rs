//! Quarkus Tools for OpenIDEA: completion and checks for Quarkus configuration
//! files, ported from Quarkus Tools for IntelliJ 2.6.1 (Red Hat, EPL-2.0).

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Error, Invocation, Plugin, Position, register_plugin};
use spring_text::config::is_config_file;
use spring_text::index::project_of;
use spring_text::metadata::{Catalog, Lookup, parse_tsv, properties_completion, yaml_completion};
use spring_text::range;
use std::collections::BTreeMap;
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

#[derive(Default)]
pub struct QuarkusTools {
    /// Whether a directory is inside a Quarkus module, by absolute path.
    modules: BTreeMap<String, bool>,
}

impl QuarkusTools {
    /// Looks for `io.quarkus` in the build files of the file's directory and
    /// its parents, up to the project root: in a multi-module build only the
    /// root (or a parent) may name Quarkus.
    fn uses_quarkus(&mut self, path: &str) -> bool {
        let Some(project) = project_of(path) else { return false };
        let Ok(root) = project.root() else { return false };
        let root = root.trim_end_matches('/');
        let start = path.rsplit_once('/').map_or("", |(dir, _)| dir);
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
    if !typed.is_empty() && (!typed.starts_with('%') || typed.contains('.')) {
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

impl Providers for QuarkusTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !is_quarkus_config(&doc.path) || !self.uses_quarkus(&doc.path) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let lookup = Lookup(vec![bundled()]);
        if !doc.path.ends_with(".properties") {
            return yaml_completion(&doc.text, at, &lookup);
        }
        let mut items = profile_items(&doc.text, at);
        items.extend(properties_completion(&doc.text, at, &lookup));
        items
    }
}

impl Plugin for QuarkusTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(QuarkusTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::metadata::Lookup;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{CompletionItem, Document, Providers};
    use openidea_plugin::text::position;
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
    fn completes_profiles() {
        let _host = quarkus_project();
        assert_eq!(labels(&complete(PROPS, "%")), ["%dev.", "%test.", "%prod."]);
        assert_eq!(find(&complete(PROPS, ""), "%dev.").insert_text, "%dev.");
        let text = "server.x=1\n%dev.quarkus.http.po";
        assert_eq!(find(&complete(PROPS, text), "quarkus.http.port").replace, Some(range(text, 16, text.len())));
        assert_eq!(labels(&complete(PROPS, "%test.quarkus.http.ssl.client-auth=")), ["none", "request", "required"]);
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
}
