//! Keys used outside `.properties` files: the text search behind
//! `UnusedPropertyInspection`, and the string literals of Java/Kotlin i18n
//! calls that complete and navigate to keys (`PropertiesCompletionContributor`,
//! `PropertyReference`).

use crate::props::{dir, file_name, parse, range_of, split_locale, stem, suffix, unescape};
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

/// A key used in code or a template, and whether a missing declaration is worth a warning.
pub struct KeyUse {
    pub span: Span<usize>,
    /// The literal base name of the bundle the call reads, when it names one.
    pub bundle: Option<String>,
    pub report: bool,
}

fn comment_line(text: &str, at: usize) -> bool {
    let line = text[..at].rsplit('\n').next().unwrap_or_default().trim_start();
    line.starts_with("//") || line.starts_with('*') || line.starts_with("/*")
}

fn string_end(text: &str, from: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = from;
    while i < b.len() && b[i] != b'\n' {
        match b[i] {
            b'\\' => i += 1,
            b'"' => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// The top-level arguments of the call whose `(` ends just before `from`.
fn call_arguments(text: &str, from: usize) -> Vec<&str> {
    let b = text.as_bytes();
    let (mut args, mut depth, mut start, mut i) = (Vec::new(), 0, from, from);
    while i < b.len() {
        match b[i] {
            b'"' => match string_end(text, i + 1) {
                Some(end) => i = end,
                None => break,
            },
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => {
                args.push(text[start..i].trim());
                return args;
            }
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                args.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    args
}

fn string_literal(arg: &str) -> Option<&str> {
    arg.strip_prefix('"')?.strip_suffix('"').filter(|s| !s.contains('"'))
}

/// Keys read through `MessageSource.getMessage("k", …)` and through
/// `ResourceBundle.getBundle("name")…getString("k")` in Java/Kotlin source.
/// Only calls whose key is a plain literal are reported; a default message
/// argument or a bundle that can't be told stay quiet.
// ponytail: text scan, no types: `getMessage` of any receiver counts, `message(…)` (IntelliJ's helper) is never reported.
pub fn source_uses(text: &str) -> Vec<KeyUse> {
    let mut uses = Vec::new();

    for name in ["getMessage", "getString", "getObject", "getStringArray"] {
        for (at, _) in text.match_indices(name) {
            let after = at + name.len();
            if text[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') || comment_line(text, at) {
                continue;
            }
            let Some(rest) = text[after..].strip_prefix('(') else { continue };
            let Some(literal) = rest.trim_start().strip_prefix('"') else { continue };
            let start = text.len() - literal.len();
            let Some(end) = string_end(text, start) else { continue };
            let key = &text[start..end];
            if key.is_empty() || !key.chars().all(key_char) {
                continue;
            }
            let args = call_arguments(text, after + 1);
            if !matches!(text[end + 1..].trim_start().chars().next(), Some(',' | ')')) {
                continue;
            }

            let bundle = if name == "getMessage" {
                let has_default = args.len() >= 4 || (args.len() == 3 && args[2].starts_with('"'));
                if has_default {
                    continue;
                }
                None
            } else {
                let statement = text[..at].rfind([';', '{', '}']).map_or(0, |i| i + 1);
                let Some(call) = text[statement..at].rfind("getBundle(") else { continue };
                let opened = statement + call + "getBundle(".len();
                let Some(literal) = call_arguments(text, opened).first().and_then(|a| string_literal(a)) else { continue };
                Some(literal.to_owned())
            };

            uses.push(KeyUse { span: start..end, bundle, report: true });
        }
    }

    uses
}

/// Keys in a template: `#{a.b}` (Thymeleaf messages; Java EL in `.xhtml`/JSP
/// is only navigated), `<spring:message code="a.b"/>` and `<fmt:message key="a.b"/>`.
pub fn markup_uses(text: &str, path: &str) -> Vec<KeyUse> {
    let mut uses = Vec::new();
    let thymeleaf = path.ends_with(".html") || path.ends_with(".htm");

    for (at, _) in text.match_indices("#{") {
        let start = at + 2;
        let len = text[start..].find(|c: char| !key_char(c)).unwrap_or(text.len() - start);
        if len > 0 && matches!(text[start + len..].chars().next(), Some('}' | '(')) {
            uses.push(KeyUse { span: start..start + len, bundle: None, report: thymeleaf });
        }
    }

    let basenames: Vec<&str> = text
        .match_indices("basename=")
        .filter_map(|(at, _)| text[at + 9..].strip_prefix(['"', '\'']).and_then(|v| v.split(['"', '\'']).next()))
        .collect();

    for (tag, attribute, quiet_attribute) in [("<spring:message", "code", "text"), ("<s:message", "code", "text"), ("<fmt:message", "key", "bundle")] {
        for (at, _) in text.match_indices(tag) {
            let end = text[at..].find('>').map_or(text.len(), |i| at + i);
            let element = &text[at..end];
            let Some(found) = element.find(&format!(" {attribute}=")) else { continue };
            let value_at = at + found + attribute.len() + 2;
            let Some(quote) = text[value_at..].chars().next().filter(|c| matches!(c, '"' | '\'')) else { continue };
            let start = value_at + 1;
            let Some(len) = text[start..end].find(quote) else { continue };
            if len == 0 || !text[start..start + len].chars().all(key_char) {
                continue;
            }

            let mixed_bundles = basenames.iter().any(|b| *b != basenames[0]);
            let report = !element.contains(&format!(" {quiet_attribute}=")) && !(tag == "<fmt:message" && mixed_bundles);
            let bundle = (tag == "<fmt:message").then(|| basenames.first().map(|b| (*b).to_owned())).flatten();
            uses.push(KeyUse { span: start..start + len, bundle, report });
        }
    }

    uses
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
    pub files: Vec<String>,
}

impl KeyIndex {
    pub fn build(project: &Project) -> Result<Self, Error> {
        let mut index = Self::default();
        for path in project.find_files("**/*.properties", 5000)? {
            let Ok(text) = project.read_file(&path) else { continue };
            index.files.push(path.clone());
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

    /// The base name of the project's only MessageSource-style bundle
    /// (`messages`, `ValidationMessages`, or anything under `i18n/`), if there is exactly one.
    pub fn conventional_bundle(&self) -> Option<&str> {
        let mut found: Vec<(&str, &str)> = Vec::new();
        for path in &self.files {
            let base = split_locale(stem(path)).0;
            let folder = dir(path);
            let conventional = matches!(base, "messages" | "message" | "ValidationMessages") || folder.rsplit('/').next() == Some("i18n");
            if conventional && !found.contains(&(folder, base)) {
                found.push((folder, base));
            }
        }
        match found.as_slice() {
            [(_, base)] => Some(base),
            _ => None,
        }
    }

    /// The uses whose key no `.properties` file declares, in a project with one unambiguous bundle.
    pub fn undeclared<'a>(&self, uses: &'a [KeyUse], text: &str) -> Vec<&'a KeyUse> {
        let Some(base) = self.conventional_bundle() else { return Vec::new() };
        uses.iter()
            .filter(|u| u.report)
            .filter(|u| u.bundle.as_deref().is_none_or(|name| name.rsplit(['.', '/']).next() == Some(base)))
            .filter(|u| !self.keys.contains_key(&text[u.span.clone()]))
            .collect()
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
