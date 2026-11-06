//! Spring Cloud Tools for OpenIDEA: OpenFeign client methods ↔ the controller
//! endpoints they call, Spring Cloud Stream function beans ↔ their bindings in
//! configuration files, and Spring Cloud Gateway route predicates and filters.
//! Text-based, Java and Kotlin: names and paths match as written.

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Location, Plugin, Position, Range, Severity, host, register_plugin};
use spring_text::config::{self, Key};
use spring_text::index::{Index, Indexes, PROVIDER_BUDGET, project_of};
use spring_text::java;
use spring_text::{SOURCE_GLOBS, contains, parse_source, range, source_calls};

const JAVA_GLOBS: &[&str] = SOURCE_GLOBS;
const CONFIG_GLOBS: &[&str] = &[
    "**/application*.yml",
    "**/application*.yaml",
    "**/application*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
    "**/bootstrap*.properties",
];
/// Words a Java file needs to hold anything this plugin indexes.
const JAVA_HINTS: [&str; 7] = ["FeignClient", "Controller", "Function", "Consumer", "Supplier", "send(", "Factory"];
const FUNCTIONAL: [&str; 3] = ["Function", "Consumer", "Supplier"];
const BINDINGS: &str = "spring.cloud.stream.bindings.";
const FUNCTION_BINDINGS: &str = "spring.cloud.stream.function.bindings.";
const DEFINITION: &str = "spring.cloud.function.definition";
/// Bindings declared without a function (for `StreamBridge`): `foo` makes `foo-out-0`.
const DECLARED: [&str; 3] =
    ["spring.cloud.stream.output-bindings", "spring.cloud.stream.input-bindings", "spring.cloud.stream.source"];
