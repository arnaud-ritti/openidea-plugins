//! OSV advisories: parsing, version comparison and per-dependency findings.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use openidea_plugin::{Error, HttpRequest, Severity, host};
use serde::{Deserialize, Serialize};

use crate::manifest::Dep;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advisory {
    pub id: String,
    pub modified: String,
    pub summary: String,
    pub severity: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Keyed `"{ecosystem}:{name}"` in OSV spelling.
    pub fixed: BTreeMap<String, Vec<String>>,
}

pub fn parse_advisory(json: &str) -> Option<Advisory> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |v: &serde_json::Value| v.as_str().map(str::to_string);
    let affected = value["affected"].as_array().map(Vec::as_slice).unwrap_or_default();

    let severity = text(&value["database_specific"]["severity"])
        .or_else(|| affected.iter().find_map(|a| text(&a["ecosystem_specific"]["severity"])))
        .or_else(|| affected.iter().find_map(|a| text(&a["database_specific"]["severity"])))
        .map(|s| s.to_uppercase());

    let mut fixed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for package in affected {
        let (Some(ecosystem), Some(name)) = (package["package"]["ecosystem"].as_str(), package["package"]["name"].as_str()) else {
            continue;
        };
        let versions = fixed.entry(format!("{ecosystem}:{name}")).or_default();
        let events = package["ranges"].as_array().into_iter().flatten().filter(|r| r["type"].as_str() != Some("GIT")).flat_map(|r| r["events"].as_array().into_iter().flatten());
        versions.extend(events.filter_map(|e| text(&e["fixed"])));
    }

    Some(Advisory {
        id: text(&value["id"])?,
        modified: text(&value["modified"]).unwrap_or_default(),
        summary: text(&value["summary"]).unwrap_or_default(),
        severity,
        aliases: value["aliases"].as_array().into_iter().flatten().filter_map(|a| text(a)).collect(),
        fixed,
    })
}

pub fn level(severity: Option<&str>) -> Severity {
    match severity {
        Some("CRITICAL" | "HIGH") => Severity::Error,
        Some("LOW") => Severity::Info,
        _ => Severity::Warning,
    }
}

// ponytail: dotted-number compare; Maven qualifiers (Final, SP) and SemVer build metadata aren't ranked, use the ecosystem's rules if fixes come out wrong
pub fn compare(a: &str, b: &str) -> Ordering {
    let separators = ['.', '-', '+', '_'];
    let mut a = a.split(separators);
    let mut b = b.split(separators);

    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (Some(x), None) => return if is_number(x) { Ordering::Greater } else { Ordering::Less },
            (None, Some(y)) => return if is_number(y) { Ordering::Less } else { Ordering::Greater },
            (Some(x), Some(y)) => match compare_segments(x, y) {
                Ordering::Equal => {}
                other => return other,
            },
        }
    }
}

fn is_number(segment: &str) -> bool {
    !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit())
}

fn compare_segments(a: &str, b: &str) -> Ordering {
    match (is_number(a), is_number(b)) {
        (true, true) => {
            let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        }
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
    }
}

pub fn nearest_fix(version: &str, fixed: &[String]) -> Option<String> {
    fixed
        .iter()
        .filter(|f| compare(f, version) == Ordering::Greater)
        .min_by(|a, b| compare(a, b))
        .cloned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub dep: Dep,
    pub advisories: Vec<Advisory>,
}

impl Finding {
    pub fn fix(&self) -> Option<String> {
        let key = format!("{}:{}", self.dep.ecosystem.osv(), self.dep.name);

        self.advisories
            .iter()
            .filter_map(|a| nearest_fix(&self.dep.version, a.fixed.get(&key)?))
            .max_by(|a, b| compare(a, b))
    }

    pub fn severity(&self) -> Severity {
        let levels: Vec<Severity> = self.advisories.iter().map(|a| level(a.severity.as_deref())).collect();

        [Severity::Error, Severity::Warning, Severity::Info]
            .into_iter()
            .find(|s| levels.contains(s))
            .unwrap_or(Severity::Warning)
    }

