//! Navigation and extra completion in configuration files: class-valued keys
//! and `spring.config.import` to their files, logger packages, profile names
//! and `${}` placeholders.

use openidea_plugin::providers::{CompletionItem, CompletionKind};
use openidea_plugin::{Position, Range};
use spring_text::config::{self, Key, canonical, yaml_parent};
use spring_text::metadata::Lookup;
use spring_text::range;
use std::collections::BTreeSet;

pub const PROFILE_KEYS: &[&str] = &["spring.profiles.active", "spring.profiles.include", "spring.config.activate.on-profile"];
pub const SOURCE_GLOBS: &[&str] = &["**/*.java", "**/*.kt"];

/// The package of a Java or Kotlin source.
pub fn package_of(_path: &str, text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| l.starts_with("package "))?;
    let name = line.strip_prefix("package ")?.trim_end_matches(';').trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// What the caret is in: a key being typed, or the value of a key.
#[derive(Debug, PartialEq, Eq)]
pub enum Cursor {
    Key { path: String, typed_at: usize },
    Value { key: String, value_at: usize },
}

pub fn cursor(path: &str, text: &str, at: usize) -> Option<Cursor> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[line_start..at];
    if line.trim_start().starts_with(['#', '!']) {
        return None;
    }
    if path.ends_with(".properties") {
        return Some(match line.find(['=', ':']) {
            Some(sep) => {
                let after = &line[sep + 1..];
                Cursor::Value { key: line[..sep].trim().to_owned(), value_at: line_start + sep + 1 + after.len() - after.trim_start().len() }
            }
            None => {
                let typed_at = line_start + line.len() - line.trim_start().len();
                Cursor::Key { path: text[typed_at..at].to_owned(), typed_at }
            }
        });
    }
    let (parent, typed) = yaml_parent(text, at);
    let typed_at = at - typed.len();
    let join = |a: &str, b: &str| if a.is_empty() || b.is_empty() { format!("{a}{b}") } else { format!("{a}.{b}") };
    Some(match typed.find(": ") {
        Some(colon) => {
            let value = &typed[colon + 1..];
            Cursor::Value {
                key: join(&parent, typed[..colon].trim().trim_matches(['"', '\''])),
                value_at: typed_at + colon + 1 + value.len() - value.trim_start().len(),
            }
        }
        None if typed.is_empty() && !parent.is_empty() => Cursor::Key { path: format!("{parent}."), typed_at },
        None => Cursor::Key { path: join(&parent, &typed), typed_at },
    })
}

fn item(label: &str, insert: &str, kind: CompletionKind, replace: Range) -> CompletionItem {
    CompletionItem { insert_text: insert.to_owned(), replace: Some(replace), ..CompletionItem::new(label, kind) }
}

/// Every package and its parents: `com.acme.web` gives `com`, `com.acme`, `com.acme.web`.
fn with_parents<'p>(packages: impl Iterator<Item = &'p str>) -> BTreeSet<String> {
    let mut all = BTreeSet::new();
    for package in packages {
        let mut name = String::new();
        for segment in package.split('.') {
            if !name.is_empty() {
                name.push('.');
            }
            name.push_str(segment);
            all.insert(name.clone());
        }
    }
    all
}

pub struct Context<'a> {
    pub path: &'a str,
    pub text: &'a str,
    pub at: usize,
    pub lookup: &'a Lookup<'a>,
    pub packages: &'a [String],
    pub profiles: &'a [String],
}

/// Placeholder keys, logger packages and profile names where the caret is.
pub fn completion(cx: &Context) -> Vec<CompletionItem> {
    let Some(cursor) = cursor(cx.path, cx.text, cx.at) else { return Vec::new() };
    match cursor {
        Cursor::Key { path, .. } => {
            let Some(prefix) = path.get(..path.len().min("logging.level.".len())).filter(|p| p.eq_ignore_ascii_case("logging.level.")) else {
                return Vec::new();
            };
            let key_in_yaml = !cx.path.ends_with(".properties");
            let seg_start = cx.at - path[prefix.len()..].len();
            let replace = range(cx.text, seg_start, cx.at);
            let suffix = if key_in_yaml { ": " } else { "=" };
            with_parents(cx.packages.iter().map(String::as_str))
                .iter()
                .map(|p| item(p, &format!("{p}{suffix}"), CompletionKind::Module, replace))
                .collect()
        }
        Cursor::Value { key, value_at } => {
            let typed = &cx.text[value_at.min(cx.at)..cx.at];
            if let Some(open) = typed.rfind("${").filter(|&o| !typed[o..].contains('}')) {
                let replace = range(cx.text, value_at + open + 2, cx.at);
                let own = config::keys(cx.path, cx.text);
                let mut names: BTreeSet<&str> = own.iter().map(|k: &Key| k.key.as_str()).collect();
                names.extend(cx.lookup.all().map(|p| p.name.as_str()));
                return names.into_iter().map(|n| item(n, n, CompletionKind::Property, replace)).collect();
            }
            if !PROFILE_KEYS.iter().any(|k| canonical(k) == canonical(&key)) {
                return Vec::new();
            }
            let from = typed.rfind(',').map_or(0, |c| c + 1);
            let blanks = typed[from..].len() - typed[from..].trim_start().len();
            let replace = range(cx.text, value_at + from + blanks, cx.at);
            cx.profiles.iter().map(|p| item(p, p, CompletionKind::Value, replace)).collect()
        }
    }
}

