//! Checks of Java sources that need no type information: `@ConfigProperty` keys that no config file sets,
//! `@RestClient` injections of interfaces without `@RegisterRestClient`, and `@Scheduled` expressions.
//! Written from the Quarkus guides (config reference, REST client, scheduler).

use crate::checks::{duration, squash};
use crate::java::{Injection, injections};
use spring_text::finding::Finding;
use spring_text::java::{self, Annotation, JavaFile};
use spring_text::metadata::{Lookup, strip_profile};
use std::collections::{BTreeMap, BTreeSet};

pub enum Fix {
    DefaultValue { at: usize, value: &'static str },
    AddKey { key: String },
    RegisterRestClient { name: String },
    /// Byte-range edits of the checked document.
    Edits { title: String, edits: Vec<(usize, usize, String)> },
}

const HEALTH_QUALIFIERS: [&str; 4] = ["Liveness", "Readiness", "Wellness", "Health"];
const SCOPES: [&str; 4] = ["ApplicationScoped", "RequestScoped", "SessionScoped", "Singleton"];
pub const REGISTER_REST_CLIENT: &str = "org.eclipse.microprofile.rest.client.inject.RegisterRestClient";

fn import_edit(text: &str, fqcn: &str) -> Option<(usize, usize, String)> {
    if text.contains(&format!("import {fqcn};")) {
        return None;
    }
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        if line.starts_with("package ") {
            let prefix = if line.ends_with('\n') { "" } else { "\n" };
            return Some((offset, offset, format!("{prefix}import {fqcn};\n")));
        }
    }
    Some((0, 0, format!("import {fqcn};\n")))
}

/// Edits that put `annotation` on its own line before the declaration starting at `start`, with its import.
pub fn annotate_edits(text: &str, start: usize, annotation: &str, import: &str) -> Vec<(usize, usize, String)> {
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let indent = &text[line_start..start];
    let indent = if indent.trim().is_empty() { indent } else { "" };
    let mut edits = vec![(line_start, line_start, format!("{indent}@{annotation}\n"))];
    if let Some(import) = import_edit(text, import) {
        edits.push(import);
    }
    merge(edits)
}

fn merge(mut edits: Vec<(usize, usize, String)>) -> Vec<(usize, usize, String)> {
    edits.sort_by_key(|(a, b, _)| (*a, *b));
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    for edit in edits {
        match out.last_mut() {
            Some(last) if last.1 == edit.0 && last.0 == last.1 && edit.0 == edit.1 => last.2.push_str(&edit.2),
            _ => out.push(edit),
        }
    }
    out
}

fn with_import(text: &str, mut edits: Vec<(usize, usize, String)>, import: &str) -> Vec<(usize, usize, String)> {
    edits.extend(import_edit(text, import));
    merge(edits)
}

pub struct Problem {
    pub finding: Finding,
    pub fixes: Vec<Fix>,
}

pub struct Context<'a> {
    pub lookup: &'a Lookup<'a>,
    /// The squashed keys every config file and `.env` sets; `None` while the project is not fully read.
    pub configured: Option<&'a BTreeSet<String>>,
    /// Interface name to whether it carries `@RegisterRestClient`; `None` while the project is not fully read.
    pub rest_clients: Option<&'a BTreeMap<String, bool>>,
    pub unassigned: bool,
    pub annotations: bool,
    pub health_metrics: bool,
}

pub fn check(text: &str, ctx: &Context) -> Vec<Problem> {
    let mentions = |needle: &str| text.contains(needle);
    let mut out = Vec::new();
    let health_metrics = ctx.health_metrics && (mentions("HealthCheck") || mentions("@Liveness") || mentions("@Readiness") || mentions("@Wellness") || mentions("@Gauge"));
    if !(mentions("@ConfigProperty") || mentions("@RestClient") || mentions("@Scheduled") || health_metrics) {
        return out;
    }
    let file = java::parse(text);
    if ctx.unassigned && mentions("@ConfigProperty") {
        unassigned(&file, ctx, &mut out);
    }
    if ctx.annotations {
        rest_clients(&file, ctx, &mut out);
        scheduled(&file, ctx, &mut out);
    }
    if health_metrics {
        health(&file, text, &mut out);
        gauges(&file, text, &mut out);
    }
    out
}

fn default_for(ty: &str) -> &'static str {
    match ty {
        "int" | "long" | "short" | "byte" | "double" | "float" | "Integer" | "Long" | "Short" | "Byte" | "Double" | "Float" => "0",
        "boolean" | "Boolean" => "false",
        _ => "",
    }
}

