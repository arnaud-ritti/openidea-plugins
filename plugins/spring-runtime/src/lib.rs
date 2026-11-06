//! Spring Runtime Insight for OpenIDEA: reads a running Spring Boot
//! application's Actuator endpoints on localhost into a snapshot (on demand,
//! never from an editor provider), shows it in a tool window and marks the
//! beans, `@Value` keys and configuration keys it knows about in the editor.

mod snapshot;

pub use snapshot::*;

use openidea_plugin::providers::{Document, LineMarker, Providers};
use openidea_plugin::text::position;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, HttpRequest, Invocation, LogLevel, Plugin, Project, Severity, host, register_plugin,
};
use serde_json::Value;
use spring_text::config::{self, canonical};
use spring_text::index::{COMMAND_BUDGET, Indexes};
use spring_text::java;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

const VIEW: &str = "runtime";
const STORAGE_KEY: &str = "snapshot";
/// Storage holds 1 MiB per plugin.
const STORAGE_MAX: usize = 900 * 1024;
const DEFAULT_URL: &str = "http://localhost:8080/actuator";
const CONFIG_GLOBS: [&str; 6] = [
    "**/application*.properties",
    "**/application*.yml",
    "**/application*.yaml",
    "**/bootstrap*.properties",
    "**/bootstrap*.yml",
    "**/bootstrap*.yaml",
];

/// Where a Java type of the project is, for opening runtime items in source.
#[derive(Clone, Debug)]
struct JavaType {
    fqn: String,
    line: u32,
    column: u32,
    methods: Vec<(String, u32, u32)>,
}

fn index_java(_: &str, text: &str) -> Option<Vec<JavaType>> {
    let file = java::parse(text);
    let at = |offset| {
        let p = position(text, offset);
        (p.line, p.character)
    };
    let types: Vec<JavaType> = file
        .types
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let (line, column) = at(t.at);
            let methods = t.methods().map(|m| (m.name.clone(), at(m.at).0, at(m.at).1)).collect();
            JavaType { fqn: file.qualified(i), line, column, methods }
        })
        .collect();
    (!types.is_empty()).then_some(types)
}

/// A config file's keys (relaxed form) and where they are.
fn index_config(path: &str, text: &str) -> Option<Vec<(String, u32, u32)>> {
    config::is_config_file(path).then(|| {
        config::keys(path, text)
            .into_iter()
            .map(|k| {
                let p = position(text, k.at);
                (canonical(&k.key), p.line, p.character)
            })
            .collect()
    })
}

/// What activating a tool window item opens.
#[derive(Clone, Debug, PartialEq)]
enum Target {
    /// A class by qualified (or `Outer.Inner`) name, at a method if given.
    Type(String, Option<String>),
    /// A configuration key, preferably in the named file.
    Key(String, Option<String>),
    File(String),
}

