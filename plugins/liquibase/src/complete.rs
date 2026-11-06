//! Completion for YAML changelogs and Liquibase formatted SQL.

use crate::catalog::*;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use spring_text::config::yaml_parent;
use spring_text::range;
use spring_text::xml;

/// The line around byte `at`: its start and its end without trailing blanks.
fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, start + text[start..end].trim_end().len())
}

/// `[0]`, `[1]`, ... become `[]`.
fn without_indices(parent: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for c in parent.chars() {
        match c {
            '[' => {
                skipping = true;
                out.push('[');
            }
            ']' => {
                skipping = false;
                out.push(']');
            }
            _ if skipping => {}
            _ => out.push(c),
        }
    }
    out
}

/// The keys offered under `parent`, each with whether its value is a mapping.
fn keys_under(parent: &str) -> Vec<(&'static str, bool)> {
    let plain = |keys: &[&'static str]| keys.iter().map(|k| (*k, false)).collect();
    if parent.ends_with("databaseChangeLog[]") {
        return TOP_LEVEL.iter().map(|k| (*k, true)).collect();
    }
    if parent.ends_with(".changeSet") {
        return plain(CHANGE_SET);
    }
    if parent.ends_with(".changes[]") || parent.ends_with(".rollback[]") {
        return CHANGE_TYPES.iter().map(|(name, _)| (*name, true)).collect();
    }
    if parent.ends_with(".columns[]") {
        return vec![("column", true)];
    }
    if parent.ends_with(".column") {
        return COLUMN.iter().map(|k| (*k, *k == "constraints")).collect();
    }
    if parent.ends_with(".constraints") {
        return plain(CONSTRAINTS);
    }
    if parent.ends_with(".include") {
        return plain(INCLUDE);
    }
    if parent.ends_with(".includeAll") {
        return plain(INCLUDE_ALL);
    }
    if let Some((owner, name)) = parent.rsplit_once('.')
        && (owner.ends_with(".changes[]") || owner.ends_with(".rollback[]"))
        && let Some(attrs) = change_type(name)
    {
        return plain(attrs);
    }
    Vec::new()
}

/// Keys of a YAML changelog that fit where the caret is.
pub fn yaml(text: &str, at: usize) -> Vec<CompletionItem> {
    let (parent, typed) = yaml_parent(text, at);
    if typed.contains(':') || typed.starts_with('#') {
        return Vec::new();
    }
    let (line_start, end) = line_bounds(text, at);
    let typed_at = at - typed.len();
    let indent = typed_at - line_start;
    let rest = &text[at..end.max(at)];
    let word_end = at + rest.find(|c: char| c == ':' || c.is_whitespace()).unwrap_or(rest.len());
    let has_colon = text[word_end..end.max(word_end)].trim_start().starts_with(':');
    let replace = range(text, typed_at, word_end);
    let child = " ".repeat(indent + 2);

    keys_under(&without_indices(&parent))
        .into_iter()
        .map(|(key, nested)| {
            if has_colon {
                return CompletionItem { replace: Some(replace), ..CompletionItem::new(key, CompletionKind::Property) };
            }
            if key == "changeSet" {
                let i = &child;
                return CompletionItem {
                    insert_text: format!("changeSet:\n{i}id: ${{1:id}}\n{i}author: ${{2:author}}\n{i}changes:\n{i}  - $0"),
                    snippet: true,
                    replace: Some(replace),
                    ..CompletionItem::new(key, CompletionKind::Snippet)
                };
            }
            let (insert_text, kind) = if LISTS.contains(&key) {
                (format!("{key}:\n{child}- "), CompletionKind::Module)
            } else if nested {
                (format!("{key}:\n{child}"), CompletionKind::Module)
            } else {
                (format!("{key}: "), CompletionKind::Property)
            };
            CompletionItem { insert_text, replace: Some(replace), ..CompletionItem::new(key, kind) }
        })
        .collect()
}

/// `--liquibase formatted sql` on the first line and, in a changelog, the `--changeset`, `--rollback`
/// and `--comment` lines.
pub fn sql(text: &str, at: usize, changelog: bool) -> Vec<CompletionItem> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..at];
    let typed = before.trim_start();
    let letters = typed.strip_prefix("--").or_else(|| typed.strip_prefix('-'));
    if !letters.is_some_and(|l| l.bytes().all(|b| b.is_ascii_alphabetic())) {
        return Vec::new();
    }
    let replace = Some(range(text, at - typed.len(), at));
    let item = |label: &str, insert: &str, snippet: bool| CompletionItem {
        insert_text: insert.to_owned(),
        snippet,
        replace,
        filter_text: Some(label.to_owned()),
        ..CompletionItem::new(label, CompletionKind::Keyword)
    };
    let mut items = Vec::new();
    if line_start == 0 {
        items.push(item("--liquibase formatted sql", "--liquibase formatted sql", false));
    }
    if changelog {
        items.push(item("--changeset", "--changeset ${1:author}:${2:id}\n$0", true));
        items.push(item("--rollback", "--rollback ", false));
        items.push(item("--comment", "--comment: ", false));
    }
    items
}

const CHANGELOG_EXTENSIONS: [&str; 5] = ["xml", "yaml", "yml", "json", "sql"];

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Changelog files as an `include` would write them: beside `changelog` when `relative`, else from the classpath root
/// (`src/main/resources`) or the project root.
pub fn file_paths(changelog: &str, root: &str, relative: bool, files: &[String]) -> Vec<String> {
    let base = if relative {
        dir_of(changelog).to_owned()
    } else if let Some(i) = changelog.find("/src/main/resources/") {
        changelog[..i + "/src/main/resources".len()].to_owned()
    } else {
        root.trim_end_matches('/').to_owned()
    };
    let prefix = format!("{base}/");
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| f.as_str() != changelog)
        .filter(|f| f.rsplit_once('.').is_some_and(|(_, ext)| CHANGELOG_EXTENSIONS.contains(&ext)))
        .filter_map(|f| f.strip_prefix(&prefix))
        .map(str::to_owned)
        .collect();
    out.sort();
    out
}