fn unassigned(file: &JavaFile, ctx: &Context, out: &mut Vec<Problem>) {
    let Some(configured) = ctx.configured.filter(|set| !set.is_empty()) else { return };
    for Injection { key, at, end, optional, has_default, ty, insert_at } in injections(file) {
        let provided = key.contains("${")
            || key.starts_with("quarkus.")
            || ctx.lookup.prop(&key).is_some_and(|p| !p.default.is_empty())
            || configured.contains(&squash(strip_profile(&key)));
        if optional || has_default || provided || key.is_empty() {
            continue;
        }
        let message = format!("`{key}` has no default and no application config file or .env sets it. Quarkus fails at startup unless a system property, the environment or another source supplies it.");
        out.push(Problem {
            finding: Finding::new(at, end, "unassigned-property", message),
            fixes: vec![Fix::DefaultValue { at: insert_at, value: default_for(&ty) }, Fix::AddKey { key }],
        });
    }
}

fn rest_clients(file: &JavaFile, ctx: &Context, out: &mut Vec<Problem>) {
    let Some(known) = ctx.rest_clients else { return };
    let mut injected = Vec::new();
    for t in &file.types {
        for m in &t.members {
            if let (Some(_), Some(ty)) = (m.annotation("RestClient"), &m.ty) {
                injected.push((ty, m.at, m.name.len()));
            }
            for p in &m.params {
                if p.annotation("RestClient").is_some() {
                    injected.push((&p.ty, p.at, p.name.len()));
                }
            }
        }
    }
    for (ty, at, len) in injected {
        if known.get(&ty.name) != Some(&false) {
            continue;
        }
        let message = format!("`{}` is injected with @RestClient but is not annotated with @RegisterRestClient.", ty.name);
        out.push(Problem { finding: Finding::new(at, at + len, "rest-client-not-registered", message), fixes: vec![Fix::RegisterRestClient { name: ty.name.clone() }] });
    }
}

fn health(file: &JavaFile, text: &str, out: &mut Vec<Problem>) {
    for t in file.types.iter().filter(|t| t.kind == "class" && t.outer.is_none() && !t.modifiers.iter().any(|m| m == "abstract")) {
        let implements = t.implements.iter().any(|i| matches!(i.name.as_str(), "HealthCheck" | "AsyncHealthCheck"));
        let qualifier = t.annotations.iter().find(|a| HEALTH_QUALIFIERS.contains(&a.name.as_str()));

        if let Some(qualifier) = qualifier.filter(|_| !implements && t.extends.is_empty()) {
            let message = format!("`{}` is annotated with @{} but does not implement HealthCheck, so it is not a health check.", t.name, qualifier.name);
            if let Some(brace) = text[t.at..].find('{').map(|i| t.at + i) {
                let header_end = text[..brace].trim_end().len();
                let insert = if t.implements.is_empty() { " implements HealthCheck" } else { ", HealthCheck" };
                let edits = with_import(text, vec![(header_end, header_end, insert.to_owned())], "org.eclipse.microprofile.health.HealthCheck");
                out.push(Problem {
                    finding: Finding::new(qualifier.at, qualifier.end, "health-not-implemented", message),
                    fixes: vec![Fix::Edits { title: format!("Let {} implement HealthCheck", t.name), edits }],
                });
            }
        }

        let Some(check) = t.implements.iter().find(|i| i.name == "HealthCheck").filter(|_| qualifier.is_none()) else { continue };
        let message = format!("`{}` implements HealthCheck but has no @Liveness, @Readiness or @Wellness annotation, so Quarkus does not register it.", t.name);
        let fixes = ["Liveness", "Readiness"]
            .iter()
            .map(|q| Fix::Edits { title: format!("Annotate {} with @{q}", t.name), edits: annotate_edits(text, t.start, q, &format!("org.eclipse.microprofile.health.{q}")) })
            .collect();
        out.push(Problem { finding: Finding::new(check.at, check.at + check.name.len(), "health-without-qualifier", message), fixes });
    }
}

