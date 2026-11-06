//! Text inspections for PHP, YAML and XML files, written from the Symfony
//! documentation (routing, controllers, service container, forms, assets,
//! translation). Each group reads one file's text and the project index; a
//! check stays silent when the class, service or file it would need is not
//! in the index.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Range;

use openidea_plugin::Severity;

use crate::index::{self, Index};
use crate::refs;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
    /// Text that replaces `start..end` in a quick fix.
    pub replacement: Option<String>,
}

impl Finding {
    fn with_severity(range: Range<usize>, code: &'static str, message: String, severity: Severity) -> Finding {
        Finding { start: range.start, end: range.end, code, message, severity, replacement: None }
    }

    pub(crate) fn warn(range: Range<usize>, code: &'static str, message: String) -> Finding {
        Finding::with_severity(range, code, message, Severity::Warning)
    }

    pub(crate) fn hint(range: Range<usize>, code: &'static str, message: String) -> Finding {
        Finding::with_severity(range, code, message, Severity::Hint)
    }

    pub(crate) fn error(range: Range<usize>, code: &'static str, message: String) -> Finding {
        Finding::with_severity(range, code, message, Severity::Error)
    }
}

/// Source of a project file by absolute path.
pub type Reader<'a> = &'a dyn Fn(&str) -> Option<String>;

const MAX_CLASS_BYTES: usize = 256 * 1024;
const MAX_CLASS_READS: usize = 500;

pub struct Context<'a> {
    pub index: &'a Index,
    read: Reader<'a>,
    sources: RefCell<BTreeMap<String, Option<String>>>,
}

impl<'a> Context<'a> {
    pub fn new(index: &'a Index, read: Reader<'a>) -> Context<'a> {
        Context { index, read, sources: RefCell::default() }
    }

    /// A class file, read at most once per run; `None` when it is too large
    /// or the run already read `MAX_CLASS_READS` files.
    fn source(&self, path: &str) -> Option<String> {
        let mut sources = self.sources.borrow_mut();
        if let Some(cached) = sources.get(path) {
            return cached.clone();
        }

        if sources.len() >= MAX_CLASS_READS {
            return None;
        }

        let source = (self.read)(path).filter(|text| text.len() <= MAX_CLASS_BYTES);
        sources.insert(path.to_owned(), source.clone());
        source
    }
}

fn is_route_file(path: &str) -> bool {
    path.contains("/config/routes") || path.contains("/config/routing") || path.contains("/Resources/config/routing")
}

pub(crate) fn is_service_file(path: &str) -> bool {
    path.contains("/config/") && !is_route_file(path) && !path.contains("/var/cache/")
}

pub(crate) fn offset_in(text: &str, part: &str) -> usize {
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
        if word == "extends" && !is_framework_controller(source, words.next().unwrap_or_default()) {
            return false;
        }
    }

    let body = &source[header_end..];
    !body.contains("function __call") && !body.lines().any(|line| line.trim_start().starts_with("use ") && !line.contains('('))
}

const FRAMEWORK_CONTROLLERS: &str = "Symfony\\Bundle\\FrameworkBundle\\Controller\\";