/// Checks the Actuator URL: plain HTTP is only allowed to this machine, and
/// the plugin only asks for `net:localhost` and `net:127.0.0.1`.
pub fn check_url(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"));
    let host = rest.and_then(|r| r.split(['/', ':', '?', '#']).next()).unwrap_or_default().to_ascii_lowercase();
    if host == "localhost" || host == "127.0.0.1" {
        return Ok(url.to_owned());
    }
    Err(format!(
        "Spring Runtime only reads applications running on this machine, but the Actuator base URL is {url:?}. Set Settings › Plugins › Spring Runtime Insight › Actuator base URL to http://localhost:<port>/actuator."
    ))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn age(at: u64) -> String {
    match now().saturating_sub(at) {
        0..60 => "just now".into(),
        s @ 60..3600 => format!("{} min ago", s / 60),
        s => format!("{} h ago", s / 3600),
    }
}

fn simple(ty: &str) -> &str {
    ty.rsplit('.').next().unwrap_or(ty)
}

fn package(ty: &str) -> &str {
    ty.rsplit_once('.').map_or("", |(p, _)| p)
}

/// `com.acme.Jobs.cleanup` → (`com.acme.Jobs`, `cleanup`).
fn class_and_method(target: &str) -> Target {
    match target.rsplit_once('.') {
        Some((class, method)) => Target::Type(class.into(), Some(method.into())),
        None => Target::Type(target.into(), None),
    }
}

/// The keys of the `${key:default}` placeholders in a string.
fn placeholders(s: &str) -> Vec<&str> {
    s.match_indices("${")
        .filter_map(|(i, _)| {
            let rest = &s[i + 2..];
            let end = rest.find([':', '}'])?;
            Some(rest[..end].trim()).filter(|k| !k.is_empty())
        })
        .collect()
}

fn marker(text: &str, at: usize, tooltip: String) -> LineMarker {
    LineMarker { line: position(text, at).line, icon: None, tooltip, target: None }
}

/// The token is a secret the host substitutes into the header; the plugin never reads it.
pub fn actuator_request(url: &str, endpoint: &str, timeout: u32, auth: &str) -> HttpRequest {
    let request = HttpRequest { timeout_ms: Some(timeout), ..HttpRequest::get(&format!("{url}/{endpoint}")) };
    match auth {
        "bearer" => request.header("Authorization", "Bearer {{secret:actuator-token}}"),
        "basic" => request.header("Authorization", "Basic {{secret:actuator-token}}"),
        _ => request,
    }
}

/// Said in the view when an endpoint answered 401 or 403.
pub fn auth_hint(missing: &[String]) -> Option<&'static str> {
    missing
        .iter()
        .any(|m| m.ends_with("(401)") || m.ends_with("(403)"))
        .then_some("needs authentication: set actuatorAuth and the actuator-token secret")
}

fn fetch_snapshot() -> Result<Snapshot, Error> {
    let url = check_url(&host::setting_string("actuatorUrl").unwrap_or_else(|| DEFAULT_URL.into()))?;
    let timeout = host::setting_number("timeoutMs").unwrap_or(2000.0).clamp(100.0, 30_000.0) as u32;
    let auth = host::setting_string("actuatorAuth").unwrap_or_default();
    let task = host::begin_task("Reading Spring Boot Actuator endpoints", true);
    let mut snap = Snapshot { url: url.clone(), fetched_at: now(), ..Snapshot::default() };
    for (i, endpoint) in ENDPOINTS.iter().enumerate() {
        if task.is_cancelled() {
            return Err(Error::Cancelled);
        }
        task.report(i as u32, Some(ENDPOINTS.len() as u32), Some(endpoint));
        let request = actuator_request(&url, endpoint, timeout, &auth);
        let response = match host::fetch(&request) {
            Ok(r) => r,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(Error::Denied(cap)) => {
                return Err(format!("{cap} is not granted. Allow it in Settings › Plugins, or use http://localhost.").into());
            }
            // The application isn't up: don't wait for every endpoint to time out.
            Err(e) if i == 0 => {
                return Err(format!("Could not reach {url} ({e}). Is the application running with Spring Boot Actuator?").into());
            }
            Err(e) => {
                snap.missing.push(format!("{endpoint} ({e})"));
                continue;
            }
        };
        if response.status != 200 {
            snap.missing.push(format!("{endpoint} ({})", response.status));
            continue;
        }
        match serde_json::from_slice::<Value>(&response.body) {
            Ok(json) => snap.read(endpoint, &json),
            Err(_) => snap.missing.push(format!("{endpoint} (not JSON)")),
        }
    }
    if snap.missing.len() == ENDPOINTS.len() {
        return Err(format!(
            "No Actuator endpoint answered at {url}. Expose the endpoints with management.endpoints.web.exposure.include."
        )
        .into());
    }
    Ok(snap)
}

/// Keeps the snapshot across instance reclaim; only beans and environment when it is too big.
fn store(snap: &Snapshot) {
    let mut json = serde_json::to_string(snap).unwrap_or_default();
    if json.len() > STORAGE_MAX {
        json = serde_json::to_string(&snap.trimmed()).unwrap_or_default();
    }
    if json.len() > STORAGE_MAX {
        host::log(LogLevel::Warn, "runtime snapshot too big for storage; kept in memory only");
        let _ = host::storage::delete(STORAGE_KEY);
        return;
    }
    let _ = host::storage::set(STORAGE_KEY, &json);
}

/// Builds the tool window's tree; only expanded items get their children listed.
struct Tree<'a> {
    items: Vec<TreeItem>,
    expanded: &'a BTreeSet<String>,
    targets: &'a mut BTreeMap<String, Target>,
}

