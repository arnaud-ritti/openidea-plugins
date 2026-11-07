//! What the project's sources declare that configuration values name: classes and enums.

use spring_text::parse_source;
use std::collections::{BTreeMap, BTreeSet};

/// Enums of Spring Boot's and the JDK's types that keys in the bundled metadata have, as written in configuration.
const KNOWN: &[(&str, &[&str])] = &[
    ("Access", &["none", "read-only", "unrestricted"]),
    ("TimeUnit", &["nanoseconds", "microseconds", "milliseconds", "seconds", "minutes", "hours", "days"]),
    ("DatabaseInitializationMode", &["always", "embedded", "never"]),
    ("Show", &["never", "when-authorized", "always"]),
    ("ErrorProperties.IncludeAttribute", &["never", "always", "on-param"]),
    ("WebApplicationType", &["none", "servlet", "reactive"]),
    ("Cookie.SameSite", &["none", "lax", "strict"]),
    ("Ssl.ClientAuth", &["none", "want", "need"]),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub package: String,
    pub types: Vec<String>,
    pub enums: Vec<(String, Vec<String>)>,
}

pub fn read(path: &str, text: &str) -> Option<Facts> {
    let package = crate::nav::package_of(path, text).unwrap_or_default();
    let file = parse_source(path, text);
    let types = file
        .types
        .iter()
        .map(|t| {
            let mut name = t.name.clone();
            let mut outer = t.outer;
            while let Some(i) = outer {
                name = format!("{}.{name}", file.types[i].name);
                outer = file.types[i].outer;
            }
            if package.is_empty() { name } else { format!("{package}.{name}") }
        })
        .collect();
    let enums = enum_constants(text);
    Some(Facts { package, types, enums })
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

fn enum_constants(text: &str) -> Vec<(String, Vec<String>)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find("enum") {
        let at = from + found;
        from = at + 4;
        let boundary = at == 0 || !is_ident(bytes[at - 1]);
        if !boundary || !text[from..].starts_with(char::is_whitespace) {
            continue;
        }
        let mut rest = text[from..].trim_start();
        if let Some(after) = rest.strip_prefix("class").filter(|r| r.starts_with(char::is_whitespace)) {
            rest = after.trim_start();
        }
        let name: String = rest.chars().take_while(|c| is_ident(*c as u8) && c.is_ascii()).collect();
        let Some(open) = rest.find('{').filter(|_| !name.is_empty()) else { continue };
        let body_at = text.len() - rest.len() + open + 1;
        out.push((name, constants(&text[body_at..])));
    }
    out
}

fn constants(body: &str) -> Vec<String> {
    let bytes = body.as_bytes();
    let (mut depth, mut i, mut start) = (0usize, 0usize, 0usize);
    let mut entries = Vec::new();
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => i += body[i..].find('\n').unwrap_or(bytes.len() - i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i += body[i..].find("*/").map_or(bytes.len() - i, |e| e + 1),
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += usize::from(bytes[i] == b'\\') + 1;
                }
            }
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' if depth == 0 => {
                entries.push(&body[start..i]);
                break;
            }
            b'}' => depth -= 1,
            b';' if depth == 0 => {
                entries.push(&body[start..i]);
                break;
            }
            b',' if depth == 0 => {
                entries.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    entries
        .into_iter()
        .filter_map(|entry| {
            let mut entry = strip_comments(entry);
            while let Some(annotated) = entry.trim_start().strip_prefix('@') {
                let end = annotated.find(char::is_whitespace).unwrap_or(annotated.len());
                entry = annotated[end..].to_owned();
            }
            let name: String = entry.trim_start().chars().take_while(|c| c.is_ascii() && is_ident(*c as u8)).collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn strip_comments(entry: &str) -> String {
    let mut out = String::new();
    let mut rest = entry;
    while let Some(i) = rest.find("/*").into_iter().chain(rest.find("//")).min() {
        out.push_str(&rest[..i]);
        rest = if rest[i..].starts_with("//") {
            rest[i..].find('\n').map_or("", |n| &rest[i + n..])
        } else {
            rest[i..].find("*/").map_or("", |n| &rest[i + n + 2..])
        };
    }
    out + rest
}

/// What every source read so far declares.
#[derive(Default)]
pub struct Summary {
    /// Every source of the project was read.
    pub complete: bool,
    pub types: BTreeSet<String>,
    pub packages: BTreeSet<String>,
    enums: BTreeMap<String, Vec<String>>,
}

pub fn dashed(constant: &str) -> String {
    constant.to_lowercase().replace('_', "-")
}

fn relaxed(value: &str) -> String {
    value.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

impl Summary {
    pub fn merge<'a>(facts: impl Iterator<Item = &'a Facts>, complete: bool) -> Self {
        let mut s = Summary { complete, ..Summary::default() };
        for (name, values) in KNOWN {
            s.enums.insert((*name).to_owned(), values.iter().map(|v| (*v).to_owned()).collect());
        }
        for f in facts {
            s.types.extend(f.types.iter().cloned());
            if !f.package.is_empty() {
                s.packages.insert(f.package.clone());
            }
            for (name, constants) in &f.enums {
                s.enums.insert(name.clone(), constants.iter().map(|c| dashed(c)).collect());
            }
        }
        s
    }

    /// The values of the enum named `ty` (`Mode`, `Outer.Mode`), as written in configuration.
    pub fn enum_values(&self, ty: &str) -> Option<&Vec<String>> {
        let last = ty.rsplit('.').next().unwrap_or(ty);
        self.enums.get(last).or_else(|| self.enums.get(ty)).filter(|v| !v.is_empty())
    }

    /// Whether `value` is one of the enum's constants under Spring's relaxed matching.
    pub fn accepts(&self, ty: &str, value: &str) -> Option<bool> {
        let values = self.enum_values(ty)?;
        let wanted = relaxed(value);
        Some(values.iter().any(|v| relaxed(v) == wanted))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_types_and_enum_constants() {
        let src = "package com.acme;\npublic class Outer {\n  enum Mode { FAST, SAFE_MODE(\"x, y\"), // c\n  /* d */ LAST; int code; }\n}\nenum class Kt { A, B }\n";
        let facts = read("A.java", src).unwrap();
        assert!(facts.types.contains(&"com.acme.Outer".to_owned()));
        assert_eq!(facts.enums[0], ("Mode".to_owned(), vec!["FAST".to_owned(), "SAFE_MODE".to_owned(), "LAST".to_owned()]));
        assert_eq!(facts.enums[1].0, "Kt");
        let summary = Summary::merge([&facts].into_iter(), true);
        assert_eq!(summary.accepts("Outer.Mode", "safe-mode"), Some(true));
        assert_eq!(summary.accepts("Mode", "slow"), Some(false));
        assert_eq!(summary.accepts("Show", "WHEN_AUTHORIZED"), Some(true));
        assert_eq!(summary.accepts("Foreign", "x"), None);
    }
}
