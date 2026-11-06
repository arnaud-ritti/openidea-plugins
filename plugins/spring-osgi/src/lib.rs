//! Spring OSGi (Spring Dynamic Modules) for OpenIDEA. Port of JetBrains'
//! Spring OSGi plugin (Apache-2.0, intellij-obsolete-plugins/spring-osgi):
//! its inspections and reference converters, evaluated on the XML text and
//! the project's Java sources (classes by fully qualified name, methods by
//! name and simple parameter types) instead of the IntelliJ Spring model.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{
    Diagnostic, Error, Invocation, Location, PickItem, Plugin, Position, Range, Severity, TextEdit, host, register_plugin,
};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET};
use spring_text::xml::{self, Attr, Element};
use spring_text::{beans, java, range};

const OSGI: &str = "http://www.springframework.org/schema/osgi";
const COMPENDIUM: &str = "http://www.springframework.org/schema/osgi-compendium";
const LIFECYCLE_LISTENER: &str = "org.springframework.osgi.service.importer.OsgiServiceLifecycleListener";

// SpringOsgiBundle.properties
const ILLEGAL_INTERFACES: &str = "It is illegal to use both interface attribute and interfaces element at the same time";
const ILLEGAL_REF_AND_BEAN: &str = "It is illegal to use both ref attribute and bean element at the same time";
const ILLEGAL_COMPARATOR: &str =
    "It is illegal to use both comparator-ref attribute and comparator element at the same time";
const RETURN_TYPE: &str = "Method must have 'void' return type";
const PUBLIC: &str = "Method must be public";
const PARAMETERS: &str = "Method must have parameters: anyMethodName(ServiceType service, Dictionary properties), anyMethodName(ServiceType service, Map properties), anyMethodName(ServiceReference ref)";

#[derive(Clone, Debug)]
struct Method {
    name: String,
    range: Range,
    public: bool,
    void: bool,
    /// Simple names of the parameter types.
    params: Vec<String>,
}

#[derive(Clone, Debug)]
struct Class {
    fqn: String,
    interface: bool,
    range: Range,
    /// Fully qualified, as far as the file's imports tell.
    supers: Vec<String>,
    methods: Vec<Method>,
}

#[derive(Clone, Debug)]
struct BeanDecl {
    name: String,
    class: Option<String>,
    range: Range,
}

/// What a project file declares: Java classes, or the beans of a Spring XML file / Java class.
#[derive(Clone, Debug, Default)]
struct Info {
    beans: Vec<BeanDecl>,
    classes: Vec<Class>,
}

fn parse(path: &str, text: &str) -> Option<Info> {
    if path.ends_with(".java") {
        return Some(java_info(text));
    }
    if beans::ignored(path) || !text.contains("springframework.org/schema/") {
        return None;
    }
    let els = xml::elements(text);
    if xml::root(&els) != Some("beans") {
        return None;
    }
    let mut beans = Vec::new();
    for a in xml::bean_ids(&els) {
        let class = xml::element_at(&els, a.at)
            .map(|e| &els[e])
            .filter(|e| e.local() == "bean")
            .and_then(|e| e.value("class"))
            .map(|c| c.replace('$', "."));
        for (name, from, to) in split_names(&a.value, a.value_at) {
            beans.push(BeanDecl { name: name.to_owned(), class: class.clone(), range: range(text, from, to) });
        }
    }
    Some(Info { beans, classes: Vec::new() })
}

fn java_info(text: &str) -> Info {
    let f = java::parse(text);
    let classes = f
        .types
        .iter()
        .enumerate()
        .map(|(i, t)| Class {
            fqn: f.qualified(i),
            interface: t.kind == "interface",
            range: range(text, t.at, t.at + t.name.len()),
            supers: t.supers().map(|s| qualified(&f, s)).collect(),
            methods: t
                .methods()
                .map(|m| Method {
                    name: m.name.clone(),
                    range: range(text, m.at, m.at + m.name.len()),
                    public: t.kind == "interface" || m.modifiers.iter().any(|x| x == "public"),
                    void: m.ty.as_ref().is_some_and(|t| t.name == "void"),
                    params: m.params.iter().map(|p| p.ty.name.clone()).collect(),
                })
                .collect(),
        })
        .collect();
    let beans = java::bean_names(&f)
        .into_iter()
        .map(|b| {
            let class = match f.types.iter().position(|t| t.at == b.at) {
                Some(i) => f.qualified(i),
                None => f.resolve(&b.ty),
            };
            BeanDecl { name: b.name, class: Some(class), range: range(text, b.at, b.at + b.len) }
        })
        .collect();
    Info { beans, classes }
}

/// A supertype's fully qualified name: as written when qualified, else through the imports.
fn qualified(f: &java::JavaFile, t: &java::TypeRef) -> String {
    let head: String = t.text.split('<').next().unwrap_or("").split_whitespace().collect();
    if head.contains('.') { head } else { f.resolve(&t.name) }
}