/// Route lists: (prefix, WebMVC server).
const ROUTES: [(&str, bool); 4] = [
    ("spring.cloud.gateway.routes", false),
    ("spring.cloud.gateway.server.webflux.routes", false),
    ("spring.cloud.gateway.server.webmvc.routes", true),
    ("spring.cloud.gateway.mvc.routes", true),
];
/// The route predicate factories Spring Cloud Gateway ships.
const PREDICATES: [&str; 14] = [
    "After",
    "Before",
    "Between",
    "Cookie",
    "Header",
    "Host",
    "Method",
    "Path",
    "Query",
    "ReadBody",
    "RemoteAddr",
    "XForwardedRemoteAddr",
    "Weight",
    "CloudFoundryRouteService",
];
/// The gateway filter factories Spring Cloud Gateway ships.
const FILTERS: [&str; 38] = [
    "AddRequestHeader",
    "AddRequestHeadersIfNotPresent",
    "AddRequestParameter",
    "AddResponseHeader",
    "CircuitBreaker",
    "CacheRequestBody",
    "DedupeResponseHeader",
    "FallbackHeaders",
    "JsonToGrpc",
    "LocalResponseCache",
    "MapRequestHeader",
    "ModifyRequestBody",
    "ModifyResponseBody",
    "PrefixPath",
    "PreserveHostHeader",
    "RedirectTo",
    "RemoveJsonAttributesResponseBody",
    "RemoveRequestHeader",
    "RemoveRequestParameter",
    "RemoveResponseHeader",
    "RequestHeaderSize",
    "RequestHeaderToRequestUri",
    "RequestRateLimiter",
    "RewriteLocationResponseHeader",
    "RewritePath",
    "RewriteRequestParameter",
    "RewriteResponseHeader",
    "SaveSession",
    "SecureHeaders",
    "SetPath",
    "SetRequestHeader",
    "SetResponseHeader",
    "SetRequestHostHeader",
    "SetStatus",
    "StripPrefix",
    "Retry",
    "RequestSize",
    "TokenRelay",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub verb: String,
    pub path: String,
    /// The method name.
    pub range: Range,
    /// `Class.method()`.
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionBean {
    pub name: String,
    /// `Function`, `Consumer` or `Supplier`.
    pub kind: String,
    pub range: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JavaFacts {
    /// Feign client methods.
    pub clients: Vec<Endpoint>,
    /// Controller methods.
    pub servers: Vec<Endpoint>,
    /// Feign methods without a request mapping; `true` when the client sets its
    /// own `configuration` (which may bring another contract).
    pub unmapped: Vec<(Range, bool)>,
    pub functions: Vec<FunctionBean>,
    /// String literals passed first to a `send(…)` call (`StreamBridge` destinations).
    pub sent: Vec<String>,
    /// Names of the project's own route predicate and gateway filter factories.
    pub factories: Vec<String>,
}

pub fn java_facts(text: &str) -> JavaFacts {
    facts_in("A.java", text)
}

pub fn facts_in(path: &str, text: &str) -> JavaFacts {
    let file = parse_source(path, text);
    let span = |at: usize, s: &str| range(text, at, at + s.len());
    let mut out = JavaFacts::default();
    for c in &file.types {
        if let Some(feign) = c.annotation("FeignClient") {
            let prefix = feign.strings(&["path"]).first().map(|s| s.value.clone()).unwrap_or_default();
            let abstract_methods = c.methods().filter(|m| {
                m.body.is_none() && !m.modifiers.iter().any(|x| x == "static" || x == "default" || x == "private")
            });
            for m in abstract_methods {
                let mut found = java::mappings(c, m);
                // Feign's own contract: `@RequestLine("GET /users/{id}?full={full}")`.
                if let Some(line) = m.annotation("RequestLine").and_then(|a| a.strings(&["value"]).first().copied())
                    && let Some((verb, path)) = line.value.trim().split_once(' ')
                {
                    found.push((verb.to_owned(), path.trim().split('?').next().unwrap_or("").to_owned()));
                }
                if found.is_empty() {
                    out.unmapped.push((span(m.at, &m.name), feign.arg("configuration").is_some()));
                }
                for (verb, path) in found {
                    out.clients.push(Endpoint {
                        // Spring Cloud OpenFeign sends a `@RequestMapping` without a method as GET.
                        verb: if verb.is_empty() { "GET".into() } else { verb },
                        path: java::join_paths(&prefix, &path),
                        range: span(m.at, &m.name),
                        label: format!("{}.{}()", c.name, m.name),
                    });
                }
            }
        } else if c.has(&["RestController", "Controller"]) {
            for m in c.methods() {
                for (verb, path) in java::mappings(c, m) {
                    let label = format!("{}.{}()", c.name, m.name);
                    out.servers.push(Endpoint { verb, path, range: span(m.at, &m.name), label });
                }
            }
        }
        let factory = ["RoutePredicateFactory", "GatewayFilterFactory"].iter().find_map(|s| c.name.strip_suffix(s));
        let extends_factory = c.supers().any(|s| s.name.ends_with("PredicateFactory") || s.name.ends_with("FilterFactory"));
        if let Some(name) = factory.or(extends_factory.then_some(c.name.as_str())) {
            out.factories.push(name.to_owned());
        }
    }
    for b in java::bean_names(&file) {
        let kind = match file.types.iter().find(|t| t.at == b.at) {
            Some(class) => class.implements.iter().map(|t| t.name.as_str()).find(|n| FUNCTIONAL.contains(n)),
            None => FUNCTIONAL.iter().copied().find(|f| *f == b.ty),
        };
        if let Some(kind) = kind {
            out.functions.push(FunctionBean { name: b.name, kind: kind.to_owned(), range: range(text, b.at, b.at + b.len) });
        }
    }
    if text.contains("send(") {
        out.sent = source_calls(path, text)
            .into_iter()
            .filter(|c| c.name == "send")
            .filter_map(|c| Some(c.args.first()?.string.as_ref()?.value.clone()))
            .collect();
    }
    out
}

/// A name written in a configuration file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    pub name: String,
    pub range: Range,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamConfig {
    /// Binding names written under `spring.cloud.stream.bindings.` and `spring.cloud.stream.function.bindings.`.
    pub bindings: Vec<Name>,
    /// The values of `spring.cloud.stream.function.bindings.*` (binding aliases).
    pub aliases: Vec<String>,
    /// `spring.cloud.function.definition` entries (`;`), each split at `|`.
    pub definitions: Vec<Vec<Name>>,
    /// Names declared by `spring.cloud.stream.output-bindings` and the like.
    pub declared: Vec<String>,
}

/// The segment of `k` right after `prefix` (a binding name), when it is written on that key's line.
fn segment(text: &str, k: &Key, prefix: &str) -> Option<Name> {
    let name = k.key.strip_prefix(prefix)?.split('.').next().filter(|n| !n.is_empty())?;
    let written = &text[k.at..k.end];
    let written_from = k.key.len().checked_sub(written.len()).filter(|_| k.key.ends_with(written))?;
    let at = k.at + prefix.len().checked_sub(written_from)?;
    Some(Name { name: name.to_owned(), range: range(text, at, at + name.len()) })
}

pub fn stream_config(text: &str, keys: &[Key]) -> StreamConfig {
    let mut out = StreamConfig::default();
    for k in keys {
        out.bindings.extend([BINDINGS, FUNCTION_BINDINGS].iter().filter_map(|p| segment(text, k, p)));
        if k.key.strip_prefix(FUNCTION_BINDINGS).is_some_and(|b| !b.contains('.')) && !k.value.is_empty() {
            out.aliases.push(k.value.clone());
        }
        if DECLARED.contains(&k.key.as_str()) {
            out.declared.extend(k.value.split([';', ',']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned));
        }
        if k.key == DEFINITION {
            let mut entry = Vec::new();
            let mut from = 0;
            for (i, c) in k.value.char_indices().chain([(k.value.len(), ';')]) {
                if c != ';' && c != '|' {
                    continue;
                }
                let raw = &k.value[from..i];
                let name = raw.trim();
                if !name.is_empty() {
                    let at = k.value_at + from + raw.len() - raw.trim_start().len();
                    entry.push(Name { name: name.to_owned(), range: range(text, at, at + name.len()) });
                }
                if c == ';' && !entry.is_empty() {
                    out.definitions.push(std::mem::take(&mut entry));
                }
                from = i + 1;
            }
        }
    }
    out
}

/// The function a binding name is for: `process-in-0` → `process`.
fn function_of(binding: &str) -> Option<&str> {
    let (rest, n) = binding.rsplit_once('-')?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.strip_suffix("-in").or_else(|| rest.strip_suffix("-out")).filter(|f| !f.is_empty())
}

/// A composed definition's binding prefix: `uppercase|echo` → `uppercaseecho`.
fn composed(entry: &[Name]) -> String {
    entry.iter().map(|n| n.name.as_str()).collect()
}

/// `predicates` or `filters` when `key` is an item of a route's list, and whether it is a WebMVC route.
fn route_list(key: &str) -> Option<(&'static str, bool)> {
    let index = |s: &str| -> Option<usize> {
        let close = s.strip_prefix('[')?.find(']')? + 1;
        s[1..close].bytes().all(|b| b.is_ascii_digit()).then_some(close + 1)
    };
    let (rest, mvc) = ROUTES.iter().find_map(|(p, mvc)| Some((key.strip_prefix(p)?, *mvc)))?;
    let rest = rest[index(rest)?..].strip_prefix('.')?;
    let list = ["predicates", "filters"].into_iter().find(|l| rest.starts_with(l))?;
    (index(&rest[list.len()..])? == rest.len() - list.len()).then_some((list, mvc))
}

fn is_jvm(doc: &Document) -> bool {
    matches!(doc.language.as_str(), "java" | "kotlin")
}

fn java_file(path: &str, text: &str) -> Option<JavaFacts> {
    if !JAVA_HINTS.iter().any(|w| text.contains(w)) {
        return None;
    }
    let facts = facts_in(path, text);
    (facts != JavaFacts::default()).then_some(facts)
}

fn config_file(path: &str, text: &str) -> Option<StreamConfig> {
    if !config::is_config_file(path) {
        return None;
    }
    let found = stream_config(text, &config::keys(path, text));
    (found != StreamConfig::default()).then_some(found)
}

fn same_path(a: &str, b: &str) -> bool {
    let segments = |p: &str| p.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let (a, b) = (segments(a), segments(b));
    a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| x == y || x.starts_with('{') || y.starts_with('{'))
}

/// Whether a Feign `client` method calls the `server` endpoint (any service: the name isn't compared).
fn calls(client: &Endpoint, server: &Endpoint) -> bool {
    !client.path.contains("${")
        && !server.path.contains("${")
        && (server.verb.is_empty() || server.verb == client.verb)
        && same_path(&client.path, &server.path)
}

fn functions<'a>(java: &'a Index<JavaFacts>, name: &'a str) -> impl Iterator<Item = (&'a str, &'a FunctionBean)> {
    java.iter().flat_map(|(p, f)| f.functions.iter().map(move |b| (p, b))).filter(move |(_, b)| b.name == name)
}