impl Tree<'_> {
    /// Adds an item; whether its children are to be listed.
    fn node(&mut self, key: &str, parent: Option<&str>, label: &str, detail: &str, children: bool) -> bool {
        let mut item = TreeItem::new(key, label);
        if let Some(p) = parent {
            item = item.parent(p);
        }
        if !detail.is_empty() {
            item = item.detail(detail);
        }
        let open = children && self.expanded.contains(key);
        if children {
            item = item.expandable(open);
        }
        self.items.push(item);
        open
    }

    /// [`Tree::node`] with an icon.
    fn group(&mut self, icon: &str, key: &str, parent: Option<&str>, label: &str, detail: &str, children: bool) -> bool {
        let open = self.node(key, parent, label, detail, children);
        if let Some(item) = self.items.last_mut() {
            item.icon = Some(icon.into());
        }
        open
    }

    fn leaf(&mut self, key: &str, parent: &str, label: &str, detail: &str, target: Option<Target>) {
        if let Some(t) = target {
            self.targets.insert(key.into(), t);
        }
        self.node(key, Some(parent), label, detail, false);
    }

    fn beans(&mut self, snap: &Snapshot) {
        if !snap.has("beans") || !self.group("spring-boot", "beans", None, "Beans", &snap.beans.len().to_string(), true) {
            return;
        }
        let contexts: BTreeSet<&str> = snap.beans.iter().map(|b| b.context.as_str()).collect();
        for context in contexts {
            let ck = format!("beans/{context}");
            let count = snap.beans.iter().filter(|b| b.context == context).count();
            if !self.node(&ck, Some("beans"), context, &count.to_string(), true) {
                continue;
            }
            for (i, b) in snap.beans.iter().enumerate().filter(|(_, b)| b.context == context) {
                let bk = format!("bean/{i}");
                self.targets.insert(bk.clone(), Target::Type(b.ty.clone(), None));
                let detail = format!("{} · {}", b.scope, simple(&b.ty));
                if !self.node(&bk, Some(&ck), &b.name, &detail, !b.deps.is_empty()) {
                    continue;
                }
                for (j, dep) in b.deps.iter().enumerate() {
                    let ty = snap.beans.iter().find(|d| d.context == context && d.name == *dep).map(|d| d.ty.as_str());
                    let target = ty.map(|t| Target::Type(t.into(), None));
                    self.leaf(&format!("{bk}/{j}"), &bk, dep, ty.map(simple).unwrap_or_default(), target);
                }
            }
        }
    }

    /// Small: listed whole, every component open.
    fn health(&mut self, snap: &Snapshot) {
        let Some((_, status)) = snap.health.first().filter(|_| snap.has("health")) else { return };
        let mut root = TreeItem::new("health", "Health").detail(status).icon(health_icon(status));
        if snap.health.len() > 1 {
            root = root.expandable(self.expanded.contains("health"));
        }
        let open = root.expanded;
        self.items.push(root);
        if !open {
            return;
        }
        for (path, status) in &snap.health[1..] {
            let parent = path.rsplit_once('/').map_or("health".to_owned(), |(p, _)| format!("health/{p}"));
            let mut item = TreeItem::new(&format!("health/{path}"), simple_path(path)).parent(&parent).detail(status).icon(health_icon(status));
            if snap.health.iter().any(|(p, _)| p.starts_with(&format!("{path}/"))) {
                item = item.expandable(true);
            }
            self.items.push(item);
        }
    }

    fn configuration(&mut self, snap: &Snapshot) {
        let profiles = match snap.profiles.as_slice() {
            [] => "default profile".to_owned(),
            p => format!("profiles: {}", p.join(", ")),
        };
        if !snap.has("env") || !self.group("settings", "env", None, "Configuration", &profiles, !snap.sources.is_empty()) {
            return;
        }
        let file_of = |s: &Source| is_config_source(&s.name).then(|| short_source(&s.name).rsplit(['/', ':']).next().unwrap_or("").to_owned());
        let files: Vec<&Source> = snap.sources.iter().filter(|s| is_config_source(&s.name)).collect();
        if !files.is_empty() && self.group("file-properties", "env/files", Some("env"), "Config files", &files.len().to_string(), true) {
            for (i, s) in files.iter().enumerate() {
                let detail = format!("{} properties", s.props.len());
                self.leaf(&format!("env/files/{i}"), "env/files", short_source(&s.name), &detail, file_of(s).map(Target::File));
            }
        }
        for (i, s) in snap.sources.iter().enumerate() {
            let sk = format!("env/{i}");
            if !self.node(&sk, Some("env"), short_source(&s.name), &format!("{} properties", s.props.len()), !s.props.is_empty()) {
                continue;
            }
            for (j, (key, value)) in s.props.iter().enumerate() {
                self.leaf(&format!("{sk}/{j}"), &sk, key, value, Some(Target::Key(key.clone(), file_of(s))));
            }
        }
    }

    fn conditions(&mut self, snap: &Snapshot) {
        if !snap.has("conditions") || !self.group("spring-config", "auto", None, "Auto-configuration", "", !snap.conditions.is_empty()) {
            return;
        }
        for (matched, key, label, icon) in [(true, "auto/+", "Positive matches", "success"), (false, "auto/-", "Negative matches", "error")] {
            let list: Vec<(usize, &Condition)> = snap.conditions.iter().enumerate().filter(|(_, c)| c.matched == matched).collect();
            if !self.group(icon, key, Some("auto"), label, &list.len().to_string(), !list.is_empty()) {
                continue;
            }
            for (i, c) in list {
                let ck = format!("{key}/{i}");
                let target = match c.key.split_once('#') {
                    Some((class, method)) => Target::Type(class.into(), Some(method.into())),
                    None => Target::Type(c.key.clone(), None),
                };
                self.targets.insert(ck.clone(), target);
                if self.node(&ck, Some(key), &c.key, "", !c.messages.is_empty()) {
                    for (j, m) in c.messages.iter().enumerate() {
                        self.leaf(&format!("{ck}/{j}"), &ck, m, "", None);
                    }
                }
            }
        }
    }

    fn mappings(&mut self, snap: &Snapshot) {
        if !snap.has("mappings") || !self.group("web", "mappings", None, "Mappings", &snap.mappings.len().to_string(), true) {
            return;
        }
        let groups: BTreeSet<&str> = snap.mappings.iter().map(|m| m.group.as_str()).collect();
        for group in groups {
            let gk = format!("mappings/{group}");
            let count = snap.mappings.iter().filter(|m| m.group == group).count();
            if !self.node(&gk, Some("mappings"), group, &count.to_string(), true) {
                continue;
            }
            for (i, m) in snap.mappings.iter().enumerate().filter(|(_, m)| m.group == group) {
                let target = m.method.clone().map(|(class, name)| Target::Type(class, Some(name))).or_else(|| {
                    let (class, rest) = m.handler.split_once('#')?;
                    let method = rest.split('(').next().unwrap_or(rest);
                    (!class.contains(' ')).then(|| Target::Type(class.into(), Some(method.into())))
                });
                self.leaf(&format!("mapping/{i}"), &gk, &m.predicate, &m.handler, target);
            }
        }
    }

    fn configprops(&mut self, snap: &Snapshot) {
        let count = snap.configprops.len().to_string();
        if !snap.has("configprops") || !self.group("property", "props", None, "Configuration properties", &count, true) {
            return;
        }
        for (i, p) in snap.configprops.iter().enumerate() {
            let pk = format!("props/{i}");
            self.targets.insert(pk.clone(), Target::Type(p.ty.clone(), None));
            if !self.node(&pk, Some("props"), &p.prefix, simple(&p.ty), !p.props.is_empty()) {
                continue;
            }
            for (j, (key, value)) in p.props.iter().enumerate() {
                let target = Target::Key(format!("{}.{key}", p.prefix), None);
                self.leaf(&format!("{pk}/{j}"), &pk, key, value, Some(target));
            }
        }
    }

    fn scheduled(&mut self, snap: &Snapshot) {
        let count = snap.scheduled.len().to_string();
        if !snap.has("scheduledtasks") || !self.group("clock", "sched", None, "Scheduled tasks", &count, !snap.scheduled.is_empty()) {
            return;
        }
        for (i, t) in snap.scheduled.iter().enumerate() {
            let detail = format!("{} {}", t.kind, t.detail);
            self.leaf(&format!("sched/{i}"), "sched", &t.target, &detail, Some(class_and_method(&t.target)));
        }
    }

    fn loggers(&mut self, snap: &Snapshot) {
        let count = snap.loggers.len().to_string();
        if !snap.has("loggers") || !self.group("settings", "log", None, "Loggers", &count, !snap.loggers.is_empty()) {
            return;
        }
        for (i, l) in snap.loggers.iter().enumerate() {
            let detail = format!("configured {}, effective {}", l.configured, l.effective);
            self.leaf(&format!("log/{i}"), "log", &l.name, &detail, None);
        }
    }

    fn caches(&mut self, snap: &Snapshot) {
        let count = snap.caches.len().to_string();
        if !snap.has("caches") || !self.group("database", "cache", None, "Caches", &count, !snap.caches.is_empty()) {
            return;
        }
        for (i, c) in snap.caches.iter().enumerate() {
            let label = format!("{} / {}", c.manager, c.name);
            self.leaf(&format!("cache/{i}"), "cache", &label, simple(&c.target), Some(Target::Type(c.target.clone(), None)));
        }
    }
}

