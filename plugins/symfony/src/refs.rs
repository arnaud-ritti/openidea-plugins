//! What the code around a position refers to: the plugin's PSI patterns
//! (`MethodMatcher`, `TwigPattern`, YAML/XML element patterns) as text
//! patterns on the surrounding code.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Route,
    Template,
    Service,
    Translation,
    Class,
    Asset,
    Entry,
    Role,
    Block,
    Tag,
}

/// A name in the document and what kind of thing it names.
#[derive(Debug, PartialEq, Eq)]
pub struct Reference {
    pub kind: Kind,
    /// Byte range of the name in the document.
    pub start: usize,
    pub end: usize,
}

const ROUTE_CALLS: &[&str] = &["generateUrl(", "redirectToRoute(", "->generate("];
pub const TEMPLATE_CALLS: &[&str] = &[
    "->render(",
    "->renderView(",
    "->renderResponse(",
    "->stream(",
    "->renderBlock(",
    "->renderBlockView(",
    "->htmlTemplate(",
    "->textTemplate(",
    "->display(",
    "#[Template(",
];
const TRANSLATION_CALLS: &[&str] = &["->trans(", "new TranslatableMessage("];
const ROLE_CALLS: &[&str] = &["isGranted(", "denyAccessUnlessGranted(", "#[IsGranted("];
const TWIG_TEMPLATE_TAGS: &[&str] = &["extends", "include", "embed", "import", "from", "use"];
const TWIG_ENTRY_FUNCTIONS: &[&str] = &["encore_entry_link_tags", "encore_entry_script_tags", "importmap"];

/// Lines longer than this (minified code) are not looked at.
const MAX_LINE: usize = 4000;

pub fn is_identifier(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `before` ends with the function call `name(`, not with `…name(` or `->name(`.
fn ends_with_function(before: &str, name: &str) -> bool {
    let call = format!("{name}(");
    let Some(rest) = before.strip_suffix(&call) else { return false };
    rest.bytes().last().is_none_or(|b| !is_identifier(b) && b != b'>' && b != b':' && b != b'$')
}

fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    (start, end)
}

/// Start of the Twig code the position is in on its line: after the last
/// `{{` / `{%`, unless it was closed again.
pub(crate) fn twig_code_start(text: &str, at: usize) -> Option<usize> {
    let (line_start, _) = line_bounds(text, at);
    let line = &text[line_start..at];
    let open = line.rfind("{{").into_iter().chain(line.rfind("{%")).max()?;
    let closed = line[open..].contains("}}") || line[open..].contains("%}");
    (!closed).then_some(line_start + open)
}

/// The quoted string `at` is in, on its line: (contents start, contents end,
/// closed). In Twig only quotes in Twig code count: it may sit inside an HTML
/// attribute.
pub(crate) fn literal_at(language: &str, text: &str, at: usize) -> Option<(usize, usize, bool)> {
    let (mut line_start, line_end) = line_bounds(text, at);
    if line_end - line_start > MAX_LINE {
        return None;
    }

    if language == "twig" {
        let line = &text[line_start..at];
        line_start += line.rfind("{{").into_iter().chain(line.rfind("{%")).max()?;
    }

    let bytes = text.as_bytes();
    let mut open: Option<(u8, usize)> = None;
    let mut i = line_start;
    while i < at {
        let b = bytes[i];
        match open {
            Some(_) if b == b'\\' => i += 1,
            Some((quote, _)) if b == quote => open = None,
            Some(_) => {}
            None if b == b'\'' || b == b'"' => open = Some((b, i + 1)),
            None => {}
        }
        i += 1;
    }

    let (quote, start) = open?;
    let mut end = at;
    while end < line_end {
        match bytes[end] {
            b'\\' => end += 1,
            b if b == quote => return Some((start, end, true)),
            _ => {}
        }
        end += 1;
    }

    Some((start, at.min(line_end), false))
}

/// The identifier-ish word around `at` made of `allowed` bytes.
pub(crate) fn word_at(text: &str, at: usize, allowed: fn(u8) -> bool) -> (usize, usize) {
    let bytes = text.as_bytes();
    let start = (0..at).rev().take_while(|&i| allowed(bytes[i])).last().unwrap_or(at);
    let end = (at..bytes.len()).take_while(|&i| allowed(bytes[i])).last().map_or(at, |i| i + 1);
    (start, end)
}

fn is_class_byte(b: u8) -> bool {
    is_identifier(b) || b == b'\\'
}

