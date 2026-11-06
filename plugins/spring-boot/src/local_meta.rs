//! Checks on a project's `(additional-)spring-configuration-metadata.json` and
//! the edit that defines a missing key in it. Types are judged by their shape
//! only: classes of libraries are invisible.

use openidea_plugin::Severity;
use spring_text::finding::Finding;
use std::collections::BTreeSet;

pub const FILE_NAME: &str = "additional-spring-configuration-metadata.json";

pub fn is_metadata_file(path: &str) -> bool {
    path.ends_with(FILE_NAME) || path.ends_with("/spring-configuration-metadata.json")
}

const TOP_LEVEL: &[&str] = &["groups", "properties", "hints", "ignored"];
const PROPERTY_FIELDS: &[&str] = &["name", "type", "description", "sourceType", "sourceMethod", "defaultValue", "deprecation"];
const GROUP_FIELDS: &[&str] = &["name", "type", "description", "sourceType", "sourceMethod"];
const HINT_FIELDS: &[&str] = &["name", "values", "providers"];

#[derive(Debug)]
enum Node {
    Str { at: usize, end: usize, value: String },
    Obj(Vec<Member>),
    Arr(Vec<Node>),
    Other,
}

#[derive(Debug)]
struct Member {
    key: String,
    at: usize,
    end: usize,
    value: Node,
}

struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl Scanner<'_> {
    fn skip_blanks(&mut self) {
        while self.text[self.at..].starts_with(|c: char| c.is_whitespace()) {
            self.at += self.text[self.at..].chars().next().map_or(1, char::len_utf8);
        }
    }

    fn string(&mut self) -> Option<(usize, usize, String)> {
        let start = self.at;
        let bytes = self.text.as_bytes();
        if bytes.get(start) != Some(&b'"') {
            return None;
        }
        let mut i = start + 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    self.at = i + 1;
                    return Some((start, i + 1, self.text[start + 1..i].to_owned()));
                }
                _ => i += 1,
            }
        }
        None
    }

    fn value(&mut self, depth: usize) -> Option<Node> {
        if depth > 12 {
            return None;
        }
        self.skip_blanks();
        match self.text.as_bytes().get(self.at)? {
            b'"' => self.string().map(|(at, end, value)| Node::Str { at, end, value }),
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                loop {
                    self.skip_blanks();
                    match self.text.as_bytes().get(self.at)? {
                        b'}' => break,
                        b',' => self.at += 1,
                        _ => {
                            let (at, end, key) = self.string()?;
                            self.skip_blanks();
                            if self.text.as_bytes().get(self.at) != Some(&b':') {
                                return None;
                            }
                            self.at += 1;
                            members.push(Member { key, at, end, value: self.value(depth + 1)? });
                        }
                    }
                }
                self.at += 1;
                Some(Node::Obj(members))
            }
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_blanks();
                    match self.text.as_bytes().get(self.at)? {
                        b']' => break,
                        b',' => self.at += 1,
                        _ => items.push(self.value(depth + 1)?),
                    }
                }
                self.at += 1;
                Some(Node::Arr(items))
            }
            _ => {
                while self.text[self.at..].starts_with(|c: char| !matches!(c, ',' | '}' | ']') && !c.is_whitespace()) {
                    self.at += 1;
                }
                Some(Node::Other)
            }
        }
    }
}

fn parse(text: &str) -> Option<Vec<Member>> {
    match (Scanner { text, at: 0 }).value(0)? {
        Node::Obj(members) => Some(members),
        _ => None,
    }
}

fn valid_type(ty: &str) -> bool {
    let first_ok = ty.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$');
    let chars_ok = ty.chars().all(|c| c.is_alphanumeric() || "_$.<>,?[] ".contains(c));
    let mut depth = 0i32;
    let balanced = ty.chars().all(|c| {
        depth += i32::from(c == '<') - i32::from(c == '>');
        depth >= 0
    }) && depth == 0;
    first_ok && chars_ok && balanced && !ty.contains("..") && !ty.ends_with('.')
}

fn string_of<'n>(members: &'n [Member], key: &str) -> Option<(&'n Member, &'n str)> {
    members.iter().find(|m| m.key == key).and_then(|m| match &m.value {
        Node::Str { value, .. } => Some((m, value.as_str())),
        _ => None,
    })
}

fn entries<'n>(top: &'n [Member], list: &str) -> impl Iterator<Item = &'n [Member]> {
    top.iter().filter(move |m| m.key == list).filter_map(|m| match &m.value {
        Node::Arr(items) => Some(items),
        _ => None,
    }).flatten().filter_map(|n| match n {
        Node::Obj(members) => Some(members.as_slice()),
        _ => None,
    })
}

fn unknown_fields(members: &[Member], allowed: &[&str], what: &str, out: &mut Vec<Finding>) {
    for m in members.iter().filter(|m| !allowed.contains(&m.key.as_str())) {
        out.push(Finding::new(m.at, m.end, "unknown-metadata-field", format!("`{}` is not a field of a metadata {what}.", m.key)));
    }
}

