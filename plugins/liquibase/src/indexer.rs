//! Project-wide navigation for the host's document index: changeSets, changelog
//! files and tables as keys (`lb:cs:<id>::<author>`, `lb:file:<name>`, `lb:table:<name>`).

use crate::Liquibase;
use crate::changelog::{self, Changelog};
use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use spring_text::config;
use spring_text::range;
use spring_text::xml;

const MAX_ANCHORS: usize = 4096;
const TABLE_ATTRS: [&str; 3] = ["tableName", "baseTableName", "referencedTableName"];

fn changeset_key(id: &str, author: &str) -> String {
    format!("lb:cs:{id}::{author}")
}

fn file_key(path: &str) -> String {
    format!("lb:file:{}", path.rsplit(['/', '\\']).next().unwrap_or(path))
}

fn table_key(name: &str) -> String {
    format!("lb:table:{}", name.to_lowercase())
}

#[derive(Default)]
struct Found {
    definitions: Vec<Anchor>,
    references: Vec<Anchor>,
}

impl Found {
    fn define(&mut self, key: String, text: &str, from: usize, to: usize) {
        self.definitions.push(Anchor { key, range: range(text, from, to) });
    }

    fn refer(&mut self, key: String, text: &str, from: usize, to: usize) {
        self.references.push(Anchor { key, range: range(text, from, to) });
    }
}

fn xml_tables_and_rollbacks(text: &str, found: &mut Found) {
    for e in &xml::elements(text) {
        let creates = e.local() == "createTable";
        for attr in e.attrs.iter().filter(|a| TABLE_ATTRS.contains(&a.local()) && !a.value.is_empty()) {
            let key = table_key(&attr.value);
            if creates && attr.local() == "tableName" {
                found.define(key, text, attr.value_at, attr.value_end);
            } else {
                found.refer(key, text, attr.value_at, attr.value_end);
            }
        }
        if e.local() != "rollback" {
            continue;
        }
        let (Some(id), Some(author)) = (e.attr("changeSetId"), e.value("changeSetAuthor")) else { continue };
        found.refer(changeset_key(&id.value, author), text, id.value_at, id.value_end);
    }
}

fn yaml_tables_and_rollbacks(text: &str, found: &mut Found) {
    let keys = config::yaml(text);
    for key in keys.iter().filter(|k| !k.value.is_empty()) {
        let Some((parent, last)) = key.key.rsplit_once('.') else { continue };
        if TABLE_ATTRS.contains(&last) {
            let table = table_key(&key.value);
            if parent.ends_with("createTable") && last == "tableName" {
                found.define(table, text, key.value_at, key.value_end);
            } else {
                found.refer(table, text, key.value_at, key.value_end);
            }
        }
        if last != "changeSetId" {
            continue;
        }
        let author_path = format!("{parent}.changeSetAuthor");
        let Some(author) = keys.iter().find(|k| k.key == author_path) else { continue };
        found.refer(changeset_key(&key.value, &author.value), text, key.value_at, key.value_end);
    }
}

fn collect(path: &str, text: &str, log: &Changelog) -> Found {
    let mut found = Found::default();
    found.define(file_key(path), "", 0, 0);
    for set in &log.change_sets {
        if let (Some(id), Some(author)) = (&set.id, &set.author) {
            found.define(changeset_key(id, author), text, set.at, set.end);
        }
    }
    for include in log.includes.iter().filter(|i| !i.dir && !i.path.contains("${")) {
        found.refer(file_key(&include.path), text, include.at, include.end);
    }
    match log.format {
        changelog::Format::Xml => xml_tables_and_rollbacks(text, &mut found),
        changelog::Format::Yaml => yaml_tables_and_rollbacks(text, &mut found),
        changelog::Format::Sql => {}
    }
    found
}

impl DocumentIndexer for Liquibase {
    fn index(&mut self, doc: &Document) -> Edges {
        let Some(log) = changelog::parse(&doc.path, &doc.text) else { return Edges::default() };
        let mut found = collect(&doc.path, &doc.text, &log);
        found.definitions.truncate(MAX_ANCHORS);
        found.references.truncate(MAX_ANCHORS);
        Edges { definitions: found.definitions, references: found.references, aliases: Vec::new(), markers: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(path: &str, text: &str) -> Edges {
        Liquibase::default().index(&Document { path: path.into(), text: text.into(), symbols: Vec::new() })
    }

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    const XML: &str = "<databaseChangeLog>\n  <include file=\"db/changes/001.xml\"/>\n  <changeSet id=\"1\" author=\"bob\">\n    <createTable tableName=\"Users\"/>\n    <addColumn tableName=\"orders\"/>\n    <rollback changeSetId=\"0\" changeSetAuthor=\"ann\"/>\n  </changeSet>\n</databaseChangeLog>\n";

    #[test]
    fn xml_defines_changesets_files_and_tables() {
        let edges = index("/p/db/master.xml", XML);
        let defined = keys(&edges.definitions);
        assert_eq!(defined, ["lb:file:master.xml", "lb:cs:1::bob", "lb:table:users"]);
    }

    #[test]
    fn xml_references_include_table_and_rollback() {
        let edges = index("/p/db/master.xml", XML);
        assert_eq!(keys(&edges.references), ["lb:file:001.xml", "lb:table:orders", "lb:cs:0::ann"]);
        let include = &edges.references[0];
        assert_eq!((include.range.start.line, include.range.start.character), (1, 17));
    }

    #[test]
    fn yaml_defines_and_references() {
        let yaml = "databaseChangeLog:\n  - include:\n      file: db/a.yaml\n  - changeSet:\n      id: 2\n      author: bob\n      changes:\n        - createTable:\n            tableName: t\n        - dropTable:\n            tableName: old\n      rollback:\n        changeSetId: 1\n        changeSetAuthor: bob\n";
        let edges = index("/p/master.yaml", yaml);
        let defined = keys(&edges.definitions);
        assert!(defined.contains(&"lb:cs:2::bob") && defined.contains(&"lb:table:t"));
        let referenced = keys(&edges.references);
        for key in ["lb:file:a.yaml", "lb:table:old"] {
            assert!(referenced.contains(&key), "{key}");
        }
    }

    #[test]
    fn non_changelogs_publish_nothing() {
        assert!(index("/p/other.xml", "<project/>").definitions.is_empty());
        assert!(index("/p/x.json", "{}").references.is_empty());
    }
}