fn definitions(config: &Index<StreamConfig>) -> impl Iterator<Item = &Vec<Name>> {
    config.iter().flat_map(|(_, c)| &c.definitions)
}

/// The function beans a binding's function name stands for (each part of a composed definition).
fn binding_targets(java: &Index<JavaFacts>, config: &Index<StreamConfig>, function: &str) -> Vec<Location> {
    let direct: Vec<Location> =
        functions(java, function).map(|(p, b)| Location { path: p.to_owned(), range: b.range }).collect();
    if !direct.is_empty() {
        return direct;
    }
    definitions(config)
        .filter(|d| d.len() > 1 && composed(d) == function)
        .flat_map(|d| d.iter().flat_map(|n| functions(java, &n.name)))
        .map(|(p, b)| Location { path: p.to_owned(), range: b.range })
        .collect()
}

/// Whether a binding's function name is backed by anything we can see.
fn known_binding(java: &Index<JavaFacts>, config: &Index<StreamConfig>, binding: &str, function: &str) -> bool {
    functions(java, function).next().is_some()
        || definitions(config).any(|d| composed(d) == function || d.iter().any(|n| n.name == function))
        || config.iter().any(|(_, c)| c.declared.iter().any(|d| d == function) || c.aliases.iter().any(|a| a == binding))
        || java.iter().any(|(_, f)| f.sent.iter().any(|s| s == binding || s == function))
}

/// The configuration places using function bean `name`: binding keys first, then definition entries.
fn uses(config: &Index<StreamConfig>, name: &str) -> Vec<(Location, String)> {
    let mut prefixes = vec![name.to_owned()];
    prefixes.extend(definitions(config).filter(|d| d.len() > 1 && d.iter().any(|n| n.name == name)).map(|d| composed(d)));
    let mut found: Vec<(Location, String)> = Vec::new();
    for (path, c) in config.iter() {
        for b in c.bindings.iter().filter(|b| function_of(&b.name).is_some_and(|f| prefixes.iter().any(|p| p == f))) {
            found.push((Location { path: path.to_owned(), range: b.range }, b.name.clone()));
        }
    }
    for (path, c) in config.iter() {
        for n in c.definitions.iter().flatten().filter(|n| n.name == name) {
            found.push((Location { path: path.to_owned(), range: n.range }, "spring.cloud.function.definition".into()));
        }
    }
    found
}

