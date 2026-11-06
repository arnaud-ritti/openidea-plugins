//! Naming rules ported from MicroProfileConfigPropertyProvider and QuarkusConfigMappingProvider (Quarkus Tools for IntelliJ 2.6.1, Red Hat, EPL-2.0).

use openidea_plugin::providers::{CompletionItem, CompletionKind};
use spring_text::java::{self, Annotation, JavaFile, TypeRef};
use spring_text::metadata::{Lookup, Meta, Prop};
use spring_text::range;
use std::collections::BTreeSet;

/// `maxPoolSize` -> `max-pool-size`; a run of capitals is one hump (`URLPath` -> `url-path`).
pub fn hyphenate(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() {
            let after_lower = chars[i - 1].is_lowercase() || chars[i - 1].is_ascii_digit();
            let ends_run = chars[i - 1].is_uppercase() && chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if after_lower || ends_run {
                out.push('-');
            }
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[derive(Clone, Copy)]
enum Naming {
    Kebab,
    Snake,
    Verbatim,
}

impl Naming {
    fn of(annotation: &Annotation) -> Self {
        match annotation.arg("namingStrategy").map(|a| a.text.as_str()) {
            Some(t) if t.ends_with("VERBATIM") => Self::Verbatim,
            Some(t) if t.ends_with("SNAKE_CASE") => Self::Snake,
            _ => Self::Kebab,
        }
    }

    fn convert(self, name: &str) -> String {
        match self {
            Self::Kebab => hyphenate(name),
            Self::Snake => hyphenate(name).replace('-', "_"),
            Self::Verbatim => name.to_owned(),
        }
    }
}

fn unwrapped(ty: &TypeRef) -> &TypeRef {
    match ty.args.as_slice() {
        [inner] if ty.name == "Optional" => inner,
        _ => ty,
    }
}

fn first_string(annotation: Option<&Annotation>, names: &[&str]) -> Option<String> {
    annotation?.strings(names).first().map(|s| s.value.clone())
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_owned() } else { format!("{prefix}.{name}") }
}

/// The configuration keys a Java file declares, `None` when it declares none.
pub fn config_meta(_path: &str, text: &str) -> Option<Meta> {
    if !text.contains("@ConfigProperty") && !text.contains("@ConfigMapping") {
        return None;
    }
    let file = java::parse(text);
    let mut props = Vec::new();
    for (i, t) in file.types.iter().enumerate() {
        for m in &t.members {
            if let (Some(a), Some(ty)) = (m.annotation("ConfigProperty"), &m.ty) {
                injection(a, ty, &mut props);
            }
            for p in &m.params {
                if let Some(a) = p.annotation("ConfigProperty") {
                    injection(a, &p.ty, &mut props);
                }
            }
        }
        for c in &t.components {
            if let Some(a) = c.annotation("ConfigProperty") {
                injection(a, &c.ty, &mut props);
            }
        }
        if let Some(mapping) = t.annotation("ConfigMapping") {
            let prefix = first_string(Some(mapping), &["prefix", "value"]).unwrap_or_default();
            bind(&file, i, &prefix, Naming::of(mapping), 0, &mut props, &mut Vec::new());
        }
    }
    (!props.is_empty()).then_some(Meta { props, hints: Vec::new() })
}

/// What the project index keeps of a Java file.
#[derive(Clone)]
pub struct JavaInfo {
    pub meta: Option<Meta>,
    /// The interfaces of a file that has `@Path`: their name and whether they carry `@RegisterRestClient`.
    pub interfaces: Vec<(String, bool)>,
}

pub fn java_info(path: &str, text: &str) -> Option<JavaInfo> {
    let meta = config_meta(path, text);
    let mentions_rest = text.contains("@Path") || text.contains("@RegisterRestClient");
    let interfaces: Vec<(String, bool)> = if mentions_rest {
        let file = java::parse(text);
        file.types.iter().filter(|t| t.kind == "interface").map(|t| (t.name.clone(), t.has(&["RegisterRestClient"]))).collect()
    } else {
        Vec::new()
    };
    (meta.is_some() || !interfaces.is_empty()).then_some(JavaInfo { meta, interfaces })
}

/// A `@ConfigProperty(name = "…")` injection point with a literal name.
pub struct Injection {
    pub key: String,
    pub at: usize,
    pub end: usize,
    pub optional: bool,
    pub has_default: bool,
    pub ty: String,
    /// Where `, defaultValue = "…"` goes.
    pub insert_at: usize,
}

pub fn injections(file: &JavaFile) -> Vec<Injection> {
    let mut found = Vec::new();
    for t in &file.types {
        for m in &t.members {
            if let (Some(a), Some(ty)) = (m.annotation("ConfigProperty"), &m.ty) {
                found.push((a, ty));
            }
            for p in &m.params {
                if let Some(a) = p.annotation("ConfigProperty") {
                    found.push((a, &p.ty));
                }
            }
        }
        for c in &t.components {
            if let Some(a) = c.annotation("ConfigProperty") {
                found.push((a, &c.ty));
            }
        }
    }
    found
        .into_iter()
        .filter_map(|(a, ty)| {
            let name = a.arg("name")?;
            let [literal] = name.strings.as_slice() else { return None };
            if name.text.matches('"').count() != 2 || !name.text.starts_with('"') || !name.text.ends_with('"') {
                return None;
            }
            let optional = ty.name.starts_with("Optional") || ty.name == "Provider" || ty.name == "Instance";
            Some(Injection {
                key: literal.value.clone(),
                at: literal.at,
                end: literal.end,
                optional,
                has_default: a.arg("defaultValue").is_some(),
                ty: ty.text.clone(),
                insert_at: name.end,
            })
        })
        .collect()
}

pub enum Slot {
    Key,
    Prefix,
}

/// Whether `at` is inside the `name` of `@ConfigProperty(` or the `prefix` of `@ConfigMapping(`: the bytes of
/// the string's contents. Read from the text around the cursor, so it also works in a file that does not parse.
pub fn completion_slot(text: &str, at: usize) -> Option<(Slot, usize, usize)> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let before = &text[line_start..at];
    if before.matches('"').count() % 2 == 0 {
        return None;
    }
    let open = line_start + before.rfind('"')?;
    let head = text[line_start..open].trim_end().strip_suffix('=')?.trim_end();
    let attribute = head.rsplit(|c: char| !c.is_alphanumeric() && c != '_').next()?;
    let slot = match attribute {
        "name" => Slot::Key,
        "prefix" => Slot::Prefix,
        _ => return None,
    };
    let window = &text[text.floor_char_boundary(open.saturating_sub(400))..open];
    let annotation = window.rfind("@Config")?;
    let tail = &window[annotation..];
    let expected = match slot {
        Slot::Key => "@ConfigProperty(",
        Slot::Prefix => "@ConfigMapping(",
    };
    let opened = tail.starts_with(expected) && tail.matches('(').count() > tail.matches(')').count();
    if !opened || tail.contains([';', '{', '}']) {
        return None;
    }
    let end = text[at..line_end].find('"').map_or(at, |i| at + i);
    Some((slot, open + 1, end))
}

