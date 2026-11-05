//! Completion for YAML changelogs and Liquibase formatted SQL.

use crate::catalog::*;
use openidea_plugin::providers::{CompletionItem, CompletionKind};
use spring_text::config::yaml_parent;
use spring_text::range;

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