fn marker(range: Range, prefix: &str, found: &[(Location, String)]) -> Option<LineMarker> {
    let mut labels: Vec<&str> = found.iter().map(|(_, l)| l.as_str()).collect();
    labels.dedup();
    Some(LineMarker {
        line: range.start.line,
        icon: None,
        tooltip: format!("{prefix} {}", labels.join(", ")),
        target: Some(found.first()?.0.clone()),
    })
}

#[derive(Clone, Copy)]
enum Slot {
    /// A function name in `spring.cloud.function.definition`.
    Function,
    /// A binding name after `spring.cloud.stream.bindings.`.
    Binding,
    /// A route predicate or filter name.
    Route(&'static str),
}

/// What is typed at byte `at` of a configuration file, and where it starts.
fn config_slot(path: &str, text: &str, at: usize) -> Option<(Slot, usize)> {
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..at];
    let word_from = |seps: &[char]| before.rfind(seps).map_or(line_start, |i| line_start + i + 1);
    if config::keys(path, text).iter().any(|k| k.key == DEFINITION && k.at >= line_start && k.end <= at) {
        return Some((Slot::Function, word_from(&[';', '|', '=', ':', ' ', '"', '\''])));
    }
    let (key, value) = if path.ends_with(".properties") {
        match before.find(['=', ':']) {
            Some(i) => (before[..i].trim().to_owned(), Some(&before[i + 1..])),
            None => (before.trim_start().to_owned(), None),
        }
    } else {
        let (parent, typed) = config::yaml_parent(text, at);
        match route_list(&parent) {
            Some(_) => (parent, Some(&before[before.len() - typed.len()..])),
            None if typed.contains(':') => return None,
            None if parent.is_empty() => (typed, None),
            None => (format!("{parent}.{typed}"), None),
        }
    };
    match value {
        Some(value) if !value.contains('=') => {
            let list = route_list(&key)?.0;
            Some((Slot::Route(list), at - value.trim_start().len()))
        }
        Some(_) => None,
        None => {
            let rest = key.strip_prefix(BINDINGS).filter(|r| !r.contains('.'))?;
            Some((Slot::Binding, at - rest.len()))
        }
    }
}

pub struct CloudTools {
    java: Indexes<JavaFacts>,
    config: Indexes<StreamConfig>,
}

impl Default for CloudTools {
    fn default() -> Self {
        Self { java: Indexes::new(JAVA_GLOBS, java_file), config: Indexes::new(CONFIG_GLOBS, config_file) }
    }
}

impl CloudTools {
    /// Both indexes of the document's project, the document applied to its own.
    fn indexes(&mut self, doc: &Document) -> Option<(&mut Index<JavaFacts>, &mut Index<StreamConfig>)> {
        let project = project_of(&doc.path)?;
        let java = self.java.get(&project, PROVIDER_BUDGET / 2);
        let config = self.config.get(&project, PROVIDER_BUDGET / 2);
        if is_jvm(doc) {
            java.update(&doc.path, &doc.text);
        } else if config::is_config_file(&doc.path) {
            config.update(&doc.path, &doc.text);
        }
        Some((java, config))
    }

