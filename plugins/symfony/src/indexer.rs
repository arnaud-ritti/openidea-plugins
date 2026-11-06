//! Project-wide navigation for the host's document index. Keys: `service#id`,
//! `route#name`, `command#name`, `param#name`, `twig-filter#`, `twig-function#`,
//! `twig-test#`, `twig-component#`, `stimulus#`, `event#`, `event-dispatch#`,
//! `macro#`, `voter#`, `validator-of#`, `constraint-of#`; a definition is where the file
//! names it, a reference is a use. Gutter markers use keys of their own
//! (`template#`, `render#`, `block#`, `override#`, `class#`, `class-service#`,
//! `route-use#`), so one icon can open several targets. Reuses the scanners of
//! the project index.

use crate::index::{self, Sink};
use crate::php_checks::{arguments, named, trimmed};
use crate::php_text::{attributes, class_model, mask, matching_close, read_word, resolver, skip_whitespace};
use crate::refs::{self, Kind};
use crate::Main;
use openidea_plugin::indexing::{Anchor, Document, DocumentIndexer, Edges};
use openidea_plugin::text;
use openidea_plugin::{Position, Range};

const MAX_ANCHORS: usize = 4096;

fn language(path: &str) -> Option<&'static str> {
    match path.rsplit('.').next()? {
        "php" => Some("php"),
        "yaml" | "yml" => Some("yaml"),
        "xml" => Some("xml"),
        "twig" => Some("twig"),
        "js" | "ts" => Some("js"),
        _ => None,
    }
}

fn width_range(at: Position, name: &str) -> Range {
    let end = Position { line: at.line, character: at.character + name.encode_utf16().count() as u32 };
    Range::new(at, end)
}

fn anchors(prefix: &str, found: Vec<(String, Position)>) -> impl Iterator<Item = Anchor> + '_ {
    found.into_iter().map(move |(name, at)| Anchor { key: format!("{prefix}#{name}"), range: width_range(at, &name) })
}

#[derive(Default)]
struct Found(Vec<(String, Option<String>, Position)>);

impl Sink for Found {
    fn add(&mut self, name: &str, detail: Option<String>, _path: &str, at: Position) {
        if !name.is_empty() {
            self.0.push((name.to_owned(), detail, at));
        }
    }
}

impl Found {
    fn names(&self) -> Vec<(String, Position)> {
        self.0.iter().map(|(name, _, at)| (name.clone(), *at)).collect()
    }
}

#[derive(Default)]
struct Collected {
    edges: Edges,
}

impl Collected {
    fn definition(&mut self, key: String, range: Range) {
        self.edges.definitions.push(Anchor { key, range });
    }

    fn reference(&mut self, key: String, range: Range) {
        self.edges.references.push(Anchor { key, range });
    }

    fn marker(&mut self, key: String, range: Range) {
        self.edges.markers.push(Anchor { key, range });
    }
}

fn template_of(path: &str) -> Option<String> {
    let (_, relative) = path.rsplit_once("/templates/")?;
    Some(index::template_name(relative))
}

fn component_of(path: &str) -> Option<String> {
    let (_, relative) = path.rsplit_once("/templates/components/")?;
    let name = relative.strip_suffix(".html.twig").or_else(|| relative.strip_suffix(".twig"))?;
    Some(name.replace('/', ":"))
}

fn stimulus_of(path: &str) -> Option<String> {
    let (_, relative) = path.rsplit_once("/assets/controllers/")?;
    let name = relative.strip_suffix(".js").or_else(|| relative.strip_suffix(".ts"))?;
    let name = name.strip_suffix("_controller").unwrap_or(name);
    Some(name.replace('/', "--").replace('_', "-"))
}

