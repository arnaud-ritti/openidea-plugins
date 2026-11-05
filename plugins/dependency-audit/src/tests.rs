use super::*;
use crate::manifest::{Dep, Ecosystem};
use crate::osv::tests::{JACKSON, dep};
use openidea_plugin::mock::MockHost;
use openidea_plugin::views::{Body, TreeItem, ViewEvent};
use openidea_plugin::{Event, HttpRequest, HttpResponse, Position, Range, Severity};
use serde_json::{Value, json};

pub const CAPS: &[&str] = &["project.read", "storage", "events", "net:api.osv.dev"];

pub const LODASH: &str = r#"{"id": "GHSA-35jh-r3h4-6jhm", "modified": "2024-01-01T00:00:00Z", "summary": "Command injection in lodash",
 "database_specific": {"severity": "HIGH"},
 "affected": [{"package": {"ecosystem": "npm", "name": "lodash"}, "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}]}]}"#;

const VULNERABLE: &[(&str, &str, &str, &str)] = &[
    ("Maven", "com.fasterxml.jackson.core:jackson-databind", "2.12.3", "GHSA-57j2-w4cx-62h2"),
    ("npm", "lodash", "4.17.15", "GHSA-35jh-r3h4-6jhm"),
];

fn ok(body: String) -> Result<HttpResponse, Error> {
    Ok(HttpResponse { status: 200, headers: vec![], body: body.into_bytes() })
}

pub fn osv(req: &HttpRequest) -> Result<HttpResponse, Error> {
    match (req.method.as_str(), req.url.as_str()) {
        ("POST", "https://api.osv.dev/v1/querybatch") => {
            let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
            let results: Vec<Value> = body["queries"].as_array().unwrap().iter().map(|q| {
                let hit = VULNERABLE.iter().find(|(e, n, v, _)| q["package"]["ecosystem"] == *e && q["package"]["name"] == *n && q["version"] == *v);
                match hit {
                    Some((.., id)) => json!({"vulns": [{"id": id, "modified": if id.contains("57j2") { "2024-03-15T05:00:00Z" } else { "2024-01-01T00:00:00Z" }}]}),
                    None => json!({}),
                }
            }).collect();
            ok(json!({"results": results}).to_string())
        }
        ("GET", "https://api.osv.dev/v1/vulns/GHSA-57j2-w4cx-62h2") => ok(JACKSON.into()),
        ("GET", "https://api.osv.dev/v1/vulns/GHSA-35jh-r3h4-6jhm") => ok(LODASH.into()),
        _ => Ok(HttpResponse { status: 404, headers: vec![], body: vec![] }),
    }
}

pub fn posts(host: &MockHost) -> usize {
    host.requests().iter().filter(|r| r.method == "POST").count()
}

fn jackson() -> Dep {
    dep(Ecosystem::Maven, "com.fasterxml.jackson.core:jackson-databind", "2.12.3")
}

#[test]
fn audit_queries_osv_and_caches_results() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let deps = [jackson(), dep(Ecosystem::Maven, "org.yaml:snakeyaml", "2.2")];

    let findings = osv::audit(&deps, true, 1_000).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].dep.name, "com.fasterxml.jackson.core:jackson-databind");
    assert_eq!(findings[0].fix().as_deref(), Some("2.12.7.1"));
    assert_eq!(host.requests().len(), 2, "one batch, one advisory");
    assert_eq!(host.requests()[0].headers, [("Content-Type".to_string(), "application/json".to_string())]);

    assert_eq!(osv::audit(&deps, true, 2_000).unwrap(), findings);
    assert_eq!(host.requests().len(), 2, "answered from the cache");

    osv::audit(&deps, true, 1_000 + osv::QUERY_TTL_SECS + 1).unwrap();
    assert_eq!(host.requests().len(), 3, "the query expired; the advisory is unchanged");
}

#[test]
fn offline_audit_uses_only_the_cache() {
    let host = MockHost::install().with_capabilities(CAPS);

    assert_eq!(osv::audit(&[jackson()], false, 0).unwrap(), vec![]);
    assert!(host.requests().is_empty());
}

#[test]
fn audit_sends_at_most_1000_queries_per_request() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let deps: Vec<Dep> = (0..1001).map(|i| dep(Ecosystem::Npm, &format!("pkg{i}"), "1.0.0")).collect();

    osv::audit(&deps, true, 0).unwrap();
    assert_eq!(posts(&host), 2);
}

