use crate::file_name;
use openidea_plugin::{Diagnostic, Location, Range, Severity};
use spring_text::index::Index;
use spring_text::xml::{self, Element};
use spring_text::xml_check::{Rule, check};
use spring_text::{beans, parse_source, range};

const BOOLEAN: &[&str] = &["true", "false"];

/// Written by hand from the Spring AMQP and Spring JMS XSDs: only elements listed completely.
const RABBIT: &[Rule] = &[
    Rule {
        element: "listener-container",
        required: &[],
        enums: &[
            ("acknowledge", &["auto", "manual", "none"]),
            ("auto-startup", BOOLEAN),
            ("missing-queues-fatal", BOOLEAN),
            ("mismatched-queues-fatal", BOOLEAN),
            ("auto-declare", BOOLEAN),
        ],
        refs: &[],
    },
    Rule { element: "listener", required: &["ref"], enums: &[], refs: &["ref"] },
    Rule { element: "connection-factory", required: &[], enums: &[("publisher-confirms", BOOLEAN), ("publisher-returns", BOOLEAN)], refs: &[] },
    Rule { element: "admin", required: &[], enums: &[("auto-startup", BOOLEAN), ("ignore-declaration-exceptions", BOOLEAN)], refs: &[] },
    Rule { element: "queue", required: &[], enums: &[("durable", BOOLEAN), ("exclusive", BOOLEAN), ("auto-delete", BOOLEAN)], refs: &[] },
    Rule { element: "direct-exchange", required: &["name"], enums: &[("durable", BOOLEAN), ("auto-delete", BOOLEAN)], refs: &[] },
    Rule { element: "topic-exchange", required: &["name"], enums: &[("durable", BOOLEAN), ("auto-delete", BOOLEAN)], refs: &[] },
    Rule { element: "fanout-exchange", required: &["name"], enums: &[("durable", BOOLEAN), ("auto-delete", BOOLEAN)], refs: &[] },
    Rule { element: "headers-exchange", required: &["name"], enums: &[("durable", BOOLEAN), ("auto-delete", BOOLEAN)], refs: &[] },
];

const JMS: &[Rule] = &[
    Rule {
        element: "listener-container",
        required: &[],
        enums: &[
            ("acknowledge", &["auto", "client", "dups-ok", "transacted"]),
            ("container-type", &["default", "default102", "simple", "simple102"]),
            ("destination-type", &["queue", "topic", "durableTopic", "sharedTopic", "sharedDurableTopic"]),
            ("cache", &["none", "connection", "session", "consumer", "auto"]),
        ],
        refs: &[],
    },
    Rule { element: "listener", required: &["destination", "ref"], enums: &[], refs: &["ref"] },
];

#[derive(Clone, Debug)]
pub struct BeanDef {
    pub name: String,
    pub class: Option<String>,
    pub range: Range,
}

#[derive(Clone, Debug)]
pub struct ClassDef {
    pub fqn: String,
    /// It extends or implements something, so methods may come from a type that is not scanned.
    pub inherits: bool,
    pub methods: Vec<(String, Range)>,
}

/// The beans and classes one project file declares.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub beans: Vec<BeanDef>,
    pub classes: Vec<ClassDef>,
}

pub fn parse(path: &str, text: &str) -> Option<Info> {
    if beans::ignored(path) {
        return None;
    }
    let info = if path.ends_with(".xml") { xml_info(text)? } else { source_info(path, text) };
    (!info.beans.is_empty() || !info.classes.is_empty()).then_some(info)
}

fn xml_info(text: &str) -> Option<Info> {
    if !text.contains("springframework.org/schema/") {
        return None;
    }
    let els = xml::elements(text);
    if xml::root(&els) != Some("beans") {
        return None;
    }
    let beans = xml::bean_ids(&els)
        .into_iter()
        .flat_map(|a| {
            let class = xml::element_at(&els, a.at)
                .map(|e| &els[e])
                .filter(|e| e.local() == "bean")
                .and_then(|e| e.value("class"))
                .map(|c| c.replace('$', "."));
            a.value
                .split([',', ';', ' '])
                .filter(|n| !n.is_empty())
                .map(|n| BeanDef { name: n.to_owned(), class: class.clone(), range: range(text, a.value_at, a.value_end) })
                .collect::<Vec<_>>()
        })
        .collect();
    Some(Info { beans, classes: Vec::new() })
}

fn source_info(path: &str, text: &str) -> Info {
    let file = parse_source(path, text);
    let classes = file
        .types
        .iter()
        .enumerate()
        .map(|(i, t)| ClassDef {
            fqn: file.qualified(i),
            inherits: t.supers().next().is_some(),
            methods: t
                .methods()
                .filter(|m| !m.modifiers.iter().any(|x| x == "private"))
                .map(|m| (m.name.clone(), range(text, m.at, m.at + m.name.len())))
                .collect(),
        })
        .collect();
    let beans = spring_text::java::bean_names(&file)
        .into_iter()
        .map(|b| {
            let class = match file.types.iter().position(|t| t.at == b.at) {
                Some(i) => file.qualified(i),
                None => file.resolve(&b.ty),
            };
            BeanDef { name: b.name, class: Some(class), range: range(text, b.at, b.at + b.len) }
        })
        .collect();
    Info { beans, classes }
}

