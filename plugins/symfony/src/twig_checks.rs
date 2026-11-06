//! Twig text inspections: route comparisons, translation domains and syntax
//! mistakes. They scan only the Twig code of a template: `{# #}` comments and
//! `{% verbatim %}` bodies are skipped. Written from the public source of the
//! Symfony Plugin inspections (MIT) and the Twig documentation (the `for`
//! condition is the paid original's: own message).

use std::ops::Range;

use crate::checks::{self, Context, Finding};
use crate::refs::{self, is_identifier};
use crate::twig::code_regions;

const ROUTE_ATTRIBUTE: &str = "app.request.attributes.get(";
const CURRENT_ROUTE: &str = "app.current_route";

/// `twig-route-compare` and `twig-missing-translation-domain`.
pub fn names(ctx: &Context, text: &str) -> Vec<Finding> {
    let regions = code_regions(text);
    let mut out = Vec::new();

    route_compares(ctx, text, &regions, &mut out);
    translation_domains(ctx, text, &regions, &mut out);
    out.sort_by_key(|finding| finding.start);
    out
}

/// `twig-for-condition`, `twig-component-block-syntax` and
/// `twig-component-self-import`.
pub fn syntax(text: &str) -> Vec<Finding> {
    let regions = code_regions(text);
    let mut out = Vec::new();

    for_conditions(text, &regions, &mut out);
    component_mistakes(text, &regions, &mut out);
    out.sort_by_key(|finding| finding.start);
    out
}

fn in_code(regions: &[Range<usize>], at: usize) -> bool {
    regions.iter().any(|region| region.contains(&at))
}

fn skip_space(text: &str, at: usize) -> usize {
    at + text[at..].len() - text[at..].trim_start().len()
}

/// Contents of the quoted literal that starts at `at`, and the offset after
/// its closing quote. `None` for escapes and interpolation.
fn literal_at(text: &str, at: usize) -> Option<(Range<usize>, usize)> {
    let quote = *text.as_bytes().get(at).filter(|b| matches!(b, b'\'' | b'"'))?;
    let length = text[at + 1..].bytes().position(|b| b == quote)?;
    let range = at + 1..at + 1 + length;
    let inner = &text[range.clone()];
    if inner.contains(['\\', '#']) {
        return None;
    }

    Some((range, at + length + 2))
}

/// A literal in a comparison is a plain name unless an operator or filter
/// continues it.
fn standalone_after(text: &str, end: usize) -> bool {
    let rest = &text[skip_space(text, end)..];
    !rest.starts_with(['~', '|', '.', '[', '(', '+', '?'])
}

/// Offsets (start, end) of `app.current_route` and
/// `app.request.attributes.get('_route')`.
fn route_expressions(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();

    for (at, _) in text.match_indices(CURRENT_ROUTE) {
        let end = at + CURRENT_ROUTE.len();
        let before = text[..at].bytes().last();
        let after = text.as_bytes().get(end).copied();
        if before.is_some_and(|b| is_identifier(b) || b == b'.') || after.is_some_and(|b| is_identifier(b) || b == b'.') {
            continue;
        }

        found.push(at..end);
    }

    for (at, _) in text.match_indices(ROUTE_ATTRIBUTE) {
        let start = skip_space(text, at + ROUTE_ATTRIBUTE.len());
        let Some((range, after)) = literal_at(text, start) else { continue };
        if &text[range] != "_route" {
            continue;
        }

        let close = skip_space(text, after);
        if text[close..].starts_with(')') {
            found.push(at..close + 1);
        }
    }

    found
}

/// The literal a comparison, `is same as(...)` or `in [...]` puts after the
/// route expression.
fn compared_after(text: &str, end: usize) -> Vec<Range<usize>> {
    let start = skip_space(text, end);
    let rest = &text[start..];
    let literal_of = |at: usize| literal_at(text, skip_space(text, at)).filter(|(_, after)| standalone_after(text, *after)).map(|(range, _)| range);

    if let Some(operator) = ["==", "!="].iter().find(|operator| rest.starts_with(**operator)) {
        return literal_of(start + operator.len()).into_iter().collect();
    }

    let word = |at: usize, word: &str| {
        let at = skip_space(text, at);
        let boundary = text.as_bytes().get(at + word.len()).is_none_or(|b| !is_identifier(*b));
        (text[at..].starts_with(word) && boundary).then_some(at + word.len())
    };

    let after_is = word(start, "is");
    if let Some(after_is) = after_is {
        let after_not = word(after_is, "not").unwrap_or(after_is);
        let after_same = word(after_not, "same").and_then(|at| word(at, "as"));
        let open = after_same.map(|at| skip_space(text, at)).filter(|at| text[*at..].starts_with('('));
        return open.and_then(|open| literal_of(open + 1)).into_iter().collect();
    }

    let after_not = word(start, "not").unwrap_or(start);
    let open = word(after_not, "in").map(|at| skip_space(text, at)).filter(|at| text[*at..].starts_with('['));
    let Some(open) = open else { return Vec::new() };

    refs::arguments(text, open)
        .into_iter()
        .filter_map(|item| checks::literal(text, item))
        .filter(|range| !checks::is_dynamic(&text[range.clone()]))
        .collect()
}

