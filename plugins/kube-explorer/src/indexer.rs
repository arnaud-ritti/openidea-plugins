//! Cross-file references between manifests for the host's document index: a
//! resource's `metadata.name` defines `kube:<Kind>/<name>`, and the fields that
//! name another resource (`secretName`, `configMapKeyRef.name`, ...) refer to it.

use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use openidea_plugin::text::position;
use openidea_plugin::Range;

use crate::KubeExplorer;

const MAX_ANCHORS: usize = 4096;

/// Fields whose value is the name of a resource of that kind.
const NAMING_FIELDS: &[(&str, &str)] = &[
    ("secretName", "Secret"),
    ("serviceName", "Service"),
    ("claimName", "PersistentVolumeClaim"),
    ("serviceAccountName", "ServiceAccount"),
];

/// `name:` children of these keys name a resource of that kind.
const NAMING_PARENTS: &[(&str, &str)] = &[
    ("configMapKeyRef", "ConfigMap"),
    ("configMapRef", "ConfigMap"),
    ("configMap", "ConfigMap"),
    ("secretKeyRef", "Secret"),
    ("secretRef", "Secret"),
    ("service", "Service"),
];

fn key(kind: &str, name: &str) -> String {
    format!("kube:{kind}/{name}")
}

/// Defined at every reference to a resource, so a marker on the resource can jump to its users.
fn users_key(resource_key: &str) -> String {
    format!("kube-users:{}", resource_key.trim_start_matches("kube:"))
}

struct Value {
    text: String,
    start: usize,
}

fn value_of(line: &str, line_start: usize, after_colon: usize) -> Option<Value> {
    let rest = &line[after_colon..];
    let offset = after_colon + (rest.len() - rest.trim_start().len());
    let mut raw = line[offset..].trim_end();
    let mut start = offset;
    if let Some(quote) = raw.chars().next().filter(|c| matches!(c, '"' | '\'')) {
        raw = &raw[1..];
        start += 1;
        raw = raw.split(quote).next().unwrap_or(raw);
    } else if let Some((before, _)) = raw.split_once(" #") {
        raw = before.trim_end();
    }
    if raw.is_empty() || raw.contains("{{") || raw.contains("${") {
        return None;
    }
    Some(Value { text: raw.to_owned(), start: line_start + start })
}

#[derive(Default)]
struct Manifest {
    kind: Option<String>,
    name: Option<(String, Range)>,
    scale_kind: Option<String>,
    scale_name: Option<(String, Range)>,
    references: Vec<Anchor>,
}

fn range_of(text: &str, value: &Value) -> Range {
    Range::new(position(text, value.start), position(text, value.start + value.text.len()))
}

fn anchor(text: &str, key: String, value: &Value) -> Anchor {
    Anchor { key, range: range_of(text, value) }
}

fn flush(doc: Manifest, definitions: &mut Vec<Anchor>, references: &mut Vec<Anchor>) {
    if let (Some(kind), Some((name, range))) = (&doc.kind, doc.name) {
        definitions.push(Anchor { key: key(kind, &name), range });
    }
    if let (Some(kind), Some((name, range))) = (doc.scale_kind, doc.scale_name) {
        references.push(Anchor { key: key(&kind, &name), range });
    }
    references.extend(doc.references);
}

