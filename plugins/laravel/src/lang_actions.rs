//! Generators that fill existing files: a lang key for a selected string, and
//! validation rules or `$fillable` entries for the columns of a model.

use std::ops::Range;

use openidea_plugin::providers::{CodeAction, Document};
use openidea_plugin::{Project, TextEdit, text};

use crate::actions::Rewrite;
use crate::index::Index;
use crate::php;
use crate::refs;

const LEFT_OUT_COLUMNS: &[&str] = &["id", "created_at", "updated_at", "deleted_at", "remember_token", "email_verified_at"];

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn slug(value: &str) -> String {
    let mut key = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
        } else if !key.ends_with('_') && !key.is_empty() {
            key.push('_');
        }
    }

    key.trim_end_matches('_').chars().take(40).collect::<String>().trim_end_matches('_').to_owned()
}

/// The edit adding `'key' => 'value',` before the closing `];` of a lang file.
pub fn entry_edit(file_text: &str, key: &str, value: &str) -> Option<TextEdit> {
    let closing = file_text.rfind("];").or_else(|| file_text.rfind(");"))?;
    let line_start = file_text[..closing].rfind('\n').map_or(0, |i| i + 1);
    let at = text::position(file_text, line_start);
    let quote = |s: &str| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"));

    Some(TextEdit { range: openidea_plugin::Range::new(at, at), text: format!("    {} => {},{}", quote(key), quote(value), eol_of(file_text)) })
}

fn selected_range(doc: &Document, selection: openidea_plugin::Range) -> Option<Range<usize>> {
    let start = text::offset(&doc.text, selection.start)?;
    let end = text::offset(&doc.text, selection.end)?;
    (start < end && !doc.text[start..end].contains('\n')).then_some(start..end)
}

fn inside_tag(text: &str, at: usize) -> bool {
    text[..at].rfind('<') > text[..at].rfind('>')
}

fn plain_words(selected: &str) -> bool {
    let trimmed = selected.trim();
    trimmed.len() >= 2
        && trimmed.len() <= 100
        && trimmed == selected
        && trimmed.chars().any(char::is_alphabetic)
        && !trimmed.contains(['{', '}', '<', '>', '@', '$', '"', '`', '\\', '%'])
}

/// Replaces the selected text with `__('file.key')` and adds the entry to the main locale's lang file.
pub fn extract_string(index: &Index, project: &Project, doc: &Document, language: &str, selection: openidea_plugin::Range) -> Option<CodeAction> {
    let range = selected_range(doc, selection)?;
    let selected = &doc.text[range.clone()];
    if !plain_words(selected) {
        return None;
    }

    let file_key = if index.translation_files.contains_key("messages") {
        "messages".to_owned()
    } else {
        index.translation_files.keys().find(|key| !key.contains(['/', ':']))?.clone()
    };
    let lang_file = index.translation_files.get(&file_key)?.first()?.clone();

    let replace = if language == "blade" {
        let regions = refs::blade_regions(&doc.text);
        if inside_tag(&doc.text, range.start) || regions.iter().any(|region| region.start < range.end && range.start < region.end) {
            return None;
        }
        range.clone()
    } else {
        let before = doc.text[..range.start].chars().next_back()?;
        let after = doc.text[range.end..].chars().next()?;
        if before != '\'' || after != '\'' {
            return None;
        }
        range.start - 1..range.end + 1
    };

    let base = slug(selected);
    if base.is_empty() {
        return None;
    }

    let mut key = base.clone();
    let mut counter = 2;
    while index.translations.contains_key(&format!("{file_key}.{key}")) {
        key = format!("{base}_{counter}");
        counter += 1;
    }

    let call = format!("__('{file_key}.{key}')");
    let new_text = if language == "blade" { format!("{{{{ {call} }}}}") } else { call };
    let lang_text = project.read_file(&lang_file).ok()?;
    let entry = entry_edit(&lang_text, &key, selected)?;

    Some(CodeAction {
        title: format!("Extract to translation key \"{file_key}.{key}\""),
        kind: Some("refactor.extract".into()),
        preferred: false,
        edits: vec![
            (doc.path.clone(), vec![TextEdit { range: crate::range_of(&doc.text, replace.start, replace.end), text: new_text }]),
            (lang_file, vec![entry]),
        ],
        data: None,
    })
}

