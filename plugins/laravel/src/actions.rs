//! Rewrites of the string or echo under the caret, as byte-range edits.

use std::ops::Range;

use crate::php::{self, is_identifier};
use crate::refs::{self, Kind};

pub struct Rewrite {
    pub title: String,
    pub kind: &'static str,
    pub edits: Vec<(Range<usize>, String)>,
}

const CONTROLLERS_NAMESPACE: &str = "App\\Http\\Controllers";

pub fn rewrites(path: &str, language: &str, text: &str, at: usize) -> Vec<Rewrite> {
    let mut found = Vec::new();

    if path.ends_with(".blade.php") {
        found.extend(echo_toggle(text, at));
    }

    if let Some((kind, literal)) = refs::literal_at(language, text, at) {
        match kind {
            Kind::Rule => found.extend(rules_array(text, literal)),
            Kind::Action => found.extend(controller_array(text, literal)),
            _ => {}
        }
    }

    if let Some(open) = refs::rule_list_at(language, text, at) {
        found.extend(rules_string(text, open));
    }

    found
}

fn rules_string(text: &str, open: usize) -> Option<Rewrite> {
    let close = php::closing(text, open)?;
    if text.as_bytes()[open] != b'[' || text[open..close].contains("//") || text[open..close].contains("/*") {
        return None;
    }

    let mut rules = Vec::new();
    for item in php::items(text, open) {
        let (rule, _) = php::literal(text, item.value).filter(|_| item.key.is_none())?;
        let is_regex = rule.starts_with("regex:") || rule.starts_with("not_regex:");
        if rule.is_empty() || rule.contains(['|', '\'', '"', '\\', '$']) || is_regex && rule.contains(',') {
            return None;
        }
        rules.push(rule);
    }

    let quote = text[open + 1..close].trim_start().chars().next()?;
    Some(Rewrite {
        title: "Convert validation rules to a string".into(),
        kind: "refactor.rewrite",
        edits: vec![(open..close + 1, format!("{quote}{}{quote}", rules.join("|")))],
    })
}

fn rules_array(text: &str, literal: Range<usize>) -> Option<Rewrite> {
    let contents = &text[literal.clone()];
    let quote = text[..literal.start].chars().next_back()?;
    if contents.contains("regex:") || !contents.contains('|') || contents.contains(['\'', '"', '\\', '$']) {
        return None;
    }

    let items: Vec<_> = contents.split('|').collect();
    if items.iter().any(|item| item.trim().is_empty()) {
        return None;
    }

    let array = items.iter().map(|item| format!("{quote}{item}{quote}")).collect::<Vec<_>>().join(", ");
    Some(Rewrite {
        title: "Convert validation rules to an array".into(),
        kind: "refactor.rewrite",
        edits: vec![(literal.start - 1..literal.end + 1, format!("[{array}]"))],
    })
}

fn controller_array(text: &str, literal: Range<usize>) -> Option<Rewrite> {
    let (class, method) = text[literal.clone()].split_once('@')?;
    let is_name = |name: &str| !name.is_empty() && name.bytes().all(is_identifier) && !name.as_bytes()[0].is_ascii_digit();
    if !is_name(class) || !is_name(method) || !resolves(text, class) {
        return None;
    }

    let quote = text[..literal.start].chars().next_back()?;
    Some(Rewrite {
        title: "Convert controller string to a class array".into(),
        kind: "refactor.rewrite",
        edits: vec![(literal.start - 1..literal.end + 1, format!("[{class}::class, {quote}{method}{quote}]"))],
    })
}

/// `Class::class` already names the controller: the file is in its namespace or imports it.
fn resolves(text: &str, class: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if let Some(namespace) = line.strip_prefix("namespace ") {
            return namespace.trim_end_matches(';').trim() == CONTROLLERS_NAMESPACE;
        }

        let Some(import) = line.strip_prefix("use ").and_then(|rest| rest.strip_suffix(';')) else { return false };
        import.rsplit('\\').next() == Some(class) && !import.contains(['{', ' ', ','])
    })
}

fn echo_toggle(text: &str, at: usize) -> Option<Rewrite> {
    let region = refs::blade_regions(text).into_iter().find(|r| r.start <= at && at <= r.end)?;
    let echo = &text[region.clone()];
    if text[..region.start].ends_with('@') {
        return None;
    }

    let (open, close, new_open, new_close, title) = if echo.starts_with("{{") && echo.ends_with("}}") {
        ("{{", "}}", "{!!", "!!}", "Output unescaped: {!! !!}")
    } else if echo.starts_with("{!!") && echo.ends_with("!!}") {
        ("{!!", "!!}", "{{", "}}", "Output escaped: {{ }}")
    } else {
        return None;
    };

    if echo.len() < open.len() + close.len() || echo.starts_with("{{--") {
        return None;
    }

    Some(Rewrite {
        title: title.into(),
        kind: "refactor.rewrite",
        edits: vec![
            (region.start..region.start + open.len(), new_open.into()),
            (region.end - close.len()..region.end, new_close.into()),
        ],
    })
}
