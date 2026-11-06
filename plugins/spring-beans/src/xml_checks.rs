//! Checks on Spring XML configuration files. Quiet on anything that is not a
//! beans file, on other namespaces and on `${...}` placeholders.

use crate::aop;
use crate::checks::{Groups, capitalised, coexist, names_of, same_scope, weaving_plugin_in_build};
use crate::model::{is_xml_ref, name_tokens};
use crate::{Found, World};
use openidea_plugin::TextEdit;
use openidea_plugin::text::offset;
use openidea_plugin::{Position, Severity};
use spring_text::finding::Finding;
use spring_text::range;
use spring_text::xml;
use spring_text::xml_check::{self, Rule};
use std::collections::{BTreeMap, BTreeSet};

pub const BEANS_NS: &str = "http://www.springframework.org/schema/beans";
const AOP_NS: &str = "http://www.springframework.org/schema/aop";
pub const KNOWN_SCOPES: [&str; 7] = ["singleton", "prototype", "request", "session", "application", "websocket", "bundle"];
const ADVICE: [&str; 5] = ["before", "after", "after-returning", "after-throwing", "around"];

pub fn check_xml(path: &str, text: &str, world: &World, groups: Groups) -> Vec<Finding> {
    let els = xml::elements(text);
    if !is_beans_file(&els) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let by_name = if groups.duplicate_names || groups.xml || groups.unresolved_references { beans_by_name(world) } else { ByName::new() };
    let children = children_of(&els);
    if groups.aop {
        aop_structure(&els, &mut out);
        pointcut_designators(path, &els, &mut out);
    }
    if groups.naming {
        name_conventions(&els, &mut out);
    }
    if groups.duplicate_names {
        duplicate_names(path, &els, &by_name, &mut out);
    }
    if groups.xml {
        out.extend(xml_check::check(&els, SCAN_RULES, |_| true));
        scan_filters(&els, &children, &mut out);
        import_resources(path, &els, groups, &mut out);
        inactive_profiles(&els, &mut out);
    }
    if groups.unresolved_references && world.complete {
        unresolved_references(&els, &by_name, &mut out);
        unresolved_placeholders(path, &els, &mut out);
    }
    if groups.xml {
        abstract_references(&els, &by_name, &mut out);
        bean_attributes(&els, &mut out);
        injection_values(&els, &children, &mut out);
        bean_scopes(&els, &mut out);
        value_style(text, &els, &children, &mut out);
    }
    out
}

const FILTER_TYPES: &[&str] = &["annotation", "assignable", "aspectj", "regex", "custom"];
const FILTER_RULE: Rule = Rule { required: &["type", "expression"], enums: &[("type", FILTER_TYPES)], ..Rule::new("include-filter") };
const SCAN_RULES: &[Rule] = &[
    Rule { required: &["base-package"], ..Rule::new("component-scan") },
    FILTER_RULE,
    Rule { required: &["type", "expression"], enums: &[("type", FILTER_TYPES)], ..Rule::new("exclude-filter") },
];
const REF_RULES: &[Rule] = &[
    Rule { refs: &["ref"], ..Rule::new("property") },
    Rule { refs: &["ref"], ..Rule::new("constructor-arg") },
    Rule { refs: &["parent", "factory-bean"], ..Rule::new("bean") },
    Rule { refs: &["bean"], ..Rule::new("ref") },
    Rule { refs: &["bean"], ..Rule::new("idref") },
    Rule { refs: &["key-ref", "value-ref"], ..Rule::new("entry") },
    Rule { refs: &["name"], ..Rule::new("alias") },
];
const CONTAINER_BEANS: [&str; 6] = ["environment", "systemProperties", "systemEnvironment", "messageSource", "applicationEventMulticaster", "lifecycleProcessor"];

/// The project's beans under every name they answer to, as `World::named` finds them.
type ByName<'a> = BTreeMap<&'a str, Vec<Found<'a>>>;

fn beans_by_name<'a>(world: &World<'a>) -> ByName<'a> {
    let mut map = ByName::new();
    for (path, bean) in world.beans() {
        let keys: BTreeSet<&str> = names_of(bean).chain(bean.qualifiers.iter().map(String::as_str)).collect();
        for key in keys {
            map.entry(key).or_default().push((path, bean));
        }
    }
    map
}

