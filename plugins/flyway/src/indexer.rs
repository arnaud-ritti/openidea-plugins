//! Project-wide navigation for the host's document index: tables between JPA
//! entities and migrations (`fw:table:<t>`, `fw:entity:<t>`) and `${placeholders}`
//! (`fw:ph:<name>`).

use crate::Flyway;
use crate::config::is_flyway_config;
use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use spring_text::config::properties;
use spring_text::range;

const MAX_ANCHORS: usize = 4096;
const PLACEHOLDERS_KEY: &str = "flyway.placeholders.";

fn table_key(name: &str) -> String {
    format!("fw:table:{}", name.to_lowercase())
}

fn entity_key(table: &str) -> String {
    format!("fw:entity:{}", table.to_lowercase())
}

fn placeholder_key(name: &str) -> String {
    format!("fw:ph:{name}")
}

fn anchor(key: String, text: &str, from: usize, to: usize) -> Anchor {
    Anchor { key, range: range(text, from, to) }
}

fn is_migration(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.starts_with(['V', 'U', 'R']) && name.contains("__") && name.ends_with(".sql")
}

fn is_identifier(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn skip_whitespace(text: &str, at: usize) -> usize {
    at + (text[at..].len() - text[at..].trim_start().len())
}

fn created_tables(text: &str) -> Vec<(String, usize, usize)> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = lower[from..].find("create table") {
        let mut at = from + found + "create table".len();
        from = at;
        if !lower[at..].starts_with(char::is_whitespace) {
            continue;
        }
        at = skip_whitespace(&lower, at);
        if lower[at..].starts_with("if not exists") {
            at = skip_whitespace(&lower, at + "if not exists".len());
        }
        let quoted = |c: char| matches!(c, '"' | '`' | '[' | ']');
        let len = text[at..].find(|c: char| !(is_identifier(c) || c == '.' || quoted(c))).unwrap_or(text.len() - at);
        let token = &text[at..at + len];
        let name = token.rsplit('.').next().unwrap_or(token);
        let bare = name.trim_matches(quoted);
        if bare.is_empty() {
            continue;
        }
        let start = at + len - name.len() + (name.len() - name.trim_start_matches(quoted).len());
        out.push((bare.to_owned(), start, start + bare.len()));
    }
    out
}

fn sql_edges(path: &str, text: &str) -> Edges {
    let mut edges = Edges::default();
    if !is_migration(path) {
        return edges;
    }
    for (table, from, to) in created_tables(text) {
        edges.definitions.push(anchor(table_key(&table), text, from, to));
        edges.markers.push(anchor(entity_key(&table), text, from, to));
    }
    let mut from = 0;
    while let Some(found) = text[from..].find("${") {
        let start = from + found + 2;
        from = start;
        let Some(len) = text[start..].find('}') else { break };
        let name = text[start..start + len].trim();
        if name.is_empty() || name.contains(':') || name.contains(char::is_whitespace) {
            continue;
        }
        edges.references.push(anchor(placeholder_key(name), text, start, start + len));
    }
    edges
}

fn config_edges(path: &str, text: &str) -> Edges {
    let mut edges = Edges::default();
    if path.ends_with(".conf") {
        for entry in properties(text) {
            if let Some(name) = entry.key.strip_prefix(PLACEHOLDERS_KEY) {
                edges.definitions.push(anchor(placeholder_key(name), text, entry.at, entry.end));
            }
        }
        return edges;
    }
    let mut in_table = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let body = line.trim();
        if body.starts_with('[') {
            in_table = matches!(body.trim_matches(['[', ']']).trim(), "flyway.placeholders" | "placeholders");
            continue;
        }
        let Some((name, _)) = body.split_once('=').filter(|_| in_table) else { continue };
        let name = name.trim().trim_matches('"');
        let at = start + line.find(name).unwrap_or(0);
        edges.definitions.push(anchor(placeholder_key(name), text, at, at + name.len()));
    }
    edges
}

fn snake_case(class: &str) -> String {
    let mut out = String::new();
    for (i, c) in class.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push('_');
        }
        out.extend(c.to_lowercase());
    }
    out
}

