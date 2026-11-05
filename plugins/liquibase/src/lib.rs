//! Liquibase Changelogs: an OpenIDEA plugin.

#[allow(dead_code)]
mod catalog;
mod changelog;
mod checks;

use changelog::{Include, Target};
use openidea_plugin::providers::{Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Project, Range, host, register_plugin};
use spring_text::index::project_of;
use std::cell::OnceCell;

#[derive(Default)]
pub struct Liquibase;

impl Plugin for Liquibase {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

/// Whether reading a file worked; `None` when the host failed for a reason other than absence.
/// The host reports a missing file as `Invalid` (and the mock as `NotFound`); an unreadable one as `Failed`.
fn file_exists(read: Result<String, Error>) -> Option<bool> {
    match read {
        Ok(_) => Some(true),
        Err(Error::NotFound(_) | Error::Invalid(_)) => Some(false),
        Err(_) => None,
    }
}

/// Whether a directory glob matched a file; `None` on any error, since the host also
/// answers `Invalid` when the glob doesn't compile (a directory named with `[` or `{`).
fn has_files(found: Result<Vec<String>, Error>) -> Option<bool> {
    found.ok().map(|files| !files.is_empty())
}

/// Where `include`, written in `changelog` (inside `project`, rooted at `root`), points.
fn target(project: &Project, root: &str, changelog: &str, include: &Include) -> Target {
    if include.path.is_empty() {
        return Target::Unknown;
    }
    let candidates = changelog::candidates(changelog, root, include);
    if candidates.is_empty() {
        return Target::Unknown;
    }
    let mut unsure = false;
    // ponytail: reads each included file to test it exists (no exists API); cap or cache if master changelogs with hundreds of includes miss the 50 ms budget
    for candidate in candidates {
        let seen = if include.dir {
            let relative = candidate.strip_prefix(root.trim_end_matches('/')).unwrap_or(&candidate).trim_start_matches('/');
            let glob = if relative.is_empty() { "**/*".to_owned() } else { format!("{relative}/**/*") };
            has_files(project.find_files(&glob, 1))
        } else {
            file_exists(project.read_file(&candidate))
        };
        match seen {
            Some(true) => return Target::Found(candidate),
            Some(false) => {}
            None => unsure = true,
        }
    }
    if unsure { Target::Unknown } else { Target::Missing }
}

impl Providers for Liquibase {
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(log) = changelog::parse(&doc.path, &doc.text) else {
            return Vec::new();
        };
        let Some(caret) = offset(&doc.text, at) else {
            return Vec::new();
        };
        let Some(include) = log.includes.iter().find(|i| !i.dir && i.at <= caret && caret <= i.end) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root) = project.root() else {
            return Vec::new();
        };
        match target(&project, &root, &doc.path, include) {
            Target::Found(path) => vec![Location { path, range: Range::new(Position::new(0, 0), Position::new(0, 0)) }],
            _ => Vec::new(),
        }
    }

    /// Publishes the changelog's problems; there are no gutter markers.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let problems = match changelog::parse(&doc.path, &doc.text) {
            Some(log) => {
                let context = OnceCell::new();
                let resolve = |include: &Include| {
                    let context = context.get_or_init(|| {
                        let project = project_of(&doc.path)?;
                        let root = project.root().ok()?;
                        Some((project, root))
                    });
                    match context {
                        Some((project, root)) => target(project, root, &doc.path, include),
                        None => Target::Unknown,
                    }
                };
                checks::problems(&doc.path, &doc.text, &log, &resolve)
            }
            None => Vec::new(),
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