/// The child indices of each element, in document order.
fn children_of(els: &[xml::Element]) -> Vec<Vec<usize>> {
    let mut children = vec![Vec::new(); els.len()];
    for (i, e) in els.iter().enumerate() {
        if let Some(p) = e.parent {
            children[p].push(i);
        }
    }
    children
}

pub(crate) fn is_beans_file(els: &[xml::Element]) -> bool {
    xml::root(els) == Some("beans")
}

pub(crate) fn in_namespace(els: &[xml::Element], i: usize, ns: &str) -> bool {
    xml::namespace(els, i) == Some(ns)
}

pub(crate) fn dynamic(value: &str) -> bool {
    value.contains("${") || value.contains("#{")
}

fn inside_aop_config(els: &[xml::Element], i: usize) -> bool {
    let mut at = els[i].parent;
    while let Some(p) = at {
        if els[p].local() == "config" && in_namespace(els, p, AOP_NS) {
            return true;
        }
        at = els[p].parent;
    }
    false
}

fn aop_structure(els: &[xml::Element], out: &mut Vec<Finding>) {
    for (i, e) in els.iter().enumerate() {
        if !in_namespace(els, i, AOP_NS) || !inside_aop_config(els, i) {
            continue;
        }
        let missing = |attr: &str| e.attr(attr).is_none();
        let problem = match e.local() {
            "pointcut" if missing("id") || missing("expression") => "An aop:pointcut needs both an id and an expression.",
            "aspect" if missing("ref") => "An aop:aspect needs a ref naming the bean that holds the advice methods.",
            "advisor" if missing("advice-ref") => "An aop:advisor needs an advice-ref naming the advice bean.",
            name if ADVICE.contains(&name) => advice_problem(e),
            _ => continue,
        };
        if problem.is_empty() {
            continue;
        }
        out.push(Finding::new(e.at, e.at + e.name.len(), "aop-structure", problem).with_severity(Severity::Error));
    }
}

fn advice_problem(e: &xml::Element) -> &'static str {
    if e.attr("method").is_none() {
        return "An advice element needs a method naming the aspect method to call.";
    }
    let (pointcut, reference) = (e.attr("pointcut"), e.attr("pointcut-ref"));
    match (pointcut, reference) {
        (None, None) => "An advice element needs a pointcut expression or a pointcut-ref.",
        (Some(a), Some(b)) if !dynamic(&a.value) && !dynamic(&b.value) => "An advice element takes either pointcut or pointcut-ref, not both.",
        _ => "",
    }
}

fn pointcut_designators(path: &str, els: &[xml::Element], out: &mut Vec<Finding>) {
    let mut found = Vec::new();
    for (i, e) in els.iter().enumerate() {
        if !in_namespace(els, i, AOP_NS) || !inside_aop_config(els, i) {
            continue;
        }
        let attr = match e.local() {
            "pointcut" => e.attr("expression"),
            name if ADVICE.contains(&name) => e.attr("pointcut"),
            _ => None,
        };
        let Some(attr) = attr.filter(|a| !dynamic(&a.value)) else { continue };
        found.extend(aop::findings(&attr.value, attr.value_at));
    }
    if !found.is_empty() && !weaving_plugin_in_build(path) {
        out.extend(found);
    }
}

fn error(from: usize, to: usize, code: &'static str, message: &str) -> Finding {
    Finding::new(from, to, code, message).with_severity(Severity::Error)
}

fn beans_elements<'a>(els: &'a [xml::Element], name: &'a str) -> impl Iterator<Item = (usize, &'a xml::Element)> {
    els.iter().enumerate().filter(move |(i, e)| e.local() == name && in_namespace(els, *i, BEANS_NS))
}

/// The explicit, literal names of a `<bean>`: `(name, start, end)` of each `id` or `name` token.
fn bean_names(e: &xml::Element) -> Vec<(&str, usize, usize)> {
    let mut found = Vec::new();
    for attr in e.attrs.iter().filter(|a| matches!(a.name.as_str(), "id" | "name")) {
        if dynamic(&attr.value) {
            continue;
        }
        for (at, name) in name_tokens(&attr.value).filter(|(_, n)| !n.contains('#')) {
            let start = attr.value_at + at;
            found.push((name, start, start + name.len()));
        }
    }
    found
}

