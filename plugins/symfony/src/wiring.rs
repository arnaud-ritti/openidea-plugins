//! Service definitions checked against the project classes they name:
//! constructor argument counts and types, tags that need an interface,
//! event listener methods and `!php/const` constants. Everything stays quiet
//! when a class, a parent or a file is not in the index. Tag requirements are
//! the interfaces the Symfony service-tag documentation names.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::checks::{self, is_service_file, Context, Finding};
use crate::index::{self, scalar_slot, split_flow, without_comment};
use crate::php_text::{class_model, ClassModel};

const TAG_INTERFACES: [(&str, &str); 7] = [
    ("kernel.event_subscriber", "Symfony\\Component\\EventDispatcher\\EventSubscriberInterface"),
    ("twig.extension", "Twig\\Extension\\ExtensionInterface"),
    ("form.type", "Symfony\\Component\\Form\\FormTypeInterface"),
    ("validator.constraint_validator", "Symfony\\Component\\Validator\\ConstraintValidatorInterface"),
    ("security.voter", "Symfony\\Component\\Security\\Core\\Authorization\\Voter\\VoterInterface"),
    ("kernel.cache_warmer", "Symfony\\Component\\HttpKernel\\CacheWarmer\\CacheWarmerInterface"),
    ("doctrine.event_subscriber", "Doctrine\\Common\\EventSubscriber"),
];