fn top() -> Range {
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

fn name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// `%name%` placeholders (not `%%` and not `%env(X)%`).
fn percent_parameters(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut from = 0;

    while let Some(found_at) = text[from..].find('%') {
        let start = from + found_at + 1;
        if bytes.get(start) == Some(&b'%') {
            from = start + 1;
            continue;
        }

        let length = bytes[start.min(bytes.len())..].iter().take_while(|b| name_char(**b)).count();
        if length > 0 && bytes.get(start + length) == Some(&b'%') {
            found.push((start, start + length));
            from = start + length + 1;
            continue;
        }

        from = start;
    }

    found
}

/// Names that follow `marker` made of `chars`, with their byte ranges.
fn after(text: &str, marker: &str, chars: impl Fn(u8) -> bool) -> Vec<(usize, usize)> {
    text.match_indices(marker)
        .filter_map(|(at, matched)| {
            let start = at + matched.len();
            let length = text.as_bytes()[start..].iter().take_while(|b| chars(**b)).count();
            (length > 0).then_some((start, start + length))
        })
        .collect()
}

fn quoted_after(text: &str, call: &str) -> Vec<(usize, usize)> {
    text.match_indices(call)
        .filter_map(|(at, matched)| {
            let start = at + matched.len();
            let rest = &text[start..];
            let skipped = rest.len() - rest.trim_start().len();
            let quote = rest.trim_start().chars().next().filter(|c| *c == '\'' || *c == '"')?;
            let from = start + skipped + 1;
            let length = text[from..].find(quote)?;
            (length > 0).then_some((from, from + length))
        })
        .collect()
}

fn identifier(text: &str, from: usize, end: usize) -> (usize, usize) {
    let bytes = text.as_bytes();
    let mut start = from;
    while start < end && bytes[start] == b' ' {
        start += 1;
    }

    let mut stop = start;
    while stop < end && (refs::is_identifier(bytes[stop])) {
        stop += 1;
    }

    if start < end && bytes[start].is_ascii_digit() { (start, start) } else { (start, stop) }
}

/// Filters (`|x`), functions (`x(`) and tests (`is x`) inside `{{ }}` and `{% %}`: (kind, start, end).
fn twig_uses(text: &str) -> Vec<(&'static str, usize, usize)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut from = 0;

    while let Some(open) = text[from..].find('{').map(|i| from + i) {
        let close = match bytes.get(open + 1) {
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            _ => {
                from = open + 1;
                continue;
            }
        };
        let start = open + 2;
        let end = text[start..].find(close).map_or(text.len(), |i| start + i);
        let mut i = start;

        while i < end {
            let previous = if i == start { b' ' } else { bytes[i - 1] };
            if bytes[i] == b'|' && previous != b'|' && bytes.get(i + 1) != Some(&b'|') {
                let (name_start, name_end) = identifier(text, i + 1, end);
                if name_end > name_start {
                    found.push(("filter", name_start, name_end));
                }
                i = name_end.max(i + 1);
                continue;
            }

            if refs::is_identifier(bytes[i]) && !refs::is_identifier(previous) && previous != b'.' {
                let mut stop = i;
                while stop < end && refs::is_identifier(bytes[stop]) {
                    stop += 1;
                }

                if &text[i..stop] == "is" {
                    let (mut name_start, mut name_end) = identifier(text, stop, end);
                    if &text[name_start..name_end] == "not" {
                        (name_start, name_end) = identifier(text, name_end, end);
                    }
                    if name_end > name_start {
                        found.push(("test", name_start, name_end));
                    }
                    i = name_end.max(stop);
                    continue;
                }

                if bytes.get(stop) == Some(&b'(') && !bytes[i].is_ascii_digit() {
                    found.push(("function", i, stop));
                }
                i = stop;
                continue;
            }

            i += 1;
        }

        from = end;
    }

    found
}

fn php(path: &str, text: &str, out: &mut Collected) {
    let lines_of = |name: &str, at: Position| width_range(at, name);

    let mut commands = Found::default();
    index::php_commands(path, text, &mut commands);
    for (name, at) in commands.names() {
        out.definition(format!("command#{name}"), lines_of(&name, at));
    }

    for (kind, name, at) in index::php_twig_extensions(text) {
        out.definition(format!("twig-{kind}#{name}"), lines_of(&name, at));
    }

    for (name, at) in index::php_components(text) {
        out.definition(format!("twig-component#{name}"), lines_of(&name, at));
    }

    for render in index::php_renders(path, text) {
        out.definition(format!("render#{}", render.template), lines_of(&render.template, render.at));
    }

    if let Some((class, offset)) = index::php_class(text) {
        let short = class.rsplit('\\').next().unwrap_or(&class);
        let range = lines_of(short, text::position(text, offset));
        out.definition(format!("class#{class}"), range);
        out.marker(format!("class-service#{class}"), range);
    }

    let masked = mask(text);
    php_events(text, &masked, out);
    php_voters(text, &masked, out);
    php_validators(text, out);
    granted_references(text, out);

    for (start, end) in quoted_after(text, "getParameter(").into_iter().chain(quoted_after(text, "hasParameter(")) {
        out.reference(format!("param#{}", &text[start..end]), range_of(text, start, end));
    }

    for (start, end) in quoted_after(text, "->find(") {
        out.reference(format!("command#{}", &text[start..end]), range_of(text, start, end));
    }
}

const KERNEL_EVENTS_CLASS: &str = "Symfony\\Component\\HttpKernel\\KernelEvents";
const KERNEL_EVENTS: [(&str, &str); 8] = [
    ("REQUEST", "kernel.request"),
    ("CONTROLLER", "kernel.controller"),
    ("CONTROLLER_ARGUMENTS", "kernel.controller_arguments"),
    ("VIEW", "kernel.view"),
    ("RESPONSE", "kernel.response"),
    ("FINISH_REQUEST", "kernel.finish_request"),
    ("TERMINATE", "kernel.terminate"),
    ("EXCEPTION", "kernel.exception"),
];
const AS_EVENT_LISTENER: &str = "Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener";
const CONSTRAINT: &str = "Symfony\\Component\\Validator\\Constraint";
const CONSTRAINT_VALIDATOR: &str = "Symfony\\Component\\Validator\\ConstraintValidator";
const GRANTED_CALLS: [&str; 3] = ["isGranted(", "denyAccessUnlessGranted(", "is_granted("];

/// The event an expression names (`'kernel.request'`, `Foo::class`,
/// `KernelEvents::REQUEST`) and the byte range to anchor.
fn event_of(text: &str, masked: &str, resolve: &dyn Fn(&str) -> String, expression: std::ops::Range<usize>) -> Option<(String, std::ops::Range<usize>)> {
    let expression = trimmed(masked, expression);
    let raw = &text[expression.clone()];

    if let Some(quote) = raw.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        let inner = raw.strip_prefix(quote)?.strip_suffix(quote)?;
        let valid = !inner.is_empty() && !inner.contains(['$', '{', quote, '\\']);
        return valid.then(|| (inner.to_owned(), expression.start + 1..expression.end - 1));
    }

    if let Some(class) = raw.strip_suffix("::class") {
        let class = class.trim_end();
        let valid = !class.is_empty() && class.bytes().all(|b| refs::is_identifier(b) || b == b'\\');
        return valid.then(|| (resolve(class), expression.start..expression.start + class.len()));
    }

    let (owner, constant) = raw.split_once("::")?;
    if resolve(owner.trim()) != KERNEL_EVENTS_CLASS {
        return None;
    }

    KERNEL_EVENTS.iter().find(|(name, _)| *name == constant.trim()).map(|(_, event)| ((*event).to_owned(), expression))
}

