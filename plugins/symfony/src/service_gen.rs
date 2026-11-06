//! Service definitions written from a PHP class: a new entry in an existing
//! `services.yaml` or XML file, constructor arguments kept in sync, and the
//! tags a class implies. Tag and argument rules follow the Symfony service
//! container documentation; nothing here creates a file.

use std::ops::Range;

use crate::checks::Context;
use crate::index::{self, YamlKey};
use crate::intentions::{eol_of, line_start};
use crate::php_text::{ClassModel, Param, BUILT_IN_TYPES};
use crate::wiring::{lineage, model_of};

type Edit = (Range<usize>, String);

const IMPLIED_TAGS: [(&str, &str); 12] = [
    ("Symfony\\Component\\EventDispatcher\\EventSubscriberInterface", "kernel.event_subscriber"),
    ("Symfony\\Component\\Security\\Core\\Authorization\\Voter\\VoterInterface", "security.voter"),
    ("Symfony\\Component\\Security\\Core\\Authorization\\Voter\\Voter", "security.voter"),
    ("Symfony\\Component\\Validator\\ConstraintValidatorInterface", "validator.constraint_validator"),
    ("Symfony\\Component\\Validator\\ConstraintValidator", "validator.constraint_validator"),
    ("Twig\\Extension\\ExtensionInterface", "twig.extension"),
    ("Twig\\Extension\\AbstractExtension", "twig.extension"),
    ("Symfony\\Component\\Form\\FormTypeInterface", "form.type"),
    ("Symfony\\Component\\Form\\AbstractType", "form.type"),
    ("Symfony\\Component\\HttpKernel\\CacheWarmer\\CacheWarmerInterface", "kernel.cache_warmer"),
    ("Symfony\\Component\\Console\\Command\\Command", "console.command"),
    ("Doctrine\\Common\\EventSubscriber", "doctrine.event_subscriber"),
];

enum Value {
    Service(String),
    Scalar { yaml: &'static str, xml_type: &'static str, xml_text: &'static str },
}

pub(crate) struct Argument {
    name: String,
    value: Value,
}

pub(crate) struct Plan {
    pub fqcn: String,
    pub arguments: Vec<Argument>,
    pub tags: Vec<&'static str>,
}

fn value_of(param: &Param) -> Option<Value> {
    if param.variadic {
        return None;
    }

    let Some(type_name) = param.type_name.as_deref() else {
        return Some(Value::Scalar { yaml: "~", xml_type: "expression", xml_text: "null" });
    };

    if !BUILT_IN_TYPES.contains(&type_name) {
        return Some(Value::Service(type_name.to_owned()));
    }

    Some(match type_name {
        "string" => Value::Scalar { yaml: "''", xml_type: "string", xml_text: "" },
        "int" => Value::Scalar { yaml: "0", xml_type: "int", xml_text: "0" },
        "float" => Value::Scalar { yaml: "0.0", xml_type: "float", xml_text: "0.0" },
        "bool" => Value::Scalar { yaml: "false", xml_type: "bool", xml_text: "false" },
        "array" | "iterable" => Value::Scalar { yaml: "[]", xml_type: "collection", xml_text: "" },
        _ => Value::Scalar { yaml: "~", xml_type: "expression", xml_text: "null" },
    })
}

/// Tags the class's known ancestors imply (project classes are followed;
/// a class whose ancestry cannot be read implies none).
pub(crate) fn implied_tags(ctx: &Context, model: &ClassModel) -> Vec<&'static str> {
    let Some(lineage) = lineage(ctx, &model.fqcn) else { return Vec::new() };
    let mut tags: Vec<&'static str> = Vec::new();