fn health_icon(status: &str) -> &'static str {
    match status {
        "UP" => "success",
        "DOWN" | "OUT_OF_SERVICE" => "error",
        _ => "warning",
    }
}

fn simple_path(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub struct Runtime {
    snapshot: Option<Snapshot>,
    /// Storage was read (the snapshot may survive instance reclaim).
    loaded: bool,
    /// Why the last refresh failed.
    error: Option<String>,
    expanded: BTreeSet<String>,
    /// Tree item key → what activating it opens, as last rendered.
    targets: BTreeMap<String, Target>,
    java: Indexes<Vec<JavaType>>,
    configs: Indexes<Vec<(String, u32, u32)>>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            snapshot: None,
            loaded: false,
            error: None,
            expanded: BTreeSet::new(),
            targets: BTreeMap::new(),
            java: Indexes::new(&["**/*.java"], index_java),
            configs: Indexes::new(&CONFIG_GLOBS, index_config),
        }
    }
}

impl Runtime {
    fn snapshot(&mut self) -> Option<&Snapshot> {
        if !self.loaded {
            self.loaded = true;
            let stored = host::storage::get(STORAGE_KEY).ok().flatten();
            self.snapshot = stored.and_then(|s| serde_json::from_str(&s).ok());
        }
        self.snapshot.as_ref()
    }