pub fn key_items(text: &str, at: usize, lookup: &Lookup) -> Vec<CompletionItem> {
    let Some((slot, start, end)) = completion_slot(text, at) else { return Vec::new() };
    let replace = range(text, start, end);
    let typed = text[start..at].to_lowercase();
    let mut prefixes = BTreeSet::new();
    let mut items = Vec::new();
    for p in lookup.all().filter(|p| p.deprecated.as_ref().is_none_or(|d| !d.removed)) {
        if !p.name.to_lowercase().contains(&typed) {
            continue;
        }
        match slot {
            Slot::Prefix => {
                let mut group = p.name.as_str();
                while let Some((parent, _)) = group.rsplit_once('.') {
                    prefixes.insert(parent.to_owned());
                    group = parent;
                }
            }
            Slot::Key => {
                let documentation = [p.description.clone(), if p.default.is_empty() { String::new() } else { format!("Default: `{}`", p.default) }];
                let mut item = CompletionItem::new(&p.name, CompletionKind::Property);
                item.detail = Some(p.ty.clone()).filter(|t| !t.is_empty());
                item.insert_text = p.name.clone();
                item.replace = Some(replace);
                item.documentation = Some(documentation.join("\n\n").trim().to_owned()).filter(|d| !d.is_empty());
                items.push(item);
            }
        }
    }
    items.extend(prefixes.into_iter().map(|prefix| {
        let mut item = CompletionItem::new(&prefix, CompletionKind::Module);
        item.insert_text = prefix.clone();
        item.replace = Some(replace);
        item
    }));
    items
}

/// Every key the file declares with the bytes of the declaring name: the literal of a `@ConfigProperty`
/// and the accessor of a `@ConfigMapping` member.
pub fn declared_keys(text: &str) -> Vec<(String, usize, usize)> {
    if !text.contains("@ConfigProperty") && !text.contains("@ConfigMapping") {
        return Vec::new();
    }
    let file = java::parse(text);
    let mut keys: Vec<(String, usize, usize)> = injections(&file).into_iter().map(|i| (i.key, i.at, i.end)).collect();
    for (i, t) in file.types.iter().enumerate() {
        let Some(mapping) = t.annotation("ConfigMapping") else { continue };
        let prefix = first_string(Some(mapping), &["prefix", "value"]).unwrap_or_default();
        let (mut props, mut spans) = (Vec::new(), Vec::new());
        bind(&file, i, &prefix, Naming::of(mapping), 0, &mut props, &mut spans);
        keys.extend(props.into_iter().zip(spans).map(|(p, (at, end))| (p.name, at, end)));
    }
    keys
}

