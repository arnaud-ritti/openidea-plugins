//! Flyway Migrations for OpenIDEA: Flyway's SQL migrations, read from file
//! names and project configuration (there is no database to ask).
//!
//! - Migration file names are checked against the configured naming scheme.

mod checks;
mod complete;
mod config;
mod naming;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use openidea_plugin::host;
use openidea_plugin::providers::{CompletionItem, Document, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Event, Invocation, NotificationLevel, Plugin, Position, Project, register_plugin};
use spring_text::config::is_config_file;
use spring_text::index::project_of;

use config::{Config, is_flyway_config, load, location_glob, migration_files, vendor_prefixes};
use naming::{Kind, Name, next_version, parse, version_key};

const TITLE: &str = "Flyway";

#[derive(Default)]
pub struct Flyway {
    /// Placeholder names by project root.
    placeholders: BTreeMap<String, Vec<String>>,
    /// Paths this instance published problems for.
    flagged: BTreeSet<String>,
    /// Project roots scanned by this instance; problems from before it started are unknown.
    scanned: BTreeSet<String>,
    /// When `clean` was asked for the first time.
    clean_asked: Option<Instant>,
}

fn root_relative<'a>(path: &'a str, root: &str) -> &'a str {
    path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches('/'))
}

impl Flyway {
    fn scan(&mut self, project: &Project) -> (Config, Vec<String>, BTreeMap<String, Vec<Diagnostic>>) {
        let config = load(project);
        let files = migration_files(project, &config);
        let root = project.root().unwrap_or_default();
        self.placeholders.insert(root.clone(), config.placeholders.clone());
        let vendors = vendor_prefixes(&config.locations, &root);
        let results = checks::check(&root, &files, &config.naming, &vendors);
        (config, files, results)
    }

    fn publish(&mut self, root: &str, results: &BTreeMap<String, Vec<Diagnostic>>, everything: bool) {
        let everything = self.scanned.insert(root.to_owned()) || everything;
        let prefix = format!("{}/", root.trim_end_matches('/'));
        let flagged_here: BTreeSet<String> = self.flagged.iter().filter(|path| path.starts_with(&prefix)).cloned().collect();
        let paths: BTreeSet<&String> = results.keys().chain(flagged_here.iter()).collect();
        for path in paths {
            let diagnostics = results.get(path).map(Vec::as_slice).unwrap_or_default();
            if diagnostics.is_empty() && !flagged_here.contains(path) && !everything {
                continue;
            }

            let _ = host::publish_diagnostics(path, diagnostics);
        }

        self.flagged.retain(|path| !path.starts_with(&prefix));
        self.flagged.extend(results.iter().filter(|(_, list)| !list.is_empty()).map(|(path, _)| path.clone()));
    }

    fn check_command(&mut self, project: &Project) {
        let (config, _, results) = self.scan(project);
        let root = project.root().unwrap_or_default();
        self.publish(&root, &results, true);

        if results.is_empty() {
            let locations = config.locations.join(", ");
            host::notify(
                NotificationLevel::Info,
                TITLE,
                &format!("No Flyway migrations found in {locations}. Set the locations in Settings › Plugins › Flyway Migrations if they are elsewhere."),
            );
            return;
        }

        let problems: usize = results.values().map(Vec::len).sum();
        host::notify(NotificationLevel::Info, TITLE, &format!("Checked {} migrations: {problems} problems.", results.len()));
    }