fn rule_for(name: &str, kind: &str) -> String {
    let rules: &[&str] = match kind {
        "string" | "char" if name.contains("email") => &["required", "email", "max:255"],
        "string" | "char" => &["required", "string", "max:255"],
        "text" | "tinyText" | "mediumText" | "longText" => &["required", "string"],
        "boolean" => &["required", "boolean"],
        "date" | "dateTime" | "dateTimeTz" | "timestamp" | "timestampTz" => &["required", "date"],
        "float" | "double" | "decimal" | "unsignedDecimal" => &["required", "numeric"],
        "json" | "jsonb" => &["required", "array"],
        "uuid" | "ulid" => &["required", "uuid"],
        kind if kind.contains("nteger") || kind == "foreignId" => &["required", "integer"],
        _ => &["required"],
    };

    format!("[{}]", rules.iter().map(|rule| format!("'{rule}'")).collect::<Vec<_>>().join(", "))
}

fn indentation_of_line(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// The edit adding `lines` to the multi-line or empty array opened at `open`.
fn append_items(text: &str, open: usize, lines: &[String]) -> Option<(Range<usize>, String)> {
    let close = php::closing(text, open)?;
    let eol = eol_of(text);
    let items = php::items(text, open);
    let base = indentation_of_line(text, open);

    if items.is_empty() {
        let body = lines.iter().map(|line| format!("{base}    {line},")).collect::<Vec<_>>().join(eol);
        return Some((open + 1..close, format!("{eol}{body}{eol}{base}")));
    }

    if !text[open..close].contains('\n') {
        return None;
    }

    let first = items.first()?;
    let first_start = php::trimmed(text, first.key.clone().unwrap_or(first.value.clone())).start;
    let item_indent = indentation_of_line(text, first_start);
    let last = php::trimmed(text, items.last()?.value.clone()).end;
    let rest = &text[last..close];
    let comma = rest.trim_start().starts_with(',');
    let at = if comma { last + rest.find(',')? + 1 } else { last };
    let body: String = lines.iter().map(|line| format!("{eol}{item_indent}{line},")).collect();

    Some((at..at, if comma { body } else { format!(",{body}") }))
}

fn model_of_request(stem: &str) -> Option<String> {
    let name = stem.strip_suffix("Request")?;
    let name = ["Store", "Update", "Create", "Edit", "Destroy"].iter().find_map(|prefix| name.strip_prefix(prefix)).unwrap_or(name);
    (!name.is_empty()).then(|| name.to_owned())
}

pub fn column_rewrites(index: &Index, path: &str, text: &str, at: usize) -> Vec<Rewrite> {
    let masked = php::mask_comments(text);
    let mut found = Vec::new();
    found.extend(request_rules(index, path, text, &masked, at));
    found.extend(fillable_columns(index, path, text, &masked, at));
    found
}

fn request_rules(index: &Index, path: &str, text: &str, masked: &str, at: usize) -> Option<Rewrite> {
    if !masked.contains("extends FormRequest") {
        return None;
    }

    let stem = path.rsplit('/').next()?.strip_suffix(".php")?;
    let model = model_of_request(stem)?;
    let table = index.models.get(&model)?;
    let columns = index.columns.get(table)?;

    let function = masked.find("function rules(")?;
    let body_open = masked[function..].find('{').map(|i| function + i)?;
    let body_close = php::closing(masked, body_open)?;
    if at < function || at > body_close {
        return None;
    }

    let returned = masked[body_open..body_close].find("return")? + body_open;
    let open = masked[returned..body_close].find('[').map(|i| returned + i)?;
    let present: Vec<String> = php::items(masked, open).into_iter().filter_map(|item| php::literal(masked, item.key?).map(|(key, _)| key.to_owned())).collect();

    let lines: Vec<String> = columns
        .iter()
        .filter(|(name, _)| !LEFT_OUT_COLUMNS.contains(&name.as_str()) && !present.contains(name))
        .map(|(name, entry)| format!("'{name}' => {}", rule_for(name, entry.detail.as_deref().unwrap_or_default())))
        .collect();
    if lines.is_empty() {
        return None;
    }

    let edit = append_items(text, open, &lines)?;
    Some(Rewrite { title: format!("Add validation rules for the columns of {model}"), kind: "refactor.rewrite", edits: vec![edit] })
}

fn fillable_columns(index: &Index, path: &str, text: &str, masked: &str, at: usize) -> Option<Rewrite> {
    let table = index.model_files.get(path)?;
    let columns = index.columns.get(table)?;

    let declaration = masked.find("$fillable")?;
    let open = masked[declaration..].find('[').map(|i| declaration + i)?;
    let semicolon = masked[declaration..].find(';').map(|i| declaration + i)?;
    if open > semicolon || at < declaration || at > semicolon {
        return None;
    }

    let present: Vec<String> = php::strings(masked, open, false);
    let lines: Vec<String> = columns
        .keys()
        .filter(|name| !LEFT_OUT_COLUMNS.contains(&name.as_str()) && !present.contains(name))
        .map(|name| format!("'{name}'"))
        .collect();
    if lines.is_empty() {
        return None;
    }

    let edit = append_items(text, open, &lines)?;
    Some(Rewrite { title: "Add the missing columns to $fillable".into(), kind: "refactor.rewrite", edits: vec![edit] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_snake_case_and_short() {
        assert_eq!(slug("Welcome back, friend!"), "welcome_back_friend");
        assert_eq!(slug("  Hi  "), "hi");
        assert_eq!(slug("!!!"), "");
    }

    #[test]
    fn lang_entries_go_before_the_closing_bracket() {
        let file = "<?php\n\nreturn [\n    'a' => 'b',\n];\n";
        let edit = entry_edit(file, "k", "it's").unwrap();
        assert_eq!((edit.range.start.line, edit.text.as_str()), (4, "    'k' => 'it\\'s',\n"));
    }

    #[test]
    fn items_are_appended_to_empty_and_filled_arrays() {
        let empty = "return [\n];";
        let open = empty.find('[').unwrap();
        let (range, inserted) = append_items(empty, open, &["'a' => 1".into()]).unwrap();
        let mut out = empty.to_owned();
        out.replace_range(range, &inserted);
        assert_eq!(out, "return [\n    'a' => 1,\n];");

        let filled = "    return [\n        'x' => 1,\n        'y' => 2\n    ];";
        let open = filled.find('[').unwrap();
        let (range, inserted) = append_items(filled, open, &["'z' => 3".into()]).unwrap();
        let mut out = filled.to_owned();
        out.replace_range(range, &inserted);
        assert_eq!(out, "    return [\n        'x' => 1,\n        'y' => 2,\n        'z' => 3,\n    ];");

        assert!(append_items("[1, 2]", 0, &["3".into()]).is_none());
    }

    #[test]
    fn rules_follow_the_column_type() {
        assert_eq!(rule_for("title", "string"), "['required', 'string', 'max:255']");
        assert_eq!(rule_for("contact_email", "string"), "['required', 'email', 'max:255']");
        assert_eq!(rule_for("owner_id", "foreignId"), "['required', 'integer']");
        assert_eq!(rule_for("x", "weird"), "['required']");
    }

    #[test]
    fn request_names_map_to_models() {
        assert_eq!(model_of_request("StorePostRequest").as_deref(), Some("Post"));
        assert_eq!(model_of_request("PostRequest").as_deref(), Some("Post"));
        assert_eq!(model_of_request("Request"), None);
    }
}