    fn refresh(&mut self) -> Result<String, Error> {
        let result = fetch_snapshot();
        self.error = result.as_ref().err().filter(|e| **e != Error::Cancelled).map(Error::to_string);
        host::refresh_view(VIEW);
        let snap = result?;
        store(&snap);
        let mut summary = format!("Read {} beans and {} property sources from {}.", snap.beans.len(), snap.sources.len(), snap.url);
        if !snap.missing.is_empty() {
            summary.push_str(&format!(" Not available: {}.", snap.missing.join(", ")));
        }
        self.snapshot = Some(snap);
        self.loaded = true;
        Ok(summary)
    }

    fn clear(&mut self) {
        self.snapshot = None;
        self.loaded = true;
        self.error = None;
        let _ = host::storage::delete(STORAGE_KEY);
        host::refresh_view(VIEW);
    }

    fn java_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let text = doc.text.as_str();
        let Some(snap) = self.snapshot() else {
            let _ = host::publish_diagnostics(&doc.path, &[]);
            return Vec::new();
        };
        let file = java::parse(text);
        let mut markers = Vec::new();
        let mut problems = Vec::new();
        if snap.has("beans") {
            for b in java::bean_names(&file) {
                let class = file.types.iter().position(|t| t.at == b.at);
                if class.is_some_and(|i| file.types[i].kind == "@interface") {
                    continue;
                }
                let fqn = class.map(|i| file.qualified(i));
                let loaded = snap.beans.iter().find(|r| match &fqn {
                    Some(fqn) => r.ty == *fqn || r.name == b.name && simple(&r.ty) == b.ty,
                    None => r.name == b.name,
                });
                if let Some(r) = loaded {
                    let dependents = snap.beans.iter().filter(|d| d.deps.contains(&r.name)).count();
                    let mut tip = format!("Runtime bean `{}` ({})", r.name, r.scope);
                    if dependents > 0 {
                        tip.push_str(&format!(" · {dependents} dependents"));
                    }
                    markers.push(marker(text, b.at, tip));
                } else if let Some(fqn) = fqn
                    && snap.beans.iter().any(|r| package(&r.ty) == file.package)
                {
                    let short = fqn.strip_prefix(&format!("{}.", file.package)).unwrap_or(&fqn);
                    let why = snap.conditions.iter().find(|c| !c.matched && [fqn.as_str(), short, &b.name].contains(&c.key.as_str()));
                    let message = match why {
                        Some(c) => format!("`{}` is not loaded in the running application: {}.", b.ty, c.messages.join("; ")),
                        None => format!("`{}` is not loaded in the running application. A profile or a condition may exclude it.", b.ty),
                    };
                    let range = spring_text::range(text, b.at, b.at + b.len);
                    problems.push(Diagnostic::new(range, Severity::Info, message).code("not-loaded"));
                }
            }
        }
        for t in &file.types {
            let params = t.members.iter().flat_map(|m| &m.params).chain(&t.components);
            let annotations = t.members.iter().flat_map(|m| &m.annotations).chain(params.flat_map(|p| &p.annotations));
            for s in annotations.filter(|a| a.name == "Value").flat_map(|a| a.strings(&["value"])) {
                let lines: Vec<String> = placeholders(&s.value)
                    .into_iter()
                    .filter_map(|key| {
                        let (value, source) = snap.effective(key)?;
                        Some(format!("`{key}` = {value} (from {})", short_source(source)))
                    })
                    .collect();
                if !lines.is_empty() {
                    markers.push(marker(text, s.at, lines.join("\n")));
                }
            }
            let Some(prefix) = t.annotation("ConfigurationProperties").and_then(|a| a.strings(&["prefix", "value"]).first().map(|s| s.value.clone()))
            else {
                continue;
            };
            let bound = snap.configprops.iter().find(|p| canonical(&p.prefix) == canonical(&prefix));
            if let Some(b) = bound {
                markers.push(marker(text, t.at, format!("`{prefix}` bound at runtime ({} properties)", b.props.len())));
            }
            let fields = t.fields().filter(|f| !f.modifiers.iter().any(|m| m == "static")).map(|f| (&f.name, f.at));
            for (name, at) in fields.chain(t.components.iter().map(|c| (&c.name, c.at))) {
                let tip = match snap.effective(&format!("{prefix}.{name}")) {
                    Some((value, source)) => format!("Runtime: {value} from {}", short_source(source)),
                    None => match bound.and_then(|b| b.props.iter().find(|(k, _)| canonical(k) == canonical(name))) {
                        Some((_, value)) => format!("Runtime: {value} (no property set, so the field's default)"),
                        None => continue,
                    },
                };
                markers.push(marker(text, at, tip));
            }
        }
        let _ = host::publish_diagnostics(&doc.path, &problems);
        markers
    }

    fn config_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(snap) = self.snapshot() else { return Vec::new() };
        let file = doc.path.rsplit(['/', '\\']).next().unwrap_or(&doc.path);
        config::keys(&doc.path, &doc.text)
            .into_iter()
            .filter(|k| !k.value.is_empty() && !k.value.starts_with(['|', '>']))
            .filter_map(|k| {
                let (value, source) = snap.effective(&k.key)?;
                let tip = if is_file_source(source, file) && (value == k.value || value == MASK) {
                    format!("Active (from {})", short_source(source))
                } else {
                    format!("Runtime: {value} from {}", short_source(source))
                };
                Some(marker(&doc.text, k.at, tip))
            })
            .collect()
    }

    fn open(&mut self, target: &Target, project: &Project) -> Result<(), Error> {
        let found = match target {
            Target::Type(class, method) => {
                let nested = format!(".{class}");
                self.java.get(project, COMMAND_BUDGET).iter().find_map(|(path, types)| {
                    let t = types.iter().find(|t| t.fqn == *class || t.fqn.ends_with(&nested))?;
                    let m = method.as_ref().and_then(|m| t.methods.iter().find(|x| x.0 == *m));
                    Some(m.map_or((path.to_owned(), t.line, t.column), |m| (path.to_owned(), m.1, m.2)))
                })
            }
            Target::Key(key, file) => {
                let key = canonical(key);
                let index = self.configs.get(project, COMMAND_BUDGET);
                let hits: Vec<(&str, u32, u32)> = index
                    .iter()
                    .flat_map(|(path, keys)| keys.iter().filter(|k| k.0 == key).map(move |k| (path, k.1, k.2)))
                    .collect();
                let in_file = hits.iter().find(|h| file.as_ref().is_some_and(|f| h.0.ends_with(&format!("/{f}"))));
                in_file.or(hits.first()).map(|h| (h.0.to_owned(), h.1, h.2))
            }
            Target::File(name) => {
                let suffix = format!("/{name}");
                let index = self.configs.get(project, COMMAND_BUDGET);
                index.iter().find(|(path, _)| path.ends_with(&suffix)).map(|(path, _)| (path.to_owned(), 0, 0))
            }
        };
        match found {
            Some((path, line, column)) => host::open_location(&path, line, column),
            None => Ok(()),
        }
    }
}