/// The names of a `name`/`depends-on` list (`a, b c`) with their byte ranges.
fn split_names(value: &str, at: usize) -> Vec<(&str, usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in value.char_indices().chain([(value.len(), ',')]) {
        if c == ',' || c == ';' || c.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((&value[s..i], at + s, at + i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    out
}

fn class<'a>(index: &'a Index<Info>, fqn: &str) -> Option<(&'a str, &'a Class)> {
    index.iter().find_map(|(p, info)| info.classes.iter().find(|c| c.fqn == fqn).map(|c| (p, c)))
}

fn named<'a>(index: &'a Index<Info>, name: &'a str) -> impl Iterator<Item = (&'a str, &'a BeanDecl)> {
    index.iter().flat_map(move |(p, info)| info.beans.iter().filter(move |b| b.name == name).map(move |b| (p, b)))
}

/// `fqn` and its supertypes, and whether they are all project classes (so
/// the list is complete).
fn ancestors(index: &Index<Info>, fqn: &str) -> (Vec<String>, bool) {
    let mut seen: Vec<String> = Vec::new();
    let mut todo = vec![fqn.to_owned()];
    let mut known = true;
    while let Some(next) = todo.pop() {
        if seen.contains(&next) {
            continue;
        }
        match class(index, &next) {
            Some((_, c)) => todo.extend(c.supers.iter().cloned()),
            None if next != "java.lang.Object" => known = false,
            None => {}
        }
        seen.push(next);
    }
    (seen, known)
}

/// The methods named `name` (any name when `None`) of `fqn` and its project supertypes.
fn methods<'a>(index: &'a Index<Info>, fqn: &str, name: Option<&str>) -> Vec<(&'a str, &'a Method)> {
    let (all, _) = ancestors(index, fqn);
    all.iter()
        .filter_map(|c| class(index, c))
        .flat_map(|(p, c)| c.methods.iter().map(move |m| (p, m)))
        .filter(|(_, m)| name.is_none_or(|n| m.name == n))
        .collect()
}

/// `ReferenceListenerMethodConverter.checkProperParameters`, or the
/// registration listener's `(ServiceType, Map|Dictionary)`, by simple name.
fn proper_parameters(m: &Method, registration: bool) -> bool {
    let map = |t: &str| matches!(t, "Map" | "Dictionary" | "Object");
    match m.params.as_slice() {
        [_, props] => map(props),
        [r] => !registration && matches!(r.as_str(), "ServiceReference" | "Object"),
        _ => false,
    }
}

fn accepts(m: &Method, registration: bool) -> bool {
    m.void && m.public && proper_parameters(m, registration)
}

/// A Spring DM XML document: its elements and the prefixes of the OSGi namespaces.
struct OsgiXml<'a> {
    text: &'a str,
    els: Vec<Element>,
    prefixes: Vec<String>,
}

impl<'a> OsgiXml<'a> {
    fn new(text: &'a str) -> Option<Self> {
        if !text.contains(OSGI) {
            return None;
        }
        let els = xml::elements(text);
        let prefixes: Vec<String> = els
            .iter()
            .flat_map(|e| &e.attrs)
            .filter(|a| matches!(a.value.trim(), OSGI | COMPENDIUM))
            .filter_map(|a| if a.name == "xmlns" { Some(String::new()) } else { a.name.strip_prefix("xmlns:").map(str::to_owned) })
            .collect();
        (!prefixes.is_empty()).then_some(Self { text, els, prefixes })
    }

    fn osgi(&self, e: &Element) -> bool {
        let prefix = e.name.rsplit_once(':').map_or("", |(p, _)| p);
        self.prefixes.iter().any(|p| p == prefix)
    }

    fn children(&self, i: usize) -> impl Iterator<Item = &Element> {
        self.els.iter().filter(move |c| c.parent == Some(i))
    }

    fn child(&self, i: usize, local: &str) -> Option<&Element> {
        self.children(i).find(|c| c.local() == local)
    }

    fn name_range(&self, e: &Element) -> (usize, usize) {
        (e.at, e.at + e.name.len())
    }

    /// The trimmed text of an element without children (`<value>a.B</value>`).
    fn content(&self, e: &Element) -> Option<(usize, usize)> {
        let inner = &self.text[e.tag_end.min(e.end)..e.end];
        let close = inner.rfind("</")?;
        let raw = &inner[..close];
        let from = e.tag_end + (raw.len() - raw.trim_start().len());
        Some((from, from + raw.trim().len()))
    }

    /// The service interfaces of a service/reference: its `interface` and `<interfaces>` values.
    fn interfaces(&self, i: usize) -> Vec<(String, usize, usize)> {
        let e = &self.els[i];
        let mut out: Vec<_> = e.attr("interface").map(|a| (a.value.clone(), a.value_at, a.value_end)).into_iter().collect();
        if let Some(list) = self.els.iter().position(|c| c.parent == Some(i) && c.local() == "interfaces") {
            for v in self.children(list).filter(|c| c.local() == "value") {
                if let Some((from, to)) = self.content(v) {
                    out.push((self.text[from..to].to_owned(), from, to));
                }
            }
        }
        out.retain(|(v, ..)| !v.is_empty() && !beans::is_expression(v));
        out
    }

    /// The class of a listener's bean (its `ref`, or a nested `<bean class>`) and where it's named.
    fn listener_class(&self, index: &Index<Info>, i: usize) -> Option<(String, (usize, usize))> {
        let e = &self.els[i];
        if let Some(r) = e.attr("ref") {
            let class = named(index, &r.value).find_map(|(_, b)| b.class.clone())?;
            return Some((class, (r.value_at, r.value_end)));
        }
        let bean = self.child(i, "bean")?;
        Some((bean.value("class")?.replace('$', "."), self.name_range(bean)))
    }
}

