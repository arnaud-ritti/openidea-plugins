//! Completion of attribute arguments (`#[Route(`, `#[ORM\Column(`): argument
//! names and the values of arguments with a fixed set. The tables come from the
//! Symfony and Doctrine ORM attribute references. Only attributes that resolve,
//! through `use`, to one of the listed classes are completed.

use std::ops::Range;

use crate::php_checks::{arguments, named};
use crate::php_text::{mask, resolver};

const ROUTE: &[&str] = &["Symfony\\Component\\Routing\\Attribute\\Route", "Symfony\\Component\\Routing\\Annotation\\Route"];
const TEMPLATE: &[&str] = &["Symfony\\Bridge\\Twig\\Attribute\\Template", "Sensio\\Bundle\\FrameworkExtraBundle\\Configuration\\Template"];

const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
const SECURITY_ATTRIBUTES: &[&str] = &["ROLE_USER", "ROLE_ADMIN", "IS_AUTHENTICATED", "IS_AUTHENTICATED_FULLY", "IS_AUTHENTICATED_REMEMBERED", "IS_REMEMBERED", "IS_IMPERSONATOR", "PUBLIC_ACCESS"];
const CORE_EVENTS: &[&str] = &[
    "kernel.request", "kernel.controller", "kernel.controller_arguments", "kernel.view", "kernel.response", "kernel.finish_request", "kernel.terminate", "kernel.exception",
    "console.command", "console.error", "console.terminate", "console.signal", "security.interactive_login", "security.switch_user",
];
const FETCH: &[&str] = &["LAZY", "EAGER", "EXTRA_LAZY"];
const CASCADE: &[&str] = &["persist", "remove", "merge", "detach", "refresh", "all"];
const STRATEGIES: &[&str] = &["AUTO", "SEQUENCE", "IDENTITY", "NONE", "UUID", "CUSTOM"];
const ON_DELETE: &[&str] = &["CASCADE", "SET NULL", "RESTRICT", "NO ACTION"];
const COLUMN_TYPES: &[&str] = &[
    "string", "text", "integer", "smallint", "bigint", "boolean", "float", "decimal", "date", "date_immutable", "datetime", "datetime_immutable", "datetimetz",
    "datetimetz_immutable", "time", "time_immutable", "json", "simple_array", "guid", "blob", "binary", "ascii_string",
];

const ASSOCIATION_ARGUMENTS: &[&str] = &["targetEntity", "mappedBy", "inversedBy", "cascade", "fetch", "orphanRemoval", "indexBy"];

struct Spec {
    classes: &'static [&'static str],
    arguments: &'static [&'static str],
    values: &'static [(&'static str, &'static [&'static str])],
}

