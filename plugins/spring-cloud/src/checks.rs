//! Text checks that need the whole project: Stream bindings that no
//! configuration declares and a bootstrap file whose starter is missing. Each
//! stays quiet when files are unread, a name is dynamic or the build is unclear.

use crate::{JavaFacts, StreamConfig, function_of, functions};
use openidea_plugin::{Severity, host};
use spring_text::config;
use spring_text::finding::Finding;
use spring_text::index::Index;
use spring_text::source_calls;

pub const BUILD_GLOBS: &[&str] = &[
    "**/pom.xml",
    "**/build.gradle",
    "**/build.gradle.kts",
    "**/libs.versions.toml",
    "**/application*.yml",
    "**/application*.yaml",
    "**/application*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
    "**/bootstrap*.properties",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub stream_bindings: bool,
    pub bootstrap: bool,
    pub legacy_stream: bool,
}

impl Groups {
    pub fn from_settings() -> Self {
        let on = |key: &str| host::setting_bool(&format!("springCloud.{key}")).unwrap_or(true);
        Self { stream_bindings: on("checkStreamBindings"), bootstrap: on("checkBootstrap"), legacy_stream: on("checkLegacyStream") }
    }
}

/// Whether a binding name is declared by the configuration or derived from a function the project can see.
fn bound(java: &Index<JavaFacts>, config: &Index<StreamConfig>, name: &str) -> bool {
    let declared = |c: &StreamConfig| {
        c.bindings.iter().any(|b| b.name == name)
            || c.aliases.iter().any(|a| a == name)
            || c.declared.iter().any(|d| d == name || *name == format!("{d}-out-0") || *name == format!("{d}-in-0"))
    };
    if config.iter().any(|(_, c)| declared(c)) {
        return true;
    }
    let Some(function) = function_of(name) else { return false };
    functions(java, function).next().is_some()
        || config.iter().flat_map(|(_, c)| &c.definitions).any(|d| {
            d.iter().any(|n| n.name == function) || d.iter().map(|n| n.name.as_str()).collect::<String>() == function
        })
}

/// Both indexes are fully read, the project declares functions or bindings, and the name is a plain literal.
fn visible(java: &Index<JavaFacts>, config: &Index<StreamConfig>) -> bool {
    java.complete()
        && config.complete()
        && (java.iter().any(|(_, f)| !f.functions.is_empty()) || config.iter().any(|(_, c)| !c.bindings.is_empty()))
}

/// `StreamBridge.send("name", …)` creates a binding on the fly when none is configured: with the project's bindings spelled out in configuration, a name that matches none is most likely a typo.
pub fn stream_channel_unbound(
    path: &str,
    text: &str,
    java: &Index<JavaFacts>,
    config: &Index<StreamConfig>,
) -> Vec<Finding> {
    if !text.contains("StreamBridge") || !visible(java, config) || config.iter().all(|(_, c)| c.bindings.is_empty()) {
        return Vec::new();
    }
    source_calls(path, text)
        .into_iter()
        .filter(|c| c.name == "send" && c.receiver.is_some() && c.args.len() >= 2)
        .filter_map(|c| c.args.first()?.string.clone())
        .filter(|s| !s.value.contains("${") && !s.value.is_empty() && !bound(java, config, &s.value))
        .map(|s| {
            Finding::new(
                s.at,
                s.end,
                "stream-channel-unbound",
                format!(
                    "No binding `{0}` is configured under spring.cloud.stream.bindings; StreamBridge creates one on the fly with default settings (destination `{0}`, no group).",
                    s.value
                ),
            )
            .with_severity(Severity::Info)
        })
        .collect()
}

/// Whether `name`, a binder-specific binding such as `spring.cloud.stream.kafka.bindings.<name>.consumer.…`, is declared nowhere else.
pub fn binder_binding_unbound(
    name: &str,
    java: &Index<JavaFacts>,
    config: &Index<StreamConfig>,
) -> bool {
    !name.contains("${") && visible(java, config) && !bound(java, config, name)
}