enum Target {
    Bean,
    Class,
    /// A listener method of this class (registration listener or reference listener).
    Method(Option<String>, bool),
}

/// The reference under the byte offset `at`: what it names, its text and bytes.
fn target_at(doc: &OsgiXml, index: &Index<Info>, at: usize) -> Option<(Target, String, usize, usize)> {
    if let Some(i) = xml::element_at(&doc.els, at) {
        let e = &doc.els[i];
        let a: &Attr = e.attrs.iter().find(|a| a.holds(at))?;
        if !doc.osgi(e) {
            return None;
        }
        let whole = |t| Some((t, a.value.clone(), a.value_at, a.value_end));
        return match (e.local(), a.local()) {
            ("service" | "listener" | "registration-listener", "ref") | ("list" | "set", "comparator-ref") => {
                whole(Target::Bean)
            }
            (_, "depends-on") => {
                let (name, from, to) = split_names(&a.value, a.value_at)
                    .into_iter()
                    .find(|&(_, from, to)| from <= at && at <= to)
                    .unwrap_or(("", at, at));
                Some((Target::Bean, name.to_owned(), from, to))
            }
            ("service" | "reference" | "list" | "set", "interface") => whole(Target::Class),
            ("listener", "bind-method" | "unbind-method") => {
                whole(Target::Method(doc.listener_class(index, i).map(|c| c.0), false))
            }
            ("registration-listener", "registration-method" | "unregistration-method") => {
                whole(Target::Method(doc.listener_class(index, i).map(|c| c.0), true))
            }
            _ => None,
        };
    }
    doc.els.iter().find_map(|v| {
        let list = &doc.els[v.parent?];
        if v.local() != "value" || list.local() != "interfaces" || !doc.osgi(list) {
            return None;
        }
        let (from, to) = doc.content(v)?;
        (from <= at && at <= to).then(|| (Target::Class, doc.text[from..to].to_owned(), from, to))
    })
}

fn problem(text: &str, (from, to): (usize, usize), message: impl Into<String>, code: &str) -> Diagnostic {
    Diagnostic { code: Some(code.into()), ..Diagnostic::new(range(text, from, to), Severity::Error, message) }
}

/// The three inspections of the original plugin, plus unresolved listener methods.
fn check(doc: &OsgiXml, index: &Index<Info>) -> Vec<Diagnostic> {
    let text = doc.text;
    let mut out = Vec::new();
    for (i, e) in doc.els.iter().enumerate().filter(|(_, e)| doc.osgi(e)) {
        let has = |local: &str| doc.child(i, local).is_some();
        let at = doc.name_range(e);
        let inconsistent = |message| problem(text, at, message, "osgi-elements-inconsistency");
        match e.local() {
            "service" | "reference" | "list" | "set" if e.attr("interface").is_some() && has("interfaces") => {
                out.push(inconsistent(ILLEGAL_INTERFACES));
            }
            _ => {}
        }
        check_values(doc, i, &mut out);
        match e.local() {
            "service" => {
                if e.attr("ref").is_some() && has("bean") {
                    out.push(inconsistent(ILLEGAL_REF_AND_BEAN));
                }
                check_service(doc, index, i, &mut out);
            }
            "list" | "set" if e.attr("comparator-ref").is_some() && has("comparator") => {
                out.push(inconsistent(ILLEGAL_COMPARATOR));
            }
            "listener" if e.parent.is_some_and(|p| matches!(doc.els[p].local(), "reference" | "list" | "set")) => {
                check_listener(doc, index, i, false, &mut out);
            }
            "registration-listener" => check_listener(doc, index, i, true, &mut out),
            _ => {}
        }
    }
    out
}

const BOOLEANS: &[&str] = &["true", "false"];

/// The enumerated and numeric attributes of the Spring DM schema: allowed values per element.
fn check_values(doc: &OsgiXml, i: usize, out: &mut Vec<Diagnostic>) {
    let e = &doc.els[i];
    let local = e.local();
    let importer = matches!(local, "reference" | "list" | "set");
    let comparator_basis = local == "natural" && e.parent.is_some_and(|p| doc.els[p].local() == "comparator");
    let enumerated: &[(&str, &[&str])] = match local {
        "service" => &[
            ("auto-export", &["disabled", "interfaces", "class-hierarchy", "all-classes"]),
            ("context-class-loader", &["unmanaged", "service-provider"]),
        ],
        "reference" => &[
            ("cardinality", &["0..1", "1..1"]),
            ("context-class-loader", &["client", "service-provider", "unmanaged"]),
        ],
        "list" | "set" => &[
            ("cardinality", &["0..N", "1..N"]),
            ("context-class-loader", &["client", "service-provider", "unmanaged"]),
            ("member-type", &["service-object", "service-reference"]),
        ],
        "natural" if comparator_basis => &[("basis", &["service", "service-reference"])],
        _ => &[],
    };
    let booleans = (importer || local == "service").then_some(("greedy-proxying", BOOLEANS));
    for &(name, allowed) in enumerated.iter().chain(booleans.iter()) {
        let Some(a) = e.attr(name) else {
            continue;
        };
        if a.value.contains("${") || allowed.contains(&a.value.trim()) {
            continue;
        }
        let message = format!("Invalid value '{}' for '{name}': must be one of {}", a.value.trim(), allowed.join(", "));
        out.push(problem(doc.text, (a.value_at, a.value_end), message, "osgi-attribute-value"));
    }
    let numeric: Option<(&str, &str, bool)> = match local {
        "service" => Some(("ranking", "an integer", false)),
        _ if importer => Some(("timeout", "a non-negative integer", true)),
        _ => None,
    };
    if let Some((name, expected, non_negative)) = numeric
        && let Some(a) = e.attr(name)
        && !a.value.contains("${")
    {
        let valid = a.value.trim().parse::<i64>().is_ok_and(|n| !non_negative || n >= 0);
        if !valid {
            let message = format!("Invalid value '{}' for '{name}': must be {expected}", a.value.trim());
            out.push(problem(doc.text, (a.value_at, a.value_end), message, "osgi-attribute-value"));
        }
    }
}

