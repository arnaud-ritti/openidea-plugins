//! What a Blade component or Livewire component accepts: attributes from
//! `@props([...])`, constructor parameters or public properties, and slot names
//! from the variables its view prints.

use crate::index::separated;
use crate::php;
use crate::refs::Kind;

#[derive(Debug, PartialEq, Eq)]
pub struct Prop {
    pub name: String,
    pub default: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Context {
    /// An attribute name being typed in `<x-name ` or `<livewire:name `.
    Attribute { kind: Kind, component: String, start: usize },
    /// The slot name of `<x-slot:name`, inside the component `component`.
    Slot { component: String, start: usize },
}

fn word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')
}

/// Whether `tag` (from its `<` to the caret) is still open: no `>` outside quotes and echoes.
fn still_open(tag: &str) -> Option<bool> {
    let bytes = tag.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;

    while i < bytes.len() {
        match (quote, bytes[i]) {
            (Some(q), b) if b == q => quote = None,
            (None, b'"' | b'\'') => quote = Some(bytes[i]),
            (None, b'>') => return None,
            (None, b'{') if bytes[i..].starts_with(b"{{") || bytes[i..].starts_with(b"{!!") => {
                let close = if bytes[i + 1] == b'{' { "}}" } else { "!!}" };
                match tag[i..].find(close) {
                    Some(n) => i += n + close.len() - 1,
                    None => return Some(false),
                }
            }
            _ => {}
        }
        i += 1;
    }

    Some(quote.is_none())
}

fn enclosing_component(text: &str, before: usize) -> Option<String> {
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;

    while let Some(found) = text[i..before].find("<") {
        let at = i + found;
        i = at + 1;
        let rest = &text[at + 1..before];
        let closing = rest.starts_with('/');
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        let Some(rest) = rest.strip_prefix("x-") else { continue };
        let length = rest.bytes().take_while(|b| word(*b) || *b == b':').count();
        let name = &rest[..length];
        if name.is_empty() || name.contains(':') || name == "slot" {
            continue;
        }

        if closing {
            if let Some(position) = stack.iter().rposition(|open| open == name) {
                stack.truncate(position);
            }
            continue;
        }

        let Some(end) = text[at..before].find('>') else {
            stack.push(name.to_owned());
            continue;
        };
        if !text[..at + end].ends_with('/') {
            stack.push(name.to_owned());
        }
    }

    stack.pop()
}

pub fn context_at(text: &str, at: usize) -> Option<Context> {
    let tag_start = text[..at].rfind('<')?;
    let tag = &text[tag_start..at];
    if !still_open(tag)? {
        return None;
    }

    if let Some(typed) = tag.strip_prefix("<x-slot:") {
        if typed.bytes().any(|b| !word(b)) {
            return None;
        }
        let component = enclosing_component(text, tag_start)?;
        return Some(Context::Slot { component, start: at - typed.len() });
    }

    let (kind, rest) = if let Some(rest) = tag.strip_prefix("<x-") {
        (Kind::Component, rest)
    } else {
        (Kind::Livewire, tag.strip_prefix("<livewire:")?)
    };

    let length = rest.bytes().take_while(|b| word(*b)).count();
    let component = &rest[..length];
    let after = &rest[length..];
    if component.is_empty() || !after.starts_with(char::is_whitespace) || rest[length..].starts_with(':') {
        return None;
    }

    let token = after.rsplit(char::is_whitespace).next().unwrap_or_default();
    let mut start = at - token.len();
    if token.starts_with(':') {
        start += 1;
    }

    Some(Context::Attribute { kind, component: component.to_owned(), start })
}

/// Attributes named by `@props([...])` of an anonymous component view.
pub fn view_props(view: &str) -> Vec<Prop> {
    let Some(at) = view.find("@props(") else { return Vec::new() };
    let open = at + "@props".len();
    let Some(array) = php::arguments(view, open).into_iter().next().and_then(|range| php::array_open(view, range)) else { return Vec::new() };

    php::items(view, array)
        .into_iter()
        .filter_map(|item| {
            let key = item.key.and_then(|key| php::literal(view, key)).map(|(key, _)| key.to_owned());
            match key {
                Some(name) => Some(Prop { name, default: Some(view[php::trimmed(view, item.value)].to_owned()) }),
                None => php::literal(view, item.value).map(|(name, _)| Prop { name: name.to_owned(), default: None }),
            }
        })
        .collect()
}

/// Constructor parameters of a class component.
pub fn constructor_props(class: &str) -> Vec<Prop> {
    let masked = php::mask_comments(class);
    let Some(at) = masked.find("function __construct(") else { return Vec::new() };
    let open = at + "function __construct".len();

    php::arguments(&masked, open)
        .into_iter()
        .filter_map(|range| {
            let parameter = masked[range].trim();
            let (declaration, default) = match parameter.split_once('=') {
                Some((declaration, default)) => (declaration.trim(), Some(default.trim().to_owned())),
                None => (parameter, None),
            };
            let name = declaration.rsplit(char::is_whitespace).next()?.strip_prefix('$')?;
            Some(Prop { name: name.to_owned(), default })
        })
        .collect()
}