/// Whether a parent class name resolves to the framework's base controller.
fn is_framework_controller(source: &str, parent: &str) -> bool {
    let parent = parent.trim_start_matches('\\');
    if let Some(name) = parent.strip_prefix(FRAMEWORK_CONTROLLERS) {
        return matches!(name, "AbstractController" | "Controller");
    }

    matches!(parent, "AbstractController" | "Controller") && source.contains(&format!("use {FRAMEWORK_CONTROLLERS}{parent};"))
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
    let source = ctx.source(&entry.path)?;
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

fn annotation_deprecated(ctx: &Context) -> bool {
    ctx.index.framework_version.is_some_and(|version| version >= (6, 4))
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
            "type" if top_level && annotation_deprecated(ctx) => {
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

    for (tag, at) in index::xml_tags(text, "import").into_iter().filter(|_| annotation_deprecated(ctx)) {
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

struct ServiceDef {
    class: Option<(String, Range<usize>)>,
    named: Vec<(String, Range<usize>)>,
}

fn clean_class(class: &str) -> String {
    class.replace("\\\\", "\\").trim_start_matches('\\').to_owned()
}

/// `$name` keys of a flow mapping on one line: `{ $dsn: x, $port: 25 }`.
fn flow_named_arguments(text: &str, from: usize) -> Vec<(String, Range<usize>)> {
    let line_end = text[from..].find('\n').map_or(text.len(), |i| from + i);
    let line = &text[from..line_end];
    let Some(open) = line.find('{') else { return Vec::new() };

    line.match_indices('$')
        .filter(|(at, _)| *at > open)
        .filter_map(|(at, _)| {
            let length = 1 + line[at + 1..].bytes().take_while(|b| refs::is_identifier(*b)).count();
            let follows = line[at + length..].trim_start().starts_with(':');
            (length > 1 && follows).then(|| (line[at..at + length].to_owned(), from + at..from + at + length))
        })
        .collect()
}

fn yaml_definitions(text: &str) -> Vec<ServiceDef> {
    let keys = index::yaml_keys(text);
    let mut definitions = Vec::new();

    for service in keys.iter().filter(|k| k.parents == ["services"] && !k.key.starts_with('_') && !k.key.ends_with('\\')) {
        let own = |k: &&index::YamlKey| k.parents.len() >= 2 && k.parents[0] == "services" && k.parents[1] == service.key;
        let child = |name: &str| keys.iter().filter(own).find(|k| k.parents.len() == 2 && k.key == name);

        let class = match child("class") {
            Some(key) => yaml_scalar(text, key).map(|range| (clean_class(&text[range.clone()]), range)),
            None if service.key.contains('\\') && !service.value.starts_with('@') && child("alias").is_none() => {
                Some((clean_class(&service.key), service.offset..service.offset + service.key.len()))
            }
            None => None,
        };

        let mut named = Vec::new();
        if child("parent").is_none() && child("factory").is_none() && child("constructor").is_none() {
            for key in keys.iter().filter(own) {
                if key.parents.len() == 3 && key.parents[2] == "arguments" && key.key.starts_with('$') {
                    named.push((key.key.clone(), key.offset..key.offset + key.key.len()));
                }
                if key.parents.len() == 2 && key.key == "arguments" && key.value.starts_with('{') {
                    named.extend(flow_named_arguments(text, key.offset));
                }
            }
        }

        definitions.push(ServiceDef { class, named });
    }

    definitions
}

fn xml_definitions(text: &str) -> Vec<ServiceDef> {
    let mut definitions = Vec::new();

    for (tag, at) in index::xml_tags(text, "service") {
        let attribute = |name: &str| index::xml_attribute(tag, name).map(|(value, offset)| (value, at + offset));
        let class = attribute("class")
            .or_else(|| attribute("id").filter(|(id, _)| id.contains('\\')))
            .filter(|(class, _)| !class.starts_with('%'))
            .map(|(class, start)| (clean_class(class), start..start + class.len()));

        let mut named = Vec::new();
        let body_end = if tag.ends_with('/') { None } else { text[at..].find("</service>").map(|i| at + i) };
        let body_end = body_end.filter(|end| attribute("parent").is_none() && attribute("constructor").is_none() && !text[at..*end].contains("<factory"));
        if let Some(end) = body_end {
            let body = &text[at + tag.len()..end];
            let skipped = ["call", "configurator"].map(|name| xml_spans(text, at + tag.len(), end, name));

            for (argument, argument_at) in index::xml_tags(body, "argument") {
                let argument_at = at + tag.len() + argument_at;
                let inside_skipped = skipped.iter().flatten().any(|span| span.contains(&argument_at));
                let Some((key, offset)) = index::xml_attribute(argument, "key").filter(|(key, _)| key.starts_with('$')) else { continue };
                if !inside_skipped {
                    named.push((key.to_owned(), argument_at + offset..argument_at + offset + key.len()));
                }
            }
        }

        definitions.push(ServiceDef { class, named });
    }

    definitions
}

/// Byte spans of `<name …>…</name>` (or self-closing) elements within a range.
fn xml_spans(text: &str, from: usize, to: usize, name: &str) -> Vec<Range<usize>> {
    index::xml_tags(&text[from..to], name)
        .into_iter()
        .map(|(tag, at)| {
            let start = from + at;
            let end = if tag.ends_with('/') { start + tag.len() } else { text[start..to].find(&format!("</{name}>")).map_or(to, |i| start + i) };
            start..end
        })
        .collect()
}

fn is_deprecated(source: &str) -> bool {
    let Some(declaration) = index::class_declaration(source) else { return false };
    let header = source[..declaration].rsplit([';', '}']).next().unwrap_or_default();
    header.contains("@deprecated") || header.contains("#[Deprecated") || header.contains("#[\\Deprecated")
}

/// Parameter names of a class's constructor; `None` when the class has no
/// constructor of its own or the constructor is variadic.
fn constructor_parameters(source: &str) -> Option<Vec<String>> {
    let at = source.find("function __construct")?;
    let open = at + source[at..].find('(')?;
    let close = index::closing_paren(source, open + 1).min(source.len());
    let parameters = &source[open + 1..close];

    if parameters.contains("...") {
        return None;
    }

    let names = parameters.match_indices('$').map(|(at, _)| {
        let length = 1 + parameters[at + 1..].bytes().take_while(|b| refs::is_identifier(*b)).count();
        parameters[at..at + length].to_owned()
    });

    Some(names.collect())
}

fn duplicate_parameters(language: &str, text: &str, out: &mut Vec<Finding>) {
    let mut names: Vec<String> = Vec::new();
    let mut report = |name: &str, range: Range<usize>, names: &mut Vec<String>| {
        if names.iter().any(|earlier| earlier == name) {
            let message = format!("Parameter \"{name}\" is already defined in this file.");
            out.push(Finding::warn(range, "duplicate-parameter-key", message));
        }
        names.push(name.to_owned());
    };

    if language == "xml" {
        let mut events: Vec<(usize, Option<(&str, usize)>, bool)> = Vec::new();
        for (tag, at) in index::xml_tags(text, "parameter") {
            let key = index::xml_attribute(tag, "key").map(|(value, offset)| (value, at + offset));
            events.push((at, key, !tag.ends_with('/')));
        }
        for (at, _) in text.match_indices("</parameter>") {
            events.push((at, None, false));
        }
        events.sort_by_key(|(at, _, _)| *at);

        let mut depth = 0usize;
        for (_, key, opens) in events {
            let is_close = key.is_none() && !opens;
            if is_close {
                depth = depth.saturating_sub(1);
                continue;
            }
            if depth == 0 {
                if let Some((name, start)) = key {
                    report(name, start..start + name.len(), &mut names);
                }
            }
            if opens {
                depth += 1;
            }
        }
        return;
    }

    let mut in_parameters = false;
    let mut child_indent: Option<usize> = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let start = offset;
        offset += line.len();

        let content = line.trim_end();
        let trimmed = content.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = content.len() - trimmed.len();
        if indent == 0 {
            in_parameters = trimmed.starts_with("parameters:");
            child_indent = None;
            continue;
        }

        if !in_parameters || *child_indent.get_or_insert(indent) != indent {
            continue;
        }

        if let Some((key, _)) = index::split_yaml_key(trimmed) {
            let quoted = usize::from(trimmed.starts_with(['\'', '"']));
            let key_start = start + indent + quoted;
            report(key, key_start..key_start + key.len(), &mut names);
        }
    }
}

/// `deprecated-service-class`, `duplicate-parameter-key` and `named-argument-missing`.
pub fn check_services(ctx: &Context, path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();

    let definitions = match language {
        "yaml" if is_service_file(path) => yaml_definitions(text),
        "xml" if is_service_file(path) && text.contains("<container") => xml_definitions(text),
        _ => return out,
    };
    duplicate_parameters(language, text, &mut out);

    for definition in &definitions {
        let Some((class, range)) = &definition.class else { continue };
        let Some(entry) = ctx.index.classes.get(class) else { continue };
        let Some(source) = ctx.source(&entry.path) else { continue };

        if is_deprecated(&source) {
            out.push(Finding::warn(range.clone(), "deprecated-service-class", format!("Service class \"{class}\" is deprecated.")));
        }

        let Some(parameters) = constructor_parameters(&source) else { continue };
        for (name, range) in &definition.named {
            if !parameters.contains(name) {
                let message = format!("Constructor of \"{class}\" has no argument \"{name}\".");
                out.push(Finding::warn(range.clone(), "named-argument-missing", message));
            }
        }
    }

    out
}

pub(crate) const FORM_TYPES: &[(&str, &str)] = &[
    ("text", "TextType"),
    ("textarea", "TextareaType"),
    ("email", "EmailType"),
    ("integer", "IntegerType"),
    ("money", "MoneyType"),
    ("number", "NumberType"),
    ("password", "PasswordType"),
    ("percent", "PercentType"),
    ("search", "SearchType"),
    ("url", "UrlType"),
    ("range", "RangeType"),
    ("choice", "ChoiceType"),
    ("entity", "EntityType"),
    ("country", "CountryType"),
    ("language", "LanguageType"),
    ("locale", "LocaleType"),
    ("timezone", "TimezoneType"),
    ("currency", "CurrencyType"),
    ("date", "DateType"),
    ("datetime", "DateTimeType"),
    ("time", "TimeType"),
    ("birthday", "BirthdayType"),
    ("checkbox", "CheckboxType"),
    ("file", "FileType"),
    ("radio", "RadioType"),
    ("collection", "CollectionType"),
    ("repeated", "RepeatedType"),
    ("hidden", "HiddenType"),
    ("button", "ButtonType"),
    ("submit", "SubmitType"),
    ("reset", "ResetType"),
    ("form", "FormType"),
];

/// Translation domains that Symfony's own bundles provide.
const BUILT_IN_DOMAINS: &[&str] = &["messages", "validators", "security"];

/// Positions of the `(` of every call that ends with `needle` (which ends in
/// `(`). A needle that starts with a name is a function call: not a method,
/// not part of a longer name.
pub(crate) fn call_opens<'a>(text: &'a str, needle: &'a str) -> impl Iterator<Item = usize> + 'a {
    let function = needle.as_bytes()[0].is_ascii_alphabetic();

    text.match_indices(needle).filter_map(move |(at, _)| {
        let before = text[..at].bytes().last();
        let member = before.is_some_and(|b| refs::is_identifier(b) || matches!(b, b'>' | b':' | b'$' | b'\\'));
        (!(function && member)).then_some(at + needle.len() - 1)
    })
}

/// Range of the contents of a quoted string argument.
pub(crate) fn literal(text: &str, argument: &str) -> Option<Range<usize>> {
    let argument = argument.trim();
    let quote = argument.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let inner = argument.strip_prefix(quote)?.strip_suffix(quote)?;
    let start = offset_in(text, inner);
    Some(start..start + inner.len())
}

pub(crate) fn is_dynamic(name: &str) -> bool {
    name.is_empty() || name.contains(['{', '$', '~', '#'])
}

/// Literal aliases in the second argument of `->add()` calls of a form type,
/// with the alias and the class that replaced it.
pub(crate) fn form_type_literals(text: &str) -> Vec<(Range<usize>, &'static str, &'static str)> {
    if !text.contains("FormBuilderInterface") && !text.contains("AbstractType") && !text.contains("$builder") {
        return Vec::new();
    }

    let mut found = Vec::new();
    for open in call_opens(text, "->add(") {
        let arguments = refs::arguments(text, open);
        let Some(range) = arguments.get(1).and_then(|argument| literal(text, argument)) else { continue };
        let Some((alias, class)) = FORM_TYPES.iter().find(|(alias, _)| *alias == &text[range.clone()]) else { continue };

        found.push((range, *alias, *class));
    }

    found
}

fn form_type_strings(text: &str, out: &mut Vec<Finding>) {
    for (range, alias, class) in form_type_literals(text) {
        let message = format!("Form type \"{alias}\" was removed in Symfony 3; use {class}::class.");
        out.push(Finding::warn(range, "form-type-string", message));
    }
}

fn missing_assets(ctx: &Context, text: &str, out: &mut Vec<Finding>) {
    if ctx.index.assets.is_empty() {
        return;
    }

    let uses_asset_component = text.contains("Packages") || text.contains("Symfony\\Component\\Asset");
    let get_url = call_opens(text, "->getUrl(").filter(|_| uses_asset_component);

    for open in get_url.chain(call_opens(text, "asset(")) {
        let arguments = refs::arguments(text, open);
        if arguments.len() != 1 {
            continue;
        }

        let Some(range) = literal(text, arguments[0]) else { continue };
        let name = &text[range.clone()];
        if asset_is_missing(ctx, name) {
            out.push(Finding::warn(range, "missing-asset", format!("Asset \"{name}\" does not exist in the public directory.")));
        }
    }
}

/// An `asset()` path that no indexed public file matches; false for URLs,
/// dynamic names, generated output and directories the index never saw.
fn asset_is_missing(ctx: &Context, name: &str) -> bool {
    let external = name.contains("://") || name.starts_with(['/', '#']) || name.starts_with("data:");
    if external || is_dynamic(name) {
        return false;
    }

    let file = name.split(['?', '#']).next().unwrap_or(name);
    let generated = matches!(file.split('/').next(), Some("build" | "bundles" | "assets" | "_assets"));
    let directory_indexed = file.rsplit_once('/').is_none_or(|(directory, _)| {
        let prefix = format!("{directory}/");
        ctx.index.assets.keys().any(|known| known.starts_with(&prefix))
    });
    !generated && directory_indexed && !ctx.index.assets.contains_key(file)
}

/// A literal domain that looks project-local (lowercase, `+intl-icu` allowed)
/// yet has no translation files; never true before translations were indexed.
pub(crate) fn translation_domain_is_unknown(ctx: &Context, domain: &str) -> bool {
    let name = domain.strip_suffix("+intl-icu").unwrap_or(domain);
    let project_local = !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'));

    !ctx.index.translations.is_empty() && project_local && !BUILT_IN_DOMAINS.contains(&name) && !ctx.index.translations.contains_key(name)
}

fn missing_translation_domains(ctx: &Context, text: &str, out: &mut Vec<Finding>) {
    if ctx.index.translations.is_empty() {
        return;
    }

    for open in call_opens(text, "->trans(").chain(call_opens(text, "t(")) {
        let arguments = refs::arguments(text, open);
        let named = arguments.iter().find_map(|argument| argument.trim().strip_prefix("domain:"));
        let Some(range) = named.or_else(|| arguments.get(2).copied()).and_then(|argument| literal(text, argument)) else { continue };

        let domain = &text[range.clone()];
        if !translation_domain_is_unknown(ctx, domain) {
            continue;
        }

        out.push(Finding::warn(range, "missing-translation-domain", format!("Translation domain \"{domain}\" has no translation files.")));
    }
}

/// `form-type-string`, `missing-asset` and `missing-translation-domain` in PHP.
pub fn check_php(ctx: &Context, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if language != "php" {
        return out;
    }

    form_type_strings(text, &mut out);
    missing_assets(ctx, text, &mut out);
    missing_translation_domains(ctx, text, &mut out);
    out.sort_by_key(|finding| finding.start);
    out
}

/// `twig-missing-route` and `twig-missing-asset`: `path()`, `url()` and
/// `asset()` calls with a literal name. Routes are only checked once the
/// compiled routes are known; `_`-prefixed routes belong to the framework.
pub fn check_twig(ctx: &Context, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if language != "twig" {
        return out;
    }

    for reference in refs::references("twig", text) {
        let name = &text[reference.start..reference.end];
        let rest = text[reference.end..].strip_prefix(['\'', '"']).unwrap_or(" ").trim_start();
        if is_dynamic(name) || !rest.starts_with([')', ',']) {
            continue;
        }

        let range = reference.start..reference.end;
        match reference.kind {
            refs::Kind::Route if ctx.index.compiled_routes && !name.starts_with('_') && !ctx.index.routes.contains_key(name) => {
                out.push(Finding::warn(range, "twig-missing-route", format!("Route \"{name}\" is not defined.")));
            }
            refs::Kind::Asset if !ctx.index.assets.is_empty() && asset_is_missing(ctx, name) => {
                out.push(Finding::warn(range, "twig-missing-asset", format!("Asset \"{name}\" does not exist in the public directory.")));
            }
            _ => {}
        }
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
    use crate::Main;

    const BLOG: &str = "<?php\nnamespace App\\Controller;\nclass BlogController\n{\n    public function index() {}\n    private function hidden() {}\n}\n";
    const INVOKABLE: &str = "<?php\nnamespace App\\Controller;\nclass InvokableController\n{\n    public function __invoke() {}\n}\n";
    const CHILD: &str = "<?php\nnamespace App\\Controller;\nclass ChildController extends BaseController\n{\n}\n";

    const OLD_MAILER: &str = "<?php\nnamespace App\\Legacy;\n\nuse Foo\\Bar;\n\n/**\n * @deprecated since 2.0, use Mailer\n */\nclass OldMailer\n{\n}\n";
    const ATTRIBUTED: &str = "<?php\nnamespace App\\Legacy;\n\n#[\\Deprecated('use Mailer')]\nfinal class Attributed\n{\n}\n";
    const METHOD_ONLY: &str = "<?php\nnamespace App\\Legacy;\n\nclass MethodOnly\n{\n    /** @deprecated */\n    public function old() {}\n}\n";
    const MAILER: &str = "<?php\nnamespace App;\n\nclass Mailer\n{\n    public function __construct(private string $dsn, int $port = 25)\n    {\n    }\n}\n";
    const VARIADIC: &str = "<?php\nnamespace App;\n\nclass Variadic\n{\n    public function __construct(string ...$parts)\n    {\n    }\n}\n";
    const NO_CONSTRUCTOR: &str = "<?php\nnamespace App;\n\nclass NoConstructor\n{\n}\n";

    const CLASSES: [(&str, &str); 9] = [
        ("App\\Controller\\BlogController", BLOG),
        ("App\\Controller\\InvokableController", INVOKABLE),
        ("App\\Controller\\ChildController", CHILD),
        ("App\\Legacy\\OldMailer", OLD_MAILER),
        ("App\\Legacy\\Attributed", ATTRIBUTED),
        ("App\\Legacy\\MethodOnly", METHOD_ONLY),
        ("App\\Mailer", MAILER),
        ("App\\Variadic", VARIADIC),
        ("App\\NoConstructor", NO_CONSTRUCTOR),
    ];

    fn class_path(class: &str) -> String {
        format!("/project/{}.php", class.replace('\\', "/"))
    }

    fn sources() -> BTreeMap<String, String> {
        CLASSES.iter().map(|(class, text)| (class_path(class), (*text).to_owned())).collect()
    }

    fn fixture_index() -> Index {
        let mut index = Index { framework_version: Some((7, 0)), ..Index::default() };
        for (class, _) in CLASSES {
            index.classes.insert(class.into(), Entry { detail: None, path: class_path(class), at: Position::new(0, 0) });
        }
        index
    }

    fn run_with(index: &Index, path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        let files = sources();
        let read = |file: &str| files.get(file).cloned();
        let groups = crate::Groups::all_on();
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
        host.add_file("src/Controller/BlogController.php", BLOG);
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

    const SERVICES: &str = "/project/config/services.yaml";

    #[test]
    fn deprecated_service_class_warns() {
        let yaml = "services:\n    app.mailer:\n        class: App\\Legacy\\OldMailer\n    App\\Legacy\\Attributed: ~\n";
        let found = run(SERVICES, "yaml", yaml);
        assert_eq!(codes(&found), ["deprecated-service-class"; 2]);
        assert_eq!(found[0].range, TextRange::new(Position::new(2, 15), Position::new(2, 35)));
        assert_eq!(found[0].message, "Service class \"App\\Legacy\\OldMailer\" is deprecated.");
        assert_eq!(found[1].range.start, Position::new(3, 4));

        let xml = "<container><services><service id=\"a\" class=\"App\\Legacy\\OldMailer\"/></services></container>";
        assert_eq!(codes(&run("/project/config/services.xml", "xml", xml)), ["deprecated-service-class"]);
    }

    #[test]
    fn deprecated_service_class_is_quiet_for_library_or_dynamic_name() {
        let yaml = "services:\n    a:\n        class: Vendor\\Old\\Thing\n    b:\n        class: '%app.mailer.class%'\n    c:\n        class: App\\Legacy\\MethodOnly\n    d:\n        class: App\\Mailer\n    App\\Controller\\:\n        resource: ../src\n    e:\n        alias: App\\Legacy\\OldMailer\n";
        assert!(run(SERVICES, "yaml", yaml).is_empty());

        let xml = "<container><service id=\"a\" class=\"Vendor\\Old\"/><service id=\"b\" class=\"%x%\"/></container>";
        assert!(run("/project/config/services.xml", "xml", xml).is_empty());
        assert!(run_with(&Index::default(), SERVICES, "yaml", "services:\n    a:\n        class: App\\Legacy\\OldMailer\n").is_empty());
    }

    #[test]
    fn duplicate_parameter_key_warns() {
        let xml = "<container>\n  <parameters>\n    <parameter key=\"app.a\">1</parameter>\n    <parameter key=\"app.b\"/>\n    <parameter key=\"app.a\">2</parameter>\n  </parameters>\n</container>\n";
        let found = run("/project/config/services.xml", "xml", xml);
        assert_eq!(codes(&found), ["duplicate-parameter-key"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(4, 20), Position::new(4, 25)));
        assert_eq!(found[0].message, "Parameter \"app.a\" is already defined in this file.");

        let yaml = "parameters:\n    app.a: 1\n    # app.a: commented\n    app.b: 2\n    'app.a': 3\nservices:\n    app.a: ~\n";
        let found = run(SERVICES, "yaml", yaml);
        assert_eq!(codes(&found), ["duplicate-parameter-key"]);
        assert_eq!(found[0].range, TextRange::new(Position::new(4, 5), Position::new(4, 10)));
    }

    #[test]
    fn duplicate_parameter_key_is_quiet_for_library_or_dynamic_name() {
        let xml = "<container><parameters><parameter key=\"a\" type=\"collection\"><parameter key=\"x\">1</parameter></parameter><parameter key=\"b\" type=\"collection\"><parameter key=\"x\">1</parameter></parameter><parameter key=\"c\"/></parameters></container>";
        assert!(run("/project/config/services.xml", "xml", xml).is_empty());
        let compiled = "<container><parameters><parameter key=\"a\"/><parameter key=\"a\"/></parameters></container>";
        assert!(run("/project/var/cache/dev/Kernel.xml", "xml", compiled).is_empty());

        let yaml = "parameters:\n    app.a: 1\n    app.b:\n        x: 1\n        x: 2\nwhen@dev:\n    parameters:\n        app.a: 2\n";
        assert!(run(SERVICES, "yaml", yaml).is_empty());
        assert!(run("/project/docker/compose.yaml", "yaml", "parameters:\n    a: 1\n    a: 2\n").is_empty());
    }

    #[test]
    fn named_argument_missing_warns() {
        let yaml = "services:\n    app.mailer:\n        class: App\\Mailer\n        arguments:\n            $dsn: 'smtp://'\n            $transport: '@t'\n    App\\Mailer:\n        arguments: { $dsn: x, $nope: y }\n";
        let found = run(SERVICES, "yaml", yaml);
        assert_eq!(codes(&found), ["named-argument-missing"; 2]);
        assert_eq!(found[0].range, TextRange::new(Position::new(5, 12), Position::new(5, 22)));
        assert_eq!(found[0].message, "Constructor of \"App\\Mailer\" has no argument \"$transport\".");
        assert_eq!(found[1].range, TextRange::new(Position::new(7, 30), Position::new(7, 35)));

        let xml = "<container><services><service id=\"a\" class=\"App\\Mailer\"><argument key=\"$dsn\" type=\"string\">x</argument><argument key=\"$nope\"/></service></services></container>";
        let found = run("/project/config/services.xml", "xml", xml);
        assert_eq!(codes(&found), ["named-argument-missing"]);
        assert_eq!(found[0].range.start.character as usize, xml.find("$nope").unwrap());
    }

    #[test]
    fn named_argument_missing_is_quiet_for_library_or_dynamic_name() {
        let yaml = "services:\n    a:\n        class: Vendor\\Mailer\n        arguments: { $x: 1 }\n    b:\n        class: App\\Variadic\n        arguments: { $x: 1 }\n    c:\n        class: App\\NoConstructor\n        arguments: { $x: 1 }\n    d:\n        class: App\\Mailer\n        arguments:\n            $port: 25\n            $dsn: x\n    e:\n        parent: a\n        class: App\\Mailer\n        arguments: { $x: 1 }\n    f:\n        class: '%mailer.class%'\n        arguments: { $x: 1 }\n    g:\n        class: App\\Mailer\n        bind:\n            $x: 1\n";
        assert!(run(SERVICES, "yaml", yaml).is_empty());

        let xml = "<container><service id=\"a\" class=\"App\\Mailer\"><call method=\"set\"><argument key=\"$whatever\"/></call><bind key=\"$z\"/></service><service id=\"b\" class=\"App\\Mailer\"><factory service=\"f\"/><argument key=\"$q\"/></service></container>";
        assert!(run("/project/config/services.xml", "xml", xml).is_empty());
    }

    #[test]
    fn service_group_ranges_use_utf16_columns_with_multibyte_text() {
        let xml = "<!-- é😀 --><container><service id=\"a\" class=\"App\\Legacy\\OldMailer\"/></container>";
        let found = run("/project/config/services.xml", "xml", xml);
        assert_eq!(found[0].range.start, Position::new(0, 12 + 11 + 23));
        assert_eq!(found[0].range.end.character, 12 + 11 + 23 + 20);
    }

    #[test]
    fn service_group_crlf_files_report_correct_lines() {
        let yaml = "parameters:\r\n    app.a: 1\r\n    app.a: 2\r\nservices:\r\n    a:\r\n        class: App\\Legacy\\OldMailer\r\n";
        let found = run(SERVICES, "yaml", yaml);
        assert_eq!(found[0].range, TextRange::new(Position::new(2, 4), Position::new(2, 9)));
        assert_eq!(found[1].range.start, Position::new(5, 15));
    }

    #[test]
    fn service_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        let path = "/project/config/services.yaml";

        host.add_file("config/services.yaml", "parameters:\n    a: 1\n    a: 2\n");
        assert_eq!(codes(&opened(&host, &mut plugin, path)), ["duplicate-parameter-key"]);

        host.add_file("config/services.yaml", "parameters:\n    a: 1\n");
        plugin.handle_events(&[Event::FileSaved(path.into())], &host.invocation());
        assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn service_group_missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        assert!(opened(&host, &mut plugin, "/project/config/services.xml").is_empty());
    }

    fn php_index() -> Index {
        let mut index = fixture_index();
        index.assets.insert("img/logo.png".into(), Entry { detail: None, path: "/project/public/img/logo.png".into(), at: Position::new(0, 0) });
        index.assets.insert("css/app.css".into(), Entry { detail: None, path: "/project/public/css/app.css".into(), at: Position::new(0, 0) });
        index.translations.insert("messages".into(), Default::default());
        index.translations.insert("admin".into(), Default::default());
        index
    }

    fn php(body: &str) -> String {
        format!("<?php\nclass F extends AbstractType\n{{\n    public function buildForm(FormBuilderInterface $builder, array $options)\n    {{\n{body}\n    }}\n}}\n")
    }

    fn run_php(text: &str) -> Vec<Diagnostic> {
        run_with(&php_index(), "/project/src/Form/F.php", "php", text)
    }

    const FORM: &str = "/project/src/Form/F.php";

    #[test]
    fn form_type_string_warns() {
        let text = php("        $builder->add('name', 'text')->add('body', \"textarea\", ['required' => false]);");
        let found = run_php(&text);
        assert_eq!(codes(&found), ["form-type-string"; 2]);
        assert_eq!(found[0].message, "Form type \"text\" was removed in Symfony 3; use TextType::class.");
        assert_eq!(found[0].range, TextRange::new(Position::new(5, 31), Position::new(5, 35)));
        assert_eq!(found[1].message, "Form type \"textarea\" was removed in Symfony 3; use TextareaType::class.");
    }

    #[test]
    fn form_type_string_is_quiet_for_library_or_dynamic_name() {
        for call in [
            "$builder->add('name', TextType::class);",
            "$builder->add('name', 'custom_alias');",
            "$builder->add('name', 'Text');",
            "$builder->add('name');",
            "$builder->add('name', $type);",
            "$builder->add('name', 'text' . $suffix);",
            "$builder->add('name', 'App\\\\Form\\\\TextType');",
        ] {
            assert!(run_php(&php(&format!("        {call}"))).is_empty(), "{call}");
        }

        let unrelated = "<?php\nclass Bag\n{\n    public function fill($collection)\n    {\n        $collection->add('name', 'text');\n    }\n}\n";
        assert!(run_php(unrelated).is_empty());
    }

    #[test]
    fn missing_asset_warns() {
        let text = "<?php // Packages\nclass A\n{\n    public function url($packages)\n    {\n        $packages->getUrl('img/missing.png');\n        return asset(\"css/none.css\");\n    }\n}\n";
        let found = run_php(text);
        assert_eq!(codes(&found), ["missing-asset"; 2]);
        assert_eq!(found[0].message, "Asset \"img/missing.png\" does not exist in the public directory.");
        assert_eq!(found[0].range, TextRange::new(Position::new(5, 27), Position::new(5, 42)));
    }

    #[test]
    fn missing_asset_is_quiet_for_library_or_dynamic_name() {
        for call in [
            "$packages->getUrl('img/logo.png');",
            "$packages->getUrl('img/logo.png?v=2');",
            "$packages->getUrl('https://cdn.example.com/x.js');",
            "$packages->getUrl('//cdn.example.com/x.js');",
            "$packages->getUrl('/already/absolute.png');",
            "$packages->getUrl('data:image/png;base64,AAAA');",
            "$packages->getUrl(\"img/$name.png\");",
            "$packages->getUrl('img/' . $name);",
            "$packages->getUrl('img/none.png', 'cdn');",
            "$this->asset('img/none.png');",
            "$packages->getUrl($path);",
        ] {
            let text = format!("<?php // Packages\nfunction f($packages) {{\n    {call}\n}}\n");
            assert!(run_php(&text).is_empty(), "{call}");
        }

        let text = "<?php // Packages\n$packages->getUrl('img/none.png');\n";
        assert!(run_with(&fixture_index(), FORM, "php", text).is_empty());
    }

    #[test]
    fn missing_translation_domain_warns() {
        let text = "<?php\nfunction f($translator) {\n    $translator->trans('k', [], 'nodomain');\n    t('k', [], \"other\");\n    $translator->trans('k', domain: 'named');\n}\n";
        let found = run_php(text);
        assert_eq!(codes(&found), ["missing-translation-domain"; 3]);
        assert_eq!(found[0].message, "Translation domain \"nodomain\" has no translation files.");
        assert_eq!(found[0].range, TextRange::new(Position::new(2, 33), Position::new(2, 41)));
    }

    #[test]
    fn missing_translation_domain_is_quiet_for_library_or_dynamic_name() {
        for call in [
            "$translator->trans('k', [], 'admin');",
            "$translator->trans('k', [], 'messages+intl-icu');",
            "$translator->trans('k', [], 'validators');",
            "$translator->trans('k', [], 'security');",
            "$translator->trans('k', [], $domain);",
            "$translator->trans('k', [], \"dom_{$x}\");",
            "$translator->trans('k', [], null);",
            "$translator->trans('k');",
            "$this->format('k', [], 'nodomain');",
            "$translator->trans('k', domain: $domain);",
        ] {
            let text = format!("<?php\nfunction f($translator) {{\n    {call}\n}}\n");
            assert!(run_php(&text).is_empty(), "{call}");
        }

        let text = "<?php\n$translator->trans('k', [], 'nodomain');\n";
        assert!(run_with(&fixture_index(), FORM, "php", text).is_empty());
    }

    #[test]
    fn php_group_ranges_use_utf16_columns_with_multibyte_text() {
        let text = "<?php /* é😀 */ $packages->getUrl('img/missing.png');\n// Packages\n";
        let found = run_php(text);
        assert_eq!(found[0].range.start, Position::new(0, 15 + 20));
        assert_eq!(found[0].range.end.character, 15 + 20 + 15);
    }

    #[test]
    fn php_group_crlf_files_report_correct_lines() {
        let text = php("        $builder->add('name', 'text');").replace('\n', "\r\n");
        let found = run_php(&text);
        assert_eq!(found[0].range, TextRange::new(Position::new(5, 31), Position::new(5, 35)));
    }

    #[test]
    fn php_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut plugin = project(&host);

        host.add_file("src/Form/F.php", &php("        $builder->add('name', 'text');"));
        assert_eq!(codes(&opened(&host, &mut plugin, FORM)), ["form-type-string"]);

        host.add_file("src/Form/F.php", &php("        $builder->add('name', TextType::class);"));
        plugin.handle_events(&[Event::FileSaved(FORM.into())], &host.invocation());
        assert!(host.diagnostics().get(FORM).is_none_or(Vec::is_empty));
    }

    #[test]
    fn php_group_missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        assert!(opened(&host, &mut plugin, "/project/src/Gone.php").is_empty());
    }

    fn run_custom(index: &Index, files: &[(&str, &str)], path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        let files: BTreeMap<String, String> = files.iter().map(|(p, t)| ((*p).to_owned(), (*t).to_owned())).collect();
        let read = |file: &str| files.get(file).cloned();
        let groups = crate::Groups::all_on();
        crate::extra_problems(index, groups, &read, path, language, text)
    }

    fn class_index(class: &str, path: &str) -> Index {
        let mut index = Index { framework_version: Some((7, 0)), ..Index::default() };
        index.classes.insert(class.into(), Entry { detail: None, path: path.into(), at: Position::new(0, 0) });
        index
    }

    #[test]
    fn missing_asset_is_quiet_for_build_and_bundles_output() {
        for call in [
            "asset('build/app.js')",
            "asset('bundles/foo/x.css')",
            "asset('assets/app.js')",
            "asset('_assets/a.js')",
            "asset('unindexed/dir/file.png')",
        ] {
            let text = format!("<?php\nfunction f() {{\n    return {call};\n}}\n");
            assert!(run_php(&text).is_empty(), "{call}");
        }
    }

    #[test]
    fn get_url_on_unrelated_object_is_quiet() {
        let text = "<?php\nfunction f($page) {\n    return $page->getUrl('en');\n}\n";
        assert!(run_php(text).is_empty());
        let text = "<?php\nfunction f($page) {\n    return $page->getUrl('img/missing.png');\n}\n";
        assert!(run_php(text).is_empty());
    }

    #[test]
    fn vendor_bundle_domain_is_quiet() {
        for domain in ["EasyAdminBundle", "SonataAdminBundle", "FOSUserBundle", "KnpPaginatorBundle", "vendor.admin", "Mixed"] {
            let text = format!("<?php\nfunction f($translator) {{\n    $translator->trans('k', [], '{domain}');\n}}\n");
            assert!(run_php(&text).is_empty(), "{domain}");
        }
    }

    #[test]
    fn annotation_type_is_quiet_without_a_64_project() {
        let yaml = "import:\n    resource: ../src/\n    type: annotation\n";
        for version in [None, Some((6, 3)), Some((5, 4))] {
            let index = Index { framework_version: version, ..Index::default() };
            assert!(run_with(&index, ROUTES, "yaml", yaml).is_empty(), "{version:?}");
        }
        let index = Index { framework_version: Some((6, 4)), ..Index::default() };
        assert_eq!(codes(&run_with(&index, ROUTES, "yaml", yaml)), ["deprecated-route-setting"]);

        assert_eq!(crate::index::framework_version(r#"{"require": {"symfony/framework-bundle": "^6.4"}}"#), Some((6, 4)));
        assert_eq!(crate::index::framework_version(r#"{"require": {"symfony/framework-bundle": "7.*"}}"#), Some((7, 0)));
        assert_eq!(crate::index::framework_version(r#"{"require": {"symfony/framework-bundle": "*"}}"#), None);
        assert_eq!(crate::index::framework_version("{}"), None);
    }

    #[test]
    fn project_base_class_named_controller_is_unknown() {
        let base = "<?php\nnamespace App\\Controller;\nuse App\\Base\\Controller;\nclass Child extends Controller\n{\n    public function own() {}\n}\n";
        let index = class_index("App\\Controller\\Child", "/project/Child.php");
        let yaml = "a:\n    controller: App\\Controller\\Child::inheritedFromBase\n";
        assert!(run_custom(&index, &[("/project/Child.php", base)], ROUTES, "yaml", yaml).is_empty());

        let framework = base.replace("App\\Base\\Controller", "Symfony\\Bundle\\FrameworkBundle\\Controller\\AbstractController").replace("extends Controller", "extends AbstractController");
        assert_eq!(codes(&run_custom(&index, &[("/project/Child.php", &framework)], ROUTES, "yaml", yaml)), ["controller-method-missing"]);
    }

    #[test]
    fn static_constructor_service_is_quiet() {
        let yaml = "services:\n    a:\n        class: App\\Mailer\n        constructor: create\n        arguments:\n            $nope: 1\n";
        assert!(run(SERVICES, "yaml", yaml).is_empty());
        let xml = "<container><service id=\"a\" class=\"App\\Mailer\" constructor=\"create\"><argument key=\"$nope\"/></service></container>";
        assert!(run("/project/config/services.xml", "xml", xml).is_empty());
    }

    #[test]
    fn class_file_is_read_once_per_run() {
        let reads = std::cell::Cell::new(0);
        let read = |_: &str| {
            reads.set(reads.get() + 1);
            Some(MAILER.to_owned())
        };
        let index = fixture_index();
        let ctx = Context::new(&index, &read);
        let yaml = "services:\n    a:\n        class: App\\Mailer\n        arguments: { $x: 1 }\n    b:\n        class: App\\Mailer\n        arguments: { $y: 1 }\n";
        assert_eq!(check_services(&ctx, SERVICES, "yaml", yaml).len(), 2);
        assert_eq!(reads.get(), 1);

        let big = |_: &str| Some(format!("{MAILER}{}", " ".repeat(MAX_CLASS_BYTES)));
        let ctx = Context::new(&index, &big);
        assert!(check_services(&ctx, SERVICES, "yaml", yaml).is_empty());
    }

    fn twig_index() -> Index {
        let mut index = php_index();
        index.compiled_routes = true;
        index.routes.insert("blog_show".into(), Entry { detail: None, path: "/project/src/B.php".into(), at: Position::new(0, 0) });
        index
    }

    const TEMPLATE: &str = "/project/templates/a.html.twig";

    fn run_twig(text: &str) -> Vec<Diagnostic> {
        run_with(&twig_index(), TEMPLATE, "twig", text)
    }

    #[test]
    fn twig_missing_route_warns() {
        let found = run_twig("<a href=\"{{ path('nope') }}\">{{ url(\"gone\", {id: 1}) }}</a>");
        assert_eq!(codes(&found), ["twig-missing-route", "twig-missing-route"]);
        assert_eq!(found[0].message, "Route \"nope\" is not defined.");
    }

    #[test]
    fn twig_missing_route_is_quiet_for_library_or_dynamic_name() {
        for call in ["path('blog_show')", "path('_wdt')", "url('_profiler_home')", "path('a_' ~ kind)", "path(name)", "path(\"x#{y}\")"] {
            assert!(run_twig(&format!("{{{{ {call} }}}}")).is_empty(), "{call}");
        }

        let mut index = twig_index();
        index.compiled_routes = false;
        assert!(run_with(&index, TEMPLATE, "twig", "{{ path('nope') }}").is_empty());
        assert!(run_with(&twig_index(), "/project/src/A.php", "php", "<?php // {{ path('nope') }}").is_empty());
    }

    #[test]
    fn twig_missing_asset_warns() {
        let found = run_twig("<img src=\"{{ asset('img/missing.png') }}\"><link href=\"{{ asset('css/app.css?v=1') }}\">{{ asset('css/none.css', 'cdn') }}");
        assert_eq!(codes(&found), ["twig-missing-asset", "twig-missing-asset"]);
        assert_eq!(found[0].message, "Asset \"img/missing.png\" does not exist in the public directory.");
    }

    #[test]
    fn twig_missing_asset_is_quiet_for_library_or_dynamic_name() {
        for call in [
            "asset('img/logo.png')",
            "asset('build/app.js')",
            "asset('bundles/foo/x.css')",
            "asset('assets/app.js')",
            "asset('_assets/a.js')",
            "asset('unindexed/dir/file.png')",
            "asset('https://cdn.example.com/x.js')",
            "asset('//cdn.example.com/x.js')",
            "asset('/already/absolute.png')",
            "asset('data:image/png;base64,AAAA')",
            "asset('img/' ~ name)",
            "asset(path)",
        ] {
            assert!(run_twig(&format!("{{{{ {call} }}}}")).is_empty(), "{call}");
        }

        let mut index = twig_index();
        index.assets.clear();
        assert!(run_with(&index, TEMPLATE, "twig", "{{ asset('img/none.png') }}").is_empty());
    }

    #[test]
    fn twig_group_ranges_use_utf16_columns_with_multibyte_text() {
        let found = run_twig("{# é😀 #}{{ path('nope') }}");
        assert_eq!(found[0].range.start, Position::new(0, 9 + 9));
        assert_eq!(found[0].range.end.character, 9 + 9 + 4);
    }

    #[test]
    fn twig_group_crlf_files_report_correct_lines() {
        let found = run_twig("<p>\r\n{{ path('nope') }}\r\n</p>");
        assert_eq!(found[0].range, TextRange::new(Position::new(1, 9), Position::new(1, 13)));
    }

    #[test]
    fn twig_checks_run_without_the_twig_language_provider() {
        let host = MockHost::install();
        host.add_file("var/cache/dev/url_generating_routes.php", "<?php\n\nreturn [\n    'app_home' => [[], ['_controller' => 'x'], [], []],\n];\n");
        let mut plugin = project(&host);

        let template = "/project/templates/a.html.twig";
        host.add_file("templates/a.html.twig", "{{ path('nope') }}{{ asset('img/missing.png') }}{{ path('app_home') }}");
        assert_eq!(codes(&opened(&host, &mut plugin, template)), ["twig-missing-route", "twig-missing-asset"]);
    }

    #[test]
    fn twig_group_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        host.add_file("var/cache/dev/url_generating_routes.php", "<?php\n\nreturn [\n    'app_home' => [[], ['_controller' => 'x'], [], []],\n];\n");
        let mut plugin = project(&host);

        let template = "/project/templates/a.html.twig";
        host.add_file("templates/a.html.twig", "{% block a %}{{ path('nope') }}{{ include('gone.html.twig') }}");
        assert_eq!(codes(&opened(&host, &mut plugin, template)), ["missing-template", "unclosed-tag", "twig-missing-route"]);

        host.add_file("templates/a.html.twig", "{% block a %}{{ path('app_home') }}{% endblock %}");
        plugin.handle_events(&[Event::FileSaved(template.into())], &host.invocation());
        assert!(host.diagnostics().get(template).is_none_or(Vec::is_empty));
    }

    #[test]
    fn twig_group_missing_file_is_treated_as_absent() {
        let host = MockHost::install();
        let mut plugin = project(&host);
        assert!(opened(&host, &mut plugin, "/project/templates/gone.html.twig").is_empty());
    }
}
