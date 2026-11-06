//! What one source file says about beans: the beans it declares, the
//! injection points it has, its types' supertypes and its named pointcuts.

use crate::kotlin;
use openidea_plugin::Range;
use spring_text::java::{self, Annotation, MemberKind, TypeRef};
use spring_text::{decapitalize, range, xml};

const P_NAMESPACE: &str = "http://www.springframework.org/schema/p";
const C_NAMESPACE: &str = "http://www.springframework.org/schema/c";

/// Whether the attribute of `els[i]` names a bean: `ref=`-style attributes, and
/// `p:x-ref` / `c:x-ref` when the prefix is bound to the `p`/`c` namespaces.
pub fn is_xml_ref(els: &[xml::Element], i: usize, a: &xml::Attr) -> bool {
    let e = &els[i];
    if XML_REFS.contains(&a.local()) || a.local() == "bean" && matches!(e.local(), "ref" | "idref") {
        return true;
    }
    let Some((prefix, local)) = a.name.split_once(':') else { return false };
    if !local.ends_with("-ref") || local.len() == "-ref".len() {
        return false;
    }
    let decl = format!("xmlns:{prefix}");
    let mut at = Some(i);
    while let Some(k) = at {
        if let Some(d) = els[k].attrs.iter().find(|d| d.name == decl) {
            return d.value == P_NAMESPACE || d.value == C_NAMESPACE;
        }
        at = els[k].parent;
    }
    false
}

