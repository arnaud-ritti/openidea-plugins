//! Project-wide navigation for the host's document index. Keys: `service#id`,
//! `route#name`, `command#name`, `param#name`, `twig-filter#`, `twig-function#`,
//! `twig-test#`, `twig-component#`, `stimulus#`; a definition is where the file
//! names it, a reference is a use. Gutter markers use keys of their own
//! (`template#`, `render#`, `block#`, `override#`, `class#`, `class-service#`,
//! `route-use#`), so one icon can open several targets. Reuses the scanners of
//! the project index.

use crate::index::{self, Sink};
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

    for (start, end) in quoted_after(text, "getParameter(").into_iter().chain(quoted_after(text, "hasParameter(")) {
        out.reference(format!("param#{}", &text[start..end]), range_of(text, start, end));
    }

    for (start, end) in quoted_after(text, "->find(") {
        out.reference(format!("command#{}", &text[start..end]), range_of(text, start, end));
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
}
