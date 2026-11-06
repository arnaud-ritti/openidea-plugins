//! Text inspections for PHP, YAML and XML files, written from the Symfony
//! documentation (routing, controllers, service container, forms, assets,
//! translation). Each group reads one file's text and the project index; a
//! check stays silent when the class, service or file it would need is not
//! in the index.

use std::ops::Range;

use openidea_plugin::Severity;

use crate::index::{self, Index};
use crate::refs;

#[derive(Debug, PartialEq, Eq)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
}

impl Finding {
    fn warn(range: Range<usize>, code: &'static str, message: String) -> Finding {
        Finding { start: range.start, end: range.end, code, message, severity: Severity::Warning }
    }
}

/// Source of a project file by absolute path.
pub type Reader<'a> = &'a dyn Fn(&str) -> Option<String>;

pub struct Context<'a> {
    pub index: &'a Index,
    pub read: Reader<'a>,
}

fn is_route_file(path: &str) -> bool {
    path.contains("/config/routes") || path.contains("/config/routing") || path.contains("/Resources/config/routing")
}

fn offset_in(text: &str, part: &str) -> usize {
    part.as_ptr() as usize - text.as_ptr() as usize
}

/// Range of an unquoted YAML scalar value on the key's line.
fn yaml_scalar(text: &str, key: &index::YamlKey) -> Option<Range<usize>> {
    let line_end = text[key.offset..].find('\n').map_or(text.len(), |i| key.offset + i);
    let line = text[key.offset..line_end].trim_end_matches('\r');
    let after_colon = &line[line.find(':')? + 1..];
    let value = after_colon.split(" #").next()?.trim();

    let value = match value.chars().next()? {
        quote @ ('\'' | '"') => value.strip_prefix(quote)?.strip_suffix(quote)?,
        '[' | '{' | '|' | '>' | '&' | '*' | '!' => return None,
        _ => value,
    };

    let start = offset_in(text, value);
    Some(start..start + value.len())
}

/// Whether text after `class Name` is a plain controller: a method it lacks
/// cannot come from a parent class, a trait or `__call`.
fn is_plain_class(source: &str) -> bool {
    let Some(declaration) = index::class_declaration(source) else { return false };
    let header_end = source[declaration..].find('{').map_or(source.len(), |i| declaration + i);

    let mut words = source[declaration..header_end].split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty());
    while let Some(word) = words.next() {
        if word == "extends" {
            let parent = words.next().unwrap_or_default().rsplit('\\').next().unwrap_or_default();
            if !matches!(parent, "AbstractController" | "Controller") {
                return false;
            }
        }
    }

    let body = &source[header_end..];
    !body.contains("function __call") && !body.lines().any(|line| line.trim_start().starts_with("use ") && !line.contains('('))
}

#[derive(Debug, PartialEq, Eq)]
enum Method {
    Public,
    NonPublic,
    Absent,
}

fn method_state(source: &str, name: &str) -> Method {
    let mut state = Method::Absent;

    for (at, _) in source.match_indices("function ") {
        let declared = source[at + "function ".len()..].trim_start().trim_start_matches('&');
        let length = declared.bytes().take_while(|b| refs::is_identifier(*b)).count();
        if !declared[..length].eq_ignore_ascii_case(name) {
            continue;
        }

        let line_start = source[..at].rfind('\n').map_or(0, |i| i + 1);
        let modifiers = &source[line_start..at];
        if modifiers.contains("private") || modifiers.contains("protected") {
            state = Method::NonPublic;
            continue;
        }

        return Method::Public;
    }

    state
}

/// A project class's source and whether it is plain, or `None` for classes
/// that are unknown, vendor code or may inherit what they lack.
fn plain_class_source(ctx: &Context, class: &str) -> Option<String> {
    let entry = ctx.index.classes.get(class.trim_start_matches('\\'))?;
    let source = (ctx.read)(&entry.path)?;
    is_plain_class(&source).then_some(source)
}

/// The controller of a route: `App\Controller\Foo::bar` or an invokable class.
fn check_controller(ctx: &Context, spec: &str, range: Range<usize>, out: &mut Vec<Finding>) {
    let spec = spec.replace("\\\\", "\\");

    if let Some((class, method)) = spec.split_once("::") {
        let Some(source) = plain_class_source(ctx, class) else { return };
        if method_state(&source, method) == Method::Public {
            return;
        }

        let message = format!("Controller \"{}\" has no public method \"{method}\".", class.trim_start_matches('\\'));
        out.push(Finding::warn(range, "controller-method-missing", message));
        return;
    }

    if !spec.contains('\\') {
        return;
    }

    let Some(source) = plain_class_source(ctx, &spec) else { return };
    if method_state(&source, "__invoke") == Method::Public {
        return;
    }

    let message = format!("Controller \"{spec}\" is not invokable: add a public __invoke() method or name the method with \"::\".");
    out.push(Finding::warn(range, "controller-method-missing", message));
}