#[test]
fn audit_reports_osv_errors() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));

    let error = osv::audit(&[jackson()], true, 0).unwrap_err();
    assert!(error.to_string().contains("503"), "{error}");
}

#[test]
fn a_failed_advisory_still_reports_its_id() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|r| if r.method == "GET" { Err(Error::Failed("timed out".into())) } else { osv(r) });

    let findings = osv::audit(&[jackson()], true, 0).unwrap();
    assert_eq!(findings[0].advisories[0].id, "GHSA-57j2-w4cx-62h2");
    assert_eq!(findings[0].advisories[0].severity, None);
    assert!(!host.storage().contains_key("v:GHSA-57j2-w4cx-62h2"), "a failed advisory isn't cached");
}

#[test]
fn a_large_audit_stores_all_queries_under_one_key() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let deps: Vec<Dep> = (0..1500).map(|i| dep(Ecosystem::Npm, &format!("pkg{i}"), "1.0.0")).collect();

    osv::audit(&deps, true, 0).unwrap();

    let storage = host.storage();
    let queries: Value = serde_json::from_str(&storage["queries"]).unwrap();
    assert_eq!(queries.as_object().unwrap().len(), 1500);
    assert!(storage.keys().all(|k| !k.starts_with("q:")));
}

#[test]
fn a_hostile_advisory_id_is_never_fetched_or_stored() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|r| {
        if r.method == "POST" {
            return ok(json!({"results": [{"vulns": [{"id": "../querybatch", "modified": "x"}]}]}).to_string());
        }
        osv(r)
    });

    let findings = osv::audit(&[jackson()], true, 0).unwrap();
    assert_eq!(findings[0].advisories[0].id, "../querybatch");
    assert!(host.requests().iter().all(|r| r.method == "POST"));
    assert!(host.storage().keys().all(|k| !k.starts_with("v:")));
}

pub const POM: &str = "<project>\n  <dependencies>\n    <dependency><groupId>org.yaml</groupId><artifactId>snakeyaml</artifactId><version>2.2</version></dependency>\n    <dependency><!-- 😀 --><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>2.12.3</version></dependency>\n  </dependencies>\n</project>\n";
pub const PACKAGE: &str = "{\n  \"dependencies\": {\n    \"lodash\": \"^4.17.15\"\n  }\n}\n";
pub const PACKAGE_LOCK: &str = r#"{"lockfileVersion": 3, "packages": {"": {}, "node_modules/lodash": {"version": "4.17.15"}}}"#;

fn opened(host: &MockHost, path: &str) {
    DependencyAudit::default().handle_events(&[Event::FileOpened(path.into())], &host.invocation());
}

#[test]
fn opening_a_pom_publishes_a_diagnostic_on_the_dependency() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    let problems = &host.diagnostics()["/project/pom.xml"];
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].severity, Severity::Error);
    assert_eq!(problems[0].code.as_deref(), Some("GHSA-57j2-w4cx-62h2"));
    assert_eq!(problems[0].source.as_deref(), Some("OSV"));
    assert!(problems[0].message.ends_with("Upgrade to 2.12.7.1."), "{}", problems[0].message);
}

#[test]
fn diagnostic_range_counts_utf16_columns() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    let range = host.diagnostics()["/project/pom.xml"][0].range;
    assert_eq!((range.start.line, range.start.character, range.end.character), (3, 84, 100));
}

#[test]
fn package_json_uses_the_sibling_lockfile() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("web/package.json", PACKAGE);
    host.add_file("web/package-lock.json", PACKAGE_LOCK);

    opened(&host, "/project/web/package.json");

    assert_eq!(host.diagnostics()["/project/web/package.json"][0].range.start.line, 2);
}

#[test]
fn auto_check_off_sends_nothing() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.set_setting("autoCheck", "false");
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    assert!(host.requests().is_empty());
    assert!(host.diagnostics().is_empty());
}

#[test]
fn other_files_are_ignored() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);

    DependencyAudit::default().handle_events(&[Event::FileSaved("/project/src/App.java".into())], &host.invocation());

    assert!(host.requests().is_empty());
}

#[test]
fn an_osv_outage_logs_and_keeps_quiet() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Err(Error::Failed("offline".into())));
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    assert!(host.diagnostics().is_empty());
    assert!(host.notifications().is_empty());
    assert!(host.logs().iter().any(|(_, line)| line.contains("OSV check of /project/pom.xml: offline")));
}

