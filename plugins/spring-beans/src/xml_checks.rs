//! Checks on Spring XML configuration files. Quiet on anything that is not a
//! beans file, on other namespaces and on `${...}` placeholders.

use crate::World;
use crate::aop;
use crate::checks::{Groups, weaving_plugin_in_build};
use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::xml;

#[allow(dead_code)] // used by the later XML checks
const BEANS_NS: &str = "http://www.springframework.org/schema/beans";
const AOP_NS: &str = "http://www.springframework.org/schema/aop";
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
    out
}

fn is_beans_file(els: &[xml::Element]) -> bool {
    xml::root(els) == Some("beans")
}

fn in_namespace(els: &[xml::Element], i: usize, ns: &str) -> bool {
    xml::namespace(els, i) == Some(ns)
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
        (Some(a), Some(b)) if !a.value.contains("${") && !b.value.contains("${") => "An advice element takes either pointcut or pointcut-ref, not both.",
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
        let Some(attr) = attr.filter(|a| !a.value.contains("${")) else { continue };
        found.extend(aop::findings(&attr.value, attr.value_at));
    }
    if !found.is_empty() && !weaving_plugin_in_build(path) {
        out.extend(found);
    }
}