    fn copy_next_version(&mut self, project: &Project) -> Result<(), Error> {
        let (config, files, _) = self.scan(project);
        let naming = &config.naming;
        let root = project.root()?;

        let numeric_key = |version: &str| -> Vec<(usize, String)> {
            version_key(version).into_iter().map(|part| (part.len(), part)).collect()
        };
        let mut versions: Vec<String> = Vec::new();
        let mut highest: Option<(Vec<(usize, String)>, &str)> = None;
        for path in &files {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            let Name::Migration { kind: Kind::Versioned | Kind::Baseline, version, .. } = parse(file_name, naming) else {
                continue;
            };

            let key = numeric_key(&version);
            if highest.as_ref().is_none_or(|(best, _)| key > *best) {
                highest = Some((key, path));
            }
            versions.push(version);
        }

        let directory = match highest {
            Some((_, path)) => root_relative(path.rsplit_once('/').map_or("", |(parent, _)| parent), &root).to_owned(),
            None => config
                .locations
                .first()
                .and_then(|location| location_glob(location, &root))
                .map(|glob| glob.trim_end_matches("/**/*").trim_start_matches("**/").to_owned())
                .unwrap_or_default(),
        };

        let version_refs: Vec<&str> = versions.iter().map(String::as_str).collect();
        let next = format!("{}{}{}", naming.versioned, next_version(&version_refs), naming.separator);
        let suffix = naming.suffixes.first().map_or(".sql", String::as_str);

        match host::copy_to_clipboard(&next) {
            Ok(()) => host::notify(
                NotificationLevel::Info,
                TITLE,
                &format!("{next} copied to the clipboard: name the new migration {next}<description>{suffix} in {directory}."),
            ),
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                TITLE,
                &format!("Next migration: {next}<description>{suffix} in {directory}. Allow the clipboard for Flyway Migrations in Settings › Plugins to copy it."),
            ),
            Err(e) => return Err(e),
        }
        Ok(())
    }

    fn run_flyway(&mut self, goal: &str) -> Result<Option<String>, Error> {
        if goal == "clean" {
            let confirmed = self.clean_asked.take().is_some_and(|asked| asked.elapsed() < Duration::from_secs(30));
            if !confirmed {
                self.clean_asked = Some(Instant::now());
                host::notify(
                    NotificationLevel::Warning,
                    TITLE,
                    "Clean drops every object in the schemas Flyway manages. Run Flyway: Clean (Drops All Objects)… again within 30 seconds to confirm.",
                );
                return Ok(None);
            }
        }

        let maven = host::setting_string("runner").as_deref() == Some("mvn");
        let program = if maven { "mvn" } else { "flyway" };
        let mut args = vec![if maven { format!("flyway:{goal}") } else { goal.to_owned() }];
        // ponytail: no shell quoting; quote-aware split when someone needs spaces in a value
        args.extend(host::setting_string("arguments").unwrap_or_default().split_whitespace().map(str::to_owned));

        match host::open_terminal(&format!("Flyway {goal}"), program, &args) {
            Ok(()) => {}
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                TITLE,
                &format!("Allow running {program} for Flyway Migrations in Settings › Plugins."),
            ),
            Err(e) => return Err(e),
        }
        Ok(None)
    }
}

impl Plugin for Flyway {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "check" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                self.check_command(&project);
                Ok(None)
            }
            "copy-next-version" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                self.copy_next_version(&project)?;
                Ok(None)
            }
            "info" | "validate" | "migrate" | "repair" | "clean" => self.run_flyway(id),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        // ponytail: custom sqlMigrationSuffixes are checked by Flyway: Check Migrations only
        let qualifying = events.iter().find_map(|event| match event {
            Event::FileOpened(path) | Event::FileSaved(path)
                if path.ends_with(".sql") || is_flyway_config(path) || is_config_file(path) =>
            {
                Some(path)
            }
            _ => None,
        });
        let Some(path) = qualifying else { return };
        let Some(project) = project_of(path) else { return };

        let (_, _, results) = self.scan(&project);
        let root = project.root().unwrap_or_default();
        self.publish(&root, &results, false);
    }
}

impl Providers for Flyway {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };
        match doc.language.as_str() {
            "sql" => {
                let project = self
                    .placeholders
                    .iter()
                    .filter(|(root, _)| doc.path.starts_with(&format!("{}/", root.trim_end_matches('/'))))
                    .max_by_key(|(root, _)| root.len())
                    .map(|(_, names)| names.as_slice())
                    .unwrap_or_default();
                complete::placeholders(&doc.text, at, project)
            }
            "ini" if is_flyway_config(&doc.path) => complete::conf_keys(&doc.text, at),
            "toml" if is_flyway_config(&doc.path) => complete::toml_keys(&doc.text, at),
            _ => Vec::new(),
        }
    }
}