fn document(path: &str, language: &str, text: &str) -> openidea_plugin::providers::Document {
    openidea_plugin::providers::Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn line(n: u32) -> Range {
    Range::new(Position::new(n, 0), Position::new(n, 0))
}

#[test]
fn quick_fix_upgrades_to_the_nearest_fixed_version() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    opened(&host, "/project/pom.xml");
    let sent = host.requests().len();

    let actions = DependencyAudit::default().code_actions(&document("/project/pom.xml", "xml", POM), line(3));

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Upgrade jackson-databind to 2.12.7.1");
    assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
    let (path, edits) = &actions[0].edits[0];
    assert_eq!(path, "/project/pom.xml");
    assert_eq!(edits[0].text, "2.12.7.1");
    assert_eq!(edits[0].range.start, Position::new(3, 122));
    assert_eq!(host.requests().len(), sent, "no network on the edit path");
}

#[test]
fn quick_fix_keeps_the_npm_range_operator() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("package.json", PACKAGE);
    host.add_file("package-lock.json", PACKAGE_LOCK);
    opened(&host, "/project/package.json");

    let actions = DependencyAudit::default().code_actions(&document("/project/package.json", "json", PACKAGE), line(2));

    assert_eq!(actions[0].title, "Upgrade lodash to 4.17.21");
    assert_eq!(actions[0].edits[0].1[0].text, "^4.17.21");
}

#[test]
fn no_quick_fix_for_a_compound_range() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let package = PACKAGE.replace("^4.17.15", ">=4.0.0 <5");
    host.add_file("package.json", &package);
    host.add_file("package-lock.json", PACKAGE_LOCK);
    opened(&host, "/project/package.json");

    let actions = DependencyAudit::default().code_actions(&document("/project/package.json", "json", &package), line(2));

    assert!(actions.is_empty());
}

#[test]
fn no_quick_fix_without_a_cached_result() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);

    let actions = DependencyAudit::default().code_actions(&document("/project/pom.xml", "xml", POM), line(3));

    assert!(actions.is_empty());
    assert!(host.requests().is_empty());
}

#[test]
fn no_quick_fix_off_the_dependency_line_or_in_lockfiles() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    host.add_file("package-lock.json", PACKAGE_LOCK);
    opened(&host, "/project/pom.xml");
    opened(&host, "/project/package-lock.json");
    let mut plugin = DependencyAudit::default();

    assert!(plugin.code_actions(&document("/project/pom.xml", "xml", POM), line(2)).is_empty());
    assert!(plugin.code_actions(&document("/project/package-lock.json", "json", PACKAGE_LOCK), line(0)).is_empty());
}

#[test]
fn deps_sharing_a_property_get_one_quick_fix_with_the_highest_fix() {
    let other = r#"{"id": "GHSA-aaaa-bbbb-cccc", "modified": "2024-01-01T00:00:00Z", "summary": "Other",
 "affected": [{"package": {"ecosystem": "Maven", "name": "com.example:other"}, "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.13.0"}]}]}]}"#;
    let pom = "<project>\n<properties><jackson.version>2.12.3</jackson.version></properties>\n<dependencies>\n<dependency><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>${jackson.version}</version></dependency>\n<dependency><groupId>com.example</groupId><artifactId>other</artifactId><version>${jackson.version}</version></dependency>\n</dependencies>\n</project>\n";
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(move |req| match (req.method.as_str(), req.url.as_str()) {
        ("POST", _) => ok(json!({"results": [
            {"vulns": [{"id": "GHSA-57j2-w4cx-62h2", "modified": "2024-03-15T05:00:00Z"}]},
            {"vulns": [{"id": "GHSA-aaaa-bbbb-cccc", "modified": "2024-01-01T00:00:00Z"}]},
        ]}).to_string()),
        ("GET", "https://api.osv.dev/v1/vulns/GHSA-57j2-w4cx-62h2") => ok(JACKSON.into()),
        _ => ok(other.into()),
    });
    host.add_file("pom.xml", pom);
    opened(&host, "/project/pom.xml");

    let selection = Range::new(Position::new(3, 0), Position::new(4, 0));
    let actions = DependencyAudit::default().code_actions(&document("/project/pom.xml", "xml", pom), selection);

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Upgrade jackson-databind to 2.13.0");
    assert_eq!(actions[0].edits[0].1[0].text, "2.13.0");
}

