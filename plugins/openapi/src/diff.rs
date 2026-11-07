//! A semantic comparison of two specs: operations, schemas and other top-level
//! sections that were added, removed or changed between a base and a revision.

use crate::spec::{self, Key};
use openidea_plugin::text::position;
use std::collections::BTreeMap;

pub struct Change {
    pub label: String,
    pub detail: String,
    /// `line:column:path` of the group in the file that still has it.
    pub location: String,
}

struct Group<'k> {
    label: String,
    values: BTreeMap<&'k str, &'k str>,
    at: usize,
}

fn group_of(key: &str, operation_keys: &[String]) -> String {
    if let Some(operation) = operation_keys.iter().find(|op| key == op.as_str() || key.strip_prefix(op.as_str()).is_some_and(|rest| rest.starts_with('.'))) {
        return operation.clone();
    }
    let take = if key.starts_with("components.") { 3 } else if key.starts_with("definitions.") { 2 } else { 1 };
    key.splitn(take + 1, '.').take(take).collect::<Vec<_>>().join(".")
}

fn groups<'k>(text: &str, keys: &'k [Key]) -> BTreeMap<String, Group<'k>> {
    let operations = spec::operations(text, keys);
    let operation_keys: Vec<String> = operations.iter().map(|o| o.key.clone()).collect();
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for key in keys {
        let name = group_of(&key.key, &operation_keys);
        let label = operations
            .iter()
            .find(|o| o.key == name)
            .map_or_else(|| name.clone(), |o| format!("{} {}", o.method, o.path));
        let group = groups.entry(name).or_insert_with(|| Group { label, values: BTreeMap::new(), at: key.at });
        if !key.value.is_empty() {
            group.values.insert(&key.key, &key.value);
        }
    }
    groups
}

fn location(text: &str, at: usize, path: &str) -> String {
    let at = position(text, at);
    format!("{}:{}:{path}", at.line, at.character)
}

fn counts(base: &BTreeMap<&str, &str>, revision: &BTreeMap<&str, &str>) -> String {
    let added = revision.keys().filter(|k| !base.contains_key(*k)).count();
    let removed = base.keys().filter(|k| !revision.contains_key(*k)).count();
    let changed = revision.iter().filter(|(k, v)| base.get(*k).is_some_and(|b| b != *v)).count();
    [(added, "added"), (removed, "removed"), (changed, "changed")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn compare(base_path: &str, base: &str, revision_path: &str, revision: &str) -> Vec<Change> {
    let base_keys = spec::scan(base);
    let revision_keys = spec::scan(revision);
    let before = groups(base, &base_keys);
    let after = groups(revision, &revision_keys);
    let mut out = Vec::new();
    for (name, group) in &before {
        if !after.contains_key(name) {
            out.push(Change { label: format!("- {}", group.label), detail: "removed".into(), location: location(base, group.at, base_path) });
        }
    }
    for (name, group) in &after {
        let location = location(revision, group.at, revision_path);
        match before.get(name) {
            None => out.push(Change { label: format!("+ {}", group.label), detail: "added".into(), location }),
            Some(old) => {
                let detail = counts(&old.values, &group.values);
                if !detail.is_empty() {
                    out.push(Change { label: format!("~ {}", group.label), detail, location });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "openapi: 3.0.0\npaths:\n  /pets:\n    get:\n      summary: List\n    post:\n      summary: Create\n  /old:\n    get:\n      summary: Gone\ncomponents:\n  schemas:\n    Pet:\n      type: object\n";
    const REVISION: &str = "openapi: 3.0.0\npaths:\n  /pets:\n    get:\n      summary: List all\n      deprecated: true\n  /new:\n    get:\n      summary: Fresh\ncomponents:\n  schemas:\n    Pet:\n      type: object\n    Owner:\n      type: object\n";

    #[test]
    fn reports_added_removed_and_changed_groups() {
        let changes = compare("/a.yaml", BASE, "/b.yaml", REVISION);
        let summary: Vec<_> = changes.iter().map(|c| (c.label.as_str(), c.detail.as_str())).collect();
        assert_eq!(
            summary,
            [
                ("- GET /old", "removed"),
                ("- POST /pets", "removed"),
                ("+ components.schemas.Owner", "added"),
                ("+ GET /new", "added"),
                ("~ GET /pets", "1 added, 1 changed"),
            ]
        );
        assert_eq!(changes[1].location, "5:4:/a.yaml");
        assert_eq!(changes[3].location, "7:4:/b.yaml");
    }

    #[test]
    fn identical_specs_have_no_changes() {
        assert!(compare("/a.yaml", BASE, "/b.yaml", BASE).is_empty());
    }
}