fn name_conventions(els: &[xml::Element], out: &mut Vec<Finding>) {
    for (_, e) in beans_elements(els, "bean") {
        for (name, from, to) in bean_names(e).into_iter().filter(|(n, _, _)| capitalised(n)) {
            let message = format!(
                "Bean names conventionally start with a lowercase letter (\"{}\"), as the default names Spring derives do.",
                spring_text::decapitalize(name)
            );
            out.push(Finding::new(from, to, "bean-name-convention", message).with_severity(Severity::Hint));
        }
    }
}

fn profiles_of(els: &[xml::Element], i: usize) -> Vec<String> {
    let mut profiles = Vec::new();
    let mut up = els[i].parent;
    while let Some(p) = up {
        if let Some(profile) = els[p].value("profile") {
            profiles.extend(name_tokens(profile).map(|(_, p)| p.to_owned()));
        }
        up = els[p].parent;
    }
    profiles
}

/// Same-file clashes inside one `<beans>` element, and clashes with beans declared in code.
fn duplicate_names(path: &str, els: &[xml::Element], by_name: &ByName, out: &mut Vec<Finding>) {
    let mut seen: Vec<(Option<usize>, &str)> = Vec::new();
    for (i, e) in beans_elements(els, "bean").filter(|(_, e)| e.parent.is_some_and(|p| els[p].local() == "beans")) {
        let profiles = profiles_of(els, i);
        let mut own = BTreeSet::new();
        for (name, from, to) in bean_names(e).into_iter().filter(|(n, _, _)| own.insert(*n)) {
            if seen.contains(&(e.parent, name)) {
                let message = format!("Another bean in this file is already named \"{name}\": a second definition in one beans element is rejected.");
                out.push(Finding::new(from, to, "duplicate-bean-name", message));
                continue;
            }
            seen.push((e.parent, name));
            let clash = by_name.get(name).into_iter().flatten().find(|(other_path, other)| {
                !other.kind.starts_with('<')
                    && !other.conditional
                    && names_of(other).any(|n| n == name)
                    && coexist(&profiles, &other.profiles)
                    && same_scope(path, other_path)
            });
            if let Some(&(other_path, _)) = clash {
                let message = format!(
                    "Another bean named \"{name}\" is defined in {}: two beans with one name make the context fail or silently replace one.",
                    crate::file_name(other_path)
                );
                out.push(Finding::new(from, to, "duplicate-bean-name", message));
            }
        }
    }
}

fn abstract_references(els: &[xml::Element], by_name: &ByName, out: &mut Vec<Finding>) {
    for (i, e) in els.iter().enumerate() {
        for a in e.attrs.iter().filter(|a| a.local() != "parent" && !dynamic(&a.value) && is_xml_ref(els, i, a)) {
            for (at, name) in name_tokens(&a.value) {
                let start = a.value_at + at;
                let Some(defined) = by_name.get(name) else { continue };
                if !defined.iter().all(|(_, b)| b.is_abstract) {
                    continue;
                }
                let message = format!("\"{name}\" is an abstract bean and cannot be injected or depended on; reference it as a parent instead.");
                out.push(error(start, start + name.len(), "xml-abstract-reference", &message));
            }
        }
    }
}

fn bean_attributes(els: &[xml::Element], out: &mut Vec<Finding>) {
    for (_, e) in beans_elements(els, "bean") {
        if e.attrs.iter().any(|a| dynamic(&a.value)) {
            continue;
        }
        let factory_bean = e.attr("factory-bean");
        if let (Some(_), Some(factory)) = (e.attr("class"), factory_bean) {
            let message = "A bean with a factory-bean should leave class out: Spring ignores it and takes the type from the factory method.";
            out.push(Finding::new(factory.at, factory.at + factory.name.len(), "xml-bean-attributes", message).with_severity(Severity::Warning));
        } else if let (Some(factory), None) = (factory_bean, e.attr("factory-method")) {
            out.push(error(factory.at, factory.at + factory.name.len(), "xml-bean-attributes", "A factory-bean needs a factory-method naming the method to call."));
        }
        let defines_type = ["class", "parent", "factory-bean"].iter().any(|n| e.attr(n).is_some());
        if !defines_type && e.value("abstract") != Some("true") {
            out.push(error(e.at, e.at + e.name.len(), "xml-bean-attributes", "A bean needs a class, a parent or a factory-bean, unless it is abstract."));
        }
    }
}