/// Core interfaces that no framework interface extends.
const INERT_VENDOR_TYPES: [&str; 7] = ["Countable", "Stringable", "JsonSerializable", "IteratorAggregate", "ArrayAccess", "Traversable", "Iterator"];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Argument {
    pub service: Option<String>,
    pub range: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TagUse {
    pub name: String,
    pub name_range: Range<usize>,
    pub method: Option<(String, Range<usize>)>,
}

#[derive(Debug, Default)]
pub(crate) struct Service {
    pub id: String,
    pub class: Option<String>,
    pub arguments: Option<Vec<Argument>>,
    pub inherits: bool,
    pub custom_construction: bool,
    pub tags: Vec<TagUse>,
}

fn trimmed_quotes(value: &str) -> &str {
    value.trim().trim_matches(['\'', '"'])
}

fn service_reference(raw: &str) -> Option<String> {
    let id = trimmed_quotes(raw).strip_prefix('@')?;
    if id.starts_with(['=', '@']) {
        return None;
    }

    Some(id.trim_start_matches('?').to_owned())
}

fn yaml_argument(item: &str, base: usize) -> Option<Argument> {
    let item = without_comment(item).trim_end();
    let leading = item.len() - item.trim_start().len();
    let item = item.trim_start();
    if item.is_empty() {
        return None;
    }

    let opens = item.chars().filter(|c| matches!(c, '[' | '{')).count();
    let closes = item.chars().filter(|c| matches!(c, ']' | '}')).count();
    let plain = !item.starts_with(['\'', '"', '[', '{', '@', '!', '%']);
    if opens != closes || (plain && index::split_yaml_key(item).is_some()) {
        return None;
    }

    let (start, end) = scalar_slot(item, base + leading);
    Some(Argument { service: service_reference(item), range: start..end })
}

fn yaml_arguments(text: &str, key: &index::YamlKey) -> Option<Vec<Argument>> {
    let line_start = text[..key.offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[key.offset..].find('\n').map_or(text.len(), |i| key.offset + i);
    let line = text[line_start..line_end].trim_end_matches('\r');
    let colon = key.offset - line_start + line[key.offset - line_start..].find(':')?;
    let inline = without_comment(&line[colon + 1..]);

    if !inline.trim().is_empty() {
        let trimmed = inline.trim();
        let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
        let base = line_start + colon + 1 + (inline.len() - inline.trim_start().len()) + 1;
        let parts = split_flow(inner, base);
        if parts.len() == 1 && parts[0].1.trim().is_empty() {
            return Some(Vec::new());
        }

        return parts.into_iter().map(|(offset, part)| yaml_argument(part, offset)).collect();
    }

    let key_indent = line.len() - line.trim_start().len();
    let mut arguments = Vec::new();
    let mut item_indent: Option<usize> = None;
    let mut offset = line_end + 1;

    for next in text[(line_end + 1).min(text.len())..].split_inclusive('\n') {
        let line_offset = offset;
        offset += next.len();
        let next = next.trim_end_matches(['\n', '\r']);
        let trimmed = next.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = next.len() - trimmed.len();
        let is_item = trimmed.starts_with("- ");
        if indent < key_indent || (indent == key_indent && !is_item) {
            break;
        }

        if !is_item || item_indent.is_some_and(|expected| expected != indent) {
            return None;
        }

        item_indent = Some(indent);
        let item = &trimmed[2..];
        arguments.push(yaml_argument(item, line_offset + indent + 2)?);
    }

    Some(arguments)
}

fn yaml_tag_method(text: &str, name_end: usize) -> Option<(String, Range<usize>)> {
    let line_start = text[..name_end].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[name_end..].find('\n').map_or(text.len(), |i| name_end + i);
    let method_in = |from: usize, to: usize| {
        let line = &text[from..to];
        let at = line.find("method:")?;
        let value_start = from + at + "method:".len();
        let value = without_comment(&text[value_start..to]);
        let value = value.split([',', '}']).next().unwrap_or(value);
        let (start, end) = scalar_slot(value, value_start);
        (start < end).then(|| (text[start..end].to_owned(), start..end))
    };

    if let Some(found) = method_in(line_start, line_end) {
        return Some(found);
    }

    let dash_indent = text[line_start..line_end].len() - text[line_start..line_end].trim_start().len();
    let mut offset = line_end + 1;
    for next in text[(line_end + 1).min(text.len())..].split_inclusive('\n') {
        let line_offset = offset;
        offset += next.len();
        let trimmed = next.trim_start();
        let indent = next.len() - trimmed.len();
        if trimmed.trim().is_empty() {
            continue;
        }

        if indent <= dash_indent || trimmed.starts_with('-') {
            return None;
        }

        if let Some(found) = method_in(line_offset, line_offset + next.trim_end().len()) {
            return Some(found);
        }
    }

    None
}

pub(crate) fn yaml_services(text: &str) -> Vec<Service> {
    let keys = index::yaml_keys(text);
    let slots = index::yaml_tag_slots(text);
    let mut services = Vec::new();

    for service in keys.iter().filter(|k| k.parents == ["services"] && !k.key.starts_with('_') && !k.key.ends_with('\\')) {
        let own = |k: &&index::YamlKey| k.parents.len() >= 2 && k.parents[1] == service.key && k.parents[0] == "services";
        let child = |name: &str| keys.iter().filter(own).find(|k| k.parents.len() == 2 && k.key == name);
        let end = keys.iter().find(|k| k.offset > service.offset && k.parents.len() <= 1).map_or(text.len(), |k| k.offset);

        let is_alias = service.value.trim_matches(['\'', '"']).starts_with('@') || child("alias").is_some();
        let class = match child("class") {
            Some(key) => Some(checks::clean_class(trimmed_quotes(without_comment(&key.value)))),
            None if service.key.contains('\\') && !is_alias => Some(checks::clean_class(&service.key)),
            None => None,
        };

        let arguments = child("arguments").and_then(|key| yaml_arguments(text, key));
        let tags = slots
            .iter()
            .filter(|(start, _)| (service.offset..end).contains(start))
            .map(|&(start, stop)| TagUse { name: text[start..stop].trim().to_owned(), name_range: start..stop, method: yaml_tag_method(text, stop) })
            .collect();

        services.push(Service {
            id: service.key.clone(),
            class: class.filter(|class| !class.contains('%')),
            arguments: if child("arguments").is_some() { arguments } else { Some(Vec::new()) },
            inherits: is_alias || child("parent").is_some(),
            custom_construction: child("factory").is_some() || child("constructor").is_some() || child("decorates").is_some(),
            tags,
        });
    }

    services
}

pub(crate) fn xml_services(text: &str) -> Vec<Service> {
    let visible = blank_comments(text);
    let mut services = Vec::new();

    for (tag, at) in index::xml_tags(&visible, "service") {
        let attribute = |name: &str| index::xml_attribute(tag, name).map(|(value, offset)| (value, at + offset));
        let Some((id, _)) = attribute("id") else { continue };
        let body_end = if tag.ends_with('/') { None } else { visible[at..].find("</service>").map(|i| at + i) };
        let body = body_end.map(|end| &visible[at + tag.len()..end]).unwrap_or_default();
        let body_start = at + tag.len();

        let skipped: Vec<Range<usize>> = ["call", "configurator", "factory"]
            .iter()
            .flat_map(|name| checks::xml_spans(&visible, body_start, body_start + body.len(), name))
            .collect();
        let nested = body.contains("type=\"collection\"") || body.contains("type=\"iterator\"") || body.contains("type=\"service_locator\"") || body.contains("type=\"closure\"");

        let mut arguments = Some(Vec::new());
        for (argument, argument_at) in index::xml_tags(body, "argument") {
            let argument_at = body_start + argument_at;
            if skipped.iter().any(|span| span.contains(&argument_at)) {
                continue;
            }

            let is_keyed = index::xml_attribute(argument, "key").is_some() || index::xml_attribute(argument, "index").is_some();
            if nested || is_keyed {
                arguments = None;
                break;
            }

            let reference = index::xml_attribute(argument, "type").filter(|(kind, _)| *kind == "service").and_then(|_| index::xml_attribute(argument, "id"));
            let range = reference.map_or(argument_at..argument_at + argument.len(), |(id, offset)| argument_at + offset..argument_at + offset + id.len());
            if let Some(list) = arguments.as_mut() {
                list.push(Argument { service: reference.map(|(id, _)| id.to_owned()), range });
            }
        }

        let tags = index::xml_tags(body, "tag")
            .into_iter()
            .filter_map(|(tag_text, tag_at)| {
                let tag_at = body_start + tag_at;
                let (name, offset) = index::xml_attribute(tag_text, "name")?;
                let method = index::xml_attribute(tag_text, "method").map(|(value, offset)| (value.to_owned(), tag_at + offset..tag_at + offset + value.len()));
                Some(TagUse { name: name.to_owned(), name_range: tag_at + offset..tag_at + offset + name.len(), method })
            })
            .collect();

        let class = attribute("class").map(|(class, _)| checks::clean_class(class)).or_else(|| id.contains('\\').then(|| checks::clean_class(id)));
        services.push(Service {
            id: id.to_owned(),
            class: class.filter(|class| !class.contains('%')),
            arguments,
            inherits: attribute("parent").is_some() || attribute("alias").is_some(),
            custom_construction: attribute("constructor").is_some() || attribute("decorates").is_some() || body.contains("<factory"),
            tags,
        });
    }

    services
}

fn blank_comments(text: &str) -> String {
    let mut visible = text.to_owned();
    let mut from = 0;

    while let Some(open) = text[from..].find("<!--") {
        let start = from + open;
        let end = text[start..].find("-->").map_or(text.len(), |close| start + close + 3);
        visible.replace_range(start..end, &text[start..end].chars().map(|c| if c == '\n' || c == '\r' { c } else { ' ' }).collect::<String>());
        from = end;
    }

    visible
}

pub(crate) struct Lineage {
    pub ancestors: BTreeSet<String>,
    pub outside: BTreeSet<String>,
}

pub(crate) fn model_of(ctx: &Context, class: &str) -> Option<ClassModel> {
    let entry = ctx.index.classes.get(class)?;
    class_model(&ctx.source(&entry.path)?)
}

/// Every parent and interface of a project class; `None` when any project
/// link cannot be read. Vendor links are kept apart: they cannot lead back to project types.
pub(crate) fn lineage(ctx: &Context, class: &str) -> Option<Lineage> {
    let mut lineage = Lineage { ancestors: BTreeSet::new(), outside: BTreeSet::new() };
    let mut queue = vec![class.to_owned()];
    let mut visited = BTreeSet::new();

    while let Some(next) = queue.pop() {
        if !visited.insert(next.clone()) || visited.len() > 60 {
            continue;
        }

        let model = model_of(ctx, &next)?;
        for ancestor in model.parent.iter().chain(model.interfaces.iter()) {
            lineage.ancestors.insert(ancestor.clone());
            match ctx.index.classes.contains_key(ancestor) {
                true => queue.push(ancestor.clone()),
                false => {
                    lineage.outside.insert(ancestor.clone());
                }
            }
        }
    }

    Some(lineage)
}

/// Whether `class` or a project ancestor declares `method`; `None` when it may inherit it from outside or through traits and `__call`.
fn declares_method(ctx: &Context, class: &str, method: &str) -> Option<bool> {
    let mut queue = vec![class.to_owned()];
    let mut visited = BTreeSet::new();

    while let Some(next) = queue.pop() {
        if !visited.insert(next.clone()) || visited.len() > 60 {
            return None;
        }

        let model = model_of(ctx, &next)?;
        if model.method(method).is_some() {
            return Some(true);
        }

        if model.has_traits || model.method("__call").is_some() {
            return None;
        }

        for parent in model.parent.iter() {
            if !ctx.index.classes.contains_key(parent) {
                return None;
            }
            queue.push(parent.clone());
        }
    }

    Some(false)
}

fn class_of_reference(ctx: &Context, own: &[Service], id: &str) -> Option<String> {
    if let Some(defined) = own.iter().find(|service| service.id == id) {
        let usable = !defined.inherits && !defined.custom_construction;
        return defined.class.clone().filter(|_| usable);
    }

    match ctx.index.services.get(id).and_then(|entry| entry.detail.as_ref()) {
        Some(class) => Some(class.clone()).filter(|class| !class.starts_with("alias of") && !class.contains('%')),
        None => (id.contains('\\') && ctx.index.classes.contains_key(id)).then(|| id.to_owned()),
    }
}

fn check_arguments(ctx: &Context, services: &[Service], service: &Service, out: &mut Vec<Finding>) {
    let (Some(class), Some(arguments)) = (&service.class, &service.arguments) else { return };
    if service.inherits || service.custom_construction || arguments.is_empty() || !ctx.index.classes.contains_key(class) {
        return;
    }

    let Some(model) = model_of(ctx, class) else { return };
    let Some(constructor) = model.method("__construct") else { return };
    let variadic = constructor.params.iter().any(|param| param.variadic);

    if !variadic && arguments.len() > constructor.params.len() {
        let surplus = &arguments[constructor.params.len()];
        let message = format!("Constructor of \"{class}\" takes {} argument(s), but {} are configured.", constructor.params.len(), arguments.len());
        out.push(Finding::warn(surplus.range.clone(), "service-argument-count", message));
    }

    for (argument, param) in arguments.iter().zip(&constructor.params) {
        let (Some(id), Some(expected)) = (&argument.service, &param.type_name) else { continue };
        if param.variadic || !ctx.index.classes.contains_key(expected) {
            continue;
        }

        let Some(actual) = class_of_reference(ctx, services, id) else { continue };
        if &actual == expected || !ctx.index.classes.contains_key(&actual) {
            continue;
        }

        let Some(lineage) = lineage(ctx, &actual) else { continue };
        if !lineage.ancestors.contains(expected) {
            let message = format!("Argument ${} of \"{class}\" is typed \"{expected}\", but service \"{id}\" is a \"{actual}\".", param.name);
            out.push(Finding::warn(argument.range.clone(), "service-argument-type", message));
        }
    }
}

fn check_tags(ctx: &Context, service: &Service, out: &mut Vec<Finding>) {
    let Some(class) = service.class.as_ref().filter(|class| !service.inherits && ctx.index.classes.contains_key(*class)) else { return };

    for tag in &service.tags {
        if let Some((_, required)) = TAG_INTERFACES.iter().find(|(name, _)| *name == tag.name) {
            let lacks = lineage(ctx, class).is_some_and(|lineage| {
                let opaque = lineage.outside.iter().any(|outside| !INERT_VENDOR_TYPES.contains(&outside.as_str()));
                !opaque && !lineage.ancestors.contains(*required)
            });
            if lacks {
                let message = format!("\"{class}\" is tagged \"{}\" but does not implement {required}.", tag.name);
                out.push(Finding::warn(tag.name_range.clone(), "service-tag-interface", message));
            }
        }

        let Some((method, range)) = tag.method.as_ref().filter(|_| tag.name == "kernel.event_listener") else { continue };
        if checks::is_dynamic(method) {
            continue;
        }

        if declares_method(ctx, class, method) == Some(false) {
            out.push(Finding::warn(range.clone(), "listener-method-missing", format!("Class \"{class}\" has no method \"{method}\".")));
        }
    }
}

pub fn check(ctx: &Context, path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if ctx.index.partial || ctx.index.classes.is_empty() || !is_service_file(path) {
        return out;
    }

    let services = match language {
        "yaml" => yaml_services(text),
        "xml" if text.contains("<container") => xml_services(text),
        _ => return out,
    };

    for service in &services {
        check_arguments(ctx, &services, service, &mut out);
        check_tags(ctx, service, &mut out);
    }

    out.sort_by_key(|finding| finding.start);
    out
}

/// `!php/const App\Enum\Suit::Hearts`: a project class without that constant or case.
pub fn php_constants(ctx: &Context, path: &str, language: &str, text: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    if language != "yaml" || ctx.index.partial || !path.contains("/config/") || path.contains("/var/cache/") {
        return out;
    }

    for (at, _) in text.match_indices("!php/const") {
        let rest = &text[at + "!php/const".len()..];
        let skipped = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let start = at + "!php/const".len() + skipped;
        let token: String = text[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '\\' | ':')).collect();
        let Some((class, constant)) = token.split_once("::") else { continue };
        let class = class.trim_start_matches('\\');
        if constant.is_empty() || constant == "class" || !ctx.index.classes.contains_key(class) {
            continue;
        }

        let Some(model) = model_of(ctx, class) else { continue };
        let Some(source) = ctx.index.classes.get(class).and_then(|entry| ctx.source(&entry.path)) else { continue };
        let closed = model.parent.is_none() && model.interfaces.is_empty() && !model.has_traits;
        let declared = [format!("const {constant}"), format!("case {constant}")].iter().any(|needle| {
            source.match_indices(needle.as_str()).any(|(found, _)| !source.as_bytes().get(found + needle.len()).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_'))
        });

        if closed && !declared {
            let from = start + class.len() + 2 + usize::from(token.starts_with('\\'));
            out.push(Finding::warn(from..from + constant.len(), "yaml-php-const-missing", format!("\"{class}\" has no constant \"{constant}\".")));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Diagnostic, Event, Plugin, Position};

    use crate::index::{Entry, Index};
    use crate::Main;

    const SERVICES: &str = "/project/config/services.yaml";

    const FILES: [(&str, &str, &str); 8] = [
        ("App\\Mailer", "/project/src/Mailer.php", "<?php\nnamespace App;\n\nclass Mailer\n{\n    public function __construct(private Transport $transport, private ?Logger $logger = null, private string $dsn = '')\n    {\n    }\n}\n"),
        ("App\\Transport", "/project/src/Transport.php", "<?php\nnamespace App;\n\ninterface Transport extends BaseTransport\n{\n}\n"),
        ("App\\BaseTransport", "/project/src/BaseTransport.php", "<?php\nnamespace App;\n\ninterface BaseTransport\n{\n}\n"),
        ("App\\SmtpTransport", "/project/src/SmtpTransport.php", "<?php\nnamespace App;\n\nclass SmtpTransport extends AbstractTransport implements \\Countable\n{\n}\n"),
        ("App\\AbstractTransport", "/project/src/AbstractTransport.php", "<?php\nnamespace App;\n\nabstract class AbstractTransport implements Transport\n{\n}\n"),
        ("App\\Logger", "/project/src/Logger.php", "<?php\nnamespace App;\n\nclass Logger\n{\n}\n"),
        ("App\\Subscriber", "/project/src/Subscriber.php", "<?php\nnamespace App;\n\nclass Subscriber\n{\n    public function onRequest($event)\n    {\n    }\n}\n"),
        ("App\\Vendored", "/project/src/Vendored.php", "<?php\nnamespace App;\n\nuse Vendor\\Base;\n\nclass Vendored extends Base\n{\n}\n"),
    ];

    fn index() -> Index {
        let mut index = Index { framework_version: Some((7, 0)), ..Index::default() };
        index.psr4_prefixes.push("App\\".into());
        for (class, path, _) in FILES {
            index.classes.insert(class.into(), Entry { detail: None, path: path.into(), at: Position::new(0, 0) });
            index.services.insert(class.into(), Entry { detail: None, path: SERVICES.into(), at: Position::new(0, 0) });
        }
        index
    }

    fn run(path: &str, language: &str, text: &str) -> Vec<Diagnostic> {
        let files: BTreeMap<String, String> = FILES.iter().map(|(_, path, source)| ((*path).to_owned(), (*source).to_owned())).collect();
        let read = |file: &str| files.get(file).cloned();
        crate::extra_problems(&index(), crate::Groups::all_on(), &read, path, language, text)
    }

    fn only(found: &[Diagnostic], code: &str) -> Vec<Diagnostic> {
        found.iter().filter(|d| d.code.as_deref() == Some(code)).cloned().collect()
    }

    fn word<'a>(text: &'a str, d: &Diagnostic) -> &'a str {
        let line = text.lines().nth(d.range.start.line as usize).unwrap();
        &line[d.range.start.character as usize..d.range.end.character as usize]
    }

    #[test]
    fn too_many_arguments_are_reported_on_the_first_surplus_one() {
        let text = "services:\n    App\\Mailer:\n        arguments:\n            - '@App\\SmtpTransport'\n            - '@App\\Logger'\n            - 'dsn'\n            - 'extra'\n            - 'more'\n";
        let found = only(&run(SERVICES, "yaml", text), "service-argument-count");
        assert_eq!(found.len(), 1);
        assert_eq!(word(text, &found[0]), "extra");
        assert_eq!(found[0].message, "Constructor of \"App\\Mailer\" takes 3 argument(s), but 5 are configured.");

        let flow = "services:\n    App\\Mailer:\n        arguments: ['@App\\SmtpTransport', '@App\\Logger', 'dsn', 'extra']\n";
        let found = only(&run(SERVICES, "yaml", flow), "service-argument-count");
        assert_eq!(word(flow, &found[0]), "extra");
    }

    #[test]
    fn argument_counts_stay_quiet_for_named_variadic_inherited_and_custom_construction() {
        for text in [
            "services:\n    App\\Mailer:\n        arguments:\n            $dsn: x\n",
            "services:\n    App\\Mailer:\n        arguments: { $dsn: x, $a: 1, $b: 2, $c: 3 }\n",
            "services:\n    app.m:\n        class: App\\Mailer\n        parent: app.base\n        arguments: [1, 2, 3, 4]\n",
            "services:\n    app.m:\n        class: App\\Mailer\n        factory: ['@f', create]\n        arguments: [1, 2, 3, 4]\n",
            "services:\n    app.m:\n        class: Vendor\\Mailer\n        arguments: [1, 2, 3, 4]\n",
            "services:\n    app.m:\n        class: App\\Logger\n        arguments: [1, 2]\n",
            "services:\n    app.m:\n        class: App\\Mailer\n        arguments:\n            - [a, b,\n              c]\n            - 2\n            - 3\n            - 4\n",
        ] {
            assert!(only(&run(SERVICES, "yaml", text), "service-argument-count").is_empty(), "{text}");
        }
    }

    #[test]
    fn service_argument_type_checks_the_project_hierarchy() {
        let text = "services:\n    App\\Mailer:\n        arguments:\n            - '@App\\Logger'\n            - '@App\\SmtpTransport'\n    app.smtp:\n        class: App\\SmtpTransport\n";
        let found = only(&run(SERVICES, "yaml", text), "service-argument-type");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].message, "Argument $transport of \"App\\Mailer\" is typed \"App\\Transport\", but service \"App\\Logger\" is a \"App\\Logger\".");
        assert_eq!(word(text, &found[0]), "@App\\Logger");
    }

    #[test]
    fn service_argument_type_accepts_interfaces_through_parents_and_stays_quiet_when_unsure() {
        let ok = "services:\n    App\\Mailer:\n        arguments: ['@app.smtp', '@?App\\Logger', 'dsn']\n    app.smtp:\n        class: App\\SmtpTransport\n";
        assert!(only(&run(SERVICES, "yaml", ok), "service-argument-type").is_empty());

        for text in [
            "services:\n    App\\Mailer:\n        arguments: ['@unknown.service']\n",
            "services:\n    App\\Mailer:\n        arguments: ['@app.alias']\n    app.alias:\n        alias: App\\Logger\n",
            "services:\n    App\\Mailer:\n        arguments: ['@app.deco']\n    app.deco:\n        class: App\\Logger\n        decorates: x\n",
            "services:\n    App\\Mailer:\n        arguments: ['@=service(\"x\")', 'plain']\n",
            "services:\n    App\\Mailer:\n        arguments: ['@vendor.service']\n    vendor.service:\n        class: Vendor\\Thing\n",
        ] {
            let found = only(&run(SERVICES, "yaml", text), "service-argument-type");
            assert!(found.is_empty(), "{text}");
        }
    }

    #[test]
    fn xml_arguments_are_counted_and_typed() {
        let xml = "<container>\n<services>\n<service id=\"m\" class=\"App\\Mailer\">\n<argument type=\"service\" id=\"s\"/>\n<argument type=\"service\" id=\"s\"/>\n<argument>x</argument>\n<argument>y</argument>\n<call method=\"x\"><argument>1</argument></call>\n</service>\n<service id=\"l\" class=\"App\\Logger\"/>\n<service id=\"s\" class=\"App\\SmtpTransport\"/>\n</services>\n</container>";
        let found = run("/project/config/services.xml", "xml", xml);
        let codes: Vec<_> = found.iter().filter_map(|d| d.code.as_deref()).collect();
        assert_eq!(codes, ["service-argument-type", "service-argument-count"]);
        assert_eq!(word(xml, &found[0]), "s");
    }

    #[test]
    fn xml_collection_and_keyed_arguments_are_skipped() {
        let xml = "<container><services><service id=\"m\" class=\"App\\Mailer\"><argument type=\"collection\"><argument>1</argument><argument>2</argument></argument><argument key=\"$dsn\">1</argument><argument>3</argument><argument>4</argument></service></services></container>";
        let found = run("/project/config/services.xml", "xml", xml);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn tags_need_their_interface() {
        let text = "services:\n    App\\Subscriber:\n        tags: ['kernel.event_subscriber']\n    App\\SmtpTransport:\n        tags:\n            - { name: security.voter }\n    app.vendored:\n        class: App\\Vendored\n        tags: ['security.voter']\n    App\\Logger:\n        tags: ['monolog.logger']\n";
        let found = only(&run(SERVICES, "yaml", text), "service-tag-interface");
        assert_eq!(found.len(), 2);
        assert_eq!(word(text, &found[0]), "kernel.event_subscriber");
        assert_eq!(found[1].message, "\"App\\SmtpTransport\" is tagged \"security.voter\" but does not implement Symfony\\Component\\Security\\Core\\Authorization\\Voter\\VoterInterface.");
    }

    #[test]
    fn listener_method_must_exist() {
        let text = "services:\n    App\\Subscriber:\n        tags:\n            - { name: kernel.event_listener, event: kernel.request, method: onRequest }\n            - { name: kernel.event_listener, method: onGone, event: x }\n    app.v:\n        class: App\\Vendored\n        tags: [{ name: kernel.event_listener, method: whatever }]\n    app.b:\n        class: App\\Subscriber\n        tags:\n            - name: kernel.event_listener\n              event: e\n              method: missing\n";
        let found = only(&run(SERVICES, "yaml", text), "listener-method-missing");
        let words: Vec<_> = found.iter().map(|d| word(text, d)).collect();
        assert_eq!(words, ["onGone", "missing"]);

        let xml = "<container><services><service id=\"s\" class=\"App\\Subscriber\"><tag name=\"kernel.event_listener\" event=\"e\" method=\"nope\"/><tag name=\"kernel.event_listener\" method=\"onRequest\"/></service></services></container>";
        let found = run("/project/config/services.xml", "xml", xml);
        assert_eq!(only(&found, "listener-method-missing").len(), 1);
    }

    #[test]
    fn php_const_must_exist_on_closed_project_classes() {
        let mut idx = index();
        idx.classes.insert("App\\Suit".into(), Entry { detail: None, path: "/project/src/Suit.php".into(), at: Position::new(0, 0) });
        idx.classes.insert("App\\Child".into(), Entry { detail: None, path: "/project/src/Child.php".into(), at: Position::new(0, 0) });
        let files: BTreeMap<String, String> = [
            ("/project/src/Suit.php", "<?php\nnamespace App;\n\nenum Suit: string\n{\n    case Hearts = 'h';\n    const Default = self::Hearts;\n}\n"),
            ("/project/src/Child.php", "<?php\nnamespace App;\n\nclass Child extends Base\n{\n}\n"),
        ]
        .iter()
        .map(|(path, source)| ((*path).to_owned(), (*source).to_owned()))
        .collect();
        let read = |file: &str| files.get(file).cloned();
        let text = "parameters:\n    a: !php/const App\\Suit::Hearts\n    b: !php/const App\\Suit::Spades\n    c: !php/const App\\Suit::Default\n    d: !php/const App\\Child::ANYTHING\n    e: !php/const PHP_INT_MAX\n    f: !php/const Vendor\\X::Y\n    g: !php/const App\\Suit::class\n";
        let found = crate::extra_problems(&idx, crate::Groups::all_on(), &read, "/project/config/packages/app.yaml", "yaml", text);
        let found = only(&found, "yaml-php-const-missing");
        assert_eq!(found.len(), 1);
        assert_eq!(word(text, &found[0]), "Spades");
    }

    #[test]
    fn wiring_checks_are_published_and_silent_while_the_index_is_partial() {
        let host = MockHost::install();
        host.add_file("composer.json", r#"{"require": {"symfony/framework-bundle": "^7.0"}, "autoload": {"psr-4": {"App\\": "src/"}}}"#);
        for (_, path, source) in FILES {
            host.add_file(path.trim_start_matches("/project/"), source);
        }
        host.add_file("config/services.yaml", "services:\n    App\\Mailer:\n        arguments: ['@App\\SmtpTransport', '@App\\SmtpTransport', 'd', 'e']\n");
        let mut plugin = Main::default();
        plugin.activate(&host.context("aritti.symfony")).unwrap();
        plugin.handle_events(&[Event::FileOpened(SERVICES.into())], &host.invocation());

        let codes: Vec<_> = host.diagnostics()[SERVICES].iter().filter_map(|d| d.code.clone()).collect();
        assert_eq!(codes, ["service-argument-type", "service-argument-count"]);
    }
}