fn top_level_arrow(masked: &str, item: &std::ops::Range<usize>) -> Option<usize> {
    let mut depth = 0usize;
    for at in item.clone() {
        match masked.as_bytes()[at] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b'=' if depth == 0 && masked[at..].starts_with("=>") => return Some(at),
            _ => {}
        }
    }
    None
}

fn listener(out: &mut Collected, text: &str, event: (String, std::ops::Range<usize>)) {
    let range = range_of(text, event.1.start, event.1.end);
    out.definition(format!("event#{}", event.0), range);
    out.marker(format!("event-dispatch#{}", event.0), range);
}

fn php_events(text: &str, masked: &str, out: &mut Collected) {
    let resolve = resolver(text);

    if let Some(at) = masked.find("function getSubscribedEvents") {
        let body = masked[at..].find('{').map(|found| at + found).and_then(|open| Some(open..matching_close(masked, open)?));
        let list = body.and_then(|body| Some((masked[body.clone()].find("return")? + body.start, body)));
        let open = list.and_then(|(from, body)| Some(from + masked[from..body.end].find('[')?));
        for item in open.map(|open| arguments(masked, open)).unwrap_or_default() {
            let Some(arrow) = top_level_arrow(masked, &item) else { continue };
            if let Some(event) = event_of(text, masked, &resolve, item.start..arrow) {
                listener(out, text, event);
            }
        }
    }

    for attribute in attributes(text).into_iter().filter(|attribute| attribute.name == AS_EVENT_LISTENER) {
        if !masked[attribute.range.clone()].contains('(') {
            continue;
        }

        let list = arguments(masked, attribute.arguments.start - 1);
        let expression = list.iter().find_map(|argument| named(masked, argument.clone()).filter(|(name, _)| &masked[name.clone()] == "event").map(|(_, value)| value)).or_else(|| list.first().filter(|first| named(masked, (*first).clone()).is_none()).cloned());
        if let Some(event) = expression.and_then(|expression| event_of(text, masked, &resolve, expression)) {
            listener(out, text, event);
        }
    }

    for (at, _) in masked.match_indices("->dispatch(") {
        let open = at + "->dispatch".len();
        let list = arguments(masked, open);
        let Some(first) = list.first().map(|first| trimmed(masked, first.clone())) else { continue };

        let event = if masked[first.clone()].starts_with("new ") {
            let name = read_word(masked, skip_whitespace(masked, first.start + 4));
            (!name.is_empty()).then(|| (resolve(&text[name.clone()]), name))
        } else {
            list.get(1).and_then(|second| event_of(text, masked, &resolve, second.clone()))
        };

        let Some((key, range)) = event else { continue };
        let range = range_of(text, range.start, range.end);
        out.reference(format!("event#{key}"), range);
        out.marker(format!("event#{key}"), range);
        out.definition(format!("event-dispatch#{key}"), range);
    }
}

fn php_voters(text: &str, masked: &str, out: &mut Collected) {
    let Some(model) = class_model(text) else { return };
    let is_voter = text.contains("VoterInterface") || model.parent.as_deref().is_some_and(|parent| parent.ends_with("\\Voter"));
    let Some(supports) = model.method("supports").filter(|_| is_voter) else { return };
    let Some(open) = masked[supports.parameters.end..model.body.end].find('{').map(|found| supports.parameters.end + found) else { return };
    let Some(close) = matching_close(masked, open) else { return };

    let attribute_like = |value: &str| value.len() > 1 && value.starts_with(|c: char| c.is_ascii_uppercase()) && value.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    let mut define = |start: usize, end: usize| {
        out.definition(format!("voter#{}", &text[start..end]), range_of(text, start, end));
    };

    let mut at = open;
    while at < close {
        let byte = masked.as_bytes()[at];
        if matches!(byte, b'\'' | b'"') {
            let Some(length) = masked[at + 1..close].bytes().position(|b| b == byte) else { break };
            if attribute_like(&text[at + 1..at + 1 + length]) {
                define(at + 1, at + 1 + length);
            }
            at += length + 2;
            continue;
        }
        at += 1;
    }

    for owner in ["self::", "static::"] {
        for (found, _) in masked[open..close].match_indices(owner) {
            let name = read_word(masked, open + found + owner.len());
            let declaration = format!("const {}", &masked[name.clone()]);
            let Some(declared) = masked[model.body.clone()].find(&declaration).map(|position| model.body.start + position) else { continue };
            let after = skip_whitespace(masked, declared + declaration.len());
            let value = skip_whitespace(masked, after + 1);
            let quote = masked.as_bytes().get(value).copied().filter(|b| matches!(b, b'\'' | b'"'));
            let length = quote.and_then(|quote| masked[value + 1..].bytes().position(|b| b == quote));
            if let Some(length) = length.filter(|length| attribute_like(&text[value + 1..value + 1 + length])) {
                define(value + 1, value + 1 + length);
            }
        }
    }
}