fn injection_values(els: &[xml::Element], children: &[Vec<usize>], out: &mut Vec<Finding>) {
    for name in ["property", "constructor-arg"] {
        for (i, e) in beans_elements(els, name) {
            let (reference, value) = (e.attr("ref"), e.attr("value"));
            if [reference, value].iter().flatten().any(|a| dynamic(&a.value)) {
                continue;
            }
            let has_child = children[i].iter().any(|&c| !matches!(els[c].local(), "description" | "meta"));
            let problem = match (reference.is_some(), value.is_some(), has_child) {
                (true, true, _) => "Give either a ref or a value, not both.",
                (true, false, true) | (false, true, true) => "A ref or value attribute cannot be combined with a nested element.",
                (false, false, false) if name == "property" => "A property needs a value, a ref or a nested value element.",
                _ => continue,
            };
            out.push(error(e.at, e.at + e.name.len(), "xml-injection-value", problem));
        }
    }
}

fn bean_scopes(els: &[xml::Element], out: &mut Vec<Finding>) {
    for (_, e) in beans_elements(els, "bean") {
        let Some(scope) = e.attr("scope").filter(|a| !dynamic(&a.value)) else { continue };
        if !misspelled_scope(&scope.value) {
            continue;
        }
        let message = format!("`{}` looks like a misspelled scope; the standard scopes are {}.", scope.value, KNOWN_SCOPES.join(", "));
        out.push(Finding::new(scope.value_at, scope.value_end, "xml-scope", message).with_severity(Severity::Warning));
    }
}

fn misspelled_scope(value: &str) -> bool {
    if KNOWN_SCOPES.contains(&value) {
        return false;
    }
    KNOWN_SCOPES.iter().any(|known| value.eq_ignore_ascii_case(known) || one_edit_apart(value, known))
}

fn one_edit_apart(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (short, long) = if a.len() <= b.len() { (&a, &b) } else { (&b, &a) };
    if long.len() - short.len() > 1 {
        return false;
    }
    let prefix = short.iter().zip(long.iter()).take_while(|(x, y)| x == y).count();
    let short_rest = if short.len() == long.len() { prefix + 1 } else { prefix };
    short.get(short_rest..).unwrap_or_default() == long.get(prefix + 1..).unwrap_or_default()
}

fn value_style(text: &str, els: &[xml::Element], children: &[Vec<usize>], out: &mut Vec<Finding>) {
    for (parent, child) in style_candidates(text, els, children) {
        let c = &els[child];
        let hint = format!("This nested {} can be written as an attribute of the {}.", c.local(), els[parent].local());
        out.push(Finding::new(c.at, c.at + c.name.len(), "xml-value-style", hint).with_severity(Severity::Hint));
    }
}

/// `(parent, child)` pairs where the only child holds a plain literal or bean name.
fn style_candidates(text: &str, els: &[xml::Element], children: &[Vec<usize>]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for name in ["property", "constructor-arg"] {
        for (i, parent) in beans_elements(els, name) {
            if parent.attr("value").is_some() || parent.attr("ref").is_some() || text[..parent.tag_end].ends_with("/>") {
                continue;
            }
            let &[child] = children[i].as_slice() else { continue };
            if style_attribute(text, els, child, parent).is_some() {
                found.push((i, child));
            }
        }
    }
    found
}