    pub fn message(&self) -> String {
        let advisories = self
            .advisories
            .iter()
            .map(|a| {
                let severity = a.severity.as_ref().map(|s| format!(" ({s})")).unwrap_or_default();
                let summary = if a.summary.is_empty() { String::new() } else { format!(" {}", a.summary) };
                format!("{}{severity}{summary}", a.id)
            })
            .collect::<Vec<_>>()
            .join("; ");
        let undetailed = self.advisories.iter().all(|a| a.summary.is_empty() && a.fixed.is_empty());
        let outcome = match (self.fix(), undetailed) {
            (Some(fix), _) => format!("Upgrade to {fix}."),
            (None, true) => "Details unavailable; check again later.".to_string(),
            (None, false) => "No fixed version is known.".to_string(),
        };

        format!("{} {} has known vulnerabilities: {advisories}. {outcome}", self.dep.name, self.dep.version)
    }
}

pub const QUERY_TTL_SECS: u64 = 86_400;
pub const BATCH: usize = 1000;

const QUERY_URL: &str = "https://api.osv.dev/v1/querybatch";
const VULN_URL: &str = "https://api.osv.dev/v1/vulns";
const TIMEOUT_MS: u32 = 10_000;

/// Storage layout: the single key `queries` holds a JSON object
/// `"{ecosystem}:{name}@{version}"` -> `{"at": secs, "vulns": [[id, modified], ...]}`
/// (the host rewrites the whole storage file on every write, so it is read and written once per audit);
/// `v:{id}` holds one `Advisory` each.
#[derive(Clone, Serialize, Deserialize)]
struct CachedQuery {
    at: u64,
    vulns: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct BatchResponse {
    results: Vec<BatchResult>,
}

#[derive(Deserialize)]
struct BatchResult {
    #[serde(default)]
    vulns: Vec<BatchVuln>,
}

#[derive(Deserialize)]
struct BatchVuln {
    id: String,
    #[serde(default)]
    modified: String,
}

fn query_key(dep: &Dep) -> String {
    format!("{}:{}@{}", dep.ecosystem.osv(), dep.name, dep.version)
}

fn cached_queries() -> BTreeMap<String, CachedQuery> {
    host::storage::get("queries")
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub(crate) fn cached_advisory(id: &str) -> Option<Advisory> {
    serde_json::from_str(&host::storage::get(&format!("v:{id}")).ok()??).ok()
}

fn is_safe_id(id: &str) -> bool {
    (1..=200).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || "._:-".contains(c))
}

fn fetch(request: HttpRequest) -> Result<Vec<u8>, Error> {
    let response = host::fetch(&HttpRequest { timeout_ms: Some(TIMEOUT_MS), ..request })?;

    if response.status != 200 {
        return Err(Error::Failed(format!("OSV answered {}", response.status)));
    }

    Ok(response.body)
}

fn query_batch(chunk: &[&Dep]) -> Result<Vec<BatchResult>, Error> {
    let queries: Vec<serde_json::Value> = chunk
        .iter()
        .map(|dep| serde_json::json!({"package": {"ecosystem": dep.ecosystem.osv(), "name": dep.name}, "version": dep.version}))
        .collect();
    let body = serde_json::json!({"queries": queries}).to_string();
    let request = HttpRequest::post(QUERY_URL, body).header("Content-Type", "application/json");
    // ponytail: next_page_token ignored; OSV pages only past 1,000 vulns per query or 3,000 per batch
    let response: BatchResponse = serde_json::from_slice(&fetch(request)?)
        .map_err(|e| Error::Failed(format!("OSV sent an unreadable answer: {e}")))?;

    if response.results.len() != chunk.len() {
        return Err(Error::Failed("OSV answered a different number of results".into()));
    }

    Ok(response.results)
}

/// `online` turns false at the first failed fetch or once past the deadline.
fn advisory(id: &str, modified: &str, online: &mut bool, deadline: Instant, writer: &mut CacheWriter) -> Advisory {
    let id_only = Advisory { id: id.into(), modified: modified.into(), ..Advisory::default() };

    if !is_safe_id(id) {
        return id_only;
    }

    if let Some(cached) = cached_advisory(id).filter(|a| a.modified == modified) {
        return cached;
    }

    *online &= Instant::now() < deadline;
    let fetched = if *online {
        let body = fetch(HttpRequest::get(&format!("{VULN_URL}/{id}")));
        *online = body.is_ok();
        body.ok().and_then(|body| parse_advisory(&String::from_utf8_lossy(&body)))
    } else {
        None
    };

    let Some(advisory) = fetched else {
        return id_only;
    };

    writer.set(&format!("v:{id}"), &serde_json::to_string(&advisory).unwrap_or_default());

    advisory
}

/// Writes to storage; when it is full, drops the whole cache once and retries.
#[derive(Default)]
struct CacheWriter {
    dropped: bool,
}

impl CacheWriter {
    fn set(&mut self, key: &str, value: &str) {
        if host::storage::set(key, value).is_ok() {
            return;
        }

        if self.dropped {
            return;
        }

        self.dropped = true;
        // ponytail: 1 MiB storage; the whole cache is dropped when it fills
        let _ = host::storage::delete("queries");
        for stale in host::storage::keys("v:").unwrap_or_default() {
            let _ = host::storage::delete(&stale);
        }

        let _ = host::storage::set(key, value);
    }
}

/// Lowercased advisory ids of the `ignoredAdvisories` setting: one per line, `#` starts a comment.
pub fn ignored_advisories() -> HashSet<String> {
    host::setting_string("ignoredAdvisories")
        .unwrap_or_default()
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim().to_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

pub fn is_ignored(ignored: &HashSet<String>, id: &str, aliases: &[String]) -> bool {
    ignored.contains(&id.to_lowercase()) || aliases.iter().any(|alias| ignored.contains(&alias.to_lowercase()))
}

pub fn audit(deps: &[Dep], online: bool, now: u64, deadline: Instant) -> Result<Vec<Finding>, Error> {
    let mut cache = cached_queries();
    let mut seen: HashSet<String> = HashSet::new();
    let mut unique: Vec<(&Dep, String)> = Vec::new();
    let mut missing: Vec<&Dep> = Vec::new();

    for dep in deps {
        let key = query_key(dep);

        if !seen.insert(key.clone()) {
            continue;
        }

        let fresh = cache.get(&key).is_some_and(|c| now.saturating_sub(c.at) < QUERY_TTL_SECS);

        if online && !fresh {
            missing.push(dep);
        }

        unique.push((dep, key));
    }

    let mut writer = CacheWriter::default();

    for chunk in missing.chunks(BATCH) {
        if Instant::now() >= deadline {
            return Err(Error::Failed("OSV check timed out".into()));
        }

        for (dep, result) in chunk.iter().zip(query_batch(chunk)?) {
            let vulns = result.vulns.into_iter().map(|v| (v.id, v.modified)).collect();
            cache.insert(query_key(dep), CachedQuery { at: now, vulns });
        }
    }

    let mut advisories: HashMap<String, Advisory> = HashMap::new();
    let mut online = online;
    let mut findings = Vec::new();
    let ignored = ignored_advisories();

    for (dep, key) in unique {
        let Some(query) = cache.get(&key) else {
            continue;
        };
        let mut found = Vec::new();

        for (id, modified) in &query.vulns {
            let advisory = advisories
                .entry(id.clone())
                .or_insert_with(|| advisory(id, modified, &mut online, deadline, &mut writer));
            if !is_ignored(&ignored, id, &advisory.aliases) {
                found.push(advisory.clone());
            }
        }

        if !found.is_empty() {
            findings.push(Finding { dep: dep.clone(), advisories: found });
        }
    }

    if !missing.is_empty() {
        cache.retain(|_, c| now.saturating_sub(c.at) < QUERY_TTL_SECS);
        writer.set("queries", &serde_json::to_string(&cache).unwrap_or_default());
    }

    Ok(findings)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::manifest::Ecosystem;

    pub(crate) const JACKSON: &str = r#"{"id": "GHSA-57j2-w4cx-62h2", "modified": "2024-03-15T05:00:00Z",
 "summary": "Deeply nested JSON exhausts the stack", "database_specific": {"severity": "HIGH"},
 "affected": [{"package": {"ecosystem": "Maven", "name": "com.fasterxml.jackson.core:jackson-databind"},
   "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.12.7.1"}]},
              {"type": "ECOSYSTEM", "events": [{"introduced": "2.13.0"}, {"fixed": "2.13.4.1"}]}]}]}"#;

    pub(crate) fn dep(ecosystem: Ecosystem, name: &str, version: &str) -> Dep {
        Dep { path: "/project/x".into(), ecosystem, name: name.into(), version: version.into(), at: (0, 0), version_at: None }
    }

    #[test]
    fn compares_versions_numerically() {
        assert_eq!(compare("1.10.0", "1.9.0"), Ordering::Greater);
        assert_eq!(compare("2.0.0-rc1", "2.0.0"), Ordering::Less);
        assert_eq!(compare("1.0", "1.0.1"), Ordering::Less);
        assert_eq!(compare("31.0-jre", "31.0-jre"), Ordering::Equal);
    }

    #[test]
    fn compare_survives_odd_input() {
        assert_eq!(compare("", ""), Ordering::Equal);
        assert_eq!(compare("", "1"), Ordering::Less);
        assert_eq!(compare("1..2", "1..2"), Ordering::Equal);
        assert_eq!(compare("1.007", "1.7"), Ordering::Equal);
        assert_eq!(compare("99999999999999999999999999.0", "99999999999999999999999998.0"), Ordering::Greater);
        assert_eq!(compare("99999999999999999999999", "9"), Ordering::Greater);
        assert_eq!(compare("1.é", "1.e"), Ordering::Greater);
    }

    #[test]
    fn git_ranges_and_incomplete_entries_add_no_fixed_versions() {
        let mixed = parse_advisory(
            r#"{"id": "PYSEC-1", "modified": "m", "affected": [{"package": {"ecosystem": "npm", "name": "x"},
 "ranges": [{"type": "GIT", "events": [{"introduced": "0"}, {"fixed": "0123456789abcdef0123456789abcdef01234567"}]},
            {"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "1.2.3"}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(mixed.fixed["npm:x"], ["1.2.3"]);

        let last_affected = parse_advisory(
            r#"{"id": "OSV-3", "modified": "m", "affected": [{"package": {"ecosystem": "npm", "name": "y"},
 "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"last_affected": "1.0.0"}]}]}, {"ranges": []}]}"#,
        )
        .unwrap();
        assert!(last_affected.fixed["npm:y"].is_empty());
        assert_eq!(last_affected.fixed.len(), 1);
        assert_eq!(level(Some("MODERATE")), Severity::Warning);
    }

    #[test]
    fn nearest_fix_is_the_smallest_fixed_version_above() {
        let fixed = vec!["2.9.10.8".to_string(), "2.12.7.1".into(), "2.13.4.1".into()];

        assert_eq!(nearest_fix("2.12.3", &fixed).as_deref(), Some("2.12.7.1"));
        assert_eq!(nearest_fix("2.14.0", &fixed), None);
    }

    #[test]
    fn parses_an_advisory() {
        let a = parse_advisory(JACKSON).unwrap();

        assert_eq!(a.severity.as_deref(), Some("HIGH"));
        assert_eq!(a.fixed["Maven:com.fasterxml.jackson.core:jackson-databind"], ["2.12.7.1", "2.13.4.1"]);
        assert_eq!(level(a.severity.as_deref()), Severity::Error);
    }

    #[test]
    fn severity_falls_back_to_ecosystem_specific_then_warning() {
        let a = parse_advisory(r#"{"id": "OSV-1", "modified": "m", "affected": [{"ecosystem_specific": {"severity": "low"}}]}"#).unwrap();
        assert_eq!(level(a.severity.as_deref()), Severity::Info);

        let b = parse_advisory(r#"{"id": "OSV-2", "modified": "m", "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N"}]}"#).unwrap();
        assert_eq!((b.severity, level(None)), (None, Severity::Warning));
    }

    #[test]
    fn a_finding_without_details_says_so_instead_of_claiming_no_fix() {
        let undetailed = Advisory { id: "GHSA-x".into(), ..Advisory::default() };
        let finding = Finding { dep: dep(Ecosystem::Npm, "lodash", "4.17.15"), advisories: vec![undetailed] };

        assert_eq!(finding.message(), "lodash 4.17.15 has known vulnerabilities: GHSA-x. Details unavailable; check again later.");
    }

    #[test]
    fn a_finding_upgrades_past_every_advisory_and_says_so() {
        let jackson = parse_advisory(JACKSON).unwrap();
        let older = Advisory {
            id: "GHSA-older".into(),
            severity: Some("MODERATE".into()),
            fixed: BTreeMap::from([("Maven:com.fasterxml.jackson.core:jackson-databind".into(), vec!["2.12.6.1".into()])]),
            ..Advisory::default()
        };
        let finding = Finding { dep: dep(Ecosystem::Maven, "com.fasterxml.jackson.core:jackson-databind", "2.12.3"), advisories: vec![jackson, older] };

        assert_eq!(finding.fix().as_deref(), Some("2.12.7.1"));
        assert_eq!(finding.severity(), Severity::Error);
        assert_eq!(
            finding.message(),
            "com.fasterxml.jackson.core:jackson-databind 2.12.3 has known vulnerabilities: GHSA-57j2-w4cx-62h2 (HIGH) Deeply nested JSON exhausts the stack; GHSA-older (MODERATE). Upgrade to 2.12.7.1."
        );
    }
}
