//! Macros and templates: what `{% import %}` and `{% from %}` bring in, the macros a template defines, the
//! template a string names, and the filters, functions and tests a project registers in PHP.

use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub struct Macro {
    pub name: String,
    pub arguments: String,
    pub name_range: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Import {
    /// `{% import 'x' as forms %}`.
    Alias { template: String, alias: String },
    /// `{% from 'x' import a, b as c %}`: (name in the template, name used here).
    Names { template: String, names: Vec<(String, String)> },
}

fn tag_blocks(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    text.match_indices("{%").filter_map(move |(start, _)| {
        let end = start + text[start..].find("%}")?;
        let body = text[start + 2..end].trim_start_matches(['-', '~']).trim_end_matches(['-', '~']).trim();

        Some((start..end + 2, body))
    })
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn quoted(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim_start();
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = &rest[1..];
    let end = inner.find(quote)?;

    Some((&inner[..end], &inner[end + 1..]))
}

pub fn macros(text: &str) -> Vec<Macro> {
    tag_blocks(text)
        .filter_map(|(range, body)| {
            let rest = body.strip_prefix("macro")?;
            if !rest.starts_with(char::is_whitespace) {
                return None;
            }

            let rest = rest.trim_start();
            let length = rest.bytes().take_while(|b| is_word(*b)).count();
            if length == 0 {
                return None;
            }

            let after = rest[length..].trim_start();
            let arguments = after.strip_prefix('(').and_then(|a| a.rfind(')').map(|end| a[..end].trim().to_owned())).unwrap_or_default();
            let after_keyword = range.start + text[range.clone()].find("macro")? + "macro".len();
            let start = after_keyword + text[after_keyword..range.end].find(&rest[..length])?;

            Some(Macro { name: rest[..length].to_owned(), arguments, name_range: start..start + length })
        })
        .collect()
}

pub fn imports(text: &str) -> Vec<Import> {
    tag_blocks(text)
        .filter_map(|(_, body)| {
            if let Some(rest) = body.strip_prefix("import").filter(|r| r.starts_with(char::is_whitespace)) {
                let (template, rest) = template_and_rest(rest)?;
                let alias = rest.trim().strip_prefix("as")?.trim();
                let alias: String = alias.chars().take_while(|c| is_word(*c as u8) && c.is_ascii()).collect();

                return (!alias.is_empty()).then_some(Import::Alias { template, alias });
            }

            let rest = body.strip_prefix("from").filter(|r| r.starts_with(char::is_whitespace))?;
            let (template, rest) = template_and_rest(rest)?;
            let list = rest.trim().strip_prefix("import")?;
            let names = list
                .split(',')
                .filter_map(|item| {
                    let mut words = item.split_whitespace();
                    let name = words.next()?.to_owned();
                    let alias = match (words.next(), words.next()) {
                        (Some("as"), Some(alias)) => alias.to_owned(),
                        _ => name.clone(),
                    };

                    Some((name, alias))
                })
                .collect();

            Some(Import::Names { template, names })
        })
        .collect()
}

fn template_and_rest(rest: &str) -> Option<(String, &str)> {
    if let Some((template, after)) = quoted(rest) {
        return Some((template.to_owned(), after));
    }

    let rest = rest.trim_start();
    let after = rest.strip_prefix("_self")?;
    Some(("_self".to_owned(), after))
}

/// The template a string literal at `offset` names (`extends`, `include`, `embed`, `import`, `from`, `use`, `include()`, `source()`),
/// and the literal's range without the quotes.
pub fn template_at(text: &str, offset: usize) -> Option<(String, Range<usize>)> {
    let bytes = text.as_bytes();
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let mut open = None;
    let mut i = line_start;
    while i < offset.min(bytes.len()) {
        match (open, bytes[i]) {
            (None, q @ (b'\'' | b'"')) => open = Some((q, i)),
            (Some((q, _)), b) if b == q => open = None,
            _ => {}
        }
        i += 1;
    }

    let (quote, start) = open?;
    let end = offset + text[offset..].find(quote as char)?;
    let before = text[..start].trim_end();
    let introduces = ["extends", "include", "embed", "import", "from", "use", "include(", "source(", "include (", "source ("]
        .iter()
        .any(|word| before.ends_with(word) && before[..before.len() - word.len()].bytes().next_back().is_none_or(|b| !is_word(b)));

    (introduces && end > start + 1).then(|| (text[start + 1..end].to_owned(), start + 1..end))
}

/// The word around `offset` and the identifier before a `.` in front of it.
pub fn dotted_at(text: &str, offset: usize) -> Option<(Option<String>, String, Range<usize>)> {
    let bytes = text.as_bytes();
    let offset = offset.min(bytes.len());
    let start = (0..offset).rev().take_while(|&i| is_word(bytes[i])).last().unwrap_or(offset);
    let end = (offset..bytes.len()).take_while(|&i| is_word(bytes[i])).last().map_or(offset, |i| i + 1);
    if start == end && !text[..start].ends_with('.') {
        return None;
    }

    let receiver = text[..start].strip_suffix('.').map(|before| {
        let length = before.bytes().rev().take_while(|b| is_word(*b)).count();
        before[before.len() - length..].to_owned()
    });

    Some((receiver.filter(|r| !r.is_empty()), text[start..end].to_owned(), start..end))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionKind {
    Filter,
    Function,
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    pub kind: ExtensionKind,
    pub name: String,
    /// Byte range of the name literal.
    pub range: Range<usize>,
}

const REGISTRATIONS: &[(&str, ExtensionKind)] = &[
    ("TwigFilter(", ExtensionKind::Filter),
    ("SimpleFilter(", ExtensionKind::Filter),
    ("TwigFunction(", ExtensionKind::Function),
    ("SimpleFunction(", ExtensionKind::Function),
    ("TwigTest(", ExtensionKind::Test),
    ("SimpleTest(", ExtensionKind::Test),
];

/// `new TwigFilter('name', …)`, `new TwigFunction('name', …)`, `new TwigTest('name', …)` and the `#[AsTwig…('name')]` attributes of a PHP file.
pub fn extensions(php: &str) -> Vec<Extension> {
    if !php.contains("Twig") {
        return Vec::new();
    }

    let mut found = Vec::new();
    for (marker, kind) in REGISTRATIONS {
        for (at, _) in php.match_indices(marker) {
            let rest = &php[at + marker.len()..];
            let Some((name, _)) = quoted(rest) else { continue };
            let literal = rest.trim_start();
            let start = php.len() - literal.len() + 1;
            let valid = !name.is_empty() && name.bytes().all(|b| is_word(b) || b == b'*');
            if valid {
                found.push(Extension { kind: *kind, name: name.to_owned(), range: start..start + name.len() });
            }
        }
    }

    found.sort_by_key(|e| e.range.start);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_macros_with_their_arguments() {
        let text = "{% macro input(name, value = '') %}x{% endmacro %}\n{%- macro bare -%}y{%- endmacro -%}";
        let found = macros(text);
        assert_eq!(found.iter().map(|m| (m.name.as_str(), m.arguments.as_str())).collect::<Vec<_>>(), [("input", "name, value = ''"), ("bare", "")]);
        assert_eq!(&text[found[0].name_range.clone()], "input");
    }

    #[test]
    fn reads_import_and_from() {
        let text = "{% import 'forms.twig' as forms %}{% import _self as me %}{% from \"m/x.twig\" import a, b as c %}";
        assert_eq!(
            imports(text),
            [
                Import::Alias { template: "forms.twig".into(), alias: "forms".into() },
                Import::Alias { template: "_self".into(), alias: "me".into() },
                Import::Names { template: "m/x.twig".into(), names: vec![("a".into(), "a".into()), ("b".into(), "c".into())] },
            ]
        );
    }

    #[test]
    fn finds_the_template_name_under_the_caret() {
        let text = "{% extends 'base.html.twig' %}{{ include('part/a.twig') }}{{ 'plain' }}";
        let at = text.find("base").unwrap() + 2;
        assert_eq!(template_at(text, at).unwrap().0, "base.html.twig");
        let at = text.find("part").unwrap() + 1;
        assert_eq!(template_at(text, at).unwrap().0, "part/a.twig");
        assert!(template_at(text, text.find("plain").unwrap() + 1).is_none());
    }

    #[test]
    fn finds_the_receiver_of_a_dotted_name() {
        let text = "{{ forms.input(1) }}";
        let (receiver, word, range) = dotted_at(text, text.find("input").unwrap() + 2).unwrap();
        assert_eq!((receiver.as_deref(), word.as_str(), &text[range]), (Some("forms"), "input", "input"));
        assert_eq!(dotted_at("{{ x }}", 3).unwrap().0, None);
        assert_eq!(dotted_at("{{ forms. }}", 9).unwrap().0.as_deref(), Some("forms"));
        assert!(dotted_at("{{  }}", 3).is_none());
    }

    #[test]
    fn reads_registered_filters_functions_and_tests() {
        let php = "<?php\nuse Twig\\TwigFilter;\nclass E {\n  function f() { return [new TwigFilter('money', [$this, 'm']), new \\Twig\\TwigFunction(\"asset_url\", fn() => 1), new TwigTest('big', f())]; }\n  #[AsTwigFilter('shout')] function s() {}\n}\n";
        let found: Vec<_> = extensions(php).into_iter().map(|e| (e.kind, e.name)).collect();
        assert_eq!(
            found,
            [
                (ExtensionKind::Filter, "money".to_owned()),
                (ExtensionKind::Function, "asset_url".to_owned()),
                (ExtensionKind::Test, "big".to_owned()),
                (ExtensionKind::Filter, "shout".to_owned()),
            ]
        );
        assert!(extensions("<?php new Other('x');").is_empty());
    }
}
