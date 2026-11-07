//! Twig language support independent of the project index: tag, filter,
//! function and test names (Twig's public documentation) and a check for
//! block tags that are never closed.

use std::ops::Range;

use crate::refs::{is_identifier, literal_at, twig_code_start, word_at};

pub enum Slot {
    Tag,
    Filter,
    Function,
    Test,
}

const TAGS: &[&str] = &[
    "apply", "autoescape", "block", "cache", "deprecated", "do", "else", "elseif", "embed", "extends", "flush", "for", "from", "if", "import",
    "include", "macro", "sandbox", "set", "types", "use", "verbatim", "with", "endapply", "endautoescape", "endblock", "endcache", "endembed",
    "endfor", "endif", "endmacro", "endsandbox", "endset", "endverbatim", "endwith",
];

const FILTERS: &[&str] = &[
    "abs", "batch", "capitalize", "column", "convert_encoding", "country_name", "currency_name", "currency_symbol", "data_uri", "date",
    "date_modify", "default", "e", "escape", "filter", "find", "first", "format", "format_currency", "format_date", "format_datetime",
    "format_number", "format_time", "inline_css", "join", "json_encode", "keys", "language_name", "last", "length", "locale_name", "lower", "map",
    "markdown_to_html", "merge", "nl2br", "number_format", "raw", "reduce", "replace", "reverse", "round", "slice", "slug", "sort", "spaceless",
    "split", "striptags", "sum", "timezone_name", "title", "trim", "u", "upper", "url_encode",
];

const FUNCTIONS: &[&str] = &[
    "attribute", "block", "constant", "cycle", "date", "dump", "enum", "enum_cases", "html_classes", "include", "max", "min", "parent", "random",
    "range", "source", "template_from_string",
];

const TESTS: &[&str] = &["constant", "defined", "divisible by", "empty", "even", "iterable", "mapping", "null", "odd", "same as"];

pub fn names(slot: &Slot) -> &'static [&'static str] {
    match slot {
        Slot::Tag => TAGS,
        Slot::Filter => FILTERS,
        Slot::Function => FUNCTIONS,
        Slot::Test => TESTS,
    }
}

/// What kind of name is being typed at byte offset `at` in Twig code, and the
/// word's range.
pub fn slot_at(text: &str, at: usize) -> Option<(Slot, usize, usize)> {
    let code = twig_code_start(text, at)?;
    if literal_at("twig", text, at).is_some() {
        return None;
    }

    let (start, end) = word_at(text, at, is_identifier);
    if text.as_bytes().get(start).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    let before = text[code + 2..start].trim_start_matches(['-', '~']).trim_end();
    if text[code..].starts_with("{%") && before.is_empty() {
        return Some((Slot::Tag, start, end));
    }

    let mut words = before.rsplit(char::is_whitespace);
    let last = words.next().unwrap_or_default();
    let slot = match before.bytes().last() {
        Some(b'|') => Slot::Filter,
        Some(b'.') => return None,
        _ if last == "is" || (last == "not" && words.next() == Some("is")) => Slot::Test,
        _ => Slot::Function,
    };

    Some((slot, start, end))
}

/// A variable a template defines for itself, with where it comes from.
#[derive(Debug, PartialEq, Eq)]
pub struct Local {
    pub name: String,
    pub detail: &'static str,
}

fn identifiers(list: &str) -> Vec<String> {
    list.split(',').map(str::trim).filter(|name| !name.is_empty() && name.bytes().all(is_identifier) && !name.as_bytes()[0].is_ascii_digit()).map(str::to_owned).collect()
}

/// Keys of a literal mapping `{ a: 1, 'b': 2 }` at its top level.
pub fn mapping_keys(mapping: &str) -> Vec<String> {
    let Some(inner) = mapping.trim().strip_prefix('{') else { return Vec::new() };
    let mut keys = Vec::new();
    let (mut depth, mut quote, mut from) = (0usize, None, 0usize);

    let mut push = |part: &str| {
        if let Some((key, _)) = part.split_once(':') {
            let key = key.trim().trim_matches(['\'', '"']);
            if !key.is_empty() && key.bytes().all(is_identifier) {
                keys.push(key.to_owned());
            }
        }
    };

    for (at, c) in inner.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '{' | '[' | '(') => depth += 1,
            (None, ']' | ')') => depth = depth.saturating_sub(1),
            (None, '}') if depth == 0 => {
                push(&inner[from..at]);
                return keys;
            }
            (None, '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                push(&inner[from..at]);
                from = at + 1;
            }
            _ => {}
        }
    }

    push(&inner[from..]);
    keys
}