/// What a build file or configuration file says about Spring Cloud's bootstrap context.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Build {
    pub is_build_file: bool,
    /// `spring-cloud-starter-bootstrap` (also as a catalog alias).
    pub has_starter: bool,
    /// `spring.cloud.bootstrap.enabled=true` or `spring.config.use-legacy-processing=true`.
    pub enables_bootstrap: bool,
    /// Spring Boot's (major, minor) when the file states it as a literal.
    pub boot_version: Option<(u32, u32)>,
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn read_build(path: &str, text: &str) -> Option<Build> {
    let is_build_file = matches!(file_name(path), "pom.xml" | "build.gradle" | "build.gradle.kts" | "libs.versions.toml");
    let lower = text.to_ascii_lowercase();
    let has_starter = lower.contains("starter-bootstrap") || lower.contains("starter.bootstrap") || lower.contains("starterbootstrap");
    let mut enables_bootstrap = false;
    if config::is_config_file(path) {
        enables_bootstrap = config::keys(path, text).iter().any(|k| {
            let key = config::canonical(&k.key);
            matches!(key.as_str(), "spring.cloud.bootstrap.enabled" | "spring.config.use-legacy-processing")
                && k.value.eq_ignore_ascii_case("true")
        });
    } else if is_build_file {
        enables_bootstrap = lower.contains("cloud.bootstrap.enabled") || lower.contains("use-legacy-processing");
    }
    let boot_version = if is_build_file { boot_version(path, text) } else { None };
    (is_build_file || has_starter || enables_bootstrap).then_some(Build { is_build_file, has_starter, enables_bootstrap, boot_version })
}