/// The attribute (`value="t"` or `ref="x"`) that replaces `child`, when the swap loses nothing.
fn style_attribute(text: &str, els: &[xml::Element], child: usize, parent: &xml::Element) -> Option<String> {
    let c = &els[child];
    if !in_namespace(els, child, BEANS_NS) || !text[parent.tag_end..c.start].trim().is_empty() {
        return None;
    }
    let tail_at = text[..parent.end].rfind("</").filter(|at| *at >= c.end)?;
    if !text[c.end..tail_at].trim().is_empty() {
        return None;
    }
    match c.local() {
        "value" if c.attrs.is_empty() => {
            let literal = text[c.tag_end..c.end].strip_suffix('>')?.rsplit_once("</")?.0;
            let plain = !literal.contains(['<', '"', '\n', '\r', '\t']) && !dynamic(literal);
            (plain && c.tag_end < c.end).then(|| format!(" value=\"{literal}\"/>"))
        }
        "ref" if c.tag_end == c.end && c.attrs.len() == 1 => {
            let bean = c.attr("bean")?;
            (!bean.value.contains('"') && !dynamic(&bean.value)).then(|| format!(" ref=\"{}\"/>", bean.value))
        }
        _ => None,
    }
}

/// The edit turning the single `<value>` or `<ref>` child of `property` into an attribute.
#[cfg(test)]
pub fn attribute_style_edit(text: &str, property: &xml::Element, els: &[xml::Element]) -> Option<TextEdit> {
    let parent = els.iter().position(|e| e.start == property.start)?;
    let (_, child) = style_candidates(text, els, &children_of(els)).into_iter().find(|(p, _)| *p == parent)?;
    style_edit(text, els, parent, child)
}

fn style_edit(text: &str, els: &[xml::Element], parent: usize, child: usize) -> Option<TextEdit> {
    let property = &els[parent];
    if text.as_bytes().get(property.tag_end.checked_sub(1)?) != Some(&b'>') {
        return None;
    }
    let replacement = style_attribute(text, els, child, property)?;
    Some(TextEdit { range: range(text, property.tag_end - 1, property.end), text: replacement })
}

/// The quick fix for a style hint under `cursor`: its title and edit.
pub fn style_fix(text: &str, cursor: Position) -> Option<(&'static str, TextEdit)> {
    let at = offset(text, cursor)?;
    let els = xml::elements(text);
    if !is_beans_file(&els) {
        return None;
    }
    let children = children_of(&els);
    let (parent, child) = style_candidates(text, &els, &children)
        .into_iter()
        .find(|(_, c)| els[*c].at <= at && at <= els[*c].at + els[*c].name.len())?;
    let title = if els[child].local() == "value" { "Use the value attribute" } else { "Use the ref attribute" };
    Some((title, style_edit(text, &els, parent, child)?))
}

fn scan_filters(els: &[xml::Element], children: &[Vec<usize>], out: &mut Vec<Finding>) {
    for (i, e) in els.iter().enumerate().filter(|(_, e)| e.local() == "component-scan") {
        let filtered = children[i].iter().any(|&c| els[c].local() == "include-filter");
        if e.value("use-default-filters") == Some("false") && !filtered {
            let message = "use-default-filters=\"false\" without an include-filter: this scan registers no component.";
            out.push(Finding::new(e.at, e.at + e.name.len(), "xml-component-scan-filters", message));
        }
    }
}

/// Nothing is reported for a file that imports others: the bean may be defined there.
fn unresolved_references(els: &[xml::Element], by_name: &ByName, out: &mut Vec<Finding>) {
    if beans_elements(els, "import").next().is_some() {
        return;
    }
    let known = |name: &str| by_name.contains_key(name) || CONTAINER_BEANS.contains(&name);
    out.extend(xml_check::check(els, REF_RULES, known).into_iter().map(|f| f.with_severity(Severity::Info)));
    for (_, e) in beans_elements(els, "bean") {
        let Some(attr) = e.attr("depends-on").filter(|a| !dynamic(&a.value)) else { continue };
        for (at, name) in name_tokens(&attr.value).filter(|(_, n)| !known(n)) {
            let start = attr.value_at + at;
            let message = format!("No bean named \"{name}\" is defined in the project.");
            out.push(Finding::new(start, start + name.len(), "unresolved-reference", message).with_severity(Severity::Info));
        }
    }
}

fn import_resources(path: &str, els: &[xml::Element], groups: Groups, out: &mut Vec<Finding>) {
    for (_, e) in beans_elements(els, "import") {
        let Some(attr) = e.attr("resource").filter(|a| !dynamic(&a.value) && !a.value.contains(['*', '?'])) else { continue };
        let location = attr.value.trim();
        let missing = if let Some(rest) = location.strip_prefix("classpath:") {
            groups.unresolved_references && matches!(crate::target_checks::read_classpath(path, rest.trim_start_matches('/')), Some(None))
        } else if location.contains(':') || location.starts_with('/') {
            false
        } else {
            matches!(sibling_exists(path, location), Some(false))
        };
        if missing {
            let message = format!("No file \"{location}\" to import. A build step can still generate it.");
            out.push(Finding::new(attr.value_at, attr.value_end, "xml-import-missing", message));
        }
    }
}

