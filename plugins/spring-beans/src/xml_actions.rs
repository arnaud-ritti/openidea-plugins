//! Refactorings on Spring XML configuration: p-namespace, inner beans and the
//! schema declaration. Edits are listed last position first, so they apply the
//! same one after another or all against the original text.

use crate::xml_checks::{BEANS_NS, dynamic, in_namespace, is_beans_file};
use openidea_plugin::TextEdit;
use spring_text::{decapitalize, range, xml};

const P_NS: &str = "http://www.springframework.org/schema/p";
const BEANS_XSD: &str = "https://www.springframework.org/schema/beans/spring-beans.xsd";

fn insert(text: &str, at: usize, new: String) -> TextEdit {
    TextEdit { range: range(text, at, at), text: new }
}

fn replace(text: &str, from: usize, to: usize, new: String) -> TextEdit {
    TextEdit { range: range(text, from, to), text: new }
}

fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// The whitespace before `at` when only whitespace separates it from the line start.
fn indent_of(text: &str, at: usize) -> &str {
    let line_start = text[..at].rfind('\n').map_or(0, |p| p + 1);
    let before = &text[line_start..at];
    if before.trim().is_empty() { before } else { "" }
}

/// The span of `from..to`, widened to its whole lines when nothing else shares them.
fn whole_lines(text: &str, from: usize, to: usize) -> (usize, usize) {
    let line_start = text[..from].rfind('\n').map_or(0, |p| p + 1);
    let line_end = text[to..].find('\n').map_or(text.len(), |p| to + p + 1);
    let alone = text[line_start..from].trim().is_empty() && text[to..line_end].trim().is_empty();
    if alone { (line_start, line_end) } else { (from, to) }
}

/// Where a closing `>` of the non-self-closing start tag `e` sits, if the tag is complete.
fn tag_close(text: &str, e: &xml::Element) -> Option<usize> {
    let at = e.tag_end.checked_sub(1)?;
    (text.as_bytes()[at] == b'>' && at > 0 && text.as_bytes()[at - 1] != b'/').then_some(at)
}

fn quote_of(text: &str, a: &xml::Attr) -> char {
    text[..a.value_at].chars().next_back().unwrap_or('"')
}

pub fn use_p_namespace(text: &str, caret: usize) -> Option<Vec<TextEdit>> {
    let els = xml::elements(text);
    if !is_beans_file(&els) {
        return None;
    }
    let at = xml::element_at(&els, caret)?;
    let property = &els[at];
    let bean_at = property.parent?;
    let bean = &els[bean_at];
    let beans_element = |i: usize, name: &str| els[i].local() == name && in_namespace(&els, i, BEANS_NS);
    if !beans_element(at, "property") || !beans_element(bean_at, "bean") {
        return None;
    }
    if !text[..property.end].ends_with("/>") || property.attrs.len() != 2 {
        return None;
    }
    let name = property.attrs.iter().find(|a| a.name == "name")?;
    let (suffix, injected) = match (property.attrs.iter().find(|a| a.name == "value"), property.attrs.iter().find(|a| a.name == "ref")) {
        (Some(value), None) => ("", value),
        (None, Some(reference)) => ("-ref", reference),
        _ => return None,
    };
    if name.value.is_empty() || !name.value.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let root = &els[0];
    let bound = root.attrs.iter().find(|a| a.value == P_NS).and_then(|a| a.name.strip_prefix("xmlns:"));
    if bound.is_none() && root.attr("xmlns:p").is_some_and(|a| a.name == "xmlns:p") {
        return None;
    }
    let prefix = bound.unwrap_or("p");
    let attribute = format!("{prefix}:{}", name.value);
    if bean.attrs.iter().any(|a| a.name == attribute || a.name == format!("{attribute}-ref")) {
        return None;
    }
    let close = tag_close(text, bean)?;
    let quote = quote_of(text, injected);
    let (from, to) = whole_lines(text, property.start, property.end);
    let mut edits = vec![
        replace(text, from, to, String::new()),
        insert(text, close, format!(" {attribute}{suffix}={quote}{}{quote}", injected.value)),
    ];
    if bound.is_none() {
        let after = match root.attrs.last() {
            Some(last) if text.as_bytes().get(last.value_end).is_some_and(|b| matches!(b, b'"' | b'\'')) => last.value_end + 1,
            Some(_) => return None,
            None => root.at + root.name.len(),
        };
        edits.push(insert(text, after, format!(" xmlns:p=\"{P_NS}\"")));
    }
    Some(edits)
}