register_plugin!(Liquibase, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::Document;

    const MASTER: &str = "/project/src/main/resources/db/changelog/master.yaml";
    const MASTER_YAML: &str = "databaseChangeLog:
  - include:
      file: classpath:db/changes/001.yaml
  - include:
      file: db/changes/missing.yaml
  - include:
      file: ${changes}/x.yaml
  - includeAll:
      path: changes/
      relativeToChangelogFile: true
  - changeSet:
      id: \"1\"
      author: bob
      changes:
        - createTabel:
            tableName: t
  - changeSet:
      id: 1
      author: bob
  - changeSet:
      author: ann
";

    fn doc(path: &str, text: &str) -> Document {
        let language = match path.rsplit('.').next() {
            Some("xml") => "xml",
            Some("sql") => "sql",
            _ => "yaml",
        };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn checked(host: &MockHost, path: &str, text: &str) -> Vec<(String, String)> {
        Liquibase.line_markers(&doc(path, text));
        host.diagnostics()
            .get(path)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|d| (d.code.unwrap_or_default(), d.message))
            .collect()
    }

    fn host() -> MockHost {
        let host = MockHost::install();
        host.add_file("src/main/resources/db/changes/001.yaml", "databaseChangeLog: []\n");
        host.add_file("src/main/resources/db/changelog/changes/002.yaml", "databaseChangeLog: []\n");
        host
    }

    #[test]
    fn goes_to_included_changelogs() {
        let host = MockHost::install();
        host.add_file("db/changes/001.xml", "<databaseChangeLog/>");
        let text = "<databaseChangeLog>\n    <include file=\"changes/001.xml\" relativeToChangelogFile=\"true\"/>\n    <changeSet id=\"1\" author=\"a\"/>\n</databaseChangeLog>";
        let d = doc("/project/db/master.xml", text);
        let start = Position::new(0, 0);
        assert_eq!(Liquibase.definition(&d, Position::new(1, 25)), [Location { path: "/project/db/changes/001.xml".into(), range: Range::new(start, start) }]);
        assert!(Liquibase.definition(&d, Position::new(2, 8)).is_empty());
        let yaml = "databaseChangeLog:\n  - include:\n      file: db/changes/001.xml\n";
        assert_eq!(Liquibase.definition(&doc("/project/master.yaml", yaml), Position::new(2, 14)).len(), 1);
    }

    #[test]
    fn reports_yaml_changelog_problems() {
        let host = host();
        assert_eq!(
            checked(&host, MASTER, MASTER_YAML),
            [
                ("unresolved-include".into(), "Cannot find `db/changes/missing.yaml`.".into()),
                ("unknown-change-type".into(), "`createTabel` is not a built-in Liquibase change type.".into()),
                ("duplicate-changeset".into(), "Changeset `1` by `bob` is already defined in this file.".into()),
                ("incomplete-changeset".into(), "Changeset has no id.".into()),
            ]
        );
        let duplicate = host.diagnostics()[MASTER]
            .iter()
            .find(|d| d.code.as_deref() == Some("duplicate-changeset"))
            .cloned()
            .unwrap();
        let first = MASTER_YAML.find("changeSet").unwrap();
        assert_eq!(duplicate.related[0].location.range, spring_text::range(MASTER_YAML, first, first + "changeSet".len()));
    }

    #[test]
    fn reports_xml_changelog_problems() {
        let host = host();
        let text = r#"<databaseChangeLog>
    <include file="nope.xml" relativeToChangelogFile="true"/>
    <changeSet id="1" author="bob"/>
    <changeSet id="1" author="bob"/>
    <changeSet id="2"/>
    <changeSet/>
</databaseChangeLog>"#;
        assert_eq!(
            checked(&host, "/project/db/master.xml", text),
            [
                ("unresolved-include".into(), "Cannot find `nope.xml`.".into()),
                ("duplicate-changeset".into(), "Changeset `1` by `bob` is already defined in this file.".into()),
                ("incomplete-changeset".into(), "Changeset has no author.".into()),
                ("incomplete-changeset".into(), "Changeset has no id and no author.".into()),
            ]
        );
    }

    #[test]
    fn reports_formatted_sql_problems() {
        let host = host();
        let text = "--changeset bob:1\nselect 1;\n--changeset \"Jane Doe\":1\n--changeset nobody\n";
        assert_eq!(
            checked(&host, "/project/db/a.sql", text),
            [
                ("missing-header".into(), "Formatted SQL changelogs start with `--liquibase formatted sql`.".into()),
                ("incomplete-changeset".into(), "Changeset has no id. Write `--changeset author:id`.".into()),
            ]
        );
    }

    #[test]
    fn stays_silent_outside_changelogs() {
        let host = host();
        assert!(checked(&host, "/project/pom.xml", "<project><changeSet/></project>").is_empty());
        assert!(
            checked(&host, "/project/src/main/resources/application.yml", "spring:\n  liquibase:\n    change-log: x\n")
                .is_empty()
        );
        assert!(checked(&host, "/project/schema.sql", "create table t (id int);").is_empty());
    }

    #[test]
    fn half_typed_include_is_unknown() {
        let host = host();
        let include = Include { path: String::new(), relative: false, dir: false, at: 0, end: 0 };
        let project = project_of(MASTER).unwrap();
        assert_eq!(target(&project, "/project", MASTER, &include), Target::Unknown);
        let text = "databaseChangeLog:\n  - include:\n      file: \n";
        assert!(checked(&host, MASTER, text).is_empty());
    }

    #[test]
    fn classifies_read_errors_as_absent_or_unsure() {
        assert_eq!(file_exists(Ok(String::new())), Some(true));
        assert_eq!(file_exists(Err(Error::NotFound("x".into()))), Some(false));
        assert_eq!(file_exists(Err(Error::Invalid("x: no such file".into()))), Some(false));
        assert_eq!(file_exists(Err(Error::Failed("too large".into()))), None);
    }

    #[test]
    fn directory_glob_errors_are_unsure() {
        assert_eq!(has_files(Ok(vec!["a".into()])), Some(true));
        assert_eq!(has_files(Ok(Vec::new())), Some(false));
        assert_eq!(has_files(Err(Error::Invalid("\"a[/**/*\": unclosed".into()))), None);
        assert_eq!(has_files(Err(Error::Failed("io".into()))), None);
    }
}
