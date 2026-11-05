//! Spring Initializr Launcher for OpenIDEA: a tool window form whose buttons
//! open a Spring Initializr server's project download, or its web page, with
//! the chosen options. OpenIDEA has no new-project API: the browser downloads
//! the zip.

use openidea_plugin::views::{Body, Button, Field, FieldValue, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Plugin, host, register_plugin};
use std::collections::BTreeMap;

const START: &str = "https://start.spring.io";
const STORAGE_KEY: &str = "form";

/// Key (the server's parameter name), label, default and choices as (id, shown
/// name) (none: free text), in the order of the form and of the URL.
const FIELDS: [(&str, &str, &str, &[(&str, &str)]); 12] = [
    ("server", "Server URL", START, &[]),
    (
        "type",
        "Build",
        "maven-project",
        &[("maven-project", "Maven"), ("gradle-project", "Gradle - Groovy"), ("gradle-project-kotlin", "Gradle - Kotlin")],
    ),
    ("language", "Language", "java", &[("java", "Java"), ("kotlin", "Kotlin"), ("groovy", "Groovy")]),
    ("bootVersion", "Spring Boot version (blank for the server's default)", "", &[]),
    ("groupId", "Group", "com.example", &[]),
    ("artifactId", "Artifact", "demo", &[]),
    ("name", "Name", "", &[]),
    ("description", "Description", "Demo project for Spring Boot", &[]),
    ("packageName", "Package name", "", &[]),
    ("packaging", "Packaging", "jar", &[("jar", "Jar"), ("war", "War")]),
    ("javaVersion", "Java", "17", &[("17", "17"), ("21", "21"), ("25", "25")]),
    ("dependencies", "Dependencies (comma-separated ids, such as web,data-jpa)", "", &[]),
];

fn choices(key: &str) -> &'static [(&'static str, &'static str)] {
    FIELDS.iter().find(|f| f.0 == key).map_or(&[], |f| f.3)
}

/// The id of a choice the form shows by name (an id is kept as is).
fn choice_id<'a>(key: &str, value: &'a str) -> &'a str {
    choices(key).iter().find(|(_, name)| *name == value).map_or(value, |(id, _)| id)
}