pub fn check(text: &str) -> Vec<Finding> {
    let Some(top) = parse(text) else { return Vec::new() };
    let mut out = Vec::new();
    for m in top.iter().filter(|m| !TOP_LEVEL.contains(&m.key.as_str())) {
        out.push(Finding::new(m.at, m.end, "unknown-metadata-field", format!("`{}` is not a section of a metadata file.", m.key)));
    }
    let mut seen = BTreeSet::new();
    for property in entries(&top, "properties") {
        unknown_fields(property, PROPERTY_FIELDS, "property", &mut out);
        if let Some((name, value)) = string_of(property, "name")
            && !seen.insert(value.to_owned())
        {
            out.push(Finding::new(value_span(name).0, value_span(name).1, "duplicate-metadata-name", format!("`{value}` is defined twice.")));
        }
        if let Some((member, ty)) = string_of(property, "type")
            && !valid_type(ty)
        {
            let (at, end) = value_span(member);
            out.push(Finding::new(at, end, "invalid-metadata-type", format!("`{ty}` is not a Java type name.")));
        }
        if let Some((name, _)) = string_of(property, "name")
            && !property.iter().any(|m| m.key == "description")
            && !property.iter().any(|m| m.key == "deprecation")
        {
            let (at, end) = value_span(name);
            out.push(Finding::new(at, end, "missing-metadata-description", "The property has no description.").with_severity(Severity::Hint));
        }
    }
    for group in entries(&top, "groups") {
        unknown_fields(group, GROUP_FIELDS, "group", &mut out);
    }
    for hint in entries(&top, "hints") {
        unknown_fields(hint, HINT_FIELDS, "hint", &mut out);
    }
    out
}

fn value_span(m: &Member) -> (usize, usize) {
    match &m.value {
        Node::Str { at, end, .. } => (*at, *end),
        _ => (m.at, m.end),
    }
}

/// The edit that adds a string property `key` to the metadata file `text`; `None` when the file is not the usual shape.
pub fn define_key(text: &str, key: &str) -> Option<(usize, usize, String)> {
    let top = parse(text)?;
    let entry = format!("{{\n      \"name\": \"{key}\",\n      \"type\": \"java.lang.String\",\n      \"description\": \"\"\n    }}");
    let Some(list) = top.iter().find(|m| m.key == "properties") else {
        let close = text.rfind('}')?;
        let sep = if top.is_empty() { "" } else { "," };
        let before = text[..close].trim_end().len();
        return Some((before, close, format!("{sep}\n  \"properties\": [\n    {entry}\n  ]\n")));
    };
    let Node::Arr(items) = &list.value else { return None };
    let open = text[list.end..].find('[')? + list.end;
    let close = matching_bracket(text, open)?;
    let before = text[..close].trim_end().len();
    let sep = if items.is_empty() { "" } else { "," };
    Some((before, close, format!("{sep}\n    {entry}\n  ")))
}

fn matching_bracket(text: &str, open: usize) -> Option<usize> {
    let mut scanner = Scanner { text, at: open };
    scanner.value(0)?;
    Some(scanner.at - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(text: &str) -> Vec<&'static str> {
        check(text).iter().map(|f| f.code).collect()
    }

    #[test]
    fn reports_shape_problems_only() {
        let text = r#"{"properties": [
            {"name": "a.b", "type": "java.lang.String", "description": "x", "colour": 1},
            {"name": "a.b", "type": "com.acme.Missing", "description": "x"},
            {"name": "a.c", "type": "Map<String, int[]>"},
            {"name": "a.d", "type": "not a type!", "description": "x"},
            {"name": "a.e", "type": "String", "deprecation": {"reason": "r"}}
        ], "hints": [{"name": "a.b", "values": [{"value": "x"}], "extra": 1}], "oops": []}"#;
        assert_eq!(codes(text), [
            "unknown-metadata-field",
            "unknown-metadata-field",
            "duplicate-metadata-name",
            "missing-metadata-description",
            "invalid-metadata-type",
            "unknown-metadata-field",
        ]);
        assert!(check(r#"{"properties": [{"name": "x.y", "type": "String", "description": "d"}]}"#).is_empty());
        assert!(check("{ broken").is_empty());
    }

    #[test]
    fn defines_a_key_in_existing_or_empty_lists() {
        let text = "{\n  \"properties\": [\n    {\"name\": \"a\", \"description\": \"d\"}\n  ]\n}\n";
        let (from, to, add) = define_key(text, "x.y").unwrap();
        let out = format!("{}{add}{}", &text[..from], &text[to..]);
        assert!(check(&out).iter().all(|f| f.code == "missing-metadata-description"));
        assert!(out.contains("\"x.y\"") && out.contains("\"a\""));
        assert!(parse(&out).is_some());

        let (from, to, add) = define_key("{}", "x.y").unwrap();
        assert!(parse(&format!("{}{add}{}", &"{}"[..from], &"{}"[to..])).is_some());
        let (from, to, add) = define_key("{\"properties\": []}", "k").unwrap();
        let text = "{\"properties\": []}";
        assert!(parse(&format!("{}{add}{}", &text[..from], &text[to..])).is_some());
    }
}