/// `SpringOsgiServiceCommonInspection`: the service bean implements every exported interface.
fn check_service(doc: &OsgiXml, index: &Index<Info>, i: usize, out: &mut Vec<Diagnostic>) {
    let e = &doc.els[i];
    let mut implementors = Vec::new();
    if let Some(r) = e.attr("ref")
        && let Some(class) = named(index, &r.value).find_map(|(_, b)| b.class.clone())
    {
        implementors.push((class, (r.value_at, r.value_end)));
    }
    if let Some(bean) = doc.child(i, "bean")
        && let Some(class) = bean.value("class")
    {
        implementors.push((class.replace('$', "."), doc.name_range(bean)));
    }
    let interfaces = doc.interfaces(i);
    for (class, at) in implementors {
        let (supers, known) = ancestors(index, &class);
        for (interface, ..) in interfaces.iter().filter(|(fqn, ..)| known && !supers.contains(fqn)) {
            let message = format!("Bean must be inheritor of '{interface}'");
            out.push(problem(doc.text, at, message, "osgi-service-common"));
        }
    }
}

/// `SpringOsgiListenerInspection` (reference listeners) and the method
/// converters (registration listeners: unresolved methods only).
fn check_listener(doc: &OsgiXml, index: &Index<Info>, i: usize, registration: bool, out: &mut Vec<Diagnostic>) {
    let e = &doc.els[i];
    let Some((class, named_at)) = doc.listener_class(index, i) else { return };
    let (supers, known) = ancestors(index, &class);
    let attrs: &[&str] =
        if registration { &["registration-method", "unregistration-method"] } else { &["bind-method", "unbind-method"] };
    for a in attrs.iter().filter_map(|n| e.attr(n)).filter(|a| !a.value.is_empty()) {
        let at = (a.value_at, a.value_end);
        let found = methods(index, &class, Some(&a.value));
        let Some((_, first)) = found.first() else {
            if known {
                let simple = class.rsplit('.').next().unwrap_or(&class);
                out.push(problem(doc.text, at, format!("Cannot resolve method '{}' in '{simple}'", a.value), "osgi-listener"));
            }
            continue;
        };
        if registration || found.iter().any(|(_, m)| accepts(m, false)) {
            continue;
        }
        if !first.void {
            out.push(problem(doc.text, at, RETURN_TYPE, "osgi-listener"));
        }
        if !first.public {
            out.push(problem(doc.text, at, PUBLIC, "osgi-listener"));
        }
        if !proper_parameters(first, false) {
            out.push(problem(doc.text, at, PARAMETERS, "osgi-listener"));
        }
    }
    // IDEADEV-40586: a listener with both methods needn't implement the interface.
    let both = attrs.iter().all(|n| e.attr(n).is_some());
    if !registration && !both && known && !supers.iter().any(|s| s == LIFECYCLE_LISTENER) {
        out.push(problem(doc.text, named_at, format!("Listener must implement '{LIFECYCLE_LISTENER}'"), "osgi-listener"));
    }
}

pub struct SpringOsgi {
    indexes: Indexes<Info>,
}

impl Default for SpringOsgi {
    fn default() -> Self {
        Self { indexes: Indexes::new(&["**/*.java", "**/*.xml"], parse) }
    }
}

impl SpringOsgi {
    /// The OSGi document, the byte offset of `at` and the project index.
    fn open<'d>(&mut self, doc: &'d Document, at: Position) -> Option<(OsgiXml<'d>, usize, &Index<Info>)> {
        let osgi = OsgiXml::new(&doc.text)?;
        let at = offset(&doc.text, at)?;
        let index = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        Some((osgi, at, index))
    }
}