/// What the name at byte offset `at` refers to, if anything.
pub fn reference_at(language: &str, text: &str, at: usize) -> Option<Reference> {
    if let Some((start, end)) = crate::index::tag_slot_at(language, text, at) {
        return Some(Reference { kind: Kind::Tag, start, end });
    }

    let Some((start, end, closed)) = literal_at(language, text, at) else {
        return match language {
            "yaml" => yaml_class_at(text, at),
            "twig" => block_tag_at(text, at).map(|(start, end)| Reference { kind: Kind::Block, start, end }),
            _ => None,
        };
    };

    let (line_start, _) = line_bounds(text, start);
    let before = text[line_start..start - 1].trim_end();
    let after = if closed { &text[end + 1..] } else { "" };
    let contents = &text[start..end];
    let reference = |kind| Some(Reference { kind, start, end });

    match language {
        "php" => {
            let lower = before.to_ascii_lowercase();

            if ROUTE_CALLS.iter().any(|call| before.ends_with(call)) {
                return reference(Kind::Route);
            }

            if TEMPLATE_CALLS.iter().any(|call| before.ends_with(call)) {
                return reference(Kind::Template);
            }

            if TRANSLATION_CALLS.iter().any(|call| before.ends_with(call)) || ends_with_function(before, "t") {
                return reference(Kind::Translation);
            }

            if is_constraint_message(before) && contents.contains('.') && !contents.contains(char::is_whitespace) {
                return reference(Kind::Translation);
            }

            if ROLE_CALLS.iter().any(|call| before.ends_with(call)) {
                return reference(Kind::Role);
            }

            if before.ends_with("->getUrl(") {
                return reference(Kind::Asset);
            }

            let container_call = ["container->get(", "container->has(", "container()->get(", "container()->has("]
                .iter()
                .any(|call| lower.ends_with(call));
            if container_call || ends_with_function(before, "service") || ends_with_function(before, "ref") {
                return reference(Kind::Service);
            }

            None
        }
        "twig" => {
            if ends_with_function(before, "path") || ends_with_function(before, "url") {
                return reference(Kind::Route);
            }

            if ends_with_function(before, "include") || ends_with_function(before, "source") {
                return reference(Kind::Template);
            }

            if ends_with_function(before, "asset") {
                return reference(Kind::Asset);
            }

            if TWIG_ENTRY_FUNCTIONS.iter().any(|function| ends_with_function(before, function)) {
                return reference(Kind::Entry);
            }

            if ends_with_function(before, "is_granted") {
                return reference(Kind::Role);
            }

            if ends_with_function(before, "block") {
                return reference(Kind::Block);
            }

            let mut words = before.rsplit(char::is_whitespace);
            let tag = words.next().unwrap_or_default();
            let opener = words.next().unwrap_or_default();
            if TWIG_TEMPLATE_TAGS.contains(&tag) && opener.starts_with("{%") {
                return reference(Kind::Template);
            }

            let filtered = after.trim_start().strip_prefix('|').map(str::trim_start);
            if filtered.is_some_and(|filter| filter.starts_with("trans")) {
                return reference(Kind::Translation);
            }

            None
        }
        "yaml" => {
            if contents.starts_with("@=") || contents.starts_with("@@") {
                return None;
            }

            if let Some(id) = contents.strip_prefix("@?").or_else(|| contents.strip_prefix('@')) {
                return Some(Reference { kind: Kind::Service, start: end - id.len(), end });
            }

            contents.contains('\\').then_some(Reference { kind: Kind::Class, start, end })
        }
        "xml" => {
            if before.ends_with(" class=") {
                return reference(Kind::Class);
            }

            let service_attribute = ["type=\"service\" id=", " alias=", " parent="].iter().any(|a| before.ends_with(a));
            service_attribute.then_some(Reference { kind: Kind::Service, start, end })
        }
        _ => None,
    }
}

/// An unquoted class name in YAML: a service key or a `class:` / `parent:` value.
fn yaml_class_at(text: &str, at: usize) -> Option<Reference> {
    let (start, end) = word_at(text, at, is_class_byte);
    let word = &text[start..end];
    if !word.contains('\\') || word.ends_with('\\') {
        return None;
    }

    let (line_start, _) = line_bounds(text, start);
    let before = text[line_start..start].trim();
    let is_key = before.is_empty() && text[end..].starts_with(':');
    let is_value = before.ends_with("class:") || before.ends_with("parent:") || before.ends_with("- ");
    (is_key || is_value).then_some(Reference { kind: Kind::Class, start, end })
}

/// The name of `{% block name` at `at`.
fn block_tag_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let code = twig_code_start(text, at)?;
    let tag = text[code + 2..at].trim_start_matches(['-', '~']).trim_start();
    let name = tag.strip_prefix("block")?;
    if !name.starts_with(char::is_whitespace) || !name.trim_start().bytes().all(is_identifier) {
        return None;
    }

    let (start, end) = word_at(text, at, is_identifier);
    Some((start, end))
}