    fn java_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let facts = facts_in(&doc.path, &doc.text);
        let problems: Vec<Diagnostic> = facts
            .unmapped
            .iter()
            .map(|&(range, custom)| {
                let (severity, why) = if custom {
                    (Severity::Info, " (unless the client's configuration brings another contract)")
                } else {
                    (Severity::Warning, "")
                };
                let message = format!("Feign client method without a request mapping: Feign can't build its request{why}");
                Diagnostic::new(range, severity, message).code("feign-method-without-mapping")
            })
            .collect();
        let _ = host::publish_diagnostics(&doc.path, &problems);
        let Some((java, config)) = self.indexes(doc) else { return Vec::new() };
        let endpoints = |pick: fn(&JavaFacts) -> &Vec<Endpoint>, matches: &dyn Fn(&Endpoint) -> bool| {
            java.iter()
                .flat_map(|(p, f)| pick(f).iter().map(move |e| (p, e)))
                .filter(|(_, e)| matches(e))
                .map(|(p, e)| (Location { path: p.to_owned(), range: e.range }, format!("{} ({} {})", e.label, e.verb, e.path)))
                .collect::<Vec<_>>()
        };
        let mut markers = Vec::new();
        for c in &facts.clients {
            markers.extend(marker(c.range, "Calls", &endpoints(|f| &f.servers, &|s| calls(c, s))));
        }
        for s in &facts.servers {
            markers.extend(marker(s.range, "Called by Feign client", &endpoints(|f| &f.clients, &|c| calls(c, s))));
        }
        for f in &facts.functions {
            markers.extend(marker(f.range, "Stream bindings:", &uses(config, &f.name)));
        }
        markers.dedup_by_key(|m| m.line);
        markers
    }

    fn config_problems(&mut self, doc: &Document) {
        if !config::is_config_file(&doc.path) {
            return;
        }
        let keys = config::keys(&doc.path, &doc.text);
        let mine = stream_config(&doc.text, &keys);
        let Some((java, config)) = self.indexes(doc) else { return };
        let mut problems = Vec::new();
        // Functions may come from libraries or Kotlin: only check projects that declare some.
        if java.complete() && java.iter().any(|(_, f)| !f.functions.is_empty()) {
            for b in mine.bindings.iter().filter(|b| !b.name.contains("${")) {
                if let Some(function) = function_of(&b.name)
                    && !known_binding(java, config, &b.name, function)
                {
                    let message = format!(
                        "Binding `{}`: no function bean, function definition or StreamBridge destination is named `{function}`",
                        b.name
                    );
                    problems.push(Diagnostic::new(b.range, Severity::Warning, message).code("unknown-binding-function"));
                }
            }
            for n in mine.definitions.iter().flatten().filter(|n| !n.name.contains("${")) {
                if functions(java, &n.name).next().is_none() {
                    let message = format!("No Function, Consumer or Supplier bean is named `{}`", n.name);
                    problems.push(Diagnostic::new(n.range, Severity::Warning, message).code("unknown-function"));
                }
            }
        }
        if java.complete() {
            for k in &keys {
                let Some((list, mvc)) = route_list(&k.key) else { continue };
                let name = k.value.split('=').next().unwrap_or("").trim_end();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                let shipped: &[&str] = if list == "predicates" { &PREDICATES } else { &FILTERS };
                if shipped.contains(&name) || java.iter().any(|(_, f)| f.factories.iter().any(|n| n == name)) {
                    continue;
                }
                let kind = if list == "predicates" { "route predicate" } else { "gateway filter" };
                let message = format!("Unknown {kind} `{name}`: neither shipped with Spring Cloud Gateway nor a factory of this project");
                let severity = if mvc { Severity::Info } else { Severity::Warning };
                let at = range(&doc.text, k.value_at, k.value_at + name.len());
                problems.push(Diagnostic::new(at, severity, message).code(format!("unknown-gateway-{}", &list[..list.len() - 1])));
            }
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
    }
}