fn php_validators(text: &str, out: &mut Collected) {
    let Some(model) = class_model(text) else { return };
    let range = range_of(text, model.name_range.start, model.name_range.end);

    match model.parent.as_deref() {
        Some(CONSTRAINT) => {
            out.definition(format!("constraint-of#{}", model.fqcn), range);
            out.marker(format!("validator-of#{}", model.fqcn), range);
        }
        Some(CONSTRAINT_VALIDATOR) => {
            let Some(constraint) = model.fqcn.strip_suffix("Validator") else { return };
            out.definition(format!("validator-of#{constraint}"), range);
            out.marker(format!("constraint-of#{constraint}"), range);
        }
        _ => {}
    }
}

fn granted_references(text: &str, out: &mut Collected) {
    for call in GRANTED_CALLS {
        for (start, end) in quoted_after(text, call) {
            out.reference(format!("voter#{}", &text[start..end]), range_of(text, start, end));
        }
    }
}

/// `{% macro name() %}`, `{% from 'x' import name %}` and `{% import 'x' as m %}` with `m.name(`.
fn twig_macros(text: &str, own: Option<&str>, out: &mut Collected) {
    let mut aliases: Vec<(String, String)> = Vec::new();
    let template_of = |source: &str| -> Option<String> {
        let source = source.trim();
        if source == "_self" {
            return own.map(str::to_owned);
        }
        source.strip_prefix(['\'', '"']).and_then(|rest| rest.strip_suffix(['\'', '"'])).map(str::to_owned)
    };

    for (name, rest, offset) in refs::twig_tags(text) {
        let rest_end = rest.find("%}").map_or(rest.len(), |at| at);
        let rest = rest[..rest_end].trim_end_matches(['-', '~']);

        match name {
            "macro" => {
                let trimmed_rest = rest.trim_start();
                let length = trimmed_rest.bytes().take_while(|b| refs::is_identifier(*b)).count();
                if let (Some(own), true) = (own, length > 0) {
                    let start = offset + (rest.len() - trimmed_rest.len());
                    out.definition(format!("macro#{own}::{}", &text[start..start + length]), range_of(text, start, start + length));
                }
            }
            "from" => {
                let Some((source, imported)) = rest.split_once(" import ") else { continue };
                let Some(template) = template_of(source) else { continue };
                let mut cursor = offset + source.len() + " import ".len();
                for item in imported.split(',') {
                    let leading = item.len() - item.trim_start().len();
                    let name = item.trim().split_whitespace().next().unwrap_or_default();
                    if !name.is_empty() && name.bytes().all(refs::is_identifier) {
                        let start = cursor + leading;
                        out.reference(format!("macro#{template}::{name}"), range_of(text, start, start + name.len()));
                    }
                    cursor += item.len() + 1;
                }
            }
            "import" => {
                let Some((source, alias)) = rest.rsplit_once(" as ") else { continue };
                if let Some(template) = template_of(source) {
                    aliases.push((alias.trim().to_owned(), template));
                }
            }
            _ => {}
        }
    }

    for (alias, template) in aliases {
        let call = format!("{alias}.");
        for (at, _) in text.match_indices(&call) {
            let before = text[..at].bytes().last();
            let start = at + call.len();
            let length = text[start..].bytes().take_while(|b| refs::is_identifier(*b)).count();
            let glued = before.is_some_and(|b| refs::is_identifier(b) || b == b'.');
            if !glued && length > 0 && text[start + length..].starts_with('(') {
                out.reference(format!("macro#{template}::{}", &text[start..start + length]), range_of(text, start, start + length));
            }
        }
    }
}

/// `kernel.event_listener` tags with an `event` attribute, in YAML (one line) and XML.
fn config_listeners(language: &str, text: &str, out: &mut Collected) {
    if language == "xml" {
        for (tag, at) in index::xml_tags(text, "tag") {
            let is_listener = index::xml_attribute(tag, "name").is_some_and(|(name, _)| name == "kernel.event_listener");
            if let (true, Some((event, offset))) = (is_listener, index::xml_attribute(tag, "event")) {
                listener(out, text, (event.replace("\\\\", "\\"), at + offset..at + offset + event.len()));
            }
        }
        return;
    }

    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        if !line.contains("kernel.event_listener") {
            continue;
        }

        let Some(found) = line.find("event:") else { continue };
        let value_start = found + "event:".len();
        let value = index::without_comment(&line[value_start..]);
        let value = value.split([',', '}']).next().unwrap_or(value);
        let (start, end) = index::scalar_slot(value, line_start + value_start);
        if start < end {
            listener(out, text, (text[start..end].replace("\\\\", "\\"), start..end));
        }
    }
}

fn range_of(text: &str, start: usize, end: usize) -> Range {
    Range::new(text::position(text, start), text::position(text, end))
}