/// Public properties and `mount()` parameters of a Livewire component.
pub fn livewire_props(class: &str) -> Vec<Prop> {
    let masked = php::mask_comments(class);
    let mut props = Vec::new();

    for line in masked.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("public ") else { continue };
        if rest.contains("function ") || rest.starts_with("static ") {
            continue;
        }

        let Some(dollar) = rest.find('$') else { continue };
        let name: String = rest[dollar + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        let default = rest.split_once('=').map(|(_, value)| value.trim().trim_end_matches(';').to_owned());
        if !name.is_empty() {
            props.push(Prop { name, default });
        }
    }

    if let Some(at) = masked.find("function mount(") {
        for range in php::arguments(&masked, at + "function mount".len()) {
            let parameter = masked[range].split('=').next().unwrap_or_default().trim().to_owned();
            if let Some(name) = parameter.rsplit(char::is_whitespace).next().and_then(|p| p.strip_prefix('$'))
                && !props.iter().any(|prop| prop.name == name)
            {
                props.push(Prop { name: name.to_owned(), default: None });
            }
        }
    }

    props
}

/// Variables the view prints that are neither props nor framework variables.
pub fn slot_names(view: &str, props: &[Prop]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();

    for region in crate::refs::blade_regions(view) {
        let text = &view[region];
        if !text.starts_with("{{") && !text.starts_with("{!!") {
            continue;
        }

        for (at, _) in text.match_indices('$') {
            let name: String = text[at + 1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let after = text[at + 1 + name.len()..].trim_start();
            let is_variable = after.is_empty() || after.starts_with("}}") || after.starts_with("!!}") || after.starts_with("??");
            let is_prop = props.iter().any(|prop| prop.name == name);

            if !name.is_empty() && is_variable && !is_prop && !matches!(name.as_str(), "slot" | "attributes" | "component" | "loop" | "errors") && !names.contains(&name) {
                names.push(name);
            }
        }
    }

    names
}

pub fn attribute_name(name: &str) -> String {
    separated(name, '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(marked: &str) -> Option<Context> {
        let at = marked.find('|').unwrap();
        context_at(&marked.replacen('|', "", 1), at)
    }

    #[test]
    fn attribute_position_inside_a_component_tag() {
        assert_eq!(context("<x-alert ty|"), Some(Context::Attribute { kind: Kind::Component, component: "alert".into(), start: 9 }));
        assert_eq!(context("<x-alert :ty|"), Some(Context::Attribute { kind: Kind::Component, component: "alert".into(), start: 10 }));
        assert_eq!(context("<livewire:counter |"), Some(Context::Attribute { kind: Kind::Livewire, component: "counter".into(), start: 18 }));
    }

    #[test]
    fn no_attribute_completion_on_the_tag_name_inside_values_or_after_the_tag() {
        assert_eq!(context("<x-ale|"), None);
        assert_eq!(context("<x-alert type=\"in|"), None);
        assert_eq!(context("<x-alert type=\"a > b\" ti|").is_some(), true);
        assert_eq!(context("<x-alert a=\"1\"> ti|"), None);
        assert_eq!(context("<div cla|"), None);
        assert_eq!(context("<x-alert :a=\"{{ $x }}\" b|").is_some(), true);
    }

    #[test]
    fn slot_names_belong_to_the_enclosing_component() {
        assert_eq!(context("<x-card>\n  <x-slot:he|"), Some(Context::Slot { component: "card".into(), start: 19 }));
        assert_eq!(context("<x-card></x-card>\n<x-slot:he|"), None);
        assert_eq!(context("<x-card>\n<x-alert />\n<x-slot:he|").map(|c| matches!(c, Context::Slot { component, .. } if component == "card")), Some(true));
    }

    #[test]
    fn props_come_from_the_view_the_constructor_and_the_livewire_class() {
        let view = "@props(['type' => 'info', 'message', \"dismissible\" => false])\n<div>{{ $message }} {{ $footer }} {{ $slot }}</div>\n{!! $title ?? '' !!}";
        let props = view_props(view);
        assert_eq!(props.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["type", "message", "dismissible"]);
        assert_eq!(props[0].default.as_deref(), Some("'info'"));
        assert_eq!(slot_names(view, &props), ["footer", "title"]);

        let class = "<?php\nclass Alert extends Component\n{\n    public function __construct(public string $type = 'info', $userName)\n    {\n    }\n}\n";
        let props = constructor_props(class);
        assert_eq!(props.iter().map(|p| (p.name.as_str(), p.default.as_deref())).collect::<Vec<_>>(), [("type", Some("'info'")), ("userName", None)]);
        assert_eq!(attribute_name("userName"), "user-name");

        let livewire = "<?php\nclass Counter extends Component\n{\n    public int $count = 0;\n    public $label;\n    public function mount($start, $label = null) {}\n    public static $x;\n}\n";
        let names: Vec<String> = livewire_props(livewire).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["count", "label", "start"]);
    }
}