fn parse_version(raw: &str) -> Option<(u32, u32)> {
    let mut parts = raw.trim().split(['.', '-']);
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn boot_version(path: &str, text: &str) -> Option<(u32, u32)> {
    if file_name(path) == "pom.xml" {
        let from = ["spring-boot-starter-parent", "spring-boot-dependencies"].iter().find_map(|a| text.find(a))?;
        let rest = &text[from..];
        let scope = &rest[..rest.find("</parent>").or_else(|| rest.find("</dependency>")).unwrap_or(rest.len())];
        let version = scope.split("<version>").nth(1)?.split("</version>").next()?;
        return parse_version(version);
    }
    text.lines().filter(|l| l.contains("org.springframework.boot")).find_map(|line| {
        let after = ["spring-boot-gradle-plugin:", "version"].iter().find_map(|m| line.rsplit_once(m).map(|(_, r)| r))?;
        let quote = after.find(['\'', '"'])?;
        let literal = &after[quote + 1..];
        parse_version(&literal[..literal.find(['\'', '"'])?])
    })
}

pub fn is_bootstrap_file(path: &str) -> bool {
    config::is_config_file(path) && file_name(path).starts_with("bootstrap")
}

/// Since Spring Boot 2.4 (Spring Cloud 2020.0) the bootstrap context is off unless the `spring-cloud-starter-bootstrap` starter is on the classpath or `spring.cloud.bootstrap.enabled=true` is set; `spring.config.import` replaces it. A bootstrap file is then silently ignored.
pub fn bootstrap_config_missing_dependency(path: &str, text: &str, builds: &Index<Build>) -> Vec<Finding> {
    if !is_bootstrap_file(path) || !builds.complete() {
        return Vec::new();
    }
    let files: Vec<&Build> = builds.iter().map(|(_, b)| b).collect();
    let versions: Vec<(u32, u32)> = files.iter().filter_map(|b| b.boot_version).collect();
    let modern = !versions.is_empty() && versions.iter().all(|v| *v >= (2, 4));
    if !modern || files.iter().any(|b| b.has_starter || b.enables_bootstrap) {
        return Vec::new();
    }
    let Some(first) = config::keys(path, text).into_iter().next() else { return Vec::new() };
    vec![Finding::new(
        first.at,
        first.end,
        "bootstrap-config-missing-dependency",
        "Since Spring Boot 2.4 this bootstrap file is ignored unless spring-cloud-starter-bootstrap is a dependency or spring.cloud.bootstrap.enabled=true is set. Prefer spring.config.import in application.* instead.",
    )]
}

const BUILT_IN_CHANNELS: [&str; 2] = ["input", "output"];
const BUILT_IN_BINDINGS: [&str; 3] = ["Sink", "Source", "Processor"];

/// Problems of the legacy annotation model (`@StreamListener`, `@SendTo`, `@BootstrapConfiguration`), judged
/// only against channels the project declares itself; a `@EnableBinding` of a library interface keeps the check quiet.
pub fn legacy_stream(path: &str, text: &str, java: &Index<JavaFacts>) -> Vec<Finding> {
    if !["StreamListener", "BootstrapConfiguration"].iter().any(|w| text.contains(w)) {
        return Vec::new();
    }
    let file = spring_text::parse_source(path, text);
    let mut out = Vec::new();
    let channels_known = channels_visible(java);
    for ty in &file.types {
        if let Some(a) = ty.annotation("BootstrapConfiguration")
            && ty.has(&["Configuration", "Component"])
        {
            out.push(
                Finding::new(a.at, a.end, "bootstrap-configuration-scanned", "A `@BootstrapConfiguration` class that is also a `@Configuration` or `@Component` can be found by component scanning and loaded twice; register it only in spring.factories.")
                    .with_severity(Severity::Hint),
            );
        }
        for m in ty.methods() {
            let Some(listener) = m.annotation("StreamListener") else { continue };
            let send_to = m.annotation("SendTo");
            let returns = m.ty.as_ref().is_some_and(|t| t.text != "void" && t.text != "Unit");
            let output = send_to.is_some() || m.annotation("Output").is_some() || m.params.iter().any(|p| p.annotation("Output").is_some());
            if returns && !output {
                out.push(
                    Finding::new(m.at, m.at + m.name.len(), "stream-listener-return", "This `@StreamListener` method returns a value but has no `@SendTo` or `@Output`, so the result goes nowhere.")
                        .with_severity(Severity::Info),
                );
            }
            out.extend(handler_signature(m, listener, returns));
            if !returns && let Some(a) = send_to {
                out.push(
                    Finding::new(a.at, a.end, "send-to-without-return", "`@SendTo` has nothing to send: the method returns void.").with_severity(Severity::Info),
                );
            }
            let Some(known) = channels_known.as_ref() else { continue };
            for s in listener.strings(&["value", "target"]).into_iter().chain(send_to.map(|a| a.strings(&["value"])).unwrap_or_default()) {
                if s.value.contains("${") || known.contains(&s.value) || BUILT_IN_CHANNELS.contains(&s.value.as_str()) {
                    continue;
                }
                out.push(Finding::new(s.at, s.end, "stream-listener-channel", format!("No `@Input` or `@Output` channel `{}` is declared in the project.", s.value)));
            }
        }
    }
    out
}

/// The signature rules Spring Cloud Stream enforces when it binds a `@StreamListener` method; each fails at startup.
fn handler_signature(m: &spring_text::java::Member, listener: &spring_text::java::Annotation, returns: bool) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut warn = |at: usize, end: usize, code: &'static str, message: &'static str| out.push(Finding::new(at, end, code, message));
    let on_listener = listener.arg("value").is_some() || listener.arg("target").is_some();
    let input_params: Vec<_> = m.params.iter().filter(|p| p.annotation("Input").is_some()).collect();
    let output_params: Vec<_> = m.params.iter().filter(|p| p.annotation("Output").is_some()).collect();
    if on_listener && let Some(p) = input_params.first() {
        warn(p.at, p.at + p.name.len(), "stream-listener-input-twice", "The input is named both on `@StreamListener` and on a parameter's `@Input`; use one of them.");
    }
    if !on_listener && input_params.is_empty() {
        warn(m.at, m.at + m.name.len(), "stream-listener-no-input", "No input binding: give `@StreamListener` a value or annotate a parameter with `@Input`.");
    }
    let method_output = m.annotation("SendTo").or_else(|| m.annotation("Output"));
    if let (Some(a), Some(_)) = (method_output, output_params.first()) {
        warn(a.at, a.end, "stream-listener-output-twice", "The output is named both on the method and on a parameter's `@Output`; use one of them.");
    }
    if returns && let Some(p) = output_params.first() {
        warn(p.at, p.at + p.name.len(), "stream-listener-output-param-return", "A method with an `@Output` parameter must return void.");
    }
    if let Some(condition) = listener.arg("condition")
        && m.params.iter().any(|p| matches!(p.ty.name.as_str(), "Flux" | "Mono" | "Publisher"))
    {
        warn(condition.at, condition.end, "stream-listener-condition-reactive", "A `condition` cannot be used with a reactive (`Flux`, `Mono`, `Publisher`) parameter.");
    }
    out
}

/// The channels the project declares, once every file was read and each `@EnableBinding` type is a project interface or a built-in one.
fn channels_visible(java: &Index<JavaFacts>) -> Option<Vec<String>> {
    if !java.complete() {
        return None;
    }
    let interfaces: Vec<&String> = java.iter().flat_map(|(_, f)| &f.channel_interfaces).collect();
    let bindings: Vec<&String> = java.iter().flat_map(|(_, f)| &f.enable_bindings).collect();
    let all_visible = bindings.iter().all(|b| interfaces.contains(b) || BUILT_IN_BINDINGS.contains(&b.as_str()));
    (!bindings.is_empty() && all_visible).then(|| java.iter().flat_map(|(_, f)| f.channels.clone()).collect())
}