fn table_attribute(block: &str) -> Option<String> {
    let args = &block[block.find("@Table")? + "@Table".len()..];
    let after = &args[args.find("name")? + "name".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
    Some(after[..after.find('"')?].to_owned())
}

fn entity_edges(text: &str) -> Edges {
    let mut edges = Edges::default();
    let mut from = 0;
    while let Some(found) = text[from..].find("class ") {
        let at = from + found;
        from = at + "class ".len();
        if at > 0 && text[..at].ends_with(is_identifier) {
            continue;
        }
        let name_len = text[from..].find(|c: char| !is_identifier(c)).unwrap_or(text.len() - from);
        if name_len == 0 {
            continue;
        }
        let block_start = text[..at].rfind(['}', ';']).map_or(0, |i| i + 1);
        let block = &text[block_start..at];
        if !block.contains("@Entity") {
            continue;
        }
        let name = &text[from..from + name_len];
        let table = table_attribute(block).unwrap_or_else(|| snake_case(name));
        edges.definitions.push(anchor(entity_key(&table), text, from, from + name_len));
        edges.references.push(anchor(table_key(&table), text, from, from + name_len));
        edges.markers.push(anchor(table_key(&table), text, from, from + name_len));
    }
    edges
}

impl DocumentIndexer for Flyway {
    fn index(&mut self, doc: &Document) -> Edges {
        let mut edges = if doc.path.ends_with(".sql") {
            sql_edges(&doc.path, &doc.text)
        } else if is_flyway_config(&doc.path) {
            config_edges(&doc.path, &doc.text)
        } else if doc.path.ends_with(".java") || doc.path.ends_with(".kt") {
            entity_edges(&doc.text)
        } else {
            Edges::default()
        };
        edges.definitions.truncate(MAX_ANCHORS);
        edges.references.truncate(MAX_ANCHORS);
        edges.markers.truncate(MAX_ANCHORS);
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(path: &str, text: &str) -> Edges {
        Flyway::default().index(&Document { path: path.into(), text: text.into(), symbols: Vec::new() })
    }

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    #[test]
    fn migration_defines_tables_and_references_placeholders() {
        let sql = "CREATE TABLE IF NOT EXISTS public.\"User_Account\" (id int);\ninsert into t values ('${seed}', '${flyway:timestamp}');\n";
        let edges = index("/p/db/migration/V1__init.sql", sql);
        assert_eq!(keys(&edges.definitions), ["fw:table:user_account"]);
        assert_eq!(keys(&edges.markers), ["fw:entity:user_account"]);
        assert_eq!(keys(&edges.references), ["fw:ph:seed"]);
        let table = &edges.definitions[0].range;
        assert_eq!((table.start.line, table.start.character, table.end.character), (0, 35, 47));
    }

    #[test]
    fn plain_sql_is_not_indexed() {
        assert!(index("/p/query.sql", "create table t (id int);").definitions.is_empty());
    }

    #[test]
    fn entity_marks_its_table_by_annotation_or_class_name() {
        let java = "@Entity\n@Table(name = \"accounts\")\nclass Account {}\n\n@Entity\nclass UserProfile {}\n\nclass Plain {}\n";
        let edges = index("/p/A.java", java);
        assert_eq!(keys(&edges.definitions), ["fw:entity:accounts", "fw:entity:user_profile"]);
        assert_eq!(keys(&edges.markers), ["fw:table:accounts", "fw:table:user_profile"]);
        assert_eq!(keys(&edges.references), keys(&edges.markers));
    }

    #[test]
    fn config_files_define_placeholders() {
        let conf = index("/p/flyway.conf", "flyway.url=x\nflyway.placeholders.seed=1\n");
        assert_eq!(keys(&conf.definitions), ["fw:ph:seed"]);
        let toml = index("/p/flyway.toml", "[flyway]\nx = 1\n[flyway.placeholders]\nseed = \"1\"\nother = 2\n");
        assert_eq!(keys(&toml.definitions), ["fw:ph:seed", "fw:ph:other"]);
        assert_eq!(toml.definitions[0].range.start.line, 3);
    }
}