impl Providers for SpringOsgi {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let problems = match OsgiXml::new(&doc.text) {
            Some(osgi) => match self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) {
                Some(index) => check(&osgi, index),
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some((osgi, at, index)) = self.open(doc, at) else { return Vec::new() };
        let Some((target, value, ..)) = target_at(&osgi, index, at) else { return Vec::new() };
        let location = |path: &str, range| Location { path: path.into(), range };
        match target {
            Target::Bean => named(index, &value).map(|(p, b)| location(p, b.range)).collect(),
            Target::Class => class(index, &value.replace('$', ".")).map(|(p, c)| location(p, c.range)).into_iter().collect(),
            Target::Method(Some(class), _) => methods(index, &class, Some(&value)).into_iter().map(|(p, m)| location(p, m.range)).collect(),
            Target::Method(None, _) => Vec::new(),
        }
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some((osgi, at, index)) = self.open(doc, at) else { return Vec::new() };
        let Some((target, _, from, to)) = target_at(&osgi, index, at) else { return Vec::new() };
        let replace = Some(range(&doc.text, from, to));
        let mut items: Vec<CompletionItem> = match target {
            Target::Bean => index
                .iter()
                .flat_map(|(_, info)| &info.beans)
                .map(|b| {
                    let simple = b.class.as_deref().map(|c| c.rsplit('.').next().unwrap_or(c).to_owned());
                    CompletionItem { detail: simple, ..CompletionItem::new(&b.name, CompletionKind::Reference) }
                })
                .collect(),
            Target::Class => index
                .iter()
                .flat_map(|(_, info)| &info.classes)
                .map(|c| {
                    let kind = if c.interface { CompletionKind::Interface } else { CompletionKind::Class };
                    let sort = format!("{}{}", if c.interface { 0 } else { 1 }, c.fqn);
                    CompletionItem { sort_text: Some(sort), ..CompletionItem::new(&c.fqn, kind) }
                })
                .collect(),
            Target::Method(Some(class), registration) => methods(index, &class, None)
                .into_iter()
                .filter(|(_, m)| accepts(m, registration))
                .map(|(_, m)| {
                    let detail = format!("({})", m.params.join(", "));
                    CompletionItem { detail: Some(detail), ..CompletionItem::new(&m.name, CompletionKind::Method) }
                })
                .collect(),
            Target::Method(None, _) => Vec::new(),
        };
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        for item in &mut items {
            item.replace = replace;
        }
        items
    }
}