register_plugin!(Flyway, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::Event;
    use openidea_plugin::Severity;
    use openidea_plugin::mock::MockHost;

    const DIR: &str = "/project/src/main/resources/db/migration";

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "clipboard", "process:flyway", "process:mvn"]);
        host.add_file("src/main/resources/db/migration/V1__init.sql", "create table a (id int);");
        host.add_file("src/main/resources/db/migration/V1_0__again.sql", "");
        host.add_file("src/main/resources/db/migration/V2_bad.sql", "");
        host.add_file("src/main/resources/schema.sql", "");
        host
    }

    #[test]
    fn publishes_problems_when_a_migration_opens() {
        let host = host();
        Flyway::default().handle_events(&[Event::FileOpened(format!("{DIR}/V2_bad.sql"))], &host.invocation());

        let diagnostics = host.diagnostics();
        let paths: Vec<&str> = diagnostics.keys().map(String::as_str).collect();
        assert_eq!(paths, [format!("{DIR}/V1_0__again.sql"), format!("{DIR}/V1__init.sql"), format!("{DIR}/V2_bad.sql")]);
        assert_eq!(diagnostics[&format!("{DIR}/V1__init.sql")][0].message, "Version 1 is also used by src/main/resources/db/migration/V1_0__again.sql.");
        assert_eq!(diagnostics[&format!("{DIR}/V2_bad.sql")][0].severity, Severity::Warning);
        assert!(!diagnostics.contains_key("/project/src/main/resources/schema.sql"));
    }

    #[test]
    fn clears_problems_that_went_away() {
        let host = host();
        let mut flyway = Flyway::default();
        flyway.handle_events(&[Event::FileOpened(format!("{DIR}/V2_bad.sql"))], &host.invocation());
        host.set_setting("locations", "\"filesystem:none\"");
        flyway.handle_events(&[Event::FileSaved(format!("{DIR}/V2_bad.sql"))], &host.invocation());
        assert!(host.diagnostics().is_empty(), "{:?}", host.diagnostics());
    }

    fn problem(path: &str) -> BTreeMap<String, Vec<Diagnostic>> {
        let range = openidea_plugin::Range::new(Position::new(0, 0), Position::new(0, 0));
        BTreeMap::from([(path.to_owned(), vec![Diagnostic::new(range, Severity::Error, "x")])])
    }

    #[test]
    fn publishing_one_project_leaves_the_others_problems() {
        let host = MockHost::install();
        let mut flyway = Flyway::default();
        flyway.publish("/a", &problem("/a/V1__x.sql"), true);
        flyway.publish("/b", &problem("/b/V1__x.sql"), false);
        assert_eq!(host.diagnostics().len(), 2);
        assert!(!host.diagnostics()["/a/V1__x.sql"].is_empty());

        flyway.publish("/b", &BTreeMap::new(), false);
        assert!(!host.diagnostics().contains_key("/b/V1__x.sql"));
        assert!(!host.diagnostics()["/a/V1__x.sql"].is_empty());
    }

    #[test]
    fn the_first_scan_of_a_root_clears_stale_problems() {
        let host = MockHost::install();
        let clean = BTreeMap::from([("/a/V1__x.sql".to_owned(), Vec::new())]);
        let range = openidea_plugin::Range::new(Position::new(0, 0), Position::new(0, 0));
        host::publish_diagnostics("/a/V1__x.sql", &[Diagnostic::new(range, Severity::Error, "old")]).unwrap();
        let mut flyway = Flyway::default();
        flyway.publish("/a", &clean, false);
        assert!(!host.diagnostics().contains_key("/a/V1__x.sql"));
    }

    #[test]
    fn placeholders_do_not_leak_across_projects() {
        let host = MockHost::install();
        let mut flyway = Flyway::default();
        flyway.placeholders.insert("/a".into(), vec!["tenant".into()]);
        flyway.placeholders.insert("/b".into(), vec!["other".into()]);
        let sql = |path: &str| Document { path: path.into(), language: "sql".into(), version: 1, text: "${".into() };
        let labels = |items: Vec<CompletionItem>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        let in_a = labels(flyway.completion(&sql("/a/x.sql"), Position::new(0, 2), None));
        assert!(in_a.contains(&"tenant".to_owned()) && !in_a.contains(&"other".to_owned()));
        assert!(!labels(flyway.completion(&sql("/c/x.sql"), Position::new(0, 2), None)).contains(&"tenant".to_owned()));
        drop(host);
    }

    #[test]
    fn ignores_files_that_are_not_migrations_or_config() {
        let host = host();
        Flyway::default().handle_events(&[Event::FileOpened("/project/src/Main.java".into())], &host.invocation());
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn check_command_publishes_and_summarises() {
        let host = host();
        Flyway::default().run_command("check", &host.invocation(), None).unwrap();
        assert_eq!(host.diagnostics().len(), 3);
        assert_eq!(host.notifications()[0].body, "Checked 3 migrations: 3 problems.");

        let empty = MockHost::install();
        Flyway::default().run_command("check", &empty.invocation(), None).unwrap();
        assert_eq!(
            empty.notifications()[0].body,
            "No Flyway migrations found in classpath:db/migration. Set the locations in Settings › Plugins › Flyway Migrations if they are elsewhere."
        );
    }

    #[test]
    fn copies_the_next_version() {
        let host = MockHost::install();
        host.add_file("src/main/resources/db/migration/V1__a.sql", "");
        host.add_file("src/main/resources/db/migration/V2__b.sql", "");
        host.add_file("src/main/resources/db/migration/R__views.sql", "");
        Flyway::default().run_command("copy-next-version", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("V3__"));
        assert_eq!(
            host.notifications()[0].body,
            "V3__ copied to the clipboard: name the new migration V3__<description>.sql in src/main/resources/db/migration."
        );
    }

    #[test]
    fn shows_the_next_version_without_the_clipboard() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.add_file("src/main/resources/db/migration/V7__a.sql", "");
        assert_eq!(Flyway::default().run_command("copy-next-version", &host.invocation(), None).unwrap(), None);
        assert_eq!(
            host.notifications()[0].body,
            "Next migration: V8__<description>.sql in src/main/resources/db/migration. Allow the clipboard for Flyway Migrations in Settings › Plugins to copy it."
        );
    }

    #[test]
    fn runs_flyway_goals_in_a_terminal() {
        let host = host();
        host.set_setting("arguments", "\"-environment=dev\"");
        Flyway::default().run_command("migrate", &host.invocation(), None).unwrap();
        host.set_setting("runner", "\"mvn\"");
        Flyway::default().run_command("info", &host.invocation(), None).unwrap();

        let runs: Vec<(String, String, Vec<String>)> = host.terminals().into_iter().map(|t| (t.title, t.program, t.args)).collect();
        assert_eq!(runs, [
            ("Flyway migrate".to_owned(), "flyway".to_owned(), vec!["migrate".to_owned(), "-environment=dev".to_owned()]),
            ("Flyway info".to_owned(), "mvn".to_owned(), vec!["flyway:info".to_owned(), "-environment=dev".to_owned()]),
        ]);
    }

    #[test]
    fn clean_runs_only_when_confirmed() {
        let host = host();
        let mut flyway = Flyway::default();
        flyway.run_command("clean", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());
        assert_eq!(host.notifications()[0].level, NotificationLevel::Warning);
        assert_eq!(
            host.notifications()[0].body,
            "Clean drops every object in the schemas Flyway manages. Run Flyway: Clean (Drops All Objects)… again within 30 seconds to confirm."
        );

        flyway.run_command("clean", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["clean"]);

        flyway.run_command("clean", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals().len(), 1, "confirmation is used up");
    }

    #[test]
    fn asks_for_the_program_when_denied() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        assert_eq!(Flyway::default().run_command("validate", &host.invocation(), None).unwrap(), None);
        assert_eq!(host.notifications()[0].body, "Allow running flyway for Flyway Migrations in Settings › Plugins.");
    }

    #[test]
    fn completion_uses_project_placeholders_and_file_names() {
        let host = host();
        host.add_file("flyway.conf", "flyway.placeholders.tenant=acme\n");
        let mut flyway = Flyway::default();
        flyway.handle_events(&[Event::FileOpened(format!("{DIR}/V1__init.sql"))], &host.invocation());

        let sql = Document { path: format!("{DIR}/V3__x.sql"), language: "sql".into(), version: 1, text: "${te".into() };
        assert!(flyway.completion(&sql, Position::new(0, 4), None).iter().any(|i| i.label == "tenant"));

        let conf = |path: &str| Document { path: path.into(), language: "ini".into(), version: 1, text: "flyway.".into() };
        assert!(!flyway.completion(&conf("/project/flyway.conf"), Position::new(0, 7), None).is_empty());
        assert!(flyway.completion(&conf("/project/app.conf"), Position::new(0, 7), None).is_empty());
    }
}