/// The partial identifier being typed at `at` in a Twig expression where a
/// variable can go (not after `.`, `|`, or as a tag name).
pub fn twig_variable_at(text: &str, at: usize) -> Option<(usize, usize)> {
    let code = twig_code_start(text, at)?;
    literal_at("twig", text, at).is_none().then_some(())?;

    let (start, end) = word_at(text, at, is_identifier);
    let before = text[code + 2..start].trim_start_matches(['-', '~']);
    if text[code..].starts_with("{%") && before.trim().is_empty() {
        return None;
    }

    let previous = before.trim_end().bytes().last();
    if matches!(previous, Some(b'.' | b'|')) || text.as_bytes().get(start).is_some_and(u8::is_ascii_digit) {
        return None;
    }

    Some((start, end))
}

/// Every reference in a document, in order.
pub fn references(language: &str, text: &str) -> Vec<Reference> {
    let mut found = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'\'' && bytes[i] != b'"' {
            i += 1;
            continue;
        }

        let opening = literal_at(language, text, i + 1).is_some_and(|(start, _, _)| start == i + 1);
        let reference = opening.then(|| reference_at(language, text, i + 1)).flatten();
        match reference {
            Some(reference) if reference.end >= reference.start => {
                i = reference.end + 1;
                found.push(reference);
            }
            _ => i += 1,
        }
    }

    found
}

/// Top-level arguments of a call whose `(` is at `open`.
pub(crate) fn arguments(text: &str, open: usize) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut part_start = open + 1;
    let mut i = open + 1;

    while i < bytes.len().min(open + MAX_LINE) {
        let b = bytes[i];
        match quote {
            Some(_) if b == b'\\' => i += 1,
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'\'' || b == b'"' => quote = Some(b),
            None if b"([{".contains(&b) => depth += 1,
            None if b")]}".contains(&b) && depth == 0 => {
                parts.push(&text[part_start..i]);
                return parts;
            }
            None if b")]}".contains(&b) => depth -= 1,
            None if b == b',' && depth == 0 => {
                parts.push(&text[part_start..i]);
                part_start = i + 1;
            }
            None => {}
        }
        i += 1;
    }

    parts
}

fn unquote(argument: &str) -> Option<&str> {
    let argument = argument.trim();
    let quote = argument.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    argument.strip_prefix(quote)?.strip_suffix(quote)
}

/// The domain a translation reference names explicitly: the call's `domain`
/// argument, or the template's `{% trans_default_domain %}`.
/// A named argument such as `message:` or `minMessage:` of a constraint: its text is a `validators` key.
fn is_constraint_message(before: &str) -> bool {
    before.strip_suffix(':').is_some_and(|name| name.ends_with("message") || name.ends_with("Message"))
}

pub fn translation_domain(language: &str, text: &str, reference: &Reference) -> Option<String> {
    if language == "php" && is_constraint_message(text[..reference.start - 1].trim_end()) {
        return Some("validators".to_owned());
    }

    let (call_open, position) = if language == "twig" {
        let after = text[reference.end + 1..].trim_start().strip_prefix('|')?.trim_start().strip_prefix("trans")?;
        let after = after.trim_start();
        let open = after.starts_with('(').then(|| text.len() - after.len());
        (open, 1)
    } else {
        let before = text[..reference.start - 1].trim_end();
        (before.ends_with('(').then(|| before.len() - 1), 2)
    };

    if let Some(open) = call_open {
        let args = arguments(text, open);
        let named = args.iter().find_map(|a| a.trim().strip_prefix("domain:")).and_then(unquote);
        if let Some(domain) = named.or_else(|| args.get(position).and_then(|a| unquote(a))) {
            return Some(domain.to_owned());
        }
    }

    if language != "twig" {
        return None;
    }

    let tag = text.find("trans_default_domain")?;
    let rest = &text[tag + "trans_default_domain".len()..];
    unquote(rest.split("%}").next()?.trim_end_matches(['-', '~'])).map(str::to_owned)
}

/// `{% extends 'name' %}` of a template.
pub fn twig_parent(text: &str) -> Option<&str> {
    twig_tags(text).find_map(|(name, rest, _)| (name == "extends").then(|| unquote(rest.split_whitespace().next()?)).flatten())
}