impl Plugin for SpringOsgi {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "generate-osgi-bean" => generate_osgi_bean(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

fn osgi_prefix(text: &str) -> Option<&'static str> {
    if text.contains(&format!("xmlns:osgi=\"{OSGI}\"")) {
        return Some("osgi:");
    }

    text.contains(&format!("xmlns=\"{OSGI}\"")).then_some("")
}

fn generate_osgi_bean(ctx: &Invocation) -> Result<Option<String>, Error> {
    let editor = ctx.editor().ok_or("Open the Spring XML file to add the bean to.")?;
    let doc = editor.document()?;
    let Some(prefix) = osgi_prefix(&doc.text) else {
        return Err("Declare the Spring OSGi namespace (xmlns:osgi) in this file first.".into());
    };

    let items = [PickItem::new("service", "osgi:service"), PickItem::new("reference", "osgi:reference")];
    let Some(kind) = host::pick("Generate OSGi bean", &items)? else { return Ok(None) };
    let Some(interface) = host::input("Interface (fully qualified class name)", None)? else { return Ok(None) };
    let interface = interface.trim().to_owned();
    if interface.is_empty() {
        return Ok(None);
    }

    let element = if kind == "service" {
        let Some(target) = host::input("Bean to export (ref)", None)? else { return Ok(None) };
        format!("<{prefix}service ref=\"{}\" interface=\"{}\"/>", escape(target.trim()), escape(&interface))
    } else {
        let id = interface.rsplit('.').next().unwrap_or(&interface);
        let id = format!("{}{}Reference", id[..1].to_lowercase(), &id[1..]);
        format!("<{prefix}reference id=\"{id}\" interface=\"{}\"/>", escape(&interface))
    };

    let at = editor.selections()?.first().map_or(Position::new(0, 0), |r| Position::new(r.start.line, 0));
    let line = doc.text.lines().nth(at.line as usize).unwrap_or("");
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let newline = if doc.text.contains("\r\n") { "\r\n" } else { "\n" };
    let edit = TextEdit { range: Range::new(at, at), text: format!("{indent}{element}{newline}") };
    editor.apply_edits(doc.version, &[edit])?;

    Ok(Some(format!("Added {kind} bean.")))
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

register_plugin!(SpringOsgi, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const GREETER: &str = "package com.acme.api;\n\npublic interface Greeter {\n    String greet(String name);\n}\n";
    const IMPL: &str =
        "package com.acme.impl;\n\nimport com.acme.api.Greeter;\n\npublic class GreeterImpl implements Greeter {\n}\n";
    const PLAIN: &str = "package com.acme.impl;\n\npublic class Plain {\n}\n";
    const LISTENER: &str = r#"package com.acme.impl;

import java.util.Map;
import org.osgi.framework.ServiceReference;

public class GreeterListener {
    public void bound(Greeter g, Map<String, ?> props) {}
    public void unbound(ServiceReference ref) {}
    void hidden(Greeter g, Map props) {}
    public String wrong(Greeter g, Map props) { return null; }
    public void badParams(Greeter g) {}
}
"#;
    const AUDIT: &str = r#"package com.acme.impl;

@Component
public class AuditListener implements OsgiServiceLifecycleListener {
}
"#;
    const CONTEXT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:osgi="http://www.springframework.org/schema/osgi">
  <bean id="greeter" class="com.acme.impl.GreeterImpl"/>
  <bean id="plain" class="com.acme.impl.Plain"/>
  <bean id="listener" class="com.acme.impl.GreeterListener"/>

  <osgi:service id="greeterService" ref="greeter" interface="com.acme.api.Greeter">
    <osgi:registration-listener ref="listener" registration-method="bound" unregistration-method="missing"/>
  </osgi:service>
  <osgi:service id="plainService" ref="plain" interface="com.acme.api.Greeter"/>
  <osgi:service ref="greeter" interface="com.acme.api.Greeter">
    <osgi:interfaces><value> com.acme.api.Greeter </value></osgi:interfaces>
    <bean class="com.acme.impl.GreeterImpl"/>
  </osgi:service>
  <osgi:reference id="greeterRef" interface="com.acme.api.Greeter" depends-on="greeter, plain">
    <osgi:listener ref="listener" bind-method="bound" unbind-method="hidden"/>
    <osgi:listener ref="listener" bind-method="wrong"/>
    <osgi:listener ref="auditListener" bind-method="x"/>
  </osgi:reference>
  <osgi:list id="greeters" interface="com.acme.api.Greeter" comparator-ref="cmp">
    <osgi:comparator><osgi:natural basis="service"/></osgi:comparator>
  </osgi:list>
</beans>
"#;
    /// The OSGi namespace as the default one, beans under a prefix.
    const DEFAULT_NS: &str = r#"<beans:beans xmlns="http://www.springframework.org/schema/osgi"
             xmlns:beans="http://www.springframework.org/schema/beans">
  <reference id="r" interface="com.acme.api.Greeter">
    <listener ref="listener" bind-method="bound" unbind-method="unbound"/>
  </reference>
</beans:beans>
"#;
    const ROOT: &str = "/project/src/main/";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/api/Greeter.java", GREETER);
        host.add_file("src/main/java/com/acme/impl/GreeterImpl.java", IMPL);
        host.add_file("src/main/java/com/acme/impl/Plain.java", PLAIN);
        host.add_file("src/main/java/com/acme/impl/GreeterListener.java", LISTENER);
        host.add_file("src/main/java/com/acme/impl/AuditListener.java", AUDIT);
        host.add_file("src/main/resources/META-INF/spring/context.xml", CONTEXT);
        host.add_file("src/main/resources/META-INF/spring/default.xml", DEFAULT_NS);
        host
    }

    fn doc(name: &str, text: &str) -> Document {
        Document { path: format!("{ROOT}resources/META-INF/spring/{name}"), language: "xml".into(), version: 1, text: text.into() }
    }

    /// The position right after the first `needle` in `text`.
    fn after(text: &str, needle: &str) -> Position {
        openidea_plugin::text::position(text, text.find(needle).unwrap() + needle.len())
    }

    #[test]
    fn reports_the_original_inspections() {
        let host = setup();
        let mut p = SpringOsgi::default();
        p.line_markers(&doc("context.xml", CONTEXT));
        let problems = &host.diagnostics()[&format!("{ROOT}resources/META-INF/spring/context.xml")];
        let found: Vec<_> = problems
            .iter()
            .map(|d| {
                let r = d.range;
                let line = CONTEXT.lines().nth(r.start.line as usize).unwrap();
                let at = line[r.start.character as usize..r.end.character as usize].to_owned();
                (at, d.message.as_str())
            })
            .collect();
        assert_eq!(found, [
            ("missing".to_owned(), "Cannot resolve method 'missing' in 'GreeterListener'"),
            ("plain".into(), "Bean must be inheritor of 'com.acme.api.Greeter'"),
            ("osgi:service".into(), ILLEGAL_INTERFACES),
            ("osgi:service".into(), ILLEGAL_REF_AND_BEAN),
            ("hidden".into(), PUBLIC),
            ("wrong".into(), RETURN_TYPE),
            ("listener".into(), "Listener must implement 'org.springframework.osgi.service.importer.OsgiServiceLifecycleListener'"),
            ("osgi:list".into(), ILLEGAL_COMPARATOR),
        ]);
        assert!(problems.iter().all(|d| d.severity == Severity::Error));
    }

    #[test]
    fn badly_shaped_parameters_are_reported() {
        let host = setup();
        let mut p = SpringOsgi::default();
        let text = DEFAULT_NS.replace("unbind-method=\"unbound\"", "unbind-method=\"badParams\"");
        p.line_markers(&doc("default.xml", &text));
        let problems = &host.diagnostics()[&format!("{ROOT}resources/META-INF/spring/default.xml")];
        assert_eq!(problems.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(), [PARAMETERS]);
    }

    #[test]
    fn navigates_from_references() {
        let _host = setup();
        let mut p = SpringOsgi::default();
        let d = doc("default.xml", DEFAULT_NS);
        let listener = p.definition(&d, after(DEFAULT_NS, "ref=\"list"));
        assert_eq!(listener.len(), 1);
        assert!(listener[0].path.ends_with("context.xml"));
        assert_eq!(listener[0].range.start, Position::new(5, 12));

        let bound = p.definition(&d, after(DEFAULT_NS, "bind-method=\"bo"));
        assert!(bound[0].path.ends_with("GreeterListener.java"));
        assert_eq!(bound[0].range.start, Position::new(6, 16));

        let interface = p.definition(&d, after(DEFAULT_NS, "interface=\"com"));
        assert!(interface[0].path.ends_with("Greeter.java"));

        let d = doc("context.xml", CONTEXT);
        let value = p.definition(&d, after(CONTEXT, "<value> com.acme"));
        assert!(value[0].path.ends_with("Greeter.java"));
        let plain = p.definition(&d, after(CONTEXT, "depends-on=\"greeter, pl"));
        assert_eq!(plain[0].range.start, Position::new(4, 12));
        assert!(p.definition(&d, after(CONTEXT, "<bean id=\"gr")).is_empty(), "not an OSGi reference");
    }

    #[test]
    fn completes_beans_interfaces_and_listener_methods() {
        let _host = setup();
        let mut p = SpringOsgi::default();
        let d = doc("default.xml", DEFAULT_NS);
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(p.completion(&d, after(DEFAULT_NS, "bind-method=\"bo"), None)), ["bound", "unbound"]);
        assert_eq!(labels(p.completion(&d, after(DEFAULT_NS, "ref=\"list"), None)), [
            "auditListener",
            "greeter",
            "greeterRef",
            "greeterService",
            "greeters",
            "listener",
            "plain",
            "plainService",
            "r"
        ]);
        let classes = p.completion(&d, after(DEFAULT_NS, "interface=\"com"), None);
        let greeter = classes.iter().find(|c| c.label == "com.acme.api.Greeter").unwrap();
        assert_eq!(greeter.kind, Some(CompletionKind::Interface));
        assert_eq!(greeter.replace.unwrap().start, after(DEFAULT_NS, "interface=\""));

        let d = doc("context.xml", CONTEXT);
        let registration = p.completion(&d, after(CONTEXT, "unregistration-method=\"mi"), None);
        assert_eq!(labels(registration), ["bound"], "(ServiceType, Map) only");
    }