fn edges(text: &str) -> Edges {
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut doc = Manifest::default();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let body = line.trim_end_matches(['\r', '\n']);
        if body.starts_with("---") {
            flush(std::mem::take(&mut doc), &mut definitions, &mut references);
            stack.clear();
            continue;
        }
        let mut indent = body.len() - body.trim_start().len();
        let mut trimmed = body.trim_start();
        while let Some(rest) = trimmed.strip_prefix("- ") {
            indent += 2;
            trimmed = rest;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((field, _)) = trimmed.split_once(':') else { continue };
        if field.contains(' ') || field.starts_with(['"', '\'']) {
            continue;
        }
        while stack.last().is_some_and(|(at, _)| *at >= indent) {
            stack.pop();
        }
        let after_colon = body.len() - trimmed.len() + field.len() + 1;
        let value = value_of(body, line_start, after_colon);
        let parent = stack.last().map(|(at, key)| (*at, key.as_str()));
        if let Some(value) = &value {
            match (field, parent) {
                ("kind", None) => doc.kind = Some(value.text.clone()),
                ("name", Some((0, "metadata"))) => doc.name = Some((value.text.clone(), range_of(text, value))),
                ("kind", Some((_, "scaleTargetRef"))) => doc.scale_kind = Some(value.text.clone()),
                ("name", Some((_, "scaleTargetRef"))) => doc.scale_name = Some((value.text.clone(), range_of(text, value))),
                ("name", Some((_, parent))) => {
                    if let Some((_, kind)) = NAMING_PARENTS.iter().find(|(p, _)| *p == parent) {
                        doc.references.push(anchor(text, key(kind, &value.text), value));
                    }
                }
                _ => {
                    if let Some((_, kind)) = NAMING_FIELDS.iter().find(|(f, _)| *f == field) {
                        doc.references.push(anchor(text, key(kind, &value.text), value));
                    }
                }
            }
        } else {
            stack.push((indent, field.to_owned()));
        }
    }
    flush(doc, &mut definitions, &mut references);
    definitions.truncate(MAX_ANCHORS);
    references.truncate(MAX_ANCHORS);
    let mut markers: Vec<Anchor> = references.clone();
    markers.extend(definitions.iter().map(|d| Anchor { key: users_key(&d.key), range: d.range }));
    definitions.extend(references.iter().map(|r| Anchor { key: users_key(&r.key), range: r.range }));
    markers.truncate(MAX_ANCHORS);
    definitions.truncate(2 * MAX_ANCHORS);
    Edges { definitions, references, markers, ..Edges::default() }
}

impl DocumentIndexer for KubeExplorer {
    fn index(&mut self, doc: &Document) -> Edges {
        edges(&doc.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    #[test]
    fn metadata_name_defines_and_fields_reference() {
        let text = "apiVersion: v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  template:\n    spec:\n      serviceAccountName: runner\n      containers:\n        - name: app\n          envFrom:\n            - configMapRef:\n                name: \"app-config\"\n          env:\n            - name: A\n              valueFrom:\n                secretKeyRef:\n                  name: db\n                  key: pw\n      volumes:\n        - name: v\n          secret:\n            secretName: certs\n---\nkind: HorizontalPodAutoscaler\nmetadata:\n  name: web-hpa\nspec:\n  scaleTargetRef:\n    kind: Deployment\n    name: web\n";
        let found = edges(text);
        assert_eq!(keys(&found.definitions[..2]), ["kube:Deployment/web", "kube:HorizontalPodAutoscaler/web-hpa"]);
        assert_eq!(
            keys(&found.references),
            [
                "kube:ServiceAccount/runner",
                "kube:ConfigMap/app-config",
                "kube:Secret/db",
                "kube:Secret/certs",
                "kube:Deployment/web"
            ]
        );
        assert_eq!(found.definitions[0].range.start.line, 3);
        assert_eq!(found.definitions[0].range.start.character, 8);
    }

    #[test]
    fn markers_link_a_resource_to_its_users_and_back() {
        let found = edges("kind: Secret\nmetadata:\n  name: db\n---\nkind: Pod\nmetadata:\n  name: app\nspec:\n  containers:\n    - envFrom:\n        - secretRef:\n            name: db\n");
        assert_eq!(keys(&found.markers), ["kube:Secret/db", "kube-users:Secret/db", "kube-users:Pod/app"]);
        assert_eq!(keys(&found.definitions), ["kube:Secret/db", "kube:Pod/app", "kube-users:Secret/db"]);
        assert_eq!(found.markers[0].range.start.line, 11);
        assert_eq!(found.markers[1].range.start.line, 2);
        assert_eq!(found.definitions[2].range.start.line, 11);
    }

    #[test]
    fn a_manifest_without_references_marks_only_its_definitions() {
        let found = edges("kind: Service\nmetadata:\n  name: web\n");
        assert_eq!(keys(&found.markers), ["kube-users:Service/web"]);
        assert!(found.references.is_empty());
    }

    #[test]
    fn templated_names_and_container_names_are_ignored() {
        let found = edges("kind: Service\nmetadata:\n  name: {{ .Release.Name }}\nspec:\n  containers:\n    - name: app\n");
        assert!(found.definitions.is_empty());
        assert!(found.references.is_empty());
    }
}
