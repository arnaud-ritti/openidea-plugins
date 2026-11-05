//! What the running application's Actuator endpoints said, parsed into a
//! small model (Spring Boot 2.x and 3.x JSON shapes) that is kept in memory
//! and in storage.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use spring_text::config::canonical;

/// The endpoints read, in order; the first one tells whether the application is up.
pub const ENDPOINTS: [&str; 7] = ["beans", "env", "conditions", "health", "mappings", "configprops", "scheduledtasks"];
/// What Actuator shows for a value it hides.
pub const MASK: &str = "******";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub url: String,
    /// Seconds since the Unix epoch.
    pub fetched_at: u64,
    /// Endpoints that answered nothing usable, with why (`conditions (404)`).
    pub missing: Vec<String>,
    /// Too big for storage: only beans and environment were kept.
    pub trimmed: bool,
    pub beans: Vec<Bean>,
    pub profiles: Vec<String>,
    /// Property sources, highest precedence first.
    pub sources: Vec<Source>,
    pub conditions: Vec<Condition>,
    /// `""` is the application, `db`, `db/primary`… its components.
    pub health: Vec<(String, String)>,
    pub mappings: Vec<Mapping>,
    pub configprops: Vec<PropsBean>,
    pub scheduled: Vec<Scheduled>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bean {
    pub context: String,
    pub name: String,
    pub scope: String,
    /// Fully qualified, `$` nesting as `.`, without the CGLIB suffix.
    pub ty: String,
    pub deps: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub props: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// `com.acme.MailConfig`, `DataSourceAutoConfiguration#dataSource`…
    pub key: String,
    pub matched: bool,
    pub messages: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    /// The dispatcher servlet or handler.
    pub group: String,
    pub predicate: String,
    pub handler: String,
    /// The handler method's class and name, when it is one.
    pub method: Option<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PropsBean {
    pub prefix: String,
    pub ty: String,
    pub props: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scheduled {
    /// `cron`, `fixedDelay`, `fixedRate`, `custom`.
    pub kind: String,
    /// `com.acme.Jobs.cleanup`.
    pub target: String,
    pub detail: String,
}

fn entries(v: &Value) -> impl Iterator<Item = (&String, &Value)> {
    v.as_object().into_iter().flatten()
}

fn items(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn type_name(raw: &str) -> String {
    raw.split("$$").next().unwrap_or(raw).replace('$', ".")
}

/// `{a: {b: 1}, c: [x]}` → `a.b = 1`, `c[0] = x`.
fn flatten(prefix: &str, v: &Value, out: &mut Vec<(String, String)>) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                flatten(&key, v, out);
            }
        }
        Value::Array(list) => {
            for (i, v) in list.iter().enumerate() {
                flatten(&format!("{prefix}[{i}]"), v, out);
            }
        }
        other => out.push((prefix.to_owned(), text(other))),
    }
}

