//! Routes with their path, HTTP methods and controller, for the tool window's
//! flat list; `Index::routes` only keeps names.

use openidea_plugin::{Position, text};

use crate::index::{class_declaration, closing_paren, php_class, yaml_keys};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteDef {
    pub name: Option<String>,
    pub path: String,
    pub methods: Vec<String>,
    /// Short class name.
    pub class: String,
    pub member: Option<String>,
    pub file: String,
    pub at: Position,
}

fn short_class(class: &str) -> &str {
    class.rsplit('\\').next().unwrap_or(class)
}

fn quoted(text: &str, from: usize) -> Option<(&str, usize)> {
    let rest = &text[from..];
    let start = from + rest.len() - rest.trim_start().len();
    let quote = text[start..].chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let end = start + 1 + text[start + 1..].find(quote)?;
    Some((&text[start + 1..end], start + 1))
}

/// Offset of the value of `key: …` / `key=…` inside an attribute's arguments.
fn keyword_value(text: &str, args: std::ops::Range<usize>, key: &str) -> Option<usize> {
    let inside = &text[args.clone()];
    let mut from = 0;

    while let Some(found) = inside[from..].find(key) {
        let at = from + found;
        from = at + key.len();

        let previous = at.checked_sub(1).map(|i| inside.as_bytes()[i]);
        if previous.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'/') {
            continue;
        }

        let rest = inside[from..].trim_start();
        let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else { continue };
        return Some(args.start + inside.len() - rest.len());
    }

    None
}

fn string_list(text: &str, from: usize) -> Vec<String> {
    let rest = text[from..].trim_start();
    let start = text.len() - rest.len();
    let end = match rest.chars().next() {
        Some('[') => rest.find(']'),
        Some('{') => rest.find('}'),
        _ => None,
    };
    let Some(end) = end else {
        return quoted(text, start).map(|(value, _)| vec![value.to_ascii_uppercase()]).unwrap_or_default();
    };

    rest[1..end]
        .split(',')
        .map(|method| method.trim().trim_matches(['\'', '"']).to_ascii_uppercase())
        .filter(|method| !method.is_empty())
        .collect()
}

fn member_after(text: &str, from: usize) -> Option<String> {
    let at = from + text[from..].find("function ")? + "function ".len();
    let name: String = text[at..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

/// `#[Route]` attributes and `@Route` annotations; a route on the class
/// prefixes the paths of its methods' routes.
pub fn php_route_defs(file: &str, text: &str) -> Vec<RouteDef> {
    let Some((class, _)) = php_class(text) else { return Vec::new() };
    let class_at = class_declaration(text).unwrap_or(0);
    let mut prefix = String::new();
    let mut defs = Vec::new();

    let attributes = text.match_indices("Route(").filter(|(at, _)| {
        let before = &text[..*at];
        before.ends_with("#[") || before.ends_with('@') || before.ends_with('\\') || before.ends_with(", ")
    });

    for (at, matched) in attributes {
        let args_start = at + matched.len();
        let args_end = closing_paren(text, args_start);
        let args = args_start..args_end;

        let path_at = match text[args_start..].trim_start().starts_with(['\'', '"']) {
            true => Some(args_start),
            false => keyword_value(text, args.clone(), "path"),
        };
        let (path, path_offset) = path_at.and_then(|from| quoted(text, from)).map_or((String::new(), at), |(p, o)| (p.to_owned(), o));

        if at < class_at {
            prefix = path;
            continue;
        }

        let name = keyword_value(text, args.clone(), "name").and_then(|from| quoted(text, from)).map(|(name, _)| name.to_owned());
        let methods = keyword_value(text, args, "methods").map(|from| string_list(text, from)).unwrap_or_default();
        defs.push(RouteDef {
            name,
            path: format!("{prefix}{path}"),
            methods,
            class: short_class(&class).to_owned(),
            member: member_after(text, args_end),
            file: file.to_owned(),
            at: text::position(text, path_offset),
        });
    }

    defs
}

/// Top-level keys of a routing YAML file that have a `path`.
pub fn yaml_route_defs(file: &str, text: &str) -> Vec<RouteDef> {
    let keys = yaml_keys(text);
    let child = |parent: &str, name: &str| {
        keys.iter().find(|key| key.parents == [parent] && key.key == name).map(|key| key.value.trim_matches(['\'', '"']).to_owned())
    };

    keys.iter()
        .filter(|key| key.parents.is_empty() && !key.key.starts_with("when@"))
        .filter_map(|key| {
            let path = child(&key.key, "path")?;
            let controller = child(&key.key, "controller").unwrap_or_default();
            let (class, member) = match controller.split_once("::") {
                Some((class, member)) => (short_class(class), Some(member.to_owned())),
                None => (short_class(&controller), None),
            };
            let methods = child(&key.key, "methods")
                .map(|list| list.trim_matches(['[', ']']).split(',').map(|m| m.trim().trim_matches(['\'', '"']).to_ascii_uppercase()).filter(|m| !m.is_empty()).collect())
                .unwrap_or_default();
            Some(RouteDef {
                name: Some(key.key.clone()),
                path,
                methods,
                class: class.to_owned(),
                member,
                file: file.to_owned(),
                at: text::position(text, key.offset),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_attribute_routes_with_class_prefix_methods_and_members() {
        let source = "<?php\nnamespace App\\Controller;\n#[Route('/blog')]\nclass BlogController\n{\n    #[Route('/{id}', name: 'blog_show', methods: ['GET', 'head'])]\n    public function show() {}\n\n    #[Route(path: '/new', methods: 'POST')]\n    public function new() {}\n}\n";
        let defs = php_route_defs("/p/src/BlogController.php", source);
        let shown: Vec<_> = defs.iter().map(|d| (d.path.as_str(), d.name.as_deref(), d.methods.clone(), d.class.as_str(), d.member.as_deref())).collect();
        assert_eq!(
            shown,
            [
                ("/blog/{id}", Some("blog_show"), vec!["GET".to_owned(), "HEAD".to_owned()], "BlogController", Some("show")),
                ("/blog/new", None, vec!["POST".to_owned()], "BlogController", Some("new")),
            ]
        );
        assert_eq!(defs[0].at, Position::new(5, 13));
    }

    #[test]
    fn reads_yaml_routes_with_controller_and_methods() {
        let source = "blog_list:\n    path: /blog\n    controller: App\\Controller\\BlogController::list\n    methods: [GET, POST]\nhome:\n    path: /\n    controller: App\\Controller\\HomeController\n";
        let defs = yaml_route_defs("/p/config/routes.yaml", source);
        let shown: Vec<_> = defs.iter().map(|d| (d.name.as_deref(), d.path.as_str(), d.methods.clone(), d.class.as_str(), d.member.as_deref())).collect();
        assert_eq!(
            shown,
            [
                (Some("blog_list"), "/blog", vec!["GET".to_owned(), "POST".to_owned()], "BlogController", Some("list")),
                (Some("home"), "/", vec![], "HomeController", None),
            ]
        );
    }
}