fn config(path: &str, language: &str, text: &str, services: &Found, out: &mut Collected) {
    for (name, detail, at) in &services.0 {
        let class = detail.as_deref().filter(|d| !d.starts_with("alias of ")).unwrap_or(name);
        if class.contains('\\') {
            out.definition(format!("class-service#{class}"), width_range(*at, name));
        }
    }

    let mut parameters = Found::default();
    if language == "yaml" {
        index::yaml_params(path, text, &mut parameters);
    } else {
        index::xml_params(path, text, &mut parameters);
    }
    for (name, at) in parameters.names() {
        out.definition(format!("param#{name}"), width_range(at, &name));
    }

    config_listeners(language, text, out);

    for (start, end) in percent_parameters(text) {
        out.reference(format!("param#{}", &text[start..end]), range_of(text, start, end));
    }

    if language == "yaml" {
        for key in index::yaml_keys(text) {
            let class = match key.parents.as_slice() {
                [services] if services == "services" => &key.key,
                [services, _] if services == "services" && key.key == "class" => &key.value,
                _ => continue,
            };
            let class = class.trim_matches(['\'', '"']);
            if !class.is_empty() {
                let at = text::position(text, key.offset);
                out.marker(format!("class#{class}"), width_range(at, &key.key));
            }
        }
    }
}

fn twig(path: &str, text: &str, out: &mut Collected) {
    let own = template_of(path);

    if let Some(name) = &own {
        out.definition(format!("template#{name}"), top());
        out.marker(format!("render#{name}"), top());
    }

    if let Some(name) = component_of(path) {
        out.definition(format!("twig-component#{name}"), top());
    }

    twig_macros(text, own.as_deref(), out);
    granted_references(text, out);

    let parent = refs::twig_parent(text);
    for (block, offset) in refs::twig_blocks(text) {
        let range = range_of(text, offset, offset + block.len());
        if let Some(own) = &own {
            out.definition(format!("block#{own}::{block}"), range);
            out.marker(format!("override#{own}::{block}"), range);
        }
        if let Some(parent) = parent {
            out.definition(format!("override#{parent}::{block}"), range);
            out.marker(format!("block#{parent}::{block}"), range);
        }
    }

    for (kind, start, end) in twig_uses(text) {
        out.reference(format!("twig-{kind}#{}", &text[start..end]), range_of(text, start, end));
    }

    let components = after(text, "<twig:", |b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':'))
        .into_iter()
        .chain(quoted_after(text, "component("));
    for (start, end) in components {
        out.reference(format!("twig-component#{}", &text[start..end]), range_of(text, start, end));
    }

    for (start, end) in quoted_after(text, "stimulus_controller(") {
        out.reference(format!("stimulus#{}", &text[start..end]), range_of(text, start, end));
    }

    for (at, matched) in text.match_indices("data-controller=") {
        let value_start = at + matched.len() + 1;
        let Some(quote) = text[value_start - 1..].chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        let Some(length) = text[value_start..].find(quote) else { continue };
        let mut offset = value_start;
        for token in text[value_start..value_start + length].split(' ') {
            if !token.is_empty() {
                out.reference(format!("stimulus#{token}"), range_of(text, offset, offset + token.len()));
            }
            offset += token.len() + 1;
        }
    }
}