/// `{% block name %}` of a template: names and their offsets.
pub fn twig_blocks(text: &str) -> Vec<(&str, usize)> {
    twig_tags(text)
        .filter(|(name, _, _)| *name == "block")
        .filter_map(|(_, rest, offset)| {
            let name = rest.trim_start();
            let end = name.bytes().position(|b| !is_identifier(b)).unwrap_or(name.len());
            let at = offset + (rest.len() - name.len());
            (end > 0).then(|| (&name[..end], at))
        })
        .collect()
}

/// `{% name rest %}` tags: name, the text after it, and the offset of that text.
pub(crate) fn twig_tags(text: &str) -> impl Iterator<Item = (&str, &str, usize)> {
    text.match_indices("{%").filter_map(|(open, _)| {
        let inside_start = open + 2;
        let close = text[inside_start..].find("%}").map_or(text.len(), |i| inside_start + i);
        let inside = &text[inside_start..close];
        let trimmed = inside.trim_start_matches(['-', '~']).trim_start();
        let name_len = trimmed.bytes().position(|b| !is_identifier(b)).unwrap_or(trimmed.len());
        let rest = &trimmed[name_len..];
        let rest_offset = inside_start + (inside.len() - rest.len());
        Some((&trimmed[..name_len], rest.trim_end_matches(['-', '~']), rest_offset))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_at(language: &str, marked: &str) -> Option<(Kind, String)> {
        let at = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        reference_at(language, &text, at).map(|r| (r.kind, text[r.start..r.end].to_owned()))
    }

    fn some(kind: Kind, name: &str) -> Option<(Kind, String)> {
        Some((kind, name.into()))
    }

    #[test]
    fn detects_php_references() {
        assert_eq!(kind_at("php", "$this->redirectToRoute('blog_|show', [])"), some(Kind::Route, "blog_show"));
        assert_eq!(kind_at("php", "$router->generate(\"ho|"), some(Kind::Route, "ho"));
        assert_eq!(kind_at("php", "return $this->render('blog/|index.html.twig');"), some(Kind::Template, "blog/index.html.twig"));
        assert_eq!(kind_at("php", "#[Template('a|.html.twig')]"), some(Kind::Template, "a.html.twig"));
        assert_eq!(kind_at("php", "$translator->trans('app.|title')"), some(Kind::Translation, "app.title"));
        assert_eq!(kind_at("php", "echo t('x|')"), some(Kind::Translation, "x"));
        assert_eq!(kind_at("php", "$this->container->get('rou|ter')"), some(Kind::Service, "router"));
        assert_eq!(kind_at("php", "->args([service('mail|er')])"), some(Kind::Service, "mailer"));
        assert_eq!(kind_at("php", "$this->denyAccessUnlessGranted('ROLE_|')"), some(Kind::Role, "ROLE_"));
        assert_eq!(kind_at("php", "#[IsGranted('ROLE_AD|MIN')]"), some(Kind::Role, "ROLE_ADMIN"));
        assert_eq!(kind_at("php", "$packages->getUrl('img/|a.png')"), some(Kind::Asset, "img/a.png"));
        assert_eq!(kind_at("php", "$request->get('pa|ge')"), None);
        assert_eq!(kind_at("php", "$this->set('x|')"), None);
        assert_eq!(kind_at("php", "$x = 'it\\'s |here'"), None);
    }

    #[test]
    fn detects_twig_references() {
        assert_eq!(kind_at("twig", "<a href=\"{{ path('blog_|show', {id: 1}) }}\">"), some(Kind::Route, "blog_show"));
        assert_eq!(kind_at("twig", "{% extends 'base|.html.twig' %}"), some(Kind::Template, "base.html.twig"));
        assert_eq!(kind_at("twig", "{%- include \"part|"), some(Kind::Template, "part"));
        assert_eq!(kind_at("twig", "{{ include('a|') }}"), some(Kind::Template, "a"));
        assert_eq!(kind_at("twig", "{{ 'app.|title'|trans }}"), some(Kind::Translation, "app.title"));
        assert_eq!(kind_at("twig", "{{ 'x|' | trans({}, 'messages') }}"), some(Kind::Translation, "x"));
        assert_eq!(kind_at("twig", "<img src=\"{{ asset('img/|logo.png') }}\">"), some(Kind::Asset, "img/logo.png"));
        assert_eq!(kind_at("twig", "{{ encore_entry_link_tags('ap|p') }}"), some(Kind::Entry, "app"));
        assert_eq!(kind_at("twig", "{% if is_granted('ROLE_|') %}"), some(Kind::Role, "ROLE_"));
        assert_eq!(kind_at("twig", "{{ block('tit|le') }}"), some(Kind::Block, "title"));
        assert_eq!(kind_at("twig", "{% block bo|dy %}"), some(Kind::Block, "body"));
        assert_eq!(kind_at("twig", "{%- block |"), some(Kind::Block, ""));
        assert_eq!(kind_at("twig", "{{ 'plain|'|upper }}"), None);
        assert_eq!(kind_at("twig", "{% block body %}te|xt"), None);
    }

    #[test]
    fn detects_yaml_and_xml_references() {
        assert_eq!(kind_at("yaml", "    arguments: ['@rou|ter']"), some(Kind::Service, "router"));
        assert_eq!(kind_at("yaml", "    $logger: '@?log|ger'"), some(Kind::Service, "logger"));
        assert_eq!(kind_at("yaml", "    expr: '@=serv|ice(\"x\")'"), None);
        assert_eq!(kind_at("yaml", "    path: '/bl|og'"), None);
        assert_eq!(kind_at("yaml", "    App\\Mai|ler:"), some(Kind::Class, "App\\Mailer"));
        assert_eq!(kind_at("yaml", "        class: App\\Mai|ler"), some(Kind::Class, "App\\Mailer"));
        assert_eq!(kind_at("yaml", "    App\\|:"), None);
        assert_eq!(kind_at("xml", "<argument type=\"service\" id=\"rou|ter\"/>"), some(Kind::Service, "router"));
        assert_eq!(kind_at("xml", "<service id=\"a\" class=\"App\\|X\"/>"), some(Kind::Class, "App\\X"));
    }

    #[test]
    fn finds_every_reference_of_a_document() {
        let text = "<?php\n$this->render('a.html.twig', ['x' => 'y']);\n$this->generateUrl(\"home\");\n";
        let found: Vec<_> = references("php", text).into_iter().map(|r| (r.kind, &text[r.start..r.end])).collect();
        assert_eq!(found, [(Kind::Template, "a.html.twig"), (Kind::Route, "home")]);
    }

    #[test]
    fn finds_translation_domains() {
        let domain = |language: &str, text: &str| {
            let reference = references(language, text).into_iter().find(|r| r.kind == Kind::Translation).unwrap();
            translation_domain(language, text, &reference)
        };
        assert_eq!(domain("php", "$t->trans('k', ['%n%' => 1], 'admin');"), Some("admin".into()));
        assert_eq!(domain("php", "$t->trans('k', domain: 'admin');"), Some("admin".into()));
        assert_eq!(domain("php", "$t->trans('k');"), None);
        assert_eq!(domain("twig", "{{ 'k'|trans({'%n%': 1}, 'admin') }}"), Some("admin".into()));
        assert_eq!(domain("twig", "{% trans_default_domain 'forms' %}{{ 'k'|trans }}"), Some("forms".into()));
        assert_eq!(domain("twig", "{{ 'k'|trans }}"), None);
    }

    #[test]
    fn finds_twig_variable_positions() {
        let at = |marked: &str| {
            let at = marked.find('^').unwrap();
            let text = marked.replacen('^', "", 1);
            twig_variable_at(&text, at).map(|(s, e)| text[s..e].to_owned())
        };
        assert_eq!(at("{{ po^ }}"), Some("po".into()));
        assert_eq!(at("{% if us^ %}"), Some("us".into()));
        assert_eq!(at("{{ user.na^ }}"), None);
        assert_eq!(at("{{ x|up^ }}"), None);
        assert_eq!(at("{% fo^ %}"), None);
        assert_eq!(at("{{ 'te^xt' }}"), None);
        assert_eq!(at("text^ {{ x }}"), None);
    }

    #[test]
    fn reads_twig_inheritance() {
        let text = "{% extends 'base.html.twig' %}\n{% block title %}T{% endblock %}\n{%- block body -%}{% endblock %}";
        assert_eq!(twig_parent(text), Some("base.html.twig"));
        let blocks: Vec<_> = twig_blocks(text).into_iter().map(|(n, at)| (n, &text[at..at + n.len()])).collect();
        assert_eq!(blocks, [("title", "title"), ("body", "body")]);
    }

    #[test]
    fn constraint_messages_that_look_like_keys_are_translations_of_the_validators_domain() {
        let text = "#[Assert\\NotBlank(message: 'post.title.blank')]";
        let at = text.find("blank").unwrap();
        let reference = reference_at("php", text, at).unwrap();
        assert_eq!(reference.kind, Kind::Translation);
        assert_eq!(translation_domain("php", text, &reference).as_deref(), Some("validators"));

        assert!(reference_at("php", "#[Assert\\NotBlank(message: 'Must not be blank.')]", 30).is_none());
        assert!(reference_at("php", "#[Assert\\NotBlank(message: 'blank')]", 30).is_none());
    }
}