/// Variables visible at `at`: `{% set %}`, `{% for %}` (with `loop`), macro arguments,
/// `{% with %}` and `{# @var name Type #}` comments. Blocks, macros, loops and `with`
/// have their own scope; a scope that closed before `at` contributes nothing.
pub fn local_variables(text: &str, at: usize) -> Vec<Local> {
    let regions = code_regions(text);
    let mut scopes: Vec<(&str, Vec<Local>)> = vec![("", Vec::new())];

    for (name, rest, offset) in crate::refs::twig_tags(text) {
        let end = offset + rest.len();
        if offset >= at || end >= at || !regions.iter().any(|region| region.contains(&offset)) {
            continue;
        }

        let rest_trim = rest.trim();
        let declare = |scopes: &mut Vec<(&str, Vec<Local>)>, names: Vec<String>, detail: &'static str| {
            if let Some((_, locals)) = scopes.last_mut() {
                locals.extend(names.into_iter().map(|name| Local { name, detail }));
            }
        };

        match name {
            "set" => {
                let head = rest_trim.split_once('=').map_or(rest_trim, |(head, _)| head);
                let head = head.strip_suffix(['!', '<', '>']).unwrap_or(head);
                declare(&mut scopes, identifiers(head), "{% set %}");
            }
            "for" => {
                let head = rest_trim.split_once(" in ").map_or("", |(head, _)| head);
                scopes.push(("for", Vec::new()));
                declare(&mut scopes, identifiers(head), "{% for %}");
                declare(&mut scopes, vec!["loop".to_owned()], "{% for %}");
            }
            "macro" => {
                let arguments = rest_trim.split_once('(').map_or("", |(_, arguments)| arguments.rsplit_once(')').map_or(arguments, |(arguments, _)| arguments));
                let names = arguments.split(',').filter_map(|argument| identifiers(argument.split('=').next().unwrap_or_default()).into_iter().next()).collect();
                scopes.push(("macro", Vec::new()));
                declare(&mut scopes, names, "macro argument");
            }
            "with" => {
                scopes.push(("with", Vec::new()));
                declare(&mut scopes, mapping_keys(rest_trim), "{% with %}");
            }
            "block" if rest_trim.split_whitespace().count() <= 1 => scopes.push(("block", Vec::new())),
            "embed" => scopes.push(("embed", Vec::new())),
            "endfor" | "endmacro" | "endblock" | "endwith" | "endembed" => {
                let scope = &name["end".len()..];
                if let Some(position) = scopes.iter().rposition(|(kind, _)| *kind == scope) {
                    scopes.truncate(position);
                }
            }
            _ => {}
        }
    }

    let mut found: Vec<Local> = scopes.into_iter().flat_map(|(_, locals)| locals).collect();

    for region_start in text.match_indices("{#").map(|(open, _)| open) {
        let Some(close) = text[region_start..].find("#}") else { continue };
        let comment = &text[region_start + 2..region_start + close];
        let Some(tail) = comment.trim().strip_prefix("@var") else { continue };
        let mut words = tail.split_whitespace();
        let (first, second) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
        let is_type = |word: &str| word.contains('\\') || word.starts_with(|c: char| c.is_ascii_uppercase());
        let name = if is_type(first) && !second.is_empty() { second } else { first };
        if let Some(name) = identifiers(name).into_iter().next() {
            found.push(Local { name, detail: "@var" });
        }
    }

    found
}

/// Templates a template includes with variables: (included template, variable names).
pub fn includes(text: &str) -> Vec<(String, Vec<String>)> {
    let literal = |value: &str| value.trim().strip_prefix(['\'', '"']).and_then(|rest| rest.strip_suffix(['\'', '"'])).map(str::to_owned);
    let mut found = Vec::new();
    let regions = code_regions(text);

    for (name, rest, offset) in crate::refs::twig_tags(text) {
        if !matches!(name, "include" | "embed") || !regions.iter().any(|region| region.contains(&offset)) {
            continue;
        }

        let (source, with) = rest.split_once(" with ").unwrap_or((rest, ""));
        let source = source.split(" ignore missing").next().unwrap_or(source);
        if let Some(target) = literal(source).filter(|target| !target.contains(['{', '~'])) {
            found.push((target, mapping_keys(with.trim_end_matches(" only").trim())));
        }
    }

    for (at, _) in text.match_indices("include(") {
        let before = text[..at].bytes().last();
        if before.is_some_and(|b| is_identifier(b) || matches!(b, b'.' | b'>')) || !regions.iter().any(|region| region.contains(&at)) {
            continue;
        }

        let arguments = crate::refs::arguments(text, at + "include".len());
        if let Some(target) = arguments.first().and_then(|argument| literal(argument)).filter(|target| !target.contains(['{', '~'])) {
            found.push((target, arguments.get(1).map(|mapping| mapping_keys(mapping)).unwrap_or_default()));
        }
    }

    found
}

