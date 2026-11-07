//! Spring Initializr Launcher for OpenIDEA: a tool window form whose buttons
//! open a Spring Initializr server's project download, or its web page, with
//! the chosen options. OpenIDEA has no new-project API: the browser downloads
//! the zip.

mod dependencies;
mod starters;

use openidea_plugin::views::{Body, Button, Field, FieldValue, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, Plugin, TextEdit, host, register_plugin};
use openidea_plugin::Range;
use openidea_plugin::text::position;
use std::collections::BTreeMap;
use ui_kit::sections::{SectionDef, Switcher};

const START: &str = "https://start.spring.io";
const STORAGE_KEY: &str = "form";
const PROJECT_SECTION: &str = "project";
const DEPENDENCIES_SECTION: &str = "dependencies";
const SECTIONS: &[SectionDef] = &[
    SectionDef { id: PROJECT_SECTION, label: "Project" },
    SectionDef { id: DEPENDENCIES_SECTION, label: "Dependencies" },
];

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

pub struct Launcher {
    /// Values the user changed from their defaults; read from storage on first use.
    changed: Option<BTreeMap<String, String>>,
    sections: Switcher,
    dependencies: dependencies::State,
}

impl Default for Launcher {
    fn default() -> Self {
        Self { changed: None, sections: Switcher::new("initializr.section", SECTIONS), dependencies: Default::default() }
    }
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