/// `Bundle:Controller:action`: the notation was removed in Symfony 4.0.
fn is_bundle_notation(spec: &str) -> bool {
    let parts: Vec<&str> = spec.split(':').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| refs::is_identifier(b) || b == b'\\'))
}

fn check_route_setting_controller(ctx: &Context, spec: &str, range: Range<usize>, out: &mut Vec<Finding>) {
    if is_bundle_notation(spec) {
        let message = format!("The \"{spec}\" controller notation was removed in Symfony 4.0; use \"App\\Controller\\Class::method\".");
        out.push(Finding::warn(range, "deprecated-route-setting", message));
        return;
    }

    check_controller(ctx, spec, range, out);
}

const ANNOTATION_MESSAGE: &str = "The route type \"annotation\" is deprecated since Symfony 6.4 and removed in 7.0; use \"attribute\".";
const PATTERN_MESSAGE: &str = "The route setting \"pattern\" was removed in Symfony 3.0; use \"path\".";

fn yaml_routes(ctx: &Context, text: &str, out: &mut Vec<Finding>) {
    let keys = index::yaml_keys(text);

    for key in &keys {
        let key_range = key.offset..key.offset + key.key.len();
        let top_level = key.parents.len() == 1;

        match key.key.as_str() {
            "controller" | "_controller" => {
                if let Some(range) = yaml_scalar(text, key) {
                    check_route_setting_controller(ctx, &text[range.clone()], range, out);
                }
            }
            "pattern" if top_level => out.push(Finding::warn(key_range, "deprecated-route-setting", PATTERN_MESSAGE.into())),
            "type" if top_level => {
                if let Some(range) = yaml_scalar(text, key).filter(|r| &text[r.clone()] == "annotation") {
                    out.push(Finding::warn(range, "deprecated-route-setting", ANNOTATION_MESSAGE.into()));
                }
            }
            "_method" | "_scheme" if key.parents.last().is_some_and(|p| p == "requirements") => {
                let message = format!("The \"{}\" requirement was removed in Symfony 3.0; use the \"methods\" or \"schemes\" route settings.", key.key);
                out.push(Finding::warn(key_range, "deprecated-route-setting", message));
            }
            _ => {}
        }
    }
}

fn xml_routes(ctx: &Context, text: &str, out: &mut Vec<Finding>) {
    for (tag, at) in index::xml_tags(text, "route") {
        if let Some((spec, offset)) = index::xml_attribute(tag, "controller") {
            check_route_setting_controller(ctx, spec, at + offset..at + offset + spec.len(), out);
        }

        if let Some((_, offset)) = index::xml_attribute(tag, "pattern") {
            out.push(Finding::warn(at + offset..at + offset + "pattern".len(), "deprecated-route-setting", PATTERN_MESSAGE.into()));
        }
    }

    for (tag, at) in index::xml_tags(text, "import") {
        if let Some((_, offset)) = index::xml_attribute(tag, "type").filter(|(value, _)| *value == "annotation") {
            out.push(Finding::warn(at + offset..at + offset + "annotation".len(), "deprecated-route-setting", ANNOTATION_MESSAGE.into()));
        }
    }
}

/// A class-level `#[Route]` with neither an `__invoke` nor routed methods.
fn php_route_class(text: &str, out: &mut Vec<Finding>) {
    let Some(declaration) = index::class_declaration(text) else { return };
    let header = text[..declaration].rsplit([';', '}']).next().unwrap_or_default();
    if !header.contains("#[Route") && !header.contains("@Route") {
        return;
    }

    let line_start = text[..declaration].rfind('\n').map_or(0, |i| i + 1);
    if text[line_start..declaration].contains("abstract") {
        return;
    }

    let body = &text[declaration..];
    if body.contains("#[Route") || body.contains("@Route") || method_state(body, "__invoke") == Method::Public || !is_plain_class(text) {
        return;
    }

    let start = declaration + "class ".len();
    let name = text[start..].bytes().take_while(|b| refs::is_identifier(*b)).count();
    let message = format!("Route class \"{}\" has no public __invoke() and none of its methods has a #[Route].", &text[start..start + name]);
    out.push(Finding::warn(start..start + name, "controller-method-missing", message));
}