/// Byte ranges of the template that are Twig or HTML text: everything but
/// `{# #}` comments and the bodies of `{% verbatim %}`.
pub fn code_regions(text: &str) -> Vec<Range<usize>> {
    let mut regions = Vec::new();
    let mut region_start = 0;
    let mut i = 0;

    while let Some(found) = text[i..].find('{') {
        let at = i + found;
        i = at + 1;

        if text[at..].starts_with("{#") {
            regions.push(region_start..at);
            region_start = text[at + 2..].find("#}").map_or(text.len(), |e| at + 2 + e + 2);
            i = region_start;
            continue;
        }

        if !text[at..].starts_with("{%") {
            continue;
        }

        let Some(close) = text[at + 2..].find("%}").map(|e| at + 2 + e + 2) else { break };
        let inside = text[at + 2..close - 2].trim_matches(['-', '~', ' ', '\t', '\r', '\n']);
        i = close;

        if inside != "verbatim" {
            continue;
        }

        regions.push(region_start..close);
        let body_end = text[close..].find("endverbatim").and_then(|e| text[..close + e].rfind("{%")).filter(|&at| at >= close);
        region_start = body_end.unwrap_or(text.len());
        i = region_start.max(close);
    }

    regions.push(region_start..text.len());
    regions.retain(|region| !region.is_empty());
    regions
}

/// `alias.` before the caret where `{% import 'tpl' as alias %}` binds the alias:
/// the imported template's name (`_self` for the current one) and the range of the typed macro name.
pub fn macro_import_at(text: &str, offset: usize) -> Option<(String, std::ops::Range<usize>)> {
    let before = &text[..offset];
    let typed = before.bytes().rev().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    let name_start = offset - typed;
    let alias_end = name_start.checked_sub(1).filter(|&dot| before.as_bytes()[dot] == b'.')?;
    let alias_len = before[..alias_end].bytes().rev().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    let alias = &before[alias_end - alias_len..alias_end];
    if alias.is_empty() {
        return None;
    }

    for (at, _) in text.match_indices("import") {
        let tag_start = text[..at].rfind("{%")?;
        let tag_end = text[at..].find("%}").map_or(text.len(), |found| at + found);
        if text[tag_start + 2..at].trim_matches(['-', '~', ' ']).len() != 0 || tag_end <= at {
            continue;
        }

        let body = text[at + "import".len()..tag_end].trim().trim_end_matches(['-', '~']).trim();
        let (source, bound) = body.rsplit_once(" as ")?;
        if bound.trim() != alias {
            continue;
        }

        let source = source.trim();
        let template = match source {
            "_self" => "_self".to_owned(),
            quoted if quoted.len() > 2 && (quoted.starts_with('\'') || quoted.starts_with('"')) => quoted[1..quoted.len() - 1].to_owned(),
            _ => continue,
        };
        return Some((template, name_start..offset));
    }

    None
}

