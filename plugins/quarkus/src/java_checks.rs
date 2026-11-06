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
}

pub fn check(text: &str, ctx: &Context) -> Vec<Problem> {
    let mentions = |needle: &str| text.contains(needle);
    let mut out = Vec::new();
    if !(mentions("@ConfigProperty") || mentions("@RestClient") || mentions("@Scheduled")) {
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
        out.push(Problem { finding: Finding::new(at, at + len, "rest-client-not-registered", message), fixes: Vec::new() });
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
        let ctx = Context { lookup: &lookup, configured: Some(&configured), rest_clients: Some(&clients), unassigned: true, annotations: true };
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
}
