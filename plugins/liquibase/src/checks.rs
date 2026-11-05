//! Problems in a changelog: duplicate or incomplete changeSets, unknown change types,
//! includes that point nowhere, and a missing formatted SQL header.

use crate::catalog::is_change_type;
use crate::changelog::{Changelog, Format, Include, Target};
use openidea_plugin::{Diagnostic, Location, RelatedInformation, Severity};
use spring_text::range;
use std::collections::HashMap;

/// The problems of `log` (parsed from `text`), sorted by where they start.
pub fn problems(path: &str, text: &str, log: &Changelog, target: &dyn Fn(&Include) -> Target) -> Vec<Diagnostic> {
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
        if target(include) != Target::Missing {
            continue;
        }
        let message = if include.dir {
            format!("No files under `{}`.", include.path)
        } else {
            format!("Cannot find `{}`.", include.path)
        };
        let diagnostic = Diagnostic::new(range(text, include.at, include.end), Severity::Error, message);
        out.push((include.at, diagnostic.code("unresolved-include")));
    }

    out.sort_by_key(|(start, _)| *start);
    out.into_iter().map(|(_, diagnostic)| diagnostic).collect()
}