/// `group.artifact` as a Java package: lower case, no `-`, segments that are
/// identifiers (`com.example`, `my-app` → `com.example.myapp`).
pub fn package_name(group: &str, artifact: &str) -> String {
    format!("{group}.{artifact}")
        .replace('-', "")
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(|s| s.trim_start_matches(|c: char| c.is_ascii_digit()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// Percent-encodes all but unreserved characters and the `,` of lists.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b',' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Default)]
pub struct Launcher {
    /// Values the user changed from their defaults; read from storage on first use.
    changed: Option<BTreeMap<String, String>>,
}

impl Launcher {
    fn changed(&mut self) -> &mut BTreeMap<String, String> {
        self.changed.get_or_insert_with(|| {
            let saved = host::storage::get(STORAGE_KEY).ok().flatten();
            saved.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
        })
    }

    /// What a field shows when the user didn't change it: name and package follow the artifact.
    fn initial(&mut self, key: &str) -> String {
        match key {
            "name" => self.get("artifactId"),
            "packageName" => package_name(&self.get("groupId"), &self.get("artifactId")),
            _ => FIELDS.iter().find(|f| f.0 == key).map(|f| f.2.to_owned()).unwrap_or_default(),
        }
    }

    pub fn get(&mut self, key: &str) -> String {
        match self.changed().get(key) {
            Some(v) => v.clone(),
            None => self.initial(key),
        }
    }

    fn set(&mut self, key: &str, value: &str) {
        let value = choice_id(key, value.trim());
        let derived = matches!(key, "name" | "packageName");
        if value == self.initial(key) || (derived && value.is_empty()) {
            self.changed().remove(key);
        } else {
            self.changed().insert(key.to_owned(), value.to_owned());
        }
        let json = serde_json::to_string(self.changed()).unwrap_or_default();
        // Without `storage` the form lasts as long as this instance.
        let _ = host::storage::set(STORAGE_KEY, &json);
    }

    fn reset(&mut self) {
        self.changed = Some(BTreeMap::new());
        let _ = host::storage::delete(STORAGE_KEY);
    }

    /// Why no project can be generated, if so.
    pub fn problem(&mut self) -> Option<String> {
        let server = self.get("server");
        let host = server.strip_prefix("https://").or_else(|| server.strip_prefix("http://"));
        if host.is_none_or(|h| h.trim_matches('/').is_empty()) {
            return Some(format!("The server URL must start with http:// or https:// (`{server}`)."));
        }
        if self.get("groupId").is_empty() {
            return Some("The group is required.".into());
        }
        if self.get("artifactId").is_empty() {
            return Some("The artifact is required.".into());
        }
        None
    }

    /// `name=value&…` of the non-blank fields.
    fn query(&mut self) -> String {
        let mut params = Vec::new();
        for (key, ..) in &FIELDS[1..] {
            let mut value = self.get(key);
            if *key == "dependencies" {
                value = value.split([',', ' ']).filter(|d| !d.is_empty()).collect::<Vec<_>>().join(",");
            }
            if !value.is_empty() {
                params.push(format!("{key}={}", encode(&value)));
            }
        }
        params.join("&")
    }

    /// The project zip.
    pub fn starter_url(&mut self) -> String {
        format!("{}/starter.zip?{}", self.get("server").trim_end_matches('/'), self.query())
    }

    /// The server's web page with the same options.
    pub fn explore_url(&mut self) -> String {
        format!("{}/#!{}", self.get("server").trim_end_matches('/'), self.query())
    }
}

impl Views for Launcher {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        let fields = FIELDS
            .iter()
            .map(|(key, label, _, choices)| {
                let value = self.get(key);
                let value = if choices.is_empty() {
                    FieldValue::Text(value)
                } else {
                    let name = choices.iter().find(|(id, _)| *id == value).map_or(value.as_str(), |(_, name)| name);
                    let name = name.to_owned();
                    FieldValue::Choice(choices.iter().map(|(_, name)| (*name).to_owned()).collect(), name)
                };
                Field::new(key, label, value)
            })
            .collect();
        let problem = self.problem();
        let status = match &problem {
            Some(p) => p.clone(),
            None => format!("Generate Project downloads {}.zip in the browser.", self.get("artifactId")),
        };
        Ok(View::new(Body::Form(fields))
            .button(Button::new("generate", "Generate Project").icon("export").enabled(problem.is_none()))
            .button(Button::new("explore", "Explore in Browser").icon("external-link").enabled(problem.is_none()))
            .button(Button::new("reset", "Reset").icon("revert"))
            .status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Field(key, value) if FIELDS.iter().any(|f| f.0 == key) => self.set(&key, &value),
            ViewEvent::Button(b) if b == "reset" => self.reset(),
            ViewEvent::Button(b) if self.problem().is_none() && b == "generate" => host::open_url(&self.starter_url())?,
            ViewEvent::Button(b) if self.problem().is_none() && b == "explore" => host::open_url(&self.explore_url())?,
            _ => {}
        }
        host::refresh_view(view);
        Ok(())
    }
}

impl Plugin for Launcher {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "open-start" => {
                host::open_url(START)?;
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

register_plugin!(Launcher, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const VIEW: &str = "initializr";

    fn field(l: &mut Launcher, host: &MockHost, key: &str, value: &str) {
        l.handle(VIEW, ViewEvent::Field(key.into(), value.into()), &host.invocation()).unwrap();
    }

    fn click(l: &mut Launcher, host: &MockHost, button: &str) {
        l.handle(VIEW, ViewEvent::Button(button.into()), &host.invocation()).unwrap();
    }

    #[test]
    fn builds_urls_from_the_defaults_and_changes() {
        let _host = MockHost::install();
        let mut l = Launcher::default();
        assert_eq!(
            l.starter_url(),
            "https://start.spring.io/starter.zip?type=maven-project&language=java&groupId=com.example&artifactId=demo\
             &name=demo&description=Demo%20project%20for%20Spring%20Boot&packageName=com.example.demo&packaging=jar&javaVersion=17"
        );
        l.set("server", "http://localhost:8080/");
        l.set("artifactId", "my-app");
        l.set("bootVersion", "3.5.16");
        l.set("dependencies", " web, data-jpa ,,security ");
        l.set("description", "Ünïcode & more");
        let url = l.explore_url();
        assert!(url.starts_with("http://localhost:8080/#!type=maven-project&language=java&bootVersion=3.5.16&"), "{url}");
        assert!(url.contains("&artifactId=my-app&name=my-app&description=%C3%9Cn%C3%AFcode%20%26%20more&packageName=com.example.myapp&"));
        assert!(url.ends_with("&dependencies=web,data-jpa,security"));
    }

    #[test]
    fn derives_package_names() {
        assert_eq!(package_name("com.example", "demo"), "com.example.demo");
        assert_eq!(package_name("Org.Acme", "my-app"), "org.acme.myapp");
        assert_eq!(package_name("io.1x", "2fast service"), "io.x.fast.service");
    }

    #[test]
    fn form_edits_persist_and_open_the_browser() {
        let host = MockHost::install();
        let mut l = Launcher::default();
        field(&mut l, &host, "groupId", "org.acme");
        field(&mut l, &host, "type", "Gradle - Groovy");
        field(&mut l, &host, "language", "kotlin");
        field(&mut l, &host, "packageName", "org.acme.custom");
        field(&mut l, &host, "unknown", "x");
        assert_eq!(host.refreshed_views().len(), 5);
        assert_eq!(host.storage()["form"], r#"{"groupId":"org.acme","language":"kotlin","packageName":"org.acme.custom","type":"gradle-project"}"#);

        let mut fresh = Launcher::default();
        let view = fresh.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Form(fields) = view.body else { panic!("form expected") };
        let value = |key: &str| fields.iter().find(|f| f.key == key).map(|f| f.value.clone()).unwrap();
        assert_eq!(value("groupId"), FieldValue::Text("org.acme".into()));
        assert_eq!(value("packageName"), FieldValue::Text("org.acme.custom".into()));
        let FieldValue::Choice(options, chosen) = value("type") else { panic!("choice expected") };
        assert_eq!((options, chosen.as_str()), (vec!["Maven".into(), "Gradle - Groovy".into(), "Gradle - Kotlin".into()], "Gradle - Groovy"));
        assert_eq!(view.status.as_deref(), Some("Generate Project downloads demo.zip in the browser."));

        click(&mut fresh, &host, "generate");
        click(&mut fresh, &host, "explore");
        let urls = host.opened_urls();
        assert!(urls[0].starts_with("https://start.spring.io/starter.zip?type=gradle-project&"));
        assert!(urls[0].contains("&groupId=org.acme&") && urls[0].contains("&packageName=org.acme.custom&"));
        assert!(urls[1].starts_with("https://start.spring.io/#!type=gradle-project&"));

        field(&mut fresh, &host, "packageName", "");
        assert_eq!(fresh.get("packageName"), "org.acme.demo", "blank: derived again");

        click(&mut fresh, &host, "reset");
        assert!(!host.storage().contains_key("form"));
        assert_eq!((fresh.get("groupId"), fresh.get("type")), ("com.example".into(), "maven-project".into()));
        let view = fresh.render(VIEW, None, &host.invocation()).unwrap();
        let icons: Vec<_> = view.toolbar.iter().map(|b| (b.label.as_str(), b.icon.as_deref().unwrap())).collect();
        assert_eq!(icons, [("Generate Project", "export"), ("Explore in Browser", "external-link"), ("Reset", "revert")]);
    }

    #[test]
    fn invalid_forms_show_the_problem_and_open_nothing() {
        let host = MockHost::install();
        let mut l = Launcher::default();
        field(&mut l, &host, "server", "ftp://start.spring.io");
        let view = l.render(VIEW, None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().starts_with("The server URL must start with http:// or https://"));
        assert!(view.toolbar.iter().filter(|b| b.id != "reset").all(|b| !b.enabled));
        click(&mut l, &host, "generate");
        assert!(host.opened_urls().is_empty());

        field(&mut l, &host, "server", "https://start.spring.io");
        field(&mut l, &host, "artifactId", " ");
        assert_eq!(l.problem().as_deref(), Some("The artifact is required."));
    }

    #[test]
    fn works_without_storage_and_opens_the_site() {
        let host = MockHost::install().with_capabilities(&[]);
        let mut l = Launcher::default();
        field(&mut l, &host, "artifactId", "shop");
        assert_eq!(l.get("name"), "shop");
        l.run_command("open-start", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_urls(), ["https://start.spring.io"]);
    }
}