/// Replaces the value `from..to` with each of `values`.
pub fn value_items(text: &str, from: usize, to: usize, values: &[String], kind: CompletionKind) -> Vec<CompletionItem> {
    let replace = range(text, from, to);
    values
        .iter()
        .map(|v| CompletionItem { replace: Some(replace), ..CompletionItem::new(v, kind) })
        .collect()
}

/// The `author` value being typed at `caret`, as its start and end: in YAML (`author: ...`), an XML `author="..."` or a
/// formatted SQL `--changeset ...` line.
pub fn author_value(path: &str, text: &str, caret: usize) -> Option<(usize, usize)> {
    if path.ends_with(".xml") {
        let elements = xml::elements(text);
        let element = &elements[xml::element_at(&elements, caret)?];
        let author = element.attr("author").filter(|a| a.holds(caret))?;
        return (element.local() == "changeSet").then_some((author.value_at, author.value_end));
    }
    let (line_start, _) = line_bounds(text, caret);
    let before = &text[line_start..caret];
    if path.ends_with(".sql") {
        let typed = before.trim_start().strip_prefix("--changeset")?.strip_prefix(' ')?;
        return (!typed.contains(':')).then_some((caret - typed.len(), caret));
    }
    let value = before.trim_start().trim_start_matches("- ").trim_start().strip_prefix("author:")?;
    let from = caret - value.trim_start().len();
    let (parent, _) = yaml_parent(text, line_start + before.len() - before.trim_start().len());
    parent.ends_with(".changeSet").then_some((from, caret))
}

fn xml_attributes(name: &str) -> Option<Vec<&'static str>> {
    let not_children = |keys: &[&'static str]| keys.iter().copied().filter(|k| !["changes", "rollback", "comment", "preConditions", "columns", "constraints"].contains(k)).collect();
    match name {
        "changeSet" => Some(not_children(CHANGE_SET)),
        "include" => Some(INCLUDE.to_vec()),
        "includeAll" => Some(INCLUDE_ALL.to_vec()),
        "column" => Some(not_children(COLUMN)),
        "constraints" => Some(CONSTRAINTS.to_vec()),
        _ => change_type(name).map(not_children),
    }
}

fn xml_children(name: &str) -> Vec<&'static str> {
    let types = || CHANGE_TYPES.iter().map(|(n, _)| *n);
    match name {
        "changeSet" => ["comment", "preConditions", "rollback", "validCheckSum"].into_iter().chain(types()).collect(),
        "rollback" => types().collect(),
        "column" => vec!["constraints"],
        _ if change_type(name).is_some_and(|attrs| attrs.contains(&"columns")) => vec!["column"],
        _ => Vec::new(),
    }
}

/// Element names after `<` and attribute names inside a start tag, for the Liquibase elements. Quiet outside a
/// `changeSet` and in files that declare a schema (the XML language server completes those).
pub fn xml(text: &str, caret: usize) -> Vec<CompletionItem> {
    let elements = xml::elements(text);
    if elements.first().is_some_and(|root| root.attrs.iter().any(|a| a.local() == "schemaLocation")) {
        return Vec::new();
    }
    let inside_change_set = |mut at: Option<usize>| {
        while let Some(i) = at {
            if elements[i].local() == "changeSet" {
                return true;
            }
            at = elements[i].parent;
        }
        false
    };

    let before = &text[..caret];
    let word_start = before.rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).map_or(0, |i| i + 1);
    if before[..word_start].ends_with('<') {
        let lt = word_start - 1;
        let Some(parent) = elements.iter().rposition(|e| e.tag_end <= lt && lt < e.end) else {
            return Vec::new();
        };
        if !inside_change_set(Some(parent)) {
            return Vec::new();
        }
        let word_end = caret + text[caret..].find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).unwrap_or(text.len() - caret);
        let replace = range(text, word_start, word_end);
        return xml_children(elements[parent].local())
            .into_iter()
            .map(|name| CompletionItem { replace: Some(replace), ..CompletionItem::new(name, CompletionKind::Module) })
            .collect();
    }

    let in_tag = |e: &xml::Element| e.start < caret && (caret < e.tag_end || caret == e.tag_end && !text[..e.tag_end].ends_with('>'));
    let Some(i) = elements.iter().rposition(in_tag) else {
        return Vec::new();
    };
    let element = &elements[i];
    if element.attrs.iter().any(|a| a.holds(caret) && a.value_at > a.at + a.name.len()) {
        return Vec::new();
    }
    if caret <= element.at + element.name.len() || !inside_change_set(Some(i)) {
        return Vec::new();
    }
    let Some(names) = xml_attributes(element.local()) else {
        return Vec::new();
    };
    let word_start = before.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    let word_end = caret + text[caret..].find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).unwrap_or(text.len() - caret);
    let followed_by_value = text[word_end..].trim_start().starts_with('=');
    let replace = range(text, word_start, word_end);
    names
        .into_iter()
        .filter(|name| !element.attrs.iter().any(|a| a.name == *name && !(a.at <= caret && caret <= a.at + a.name.len())))
        .map(|name| CompletionItem {
            insert_text: if followed_by_value { name.to_owned() } else { format!("{name}=\"$0\"") },
            snippet: !followed_by_value,
            replace: Some(replace),
            ..CompletionItem::new(name, CompletionKind::Property)
        })
        .collect()
}