    /// `spring init` arguments from the form, or why they cannot be built. The
    /// terminal receives them as separate arguments (no shell), so a value
    /// stays one argument whatever it contains; coordinates are still restricted.
    pub fn cli_args(&mut self) -> Result<Vec<String>, String> {
        for (key, label) in [("groupId", "group"), ("artifactId", "artifact"), ("name", "name"), ("bootVersion", "Spring Boot version"), ("javaVersion", "Java version")] {
            let value = self.get(key);
            if value.starts_with('-') || !value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
                return Err(format!("The {label} `{value}` may only contain letters, digits, `.`, `_` and `-` for the Spring Boot CLI."));
            }
        }
        if self.get("artifactId").chars().all(|c| c == '.') {
            return Err("The artifact may not consist of dots only for the Spring Boot CLI.".into());
        }
        let package = self.get("packageName");
        if package.starts_with('-') || !package.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.')) {
            return Err(format!("The package name `{package}` may only contain letters, digits, `.` and `_` for the Spring Boot CLI."));
        }
        let kind = self.get("type");
        let build = if kind.starts_with("gradle") { "gradle" } else { "maven" };
        let mut args: Vec<String> = ["init".into(), format!("--build={build}"), format!("--type={kind}")].into();
        for (flag, key) in [
            ("language", "language"),
            ("boot-version", "bootVersion"),
            ("java-version", "javaVersion"),
            ("packaging", "packaging"),
            ("groupId", "groupId"),
            ("artifactId", "artifactId"),
            ("name", "name"),
            ("description", "description"),
            ("package-name", "packageName"),
        ] {
            let value = self.get(key);
            if !value.is_empty() {
                args.push(format!("--{flag}={value}"));
            }
        }
        let dependencies = self.get("dependencies").split([',', ' ']).filter(|d| !d.is_empty()).collect::<Vec<_>>().join(",");
        let bad_char = |c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ','));
        if dependencies.chars().any(bad_char) || dependencies.split(',').any(|d| d.starts_with('-')) {
            return Err(format!("The dependencies `{dependencies}` may only be comma-separated ids for the Spring Boot CLI."));
        }
        if !dependencies.is_empty() {
            args.push(format!("--dependencies={dependencies}"));
        }
        args.push("--extract".into());
        args.push(format!("./{}", self.get("artifactId")));
        Ok(args)
    }

    /// Opens `spring init` in a terminal; without `process:spring`, the browser download instead.
    fn generate_with_cli(&mut self) -> Result<(), Error> {
        let args = match self.cli_args() {
            Ok(args) => args,
            Err(message) => {
                host::notify(NotificationLevel::Warning, "Spring Initializr", &message);
                return Ok(());
            }
        };
        match host::open_terminal("Spring Initializr: spring init", "spring", &args) {
            Err(Error::Denied(_)) => {
                host::notify(
                    NotificationLevel::Warning,
                    "Spring Initializr",
                    "The Spring Boot CLI path needs the process:spring capability; downloading the project in the browser instead.",
                );
                host::open_url(&self.starter_url())
            }
            Err(error) => {
                host::notify(NotificationLevel::Warning, "Spring Initializr", &format!("Could not open the terminal: {error}"));
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    fn warn(message: &str) {
        host::notify(NotificationLevel::Warning, "Spring Initializr", message);
    }

    /// The server's catalogue, only for the default server (the manifest allows no other host).
    fn catalogue(&mut self, load: impl FnOnce() -> Result<Vec<PickItem>, Error>) -> Option<Vec<PickItem>> {
        if self.get("server").trim_end_matches('/') != START {
            Self::warn("The pick lists are only available for https://start.spring.io; type the value for another server.");
            return None;
        }

        match load() {
            Ok(items) => Some(items),
            Err(Error::Denied(_)) => {
                Self::warn("The pick lists need the net:start.spring.io capability.");
                None
            }
            Err(error) => {
                Self::warn(&format!("Could not load the catalogue: {error}"));
                None
            }
        }
    }

    fn pick_boot_version(&mut self) -> Result<(), Error> {
        let Some(items) = self.catalogue(starters::boot_version_items) else { return Ok(()) };
        if let Some(version) = host::pick("Spring Boot version", &items)? {
            self.set("bootVersion", &version);
        }
        Ok(())
    }

    /// Inserts the picked starters into the open pom.xml or build.gradle(.kts).
    fn add_starters(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("Open a pom.xml, build.gradle or build.gradle.kts to add starters to.")?;
        let path = editor.path();
        let doc = editor.document()?;
        if !starters::supports(&path) {
            return Err("Open a pom.xml, build.gradle or build.gradle.kts to add starters to.".into());
        }
        let Some(items) = self.catalogue(starters::dependency_items) else { return Ok(None) };
        let Some(ids) = host::pick_many("Add Spring Boot starters", &items)? else { return Ok(None) };
        if ids.is_empty() {
            return Ok(None);
        }

        let (found, skipped) = starters::coordinates(&ids, &self.get("bootVersion"))?;
        let (offset, text) = starters::insertion(&path, &doc.text, &found)?;
        let at = position(&doc.text, offset);
        editor.apply_edits(doc.version, &[TextEdit { range: Range::new(at, at), text }])?;
        if !skipped.is_empty() {
            Self::warn(&format!("Not added, they need a BOM or repository: {}.", skipped.join(", ")));
        }
        Ok(Some(format!("Added {} starters.", found.len())))
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
        let problem = self.problem();
        let generate = |button: Button| button.enabled(problem.is_none());
        let status = match &problem {
            Some(p) => p.clone(),
            None => format!("Generate Project downloads {}.zip in the browser.", self.get("artifactId")),
        };
        let mut view = if self.sections.active() == DEPENDENCIES_SECTION {
            let (view, tree_status) = self.dependencies_tree();
            let status = if problem.is_some() { status } else { tree_status };
            view.status(&status)
        } else {
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
            View::new(Body::Form(fields)).status(&status)
        };
        view = view
            .button(generate(Button::new("generate", "Generate Project").icon("export")))
            .button(generate(Button::new("cli", "Generate with Spring Boot CLI").icon("terminal")))
            .button(generate(Button::new("explore", "Explore in Browser").icon("external-link")));
        if self.sections.active() == PROJECT_SECTION {
            view = view.button(Button::new("pickBoot", "Pick Spring Boot Version").icon("list")).button(Button::new("reset", "Reset").icon("revert"));
        }
        let badges = [(DEPENDENCIES_SECTION, self.dependencies_badge())];

        Ok(self.sections.buttons(view, &badges))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if !self.sections.handle(&event) && !self.handle_dependencies(&event)? {
            match event {
                ViewEvent::Field(key, value) if FIELDS.iter().any(|f| f.0 == key) => self.set(&key, &value),
                ViewEvent::Button(b) if b == "reset" => self.reset(),
                ViewEvent::Button(b) if b == "pickBoot" => self.pick_boot_version()?,
                ViewEvent::Button(b) if self.problem().is_none() && b == "generate" => host::open_url(&self.starter_url())?,
                ViewEvent::Button(b) if self.problem().is_none() && b == "cli" => self.generate_with_cli()?,
                ViewEvent::Button(b) if self.problem().is_none() && b == "explore" => host::open_url(&self.explore_url())?,
                _ => {}
            }
        }
        host::refresh_view(view);
        Ok(())
    }
}

impl Plugin for Launcher {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "add-starters" => self.add_starters(ctx),
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
    use crate::dependencies::in_range;
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
        let icons: Vec<_> = view.toolbar.iter().filter(|b| !b.id.starts_with("section:")).map(|b| (b.label.as_str(), b.icon.as_deref().unwrap())).collect();
        assert_eq!(icons, [("Generate Project", "export"), ("Generate with Spring Boot CLI", "terminal"), ("Explore in Browser", "external-link"), ("Pick Spring Boot Version", "list"), ("Reset", "revert")]);
    }

    #[test]
    fn invalid_forms_show_the_problem_and_open_nothing() {
        let host = MockHost::install();
        let mut l = Launcher::default();
        field(&mut l, &host, "server", "ftp://start.spring.io");
        let view = l.render(VIEW, None, &host.invocation()).unwrap();
        assert!(view.status.unwrap().starts_with("The server URL must start with http:// or https://"));
        assert!(view.toolbar.iter().filter(|b| b.id != "reset" && !b.id.starts_with("pick") && !b.id.starts_with("section:")).all(|b| !b.enabled));
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

    #[test]
    fn spring_init_args_match_form_values() {
        let host = MockHost::install().with_capabilities(&["storage", "process:spring"]);
        let mut l = Launcher::default();
        field(&mut l, &host, "type", "Gradle - Kotlin");
        field(&mut l, &host, "language", "kotlin");
        field(&mut l, &host, "bootVersion", "3.5.16");
        field(&mut l, &host, "artifactId", "my-app");
        field(&mut l, &host, "description", "a b; rm -rf ~ 'x'");
        field(&mut l, &host, "dependencies", "web, data-jpa");
        click(&mut l, &host, "cli");
        let terminals = host.terminals();
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].program, "spring");
        assert_eq!(
            terminals[0].args,
            [
                "init", "--build=gradle", "--type=gradle-project-kotlin", "--language=kotlin", "--boot-version=3.5.16",
                "--java-version=17", "--packaging=jar", "--groupId=com.example", "--artifactId=my-app", "--name=my-app",
                "--description=a b; rm -rf ~ 'x'", "--package-name=com.example.myapp", "--dependencies=web,data-jpa",
                "--extract", "./my-app"
            ]
        );
        assert!(host.opened_urls().is_empty());
    }

    #[test]
    fn spring_init_refuses_unsafe_coordinates() {
        let host = MockHost::install().with_capabilities(&["storage", "process:spring"]);
        let mut l = Launcher::default();
        field(&mut l, &host, "artifactId", "a b; rm -rf ~");
        click(&mut l, &host, "cli");
        assert!(host.terminals().is_empty());
        assert_eq!(host.notifications().len(), 1);
        assert!(l.cli_args().unwrap_err().contains("artifact"));
    }

    #[test]
    fn spring_init_without_capability_falls_back_to_the_browser() {
        let host = MockHost::install().with_capabilities(&["storage"]);
        let mut l = Launcher::default();
        click(&mut l, &host, "cli");
        assert!(host.terminals().is_empty());
        assert!(host.opened_urls()[0].starts_with("https://start.spring.io/starter.zip?"));
        assert!(host.notifications()[0].body.contains("process:spring"));
    }

    #[test]
    fn spring_init_refuses_leading_dash_values() {
        for (key, value, named) in [
            ("artifactId", "--help", "artifact `--help`"),
            ("artifactId", "-x", "artifact `-x`"),
            ("groupId", "-g", "group `-g`"),
            ("dependencies", "web,-web", "dependencies `web,-web`"),
            ("packageName", "-p", "package name `-p`"),
        ] {
            let host = MockHost::install().with_capabilities(&["storage", "process:spring"]);
            let mut l = Launcher::default();
            field(&mut l, &host, key, value);
            click(&mut l, &host, "cli");
            assert!(host.terminals().is_empty(), "{key}={value}");
            let notifications = host.notifications();
            assert_eq!(notifications.len(), 1, "{key}={value}");
            assert!(notifications[0].body.contains(named), "{}", notifications[0].body);
        }
    }

    #[test]
    fn spring_init_refuses_dot_only_artifacts() {
        for value in [".", ".."] {
            let host = MockHost::install().with_capabilities(&["storage", "process:spring"]);
            let mut l = Launcher::default();
            field(&mut l, &host, "artifactId", value);
            click(&mut l, &host, "cli");
            assert!(host.terminals().is_empty(), "{value}");
            assert_eq!(host.notifications().len(), 1, "{value}");
        }
    }

    #[test]
    fn spring_init_accepts_inner_dashes() {
        let host = MockHost::install().with_capabilities(&["storage", "process:spring"]);
        let mut l = Launcher::default();
        field(&mut l, &host, "artifactId", "my-app");
        field(&mut l, &host, "groupId", "a-b");
        field(&mut l, &host, "bootVersion", "1.0-SNAPSHOT");
        field(&mut l, &host, "dependencies", "spring-web,data-jpa");
        click(&mut l, &host, "cli");
        assert_eq!(host.terminals().len(), 1);
        assert!(host.notifications().is_empty());
    }

    const NET: &[&str] = &["storage", "editor.read", "editor.write", "net:start.spring.io"];

    fn serve(host: &MockHost) {
        host.on_fetch(|req| {
            let body = if req.url.contains("/metadata/client") {
                r#"{"dependencies":{"values":[{"name":"Web","values":[{"id":"web","name":"Spring Web","description":"MVC"}]},
                    {"name":"SQL","values":[{"id":"data-jpa","name":"JPA"},{"id":"flyway","name":"Flyway"}]}]},
                    "bootVersion":{"values":[{"id":"3.5.16","name":"3.5.16"},{"id":"4.0.0","name":"4.0.0"}]}}"#
            } else {
                r#"{"dependencies":{"web":{"groupId":"org.springframework.boot","artifactId":"spring-boot-starter-web"},
                    "data-jpa":{"groupId":"org.springframework.boot","artifactId":"spring-boot-starter-data-jpa","scope":"runtime"},
                    "flyway":{"groupId":"org.flywaydb","artifactId":"flyway-core","bom":"x"}}}"#
            };
            Ok(openidea_plugin::HttpResponse { status: 200, body: body.as_bytes().to_vec(), ..Default::default() })
        });
    }

    #[test]
    fn picks_the_boot_version() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        let mut l = Launcher::default();
        host.answer_next_prompt(Some(&["4.0.0"]));
        click(&mut l, &host, "pickBoot");
        assert_eq!(l.get("bootVersion"), "4.0.0");
    }

    #[test]
    fn dismissed_boot_pick_changes_nothing() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        let mut l = Launcher::default();
        host.answer_next_prompt(None);
        click(&mut l, &host, "pickBoot");
        assert_eq!(l.get("bootVersion"), "");
    }

    #[test]
    fn picks_need_the_default_server_and_the_net_capability() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        let mut l = Launcher::default();
        field(&mut l, &host, "server", "http://localhost:8080");
        click(&mut l, &host, "pickBoot");
        assert_eq!(host.requests().len(), 0);
        assert_eq!(host.notifications().len(), 1);

        let host = MockHost::install().with_capabilities(&["storage"]);
        click(&mut Launcher::default(), &host, "pickBoot");
        assert!(host.notifications()[0].body.contains("net:start.spring.io"));
    }

    const POM: &str = "<project>\n  <dependencyManagement>\n    <dependencies>\n    </dependencies>\n  </dependencyManagement>\n  <dependencies>\n    <dependency>\n      <groupId>g</groupId>\n      <artifactId>spring-boot-starter-web</artifactId>\n    </dependency>\n  </dependencies>\n  <build/>\n</project>\n";

    fn add(host: &MockHost, path: &str, text: &str, answer: Option<&[&str]>) -> Result<Option<String>, Error> {
        host.open_editor(path, "xml", text, &[(0, 0, 0, 0)]);
        host.answer_next_prompt(answer);
        Launcher::default().run_command("add-starters", &host.invocation(), None)
    }

    #[test]
    fn adds_starters_to_a_pom() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        add(&host, "/project/pom.xml", POM, Some(&["web", "data-jpa", "flyway"])).unwrap();
        let text = host.editor_text();
        assert_eq!(text.matches("spring-boot-starter-web").count(), 1, "already present");
        assert!(text.contains("  </dependencyManagement>\n  <dependencies>"));
        assert!(text.contains("    </dependency>\n    <dependency>\n      <groupId>org.springframework.boot</groupId>\n      <artifactId>spring-boot-starter-data-jpa</artifactId>\n      <scope>runtime</scope>\n    </dependency>\n  </dependencies>\n  <build/>"));
        assert!(!text.contains("flyway-core"));
        assert_eq!(host.notifications().len(), 1);
    }

    #[test]
    fn adds_starters_to_gradle_files() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        add(&host, "/project/build.gradle.kts", "plugins {\n}\ndependencies {\n    implementation(\"x:y\")\n}\n", Some(&["web", "data-jpa"])).unwrap();
        assert_eq!(
            host.editor_text(),
            "plugins {\n}\ndependencies {\n    implementation(\"x:y\")\n    implementation \"org.springframework.boot:spring-boot-starter-web\"\n    runtimeOnly \"org.springframework.boot:spring-boot-starter-data-jpa\"\n}\n"
        );
    }

    #[test]
    fn dismissed_or_unsupported_starters_leave_the_file_alone() {
        let host = MockHost::install().with_capabilities(NET);
        serve(&host);
        assert_eq!(add(&host, "/project/pom.xml", POM, None).unwrap(), None);
        assert_eq!(add(&host, "/project/pom.xml", POM, Some(&[])).unwrap(), None);
        assert_eq!(host.editor_text(), POM);
        assert!(add(&host, "/project/readme.md", "x", Some(&["web"])).is_err());
        assert!(add(&host, "/project/pom.xml", "<project/>", Some(&["web"])).is_err());
    }

    fn serve_catalogue(host: &MockHost) {
        host.on_fetch(|_| {
            let body = r#"{"dependencies":{"values":[
                {"name":"Web","values":[{"id":"web","name":"Spring Web","description":"Build MVC apps"},
                                        {"id":"old","name":"Legacy","description":"Gone since 3.4","compatibilityRange":"[2.0.0,3.4.0)"}]},
                {"name":"SQL","values":[{"id":"data-jpa","name":"JPA","description":"Persist with Hibernate"},
                                        {"id":"new","name":"Newcomer","description":"Needs 3.6","compatibilityRange":"3.6.0"}]}]},
                "bootVersion":{"default":"3.5.16","values":[]}}"#;
            Ok(openidea_plugin::HttpResponse { status: 200, body: body.as_bytes().to_vec(), ..Default::default() })
        });
    }

    fn tree(l: &mut Launcher, host: &MockHost) -> (Vec<openidea_plugin::views::TreeItem>, View) {
        let view = l.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body.clone() else { panic!("tree expected") };
        (items, view)
    }

    fn open_dependencies(host: &MockHost) -> Launcher {
        let mut l = Launcher::default();
        click(&mut l, host, "section:dependencies");
        l
    }

    fn labels(items: &[openidea_plugin::views::TreeItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn activate(l: &mut Launcher, host: &MockHost, key: &str) {
        l.handle(VIEW, ViewEvent::Activate(key.into()), &host.invocation()).unwrap();
    }

    #[test]
    fn tree_has_selected_and_categories_collapsed() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        let (items, view) = tree(&mut l, &host);
        assert_eq!(labels(&items), ["Selected (0)", "Web", "Spring Web", "SQL", "JPA"]);
        assert!(!items[0].expandable, "no chosen id, nothing to expand");
        assert!(items[1].expandable && !items[1].expanded);
        assert_eq!(items[2].detail.as_deref(), Some("Build MVC apps"));
        assert_eq!(view.status.as_deref(), Some("0 selected. Double-click a dependency to add or remove it."));
        assert!(view.toolbar.iter().any(|b| b.id == "search") && !view.toolbar.iter().any(|b| b.id == "clearSearch"));
    }

    #[test]
    fn activating_toggles_the_id_in_the_field() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        activate(&mut l, &host, "dep:web");
        activate(&mut l, &host, "dep:data-jpa");
        assert_eq!(l.get("dependencies"), "web,data-jpa");
        let (items, view) = tree(&mut l, &host);
        assert_eq!(labels(&items)[..3], ["Selected (2)", "Spring Web", "JPA"]);
        assert!(items[0].expanded);
        assert_eq!(items.iter().find(|i| i.key == "dep:web").unwrap().icon.as_deref(), Some("check"));
        let badge = view.toolbar.iter().find(|b| b.id == "section:dependencies").unwrap();
        assert_eq!(badge.label, "\u{25cf} Dependencies (2)");

        activate(&mut l, &host, "dep:web");
        assert_eq!(l.get("dependencies"), "data-jpa");
        activate(&mut l, &host, "sel:data-jpa");
        assert_eq!(l.get("dependencies"), "");
    }

    #[test]
    fn selected_shows_unknown_ids_by_their_id() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        field(&mut l, &host, "dependencies", "web, mystery");
        let (items, _) = tree(&mut l, &host);
        assert_eq!(labels(&items)[..3], ["Selected (2)", "Spring Web", "mystery"]);
        assert_eq!(items[2].detail, None);
    }

    #[test]
    fn search_filters_and_clears() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        host.answer_next_prompt(Some(&["HIBERNATE"]));
        click(&mut l, &host, "search");
        let (items, view) = tree(&mut l, &host);
        assert_eq!(labels(&items), ["Selected (0)", "SQL", "JPA"]);
        assert!(items[1].expanded);
        assert_eq!(view.status.as_deref(), Some("1 matches for \"HIBERNATE\""));
        assert!(view.toolbar.iter().any(|b| b.id == "clearSearch"));

        click(&mut l, &host, "clearSearch");
        let (items, view) = tree(&mut l, &host);
        assert_eq!(items.len(), 5);
        assert!(!items[1].expanded);
        assert!(!view.toolbar.iter().any(|b| b.id == "clearSearch"));

        host.answer_next_prompt(None);
        click(&mut l, &host, "search");
        assert_eq!(tree(&mut l, &host).0.len(), 5);
    }

    #[test]
    fn incompatible_dependencies_are_hidden_and_chosen_ones_flagged() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        let (items, _) = tree(&mut l, &host);
        assert!(!labels(&items).contains(&"Legacy") && !labels(&items).contains(&"Newcomer"));

        field(&mut l, &host, "dependencies", "old");
        field(&mut l, &host, "bootVersion", "3.6.1");
        let (items, _) = tree(&mut l, &host);
        assert_eq!(items[1].label, "Legacy");
        assert_eq!(items[1].detail.as_deref(), Some("(not compatible with 3.6.1)"));
        assert!(!labels(&items).contains(&"Gone since 3.4"));
        assert!(labels(&items).contains(&"Newcomer"));
        assert_eq!(items.iter().filter(|i| i.label == "Legacy").count(), 1, "hidden from the category");
    }

    #[test]
    fn version_ranges() {
        assert!(in_range("3.4.0", "[3.3.0,3.5.0)"));
        assert!(!in_range("3.5.0", "[3.3.0,3.5.0)"));
        assert!(in_range("3.5.0", "[3.3.0,3.5.0]"));
        assert!(!in_range("3.1.9", "3.2.0") && in_range("3.2.0", "3.2.0") && in_range("4.0.1", "3.2.0"));
        assert!(!in_range("3.4.0-M1", "[3.4.0,4.0.0-M1)"));
        assert!(in_range("3.4.0-RC1", "[3.4.0-M1,4.0.0-M1)"));
        assert!(!in_range("4.0.0", "[3.4.0-M1,4.0.0-M1)"));
        assert!(in_range("3.4.0-SNAPSHOT", "[3.4.0-M1,4.0.0-M1)"));
        assert!(in_range("garbage", "[3.3.0,3.5.0)") && in_range("3.4.0", "[nonsense") && in_range("3.4.0", "[x,y)"));
    }

    #[test]
    fn unavailable_catalogue_shows_a_message_and_selected_still_works() {
        let host = MockHost::install().with_capabilities(&["storage"]);
        let mut l = open_dependencies(&host);
        activate(&mut l, &host, "sel:web");
        field(&mut l, &host, "dependencies", "web");
        let (items, _) = tree(&mut l, &host);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].label, "web");
        assert!(items[2].label.contains("net:start.spring.io"));
        activate(&mut l, &host, "sel:web");
        assert_eq!(l.get("dependencies"), "");

        field(&mut l, &host, "server", "http://localhost:8080");
        let (items, _) = tree(&mut l, &host);
        assert!(items[1].label.contains("only available for https://start.spring.io"));
    }

    #[test]
    fn catalogue_is_fetched_once_until_refreshed() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        tree(&mut l, &host);
        tree(&mut l, &host);
        assert_eq!(host.requests().len(), 1);
        click(&mut l, &host, "refreshCatalogue");
        tree(&mut l, &host);
        assert_eq!(host.requests().len(), 2);
    }

    #[test]
    fn clearing_all_asks_first() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = open_dependencies(&host);
        field(&mut l, &host, "dependencies", "web,data-jpa");
        host.answer_next_prompt(None);
        click(&mut l, &host, "clearAll");
        assert_eq!(l.get("dependencies"), "web,data-jpa");
        host.answer_next_prompt(Some(&[]));
        click(&mut l, &host, "clearAll");
        assert_eq!(l.get("dependencies"), "");
    }

    #[test]
    fn switcher_shows_the_count_and_keeps_generate_buttons() {
        let host = MockHost::install().with_capabilities(NET);
        serve_catalogue(&host);
        let mut l = Launcher::default();
        field(&mut l, &host, "dependencies", "web,data-jpa,flyway");
        let view = l.render(VIEW, None, &host.invocation()).unwrap();
        let ids: Vec<_> = view.toolbar.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids[..2], ["section:project", "section:dependencies"]);
        assert_eq!(view.toolbar[0].label, "\u{25cf} Project");
        assert_eq!(view.toolbar[1].label, "Dependencies (3)");

        click(&mut l, &host, "section:dependencies");
        let view = l.render(VIEW, None, &host.invocation()).unwrap();
        assert_eq!(view.toolbar[1].label, "\u{25cf} Dependencies (3)");
        for id in ["generate", "cli", "explore"] {
            assert!(view.toolbar.iter().any(|b| b.id == id), "{id}");
        }
        assert!(!view.toolbar.iter().any(|b| b.id == "reset"));
    }
}