/// The profile of a file name: `application-dev.yml` gives `dev`.
pub fn profile_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next()?;
    let stem = name.rsplit_once('.')?.0;
    let profile = stem.strip_prefix("application-").or_else(|| stem.strip_prefix("bootstrap-"))?;
    (!profile.is_empty()).then(|| profile.to_owned())
}

/// What a value points at.
#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    /// A path glob to look for in the project.
    File(String),
    /// A fully qualified class name.
    Class(String),
}

/// The target of the value under `at`, for the key it belongs to.
pub fn target(path: &str, text: &str, at: usize, lookup: &Lookup) -> Option<Target> {
    let key = config::keys(path, text).into_iter().find(|k| k.value_at <= at && at <= k.value_end && !k.value.is_empty())?;
    let base = key.key.split('[').next().unwrap_or(&key.key);
    let value = &text[key.value_at..key.value_end];
    if value.contains("${") || value.contains('*') {
        return None;
    }
    if canonical(base) == canonical("spring.config.import") {
        let offset = at - key.value_at;
        let from = value[..offset].rfind(',').map_or(0, |c| c + 1);
        let to = value[offset..].find(',').map_or(value.len(), |c| offset + c);
        let one = value[from..to].trim().trim_matches(['"', '\'']);
        let one = one.strip_prefix("optional:").unwrap_or(one);
        let location = match one.split_once(':') {
            Some(("classpath" | "file", rest)) => rest,
            Some(_) => return None,
            None => one,
        };
        let location = location.trim_start_matches("./").trim_start_matches('/');
        return (!location.is_empty() && !location.ends_with('/')).then(|| Target::File(format!("**/{location}")));
    }
    let class_valued = lookup.prop(base).is_some_and(|p| p.ty == "Class" || p.ty.starts_with("Class<"));
    let name = value.trim().trim_matches(['"', '\'']);
    let valid = name.split('.').all(|s| s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') && !s.is_empty());
    (class_valued && valid).then(|| Target::Class(name.replace('$', ".")))
}

/// The position of the declaration of `simple` in a source, or the start of the file.
pub fn declaration(text: &str, simple: &str) -> Range {
    let at = ["class", "interface", "record", "enum", "object"]
        .iter()
        .filter_map(|kw| text.find(&format!("{kw} {simple}")).map(|i| i + kw.len() + 1))
        .min();
    at.map_or(Range::new(Position::new(0, 0), Position::new(0, 0)), |from| range(text, from, from + simple.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::metadata::{Catalog, Meta, Prop};

    fn catalog() -> Catalog {
        let mut c = Catalog::default();
        c.add(&Meta { props: vec![Prop { name: "acme.handler".into(), ty: "Class<?>".into(), ..Prop::default() }], hints: vec![] });
        c
    }

    fn complete(path: &str, text: &str) -> Vec<String> {
        let c = catalog();
        let lookup = Lookup(vec![&c]);
        let packages = vec!["com.acme.web".to_owned()];
        let profiles = vec!["dev".to_owned(), "prod".to_owned()];
        let cx = Context { path, text, at: text.len(), lookup: &lookup, packages: &packages, profiles: &profiles };
        completion(&cx).into_iter().map(|i| i.insert_text).collect()
    }

    #[test]
    fn completes_loggers_profiles_and_placeholders() {
        assert_eq!(complete("application.properties", "logging.level.com.a"), ["com=", "com.acme=", "com.acme.web="]);
        assert_eq!(complete("application.yml", "logging:\n  level:\n    com.a"), ["com: ", "com.acme: ", "com.acme.web: "]);
        assert!(complete("application.properties", "logging.lev").is_empty());
        assert_eq!(complete("application.properties", "spring.profiles.active=dev,"), ["dev", "prod"]);
        assert_eq!(complete("application.yml", "spring:\n  profiles:\n    active: "), ["dev", "prod"]);
        assert!(complete("application.properties", "acme.x=1\nother=${ac").contains(&"acme.x".to_owned()));
        assert!(complete("application.properties", "other=${a}").is_empty());
    }

    #[test]
    fn resolves_imports_and_classes() {
        let c = catalog();
        let lookup = Lookup(vec![&c]);
        let text = "spring.config.import=optional:classpath:extra.yml,file:./x/y.properties\nacme.handler=com.acme.Handler\nacme.other=${X}\n";
        let at = |needle: &str| text.find(needle).unwrap() + 2;
        assert_eq!(target("application.properties", text, at("extra"), &lookup), Some(Target::File("**/extra.yml".into())));
        assert_eq!(target("application.properties", text, at("x/y"), &lookup), Some(Target::File("**/x/y.properties".into())));
        assert_eq!(target("application.properties", text, at("com.acme.H"), &lookup), Some(Target::Class("com.acme.Handler".into())));
        assert_eq!(target("application.properties", text, at("${X"), &lookup), None);
        let vault = "spring.config.import=vault://secret\n";
        assert_eq!(target("application.properties", vault, 25, &lookup), None);
    }

    #[test]
    fn reads_profile_and_package() {
        assert_eq!(profile_of("/p/application-dev.yml").as_deref(), Some("dev"));
        assert_eq!(profile_of("/p/application.yml"), None);
        assert_eq!(package_of("A.java", "// x\npackage com.acme;\nclass A {}").as_deref(), Some("com.acme"));
    }
}