/// Names and argument lists of the `{% macro name(args) %}` tags of a template.
pub fn macro_definitions(source: &str) -> Vec<(String, String)> {
    source
        .match_indices("macro ")
        .filter_map(|(at, _)| {
            let tag_start = source[..at].rfind("{%")?;
            if !source[tag_start + 2..at].trim_matches(['-', '~', ' ']).is_empty() {
                return None;
            }

            let rest = &source[at + "macro ".len()..];
            let name: String = rest.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let open = rest.find('(')?;
            let close = rest[open..].find(')')? + open;
            (!name.is_empty()).then(|| (name, rest[open + 1..close].trim().to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{code_regions, macro_definitions, macro_import_at};

    #[test]
    fn code_regions_skip_comments_and_verbatim_bodies() {
        let text = "a{# x #}b{% verbatim %}{{ c }}{% endverbatim %}d";
        let parts: Vec<_> = code_regions(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(parts, ["a", "b{% verbatim %}", "{% endverbatim %}d"]);

        let text = "a{%}b{#}c";
        assert_eq!(code_regions(text), [0..text.len()]);

        let text = "a{# open";
        assert_eq!(code_regions(text), [0..1]);

        let text = "{% verbatim %}never closed";
        let parts: Vec<_> = code_regions(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(parts, ["{% verbatim %}"]);
    }

    fn names_at(marked: &str) -> Vec<String> {
        let at = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        let mut names: Vec<String> = super::local_variables(&text, at).into_iter().map(|local| local.name).collect();
        names.sort();
        names
    }

    #[test]
    fn set_and_for_variables_are_visible_after_their_tag_within_their_scope() {
        assert_eq!(names_at("{% set a = 1 %}{% set b, c = [1, 2] %}{% set cap %}x{% endset %}|"), ["a", "b", "c", "cap"]);
        assert_eq!(names_at("{% for k, v in items if v %}|{% endfor %}"), ["k", "loop", "v"]);
        assert_eq!(names_at("{% for item in items %}{% endfor %}|"), Vec::<String>::new());
        assert_eq!(names_at("{% for a in b %}{% for c in a %}|{% endfor %}{% endfor %}"), ["a", "c", "loop", "loop"]);
        assert_eq!(names_at("{% set a = 1 %}{% for x in y %}{% set z = x %}{% endfor %}|"), ["a"]);
        assert_eq!(names_at("{% if a %}{% set late = 1 %}{% endif %}|"), ["late"]);
    }

    #[test]
    fn macro_arguments_with_scopes_and_var_comments_are_collected() {
        assert_eq!(names_at("{% macro input(name, value = '', type = 'text') %}|{% endmacro %}"), ["name", "type", "value"]);
        assert_eq!(names_at("{% macro input(name) %}{% endmacro %}|"), Vec::<String>::new());
        assert_eq!(names_at("{% with { a: 1, 'b': [1, 2], c: {d: 1} } %}|{% endwith %}"), ["a", "b", "c"]);
        assert_eq!(names_at("{% block body %}{% set inner = 1 %}{% endblock %}{% block other %}|{% endblock %}"), Vec::<String>::new());
        assert_eq!(names_at("{% block title 'Short' %}{% set kept = 1 %}|"), ["kept"]);
        assert_eq!(names_at("{# @var user \\App\\User #}{# @var \\App\\Post post #}{# @var plain #}|"), ["plain", "post", "user"]);
    }

    #[test]
    fn variables_ignore_comments_verbatim_the_tag_being_typed_and_degenerate_input() {
        assert_eq!(names_at("{# {% set a = 1 %} #}{% verbatim %}{% set b = 1 %}{% endverbatim %}|"), Vec::<String>::new());
        assert_eq!(names_at("{% for x in |"), Vec::<String>::new());
        assert_eq!(names_at("{% set a = | %}"), Vec::<String>::new());
        for text in ["{% set", "{% for", "{% macro (", "{% with {", "{# @var", "{% set , = %}", "{%"] {
            let _ = super::local_variables(text, text.len());
            let _ = super::includes(text);
        }
    }

    #[test]
    fn includes_with_variables_are_collected_from_tags_and_functions() {
        let text = "{% include 'a.html.twig' with {x: 1, y: z} only %}{% include 'b.twig' %}{% include dynamic with {q: 1} %}{{ include('c.twig', {k: v}) }}{% embed 'd.twig' with {e: 1} %}{% endembed %}{# {% include 'no.twig' with {n: 1} %} #}{{ obj.include('x.twig', {m: 1}) }}";
        assert_eq!(
            super::includes(text),
            [
                ("a.html.twig".to_owned(), vec!["x".to_owned(), "y".to_owned()]),
                ("b.twig".to_owned(), Vec::new()),
                ("d.twig".to_owned(), vec!["e".to_owned()]),
                ("c.twig".to_owned(), vec!["k".to_owned()]),
            ]
        );
    }

    #[test]
    fn imported_macros_are_found_by_alias() {
        let text = "{% import 'forms.html.twig' as f %}{% import _self as me %}{{ f.in }}";
        let offset = text.find("in }}").unwrap() + 2;
        assert_eq!(macro_import_at(text, offset), Some(("forms.html.twig".to_owned(), offset - 2..offset)));

        let own = "{% import _self as me %}{{ me. }}";
        assert_eq!(macro_import_at(own, own.len() - 3).map(|(template, _)| template), Some("_self".to_owned()));
        assert_eq!(macro_import_at("{{ g.x }}", 6), None);
        assert_eq!(macro_definitions("{% macro input(name, type = 'text') %}x{% endmacro %}{%- macro row() -%}"), [("input".to_owned(), "name, type = 'text'".to_owned()), ("row".to_owned(), String::new())]);
    }
}