fn taken_names(els: &[xml::Element]) -> Vec<String> {
    let mut taken = Vec::new();
    for attr in xml::bean_ids(els) {
        taken.extend(attr.value.split([',', ';', ' ']).filter(|n| !n.is_empty()).map(str::to_owned));
    }
    taken
}

pub fn introduce_bean(text: &str, caret: usize) -> Option<Vec<TextEdit>> {
    let els = xml::elements(text);
    if !is_beans_file(&els) {
        return None;
    }
    let at = xml::element_at(&els, caret)?;
    let inner = &els[at];
    let parent_at = inner.parent?;
    if inner.local() != "bean" || !in_namespace(&els, at, BEANS_NS) || inner.attr("id").is_some() {
        return None;
    }
    let class = inner.attr("class").map(|a| a.value.as_str()).filter(|c| !c.is_empty() && !dynamic(c))?;
    let closed = text[..inner.end].ends_with("/>") || text[..inner.end].ends_with(&format!("</{}>", inner.name));
    if !closed {
        return None;
    }
    let mut top = at;
    while let Some(p) = els[top].parent {
        if els[p].local() == "beans" && in_namespace(&els, p, BEANS_NS) {
            break;
        }
        top = p;
    }
    if top == at || els[top].parent.is_none() {
        return None;
    }
    let simple = decapitalize(class.rsplit(['.', '$']).next()?);
    let taken = taken_names(&els);
    let id = std::iter::once(simple.clone())
        .chain((2..).map(|n| format!("{simple}{n}")))
        .find(|candidate| !taken.contains(candidate))?;

    let eol = line_ending(text);
    let (own_indent, top_indent) = (indent_of(text, inner.start), indent_of(text, els[top].start));
    let with_id = format!("{} id=\"{id}\"{}", &text[inner.start..inner.at + inner.name.len()], &text[inner.at + inner.name.len()..inner.end]);
    let moved = with_id
        .split('\n')
        .enumerate()
        .map(|(n, line)| {
            let line = if n == 0 { line } else { line.strip_prefix(own_indent).unwrap_or(line) };
            if n == 0 || line.trim().is_empty() { line.to_owned() } else { format!("{top_indent}{line}") }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut edits = vec![insert(text, els[top].end, format!("{eol}{top_indent}{moved}"))];

    let parent = &els[parent_at];
    let attribute_parent = ["property", "constructor-arg"].contains(&parent.local())
        && in_namespace(&els, parent_at, BEANS_NS)
        && parent.attr("ref").is_none()
        && parent.attr("value").is_none();
    let tail_at = text[..parent.end].rfind("</").filter(|t| *t >= inner.end);
    let sole_child = text[parent.tag_end..inner.start].trim().is_empty() && tail_at.is_some_and(|t| text[inner.end..t].trim().is_empty());
    match tag_close(text, parent) {
        Some(close) if attribute_parent && sole_child => edits.push(replace(text, close, parent.end, format!(" ref=\"{id}\"/>"))),
        _ => edits.push(replace(text, inner.start, inner.end, format!("<ref bean=\"{id}\"/>"))),
    }
    Some(edits)
}

pub fn update_schema(text: &str) -> Option<Vec<TextEdit>> {
    let doctype_at = text.find("<!DOCTYPE")?;
    let prolog = text[..doctype_at].trim_start_matches('\u{feff}').trim();
    let prolog_ok = prolog.is_empty() || prolog.starts_with("<?xml") && prolog.ends_with("?>") && prolog.matches("<").count() == 1;
    let doctype_end = doctype_at + text[doctype_at..].find('>')? + 1;
    let doctype = &text[doctype_at..doctype_end];
    if !prolog_ok || !doctype.contains("DOCTYPE beans") || !doctype.contains("spring-beans") || doctype.contains('[') {
        return None;
    }
    let els = xml::elements(text);
    let root = els.first().filter(|r| r.name == "beans" && r.start >= doctype_end)?;
    if root.attrs.iter().any(|a| a.name == "xmlns") {
        return None;
    }
    let eol = line_ending(text);
    let indent = " ".repeat("<beans ".len());
    let declarations = format!(
        " xmlns=\"{BEANS_NS}\"{eol}{indent}xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"{eol}{indent}xsi:schemaLocation=\"{BEANS_NS} {BEANS_XSD}\""
    );
    let line_end = if text[doctype_end..].starts_with("\r\n") { doctype_end + 2 } else if text[doctype_end..].starts_with('\n') { doctype_end + 1 } else { doctype_end };
    Some(vec![
        insert(text, root.at + root.name.len(), declarations),
        replace(text, doctype_at, line_end, String::new()),
    ])
}
