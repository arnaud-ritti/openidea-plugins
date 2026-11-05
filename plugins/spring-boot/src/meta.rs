//! What configuration keys exist: Spring Boot's own metadata (bundled), the
//! project's metadata files and its `@ConfigurationProperties` classes.

use serde_json::Value;
use spring_text::metadata::{Catalog, Deprecation, Meta, Prop, parse_tsv};
use spring_text::java::{self, JavaFile, TypeRef};
use std::sync::OnceLock;

const MAX_DESCRIPTION: usize = 110;

/// Spring Boot's own properties.
pub fn bundled() -> &'static Catalog {
    static BUNDLED: OnceLock<Catalog> = OnceLock::new();
    BUNDLED.get_or_init(|| {
        let mut c = Catalog::default();
        c.add(&parse_tsv(include_str!("../data/boot-properties.tsv")));
        c
    })
}

/// `java.util.Map<java.lang.String,Foo$Bar>` → `Map<String,Foo.Bar>`.
fn simple_type(t: &str) -> String {
    let mut out = String::new();
    let mut seg = String::new();
    for c in t.chars().chain([' ']) {
        if c.is_alphanumeric() || c == '_' {
            seg.push(c);
            continue;
        }
        if c == '.' && seg.starts_with(char::is_lowercase) {
            seg.clear();
            continue;
        }
        out.push_str(&seg);
        seg.clear();
        if !c.is_whitespace() {
            out.push(if c == '$' { '.' } else { c });
        }
    }
    out
}

fn first_sentence(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let end = text.find(". ").unwrap_or(text.len());
    let s = text[..end].trim_end_matches('.');
    if s.chars().count() <= MAX_DESCRIPTION {
        return s.to_owned();
    }
    s.chars().take(MAX_DESCRIPTION - 1).collect::<String>() + "…"
}

fn json_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(json_text).collect::<Vec<_>>().join(","),
        other => other.to_string(),
    }
}

/// A `META-INF/(additional-)spring-configuration-metadata.json` of the project.
pub fn parse_json(text: &str) -> Option<Meta> {
    let v: Value = serde_json::from_str(text).ok()?;
    let props = v["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let dep = &p["deprecation"];
            let deprecated = (dep.is_object() || p["deprecated"] == Value::Bool(true)).then(|| Deprecation {
                removed: dep["level"] == "error",
                replacement: dep["replacement"].as_str().unwrap_or_default().to_owned(),
            });
            Some(Prop {
                name: p["name"].as_str()?.to_owned(),
                ty: simple_type(p["type"].as_str().unwrap_or_default()),
                default: json_text(&p["defaultValue"]),
                deprecated,
                description: first_sentence(p["description"].as_str().unwrap_or_default()),
            })
        })
        .collect();
    let hints = v["hints"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let values: Vec<String> =
                h["values"].as_array()?.iter().map(|v| json_text(&v["value"])).filter(|s| !s.is_empty()).collect();
            Some((h["name"].as_str()?.to_owned(), values)).filter(|(_, v)| !v.is_empty())
        })
        .collect();
    Some(Meta { props, hints })
}

/// Spring's dashed form of a Java property name: `maxPoolSize` → `max-pool-size`.
pub fn dashed(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        let c = if c == '_' { '-' } else { c };
        if c.is_uppercase() && !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        out.extend(c.to_lowercase());
    }
    out
}

/// The prefix of a `@ConfigurationProperties`.
fn prefix(a: Option<&java::Annotation>) -> Option<String> {
    let p = a?.strings(&["prefix", "value"]).first()?.value.clone();
    (!p.is_empty() && !p.contains("${")).then_some(p)
}

/// The `@ConfigurationProperties` classes and `@Bean` methods of a Java source.
pub fn parse_java(text: &str) -> Option<Meta> {
    if !text.contains("ConfigurationProperties") {
        return None;
    }
    let file = java::parse(text);
    let mut props = Vec::new();
    for (i, t) in file.types.iter().enumerate() {
        if let Some(prefix) = prefix(t.annotation("ConfigurationProperties")) {
            bind(&file, text, i, &prefix, 0, &mut props);
            if !t.extends.is_empty() {
                props.push(Prop::open_prefix(prefix)); // inherited properties are not seen
            }
        }
        for m in t.methods().filter(|m| m.has(&["Bean"])) {
            if let Some(prefix) = prefix(m.annotation("ConfigurationProperties")) {
                props.push(Prop::open_prefix(prefix));
            }
        }
    }
    (!props.is_empty()).then_some(Meta { props, hints: Vec::new() })
}