pub const INJECT: [&str; 3] = ["Autowired", "Inject", "Resource"];
/// Hold one bean: `Optional<Foo>` is a `Foo`.
const SINGLE_WRAPPERS: [&str; 4] = ["Optional", "ObjectProvider", "Provider", "Lazy"];
/// Hold every bean of their last type argument: `List<Handler>`.
const MULTI_WRAPPERS: [&str; 5] = ["List", "Set", "Collection", "Iterable", "Map"];
/// XML attributes naming another bean.
const XML_REFS: [&str; 6] = ["ref", "parent", "depends-on", "factory-bean", "key-ref", "value-ref"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bean {
    pub name: String,
    pub aliases: Vec<String>,
    /// Shown as is: `@Service`, `@Bean`, `Spring Data repository`, `<bean>`.
    pub kind: String,
    /// Simple name of the bean's type; empty when unknown.
    pub ty: String,
    pub primary: bool,
    pub profiles: Vec<String>,
    /// `@Qualifier`/`@Named` values on the declaration.
    pub qualifiers: Vec<String>,
    pub range: Range,
}

impl Bean {
    pub fn answers_to(&self, name: &str) -> bool {
        self.name == name || self.aliases.iter().any(|a| a == name) || self.qualifiers.iter().any(|q| q == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Injection {
    /// Simple type name; empty for a by-name reference (XML `ref`).
    pub ty: String,
    /// The field or parameter name, Spring's last resort between candidates.
    pub name: String,
    pub qualifier: Option<String>,
    /// A collection, map or array: every candidate is injected.
    pub multiple: bool,
    pub range: Range,
    /// Index of the bean (in the same file) it is injected into.
    pub owner: Option<usize>,
    /// The `@Autowired`/`@Inject` of an injected field.
    pub field_annotation: Option<Range>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    pub name: String,
    pub qualified: String,
    pub supers: Vec<String>,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pointcut {
    pub name: String,
    pub class: String,
    pub range: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileModel {
    pub beans: Vec<Bean>,
    pub injections: Vec<Injection>,
    pub types: Vec<TypeInfo>,
    pub pointcuts: Vec<Pointcut>,
}

/// The model of a Java, Kotlin or Spring XML file (`None`: nothing to keep).
pub fn parse(path: &str, text: &str) -> Option<FileModel> {
    let model = if path.ends_with(".java") {
        java_model(text)
    } else if path.ends_with(".kt") {
        kotlin::model(text)
    } else if path.ends_with(".xml") {
        xml_model(text)
    } else {
        return None;
    };
    (model != FileModel::default()).then_some(model)
}

pub fn strings_of(a: &Annotation, names: &[&str]) -> Vec<String> {
    a.strings(names).into_iter().map(|s| s.value.clone()).filter(|s| !s.is_empty()).collect()
}

fn values(annotations: &[Annotation], names: &[&str]) -> Vec<String> {
    annotations.iter().filter(|a| names.contains(&a.name.as_str())).flat_map(|a| strings_of(a, &["value"])).collect()
}

/// The type an injection point of type `ty` asks for, and whether it takes every candidate.
fn unwrap(ty: &TypeRef) -> (&TypeRef, bool) {
    if ty.array {
        return (ty, true);
    }
    match ty.args.last() {
        Some(last) if MULTI_WRAPPERS.contains(&ty.name.as_str()) => (unwrap(last).0, true),
        Some(last) if SINGLE_WRAPPERS.contains(&ty.name.as_str()) => unwrap(last),
        _ => (ty, false),
    }
}

/// Adds the injection point `name: ty`, unless it is a `@Value`.
pub fn push_injection(
    text: &str,
    out: &mut FileModel,
    ty: &TypeRef,
    name: &str,
    annotations: &[Annotation],
    owner: Option<usize>,
    field_annotation: Option<Range>,
) {
    if annotations.iter().any(|a| a.name == "Value") {
        return;
    }
    let (target, multiple) = unwrap(ty);
    let qualifier = annotations.iter().find_map(|a| match a.name.as_str() {
        "Qualifier" | "Named" => strings_of(a, &["value"]).pop(),
        "Resource" => strings_of(a, &["name"]).pop(),
        _ => None,
    });
    out.injections.push(Injection {
        ty: target.name.clone(),
        name: name.to_owned(),
        qualifier,
        multiple,
        range: range(text, target.at, target.at + target.name.len()),
        owner,
        field_annotation,
    });
}

/// A class bean's declaration from its annotations; `None` when it isn't one.
pub fn class_bean(text: &str, name: &str, at: usize, annotations: &[Annotation], repository: bool) -> Option<Bean> {
    let stereotype = annotations.iter().find(|a| java::STEREOTYPES.contains(&a.name.as_str()));
    if stereotype.is_none() && !repository {
        return None;
    }
    let explicit = stereotype.and_then(|a| strings_of(a, &["value"]).pop());
    Some(Bean {
        name: explicit.unwrap_or_else(|| decapitalize(name)),
        aliases: Vec::new(),
        kind: stereotype.map_or("Spring Data repository".into(), |a| format!("@{}", a.name)),
        ty: name.to_owned(),
        primary: annotations.iter().any(|a| a.name == "Primary"),
        profiles: values(annotations, &["Profile"]),
        qualifiers: values(annotations, &["Qualifier", "Named"]),
        range: range(text, at, at + name.len()),
    })
}

/// A `@Bean` method's declaration; its profiles default to the class's.
pub fn method_bean(text: &str, name: &str, at: usize, ty: &str, annotations: &[Annotation], class: &[String]) -> Bean {
    let mut names = annotations.iter().find(|a| a.name == "Bean").map(|a| strings_of(a, &["name", "value"])).unwrap_or_default();
    let first = if names.is_empty() { name.to_owned() } else { names.remove(0) };
    let profiles = values(annotations, &["Profile"]);
    Bean {
        name: first,
        aliases: names,
        kind: "@Bean".into(),
        ty: ty.to_owned(),
        primary: annotations.iter().any(|a| a.name == "Primary"),
        profiles: if profiles.is_empty() { class.to_vec() } else { profiles },
        qualifiers: values(annotations, &["Qualifier", "Named"]),
        range: range(text, at, at + name.len()),
    }
}

/// Whether a type with these supertypes is a Spring Data repository interface.
pub fn is_repository<'a>(kind: &str, mut supers: impl Iterator<Item = &'a str>) -> bool {
    kind == "interface" && supers.any(|s| s.ends_with("Repository"))
}

fn java_model(text: &str) -> FileModel {
    let file = java::parse(text);
    let mut out = FileModel::default();
    for (index, t) in file.types.iter().enumerate() {
        let supers: Vec<String> = t.supers().map(|s| s.name.clone()).collect();
        out.types.push(TypeInfo {
            name: t.name.clone(),
            qualified: file.qualified(index),
            supers: supers.clone(),
            range: range(text, t.at, t.at + t.name.len()),
        });
        let repository = is_repository(&t.kind, supers.iter().map(String::as_str));
        let bean = if t.kind == "@interface" { None } else { class_bean(text, &t.name, t.at, &t.annotations, repository) };
        let class_profiles = bean.as_ref().map(|b| b.profiles.clone()).unwrap_or_default();
        let owner = bean.map(|b| {
            out.beans.push(b);
            out.beans.len() - 1
        });
        if owner.is_some() {
            for p in &t.components {
                push_injection(text, &mut out, &p.ty, &p.name, &p.annotations, owner, None);
            }
        }
        let lombok = t.has(&["RequiredArgsConstructor", "AllArgsConstructor"]);
        for m in &t.members {
            match m.kind {
                MemberKind::Constructor if owner.is_some() || m.has(&["Autowired", "Inject"]) => {
                    for p in &m.params {
                        push_injection(text, &mut out, &p.ty, &p.name, &p.annotations, owner, None);
                    }
                }
                MemberKind::Method if m.has(&["Bean"]) => {
                    let ty = m.ty.as_ref().map(|t| t.name.as_str()).unwrap_or_default();
                    out.beans.push(method_bean(text, &m.name, m.at, ty, &m.annotations, &class_profiles));
                    let bean = Some(out.beans.len() - 1);
                    for p in &m.params {
                        push_injection(text, &mut out, &p.ty, &p.name, &p.annotations, bean, None);
                    }
                }
                MemberKind::Method if m.has(&["Autowired", "Inject"]) => {
                    for p in &m.params {
                        push_injection(text, &mut out, &p.ty, &p.name, &p.annotations, owner, None);
                    }
                }
                MemberKind::Method if m.has(&["Pointcut"]) => out.pointcuts.push(Pointcut {
                    name: m.name.clone(),
                    class: t.name.clone(),
                    range: range(text, m.at, m.at + m.name.len()),
                }),
                MemberKind::Field => {
                    let Some(ty) = &m.ty else { continue };
                    let injected = m.annotations.iter().find(|a| INJECT.contains(&a.name.as_str()));
                    let modifier = |s: &str| m.modifiers.iter().any(|x| x == s);
                    let constructor_field = owner.is_some() && lombok && modifier("final") && !modifier("static");
                    if injected.is_some() || constructor_field {
                        let annotation =
                            injected.filter(|a| a.name != "Resource").map(|a| range(text, a.at, a.end));
                        push_injection(text, &mut out, ty, &m.name, &m.annotations, owner, annotation);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn simple(class: &str) -> &str {
    class.rsplit(['.', '$']).next().unwrap_or(class)
}

fn xml_model(text: &str) -> FileModel {
    let els = xml::elements(text);
    let mut out = FileModel::default();
    if xml::root(&els) != Some("beans") {
        return out;
    }
    let attr_range = |a: &xml::Attr| range(text, a.value_at, a.value_end);
    let mut owners: Vec<Option<usize>> = vec![None; els.len()];
    let mut aliases = Vec::new();
    for (i, e) in els.iter().enumerate().skip(1) {
        let parent = e.parent.and_then(|p| owners[p]);
        owners[i] = parent;
        if e.local() == "alias" {
            if let (Some(name), Some(alias)) = (e.value("name"), e.value("alias")) {
                aliases.push((name.to_owned(), alias.to_owned()));
            }
            continue;
        }
        let top = e.parent.is_some_and(|p| els[p].local() == "beans");
        let id = e.attr("id");
        let names: Vec<String> =
            e.value("name").unwrap_or("").split([',', ';', ' ']).filter(|n| !n.is_empty()).map(str::to_owned).collect();
        let class = e.value("class").unwrap_or("");
        let is_bean = e.local() == "bean" && (id.is_some() || !names.is_empty() || !class.is_empty());
        if is_bean || top && id.is_some() {
            let mut all: Vec<String> = id.map(|a| a.value.clone()).into_iter().chain(names).collect();
            if all.is_empty() {
                all.push(format!("{class}#0"));
            }
            let mut profiles = Vec::new();
            let mut up = e.parent;
            while let Some(p) = up {
                if let Some(profile) = els[p].value("profile") {
                    profiles.extend(profile.split([',', ' ']).filter(|s| !s.is_empty()).map(str::to_owned));
                }
                up = els[p].parent;
            }
            let at = id.or(e.attr("name")).or(e.attr("class")).map_or(range(text, e.at, e.at + e.name.len()), attr_range);
            out.beans.push(Bean {
                name: all.remove(0),
                aliases: all,
                kind: format!("<{}>", e.name),
                ty: simple(class).to_owned(),
                primary: e.value("primary") == Some("true"),
                profiles,
                qualifiers: Vec::new(),
                range: at,
            });
            owners[i] = Some(out.beans.len() - 1);
        }
        let by_name = e.attrs.iter().filter(|a| is_xml_ref(&els, i, a));
        for a in by_name {
            for name in a.value.split([',', ';', ' ']).filter(|n| !n.is_empty()) {
                out.injections.push(Injection {
                    ty: String::new(),
                    name: name.to_owned(),
                    qualifier: Some(name.to_owned()),
                    multiple: false,
                    range: attr_range(a),
                    owner: owners[i],
                    field_annotation: None,
                });
            }
        }
    }
    for (name, alias) in aliases {
        if let Some(b) = out.beans.iter_mut().find(|b| b.name == name) {
            b.aliases.push(alias);
        }
    }
    out
}