fn gauges(file: &JavaFile, text: &str, out: &mut Vec<Problem>) {
    for t in file.types.iter().filter(|t| t.kind == "class") {
        let scopes: Vec<_> = t.annotations.iter().filter(|a| SCOPES.contains(&a.name.as_str()) || a.name == "Dependent").collect();
        let scoped = scopes.iter().any(|a| a.name == "ApplicationScoped");

        for (member, gauge) in t.members.iter().filter_map(|m| m.annotation("Gauge").map(|a| (m, a))) {
            if !scoped && !scopes.is_empty() {
                let message = format!("@Gauge needs an @ApplicationScoped bean, `{}` has another scope.", t.name);
                let edits = scopes.iter().take(1).map(|a| (a.at, a.end, "@ApplicationScoped".to_owned())).collect();
                let edits = with_import(text, edits, "jakarta.enterprise.context.ApplicationScoped");
                out.push(Problem {
                    finding: Finding::new(gauge.at, gauge.end, "gauge-scope", message),
                    fixes: vec![Fix::Edits { title: "Use @ApplicationScoped".into(), edits }],
                });
            }

            if matches!(member.ty.as_ref().map(|ty| ty.name.as_str()), Some("void" | "String" | "boolean" | "Boolean" | "char" | "Character")) {
                let message = format!("`{}` is annotated with @Gauge but does not return a number.", member.name);
                out.push(Problem { finding: Finding::new(gauge.at, gauge.end, "gauge-return-type", message), fixes: Vec::new() });
            }
        }
    }
}

fn cron_fields_are_free(ctx: &Context) -> bool {
    ctx.configured.is_some_and(|set| !set.contains(&squash("quarkus.scheduler.cron-type")))
}