/// The literal on the left of `==` / `!=` that ends right before `start`.
fn compared_before(text: &str, start: usize) -> Option<Range<usize>> {
    let head = text[..start].trim_end();
    let head = head.strip_suffix("==").or_else(|| head.strip_suffix("!="))?.trim_end();
    let quote = head.chars().last().filter(|c| matches!(c, '\'' | '"'))?;
    let open = head[..head.len() - 1].rfind(quote)?;
    let (range, _) = literal_at(text, open)?;
    let before = text[..open].trim_end();

    (range.end + 1 == head.len() && !before.ends_with(['~', '|', '.', '+'])).then_some(range)
}

fn route_compares(ctx: &Context, text: &str, regions: &[Range<usize>], out: &mut Vec<Finding>) {
    if !ctx.index.compiled_routes {
        return;
    }

    for expression in route_expressions(text) {
        if !in_code(regions, expression.start) {
            continue;
        }

        let ranges = compared_after(text, expression.end).into_iter().chain(compared_before(text, expression.start));
        for range in ranges {
            let name = &text[range.clone()];
            if checks::is_dynamic(name) || name.starts_with('_') || ctx.index.routes.contains_key(name) {
                continue;
            }

            out.push(Finding::warn(range, "twig-route-compare", format!("Route \"{name}\" is not defined.")));
        }
    }
}

fn translation_domains(ctx: &Context, text: &str, regions: &[Range<usize>], out: &mut Vec<Finding>) {
    for (at, _) in text.match_indices("trans") {
        let after = at + "trans".len();
        let piped = text[..at].trim_end().ends_with('|');
        if !piped || !in_code(regions, at) || !text[after..].starts_with('(') {
            continue;
        }

        let arguments = refs::arguments(text, after);
        let named = arguments.iter().find_map(|argument| argument.trim().strip_prefix("domain:"));
        let Some(range) = named.or_else(|| arguments.get(1).copied()).and_then(|argument| checks::literal(text, argument)) else { continue };

        let domain = &text[range.clone()];
        if checks::translation_domain_is_unknown(ctx, domain) {
            out.push(Finding::warn(range, "twig-missing-translation-domain", format!("Translation domain \"{domain}\" has no translation files.")));
        }
    }
}

struct Tag {
    name: Range<usize>,
    head: Range<usize>,
}

/// The `{% ... %}` tag that starts at `at`; its end honours quotes.
fn tag_at(text: &str, at: usize) -> Option<(Tag, usize)> {
    let mut i = at + 2;
    i += text[i..].len() - text[i..].trim_start_matches(['-', '~']).len();
    i = skip_space(text, i);

    let name_len = text[i..].bytes().position(|b| !is_identifier(b)).unwrap_or(text.len() - i);
    let name = i..i + name_len;
    let head_start = name.end;

    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(_), b'\\') => i += 1,
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b @ (b'\'' | b'"')) => quote = Some(b),
            (None, b'%') if bytes.get(i + 1) == Some(&b'}') => {
                let head = head_start..i;
                return Some((Tag { name, head }, i + 2));
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Words of a tag head outside quotes and brackets, with their ranges.
fn top_level_words(text: &str, head: Range<usize>) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut quote: Option<u8> = None;
    let mut depth = 0usize;
    let mut i = head.start;

    while i < head.end {
        let b = bytes[i];
        match quote {
            Some(_) if b == b'\\' => i += 1,
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'\'' || b == b'"' => quote = Some(b),
            None if b"([{".contains(&b) => depth += 1,
            None if b")]}".contains(&b) => depth = depth.saturating_sub(1),
            None if depth == 0 && is_identifier(b) && (i == head.start || !is_identifier(bytes[i - 1])) => {
                let length = text[i..head.end].bytes().position(|b| !is_identifier(b)).unwrap_or(head.end - i);
                words.push(i..i + length);
                i += length;
                continue;
            }
            None => {}
        }
        i += 1;
    }

    words
}

