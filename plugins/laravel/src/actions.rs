//! Rewrites of the string or echo under the caret, as byte-range edits.

use std::ops::Range;

use crate::php::{self, is_identifier};
use crate::refs::{self, Kind};

pub struct Rewrite {
    pub title: String,
    pub kind: &'static str,
    pub edits: Vec<(Range<usize>, String)>,
}

const FACADES: &[&str] = &[
    "App", "Artisan", "Auth", "Blade", "Broadcast", "Bus", "Cache", "Config", "Context", "Cookie", "Crypt", "Date", "DB", "Event", "File", "Gate", "Hash", "Http", "Lang",
    "Log", "Mail", "Notification", "Password", "Process", "Queue", "RateLimiter", "Redirect", "Request", "Response", "Route", "Schema", "Session", "Storage",
    "URL", "Validator", "View", "Vite",
];

const SUPPORT_HELPERS: &[&str] = &["Str", "Arr", "Number", "Js", "Uri"];

pub fn rewrites(path: &str, language: &str, text: &str, at: usize, controllers_namespace: &str) -> Vec<Rewrite> {
    let mut found = Vec::new();

    if path.ends_with(".blade.php") {
        found.extend(echo_toggle(text, at));
        found.extend(class_directive(text, at));
    }

    if let Some((kind, literal)) = refs::literal_at(language, text, at) {
        match kind {
            Kind::Rule => {
                found.extend(rules_array(text, literal.clone()));
                found.extend(rules_all_arrays(text, literal));
            }
            Kind::Action => {
                found.extend(controller_array(text, literal, controllers_namespace));
                found.extend(controller_calls_all(language, text, at, controllers_namespace));
            }
            Kind::Controller => {
                found.extend(controller_class(text, literal, controllers_namespace));
                found.extend(controller_calls_all(language, text, at, controllers_namespace));
            }
            _ => {}
        }
    }

    if language == "php" {
        found.extend(rule_object(language, text, at));
        found.extend(root_facade(text, at));
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

fn array_edit(text: &str, literal: Range<usize>) -> Option<(Range<usize>, String)> {
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
    Some((literal.start - 1..literal.end + 1, format!("[{array}]")))
}

fn rules_array(text: &str, literal: Range<usize>) -> Option<Rewrite> {
    Some(Rewrite { title: "Convert validation rules to an array".into(), kind: "refactor.rewrite", edits: vec![array_edit(text, literal)?] })
}

fn enclosing_bracket(text: &str, before: usize) -> Option<usize> {
    let masked = php::mask_comments(&text[..before]);
    let bytes = masked.as_bytes();
    let mut open = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                i = php::string_end(bytes, i).max(i + 1);
                continue;
            }
            b'(' | b'[' | b'{' => open.push(i),
            b')' | b']' | b'}' => {
                open.pop();
            }
            _ => {}
        }
        i += 1;
    }

    open.pop()
}

fn rules_all_arrays(text: &str, literal: Range<usize>) -> Option<Rewrite> {
    let open = enclosing_bracket(text, literal.start)?;
    if text.as_bytes()[open] != b'[' {
        return None;
    }

    let edits: Vec<_> = php::items(text, open)
        .into_iter()
        .filter_map(|item| {
            let (contents, start) = php::literal(text, item.value)?;
            array_edit(text, start..start + contents.len())
        })
        .collect();

    if edits.len() < 2 {
        return None;
    }

    Some(Rewrite { title: "Convert all validation rules in this array to arrays".into(), kind: "refactor.rewrite", edits })
}

fn controller_edit(text: &str, kind: Kind, literal: Range<usize>, namespace: &str) -> Option<(Range<usize>, String)> {
    let is_name = |name: &str| !name.is_empty() && name.bytes().all(is_identifier) && !name.as_bytes()[0].is_ascii_digit();
    let quote = text[..literal.start].chars().next_back()?;
    let replaced = literal.start - 1..literal.end + 1;
    let contents = &text[literal];

    if kind == Kind::Controller {
        return (is_name(contents) && resolves(text, contents, namespace)).then(|| (replaced, format!("{contents}::class")));
    }

    let (class, method) = contents.split_once('@')?;
    (is_name(class) && is_name(method) && resolves(text, class, namespace)).then(|| (replaced, format!("[{class}::class, {quote}{method}{quote}]")))
}