impl Providers for Runtime {
    /// Reads only the in-memory (or stored) snapshot: never the network.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        match doc.language.as_str() {
            "java" => self.java_markers(doc),
            _ if config::is_config_file(&doc.path) => self.config_markers(doc),
            _ => Vec::new(),
        }
    }
}

impl Views for Runtime {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        self.snapshot();
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let Some(snap) = &self.snapshot else {
            let text = self.error.clone().unwrap_or_else(|| {
                let url = host::setting_string("actuatorUrl").unwrap_or_else(|| DEFAULT_URL.into());
                format!("No runtime data yet. Start the application with Spring Boot Actuator, then click Refresh to read {url}.")
            });
            return Ok(View::message(&text).button(refresh));
        };
        self.targets.clear();
        let mut tree = Tree { items: Vec::new(), expanded: &self.expanded, targets: &mut self.targets };
        tree.beans(snap);
        tree.health(snap);
        tree.configuration(snap);
        tree.conditions(snap);
        tree.mappings(snap);
        tree.configprops(snap);
        tree.scheduled(snap);
        tree.loggers(snap);
        tree.caches(snap);
        let mut status = format!("Read {} from {}", age(snap.fetched_at), snap.url);
        if !snap.missing.is_empty() {
            status.push_str(&format!(" · not available: {}", snap.missing.join(", ")));
        }
        if let Some(hint) = auth_hint(&snap.missing) {
            status.push_str(&format!(" · {hint}"));
        }
        if snap.trimmed {
            status.push_str(" · only beans and environment kept (too large for storage)");
        }
        if let Some(e) = &self.error {
            status = format!("Refresh failed: {e} · {status}");
        }
        Ok(View::new(Body::Tree(tree.items)).button(refresh).button(Button::new("clear", "Clear").icon("trash")).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            // A failure is shown in the view.
            ViewEvent::Button(b) if b == "refresh" => drop(self.refresh()),
            ViewEvent::Button(b) if b == "clear" => self.clear(),
            ViewEvent::Expand(key) => {
                self.expanded.insert(key);
                host::refresh_view(view);
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
                host::refresh_view(view);
            }
            ViewEvent::Activate(key) => {
                if let (Some(target), Some(project)) = (self.targets.get(&key).cloned(), ctx.project()) {
                    self.open(&target, &project)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Plugin for Runtime {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "refresh" => self.refresh().map(Some),
            "clear" => {
                self.clear();
                Ok(Some("Spring Runtime snapshot cleared.".into()))
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        if events.iter().any(|e| matches!(e, Event::Scheduled)) && host::setting_bool("autoRefresh") == Some(true) {
            let _ = self.refresh();
        }
    }
}

register_plugin!(Runtime, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::HttpResponse;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn marks_health_and_conditions_with_status_icons() {
        let host = MockHost::install();
        let health = [("", "UP"), ("db", "DOWN"), ("disk", "OUT_OF_SERVICE"), ("mail", "UNKNOWN")];
        let snap = Snapshot {
            health: health.iter().map(|(p, s)| (p.to_string(), s.to_string())).collect(),
            conditions: vec![Condition { key: "A".into(), matched: true, messages: vec![] }],
            sources: vec![Source { name: "Config resource 'class path resource [application.yml]'".into(), props: vec![] }],
            ..Snapshot::default()
        };
        let mut runtime = Runtime { snapshot: Some(snap), loaded: true, ..Runtime::default() };
        runtime.expanded.extend(["health", "env", "auto"].map(String::from));
        let view = runtime.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("tree expected") };
        let icons: Vec<_> = items.iter().map(|i| (i.key.as_str(), i.icon.as_deref().unwrap_or_default())).collect();
        assert_eq!(icons, [
            ("beans", "spring-boot"),
            ("health", "success"),
            ("health/db", "error"),
            ("health/disk", "error"),
            ("health/mail", "warning"),
            ("env", "settings"),
            ("env/files", "file-properties"),
            ("env/0", ""),
            ("auto", "spring-config"),
            ("auto/+", "success"),
            ("auto/-", "error"),
            ("mappings", "web"),
            ("props", "property"),
            ("sched", "clock"),
            ("log", "settings"),
            ("cache", "database"),
        ]);
        assert_eq!(view.toolbar[1].icon.as_deref(), Some("trash"));
    }

    #[test]
    fn names_the_setting_as_shown_in_settings() {
        let error = check_url("http://example.com/actuator").unwrap_err();
        assert!(error.contains("Settings › Plugins › Spring Runtime Insight › Actuator base URL"), "{error}");
    }

    fn authorization(host: &MockHost, auth: &str) -> Option<String> {
        host.set_setting("actuatorAuth", &format!("\"{auth}\""));
        host.set_secret("actuator-token", "s3cret");
        host.on_fetch(|_| Ok(HttpResponse { status: 404, headers: vec![], body: vec![] }));
        let _ = fetch_snapshot();
        let requests = host.requests();
        assert!(!requests.is_empty());
        requests[0].headers.iter().find(|(k, _)| k == "Authorization").map(|(_, v)| v.clone())
    }

    #[test]
    fn bearer_header_uses_secret_substitution() {
        let host = MockHost::install().with_capabilities(&["secrets", "net:localhost"]);
        assert_eq!(authorization(&host, "bearer").as_deref(), Some("Bearer s3cret"));
        let host = MockHost::install().with_capabilities(&["secrets", "net:localhost"]);
        assert_eq!(authorization(&host, "basic").as_deref(), Some("Basic s3cret"));
        let host = MockHost::install().with_capabilities(&["secrets", "net:localhost"]);
        assert_eq!(authorization(&host, "none"), None);
    }

    #[test]
    fn loggers_and_caches_endpoints_parse() {
        let mut snap = Snapshot::default();
        snap.read("loggers", &serde_json::json!({"loggers": {
            "ROOT": {"configuredLevel": "INFO", "effectiveLevel": "INFO"},
            "com.acme": {"configuredLevel": "DEBUG", "effectiveLevel": "DEBUG"},
            "com.acme.x": {"effectiveLevel": "DEBUG"}
        }}));
        let names: Vec<_> = snap.loggers.iter().map(|l| (l.name.as_str(), l.configured.as_str(), l.effective.as_str())).collect();
        assert_eq!(names, [("ROOT", "INFO", "INFO"), ("com.acme", "DEBUG", "DEBUG")]);
        snap.read("caches", &serde_json::json!({"cacheManagers": {"cm": {"caches": {
            "users": {"target": "java.util.concurrent.ConcurrentHashMap"}
        }}}}));
        assert_eq!(snap.caches, [Cache { manager: "cm".into(), name: "users".into(), target: "java.util.concurrent.ConcurrentHashMap".into() }]);
        assert!(snap.trimmed().loggers.is_empty());
    }

    #[test]
    fn authentication_failure_explains_itself() {
        let msg = auth_hint(&["beans (401)".to_string()]).unwrap();
        assert!(msg.contains("actuatorAuth") && msg.contains("actuator-token"));
        assert!(!msg.contains("s3cret"));
    }
}