fn orm(name: &'static str) -> &'static [&'static str] {
    match name {
        "Column" => &["Doctrine\\ORM\\Mapping\\Column"],
        "Entity" => &["Doctrine\\ORM\\Mapping\\Entity"],
        "Table" => &["Doctrine\\ORM\\Mapping\\Table"],
        "JoinColumn" => &["Doctrine\\ORM\\Mapping\\JoinColumn"],
        "ManyToOne" => &["Doctrine\\ORM\\Mapping\\ManyToOne"],
        "OneToMany" => &["Doctrine\\ORM\\Mapping\\OneToMany"],
        "OneToOne" => &["Doctrine\\ORM\\Mapping\\OneToOne"],
        "ManyToMany" => &["Doctrine\\ORM\\Mapping\\ManyToMany"],
        "GeneratedValue" => &["Doctrine\\ORM\\Mapping\\GeneratedValue"],
        "Index" => &["Doctrine\\ORM\\Mapping\\Index"],
        _ => &["Doctrine\\ORM\\Mapping\\UniqueConstraint"],
    }
}

fn specs() -> Vec<Spec> {
    vec![
        Spec {
            classes: ROUTE,
            arguments: &["path", "name", "requirements", "options", "defaults", "host", "methods", "schemes", "condition", "locale", "format", "utf8", "stateless", "priority", "env"],
            values: &[("methods", HTTP_METHODS), ("schemes", &["http", "https"])],
        },
        Spec { classes: TEMPLATE, arguments: &["template", "vars", "isStreamable", "streamable"], values: &[] },
        Spec {
            classes: orm("Column"),
            arguments: &["name", "type", "length", "precision", "scale", "unique", "nullable", "insertable", "updatable", "enumType", "options", "columnDefinition", "generated"],
            values: &[("type", COLUMN_TYPES)],
        },
        Spec { classes: orm("Entity"), arguments: &["repositoryClass", "readOnly"], values: &[] },
        Spec { classes: orm("Table"), arguments: &["name", "schema", "indexes", "uniqueConstraints", "options"], values: &[] },
        Spec {
            classes: orm("JoinColumn"),
            arguments: &["name", "referencedColumnName", "unique", "nullable", "onDelete", "columnDefinition", "fieldName", "options"],
            values: &[("onDelete", ON_DELETE)],
        },
        Spec { classes: orm("ManyToOne"), arguments: &["targetEntity", "cascade", "fetch", "inversedBy"], values: &[("fetch", FETCH), ("cascade", CASCADE)] },
        Spec { classes: orm("OneToMany"), arguments: &["targetEntity", "mappedBy", "cascade", "fetch", "orphanRemoval", "indexBy"], values: &[("fetch", FETCH), ("cascade", CASCADE)] },
        Spec { classes: orm("OneToOne"), arguments: &["targetEntity", "mappedBy", "inversedBy", "cascade", "fetch", "orphanRemoval"], values: &[("fetch", FETCH), ("cascade", CASCADE)] },
        Spec { classes: orm("ManyToMany"), arguments: ASSOCIATION_ARGUMENTS, values: &[("fetch", FETCH), ("cascade", CASCADE)] },
        Spec { classes: orm("GeneratedValue"), arguments: &["strategy"], values: &[("strategy", STRATEGIES)] },
        Spec { classes: orm("Index"), arguments: &["name", "columns", "fields", "options", "flags"], values: &[] },
        Spec { classes: orm("UniqueConstraint"), arguments: &["name", "columns", "fields", "options"], values: &[] },
        Spec { classes: &["Symfony\\Component\\Security\\Http\\Attribute\\IsGranted"], arguments: &["attribute", "subject", "message", "statusCode", "exceptionCode", "methods"], values: &[("attribute", SECURITY_ATTRIBUTES), ("methods", HTTP_METHODS)] },
        Spec { classes: &["Symfony\\Component\\Console\\Attribute\\AsCommand"], arguments: &["name", "description", "aliases", "hidden"], values: &[] },
        Spec { classes: &["Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener"], arguments: &["event", "method", "priority", "dispatcher"], values: &[("event", CORE_EVENTS)] },
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub insert: String,
    pub detail: &'static str,
}

/// Where the caret is inside an attribute's argument list.
struct Position {
    attribute: String,
    paren: usize,
    argument_start: usize,
    in_string: Option<usize>,
}

fn locate(text: &str, offset: usize) -> Option<Position> {
    let group = text[..offset].rfind("#[")?;
    let mut stack: Vec<(usize, u8)> = Vec::new();
    let mut quote: Option<(u8, usize)> = None;
    let mut argument_start = 0;
    let bytes = text.as_bytes();
    let mut at = group + 1;

    while at < offset {
        let byte = bytes[at];
        match quote {
            Some(_) if byte == b'\\' => at += 1,
            Some((open, _)) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'\'' | b'"' => quote = Some((byte, at)),
                b'(' | b'[' | b'{' => {
                    stack.push((at, byte));
                    if stack.len() == 2 && byte == b'(' {
                        argument_start = at + 1;
                    }
                }
                b')' | b']' | b'}' => {
                    stack.pop();
                    if stack.is_empty() {
                        return None;
                    }
                }
                b',' if stack.len() == 2 => argument_start = at + 1,
                b';' => return None,
                _ => {}
            },
        }
        at += 1;
    }

    let &(paren, kind) = stack.get(1)?;
    if kind != b'(' {
        return None;
    }

    let name_end = text[..paren].trim_end().len();
    let name_start = text[..name_end].rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '\\'))).map_or(0, |found| found + 1);
    let attribute = text[name_start..name_end].to_owned();
    let before = text[group + 2..name_start].trim_end();
    if attribute.is_empty() || !(before.is_empty() || before.ends_with(',')) {
        return None;
    }

    Some(Position { attribute, paren, argument_start, in_string: quote.map(|(_, start)| start) })
}