fn for_conditions(text: &str, regions: &[Range<usize>], out: &mut Vec<Finding>) {
    for region in regions {
        for (at, _) in text[region.clone()].match_indices("{%") {
            let Some((tag, _)) = tag_at(text, region.start + at) else { continue };
            if &text[tag.name.clone()] != "for" {
                continue;
            }

            let words = top_level_words(text, tag.head);
            let Some(after_in) = words.iter().position(|word| &text[word.clone()] == "in") else { continue };
            let Some(condition) = words[after_in + 1..].iter().find(|word| &text[(*word).clone()] == "if") else { continue };

            out.push(Finding::warn(
                condition.clone(),
                "twig-for-condition",
                "The \"if\" condition of \"for\" is deprecated since Twig 2.10 and removed in Twig 3; filter the sequence with the \"filter\" filter instead.".into(),
            ));
        }
    }
}

enum Open {
    Element(String),
    Component,
}

/// `<twig:Name` at `at`: the name, whether the element is self-closing and
/// where the tag ends. Quotes and `{{ }}` may contain a `>`.
fn element_at(text: &str, at: usize) -> Option<(String, bool, usize)> {
    let name_start = at + "<twig:".len();
    let name_len = text[name_start..].bytes().position(|b| !(is_identifier(b) || matches!(b, b':' | b'.' | b'-')))?;
    if name_len == 0 {
        return None;
    }

    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    let mut braces = 0usize;
    let mut i = name_start + name_len;
    if !matches!(bytes[i], b'>' | b'/' | b' ' | b'\t' | b'\r' | b'\n') {
        return None;
    }

    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(q), b) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b @ (b'\'' | b'"')) => quote = Some(b),
            (None, b'{') if bytes.get(i + 1) == Some(&b'{') => {
                braces += 1;
                i += 1;
            }
            (None, b'}') if braces > 0 && bytes.get(i + 1) == Some(&b'}') => {
                braces -= 1;
                i += 1;
            }
            (None, b'>') if braces == 0 => {
                let self_closing = bytes[i - 1] == b'/';
                return Some((text[name_start..name_start + name_len].to_owned(), self_closing, i + 1));
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Walks the template once, tracking open `<twig:Name>` elements and
/// `{% component %}` pairs. A mistake is kept once the outermost construct it
/// sits in is closed; an unbalanced file reports nothing from that point on.
fn component_mistakes(text: &str, regions: &[Range<usize>], out: &mut Vec<Finding>) {
    let mut stack: Vec<Open> = Vec::new();
    let mut pending: Vec<Finding> = Vec::new();

    for region in regions {
        let mut i = region.start;

        while let Some(found) = text[i..region.end].find(['{', '<']) {
            let at = i + found;
            i = at + 1;
            let rest = &text[at..region.end];

            if rest.starts_with("</twig:") {
                let Some(close) = rest.find('>') else { return };
                let name = rest["</twig:".len()..close].trim();
                if !matches!(stack.pop(), Some(Open::Element(open)) if open == name) {
                    return;
                }

                i = at + close + 1;
            } else if rest.starts_with("<twig:") {
                let Some((name, self_closing, end)) = element_at(text, at) else { continue };
                i = end;
                if !self_closing {
                    stack.push(Open::Element(name));
                }
            } else if rest.starts_with("{%") {
                let Some((tag, end)) = tag_at(text, at) else { return };
                i = end;

                match &text[tag.name.clone()] {
                    "component" => stack.push(Open::Component),
                    "endcomponent" if !matches!(stack.pop(), Some(Open::Component)) => return,
                    "block" if matches!(stack.last(), Some(Open::Element(name)) if name != "block") => {
                        pending.push(Finding::error(
                            tag.name,
                            "twig-component-block-syntax",
                            "A {% block %} tag cannot be used inside a <twig:...> element; wrap the content in <twig:block name=\"...\"> instead.".into(),
                        ));
                    }
                    "from" | "import" if !stack.is_empty() => {
                        let words = top_level_words(text, tag.head);
                        if let Some(source) = words.first().filter(|word| &text[(*word).clone()] == "_self") {
                            pending.push(Finding::error(
                                source.clone(),
                                "twig-component-self-import",
                                "A component template cannot import macros from \"_self\"; import them from their own template file.".into(),
                            ));
                        }
                    }
                    _ => {}
                }
            } else {
                continue;
            }

            if stack.is_empty() {
                out.append(&mut pending);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position, Range as TextRange};

    use crate::index::{Entry, Index};
    use crate::Main;

    const TEMPLATE: &str = "/project/templates/a.html.twig";

    fn entry() -> Entry {
        Entry { detail: None, path: "/project/src/B.php".into(), at: Position::new(0, 0) }
    }

    fn index() -> Index {
        let mut index = Index { compiled_routes: true, ..Index::default() };
        index.routes.insert("blog_show".into(), entry());
        index.translations.insert("messages".into(), BTreeMap::default());
        index.translations.insert("admin".into(), BTreeMap::default());
        index
    }

    fn run_with(index: &Index, text: &str) -> Vec<Diagnostic> {
        let read = |_: &str| None;
        crate::extra_problems(index, crate::Groups::all_on(), &read, TEMPLATE, "twig", text)
    }

    fn run(text: &str) -> Vec<Diagnostic> {
        run_with(&index(), text)
    }

    fn codes(found: &[Diagnostic]) -> Vec<&str> {
        found.iter().map(|d| d.code.as_deref().unwrap_or_default()).collect()
    }

    fn only<'a>(found: &'a [Diagnostic], code: &str) -> Vec<&'a Diagnostic> {
        found.iter().filter(|d| d.code.as_deref() == Some(code)).collect()
    }

    fn word<'a>(text: &'a str, d: &Diagnostic) -> &'a str {
        let line = text.lines().nth(d.range.start.line as usize).unwrap();
        &line[d.range.start.character as usize..d.range.end.character as usize]
    }

    #[test]
    fn twig_route_compare_warns_for_unknown_route() {
        let text = "{% if app.request.attributes.get('_route') == 'nope' %}{% elseif app.current_route != \"gone\" %}{% elseif app.current_route is same as('x') %}{% elseif 'y' == app.current_route %}{% endif %}";
        let found = run(text);
        let found = only(&found, "twig-route-compare");
        let words: Vec<_> = found.iter().map(|d| word(text, d)).collect();
        assert_eq!(words, ["nope", "gone", "x", "y"]);
        assert_eq!(found[0].message, "Route \"nope\" is not defined.");
    }

    #[test]
    fn twig_route_compare_checks_every_literal_of_an_in_list() {
        let text = "{% if app.current_route in ['blog_show', 'a', 'b'] %}{% elseif app.current_route not in ['c'] %}{% endif %}";
        let found = run(text);
        let words: Vec<_> = only(&found, "twig-route-compare").iter().map(|d| word(text, d)).collect();
        assert_eq!(words, ["a", "b", "c"]);
    }

    #[test]
    fn twig_route_compare_accepts_current_route_variable() {
        let found = run("{% if app.current_route == 'nope' %}{% endif %}{% if app.current_route == 'blog_show' %}{% endif %}");
        assert_eq!(codes(&found), ["twig-route-compare"]);
    }

    #[test]
    fn twig_route_compare_is_quiet_without_compiled_routes() {
        let mut without = index();
        without.compiled_routes = false;
        assert!(run_with(&without, "{% if app.current_route == 'nope' %}{% endif %}").is_empty());
    }

    #[test]
    fn twig_route_compare_is_quiet_for_known_underscore_and_dynamic_names() {
        for condition in [
            "app.current_route == 'blog_show'",
            "app.current_route == '_wdt'",
            "app.current_route == 'a' ~ b",
            "'p_' ~ 'a' == app.current_route",
            "app.current_route == \"a#{b}\"",
            "app.current_route starts with 'nope'",
            "app.current_route == name",
            "app.current_route in ['a' ~ b, name]",
            "app.current_route_name == 'nope'",
            "app.request.attributes.get('other') == 'nope'",
        ] {
            assert!(run(&format!("{{% if {condition} %}}{{% endif %}}")).is_empty(), "{condition}");
        }
    }

    #[test]
    fn for_with_if_condition_warns_on_the_if_word() {
        let text = "{% for user in users if user.active %}{{ user }}{% endfor %}";
        let found = run(text);
        assert_eq!(codes(&found), ["twig-for-condition"]);
        assert_eq!(word(text, &found[0]), "if");
        assert_eq!(found[0].range.start.character, 21);
        assert_eq!(found[0].severity, openidea_plugin::Severity::Warning);
    }

    #[test]
    fn for_without_condition_is_quiet() {
        for tag in ["{% for k, v in {if: 1} %}", "{% for a in b|filter(x -> x if y) %}", "{% for a in 1..3 %}", "{%- for a in b -%}"] {
            assert!(run(&format!("{tag}{{% endfor %}}")).is_empty(), "{tag}");
        }
    }

    #[test]
    fn if_tag_in_a_for_body_is_quiet() {
        assert!(run("{% for a in b %}{% if a %}x{% endif %}{% endfor %}").is_empty());
    }

    #[test]
    fn if_inside_a_string_literal_is_quiet() {
        assert!(run("{% for a in ['x if y', \"p if q\"] %}{% endfor %}").is_empty());
    }

    #[test]
    fn block_inside_component_element_is_an_error() {
        let text = "<twig:Alert type=\"x\">\n  {% block content %}hi{% endblock %}\n</twig:Alert>";
        let found = run(text);
        assert_eq!(codes(&found), ["twig-component-block-syntax"]);
        assert_eq!(word(text, &found[0]), "block");
        assert_eq!(found[0].severity, openidea_plugin::Severity::Error);
        assert_eq!(found[0].range.start.line, 1);
    }

    #[test]
    fn block_inside_twig_block_element_is_quiet() {
        let text = "<twig:Alert><twig:block name=\"content\">{% block inner %}{% endblock %}</twig:block></twig:Alert><twig:Card />{% block outer %}{% endblock %}";
        assert!(run(text).is_empty());
    }

    #[test]
    fn block_outside_components_is_quiet() {
        assert!(run("{% block a %}<div>{% block b %}{% endblock %}</div>{% endblock %}<twig:Card icon=\"x\" /><twig:Card>x</twig:Card>").is_empty());
        assert!(run("<twig:Card class=\"a > b\" :x=\"{{ a > b }}\" />{% block a %}{% endblock %}").is_empty());
    }

    #[test]
    fn unbalanced_component_tags_stay_quiet() {
        assert!(run("<twig:Alert>{% block a %}{% endblock %}").is_empty());
        assert!(run("<twig:Alert>{% block a %}{% endblock %}</twig:Card>").is_empty());
        assert!(run("</twig:Alert><twig:Alert>{% block a %}{% endblock %}</twig:Alert>").is_empty());
    }

    #[test]
    fn self_import_inside_component_is_an_error() {
        let text = "<twig:Alert>{% from _self import helper %}{% import _self as m %}</twig:Alert>";
        let found = run(text);
        assert_eq!(codes(&found), ["twig-component-self-import", "twig-component-self-import"]);
        assert_eq!(word(text, &found[0]), "_self");
        assert_eq!(found[1].range.start.character, 52);
        assert_eq!(found[0].severity, openidea_plugin::Severity::Error);
    }

    #[test]
    fn self_import_in_a_plain_template_is_quiet() {
        assert!(run("{% from _self import helper %}{% import _self as m %}<twig:Alert>{% from 'x.twig' import y %}</twig:Alert>{% import _self as z %}").is_empty());
    }

    #[test]
    fn self_import_inside_component_tag_pair_is_an_error() {
        let found = run("{% component 'Alert' %}{% from _self import a %}{% endcomponent %}{% from _self import b %}");
        assert_eq!(codes(&found), ["twig-component-self-import"]);
        assert_eq!(found[0].range.start.character, 31);
    }

    #[test]
    fn twig_translation_domain_warns_for_unknown_project_domain() {
        let text = "{{ 'a'|trans({}, 'ghost') }}{{ 'b' | trans({}, 'admin') }}{{ 'c'|trans({}, 'ghost+intl-icu') }}";
        let found = run(text);
        let found = only(&found, "twig-missing-translation-domain");
        let words: Vec<_> = found.iter().map(|d| word(text, d)).collect();
        assert_eq!(words, ["ghost", "ghost+intl-icu"]);
        assert_eq!(found[0].message, "Translation domain \"ghost\" has no translation files.");
    }

    #[test]
    fn twig_translation_domain_reads_the_named_argument() {
        let text = "{{ 'a'|trans(domain: 'ghost') }}{{ 'b'|trans(locale: 'fr', domain: 'ghost2') }}";
        let found = run(text);
        let words: Vec<_> = only(&found, "twig-missing-translation-domain").iter().map(|d| word(text, d)).collect();
        assert_eq!(words, ["ghost", "ghost2"]);
    }

    #[test]
    fn twig_translation_domain_is_quiet_for_library_or_dynamic_name() {
        for call in ["trans({}, 'validators')", "trans({}, 'FOSUserBundle')", "trans({}, 'a.b')", "trans({}, 'x' ~ y)", "trans({}, domain)", "trans({})", "trans", "trans({}, null)"] {
            assert!(run(&format!("{{{{ 'k'|{call} }}}}")).is_empty(), "{call}");
        }

        let mut none = index();
        none.translations.clear();
        assert!(run_with(&none, "{{ 'k'|trans({}, 'ghost') }}").is_empty());
    }

    #[test]
    fn twig_checks_ignore_comments_and_verbatim() {
        let text = "{# path('x') {% for a in b if c %} app.current_route == 'nope' 'k'|trans({}, 'ghost') #}\
            {% verbatim %}{% for a in b if c %}{{ app.current_route == 'nope' }}<twig:A>{% block x %}</twig:A>{% endverbatim %}\
            <twig:A>{# {% block y %} {% from _self import z %} #}</twig:A>";
        assert!(run(text).is_empty());
    }

    #[test]
    fn twig_group_ranges_use_utf16_columns_with_multibyte_text() {
        let found = run("{# é😀 #}{% for a in b if c %}{% endfor %}");
        assert_eq!(found[0].range.start, Position::new(0, 9 + 14));
        assert_eq!(found[0].range.end.character, 9 + 16);
    }

    #[test]
    fn twig_group_crlf_files_report_correct_lines() {
        let found = run("<p>\r\n{% for a in b if c %}{% endfor %}\r\n<twig:A>\r\n{% block x %}\r\n</twig:A>");
        assert_eq!(found[0].range, TextRange::new(Position::new(1, 14), Position::new(1, 16)));
        assert_eq!(found[1].range, TextRange::new(Position::new(3, 3), Position::new(3, 8)));
    }

    #[test]
    fn twig_syntax_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();

        host.add_file("templates/a.html.twig", "{% for a in b if c %}{% endfor %}<twig:A>{% block x %}{% endblock %}</twig:A>");
        plugin.handle_events(&[Event::FileOpened(TEMPLATE.into())], &host.invocation());
        assert_eq!(host.diagnostics()[TEMPLATE].len(), 2);

        host.add_file("templates/a.html.twig", "{% for a in b|filter(c) %}{% endfor %}<twig:A><twig:block name=\"x\"></twig:block></twig:A>");
        plugin.handle_events(&[Event::FileSaved(TEMPLATE.into())], &host.invocation());
        assert!(host.diagnostics().get(TEMPLATE).is_none_or(Vec::is_empty));
    }

    #[test]
    fn twig_syntax_group_can_be_switched_off() {
        let read = |_: &str| None;
        let text = "{% for a in b if c %}{% endfor %}{% if app.current_route == 'nope' %}{% endif %}";
        let off = crate::Groups { twig_syntax: false, ..crate::Groups::all_on() };
        let found = crate::extra_problems(&index(), off, &read, TEMPLATE, "twig", text);
        assert_eq!(codes(&found), ["twig-route-compare"]);

        let off = crate::Groups { twig: false, ..crate::Groups::all_on() };
        let found = crate::extra_problems(&index(), off, &read, TEMPLATE, "twig", text);
        assert_eq!(codes(&found), ["twig-for-condition"]);
    }

    #[test]
    fn twig_checks_do_not_run_for_other_languages() {
        let read = |_: &str| None;
        let text = "{% for a in b if c %}";
        assert!(crate::extra_problems(&index(), crate::Groups::all_on(), &read, "/project/src/A.php", "php", text).is_empty());
    }

    #[test]
    fn twig_checks_run_without_the_twig_language_provider() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}}"#);
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        host.add_file("templates/a.html.twig", "{% for a in b if c %}{% endfor %}");
        plugin.handle_events(&[Event::FileOpened(TEMPLATE.into())], &host.invocation());
        assert_eq!(codes(&host.diagnostics()[TEMPLATE]), ["twig-for-condition"]);
    }
}