impl Snapshot {
    /// Reads one endpoint's JSON.
    pub fn read(&mut self, endpoint: &str, json: &Value) {
        let contexts = || entries(&json["contexts"]);
        match endpoint {
            "beans" => {
                for (context, c) in contexts() {
                    for (name, b) in entries(&c["beans"]) {
                        self.beans.push(Bean {
                            context: context.clone(),
                            name: name.clone(),
                            scope: text(&b["scope"]),
                            ty: type_name(&text(&b["type"])),
                            deps: items(&b["dependencies"]).map(text).collect(),
                        });
                    }
                }
            }
            "env" => {
                self.profiles = items(&json["activeProfiles"]).map(text).collect();
                for s in items(&json["propertySources"]) {
                    let props = entries(&s["properties"]).map(|(k, p)| (k.clone(), text(&p["value"]))).collect();
                    self.sources.push(Source { name: text(&s["name"]), props });
                }
            }
            "conditions" => {
                let messages = |list: &Value| -> Vec<String> {
                    items(list)
                        .map(|o| {
                            let m = text(&o["message"]);
                            if m.is_empty() { text(&o["condition"]) } else { m }
                        })
                        .collect()
                };
                for (_, c) in contexts() {
                    for (key, list) in entries(&c["positiveMatches"]) {
                        self.conditions.push(Condition { key: key.clone(), matched: true, messages: messages(list) });
                    }
                    for (key, o) in entries(&c["negativeMatches"]) {
                        self.conditions.push(Condition { key: key.clone(), matched: false, messages: messages(&o["notMatched"]) });
                    }
                }
            }
            "health" => self.read_health("", json),
            "mappings" => {
                for (_, c) in contexts() {
                    let m = &c["mappings"];
                    for (group, list) in entries(&m["dispatcherServlets"]).chain(entries(&m["dispatcherHandlers"])) {
                        for h in items(list) {
                            let method = &h["details"]["handlerMethod"];
                            self.mappings.push(Mapping {
                                group: group.clone(),
                                predicate: text(&h["predicate"]),
                                handler: text(&h["handler"]),
                                method: method["className"]
                                    .as_str()
                                    .map(|class| (type_name(class), text(&method["name"]))),
                            });
                        }
                    }
                }
            }
            "configprops" => {
                for (_, c) in contexts() {
                    for (key, b) in entries(&c["beans"]) {
                        let prefix = text(&b["prefix"]);
                        let ty = key.strip_prefix(&format!("{prefix}-")).unwrap_or(key);
                        let mut props = Vec::new();
                        flatten("", &b["properties"], &mut props);
                        self.configprops.push(PropsBean { prefix, ty: type_name(ty), props });
                    }
                }
            }
            "scheduledtasks" => {
                for (kind, list) in entries(json) {
                    for t in items(list) {
                        let detail = match (t.get("expression"), t.get("interval"), t.get("trigger")) {
                            (Some(e), _, _) => text(e),
                            (_, Some(i), _) => format!("every {} ms", text(i)),
                            (_, _, Some(t)) => text(t),
                            _ => String::new(),
                        };
                        self.scheduled.push(Scheduled { kind: kind.clone(), target: text(&t["runnable"]["target"]), detail });
                    }
                }
            }
            _ => {}
        }
    }

    /// Boot 2.2+ nests `components`; older versions put them under `details`.
    fn read_health(&mut self, path: &str, v: &Value) {
        let Some(status) = v["status"].as_str() else { return };
        self.health.push((path.to_owned(), status.to_owned()));
        let children = if v["components"].is_object() { &v["components"] } else { &v["details"] };
        for (name, c) in entries(children) {
            let child = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
            self.read_health(&child, c);
        }
    }

    /// The value `key` has at runtime and the property source it comes from,
    /// matched with relaxed binding (`SPRING_DATASOURCE_URL` is `spring.datasource.url`).
    pub fn effective(&self, key: &str) -> Option<(&str, &str)> {
        let relaxed = canonical(key);
        let env_form = relaxed.replace(['.', '[', ']'], "");
        self.sources.iter().find_map(|s| {
            let env = s.name.contains("systemEnvironment");
            s.props
                .iter()
                .find(|(k, _)| if env { canonical(k).replace(['.', '[', ']'], "") == env_form } else { canonical(k) == relaxed })
                .map(|(_, v)| (v.as_str(), s.name.as_str()))
        })
    }

    /// What is kept in storage when the whole snapshot is too big.
    pub fn trimmed(&self) -> Snapshot {
        Snapshot {
            url: self.url.clone(),
            fetched_at: self.fetched_at,
            missing: self.missing.clone(),
            trimmed: true,
            beans: self.beans.clone(),
            profiles: self.profiles.clone(),
            sources: self.sources.clone(),
            ..Snapshot::default()
        }
    }

    /// Whether the snapshot holds what `endpoint` said.
    pub fn has(&self, endpoint: &str) -> bool {
        let kept = !self.trimmed || matches!(endpoint, "beans" | "env");
        kept && !self.missing.iter().any(|m| m.split(' ').next() == Some(endpoint))
    }
}

/// Whether a property source is the configuration file named `file`
/// (`Config resource 'class path resource [application.yml]' via …` in Boot 3,
/// `applicationConfig: [classpath:/application.yml]` in Boot 2).
pub fn is_file_source(source: &str, file: &str) -> bool {
    source.contains(&format!("[{file}]")) || source.contains(&format!("/{file}]"))
}

/// Whether a property source is a configuration file at all.
pub fn is_config_source(source: &str) -> bool {
    source.starts_with("Config resource") || source.starts_with("applicationConfig")
}

/// A property source's name, short enough for a tooltip: a config file's
/// location (`application.yml`), else the name.
pub fn short_source(source: &str) -> &str {
    if is_config_source(source)
        && let Some(open) = source.find('[')
        && let Some(close) = source[open..].find(']')
    {
        return &source[open + 1..open + close];
    }
    source
}