fn injection(annotation: &Annotation, ty: &TypeRef, out: &mut Vec<Prop>) {
    let Some(name) = first_string(Some(annotation), &["name"]) else { return };
    let default = first_string(Some(annotation), &["defaultValue"]).unwrap_or_default();
    out.push(Prop { name, ty: unwrapped(ty).text.clone(), default, ..Prop::default() });
}

/// The keys of `file.types[i]` under `prefix`: its accessor methods, descending into
/// interfaces of the same file.
// ponytail: super-interfaces and types from other files are not followed; needs a cross-file type index
fn bind(file: &JavaFile, i: usize, prefix: &str, naming: Naming, depth: usize, out: &mut Vec<Prop>, spans: &mut Vec<(usize, usize)>) {
    for m in file.types[i].methods() {
        let skipped = m.modifiers.iter().any(|x| x == "static" || x == "default");
        let Some(ty) = &m.ty else { continue };
        if skipped || !m.params.is_empty() || ty.text == "void" {
            continue;
        }
        let key = if m.has(&["WithParentName"]) {
            prefix.to_owned()
        } else {
            let name = first_string(m.annotation("WithName"), &["value"]).unwrap_or_else(|| naming.convert(&m.name));
            join(prefix, &name)
        };
        let inner = unwrapped(ty);
        let nested = file.types.iter().position(|n| n.name == inner.name && n.kind == "interface");
        match nested {
            Some(j) if j != i && depth < 3 => bind(file, j, &key, naming, depth + 1, out, spans),
            _ => {
                let default = first_string(m.annotation("WithDefault"), &["value"]).unwrap_or_default();
                out.push(Prop { name: key, ty: inner.text.clone(), default, ..Prop::default() });
                spans.push((m.at, m.at + m.name.len()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GREETING: &str = r#"package org.acme;
    public class GreetingResource {
        @ConfigProperty(name = "greeting.message", defaultValue = "hello")
        String message;
        @ConfigProperty(name = "greeting.suffix")
        Optional<String> suffix;
        @Inject
        GreetingResource(@ConfigProperty(name = "greeting.count", defaultValue = "3") int count) {}
        @ConfigProperty
        String unnamed;
    }"#;

    const SERVER: &str = r#"package org.acme;
    @ConfigMapping(prefix = "server")
    public interface ServerConfig {
        String hostName();
        @WithDefault("8080") int port();
        @WithName("ssl-enabled") boolean ssl();
        Optional<Log> log();
        Map<String, String> labels();
        static ServerConfig none() { return null; }
        default String url() { return hostName(); }
        interface Log {
            @WithDefault("false") boolean enabled();
            @WithParentName Level level();
        }
    }
    @ConfigMapping(prefix = "verbatim", namingStrategy = ConfigMapping.NamingStrategy.VERBATIM)
    interface Verbatim { String maxSize(); }
    @ConfigMapping(prefix = "snake", namingStrategy = ConfigMapping.NamingStrategy.SNAKE_CASE)
    interface Snake { String maxSize(); }
    "#;

    fn props(meta: &Meta) -> Vec<(&str, &str, &str)> {
        meta.props.iter().map(|p| (p.name.as_str(), p.ty.as_str(), p.default.as_str())).collect()
    }

    #[test]
    fn reads_config_property_injection_points() {
        let meta = config_meta("/p/GreetingResource.java", GREETING).unwrap();
        assert_eq!(props(&meta), [("greeting.message", "String", "hello"), ("greeting.suffix", "String", ""), ("greeting.count", "int", "3")]);
        assert_eq!(config_meta("/p/A.java", "class A {}"), None);
    }

    #[test]
    fn maps_config_mapping_interfaces() {
        let meta = config_meta("/p/ServerConfig.java", SERVER).unwrap();
        assert_eq!(props(&meta), [
            ("server.host-name", "String", ""),
            ("server.port", "int", "8080"),
            ("server.ssl-enabled", "boolean", ""),
            ("server.log.enabled", "boolean", "false"),
            ("server.log", "Level", ""),
            ("server.labels", "Map<String, String>", ""),
            ("verbatim.maxSize", "String", ""),
            ("snake.max_size", "String", ""),
        ]);
    }

    #[test]
    fn hyphenates_like_quarkus() {
        assert_eq!(hyphenate("maxPoolSize"), "max-pool-size");
        assert_eq!(hyphenate("URLPath"), "url-path");
        assert_eq!(hyphenate("useSSL"), "use-ssl");
        assert_eq!(hyphenate("host"), "host");
    }

    #[test]
    fn survives_every_prefix() {
        for end in (0..=SERVER.len()).filter(|&e| SERVER.is_char_boundary(e)) {
            config_meta("/p/ServerConfig.java", &SERVER[..end]);
        }
    }
}

pub fn parse_interface_start(text: &str, name: &str) -> Option<usize> {
    java::parse(text).types.iter().find(|t| t.kind == "interface" && t.name == name).map(|t| t.start)
}
