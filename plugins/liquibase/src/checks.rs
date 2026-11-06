//! Problems in a changelog: duplicate or incomplete changeSets, unknown change types,
//! includes that point nowhere, and a missing formatted SQL header.

use crate::catalog::is_change_type;
use crate::changelog::{Changelog, Format, Include, Target};
use openidea_plugin::{Diagnostic, Location, RelatedInformation, Severity};
use spring_text::range;
use std::collections::HashMap;

/// The problems of `log` (parsed from `text`), sorted by where they start.
/// `defined` says whether the project can supply a property the changelog does not declare (`true` when unsure); `None` turns the property check off.
pub fn problems(
    path: &str,
    text: &str,
    log: &Changelog,
    target: &dyn Fn(&Include) -> Target,
    defined: Option<&dyn Fn(&str) -> bool>,
) -> Vec<Diagnostic> {
    let mut out: Vec<(usize, Diagnostic)> = Vec::new();

    if log.format == Format::Sql && !log.header {
        let end = text.find('\n').unwrap_or(text.len());
        let message = "Formatted SQL changelogs start with `--liquibase formatted sql`.";
        out.push((0, Diagnostic::new(range(text, 0, end), Severity::Warning, message).code("missing-header")));
    }

    let mut first: HashMap<(&str, &str), (usize, usize)> = HashMap::new();
    for change_set in &log.change_sets {
        let span = range(text, change_set.at, change_set.end);
        let (Some(id), Some(author)) = (&change_set.id, &change_set.author) else {
            let mut message = match (&change_set.id, &change_set.author) {
                (None, Some(_)) => "Changeset has no id.",
                (Some(_), None) => "Changeset has no author.",
                _ => "Changeset has no id and no author.",
            }
            .to_owned();
            if log.format == Format::Sql {
                message.push_str(" Write `--changeset author:id`.");
            }
            let diagnostic = Diagnostic::new(span, Severity::Error, message).code("incomplete-changeset");
            out.push((change_set.at, diagnostic));
            continue;
        };
        let Some(&(from, to)) = first.get(&(id.as_str(), author.as_str())) else {
            first.insert((id, author), (change_set.at, change_set.end));
            continue;
        };
        let message = format!("Changeset `{id}` by `{author}` is already defined in this file.");
        let mut diagnostic = Diagnostic::new(span, Severity::Error, message).code("duplicate-changeset");
        diagnostic.related.push(RelatedInformation {
            location: Location { path: path.to_owned(), range: range(text, from, to) },
            message: "First definition".into(),
        });
        out.push((change_set.at, diagnostic));
    }

    for change in &log.changes {
        if is_change_type(&change.name) {
            continue;
        }
        let message = format!("`{}` is not a built-in Liquibase change type.", change.name);
        let diagnostic = Diagnostic::new(range(text, change.at, change.end), Severity::Warning, message);
        out.push((change.at, diagnostic.code("unknown-change-type")));
    }

    for include in &log.includes {
        if include.optional || target(include) != Target::Missing {
            continue;
        }
        let message = if include.dir {
            format!("Cannot find directory `{}`.", include.path)
        } else {
            format!("Cannot find `{}`.", include.path)
        };
        let diagnostic = Diagnostic::new(range(text, include.at, include.end), Severity::Error, message);
        out.push((include.at, diagnostic.code("unresolved-include")));
    }

    if let Some(defined) = defined {
        for (start, end, name) in placeholders(text, log.format) {
            if name.starts_with("database.") || log.properties.iter().any(|p| p == name) || defined(name) {
                continue;
            }
            let message = format!("Property `{name}` is not declared in this changelog or the changelogs it includes, nor set as a parameter in the project's Liquibase configuration.");
            out.push((start, Diagnostic::new(range(text, start, end), Severity::Warning, message).code("unresolved-property")));
        }
    }

    out.sort_by_key(|(start, _)| *start);
    out.into_iter().map(|(_, diagnostic)| diagnostic).collect()
}

/// `${name}` placeholders as `(start, end, name)`; names with other characters (`${:name}`, nested) are dynamic and skipped.
/// YAML comment lines are skipped.
fn placeholders(text: &str, format: Format) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("${") {
        let Some(close) = text[at..].find('}') else {
            continue;
        };
        let name = &text[at + 2..at + close];
        let plain = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'));
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        if !plain || (format == Format::Yaml && text[line_start..at].trim_start().starts_with('#')) {
            continue;
        }
        out.push((at, at + close + 1, name));
    }
    out
}