/// Whether `relative` exists next to `path`: in the same classpath folder for a resource, else on disk. None when it escapes the project.
fn sibling_exists(path: &str, relative: &str) -> Option<bool> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    for part in relative.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    let joined = parts.join("/");
    for marker in ["/src/main/resources/", "/src/test/resources/"] {
        if let Some((_, tail)) = joined.split_once(marker) {
            return crate::target_checks::read_classpath(path, tail).map(|found| found.is_some());
        }
    }
    let project = spring_text::index::project_of(path)?;
    Some(project.read_file(&joined).is_ok())
}

fn inactive_profiles(els: &[xml::Element], out: &mut Vec<Finding>) {
    let active = crate::commands::active_profiles();
    if active.is_empty() {
        return;
    }
    for (_, e) in beans_elements(els, "beans").filter(|(_, e)| e.parent.is_some()) {
        let Some(attr) = e.attr("profile").filter(|a| !dynamic(&a.value)) else { continue };
        let profiles: Vec<String> = name_tokens(&attr.value).map(|(_, p)| p.to_owned()).collect();
        if profiles.is_empty() || crate::checks::enabled(&profiles, &active) {
            continue;
        }
        let message = format!("Profile {} is not among the active profiles (Change Active Spring Profiles).", profiles.join(", "));
        out.push(Finding::new(attr.value_at, attr.value_end, "xml-inactive-profile", message).with_severity(Severity::Hint));
    }
}

/// `${key}` placeholders no file of the one `<context:property-placeholder>` defines. System properties and
/// the environment also resolve them, hence Info; quiet unless every location is a readable classpath file.
fn unresolved_placeholders(path: &str, els: &[xml::Element], out: &mut Vec<Finding>) {
    let mut holders = els.iter().filter(|e| e.local() == "property-placeholder");
    let (Some(holder), None) = (holders.next(), holders.next()) else { return };
    let configurer = els.iter().any(|e| e.value("class").is_some_and(|c| c.ends_with("PlaceholderConfigurer")));
    let unusual = ["properties-ref", "ignore-unresolvable", "local-override", "system-properties-mode"].iter().any(|a| holder.attr(a).is_some());
    let Some(location) = holder.value("location").filter(|l| !dynamic(l)) else { return };
    if configurer || unusual {
        return;
    }
    let mut keys = BTreeSet::new();
    for file in name_tokens(location).map(|(_, f)| f) {
        let Some(relative) = file.strip_prefix("classpath:") else { return };
        let Some(Some(text)) = crate::target_checks::read_classpath(path, relative.trim_start_matches('/')) else { return };
        keys.extend(property_keys(&text));
    }
    for attr in els.iter().flat_map(|e| e.attrs.iter()).filter(|a| !a.name.starts_with("xmlns")) {
        let mut rest = attr.value.as_str();
        while let Some(open) = rest.find("${") {
            let Some(close) = rest[open..].find('}') else { break };
            let key = &rest[open + 2..open + close];
            let at = attr.value_at + (attr.value.len() - rest.len()) + open + 2;
            rest = &rest[open + close + 1..];
            if key.is_empty() || key.contains(['$', '{', ':', '#']) || key.starts_with("env.") || keys.contains(key) {
                continue;
            }
            let message = format!("\"{key}\" is not defined in {location}. System properties and the environment can still provide it.");
            out.push(Finding::new(at, at + key.len(), "xml-placeholder-unresolved", message).with_severity(Severity::Info));
        }
    }
}

fn property_keys(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().map(str::trim_start).filter(|l| !l.is_empty() && !l.starts_with(['#', '!'])).map(|line| {
        let end = line.find(['=', ':']).into_iter().chain(line.find(char::is_whitespace)).min().unwrap_or(line.len());
        line[..end].trim_end().to_owned()
    })
}