/// Names and values to offer at `offset` and the byte range they replace.
pub fn completions(text: &str, offset: usize) -> Option<(Range<usize>, Vec<Completion>)> {
    let position = locate(text, offset)?;
    let fqcn = resolver(text)(&position.attribute);
    let spec = specs().into_iter().find(|spec| spec.classes.contains(&fqcn.as_str()))?;

    let masked = mask(text);
    let argument = &text[position.argument_start..offset];
    let name = {
        let trimmed = argument.trim_start();
        let word_len = trimmed.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
        let after = trimmed[word_len..].trim_start();
        (word_len > 0 && after.starts_with(':') && !after.starts_with("::")).then(|| trimmed[..word_len].to_owned())
    };

    let values = name.as_deref().and_then(|name| spec.values.iter().find(|(argument, _)| *argument == name)).map(|(_, values)| *values);

    if let Some(start) = position.in_string {
        let values = values?;
        let end = text[offset..].find(['\'', '"', '\n']).map_or(offset, |found| offset + found);
        let items = values.iter().map(|value| Completion { label: (*value).to_owned(), insert: (*value).to_owned(), detail: "value" }).collect();
        return Some((start + 1..end, items));
    }

    let word_start = offset - text[..offset].bytes().rev().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    let before_word = text[position.argument_start..word_start].trim();

    if name.is_some() {
        let items = values?.iter().map(|value| Completion { label: (*value).to_owned(), insert: format!("'{value}'"), detail: "value" }).collect();
        return Some((word_start..offset, items));
    }

    if !before_word.is_empty() {
        return None;
    }

    let used: Vec<String> = arguments(&masked, position.paren)
        .into_iter()
        .filter(|range| !range.contains(&offset))
        .filter_map(|range| named(&masked, range).map(|(name, _)| masked[name].to_owned()))
        .collect();

    let items = spec
        .arguments
        .iter()
        .filter(|argument| !used.iter().any(|used| used == *argument))
        .map(|argument| Completion { label: (*argument).to_owned(), insert: format!("{argument}: "), detail: "argument" })
        .collect();
    Some((word_start..offset, items))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(marked: &str) -> Option<(Range<usize>, Vec<String>, String)> {
        let offset = marked.find('|').unwrap();
        let text = marked.replacen('|', "", 1);
        completions(&text, offset).map(|(range, items)| (range.clone(), items.into_iter().map(|item| item.insert).collect(), text[range].to_owned()))
    }

    fn inserts(marked: &str) -> Vec<String> {
        at(marked).map(|(_, items, _)| items).unwrap_or_default()
    }

    const ROUTE_IMPORT: &str = "<?php\nuse Symfony\\Component\\Routing\\Attribute\\Route;\nuse Doctrine\\ORM\\Mapping as ORM;\n\n";

    #[test]
    fn route_argument_names_exclude_the_ones_already_written() {
        let items = inserts(&format!("{ROUTE_IMPORT}#[Route('/x', na|)]\nclass A {{}}"));
        assert!(items.contains(&"name: ".to_owned()) && items.contains(&"methods: ".to_owned()), "{items:?}");

        let items = inserts(&format!("{ROUTE_IMPORT}#[Route('/x', name: 'a', |)]\nclass A {{}}"));
        assert!(!items.contains(&"name: ".to_owned()) && items.contains(&"methods: ".to_owned()), "{items:?}");

        let (range, _, replaced) = at(&format!("{ROUTE_IMPORT}#[Route('/x', meth|)]\nclass A {{}}")).unwrap();
        assert_eq!(replaced, "meth");
        assert_eq!(range.len(), 4);
    }

    #[test]
    fn enumerated_values_are_offered_in_strings_arrays_and_after_the_colon() {
        let methods = inserts(&format!("{ROUTE_IMPORT}#[Route('/x', methods: ['G|'])]\nclass A {{}}"));
        assert!(methods.contains(&"GET".to_owned()) && methods.contains(&"POST".to_owned()), "{methods:?}");

        let bare = inserts(&format!("{ROUTE_IMPORT}#[Route('/x', methods: [|])]\nclass A {{}}"));
        assert!(bare.contains(&"'GET'".to_owned()), "{bare:?}");

        let (range, _, replaced) = at(&format!("{ROUTE_IMPORT}#[ORM\\Column(type: 'str|')]\nprivate $a;")).unwrap();
        assert_eq!(replaced, "str");
        assert!(range.start < range.end);

        let fetch = inserts(&format!("{ROUTE_IMPORT}#[ORM\\ManyToOne(targetEntity: A::class, fetch: |)]\nprivate $a;"));
        assert_eq!(fetch, ["'LAZY'", "'EAGER'", "'EXTRA_LAZY'"]);
    }

    #[test]
    fn doctrine_attributes_complete_through_their_alias() {
        let items = inserts(&format!("{ROUTE_IMPORT}#[ORM\\Column(|)]\nprivate $a;"));
        assert!(items.contains(&"length: ".to_owned()) && items.contains(&"nullable: ".to_owned()), "{items:?}");

        let grouped = inserts(&format!("{ROUTE_IMPORT}#[ORM\\Id, ORM\\GeneratedValue(str|)]\nprivate $a;"));
        assert_eq!(grouped, ["strategy: "]);
        let strategy = inserts(&format!("{ROUTE_IMPORT}#[ORM\\GeneratedValue(strategy: '|')]\nprivate $a;"));
        assert!(strategy.contains(&"IDENTITY".to_owned()));
    }

    #[test]
    fn nothing_is_offered_outside_known_attribute_argument_lists() {
        for marked in [
            format!("{ROUTE_IMPORT}#[Route('/x'|)]\nclass A {{}}"),
            format!("{ROUTE_IMPORT}#[Route('/x', name: 'a|')]\nclass A {{}}"),
            format!("{ROUTE_IMPORT}#[Unknown(|)]\nclass A {{}}"),
            format!("{ROUTE_IMPORT}#[Route('/x')]\nclass A {{ function f(|) {{}} }}"),
            format!("{ROUTE_IMPORT}#[Route('/x', requirements: ['id' => '\\d+'], |)]\nclass A {{}}").replace("|)]", "|)]").replace("requirements: ['id' => '\\d+'], ", "requirements: ['i|"),
            "<?php\n#[Route(|)]\nclass A {}".to_owned(),
            format!("{ROUTE_IMPORT}Route(|)"),
        ] {
            assert!(inserts(&marked).is_empty(), "{marked}");
        }
    }

    #[test]
    fn degenerate_text_does_not_panic() {
        for text in ["#[", "#[Route(", "#[Route('", "#[A, B(", "#[Route(name: [", "é#[Route(é"] {
            for offset in 0..=text.len() {
                if text.is_char_boundary(offset) {
                    let _ = completions(text, offset);
                }
            }
        }
    }

    #[test]
    fn security_attributes_and_core_events_are_offered() {
        let granted = inserts("<?php\nuse Symfony\\Component\\Security\\Http\\Attribute\\IsGranted;\n\n#[IsGranted(attribute: 'ROLE_|')]\nclass A {}");
        assert!(granted.contains(&"ROLE_ADMIN".to_owned()), "{granted:?}");

        let events = inserts("<?php\nuse Symfony\\Component\\EventDispatcher\\Attribute\\AsEventListener;\n\n#[AsEventListener(event: |)]\nclass A {}");
        assert!(events.contains(&"'kernel.request'".to_owned()), "{events:?}");
    }
}
