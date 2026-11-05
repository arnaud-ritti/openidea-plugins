//! Spring Boot Tools for OpenIDEA: completion of keys and values in
//! `application*.properties|yml`, deprecated and unknown key warnings (from
//! Spring Boot's metadata, the project's metadata files and its
//! `@ConfigurationProperties` classes) and cron expression checks in
//! `@Scheduled(cron = …)` and `*.cron` keys. Text-based, Java only.

mod cron;
mod meta;

use meta::bundled;
use openidea_plugin::providers::{CodeAction, CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::config::{self, Key, canonical, is_config_file};
use spring_text::metadata::{Catalog, Lookup, Meta, properties_completion, yaml_completion};
use spring_text::index::{Indexes, PROVIDER_BUDGET};
use spring_text::{contains, java, range};

const SOURCES: &[&str] = &["**/META-INF/*spring-configuration-metadata.json", "**/*.java"];

fn parse_source(path: &str, text: &str) -> Option<Meta> {
    if path.ends_with(".json") { meta::parse_json(text) } else { meta::parse_java(text) }
}

fn placeholder(s: &str) -> bool {
    s.contains("${") || s.contains("#{")
}

pub struct BootTools {
    sources: Indexes<Meta>,
}

impl Default for BootTools {
    fn default() -> Self {
        Self { sources: Indexes::new(SOURCES, parse_source) }
    }
}

impl BootTools {
    /// The project's own properties, and whether a whole pass over the project was read.
    fn project(&mut self, doc: &Document) -> (Catalog, bool) {
        let mut catalog = Catalog::default();
        let Some(index) = self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else {
            return (catalog, false);
        };
        for (_, meta) in index.iter() {
            catalog.add(meta);
        }
        (catalog, index.complete())
    }
}

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

fn config_problems(path: &str, text: &str, lookup: &Lookup, complete: bool) -> Vec<Problem> {
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

impl Providers for BootTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !is_config_file(&doc.path) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let (project, _) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        if doc.path.ends_with(".properties") {
            properties_completion(&doc.text, at, &lookup)
        } else {
            yaml_completion(&doc.text, at, &lookup)
        }
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !is_config_file(&doc.path) {
            return Vec::new();
        }
        let (project, complete) = self.project(doc);
        let lookup = Lookup(vec![&project, bundled()]);
        config_problems(&doc.path, &doc.text, &lookup, complete)
            .into_iter()
            .filter(|p| contains(p.diagnostic.range, selection.start))
            .filter_map(|p| {
                let (title, edit) = p.fix?;
                Some(CodeAction {
                    title,
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![edit])],
                    data: None,
                })
            })
            .collect()
    }

    /// Publishes the file's problems; there are no gutter markers.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let problems = if doc.path.ends_with(".java") {
            self.sources.for_document(&doc.path, &doc.text, PROVIDER_BUDGET);
            cron_problems(&doc.text)
        } else if is_config_file(&doc.path) {
            let (project, complete) = self.project(doc);
            let lookup = Lookup(vec![&project, bundled()]);
            config_problems(&doc.path, &doc.text, &lookup, complete).into_iter().map(|p| p.diagnostic).collect()
        } else {
            return Vec::new();
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

impl Plugin for BootTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
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
}