/// The properties of `file.types[i]` under `prefix`: record components,
/// fields and setters; a class of the same file one level down.
fn bind(file: &JavaFile, text: &str, i: usize, prefix: &str, depth: usize, out: &mut Vec<Prop>) {
    let t = &file.types[i];
    let mut found: Vec<(&str, &TypeRef, String)> = t.components.iter().map(|c| (c.name.as_str(), &c.ty, String::new())).collect();
    for m in &t.members {
        if m.modifiers.iter().any(|m| m == "static") {
            continue;
        }
        match m.kind {
            java::MemberKind::Field => {
                let Some(ty) = &m.ty else { continue };
                let init = m.body.map(|(a, b)| text[a..b].trim()).unwrap_or_default();
                let literal = !init.is_empty() && init.len() <= 60 && !init.contains(['(', '{', ' ']);
                found.push((&m.name, ty, if literal { init.trim_matches('"').to_owned() } else { String::new() }));
            }
            java::MemberKind::Method if m.params.len() == 1 && m.name.len() > 3 && m.name.starts_with("set") => {
                found.push((&m.name[3..], &m.params[0].ty, String::new()));
            }
            _ => {}
        }
    }
    for (name, ty, default) in found {
        let key = format!("{prefix}.{}", dashed(&spring_text::decapitalize(name)));
        if out.iter().any(|p| p.name == key) {
            continue;
        }
        let nested = file.types.iter().position(|n| n.name == ty.name && matches!(n.kind.as_str(), "class" | "record"));
        match nested {
            Some(j) if j != i && depth < 3 => bind(file, text, j, &key, depth + 1, out),
            _ => out.push(Prop { name: key, ty: simple_type(&ty.text), default, ..Prop::default() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spring_text::metadata::Lookup;

    #[test]
    fn reads_the_bundled_metadata() {
        let c = Lookup(vec![bundled()]);
        let port = c.prop("server.port").unwrap();
        assert_eq!((port.ty.as_str(), port.default.as_str()), ("Integer", "8080"));
        assert!(c.prop("spring.jpa.showSql").is_some(), "relaxed binding");
        let redis = c.prop("spring.redis.host").unwrap().deprecated.as_ref().unwrap();
        assert_eq!(redis.replacement, "spring.data.redis.host");
        assert_eq!(c.values("logging.level.com.acme"), ["trace", "debug", "info", "warn", "error", "fatal", "off"]);
        assert_eq!(c.values("spring.jpa.show-sql"), ["true", "false"]);
        assert!(c.values("spring.jpa.hibernate.ddl-auto").contains(&"update".to_owned()));
        assert_eq!(c.map_keys("logging.level"), ["root", "sql", "web"]);
    }

    #[test]
    fn tells_unknown_keys_only_in_known_groups() {
        let c = Lookup(vec![bundled()]);
        assert_eq!(c.unknown("spring.datasource.urll"), Some("spring.datasource"));
        assert_eq!(c.unknown("server.servlet.context-pathh"), Some("server.servlet"));
        for known in [
            "spring.datasource.url",
            "spring.datasource.driverClassName",
            "spring.datasource",
            "logging.level.com.acme",
            "spring.jpa.properties.hibernate.format_sql",
            "spring.security.oauth2.client.registration.google.client-id",
            "management.endpoints.web.exposure.include[0]",
            "spring.datasource.druid.initial-size",
            "management.health.consul.enabled",
            "spring.cloud.gateway.routes[0].id",
            "app.greeting",
            "server.prot",
            "spring.datasource.${x}",
        ] {
            assert_eq!(c.unknown(known), None, "{known}");
        }
    }

    #[test]
    fn reads_project_metadata_files() {
        let json = r#"{"groups": [{"name": "acme"}], "properties": [
            {"name": "acme.mode", "type": "java.lang.String", "description": "How it runs. More text.", "defaultValue": "fast"},
            {"name": "acme.old", "type": "java.lang.Boolean", "deprecation": {"replacement": "acme.mode", "level": "error"}},
            {"name": "acme.tags", "type": "java.util.Map<java.lang.String,java.util.List<com.acme.Tag$Kind>>"}
        ], "hints": [{"name": "acme.mode", "values": [{"value": "fast"}, {"value": "safe"}]}]}"#;
        let meta = parse_json(json).unwrap();
        assert_eq!(meta.props[0].description, "How it runs");
        assert_eq!(meta.props[0].default, "fast");
        assert_eq!(meta.props[1].deprecated, Some(Deprecation { removed: true, replacement: "acme.mode".into() }));
        assert_eq!(meta.props[2].ty, "Map<String,List<Tag.Kind>>");
        assert_eq!(meta.hints, [("acme.mode".to_owned(), vec!["fast".to_owned(), "safe".to_owned()])]);
        assert_eq!(parse_json("{ nope"), None);
    }

    #[test]
    fn binds_configuration_properties_classes() {
        let src = r#"package com.acme;
@ConfigurationProperties(prefix = "acme.mail")
public class MailProperties {
    private String host = "localhost";
    private int port = 25;
    private boolean useSsl;
    private static final int MAX = 3;
    private final Pool pool = new Pool();
    public void setReplyTo(String replyTo) {}
    public static class Pool { private int maxSize = 8; }
}

@ConfigurationProperties("acme.retry")
record RetryProperties(int maxAttempts, Duration backoff) {}

@Configuration
class Config {
    @Bean
    @ConfigurationProperties("acme.pool")
    HikariDataSource pool() { return new HikariDataSource(); }
}
"#;
        let meta = parse_java(src).unwrap();
        let names: Vec<_> = meta.props.iter().map(|p| (p.name.as_str(), p.ty.as_str(), p.default.as_str())).collect();
        assert_eq!(names, [
            ("acme.mail.host", "String", "localhost"),
            ("acme.mail.port", "int", "25"),
            ("acme.mail.use-ssl", "boolean", ""),
            ("acme.mail.pool.max-size", "int", "8"),
            ("acme.mail.reply-to", "String", ""),
            ("acme.retry.max-attempts", "int", ""),
            ("acme.retry.backoff", "Duration", ""),
            ("acme.pool", "*", ""),
        ]);
        assert_eq!(parse_java("class A {}"), None);
        assert_eq!(dashed("URLPath"), "u-r-l-path");
    }
}
