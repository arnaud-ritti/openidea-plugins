//! Keys used outside `.properties` files: the text search behind
//! `UnusedPropertyInspection`, and the string literals of Java/Kotlin i18n
//! calls that complete and navigate to keys (`PropertiesCompletionContributor`,
//! `PropertyReference`).

use crate::props::{file_name, parse, range_of, suffix, unescape};
use openidea_plugin::{Error, Location, Project};
use std::collections::{BTreeMap, HashSet};
use std::ops::Range as Span;

fn key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Removes from `unused` the keys `text` mentions: every dot-separated run of
/// a word (`a.b` in `"x.a.b.c"`), and plain substrings for keys with other
/// characters. A mention is not a reference, so this errs towards "used".
pub fn mark_used(text: &str, unused: &mut HashSet<String>) {
    for word in text.split(|c: char| !key_char(c)) {
        let segments: Vec<&str> = word.split('.').collect();
        for from in 0..segments.len() {
            for to in from + 1..=segments.len() {
                unused.remove(&segments[from..to].join("."));
            }
        }
    }

    unused.retain(|key| key.chars().all(key_char) || !text.contains(key.as_str()));
}

/// `LoggerConfigPropertyUsageProvider` and `SimpleLoggerImplicitPropertiesUsageProvider`:
/// files read by a framework, not by code.
pub fn implicitly_used(path: &str) -> bool {
    let name = file_name(path);
    ["log4j", "commons-logging", "logging"].iter().any(|prefix| name.starts_with(prefix)) || name == "simplelogger.properties"
}

/// The content span of the `"…"` literal around `at` on its line (an
/// unterminated one runs to the end of the line, as while typing).
pub fn literal_at(text: &str, at: usize) -> Option<Span<usize>> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let b = text.as_bytes();
    let mut open: Option<usize> = None;
    let mut i = line_start;

    while i < line_end {
        match b[i] {
            b'\\' if open.is_some() => i += 1,
            b'"' => match open.take() {
                Some(start) if (start + 1..=i).contains(&at) => return Some(start + 1..i),
                Some(_) => {}
                None => open = Some(i),
            },
            _ => {}
        }
        i += 1;
    }

    open.filter(|&start| at > start).map(|start| start + 1..line_end)
}

/// Methods whose first string argument is a bundle key.
// ponytail: method names stand in for IntelliJ's @PropertyKey / ResourceBundle references; add names as projects need them.
const I18N_METHODS: [&str; 5] = ["getString", "getObject", "getStringArray", "getMessage", "message"];

/// Whether the literal is the first argument of an i18n call (`bundle.getString("…")`).
pub fn is_i18n_argument(text: &str, literal: &Span<usize>) -> bool {
    let before = text[..literal.start.saturating_sub(1)].trim_end();
    let Some(callee) = before.strip_suffix('(') else { return false };
    let callee = callee.trim_end();
    let name_start = callee.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    I18N_METHODS.contains(&&callee[name_start..])
}

pub struct Declaration {
    pub location: Location,
    pub value: String,
    pub default_locale: bool,
}

/// Every key of the project's `.properties` files.
#[derive(Default)]
pub struct KeyIndex {
    pub keys: BTreeMap<String, Vec<Declaration>>,
}

impl KeyIndex {
    pub fn build(project: &Project) -> Result<Self, Error> {
        let mut index = Self::default();
        for path in project.find_files("**/*.properties", 5000)? {
            let Ok(text) = project.read_file(&path) else { continue };
            for p in parse(&text) {
                index.keys.entry(p.unescaped_key(&text)).or_default().push(Declaration {
                    location: Location { path: path.clone(), range: range_of(&text, &p.key) },
                    value: unescape(&text[p.value.clone()]),
                    default_locale: suffix(&path).is_empty(),
                });
            }
        }
        Ok(index)
    }

    /// The value shown next to a key: the default locale's, else any.
    pub fn value(&self, key: &str) -> Option<&str> {
        let declarations = self.keys.get(key)?;
        declarations.iter().find(|d| d.default_locale).or(declarations.first()).map(|d| d.value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_mentioned_keys_as_used() {
        let mut unused: HashSet<String> =
            ["app.title", "app.unused", "error.code", "with space"].iter().map(|s| (*s).to_owned()).collect();
        mark_used("label = bundle.getString(\"app.title\"); x = \"${prefix.error.code}\" + \"with space\";", &mut unused);
        assert_eq!(unused, HashSet::from(["app.unused".to_owned()]));
    }

    #[test]
    fn finds_i18n_literals() {
        let text = "String s = bundle.getString(\"app.ti\");\nlog(\"other\");";
        let at = text.find("ti\"").unwrap() + 2;
        let literal = literal_at(text, at).unwrap();
        assert_eq!(&text[literal.clone()], "app.ti");
        assert!(is_i18n_argument(text, &literal));

        let other = literal_at(text, text.find("other").unwrap()).unwrap();
        assert!(!is_i18n_argument(text, &other));
        assert_eq!(literal_at(text, 3), None);
    }
}
