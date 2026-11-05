//! OSV advisories: parsing, version comparison and per-dependency findings.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use openidea_plugin::Severity;
use serde::{Deserialize, Serialize};

use crate::manifest::Dep;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Advisory {
    pub id: String,
    pub modified: String,
    pub summary: String,
    pub severity: Option<String>,
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
        let events = package["ranges"].as_array().into_iter().flatten().flat_map(|r| r["events"].as_array().into_iter().flatten());
        versions.extend(events.filter_map(|e| text(&e["fixed"])));
    }

    Some(Advisory {
        id: text(&value["id"])?,
        modified: text(&value["modified"]).unwrap_or_default(),
        summary: text(&value["summary"]).unwrap_or_default(),
        severity,
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
        let outcome = match self.fix() {
            Some(fix) => format!("Upgrade to {fix}."),
            None => "No fixed version is known.".to_string(),
        };

        format!("{} {} has known vulnerabilities: {advisories}. {outcome}", self.dep.name, self.dep.version)
    }
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