fn controller_array(text: &str, literal: Range<usize>, namespace: &str) -> Option<Rewrite> {
    Some(Rewrite {
        title: "Convert controller string to a class array".into(),
        kind: "refactor.rewrite",
        edits: vec![controller_edit(text, Kind::Action, literal, namespace)?],
    })
}

fn controller_class(text: &str, literal: Range<usize>, namespace: &str) -> Option<Rewrite> {
    Some(Rewrite {
        title: "Convert controller string to ::class".into(),
        kind: "refactor.rewrite",
        edits: vec![controller_edit(text, Kind::Controller, literal, namespace)?],
    })
}

fn controller_calls_all(language: &str, text: &str, at: usize, namespace: &str) -> Option<Rewrite> {
    let converted: Vec<_> = refs::references(language, text)
        .into_iter()
        .filter(|reference| reference.closed && matches!(reference.kind, Kind::Action | Kind::Controller))
        .filter_map(|reference| controller_edit(text, reference.kind, reference.start..reference.end, namespace))
        .collect();

    if converted.len() < 2 || !converted.iter().any(|(range, _)| range.start <= at && at <= range.end) {
        return None;
    }

    Some(Rewrite {
        title: "Convert all controller strings in this file".into(),
        kind: "refactor.rewrite",
        edits: converted,
    })
}

fn rule_object(language: &str, text: &str, at: usize) -> Option<Rewrite> {
    let open = refs::rule_list_at(language, text, at)?;
    let (contents, start) = php::items(text, open)
        .into_iter()
        .filter(|item| item.key.is_none())
        .filter_map(|item| php::literal(text, item.value))
        .find(|(contents, start)| *start <= at && at <= start + contents.len())?;

    let (name, arguments) = contents.split_once(':')?;
    if arguments.is_empty() || contents.contains(['|', '\'', '"', '\\', '$', ' ']) {
        return None;
    }

    let arguments: Vec<_> = arguments.split(',').collect();
    if arguments.iter().any(|argument| argument.is_empty()) {
        return None;
    }

    if declares_class(&php::mask_comments(text), "Rule") {
        return None;
    }

    let import = php::insert_use(text, "Illuminate\\Validation\\Rule")?;
    let rule = &import.short_name;
    let quoted = |arguments: &[&str]| arguments.iter().map(|argument| format!("'{argument}'")).collect::<Vec<_>>().join(", ");
    let call = match name {
        "exists" | "unique" if arguments.len() <= 2 => format!("{rule}::{name}({})", quoted(&arguments)),
        "in" => format!("{rule}::in([{}])", quoted(&arguments)),
        "not_in" => format!("{rule}::notIn([{}])", quoted(&arguments)),
        _ => return None,
    };

    let mut edits = vec![(start - 1..start + contents.len() + 1, call.clone())];
    if !import.text.is_empty() {
        edits.push((import.offset..import.offset, import.text));
    }

    Some(Rewrite { title: format!("Convert to {call}"), kind: "refactor.rewrite", edits })
}

fn declares_class(masked: &str, name: &str) -> bool {
    let words: Vec<(usize, &str)> = masked
        .match_indices(|c: char| c.is_ascii_alphanumeric() || c == '_')
        .fold(Vec::<(usize, &str)>::new(), |mut words, (at, _)| {
            match words.last_mut() {
                Some((start, word)) if *start + word.len() == at => *word = &masked[*start..at + 1],
                _ => words.push((at, &masked[at..at + 1])),
            }
            words
        });

    words.windows(2).any(|pair| {
        let (at, keyword) = pair[0];
        let declares = matches!(keyword, "class" | "interface" | "trait" | "enum") && !masked[..at].trim_end().ends_with("::");
        declares && pair[1].1 == name
    })
}

