//! Checks on Spring XML configuration files. Quiet on anything that is not a
//! beans file, on other namespaces and on `${...}` placeholders.

use crate::World;
use crate::aop;
use crate::checks::{Groups, weaving_plugin_in_build};
use openidea_plugin::TextEdit;
use openidea_plugin::text::offset;
use openidea_plugin::{Position, Severity};
use spring_text::finding::Finding;
use spring_text::range;
use spring_text::xml;

pub const BEANS_NS: &str = "http://www.springframework.org/schema/beans";
const AOP_NS: &str = "http://www.springframework.org/schema/aop";
pub const KNOWN_SCOPES: [&str; 7] = ["singleton", "prototype", "request", "session", "application", "websocket", "bundle"];
const ADVICE: [&str; 5] = ["before", "after", "after-returning", "after-throwing", "around"];

pub fn check_xml(path: &str, text: &str, _world: &World, groups: Groups) -> Vec<Finding> {
    let els = xml::elements(text);
    if !is_beans_file(&els) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if groups.aop {
        aop_structure(&els, &mut out);
        pointcut_designators(path, &els, &mut out);
    }
    if groups.xml {
        bean_attributes(&els, &mut out);
        injection_values(&els, &mut out);
        bean_scopes(&els, &mut out);
        value_style(text, &els, &mut out);
    }
    out
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

fn bean_attributes(els: &[xml::Element], out: &mut Vec<Finding>) {
    for (_, e) in beans_elements(els, "bean") {
        if e.attrs.iter().any(|a| dynamic(&a.value)) {
            continue;
        }
        let factory_bean = e.attr("factory-bean");
        if let (Some(_), Some(factory)) = (e.attr("class"), factory_bean) {
            out.push(error(factory.at, factory.at + factory.name.len(), "xml-bean-attributes", "A bean takes either a class or a factory-bean, not both."));
        } else if let (Some(factory), None) = (factory_bean, e.attr("factory-method")) {
            out.push(error(factory.at, factory.at + factory.name.len(), "xml-bean-attributes", "A factory-bean needs a factory-method naming the method to call."));
        }
        let defines_type = ["class", "parent", "factory-bean"].iter().any(|n| e.attr(n).is_some());
        if !defines_type && e.value("abstract") != Some("true") {
            out.push(error(e.at, e.at + e.name.len(), "xml-bean-attributes", "A bean needs a class, a parent or a factory-bean, unless it is abstract."));
        }
    }
}

fn injection_values(els: &[xml::Element], out: &mut Vec<Finding>) {
    for name in ["property", "constructor-arg"] {
        for (i, e) in beans_elements(els, name) {
            let (reference, value) = (e.attr("ref"), e.attr("value"));
            if [reference, value].iter().flatten().any(|a| dynamic(&a.value)) {
                continue;
            }
            let has_child = els.iter().any(|c| c.parent == Some(i) && !matches!(c.local(), "description" | "meta"));
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

fn value_style(text: &str, els: &[xml::Element], out: &mut Vec<Finding>) {
    for (parent, child) in style_candidates(text, els) {
        let c = &els[child];
        let hint = format!("This nested {} can be written as an attribute of the {}.", c.local(), els[parent].local());
        out.push(Finding::new(c.at, c.at + c.name.len(), "xml-value-style", hint).with_severity(Severity::Hint));
    }
}

/// `(parent, child)` pairs where the only child holds a plain literal or bean name.
fn style_candidates(text: &str, els: &[xml::Element]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    for name in ["property", "constructor-arg"] {
        for (i, parent) in beans_elements(els, name) {
            if parent.attr("value").is_some() || parent.attr("ref").is_some() || text[..parent.tag_end].ends_with("/>") {
                continue;
            }
            let mut children = els.iter().enumerate().filter(|(_, c)| c.parent == Some(i));
            let (Some((child, _)), None) = (children.next(), children.next()) else { continue };
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
pub fn attribute_style_edit(text: &str, property: &xml::Element, els: &[xml::Element]) -> Option<TextEdit> {
    if text.as_bytes().get(property.tag_end.checked_sub(1)?) != Some(&b'>') {
        return None;
    }
    let parent = els.iter().position(|e| e.start == property.start)?;
    let (_, child) = style_candidates(text, els).into_iter().find(|(p, _)| *p == parent)?;
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
    let (parent, child) = style_candidates(text, &els)
        .into_iter()
        .find(|(_, c)| els[*c].at <= at && at <= els[*c].at + els[*c].name.len())?;
    let title = if els[child].local() == "value" { "Use the value attribute" } else { "Use the ref attribute" };
    Some((title, attribute_style_edit(text, &els[parent], &els)?))
}
