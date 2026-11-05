//! Flyway Migrations for OpenIDEA: Flyway's SQL migrations, read from file
//! names and project configuration (there is no database to ask).
//!
//! - Migration file names are checked against the configured naming scheme.

mod checks;
mod config;
mod naming;

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use openidea_plugin::host;
use openidea_plugin::{Diagnostic, Error, Event, Invocation, NotificationLevel, Plugin, Project, register_plugin};
use spring_text::config::is_config_file;
use spring_text::index::project_of;

use config::{Config, is_flyway_config, load, location_glob, migration_files};
use naming::{Kind, Name, next_version, parse, version_key};

const TITLE: &str = "Flyway";

#[derive(Default)]
pub struct Flyway {
    /// Placeholder names of the last config read (completion, Task 6).
    #[allow(dead_code)] // read by completion in a later task
    placeholders: Vec<String>,
    /// Paths this instance published problems for.
    flagged: BTreeSet<String>,
    /// When `clean` was asked for the first time (Task 5).
    #[allow(dead_code)] // read by the terminal commands in a later task
    clean_asked: Option<Instant>,
}

fn root_relative<'a>(path: &'a str, root: &str) -> &'a str {
    path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches('/'))
}

impl Flyway {
    fn scan(&mut self, project: &Project) -> (Config, Vec<String>, BTreeMap<String, Vec<Diagnostic>>) {
        let config = load(project);
        self.placeholders = config.placeholders.clone();
        let files = migration_files(project, &config);
        let root = project.root().unwrap_or_default();
        let results = checks::check(&root, &files, &config.naming);
        (config, files, results)
    }

    fn publish(&mut self, results: &BTreeMap<String, Vec<Diagnostic>>, everything: bool) {
        let paths: BTreeSet<&String> = results.keys().chain(self.flagged.iter()).collect();
        for path in paths {
            let diagnostics = results.get(path).map(Vec::as_slice).unwrap_or_default();
            if diagnostics.is_empty() && !self.flagged.contains(path) && !everything {
                continue;
            }

            let _ = host::publish_diagnostics(path, diagnostics);
        }

        self.flagged = results.iter().filter(|(_, list)| !list.is_empty()).map(|(path, _)| path.clone()).collect();
    }

    fn check_command(&mut self, project: &Project) {
        let (config, _, results) = self.scan(project);
        self.publish(&results, true);

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
        self.publish(&results, false);
    }
}

register_plugin!(Flyway);

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
}
