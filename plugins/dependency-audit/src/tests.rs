use super::*;
use crate::manifest::{Dep, Ecosystem};
use crate::osv::tests::{JACKSON, dep};
use openidea_plugin::mock::MockHost;
use openidea_plugin::{HttpRequest, HttpResponse};
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
