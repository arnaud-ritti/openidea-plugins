//! Quarkus Tools for OpenIDEA: completion and checks for Quarkus configuration
//! files, ported from Quarkus Tools for IntelliJ 2.6.1 (Red Hat, EPL-2.0).

mod java;

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Severity, host, register_plugin};
use spring_text::config::{self, is_config_file};
use spring_text::index::{Indexes, PROVIDER_BUDGET, project_of};
use spring_text::metadata::{Catalog, Lookup, Meta, parse_tsv, properties_completion, strip_profile, yaml_completion};
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

pub struct QuarkusTools {
    sources: Indexes<Meta>,
    /// Whether a directory is inside a Quarkus module, by absolute path.
    modules: BTreeMap<String, bool>,
}

impl Default for QuarkusTools {
    fn default() -> Self {
        Self { sources: Indexes::new(&["**/*.java"], java::config_meta), modules: BTreeMap::new() }
    }
}

impl QuarkusTools {
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

    /// Looks for `io.quarkus` in the build files of the file's directory and
    /// its parents, up to the project root: in a multi-module build only the
    /// root (or a parent) may name Quarkus.
    fn uses_quarkus(&mut self, path: &str) -> bool {
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

fn problems(path: &str, text: &str, lookup: &Lookup, complete: bool) -> Vec<Diagnostic> {
    let keys = config::keys(path, text);
    let mut out = Vec::new();
    for k in &keys {
        let base = strip_profile(&k.key);
        if base.is_empty() {
            continue;
        }
        let range = range(text, k.at, k.end);
        if let Some(dep) = lookup.prop(base).and_then(|p| p.deprecated.as_ref()) {
            let message = match dep.replacement.as_str() {
                "" => format!("`{base}` is deprecated."),
                r => format!("`{base}` is deprecated. Use `{r}` instead."),
            };
            let mut d = Diagnostic::new(range, Severity::Warning, message).code("deprecated-property");
            d.deprecated = true;
            out.push(d);
        } else if complete
            // A YAML parent (`http:` with keys below) is checked through its children.
            && !keys.iter().any(|o| o.key.len() > k.key.len() && o.key.starts_with(&k.key) && matches!(o.key.as_bytes()[k.key.len()], b'.' | b'['))
            && let Some(group) = lookup.unknown(base)
        {
            let message = format!("`{base}` is not a known property of `{group}`.");
            out.push(Diagnostic::new(range, Severity::Warning, message).code("unknown-property"));
        }
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
            return Vec::new();
        }
        if !is_quarkus_config(&doc.path) {
            return Vec::new();
        }
        let found = if self.uses_quarkus(&doc.path) {
            let (project, complete) = self.project(doc);
            problems(&doc.path, &doc.text, &Lookup(vec![&project, bundled()]), complete)
        } else {
            Vec::new()
        };
        let _ = host::publish_diagnostics(&doc.path, &found);
        Vec::new()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
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
        items.extend(properties_completion(&doc.text, at, &lookup));
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

register_plugin!(QuarkusTools, providers);

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
    fn opens_code_quarkus_io() {
        let host = MockHost::install();
        QuarkusTools::default().run_command("open-code-quarkus", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_urls(), ["https://code.quarkus.io"]);
        assert!(matches!(QuarkusTools::default().run_command("nope", &host.invocation(), None), Err(Error::NotFound(_))));
    }
}