fn bean<'a>(index: &'a Index<Info>, name: &str) -> impl Iterator<Item = (&'a str, &'a BeanDef)> {
    let name = name.to_owned();
    index.iter().flat_map(move |(p, info)| info.beans.iter().filter(|b| b.name == name).map(move |b| (p, b)).collect::<Vec<_>>())
}

fn class<'a>(index: &'a Index<Info>, fqn: &str) -> Option<(&'a str, &'a ClassDef)> {
    index.iter().find_map(|(p, info)| info.classes.iter().find(|c| c.fqn == fqn).map(|c| (p, c)))
}

fn rules_of(namespace: &str) -> Option<&'static [Rule]> {
    if namespace.ends_with("/schema/rabbit") {
        return Some(RABBIT);
    }
    namespace.ends_with("/schema/jms").then_some(JMS)
}

/// The class of the bean a listener's `ref` names, when the project has the bean and the class.
fn listener_class<'a>(index: &'a Index<Info>, listener: &Element) -> Option<(&'a str, &'a ClassDef)> {
    let name = listener.value("ref")?;
    let fqn = bean(index, name).find_map(|(_, b)| b.class.clone())?;
    class(index, &fqn)
}

/// Required attributes, enumerated values, unknown `ref` beans (Info: libraries and the container are invisible) and unknown `method`s of the Rabbit and JMS namespaces.
pub fn problems(text: &str, index: &Index<Info>) -> Vec<Diagnostic> {
    if !text.contains("/schema/rabbit") && !text.contains("/schema/jms") {
        return Vec::new();
    }
    let all = xml::elements(text);
    let mut out = Vec::new();
    for namespace in ["/schema/rabbit", "/schema/jms"] {
        let scoped: Vec<Element> = (0..all.len())
            .filter(|&i| xml::namespace(&all, i).is_some_and(|n| n.ends_with(namespace)))
            .map(|i| all[i].clone())
            .collect();
        let Some(rules) = scoped.first().and_then(|_| rules_of(namespace)) else { continue };
        let known = |name: &str| !index.complete() || bean(index, name).next().is_some() || beans::INFRASTRUCTURE.contains(&name);
        for finding in check(&scoped, rules, known) {
            let severity = if finding.code == "unresolved-reference" { Severity::Info } else { finding.severity };
            out.push(finding.with_severity(severity).diagnostic(text));
        }
        for listener in scoped.iter().filter(|e| e.local() == "listener") {
            let Some(method) = listener.attr("method").filter(|m| !m.value.is_empty() && !beans::is_expression(&m.value)) else { continue };
            let Some((_, class)) = listener_class(index, listener) else { continue };
            if class.inherits || class.methods.iter().any(|(n, _)| *n == method.value) {
                continue;
            }
            let simple = class.fqn.rsplit('.').next().unwrap_or(&class.fqn);
            out.push(Diagnostic {
                code: Some("unresolved-listener-method".into()),
                ..Diagnostic::new(
                    range(text, method.value_at, method.value_end),
                    Severity::Warning,
                    format!("Cannot resolve method '{}' in '{simple}'.", method.value),
                )
            });
        }
    }
    out
}

pub enum Target {
    Bean,
    /// A method of this class (the listener bean's, when the project has it).
    Method(Option<String>),
}

/// The `ref` or `method` of a listener under the byte offset: what it names, its value and bytes.
pub fn target_at(text: &str, index: &Index<Info>, at: usize) -> Option<(Target, String, usize, usize)> {
    let all = xml::elements(text);
    let i = xml::element_at(&all, at)?;
    let e = &all[i];
    if e.local() != "listener" || !xml::namespace(&all, i).is_some_and(|n| rules_of_namespace(n)) {
        return None;
    }
    let a = e.attrs.iter().find(|a| a.holds(at))?;
    let target = match a.local() {
        "ref" => Target::Bean,
        "method" => Target::Method(listener_class(index, e).map(|(_, c)| c.fqn.clone())),
        _ => return None,
    };
    Some((target, a.value.clone(), a.value_at, a.value_end))
}

fn rules_of_namespace(namespace: &str) -> bool {
    rules_of(namespace).is_some()
}

pub fn bean_locations(index: &Index<Info>, name: &str) -> Vec<Location> {
    bean(index, name).map(|(p, b)| Location { path: p.into(), range: b.range }).collect()
}

pub fn method_locations(index: &Index<Info>, class_fqn: &str, name: &str) -> Vec<Location> {
    let Some((path, class)) = class(index, class_fqn) else { return Vec::new() };
    class.methods.iter().filter(|(n, _)| n == name).map(|(_, r)| Location { path: path.into(), range: *r }).collect()
}

pub fn bean_names(index: &Index<Info>) -> Vec<(String, Option<String>)> {
    let mut names: Vec<_> = index
        .iter()
        .flat_map(|(_, info)| info.beans.iter().map(|b| (b.name.clone(), b.class.as_deref().map(|c| file_name(&c.replace('.', "/")).to_owned()))))
        .collect();
    names.sort();
    names.dedup_by(|a, b| a.0 == b.0);
    names
}

pub fn method_names(index: &Index<Info>, class_fqn: &str) -> Vec<String> {
    let Some((_, class)) = class(index, class_fqn) else { return Vec::new() };
    let mut names: Vec<_> = class.methods.iter().map(|(n, _)| n.clone()).collect();
    names.sort();
    names.dedup();
    names
}