fn scanned(host: &MockHost) -> DependencyAudit {
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    host.add_file("web/package.json", PACKAGE);
    host.add_file("web/package-lock.json", PACKAGE_LOCK);
    let mut plugin = DependencyAudit::default();
    plugin.run_command("scan", &host.invocation(), None).unwrap();
    plugin
}

fn tree(plugin: &mut DependencyAudit, host: &MockHost) -> Vec<TreeItem> {
    match plugin.render("vulnerabilities", None, &host.invocation()).unwrap().body {
        Body::Tree(items) => items,
        other => panic!("a tree, got {other:?}"),
    }
}

#[test]
fn scan_lists_vulnerable_dependencies_in_the_view() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut plugin = scanned(&host);

    assert_eq!(host.notifications()[0].body, "Vulnerable dependencies: 2 of 3 (3 files). See the Vulnerable Dependencies tool window.");
    assert!(host.refreshed_views().contains(&"vulnerabilities".to_string()));
    assert_eq!(host.diagnostics().keys().collect::<Vec<_>>(), ["/project/pom.xml", "/project/web/package-lock.json", "/project/web/package.json"]);

    let labels: Vec<String> = tree(&mut plugin, &host).into_iter().map(|i| i.label).collect();
    assert_eq!(labels.iter().filter(|l| l.starts_with("lodash")).count(), 1, "the lockfile adds no duplicate");
    assert!(labels.contains(&"com.fasterxml.jackson.core:jackson-databind 2.12.3".to_string()));
    assert!(labels.contains(&"GHSA-35jh-r3h4-6jhm".to_string()));
}

#[test]
fn the_view_survives_a_new_instance() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut first = scanned(&host);
    let before = tree(&mut first, &host);

    assert_eq!(tree(&mut DependencyAudit::default(), &host), before);
}

#[test]
fn activating_items_opens_the_line_or_osv_dev() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut plugin = scanned(&host);
    let items = tree(&mut plugin, &host);
    let key = |label: &str| items.iter().find(|i| i.label == label).unwrap().key.clone();

    plugin.handle("vulnerabilities", ViewEvent::Activate(key("GHSA-57j2-w4cx-62h2")), &host.invocation()).unwrap();
    plugin.handle("vulnerabilities", ViewEvent::Activate(key("com.fasterxml.jackson.core:jackson-databind 2.12.3")), &host.invocation()).unwrap();

    assert_eq!(host.opened_urls(), ["https://osv.dev/vulnerability/GHSA-57j2-w4cx-62h2"]);
    assert_eq!(host.opened_locations(), [("/project/pom.xml".to_string(), 3, 0)]);
}

#[test]
fn before_a_scan_the_view_offers_to_scan() {
    let host = MockHost::install().with_capabilities(CAPS);

    let view = DependencyAudit::default().render("vulnerabilities", None, &host.invocation()).unwrap();

    assert!(matches!(view.body, Body::Message(_)));
    assert_eq!(view.toolbar[0].id, "scan");
}

#[test]
fn a_failed_scan_tells_the_user() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));
    host.add_file("pom.xml", POM);

    let error = DependencyAudit::default().run_command("scan", &host.invocation(), None).unwrap_err();

    assert!(error.to_string().contains("OSV answered 503"));
}

#[test]
fn a_fresh_instance_opens_items_from_the_stored_rows() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut first = scanned(&host);
    let items = tree(&mut first, &host);
    let key = items.iter().find(|i| i.label == "GHSA-57j2-w4cx-62h2").unwrap().key.clone();

    DependencyAudit::default().handle("vulnerabilities", ViewEvent::Activate(key), &host.invocation()).unwrap();

    assert_eq!(host.opened_urls(), ["https://osv.dev/vulnerability/GHSA-57j2-w4cx-62h2"]);
}

#[test]
fn a_dependency_shared_by_two_manifests_is_marked_in_both_and_listed_once() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("a/pom.xml", POM);
    host.add_file("b/pom.xml", POM);
    let mut plugin = DependencyAudit::default();
    plugin.run_command("scan", &host.invocation(), None).unwrap();

    assert_eq!(host.diagnostics().keys().collect::<Vec<_>>(), ["/project/a/pom.xml", "/project/b/pom.xml"]);
    let jackson = tree(&mut plugin, &host).into_iter().filter(|i| i.label.starts_with("com.fasterxml")).count();
    assert_eq!(jackson, 1);
}