impl Providers for CloudTools {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !config::is_config_file(&doc.path) {
            return Vec::new();
        }
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        let Some((slot, from)) = config_slot(&doc.path, &doc.text, at) else { return Vec::new() };
        let replace = Some(range(&doc.text, from, at));
        let item = |label: String, kind: CompletionKind, detail: String| CompletionItem {
            detail: Some(detail),
            replace,
            ..CompletionItem::new(&label, kind)
        };
        if let Slot::Route(list) = slot {
            let (names, detail): (&[&str], _) =
                if list == "predicates" { (&PREDICATES, "Route predicate") } else { (&FILTERS, "Gateway filter") };
            return names.iter().map(|n| item(format!("{n}="), CompletionKind::Function, detail.into())).collect();
        }
        let Some((java, config)) = self.indexes(doc) else { return Vec::new() };
        let mut items: Vec<CompletionItem> = Vec::new();
        for f in java.iter().flat_map(|(_, f)| &f.functions) {
            let detail = format!("{} bean {}()", f.kind, f.name);
            if let Slot::Function = slot {
                items.push(item(f.name.clone(), CompletionKind::Function, detail));
                continue;
            }
            let directions: &[&str] = match f.kind.as_str() {
                "Consumer" => &["in"],
                "Supplier" => &["out"],
                _ => &["in", "out"],
            };
            items.extend(directions.iter().map(|d| item(format!("{}-{d}-0", f.name), CompletionKind::Property, detail.clone())));
        }
        if let Slot::Binding = slot {
            for d in definitions(config).filter(|d| d.len() > 1) {
                let detail = format!("Composed function {}", d.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join("|"));
                for dir in ["in", "out"] {
                    items.push(item(format!("{}-{dir}-0", composed(d)), CompletionKind::Property, detail.clone()));
                }
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        items
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if is_jvm(doc) {
            return self.java_markers(doc);
        }
        self.config_problems(doc);
        Vec::new()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if is_jvm(doc) {
            let facts = facts_in(&doc.path, &doc.text);
            let Some(client) = facts.clients.into_iter().find(|c| contains(c.range, at)) else { return Vec::new() };
            let Some((java, _)) = self.indexes(doc) else { return Vec::new() };
            return java
                .iter()
                .flat_map(|(p, f)| f.servers.iter().filter(|s| calls(&client, s)).map(move |s| (p, s)))
                .map(|(p, s)| Location { path: p.to_owned(), range: s.range })
                .collect();
        }
        if !config::is_config_file(&doc.path) {
            return Vec::new();
        }
        let mine = stream_config(&doc.text, &config::keys(&doc.path, &doc.text));
        let Some((java, config)) = self.indexes(doc) else { return Vec::new() };
        if let Some(function) = mine.bindings.iter().find(|b| contains(b.range, at)).and_then(|b| function_of(&b.name)) {
            return binding_targets(java, config, function);
        }
        mine.definitions
            .iter()
            .flatten()
            .filter(|n| contains(n.range, at))
            .flat_map(|n| functions(java, &n.name).map(|(p, b)| Location { path: p.to_owned(), range: b.range }))
            .collect()
    }
}

impl Plugin for CloudTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(CloudTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::text::position;

    const CLIENT: &str = r#"package com.acme.client;

@FeignClient(name = "users", path = "/api")
public interface UserClient {
    @GetMapping("/users/{id}")
    User find(@PathVariable("id") Long id);

    @PostMapping("/users")
    User create(@RequestBody User user);

    @RequestLine("DELETE /users/{id}?soft={soft}")
    void remove(@Param("id") Long id);

    User missing(Long id);

    default User cached(Long id) { return find(id); }
}
"#;

    const CONTROLLER: &str = r#"package com.acme.web;

@RestController
@RequestMapping("/api/users")
class UserController {
    @GetMapping("/{userId}")
    User get(@PathVariable Long userId) { return null; }

    @PostMapping
    User save(@RequestBody User u) { return null; }

    @GetMapping
    List<User> list() { return null; }
}
"#;

    const FUNCTIONS: &str = r#"package com.acme.stream;

@Configuration
class StreamFunctions {
    @Bean
    public Function<Order, Invoice> process() { return o -> null; }

    @Bean
    Consumer<Invoice> audit() { return i -> {}; }

    @Bean("ticks")
    Supplier<Long> clock() { return System::currentTimeMillis; }

    void publish(StreamBridge bridge) { bridge.send("alerts-out-0", "x"); }
}

@Component
class Uppercase implements Function<String, String> {
    public String apply(String s) { return s; }
}

class TokenGatewayFilterFactory extends AbstractGatewayFilterFactory<Object> {}
"#;

    const YAML: &str = "spring:
  cloud:
    function:
      definition: process;audit;uppercase|process;ghost
    stream:
      bindings:
        process-in-0:
          destination: orders
        uppercaseprocess-in-0:
          destination: raw
        ticks-out-0:
          destination: ticks
        alerts-out-0:
          destination: alerts
        missing-in-0:
          destination: x
        input:
          destination: y
      function:
        bindings:
          audit-in-0: input
    gateway:
      routes:
        - id: users
          uri: lb://users
          predicates:
            - Path=/users/**
            - Methd=GET
          filters:
            - StripPrefix=1
            - Token
            - Custom=1
";

    const PROPERTIES: &str = "spring.cloud.stream.bindings.audit-in-0.destination=invoices
spring.cloud.gateway.server.webmvc.routes[0].filters[0]=Bogus=1
spring.cloud.gateway.routes[0].predicates[0]=Host=**.acme.com
";

    const RESOURCES: &str = "src/main/resources";

    fn setup() -> MockHost {
        let host = MockHost::install();
        host.add_file("client/src/main/java/com/acme/client/UserClient.java", CLIENT);
        host.add_file("users/src/main/java/com/acme/web/UserController.java", CONTROLLER);
        host.add_file("src/main/java/com/acme/stream/StreamFunctions.java", FUNCTIONS);
        host.add_file(&format!("{RESOURCES}/application.yml"), YAML);
        host.add_file(&format!("{RESOURCES}/application.properties"), PROPERTIES);
        host
    }

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: format!("/project/{path}"), language: language.into(), version: 1, text: text.into() }
    }

    fn yaml() -> Document {
        doc(&format!("{RESOURCES}/application.yml"), "yaml", YAML)
    }

    fn at(text: &str, needle: &str) -> Position {
        position(text, text.find(needle).unwrap() + 1)
    }

    fn written(text: &str, r: Range) -> &str {
        &text[offset(text, r.start).unwrap()..offset(text, r.end).unwrap()]
    }

    #[test]
    fn reads_java_facts() {
        let client = java_facts(CLIENT);
        let ends: Vec<_> = client.clients.iter().map(|e| (e.verb.as_str(), e.path.as_str())).collect();
        assert_eq!(ends, [("GET", "/api/users/{id}"), ("POST", "/api/users"), ("DELETE", "/api/users/{id}")]);
        assert_eq!(client.unmapped.iter().map(|(r, custom)| (written(CLIENT, *r), *custom)).collect::<Vec<_>>(), [(
            "missing", false
        )]);
        let server = java_facts(CONTROLLER);
        let ends: Vec<_> = server.servers.iter().map(|e| (e.verb.as_str(), e.path.as_str())).collect();
        assert_eq!(ends, [("GET", "/api/users/{userId}"), ("POST", "/api/users"), ("GET", "/api/users")]);
        let stream = java_facts(FUNCTIONS);
        let fns: Vec<_> = stream.functions.iter().map(|f| (f.name.as_str(), f.kind.as_str())).collect();
        assert_eq!(fns, [("process", "Function"), ("audit", "Consumer"), ("ticks", "Supplier"), ("uppercase", "Function")]);
        assert_eq!(stream.sent, ["alerts-out-0"]);
        assert_eq!(stream.factories, ["Token"]);
    }

    const KOTLIN_CLIENT: &str = r#"package com.acme.client

@FeignClient(name = "users", path = "/api")
interface UserClient {
    @GetMapping(path = ["/users/{id}"])
    suspend fun find(@PathVariable id: Long): User

    @PostMapping("/users")
    fun save(@RequestBody u: User): User

    fun missing(): Int
}
"#;

    const KOTLIN_FUNCTIONS: &str = r#"package com.acme.stream

@Configuration
class StreamFunctions {
    @Bean
    fun process(): Function<Order, Invoice> = Function { null }

    @Bean("ticks")
    fun clock(): Supplier<Long> {
        return Supplier { 1L }
    }

    fun publish(bridge: StreamBridge) { bridge.send("alerts-out-0", "x") }
}
"#;

    #[test]
    fn kotlin_feign_client_is_indexed() {
        let host = setup();
        host.add_file("src/main/kotlin/com/acme/client/UserClient.kt", KOTLIN_CLIENT);
        let facts = facts_in("UserClient.kt", KOTLIN_CLIENT);
        let ends: Vec<_> = facts.clients.iter().map(|e| (e.verb.as_str(), e.path.as_str())).collect();
        assert_eq!(ends, [("GET", "/api/users/{id}"), ("POST", "/api/users")]);
        assert_eq!(facts.unmapped.len(), 1);
        let stream = facts_in("StreamFunctions.kt", KOTLIN_FUNCTIONS);
        let fns: Vec<_> = stream.functions.iter().map(|f| (f.name.as_str(), f.kind.as_str())).collect();
        assert_eq!(fns, [("process", "Function"), ("ticks", "Supplier")]);
        assert_eq!(stream.sent, ["alerts-out-0"]);

        let mut tools = CloudTools::default();
        let kt = doc("src/main/kotlin/com/acme/client/UserClient.kt", "kotlin", KOTLIN_CLIENT);
        let tips: Vec<_> = tools.line_markers(&kt).into_iter().map(|m| m.tooltip).collect();
        assert_eq!(tips, [
            "Calls UserController.get() (GET /api/users/{userId})",
            "Calls UserController.save() (POST /api/users)"
        ]);
        let problems = &host.diagnostics()["/project/src/main/kotlin/com/acme/client/UserClient.kt"];
        assert_eq!(problems.len(), 1);
        assert_eq!(java_facts(CLIENT).clients.len(), 3);
    }

    #[test]
    fn links_feign_clients_and_endpoints() {
        let host = setup();
        let mut tools = CloudTools::default();
        let client = doc("client/src/main/java/com/acme/client/UserClient.java", "java", CLIENT);
        let tips: Vec<_> = tools.line_markers(&client).into_iter().map(|m| (m.line, m.tooltip)).collect();
        assert_eq!(tips, [
            (5, "Calls UserController.get() (GET /api/users/{userId})".to_owned()),
            (8, "Calls UserController.save() (POST /api/users)".into()),
        ]);
        let found = tools.definition(&client, at(CLIENT, "find("));
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("UserController.java"));
        assert_eq!(found[0].range.start, position(CONTROLLER, CONTROLLER.find("get(").unwrap()));
        let problems = &host.diagnostics()["/project/client/src/main/java/com/acme/client/UserClient.java"];
        assert_eq!(problems.len(), 1);
        assert_eq!((problems[0].severity, problems[0].code.as_deref()), (Severity::Warning, Some("feign-method-without-mapping")));

        let server = doc("users/src/main/java/com/acme/web/UserController.java", "java", CONTROLLER);
        let tips: Vec<_> = tools.line_markers(&server).into_iter().map(|m| (m.line, m.tooltip)).collect();
        assert_eq!(tips, [
            (6, "Called by Feign client UserClient.find() (GET /api/users/{id})".to_owned()),
            (9, "Called by Feign client UserClient.create() (POST /api/users)".into()),
        ]);
    }

    #[test]
    fn links_function_beans_and_bindings() {
        let _host = setup();
        let mut tools = CloudTools::default();
        let java = doc("src/main/java/com/acme/stream/StreamFunctions.java", "java", FUNCTIONS);
        let markers = tools.line_markers(&java);
        let tips: Vec<_> = markers.iter().map(|m| (m.line, m.tooltip.as_str())).collect();
        assert_eq!(tips, [
            (5, "Stream bindings: process-in-0, uppercaseprocess-in-0, spring.cloud.function.definition"),
            (8, "Stream bindings: audit-in-0, spring.cloud.function.definition"),
            (11, "Stream bindings: ticks-out-0"),
            (17, "Stream bindings: uppercaseprocess-in-0, spring.cloud.function.definition"),
        ]);
        let target = markers[1].target.as_ref().unwrap();
        assert!(target.path.ends_with("application.properties"), "binding keys come first");

        let lines = |found: Vec<Location>| found.into_iter().map(|l| l.range.start.line).collect::<Vec<_>>();
        assert_eq!(lines(tools.definition(&yaml(), at(YAML, "process-in-0"))), [5]);
        assert_eq!(lines(tools.definition(&yaml(), at(YAML, "uppercaseprocess-in-0"))), [17, 5]);
        assert_eq!(lines(tools.definition(&yaml(), at(YAML, "audit;"))), [8]);
        assert_eq!(lines(tools.definition(&yaml(), at(YAML, "audit-in-0"))), [8]);
        assert!(tools.definition(&yaml(), at(YAML, "destination")).is_empty());
    }

    #[test]
    fn checks_bindings_definitions_and_routes() {
        let host = setup();
        let mut tools = CloudTools::default();
        tools.line_markers(&yaml());
        let problems: Vec<_> = host.diagnostics()[&yaml().path]
            .iter()
            .map(|d| (d.code.clone().unwrap(), written(YAML, d.range).to_owned(), d.severity))
            .collect();
        let expected = [
            ("unknown-binding-function", "missing-in-0", Severity::Warning),
            ("unknown-function", "ghost", Severity::Warning),
            ("unknown-gateway-predicate", "Methd", Severity::Warning),
            ("unknown-gateway-filter", "Custom", Severity::Warning),
        ];
        assert_eq!(problems, expected.map(|(c, w, s)| (c.to_owned(), w.to_owned(), s)));

        let props = doc(&format!("{RESOURCES}/application.properties"), "properties", PROPERTIES);
        tools.line_markers(&props);
        let problems = &host.diagnostics()[&props.path];
        assert_eq!(problems.len(), 1);
        assert_eq!((written(PROPERTIES, problems[0].range), problems[0].severity), ("Bogus", Severity::Info));
    }

    /// Completion labels at the `^` of `text` (not `|`: it composes functions).
    fn complete(tools: &mut CloudTools, name: &str, language: &str, text: &str) -> Vec<String> {
        let at = position(text, text.find('^').unwrap());
        let d = doc(&format!("{RESOURCES}/{name}"), language, &text.replacen('^', "", 1));
        let items = tools.completion(&d, at, None);
        if let Some(first) = items.first() {
            let r = first.replace.unwrap();
            assert_eq!(r.end, at);
        }
        items.into_iter().map(|c| c.label).collect()
    }

    #[test]
    fn completes_functions_bindings_and_route_names() {
        let _host = setup();
        let mut tools = CloudTools::default();
        let functions = ["audit", "process", "ticks", "uppercase"];
        let text = YAML.replace("definition: process;audit", "definition: process;au^dit");
        assert_eq!(complete(&mut tools, "application.yml", "yaml", &text), functions);
        let text = "spring.cloud.function.definition=process|up^\n";
        assert_eq!(complete(&mut tools, "application-dev.properties", "properties", text), functions);

        // A fresh index: the document above composed `process|up`.
        let mut tools = CloudTools::default();
        let bindings = [
            "audit-in-0",
            "process-in-0",
            "process-out-0",
            "ticks-out-0",
            "uppercase-in-0",
            "uppercase-out-0",
            "uppercaseprocess-in-0",
            "uppercaseprocess-out-0",
        ];
        let text = YAML.replace("        process-in-0:", "        pro^\n        process-in-0:");
        assert_eq!(complete(&mut tools, "application.yml", "yaml", &text), bindings);
        let text = "spring.cloud.stream.bindings.^\n";
        assert_eq!(complete(&mut tools, "application.properties", "properties", text), bindings);

        let text = YAML.replace("            - Methd=GET", "            - Me^");
        let labels = complete(&mut tools, "application.yml", "yaml", &text);
        assert_eq!(labels.len(), PREDICATES.len());
        assert!(labels.contains(&"Method=".to_owned()));
        let text = "spring.cloud.gateway.server.webflux.routes[0].filters[1]=Strip^\n";
        assert_eq!(complete(&mut tools, "application.properties", "properties", text).len(), FILTERS.len());
        assert!(complete(&mut tools, "application.yml", "yaml", &YAML.replace("Path=/users/**", "Path=/us^ers/**")).is_empty());
        assert!(complete(&mut tools, "messages.properties", "properties", "spring.cloud.stream.bindings.^").is_empty());
    }

    #[test]
    fn matches_route_lists_and_bindings() {
        assert_eq!(route_list("spring.cloud.gateway.routes[0].predicates[12]"), Some(("predicates", false)));
        assert_eq!(route_list("spring.cloud.gateway.server.webmvc.routes[3].filters[0]"), Some(("filters", true)));
        assert_eq!(route_list("spring.cloud.gateway.routes[0].filters[0].name"), None);
        assert_eq!(route_list("spring.cloud.gateway.routes[0].uri"), None);
        assert_eq!(function_of("process-in-0"), Some("process"));
        assert_eq!(function_of("my-fn-out-12"), Some("my-fn"));
        assert_eq!(function_of("input"), None);
        assert!(same_path("/api/users/{id}", "/api/users/{userId:\\d+}"));
        assert!(!same_path("/api/users", "/api/users/{id}"));
    }
}