fn scheduled(file: &JavaFile, ctx: &Context, out: &mut Vec<Problem>) {
    let annotations: Vec<&Annotation> = file.types.iter().flat_map(|t| &t.members).filter_map(|m| m.annotation("Scheduled")).collect();
    for a in annotations {
        let literal = |name: &str| {
            let arg = a.arg(name)?;
            let [value] = arg.strings.as_slice() else { return None };
            (arg.text.matches('"').count() == 2 && arg.text.starts_with('"') && arg.text.ends_with('"') && !value.value.contains(['{', '$'])).then_some((arg, value))
        };
        if let Some((_, cron)) = literal("cron") {
            let fields = cron.value.split_whitespace().count();
            let off = matches!(cron.value.as_str(), "off" | "disabled");
            if cron_fields_are_free(ctx) && !off && !(6..=7).contains(&fields) {
                let message = format!("A Quartz cron expression has 6 or 7 fields, `{}` has {fields}. Set quarkus.scheduler.cron-type for another syntax.", cron.value);
                out.push(Problem { finding: Finding::new(cron.at, cron.end, "invalid-cron", message), fixes: Vec::new() });
            }
        }
        if let Some((_, every)) = literal("every") {
            let value = every.value.trim();
            if !matches!(value, "off" | "disabled") && !duration(value) {
                let message = format!("`{value}` is not a duration such as `10s`, `5m` or `PT1H`.");
                out.push(Problem { finding: Finding::new(every.at, every.end, "invalid-every", message), fixes: Vec::new() });
            }
        }
        if a.arg("cron").is_none() && a.arg("every").is_none() {
            let message = "@Scheduled needs `cron` or `every`.";
            out.push(Problem { finding: Finding::new(a.at, a.name_at + a.name.len(), "scheduled-without-trigger", message), fixes: Vec::new() });
        }
        if a.arg("cron").is_some() && a.arg("every").is_some() {
            let message = "@Scheduled takes either `cron` or `every`, not both.";
            out.push(Problem { finding: Finding::new(a.at, a.end, "scheduled-both-triggers", message), fixes: Vec::new() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled;

    fn run(text: &str, configured: &[&str], clients: &[(&str, bool)]) -> Vec<(&'static str, String)> {
        let configured: BTreeSet<String> = configured.iter().map(|k| squash(k)).collect();
        let clients: BTreeMap<String, bool> = clients.iter().map(|(n, r)| ((*n).to_owned(), *r)).collect();
        let lookup = Lookup(vec![bundled()]);
        let ctx = Context { lookup: &lookup, configured: Some(&configured), rest_clients: Some(&clients), unassigned: true, annotations: true, health_metrics: true };
        check(text, &ctx).iter().map(|p| (p.finding.code, text[p.finding.start..p.finding.end].to_owned())).collect()
    }

    #[test]
    fn unassigned_property_is_reported_only_when_nothing_provides_it() {
        let text = r#"class A {
            @ConfigProperty(name = "app.a") String a;
            @ConfigProperty(name = "app.b", defaultValue = "x") String b;
            @ConfigProperty(name = "app.c") Optional<String> c;
            @ConfigProperty(name = "app.d") String d;
            @ConfigProperty(name = "quarkus.http.port") int port;
            @ConfigProperty(name = "quarkus.acme") String acme;
            @ConfigProperty(name = "${x}") String e;
            @ConfigProperty(name = "app." + SUFFIX) String f;
            @ConfigProperty String g;
        }"#;
        assert_eq!(run(text, &["app.d", "other"], &[]), [("unassigned-property", "app.a".into())]);
        assert!(run(text, &[], &[]).is_empty());
    }

    #[test]
    fn rest_client_must_be_registered() {
        let text = "class A { @Inject @RestClient Foo foo; @Inject @RestClient Bar bar; @Inject @RestClient Lib lib; }";
        assert_eq!(run(text, &[], &[("Foo", false), ("Bar", true)]), [("rest-client-not-registered", "foo".into())]);
    }

    #[test]
    fn scheduled_expressions_are_checked() {
        let text = r#"class J {
            @Scheduled(cron = "0 0/5 * * * ?") void ok() {}
            @Scheduled(cron = "0/5 * * * *") void five() {}
            @Scheduled(cron = "{job.cron}") void configured() {}
            @Scheduled(every = "10s") void every() {}
            @Scheduled(every = "soon") void bad() {}
            @Scheduled(identity = "x") void none() {}
            @Scheduled(cron = "off") void off() {}
        }"#;
        let found = run(text, &["other"], &[]);
        let codes: Vec<_> = found.iter().map(|(c, _)| *c).collect();
        assert_eq!(codes, ["invalid-cron", "invalid-every", "scheduled-without-trigger"]);
        assert_eq!(found[0].1, "0/5 * * * *");
        let other_syntax = run(text, &["quarkus.scheduler.cron-type"], &[]);
        assert!(!other_syntax.iter().any(|(c, _)| *c == "invalid-cron"));
    }

    fn apply(text: &str, edits: &[(usize, usize, String)]) -> String {
        let mut out = text.to_owned();
        for (a, b, new) in edits.iter().rev() {
            out.replace_range(*a..*b, new);
        }
        out
    }

    fn fixed(text: &str) -> Vec<(&'static str, Vec<String>)> {
        let lookup = Lookup(vec![bundled()]);
        let ctx = Context { lookup: &lookup, configured: None, rest_clients: None, unassigned: false, annotations: false, health_metrics: true };
        check(text, &ctx)
            .into_iter()
            .map(|p| {
                let results = p.fixes.iter().filter_map(|f| if let Fix::Edits { edits, .. } = f { Some(apply(text, edits)) } else { None }).collect();
                (p.finding.code, results)
            })
            .collect()
    }

    #[test]
    fn a_health_qualifier_without_the_interface_offers_to_implement_it() {
        let text = "package a;\n\n@Liveness\npublic class Ping {\n}\n";
        let found = fixed(text);
        assert_eq!(found[0].0, "health-not-implemented");
        assert_eq!(found[0].1[0], "package a;\nimport org.eclipse.microprofile.health.HealthCheck;\n\n@Liveness\npublic class Ping implements HealthCheck {\n}\n");
        assert!(fixed("@Liveness class Ping implements HealthCheck {}").is_empty());
        assert!(fixed("@Liveness class Ping extends Base {}").is_empty());
    }

    #[test]
    fn a_health_check_without_a_qualifier_offers_liveness_and_readiness() {
        let text = "import x.Y;\n\n@ApplicationScoped\npublic class Ping implements HealthCheck {\n}\n";
        let found = fixed(text);
        assert_eq!(found[0].0, "health-without-qualifier");
        assert_eq!(found[0].1.len(), 2);
        assert!(found[0].1[0].contains("@Liveness\n@ApplicationScoped") || found[0].1[0].contains("@ApplicationScoped"));
        assert!(found[0].1[0].contains("import org.eclipse.microprofile.health.Liveness;"));
        assert!(found[0].1[1].contains("@Readiness\n"));
        assert!(fixed("@Readiness class Ping implements HealthCheck {}").is_empty());
        assert!(fixed("abstract class Base implements HealthCheck {}").is_empty());
    }

    #[test]
    fn gauges_need_an_application_scoped_bean_and_a_number() {
        let text = "@RequestScoped\nclass M {\n  @Gauge(name = \"a\") public int ok() { return 1; }\n  @Gauge(name = \"b\") public String bad() { return \"\"; }\n}\n";
        let found = fixed(text);
        assert_eq!(found.iter().map(|f| f.0).collect::<Vec<_>>(), ["gauge-scope", "gauge-scope", "gauge-return-type"]);
        assert!(found[0].1[0].starts_with("import jakarta.enterprise.context.ApplicationScoped;\n@ApplicationScoped\nclass M"));
        assert!(fixed("@ApplicationScoped class M { @Gauge(name = \"a\") int ok() { return 1; } }").is_empty());
        assert!(fixed("class M { @Gauge(name = \"a\") int ok() { return 1; } }").is_empty());
    }
}