fn root_facade(text: &str, at: usize) -> Option<Rewrite> {
    let masked = php::mask_comments(text);
    let bytes = masked.as_bytes();
    let at = at.min(bytes.len());
    let start = (0..at).rev().take_while(|&i| is_identifier(bytes[i])).last().unwrap_or(at);
    let end = (at..bytes.len()).take_while(|&i| is_identifier(bytes[i])).last().map_or(at, |i| i + 1);
    let name = &masked[start..end];

    if name.is_empty() || !masked[end..].starts_with("::") || start > 0 && matches!(bytes[start - 1], b'\\' | b'$' | b':') {
        return None;
    }

    let fqcn = if FACADES.contains(&name) {
        format!("Illuminate\\Support\\Facades\\{name}")
    } else if SUPPORT_HELPERS.contains(&name) {
        format!("Illuminate\\Support\\{name}")
    } else {
        return None;
    };

    let declares_namespace = masked.lines().any(|line| line.trim_start().starts_with("namespace "));
    if !declares_namespace || declares_class(&masked, name) {
        return None;
    }

    let import = php::insert_use(text, &fqcn)?;
    if import.text.is_empty() {
        return None;
    }

    Some(Rewrite {
        title: format!("Import {fqcn}"),
        kind: "refactor.rewrite",
        edits: vec![(import.offset..import.offset, import.text)],
    })
}

fn class_directive(text: &str, at: usize) -> Option<Rewrite> {
    const OPENING: &str = "class=\"";
    let mut from = 0;

    while let Some(found) = text[from..].find(OPENING) {
        let attribute = from + found;
        let value_start = attribute + OPENING.len();
        from = value_start;

        if attribute > at {
            return None;
        }

        let value_end = value_start + text[value_start..].find('"')?;
        if at > value_end + 1 || !text[..attribute].ends_with(char::is_whitespace) {
            continue;
        }

        let tag_start = text[..attribute].rfind('<')?;
        if text[tag_start..attribute].contains('>') {
            return None;
        }

        let tag_end = value_end + text[value_end..].find('>').unwrap_or(0);
        if text[tag_start..tag_end].contains("@class") {
            return None;
        }

        let (items, dynamic) = class_items(&text[value_start..value_end])?;
        return Some(Rewrite {
            title: if dynamic { "Wrap class attribute in @class" } else { "Convert class attribute to @class" }.into(),
            kind: "refactor.rewrite",
            edits: vec![(attribute..value_end + 1, format!("@class([{}])", items.join(", ")))],
        });
    }

    None
}

fn class_items(value: &str) -> Option<(Vec<String>, bool)> {
    if value.trim().is_empty() || value.contains(['\'', '\\', '@']) || value.contains("{!!") || value.contains("{{--") {
        return None;
    }

    let mut items = Vec::new();
    let mut dynamic = false;
    let mut rest = value.trim_start();

    while !rest.is_empty() {
        if let Some(echo) = rest.strip_prefix("{{") {
            let end = echo.find("}}")?;
            let expression = echo[..end].trim();
            rest = &echo[end + 2..];
            if expression.is_empty() || !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
                return None;
            }
            items.push(expression.to_owned());
            dynamic = true;
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let token = &rest[..end];
            if token.contains("{{") || token.contains("}}") {
                return None;
            }
            items.push(format!("'{token}'"));
            rest = &rest[end..];
        }
        rest = rest.trim_start();
    }

    Some((items, dynamic))
}

/// `Class::class` already names the controller: the file is in its namespace or imports it.
fn resolves(text: &str, class: &str, controllers_namespace: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if let Some(namespace) = line.strip_prefix("namespace ") {
            return namespace.trim_end_matches(';').trim() == controllers_namespace;
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