    fn values(name: &str, body: &str) -> Vec<(String, String)> {
        let host = setup();
        let text = format!(
            "<beans xmlns=\"http://www.springframework.org/schema/beans\"\n       xmlns:osgi=\"http://www.springframework.org/schema/osgi\">\n{body}\n</beans>\n"
        );
        SpringOsgi::default().line_markers(&doc(name, &text));
        let found = host.diagnostics().get(&format!("{ROOT}resources/META-INF/spring/{name}")).cloned().unwrap_or_default();
        assert!(found.iter().all(|d| d.code.as_deref() == Some("osgi-attribute-value") && d.severity == Severity::Error));
        found
            .iter()
            .map(|d| {
                let line = text.lines().nth(d.range.start.line as usize).unwrap();
                let at = line[d.range.start.character as usize..d.range.end.character as usize].to_owned();
                (at, d.message.clone())
            })
            .collect()
    }

    #[test]
    fn wrong_cardinality_is_reported() {
        let found = values("c1.xml", r#"<osgi:reference interface="a.B" cardinality="0..N"/>"#);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "0..N");
        assert!(found[0].1.contains("0..1") && found[0].1.contains("1..1"), "{}", found[0].1);
    }

    #[test]
    fn cardinality_for_list_and_reference_differ() {
        let found = values(
            "c2.xml",
            r#"<osgi:reference interface="a.B" cardinality="1..1"/>
<osgi:list interface="a.B" cardinality="1..N"/>
<osgi:set interface="a.B" cardinality="0..N"/>
<osgi:list interface="a.B" cardinality="1..1"/>
<osgi:set interface="a.B" cardinality="0..1"/>"#,
        );
        assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["1..1", "0..1"]);
        assert!(found[0].1.contains("0..N") && found[0].1.contains("1..N"));
    }

    #[test]
    fn wrong_auto_export_is_reported() {
        let found = values("a.xml", r#"<osgi:service ref="x" auto-export="everything"/>"#);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "everything");
        for allowed in ["disabled", "interfaces", "class-hierarchy", "all-classes"] {
            assert!(found[0].1.contains(allowed), "{}", found[0].1);
        }
    }

    #[test]
    fn wrong_context_class_loader_is_reported() {
        let found = values(
            "l.xml",
            r#"<osgi:service ref="x" context-class-loader="client"/>
<osgi:reference interface="a.B" context-class-loader="client"/>
<osgi:list interface="a.B" context-class-loader="bogus"/>"#,
        );
        assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["client", "bogus"]);
        assert!(!found[0].1.contains("client,") && found[0].1.contains("unmanaged") && found[0].1.contains("service-provider"));
        assert!(found[1].1.contains("client"));
    }

    #[test]
    fn non_integer_ranking_is_reported() {
        let found = values(
            "r.xml",
            r#"<osgi:service ref="x" ranking="high"/>
<osgi:service ref="x" ranking="-3"/>
<osgi:reference interface="a.B" timeout="-1"/>
<osgi:reference interface="a.B" timeout="soon"/>
<osgi:list interface="a.B" timeout="5"/>"#,
        );
        assert_eq!(found.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["high", "-1", "soon"]);
    }