/// `controller-method-missing` and `deprecated-route-setting`.
pub fn check_routes(ctx: &Context, path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();

    match language {
        "yaml" if is_route_file(path) => yaml_routes(ctx, text, &mut out),
        "xml" if is_route_file(path) && text.contains("<routes") => xml_routes(ctx, text, &mut out),
        "php" => php_route_class(text, &mut out),
        _ => {}
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position, Range as TextRange};

    use super::*;
    use crate::index::Entry;
    use crate::{Groups, Main};

    const BLOG: &str = "<?php\nnamespace App\\Controller;\nclass BlogController\n{\n    public function index() {}\n    private function hidden() {}\n}\n";
    const INVOKABLE: &str = "<?php\nnamespace App\\Controller;\nclass InvokableController\n{\n    public function __invoke() {}\n}\n";
    const CHILD: &str = "<?php\nnamespace App\\Controller;\nclass ChildController extends BaseController\n{\n}\n";

    fn sources() -> BTreeMap<String, String> {
        let files = [
            ("App\\Controller\\BlogController", BLOG),
            ("App\\Controller\\InvokableController", INVOKABLE),
            ("App\\Controller\\ChildController", CHILD),
        ];
        files.iter().map(|(class, text)| (format!("/project/{}.php", class.replace('\\', "/")), (*text).to_owned())).collect()
    }

    fn fixture_index() -> Index {
        let mut index = Index::default();
        for class in ["App\\Controller\\BlogController", "App\\Controller\\InvokableController", "App\\Controller\\ChildController"] {
            let path = format!("/project/{}.php", class.replace('\\', "/"));
            index.classes.insert(class.into(), Entry { detail: None, path, at: Position::new(0, 0) });
        }
        index
    }

    fn run_with(index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        let files = sources();
        let read = |file: &str| files.get(file).cloned();
        let groups = Groups { controllers: true };
        crate::extra_problems(index, groups, &read, path, language, text)
    }

    fn run(path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        run_with(&fixture_index(), path, language, text)
    }

    fn codes(found: &[Diagnostic]) -> Vec<&str> {
        found.iter().map(|d| d.code.as_deref().unwrap_or_default()).collect()
    }

    fn opened(host: &MockHost, plugin: &mut Main, path: &str) -> Vec<Diagnostic> {
        plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());
        host.diagnostics().get(path).cloned().unwrap_or_default()
    }

    fn project(host: &MockHost) -> Main {
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        host.add_file("src/Controller/BlogController.php", &BLOG.replace("Controller;\n", "Controller;\n"));
        host.add_file("translations/messages.en.yaml", "app: x\n");
        host.add_file("public/img/logo.png", "");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin
    }

    const ROUTES: &str = "/project/config/routes.yaml";

    #[test]
    fn controller_method_missing_warns() {
        let yaml = "blog:\n    path: /blog\n    controller: App\\Controller\\BlogController::missing\n";
        let found = run(ROUTES, "yaml", yaml);
        assert_eq!(codes(&found), ["controller-method-missing"]);
        assert_eq!(found[0].range.start, Position::new(2, 16));
        assert_eq!(found[0].message, "Controller \"App\\Controller\\BlogController\" has no public method \"missing\".");

        let private = "blog:\n    controller: \"App\\\\Controller\\\\BlogController::hidden\"\n";
        assert_eq!(codes(&run(ROUTES, "yaml", private)), ["controller-method-missing"]);

        let xml = "<routes><route id=\"a\" path=\"/\" controller=\"App\\Controller\\BlogController::nope\"/></routes>";
        assert_eq!(codes(&run("/project/config/routes.xml", "xml", xml)), ["controller-method-missing"]);

        let not_invokable = "blog:\n    controller: App\\Controller\\BlogController\n";
        assert_eq!(codes(&run(ROUTES, "yaml", not_invokable)), ["controller-method-missing"]);

        let php = "<?php\n#[Route('/blog')]\nclass Home\n{\n    public function index() {}\n}\n";
        let found = run("/project/src/Controller/Home.php", "php", php);
        assert_eq!(codes(&found), ["controller-method-missing"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(2, 6), Position::new(2, 10)));
    }

    #[test]
    fn controller_method_missing_is_quiet_for_library_or_dynamic_name() {
        let quiet = |controller: &str| {
            let yaml = format!("blog:\n    controller: {controller}\n");
            assert!(run(ROUTES, "yaml", &yaml).is_empty(), "{controller}");
        };
        quiet("Vendor\\Package\\Controller::anything");
        quiet("app.blog_controller:index");
        quiet("app.blog_controller");
        quiet("'%app.controller%'");
        quiet("App\\Controller\\BlogController::index");
        quiet("App\\Controller\\BlogController::INDEX");
        quiet("App\\Controller\\InvokableController");
        quiet("App\\Controller\\InvokableController::__invoke");
        quiet("App\\Controller\\ChildController::inherited");
        quiet("Symfony\\Bundle\\FrameworkBundle\\Controller\\RedirectController::redirectAction");

        let invoke = "<?php\n#[Route('/x')]\nclass One\n{\n    public function __invoke() {}\n}\n";
        assert!(run("/project/src/One.php", "php", invoke).is_empty());
        let routed = "<?php\n#[Route('/x')]\nclass Two\n{\n    #[Route('/y')]\n    public function y() {}\n}\n";
        assert!(run("/project/src/Two.php", "php", routed).is_empty());
        let child = "<?php\n#[Route('/x')]\nclass Three extends Base\n{\n}\n";
        assert!(run("/project/src/Three.php", "php", child).is_empty());

        let empty = Index::default();
        let yaml = "blog:\n    controller: App\\Controller\\BlogController::missing\n";
        assert!(run_with(&empty, ROUTES, "yaml", yaml).is_empty());
        assert!(run("/project/config/services.yaml", "yaml", yaml).is_empty());
    }

    #[test]
    fn deprecated_route_setting_warns() {
        let yaml = "old:\n    pattern: /old\n    controller: AppBundle:Default:index\n    requirements:\n        _method: GET\nimport:\n    resource: ../src/\n    type: annotation\n";
        let found = run(ROUTES, "yaml", yaml);
        assert_eq!(codes(&found), ["deprecated-route-setting"; 4]);
        assert_eq!(found[2].range, TextRange::new(Position::new(4, 8), Position::new(4, 15)));
        assert_eq!(found[3].range, TextRange::new(Position::new(7, 10), Position::new(7, 20)));

        let xml = "<routes><import resource=\"x\" type=\"annotation\"/><route id=\"a\" pattern=\"/\" controller=\"A:B:c\"/></routes>";
        assert_eq!(codes(&run("/project/config/routes.xml", "xml", xml)), ["deprecated-route-setting"; 3]);
    }

    #[test]
    fn deprecated_route_setting_is_quiet_for_library_or_dynamic_name() {
        let yaml = "ok:\n    path: /ok\n    utf8: false\n    methods: [GET]\n    controller: App\\Controller\\BlogController::index\n    requirements:\n        id: '\\d+'\nimport:\n    resource: ../src/\n    type: attribute\nother:\n    controller: Vendor:Thing\n    options:\n        pattern: x\n";
        assert!(run(ROUTES, "yaml", yaml).is_empty());
        assert!(run("/project/config/services.yaml", "yaml", "a:\n    pattern: x\n    type: annotation\n").is_empty());
    }

    #[test]
    fn route_group_ranges_use_utf16_columns_with_multibyte_text() {
        let xml = "<!-- é😀 --><routes><route id=\"a\" path=\"/\" controller=\"App\\Controller\\BlogController::nope\"/></routes>";
        let found = run("/project/config/routes.xml", "xml", xml);
        assert_eq!(found[0].range.start, Position::new(0, 12 + 8 + 35));
        assert_eq!(found[0].range.end.character, 12 + 8 + 35 + 35);
    }

    #[test]
    fn route_group_crlf_files_report_correct_lines() {
        let yaml = "blog:\r\n    path: /blog\r\n    controller: App\\Controller\\BlogController::missing\r\n";
        let found = run(ROUTES, "yaml", yaml);
        assert_eq!(found[0].range.start, Position::new(2, 16));
        assert_eq!(found[0].range.end, Position::new(2, 16 + 38));
    }

    #[test]
    fn route_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        let path = "/project/config/routes.yaml";

        host.add_file("config/routes.yaml", "blog:\n    pattern: /blog\n");
        assert_eq!(codes(&opened(&host, &mut plugin, path)), ["deprecated-route-setting"]);

        host.add_file("config/routes.yaml", "blog:\n    path: /blog\n");
        plugin.handle_events(&[Event::FileSaved(path.into())], &host.invocation());
        assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn route_group_missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        assert!(opened(&host, &mut plugin, "/project/config/gone.yaml").is_empty());
    }
}
