//! Kubernetes JSON schemas for YAML manifests. The IDE's YAML language server
//! (`yaml-language-server`) reads a `$schema` modeline and then completes,
//! validates and documents the manifest's keys.

use openidea_plugin::providers::{CodeAction, Document};
use openidea_plugin::{Position, Range, TextEdit};

const SCHEMAS: &str = "https://raw.githubusercontent.com/yannh/kubernetes-json-schema/master/master-standalone";
const MODELINE: &str = "# yaml-language-server: $schema=";

/// API groups of the built-in kinds the published schemas cover (custom
/// resources have none there).
const GROUPS: &[&str] = &[
    "",
    "admissionregistration.k8s.io",
    "apiextensions.k8s.io",
    "apps",
    "autoscaling",
    "batch",
    "certificates.k8s.io",
    "coordination.k8s.io",
    "discovery.k8s.io",
    "events.k8s.io",
    "networking.k8s.io",
    "node.k8s.io",
    "policy",
    "rbac.authorization.k8s.io",
    "scheduling.k8s.io",
    "storage.k8s.io",
];

/// The schema of `kind` in `apiVersion` (`apps/v1` → `deployment-apps-v1.json`).
pub fn url(api_version: &str, kind: &str) -> Option<String> {
    let (group, version) = api_version.rsplit_once('/').unwrap_or(("", api_version));
    if !GROUPS.contains(&group) || version.is_empty() || kind.is_empty() {
        return None;
    }
    let kind = kind.to_ascii_lowercase();
    let group = group.split('.').next().unwrap_or_default();
    if group.is_empty() {
        return Some(format!("{SCHEMAS}/{kind}-{version}.json"));
    }
    Some(format!("{SCHEMAS}/{kind}-{group}-{version}.json"))
}

/// The modeline for a single-document manifest of a known kind.
pub fn modeline(yaml: &str) -> Option<String> {
    if yaml.contains(MODELINE) {
        return None;
    }
    let mut api_version = None;
    let mut kind = None;
    let mut documents = 0;
    for line in yaml.lines() {
        if line.starts_with("---") {
            documents += 1;
            continue;
        }
        if let Some(v) = line.strip_prefix("apiVersion:") {
            api_version = Some(v.trim().trim_matches(['"', '\'']));
        }
        if let Some(v) = line.strip_prefix("kind:") {
            kind = Some(v.trim().trim_matches(['"', '\'']));
        }
    }
    // One schema per file: a second document separator means several kinds.
    if documents > 1 || (documents == 1 && !yaml.trim_start().starts_with("---")) {
        return None;
    }
    Some(format!("{MODELINE}{}\n", url(api_version?, kind?)?))
}

/// ⌥⏎ on a manifest: add the modeline at the top.
pub fn code_actions(doc: &Document) -> Vec<CodeAction> {
    let Some(line) = modeline(&doc.text) else {
        return Vec::new();
    };
    let top = Range::new(Position::new(0, 0), Position::new(0, 0));
    vec![CodeAction {
        title: "Add Kubernetes schema for completion and validation".into(),
        kind: Some("source".into()),
        preferred: false,
        edits: vec![(doc.path.clone(), vec![TextEdit { range: top, text: line }])],
        data: None,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_published_schema_file() {
        assert_eq!(url("v1", "Pod").unwrap(), format!("{SCHEMAS}/pod-v1.json"));
        assert_eq!(url("apps/v1", "Deployment").unwrap(), format!("{SCHEMAS}/deployment-apps-v1.json"));
        assert_eq!(url("networking.k8s.io/v1", "Ingress").unwrap(), format!("{SCHEMAS}/ingress-networking-v1.json"));
        assert_eq!(url("cert-manager.io/v1", "Certificate"), None);
    }

    #[test]
    fn a_modeline_only_for_one_document_without_one() {
        let pod = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: x\n";
        assert_eq!(modeline(pod).unwrap(), format!("{MODELINE}{SCHEMAS}/pod-v1.json\n"));
        assert!(modeline(&format!("---\n{pod}")).is_some());
        assert_eq!(modeline(&format!("{pod}---\n{pod}")), None);
        assert_eq!(modeline(&format!("{MODELINE}x\n{pod}")), None);
        assert_eq!(modeline("name: not a manifest\n"), None);
    }
}