    #[test]
    fn wrong_member_type_is_reported() {
        let found = values(
            "m.xml",
            r#"<osgi:list interface="a.B" member-type="service-object"/>
<osgi:set interface="a.B" member-type="proxy"/>"#,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "proxy");
        assert!(found[0].1.contains("service-object") && found[0].1.contains("service-reference"));
    }

    #[test]
    fn wrong_ordering_basis_is_reported() {
        let found = values(
            "b.xml",
            r#"<osgi:list interface="a.B"><osgi:comparator><osgi:natural basis="service-reference"/></osgi:comparator></osgi:list>
<osgi:list interface="a.B"><osgi:comparator><osgi:natural basis="object"/></osgi:comparator></osgi:list>"#,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "object");
        assert!(found[0].1.contains("service") && found[0].1.contains("service-reference"));
    }

    #[test]
    fn attribute_values_are_quiet_for_library_or_dynamic_name() {
        let found = values(
            "q.xml",
            r#"<osgi:service ref="x" auto-export="all-classes" ranking="${rank}" context-class-loader="${l}"/>
<osgi:reference interface="a.B" cardinality="${card}" timeout="${t}" greedy-proxying="${g}"/>
<osgi:list interface="a.B" member-type="service-reference" greedy-proxying="true"/>
<foo:service xmlns:foo="urn:other" auto-export="nonsense" ranking="x"/>
<bean id="b" class="a.B" cardinality="nonsense"/>"#,
        );
        assert!(found.is_empty(), "{found:?}");
        let host = setup();
        let text = DEFAULT_NS.replace("interface=", "cardinality=\"bad\" greedy-proxying=\"maybe\" interface=");
        SpringOsgi::default().line_markers(&doc("default.xml", &text));
        let found = &host.diagnostics()[&format!("{ROOT}resources/META-INF/spring/default.xml")];
        let at: Vec<_> = found.iter().map(|d| &text.lines().nth(d.range.start.line as usize).unwrap()[d.range.start.character as usize..d.range.end.character as usize]).collect();
        assert_eq!(at, ["bad", "maybe"]);
        let host = setup();
        SpringOsgi::default().line_markers(&doc("default.xml", DEFAULT_NS));
        assert!(host.diagnostics().get(&format!("{ROOT}resources/META-INF/spring/default.xml")).is_none_or(Vec::is_empty));
    }

    #[test]
    fn value_ranges_use_utf16_columns_with_multibyte_text() {
        let found = {
            let host = setup();
            let text = "<beans xmlns:osgi=\"http://www.springframework.org/schema/osgi\"><!-- \u{1F600}\u{e9} --><osgi:service ref=\"x\" auto-export=\"bad\"/></beans>";
            SpringOsgi::default().line_markers(&doc("u.xml", text));
            let d = host.diagnostics()[&format!("{ROOT}resources/META-INF/spring/u.xml")].clone();
            let prefix = text.find("bad").unwrap();
            let utf16 = text[..prefix].encode_utf16().count() as u32;
            (d, utf16)
        };
        assert_eq!(found.0.len(), 1);
        assert_eq!((found.0[0].range.start.character, found.0[0].range.end.character), (found.1, found.1 + 3));
    }

    #[test]
    fn value_lines_are_right_in_crlf_files() {
        let host = setup();
        let text = "<beans xmlns:osgi=\"http://www.springframework.org/schema/osgi\">\r\n<osgi:service ref=\"x\"\r\n    auto-export=\"bad\"/>\r\n</beans>\r\n";
        SpringOsgi::default().line_markers(&doc("crlf.xml", text));
        let d = &host.diagnostics()[&format!("{ROOT}resources/META-INF/spring/crlf.xml")];
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].range.start.line, d[0].range.start.character, d[0].range.end.character), (2, 17, 20));
    }

    #[test]
    fn ignores_other_xml() {
        let host = setup();
        let mut p = SpringOsgi::default();
        let beans = r#"<beans xmlns="http://www.springframework.org/schema/beans"><bean id="a" ref="b"/></beans>"#;
        p.line_markers(&doc("plain.xml", beans));
        assert!(host.diagnostics().get(&format!("{ROOT}resources/META-INF/spring/plain.xml")).is_none_or(Vec::is_empty));
        assert!(p.completion(&doc("plain.xml", beans), after(beans, "ref=\""), None).is_empty());
    }

    const EMPTY_CONTEXT: &str = "<beans xmlns=\"http://www.springframework.org/schema/beans\"\n       xmlns:osgi=\"http://www.springframework.org/schema/osgi\">\n  <bean id=\"a\" class=\"A\"/>\n</beans>\n";

    fn generate(host: &MockHost, text: &str, answers: &[Option<&[&str]>]) -> Result<Option<String>, Error> {
        host.open_editor("/project/ctx.xml", "xml", text, &[(2, 2, 2, 2)]);
        for a in answers {
            host.answer_next_prompt(*a);
        }
        SpringOsgi::default().run_command("generate-osgi-bean", &host.invocation(), None)
    }

    #[test]
    fn generates_a_service_before_the_selected_line() {
        let host = MockHost::install();
        generate(&host, EMPTY_CONTEXT, &[Some(&["service"]), Some(&["com.acme.Api"]), Some(&["a"])]).unwrap();
        assert!(host.editor_text().contains("  <osgi:service ref=\"a\" interface=\"com.acme.Api\"/>\n  <bean id=\"a\""));
    }

    #[test]
    fn generates_a_reference() {
        let host = MockHost::install();
        generate(&host, EMPTY_CONTEXT, &[Some(&["reference"]), Some(&["com.acme.Greeter"])]).unwrap();
        assert!(host.editor_text().contains("<osgi:reference id=\"greeterReference\" interface=\"com.acme.Greeter\"/>"));
    }

    #[test]
    fn dismissed_prompts_leave_the_file_alone() {
        let host = MockHost::install();
        for answers in [vec![None], vec![Some(&["service"][..]), None], vec![Some(&["service"][..]), Some(&["X"][..]), None]] {
            assert_eq!(generate(&host, EMPTY_CONTEXT, &answers).unwrap(), None);
            assert_eq!(host.editor_text(), EMPTY_CONTEXT);
        }
    }

    #[test]
    fn refuses_a_file_without_the_osgi_namespace() {
        let host = MockHost::install();
        assert!(generate(&host, "<beans/>", &[]).is_err());
    }
}