impl DocumentIndexer for Main {
    fn index(&mut self, doc: &Document) -> Edges {
        let path = doc.path.as_str();
        let text = doc.text.as_str();
        let Some(language) = language(path) else { return Edges::default() };
        if path.contains("/vendor/") || path.contains("/var/cache/") || path.contains("/node_modules/") {
            return Edges::default();
        }

        let mut services = Found::default();
        let mut routes: Vec<(String, Position)> = Vec::new();
        let mut out = Collected::default();
        match language {
            "php" => {
                index::php_routes(path, text, &mut routes);
                php(path, text, &mut out);
            }
            "yaml" => {
                index::yaml_services(path, text, &mut services);
                if path.contains("/config/routes") {
                    index::yaml_routes(path, text, &mut routes);
                }
                config(path, language, text, &services, &mut out);
            }
            "xml" => {
                index::xml_services(path, text, &mut services);
                index::xml_routes(path, text, &mut routes);
                config(path, language, text, &services, &mut out);
            }
            "twig" => twig(path, text, &mut out),
            _ => {
                if let Some(name) = stimulus_of(path) {
                    out.definition(format!("stimulus#{name}"), top());
                }
            }
        }

        for (name, at) in &routes {
            out.marker(format!("route-use#{name}"), width_range(*at, name));
        }

        let mut definitions: Vec<Anchor> =
            anchors("service", services.names()).chain(anchors("route", routes)).chain(out.edges.definitions).collect();
        let mut references = out.edges.references;
        for reference in refs::references(language, text) {
            let name = &text[reference.start..reference.end];
            let range = range_of(text, reference.start, reference.end);
            let prefix = match reference.kind {
                Kind::Service => "service",
                Kind::Route => {
                    definitions.push(Anchor { key: format!("route-use#{name}"), range });
                    "route"
                }
                Kind::Template if language == "php" => {
                    out.edges.markers.push(Anchor { key: format!("template#{name}"), range });
                    continue;
                }
                _ => continue,
            };
            references.push(Anchor { key: format!("{prefix}#{name}"), range });
        }

        for (start, end) in after(text, "bin/console ", |b| b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-')) {
            references.push(Anchor { key: format!("command#{}", &text[start..end]), range: range_of(text, start, end) });
        }

        let mut markers = out.edges.markers;
        definitions.truncate(MAX_ANCHORS);
        references.truncate(MAX_ANCHORS);
        markers.truncate(MAX_ANCHORS);
        Edges { definitions, references, aliases: Vec::new(), markers }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(path: &str, text: &str) -> Edges {
        Main::default().index(&Document { path: path.into(), text: text.into(), symbols: Vec::new() })
    }

    fn keys(anchors: &[Anchor]) -> Vec<&str> {
        anchors.iter().map(|a| a.key.as_str()).collect()
    }

    #[test]
    fn yaml_services_are_definitions_and_at_references() {
        let text = "services:\n    app.mailer:\n        class: App\\Mailer\n    app.sender:\n        arguments: ['@app.mailer']\n";
        let edges = index("/p/config/services.yaml", text);
        assert!(keys(&edges.definitions).contains(&"service#app.mailer"));
        assert!(keys(&edges.definitions).contains(&"service#app.sender"));
        assert!(keys(&edges.references).contains(&"service#app.mailer"));
    }

    #[test]
    fn php_attribute_routes_and_generate_calls() {
        let text = "<?php\nclass BlogController {\n    #[Route('/blog', name: 'blog_list')]\n    public function list() { return $this->generateUrl('blog_show'); }\n}\n";
        let edges = index("/p/src/Controller/BlogController.php", text);
        assert_eq!(keys(&edges.definitions)[0], "route#blog_list");
        assert!(keys(&edges.references).contains(&"route#blog_show"));
        let at = edges.definitions[0].range;
        assert_eq!((at.start.line, at.end.character - at.start.character), (2, 9));
    }

    #[test]
    fn xml_services_and_twig_path_calls() {
        let xml = "<container>\n  <services>\n    <service id=\"app.x\" class=\"App\\X\"/>\n  </services>\n</container>\n";
        assert!(keys(&index("/p/config/services.xml", xml).definitions).contains(&"service#app.x"));

        let twig = index("/p/templates/a.html.twig", "<a href=\"{{ path('blog_list') }}\">x</a>");
        assert!(keys(&twig.references).contains(&"route#blog_list"));
    }

    #[test]
    fn vendor_and_unrelated_files_publish_nothing() {
        let text = "<?php\n#[Route('/x', name: 'x')]\nfunction a() {}\n";
        assert!(index("/p/vendor/a/B.php", text).definitions.is_empty());
        assert!(index("/p/README.md", text).definitions.is_empty());
    }

    fn defined(edges: &Edges, key: &str) -> bool {
        keys(&edges.definitions).contains(&key)
    }

    #[test]
    fn console_commands_and_their_uses() {
        let php = "<?php\n#[AsCommand(name: 'app:sync', description: 'x')]\nclass SyncCommand {\n    protected static $defaultName = 'app:old';\n}\n// $app->find('app:sync'); bin/console app:old -v\n";
        let edges = index("/p/src/Command/SyncCommand.php", php);
        assert!(defined(&edges, "command#app:sync") && defined(&edges, "command#app:old"));
        assert_eq!(keys(&edges.references), ["command#app:sync", "command#app:old"]);
    }

    #[test]
    fn parameters_are_defined_in_yaml_and_xml_and_used_by_percent_and_get_parameter() {
        let yaml = "parameters:\n    app.limit: 5\nservices:\n    app.x:\n        arguments: ['%app.limit%', '%%', '%env(FOO)%']\n";
        let edges = index("/p/config/services.yaml", yaml);
        assert!(defined(&edges, "param#app.limit"));
        assert_eq!(keys(&edges.references), ["param#app.limit"]);

        let xml = index("/p/config/p.xml", "<container><parameters><parameter key=\"app.mail\">a</parameter></parameters></container>");
        assert!(defined(&xml, "param#app.mail"));

        let php = index("/p/src/A.php", "<?php\n$this->getParameter('app.limit');");
        assert_eq!(keys(&php.references), ["param#app.limit"]);
    }

    #[test]
    fn twig_extensions_are_defined_in_php_and_used_in_templates() {
        let php = "<?php\nclass E {\n  function f() { return [new TwigFilter('price', [$this, 'p']), new TwigFunction('asset_v', 'x'), new TwigTest('even', 'y')]; }\n  #[AsTwigFilter(name: 'slugify')] function s() {}\n}\n";
        let edges = index("/p/src/Twig/E.php", php);
        for key in ["twig-filter#price", "twig-function#asset_v", "twig-test#even", "twig-filter#slugify"] {
            assert!(defined(&edges, key), "{key}");
        }

        let twig = "{{ total|price|raw }} {{ asset_v('a') }} {% if n is not even and a || b %}{% endif %}";
        let edges = index("/p/templates/a.html.twig", twig);
        let found = keys(&edges.references);
        for key in ["twig-filter#price", "twig-filter#raw", "twig-function#asset_v", "twig-test#even"] {
            assert!(found.contains(&key), "{key} in {found:?}");
        }
        assert!(!found.contains(&"twig-filter#"));
    }

    #[test]
    fn ux_components_and_stimulus_controllers() {
        let php = "<?php\nnamespace App\\Twig;\n#[AsTwigComponent]\nclass Alert {}\n";
        assert!(defined(&index("/p/src/Twig/Alert.php", php), "twig-component#Alert"));
        assert!(defined(&index("/p/src/Twig/B.php", "<?php\n#[AsTwigComponent('Box')]\nclass B {}"), "twig-component#Box"));
        assert!(defined(&index("/p/templates/components/Card/Header.html.twig", "x"), "twig-component#Card:Header"));
        assert!(defined(&index("/p/assets/controllers/hello_world_controller.js", "x"), "stimulus#hello-world"));

        let twig = "<twig:Alert type=\"a\"/> {{ component('Box') }} <div data-controller=\"hello-world other\" {{ stimulus_controller('chart') }}>";
        let edges = index("/p/templates/a.html.twig", twig);
        assert_eq!(
            keys(&edges.references)[2..],
            ["twig-component#Alert", "twig-component#Box", "stimulus#chart", "stimulus#hello-world", "stimulus#other"]
        );
    }

    #[test]
    fn markers_reach_every_template_block_route_and_class_target() {
        let controller = "<?php\nnamespace App;\nclass BlogController {\n  #[Route('/b', name: 'blog')]\n  function i() { return $this->render('blog/i.html.twig'); }\n}\n";
        let edges = index("/p/src/BlogController.php", controller);
        assert!(keys(&edges.markers).contains(&"template#blog/i.html.twig"));
        assert!(keys(&edges.markers).contains(&"route-use#blog"));
        assert!(keys(&edges.markers).contains(&"class-service#App\\BlogController"));
        assert!(defined(&edges, "render#blog/i.html.twig") && defined(&edges, "class#App\\BlogController"));

        let child = index("/p/templates/blog/i.html.twig", "{% extends 'base.html.twig' %}\n{% block body %}{% endblock %}");
        assert!(defined(&child, "template#blog/i.html.twig") && defined(&child, "override#base.html.twig::body"));
        assert!(keys(&child.markers).contains(&"render#blog/i.html.twig"));
        assert!(keys(&child.markers).contains(&"block#base.html.twig::body"));
        assert!(keys(&child.markers).contains(&"override#blog/i.html.twig::body"));

        let services = index("/p/config/services.yaml", "services:\n    app.blog:\n        class: App\\BlogController\n");
        assert!(defined(&services, "class-service#App\\BlogController"));
        assert!(keys(&services.markers).contains(&"class#App\\BlogController"));

        let route = index("/p/src/R.php", "<?php\nclass R {\n  #[Route('/x', name: 'blog')] function a() {}\n  function b() { return $this->generateUrl('blog'); }\n}\n");
        assert!(defined(&route, "route-use#blog"));
    }

    fn marked(edges: &Edges, key: &str) -> bool {
        keys(&edges.markers).contains(&key)
    }

    const SUBSCRIBER: &str = "<?php\nnamespace App\\EventSubscriber;\n\nuse App\\Event\\OrderPlaced;\nuse Symfony\\Component\\HttpKernel\\KernelEvents;\nuse Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener;\n\nclass S\n{\n    public static function getSubscribedEvents(): array\n    {\n        return [\n            KernelEvents::REQUEST => 'onRequest',\n            OrderPlaced::class => ['onOrder', 10],\n            'custom.event' => 'onCustom',\n        ];\n    }\n\n    #[AsEventListener(event: 'kernel.response')]\n    public function onResponse() {}\n\n    #[AsEventListener(OrderPlaced::class)]\n    public function onOther() {}\n\n    #[AsEventListener]\n    public function untyped($e) {}\n}\n";

    #[test]
    fn listeners_are_event_definitions_and_dispatches_are_references() {
        let edges = index("/p/src/EventSubscriber/S.php", SUBSCRIBER);
        let defined_events: Vec<_> = keys(&edges.definitions).into_iter().filter(|key| key.starts_with("event#")).collect();
        assert_eq!(defined_events, ["event#kernel.request", "event#App\\Event\\OrderPlaced", "event#custom.event", "event#kernel.response", "event#App\\Event\\OrderPlaced"]);
        assert!(marked(&edges, "event-dispatch#kernel.request"));

        let dispatcher = "<?php\nnamespace App\\Service;\n\nuse App\\Event\\OrderPlaced;\n\nclass Checkout\n{\n    public function go($dispatcher, $e, $dynamic)\n    {\n        $dispatcher->dispatch(new OrderPlaced($x));\n        $dispatcher->dispatch($e, 'custom.event');\n        $dispatcher->dispatch($e, $dynamic);\n        $dispatcher->dispatch($e);\n    }\n}\n";
        let edges = index("/p/src/Service/Checkout.php", dispatcher);
        let references: Vec<_> = keys(&edges.references).into_iter().filter(|key| key.starts_with("event#")).collect();
        assert_eq!(references, ["event#App\\Event\\OrderPlaced", "event#custom.event"]);
        assert!(marked(&edges, "event#custom.event"));
        assert!(defined(&edges, "event-dispatch#App\\Event\\OrderPlaced"));
    }

    #[test]
    fn event_listener_tags_in_yaml_and_xml_are_definitions() {
        let yaml = "services:\n    a:\n        tags:\n            - { name: kernel.event_listener, event: kernel.request, method: on }\n            - { name: kernel.event_listener, event: 'App\\\\Event\\\\Foo' }\n            - { name: other, event: nope }\n";
        let edges = index("/p/config/services.yaml", yaml);
        let found: Vec<_> = keys(&edges.definitions).into_iter().filter(|key| key.starts_with("event#")).collect();
        assert_eq!(found, ["event#kernel.request", "event#App\\Event\\Foo"]);

        let xml = "<container><services><service id=\"a\"><tag name=\"kernel.event_listener\" event=\"kernel.exception\"/><tag name=\"x\" event=\"no\"/></service></services></container>";
        assert!(defined(&index("/p/config/services.xml", xml), "event#kernel.exception"));
    }

    #[test]
    fn twig_macros_link_definitions_to_imports() {
        let macros = index("/p/templates/forms/_macros.html.twig", "{% macro input(name) %}<input>{% endmacro %}\n{% macro label(t) -%}x{%- endmacro %}");
        assert!(defined(&macros, "macro#forms/_macros.html.twig::input") && defined(&macros, "macro#forms/_macros.html.twig::label"));

        let user = "{% import 'forms/_macros.html.twig' as forms %}{% from 'forms/_macros.html.twig' import input, label as l %}{{ forms.input('x') }}{{ other.input('y') }}{{ xforms.label('z') }}";
        let edges = index("/p/templates/a.html.twig", user);
        let found: Vec<_> = keys(&edges.references).into_iter().filter(|key| key.starts_with("macro#")).collect();
        assert_eq!(found, ["macro#forms/_macros.html.twig::input", "macro#forms/_macros.html.twig::label", "macro#forms/_macros.html.twig::input"]);

        let own = index("/p/templates/b.html.twig", "{% macro a() %}{% endmacro %}{% import _self as me %}{{ me.a() }}");
        assert!(keys(&own.references).contains(&"macro#b.html.twig::a"));
    }

    #[test]
    fn voter_attributes_link_supports_to_is_granted_calls() {
        let voter = "<?php\nnamespace App\\Security;\n\nuse Symfony\\Component\\Security\\Core\\Authorization\\Voter\\Voter;\n\nclass PostVoter extends Voter\n{\n    public const VIEW = 'POST_VIEW';\n    const EDIT = 'POST_EDIT';\n\n    protected function supports(string $attribute, mixed $subject): bool\n    {\n        return in_array($attribute, [self::VIEW, static::EDIT, 'POST_DELETE', 'lower_case']);\n    }\n}\n";
        let edges = index("/p/src/Security/PostVoter.php", voter);
        let found: Vec<_> = keys(&edges.definitions).into_iter().filter(|key| key.starts_with("voter#")).collect();
        assert_eq!(found, ["voter#POST_DELETE", "voter#POST_VIEW", "voter#POST_EDIT"]);

        let controller = "<?php\nclass C {\n  function a() { $this->denyAccessUnlessGranted('POST_EDIT', $p); $this->isGranted(\"POST_VIEW\"); }\n}\n";
        let granted = index("/p/src/C.php", controller);
        let found: Vec<_> = keys(&granted.references).into_iter().filter(|key| key.starts_with("voter#")).collect();
        assert_eq!(found, ["voter#POST_VIEW", "voter#POST_EDIT"]);

        let twig = index("/p/templates/a.html.twig", "{% if is_granted('POST_EDIT', post) %}{% endif %}");
        assert!(keys(&twig.references).contains(&"voter#POST_EDIT"));

        let not_a_voter = "<?php\nclass X { function supports($a) { return $a === 'POST_VIEW'; } }\n";
        assert!(keys(&index("/p/src/X.php", not_a_voter).definitions).iter().all(|key| !key.starts_with("voter#")));
    }

    #[test]
    fn constraints_and_their_validators_point_at_each_other() {
        let constraint = "<?php\nnamespace App\\Validator;\n\nuse Symfony\\Component\\Validator\\Constraint;\n\nclass NoSpaces extends Constraint\n{\n}\n";
        let validator = "<?php\nnamespace App\\Validator;\n\nuse Symfony\\Component\\Validator\\ConstraintValidator;\n\nclass NoSpacesValidator extends ConstraintValidator\n{\n}\n";
        let one = index("/p/src/Validator/NoSpaces.php", constraint);
        let two = index("/p/src/Validator/NoSpacesValidator.php", validator);
        assert!(marked(&one, "validator-of#App\\Validator\\NoSpaces") && defined(&one, "constraint-of#App\\Validator\\NoSpaces"));
        assert!(marked(&two, "constraint-of#App\\Validator\\NoSpaces") && defined(&two, "validator-of#App\\Validator\\NoSpaces"));
    }

    #[test]
    fn new_keys_survive_degenerate_input_and_the_anchor_limit() {
        for text in ["<?php\nfunction getSubscribedEvents", "<?php\nclass A { function getSubscribedEvents() { return [", "<?php\n->dispatch(", "<?php\n#[AsEventListener(", "{% from", "{% macro %}{% import 'x' as %}", "{% import 'x' as m %}m."] {
            index("/p/src/A.php", text);
            index("/p/templates/a.html.twig", text);
        }

        let many = "<?php\nclass A {\n".to_owned() + &"function f() { $d->dispatch($e, 'ev.x'); }\n".repeat(5000) + "}\n";
        let edges = index("/p/src/A.php", &many);
        assert!(edges.references.len() <= MAX_ANCHORS && edges.markers.len() <= MAX_ANCHORS);
    }
}