    for (ancestor, tag) in IMPLIED_TAGS {
        if lineage.ancestors.contains(ancestor) && !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    tags
}

/// The type's own id when it is a service; else the id of the only service whose class is the type.
fn suggested_service(ctx: &Context, type_name: &str) -> String {
    if ctx.index.services.contains_key(type_name) {
        return type_name.to_owned();
    }

    let mut candidates = ctx.index.services.iter().filter(|(_, entry)| entry.detail.as_deref() == Some(type_name)).map(|(id, _)| id);
    match (candidates.next(), candidates.next()) {
        (Some(only), None) => only.clone(),
        _ => type_name.to_owned(),
    }
}

pub(crate) fn plan(ctx: &Context, model: &ClassModel, existing: &[String], with_tags: bool) -> Plan {
    let constructor = model.method("__construct");
    let arguments = constructor
        .map(|method| method.params.iter())
        .into_iter()
        .flatten()
        .filter(|param| !param.has_default || existing.contains(&param.name))
        .filter_map(|param| Some(Argument { name: param.name.clone(), value: value_of(param)? }))
        .map(|argument| match argument.value {
            Value::Service(type_name) => Argument { value: Value::Service(suggested_service(ctx, &type_name)), ..argument },
            _ => argument,
        })
        .collect();

    Plan { fqcn: model.fqcn.clone(), arguments, tags: if with_tags { implied_tags(ctx, model) } else { Vec::new() } }
}

pub(crate) fn has_autoconfigure(text: &str) -> bool {
    text.lines().any(|line| line.trim().replace(' ', "") == "autoconfigure:true" || line.contains("autoconfigure=\"true\""))
}

fn yaml_value(argument: &Argument) -> String {
    match &argument.value {
        Value::Service(id) => format!("'@{id}'"),
        Value::Scalar { yaml, .. } => (*yaml).to_owned(),
    }
}

fn tag_list(tags: &[&str]) -> String {
    format!("[{}]", tags.iter().map(|tag| format!("'{tag}'")).collect::<Vec<_>>().join(", "))
}

/// The indentation of the first service of a YAML file, and its step.
fn yaml_indent(text: &str) -> (String, String) {
    let keys = index::yaml_keys(text);
    let first = keys.iter().find(|key| key.parents == ["services"]);
    let width = first.map_or(4, |key| {
        let line = line_start(text, key.offset);
        text[line..key.offset].len() - usize::from(text[line..key.offset].ends_with(['\'', '"']))
    });
    let unit = keys.iter().find(|key| key.parents.len() == 2 && key.parents[0] == "services").map_or(width, |child| {
        let line = line_start(text, child.offset);
        (text[line..child.offset].len() - usize::from(text[line..child.offset].ends_with(['\'', '"']))).saturating_sub(width)
    });

    (" ".repeat(width.max(1)), " ".repeat(unit.max(1)))
}

fn yaml_block(plan: &Plan, indent: &str, unit: &str, eol: &str) -> String {
    let mut block = format!("{indent}{}:{eol}", plan.fqcn);
    if !plan.arguments.is_empty() {
        block.push_str(&format!("{indent}{unit}arguments:{eol}"));
        for argument in &plan.arguments {
            block.push_str(&format!("{indent}{unit}{unit}${}: {}{eol}", argument.name, yaml_value(argument)));
        }
    }

    if !plan.tags.is_empty() {
        block.push_str(&format!("{indent}{unit}tags: {}{eol}", tag_list(&plan.tags)));
    }

    if plan.arguments.is_empty() && plan.tags.is_empty() {
        block = format!("{indent}{}: ~{eol}", plan.fqcn);
    }

    block
}

/// Whether an autoloaded prototype (`App\:` with `resource:`) already registers the class.
pub(crate) fn registered_by_prototype(text: &str, fqcn: &str) -> bool {
    index::yaml_keys(text).iter().any(|key| key.parents == ["services"] && key.key.ends_with('\\') && fqcn.starts_with(&key.key))
        || text.lines().any(|line| {
            let namespace = line.split("namespace=\"").nth(1).and_then(|rest| rest.split('"').next());
            namespace.is_some_and(|namespace| fqcn.starts_with(namespace) && line.contains("<prototype"))
        })
}

pub(crate) fn yaml_insertion(text: &str, plan: &Plan) -> Edit {
    let eol = eol_of(text);
    let keys = index::yaml_keys(text);
    let (indent, unit) = yaml_indent(text);
    let block = yaml_block(plan, &indent, &unit, eol);

    let Some(services) = keys.iter().find(|key| key.parents.is_empty() && key.key == "services") else {
        let separator = if text.is_empty() || text.ends_with('\n') { "" } else { eol };
        return (text.len()..text.len(), format!("{separator}{eol}services:{eol}{block}"));
    };

    let end = keys.iter().find(|key| key.offset > services.offset && key.parents.is_empty()).map_or(text.len(), |key| line_start(text, key.offset));
    let content_end = last_content_end(text, services.offset, end);
    let separator = if text[..content_end].ends_with('\n') { "" } else { eol };
    (content_end..content_end, format!("{separator}{eol}{block}"))
}

/// Offset after the last non-blank line in `from..end` (after its line break when it has one).
fn last_content_end(text: &str, from: usize, end: usize) -> usize {
    let region = text[from..end].trim_end_matches(['\n', '\r', ' ', '\t']);
    let stop = from + region.len();
    text[stop..end].find('\n').map_or(end, |found| stop + found + 1)
}

pub(crate) fn xml_insertion(text: &str, plan: &Plan) -> Option<Edit> {
    let eol = eol_of(text);
    let close = text.find("</services>")?;
    let at = line_start(text, close);
    let unit = if text[..close].contains("\n\t") { "\t" } else { "    " };
    let service_indent = index::xml_tags(text, "service").first().map_or(unit.repeat(2), |(_, tag_at)| {
        let line = line_start(text, *tag_at);
        text[line..*tag_at].to_owned()
    });
    let inner = format!("{service_indent}{unit}");

    let childless = plan.arguments.is_empty() && plan.tags.is_empty();
    let block = match childless {
        true => format!("{service_indent}<service id=\"{}\"/>{eol}", plan.fqcn),
        false => format!("{service_indent}<service id=\"{}\">{eol}{}{service_indent}</service>{eol}", plan.fqcn, xml_children(plan, &inner, eol)),
    };

    Some((at..at, block))
}

fn xml_children(plan: &Plan, inner: &str, eol: &str) -> String {
    let mut block = String::new();
    for argument in &plan.arguments {
        block.push_str(&match &argument.value {
            Value::Service(id) => format!("{inner}<argument key=\"${}\" type=\"service\" id=\"{id}\"/>{eol}", argument.name),
            Value::Scalar { xml_type: "collection", .. } => format!("{inner}<argument key=\"${}\" type=\"collection\"/>{eol}", argument.name),
            Value::Scalar { xml_type, xml_text, .. } => format!("{inner}<argument key=\"${}\" type=\"{xml_type}\">{xml_text}</argument>{eol}", argument.name),
        });
    }
    for tag in &plan.tags {
        block.push_str(&format!("{inner}<tag name=\"{tag}\"/>{eol}"));
    }
    block
}

struct XmlService {
    class: Option<String>,
    own_indent: String,
    inner_indent: String,
    has_tags: bool,
    has_arguments: bool,
    self_closing: Option<Range<usize>>,
    close_line: usize,
}

fn xml_service_at(text: &str, caret: usize) -> Option<XmlService> {
    for (tag, at) in index::xml_tags(text, "service") {
        let self_closing = tag.ends_with('/');
        let end = match self_closing {
            true => at + tag.len() + 1,
            false => text[at..].find("</service>").map(|found| at + found + "</service>".len())?,
        };
        if !(at..end).contains(&caret) {
            continue;
        }

        let class = index::xml_attribute(tag, "class").map(|(class, _)| class.to_owned()).or_else(|| index::xml_attribute(tag, "id").map(|(id, _)| id.to_owned()));
        let body = &text[at + tag.len()..end];
        let unit = if text[..at].contains("\n\t") { "\t" } else { "    " };
        let own = crate::intentions::indent_at(text, at).to_owned();
        let close_at = end - "</service>".len().min(end);
        if !self_closing && !text[line_start(text, close_at)..close_at].trim().is_empty() {
            return None;
        }

        return Some(XmlService {
            class,
            inner_indent: format!("{own}{unit}"),
            own_indent: own,
            has_tags: body.contains("<tag"),
            has_arguments: body.contains("<argument"),
            self_closing: self_closing.then(|| at + tag.len() - 1..at + tag.len() + 1),
            close_line: line_start(text, close_at),
        });
    }

    None
}

fn xml_children_edit(text: &str, service: &XmlService, children: &str) -> Edit {
    let eol = eol_of(text);
    match &service.self_closing {
        Some(range) => {
            (range.clone(), format!(">{eol}{children}{}</service>", service.own_indent))
        }
        None => (service.close_line..service.close_line, children.to_owned()),
    }
}

/// `<tag name="…"/>` children for an XML service whose class implies tags and that has none.
pub(crate) fn add_implied_tags_xml(ctx: &Context, text: &str, caret: usize) -> Option<(Vec<&'static str>, Edit)> {
    if has_autoconfigure(text) {
        return None;
    }

    let service = xml_service_at(text, caret)?;
    let class = service.class.clone().filter(|class| ctx.index.classes.contains_key(class))?;
    if service.has_tags {
        return None;
    }

    let tags = implied_tags(ctx, &model_of(ctx, &class)?);
    if tags.is_empty() {
        return None;
    }

    let plan = Plan { fqcn: class, arguments: Vec::new(), tags: tags.clone() };
    let children = xml_children(&plan, &service.inner_indent, eol_of(text));
    Some((tags, xml_children_edit(text, &service, &children)))
}

/// Named `<argument>` children for an XML service that has none, from the constructor.
pub(crate) fn add_arguments_xml(ctx: &Context, text: &str, caret: usize) -> Option<Edit> {
    let service = xml_service_at(text, caret)?;
    let class = service.class.clone().filter(|class| ctx.index.classes.contains_key(class))?;
    if service.has_arguments {
        return None;
    }

    let plan = plan(ctx, &model_of(ctx, &class)?, &[], false);
    if plan.arguments.is_empty() {
        return None;
    }

    let children = xml_children(&plan, &service.inner_indent, eol_of(text));
    Some(xml_children_edit(text, &service, &children))
}

struct YamlService {
    class: Option<String>,
    child_indent: String,
    arguments: Option<YamlKey>,
    has_tags: bool,
    content_end: usize,
}

fn yaml_service_at(text: &str, caret: usize) -> Option<YamlService> {
    let keys = index::yaml_keys(text);
    let service = keys.iter().filter(|key| key.parents == ["services"] && !key.key.starts_with('_') && !key.key.ends_with('\\')).filter(|key| {
        let start = line_start(text, key.offset);
        let end = keys.iter().find(|next| next.offset > key.offset && next.parents.len() <= 1).map_or(text.len(), |next| line_start(text, next.offset));
        start <= caret && caret <= end
    }).last()?;

    let end = keys.iter().find(|next| next.offset > service.offset && next.parents.len() <= 1).map_or(text.len(), |next| line_start(text, next.offset));
    let own = |key: &&YamlKey| key.parents.len() >= 2 && key.parents[0] == "services" && key.parents[1] == service.key;
    let child = |name: &str| keys.iter().filter(own).find(|key| key.parents.len() == 2 && key.key == name);

    let class = match child("class") {
        Some(key) => Some(crate::checks::clean_class(key.value.trim().trim_matches(['\'', '"']))),
        None if service.key.contains('\\') && !service.value.starts_with('@') && child("alias").is_none() => Some(crate::checks::clean_class(&service.key)),
        None => None,
    };

    let first_child = keys.iter().find(|key| own(&key));
    let child_indent = first_child.map_or_else(
        || " ".repeat(yaml_indent(text).0.len() + yaml_indent(text).1.len()),
        |key| {
            let line = line_start(text, key.offset);
            " ".repeat(text[line..key.offset].len() - usize::from(text[line..key.offset].ends_with(['\'', '"'])))
        },
    );

    Some(YamlService {
        class,
        child_indent,
        arguments: child("arguments").cloned(),
        has_tags: child("tags").is_some(),
        content_end: last_content_end(text, service.offset, end),
    })
}

/// Lines of a block mapping under `arguments:`: (name, raw lines) per `$name:` entry;
/// `None` for lists, flow values and anything else.
fn named_entries(text: &str, key: &YamlKey) -> Option<(Vec<(String, String)>, Range<usize>)> {
    let line = line_start(text, key.offset);
    let line_end = text[key.offset..].find('\n').map_or(text.len(), |found| key.offset + found + 1);
    let key_indent = text[line..key.offset].len() - usize::from(text[line..key.offset].ends_with(['\'', '"']));
    if !key.value.is_empty() {
        return None;
    }

    let mut entries: Vec<(String, String)> = Vec::new();
    let mut entry_indent: Option<usize> = None;
    let mut end = line_end;
    let mut offset = line_end;

    for next in text[line_end.min(text.len())..].split_inclusive('\n') {
        let line_offset = offset;
        offset += next.len();
        let trimmed = next.trim_start();
        if trimmed.trim().is_empty() {
            continue;
        }

        let indent = next.len() - trimmed.len();
        if indent <= key_indent {
            break;
        }

        let expected = *entry_indent.get_or_insert(indent);
        if indent == expected {
            let (name, _) = index::split_yaml_key(trimmed.trim_end())?;
            entries.push((name.trim_matches(['\'', '"']).strip_prefix('$')?.to_owned(), String::new()));
        } else if entries.is_empty() {
            return None;
        }

        entries.last_mut()?.1.push_str(next);
        end = line_offset + next.len();
    }

    Some((entries, line_end.min(end)..end))
}

/// Replacement of the `arguments:` block (or an insertion when there is none) so its
/// `$name` entries match the constructor: kept values stay, new parameters get a
/// placeholder, removed ones go. `None` when the block is a list or already matches.
pub(crate) fn resync_arguments(ctx: &Context, text: &str, caret: usize) -> Option<Edit> {
    let service = yaml_service_at(text, caret)?;
    let class = service.class.filter(|class| ctx.index.classes.contains_key(class))?;
    let model = model_of(ctx, &class)?;
    let eol = eol_of(text);

    let (existing, range) = match &service.arguments {
        Some(key) => {
            let (entries, range) = named_entries(text, key)?;
            (entries, Some(range))
        }
        None => (Vec::new(), None),
    };

    let names: Vec<String> = existing.iter().map(|(name, _)| name.clone()).collect();
    let plan = plan(ctx, &model, &names, false);
    if plan.arguments.is_empty() && existing.is_empty() {
        return None;
    }

    let unit = " ".repeat(yaml_indent(text).1.len());
    let entry_indent = format!("{}{unit}", service.child_indent);
    let mut body = String::new();
    for argument in &plan.arguments {
        match existing.iter().find(|(name, _)| *name == argument.name) {
            Some((_, raw)) => body.push_str(raw),
            None => body.push_str(&format!("{entry_indent}${}: {}{eol}", argument.name, yaml_value(argument))),
        }
    }

    if body.is_empty() {
        return None;
    }

    match (service.arguments, range) {
        (Some(key), Some(range)) => {
            let header_end = text[key.offset..].find('\n').map_or(text.len(), |found| key.offset + found + 1);
            let old = &text[header_end..range.end.max(header_end)];
            (old != body).then(|| (header_end..range.end.max(header_end), body))
        }
        _ => {
            let separator = if text[..service.content_end].ends_with('\n') { "" } else { eol };
            Some((service.content_end..service.content_end, format!("{separator}{}arguments:{eol}{body}", service.child_indent)))
        }
    }
}

/// `tags: [...]` for a service whose class implies tags and that has none.
pub(crate) fn add_implied_tags(ctx: &Context, text: &str, caret: usize) -> Option<(Vec<&'static str>, Edit)> {
    if has_autoconfigure(text) {
        return None;
    }

    let service = yaml_service_at(text, caret)?;
    let class = service.class.filter(|class| ctx.index.classes.contains_key(class))?;
    if service.has_tags {
        return None;
    }

    let tags = implied_tags(ctx, &model_of(ctx, &class)?);
    if tags.is_empty() {
        return None;
    }

    let eol = eol_of(text);
    let separator = if text[..service.content_end].ends_with('\n') { "" } else { eol };
    let edit = (service.content_end..service.content_end, format!("{separator}{}tags: {}{eol}", service.child_indent, tag_list(&tags)));
    Some((tags, edit))
}
