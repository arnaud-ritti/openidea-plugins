//! The structure of a Liquibase changelog (XML, YAML or formatted SQL): its
//! changeSets, includes and change types. Offsets are UTF-8 bytes.

use spring_text::config::{self, Key};
use spring_text::xml;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Xml,
    Yaml,
    Sql,
}

/// A changeSet; `at..end`: its `changeSet` tag name or key, or the whole `--changeset` line. Empty values are `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet {
    pub id: Option<String>,
    pub author: Option<String>,
    pub at: usize,
    pub end: usize,
}

/// An `include` (`dir: false`, `path` = `file`) or `includeAll` (`dir: true`); `at..end`: the value as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include {
    pub path: String,
    pub relative: bool,
    pub dir: bool,
    pub at: usize,
    pub end: usize,
}

/// A change type key directly under `changes` (YAML only); `at..end`: the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub name: String,
    pub at: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changelog {
    pub format: Format,
    /// Formatted SQL only: the first non-blank line is the header (`true` for XML and YAML).
    pub header: bool,
    pub change_sets: Vec<ChangeSet>,
    pub includes: Vec<Include>,
    pub changes: Vec<Change>,
}

/// Where an include points, as far as the host can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Found(String),
    Missing,
    Unknown,
}

fn empty(format: Format, header: bool) -> Changelog {
    Changelog { format, header, change_sets: Vec::new(), includes: Vec::new(), changes: Vec::new() }
}

/// `None` when `text` isn't a Liquibase changelog (format by `path`'s extension).
pub fn parse(path: &str, text: &str) -> Option<Changelog> {
    if path.ends_with(".xml") {
        return parse_xml(text);
    }
    if path.ends_with(".yaml") || path.ends_with(".yml") {
        return parse_yaml(text);
    }
    if path.ends_with(".sql") {
        return parse_sql(text);
    }
    None
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn parse_xml(text: &str) -> Option<Changelog> {
    let elements = xml::elements(text);
    if xml::root(&elements) != Some("databaseChangeLog") {
        return None;
    }
    let mut log = empty(Format::Xml, true);
    for e in &elements {
        match e.local() {
            "changeSet" => log.change_sets.push(ChangeSet {
                id: e.value("id").and_then(non_empty),
                author: e.value("author").and_then(non_empty),
                at: e.at,
                end: e.at + e.name.len(),
            }),
            name @ ("include" | "includeAll") => {
                let dir = name == "includeAll";
                let Some(attr) = e.attr(if dir { "path" } else { "file" }) else {
                    continue;
                };
                let relative = e.value("relativeToChangelogFile").is_some_and(|v| v.eq_ignore_ascii_case("true"));
                log.includes.push(Include { path: attr.value.clone(), relative, dir, at: attr.value_at, end: attr.value_end });
            }
            _ => {}
        }
    }
    Some(log)
}

/// The `n` and the rest of a `databaseChangeLog[n].rest` key.
fn entry(key: &Key) -> Option<(usize, &str)> {
    let rest = key.key.strip_prefix("databaseChangeLog[")?;
    let (n, rest) = rest.split_once("].")?;
    Some((n.parse().ok()?, rest))
}

fn parse_yaml(text: &str) -> Option<Changelog> {
    let keys = config::yaml(text);
    if !keys.iter().any(|k| k.key == "databaseChangeLog") {
        return None;
    }
    let mut log = empty(Format::Yaml, true);
    let mut set_of: Vec<(usize, usize)> = Vec::new();
    let mut include_of: Vec<(usize, bool)> = Vec::new();
    for key in &keys {
        let Some((n, rest)) = entry(key) else {
            continue;
        };
        match rest {
            "changeSet" => {
                set_of.push((n, log.change_sets.len()));
                log.change_sets.push(ChangeSet { id: None, author: None, at: key.at, end: key.end });
            }
            "include.file" | "includeAll.path" => {
                let dir = rest.starts_with("includeAll");
                include_of.push((n, dir));
                log.includes.push(Include { path: key.value.clone(), relative: false, dir, at: key.value_at, end: key.value_end });
            }
            _ => {}
        }
    }
    for key in &keys {
        let Some((n, rest)) = entry(key) else {
            continue;
        };
        let set = set_of.iter().find(|(m, _)| *m == n).map(|&(_, i)| i);
        match (rest, set) {
            ("changeSet.id", Some(i)) => log.change_sets[i].id = non_empty(&key.value),
            ("changeSet.author", Some(i)) => log.change_sets[i].author = non_empty(&key.value),
            ("include.relativeToChangelogFile" | "includeAll.relativeToChangelogFile", _) => {
                let dir = rest.starts_with("includeAll");
                if let Some(i) = include_of.iter().position(|&(m, d)| m == n && d == dir) {
                    log.includes[i].relative = key.value.eq_ignore_ascii_case("true");
                }
            }
            _ => {}
        }
        let Some((_, name)) = rest.strip_prefix("changeSet.changes[").and_then(|r| r.split_once("].")) else {
            continue;
        };
        if !name.is_empty() && !name.contains(['.', '[']) {
            log.changes.push(Change { name: name.to_owned(), at: key.at, end: key.end });
        }
    }
    Some(log)
}

fn strip_comment(line: &str) -> &str {
    line.trim_start().strip_prefix("--").unwrap_or("").trim()
}

fn parse_sql(text: &str) -> Option<Changelog> {
    let mut log = empty(Format::Sql, false);
    let mut first = true;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let body = strip_comment(line);
        if std::mem::take(&mut first) && body.eq_ignore_ascii_case("liquibase formatted sql") {
            log.header = true;
            continue;
        }
        let Some(keyword) = body.get(.."changeset".len()) else {
            continue;
        };
        let args = &body["changeset".len()..];
        if !keyword.eq_ignore_ascii_case("changeset") || !args.starts_with(char::is_whitespace) {
            continue;
        }
        let (author, id) = changeset_token(args.trim_start());
        let indent = line.len() - line.trim_start().len();
        log.change_sets.push(ChangeSet { id, author, at: start + indent, end: start + line.len() });
    }
    (log.header || !log.change_sets.is_empty()).then_some(log)
}

/// `(author, id)` of the first token of a `--changeset` line: `author:id`, `"Jane Doe":id` or just `author`.
fn changeset_token(args: &str) -> (Option<String>, Option<String>) {
    let (author, rest) = match args.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
        None => {
            let token = args.split_whitespace().next().unwrap_or("");
            match token.split_once(':') {
                Some((author, _)) => (author, &args[author.len()..]),
                None => (token, ""),
            }
        }
    };
    let id = rest.strip_prefix(':').and_then(|r| r.split_whitespace().next()).unwrap_or("");
    (non_empty(author), non_empty(id))
}

/// Absolute paths `include` may mean, most likely first; empty when it can't be known
/// (a `${property}`, or a path leaving `root`).
pub fn candidates(changelog: &str, root: &str, include: &Include) -> Vec<String> {
    let path = include.path.strip_prefix("classpath:").unwrap_or(&include.path).trim_start_matches('/');
    if path.contains("${") {
        return Vec::new();
    }
    let root = root.trim_end_matches('/');
    let mut bases = Vec::new();
    if include.relative {
        bases.push(changelog.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned());
    } else {
        for marker in ["/src/main/resources/", "/src/test/resources/"] {
            if let Some(i) = changelog.find(marker) {
                bases.push(changelog[..i + marker.len() - 1].to_owned());
                break;
            }
        }
        bases.push(root.to_owned());
    }
    let mut out = Vec::new();
    for base in bases {
        let Some(resolved) = normalise(&format!("{base}/{path}")) else {
            return Vec::new();
        };
        if resolved != root && !resolved.starts_with(&format!("{root}/")) {
            return Vec::new();
        }
        out.push(resolved);
    }
    out
}

fn normalise(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?>
<databaseChangeLog xmlns="http://www.liquibase.org/xml/ns/dbchangelog">
    <include file="db/changes/001.xml"/>
    <include file="002.xml" relativeToChangelogFile="true"/>
    <includeAll path="db/more/"/>
    <changeSet id="1" author="bob"><createTable tableName="t"/></changeSet>
    <changeSet id="2"/>
</databaseChangeLog>
"#;

    const YAML: &str = "databaseChangeLog:
  - include:
      file: classpath:db/changes/001.yaml
  - changeSet:
      id: \"1\"
      author: bob
      changes:
        - createTable:
            tableName: t
        - createTabel:
            tableName: u
  - changeSet:
      id: 1
      author: bob
";

    const SQL: &str = "--liquibase formatted sql\n\n--changeset bob:1\ncreate table t (id int);\n--rollback drop table t;\n\n-- changeset \"Jane Doe\":2 runOnChange:true\nselect 1;\n--changeset nobody\n";

    fn sets(log: &Changelog) -> Vec<(Option<&str>, Option<&str>)> {
        log.change_sets.iter().map(|c| (c.id.as_deref(), c.author.as_deref())).collect()
    }

    #[test]
    fn reads_xml_changelogs() {
        let log = parse("/p/db.xml", XML).unwrap();
        assert_eq!(log.format, Format::Xml);
        assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("2"), None)]);
        let includes: Vec<_> = log.includes.iter().map(|i| (i.path.as_str(), i.relative, i.dir)).collect();
        assert_eq!(includes, [("db/changes/001.xml", false, false), ("002.xml", true, false), ("db/more/", false, true)]);
        assert_eq!(&XML[log.includes[0].at..log.includes[0].end], "db/changes/001.xml");
        assert_eq!(&XML[log.change_sets[1].at..log.change_sets[1].end], "changeSet");
        assert!(log.changes.is_empty());
    }

    #[test]
    fn reads_yaml_changelogs() {
        let log = parse("/p/db.yaml", YAML).unwrap();
        assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("1"), Some("bob"))]);
        assert_eq!(log.includes[0].path, "classpath:db/changes/001.yaml");
        let names: Vec<&str> = log.changes.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["createTable", "createTabel"]);
        assert_eq!(&YAML[log.changes[1].at..log.changes[1].end], "createTabel");
        assert_eq!(&YAML[log.change_sets[0].at..log.change_sets[0].end], "changeSet");
    }

    #[test]
    fn reads_formatted_sql() {
        let log = parse("/p/db.sql", SQL).unwrap();
        assert!(log.header);
        assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("2"), Some("Jane Doe")), (None, Some("nobody"))]);
        assert_eq!(&SQL[log.change_sets[2].at..log.change_sets[2].end], "--changeset nobody");
        assert!(!parse("/p/a.sql", "--changeset bob:1\nselect 1;").unwrap().header);
    }

    #[test]
    fn ignores_other_files() {
        assert_eq!(parse("/p/pom.xml", "<project><modelVersion>4.0.0</modelVersion></project>"), None);
        assert_eq!(parse("/p/application.yml", "spring:\n  liquibase:\n    change-log: classpath:db/master.yaml\n"), None);
        assert_eq!(parse("/p/schema.sql", "create table t (id int);"), None);
        assert_eq!(parse("/p/a.json", "{\"databaseChangeLog\": []}"), None);
    }

    #[test]
    fn survives_every_prefix() {
        for (path, text) in [("/p/a.xml", XML), ("/p/a.yaml", YAML), ("/p/a.sql", SQL)] {
            for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
                parse(path, &text[..end]);
            }
        }
    }

    #[test]
    fn include_candidates() {
        let inc = |path: &str, relative: bool| Include { path: path.into(), relative, dir: false, at: 0, end: 0 };
        let log = "/project/src/main/resources/db/changelog/master.yaml";
        assert_eq!(candidates(log, "/project", &inc("changes/001.yaml", true)), ["/project/src/main/resources/db/changelog/changes/001.yaml"]);
        assert_eq!(candidates(log, "/project", &inc("../shared/x.yaml", true)), ["/project/src/main/resources/db/shared/x.yaml"]);
        assert_eq!(
            candidates(log, "/project", &inc("classpath:/db/changes/001.yaml", false)),
            ["/project/src/main/resources/db/changes/001.yaml", "/project/db/changes/001.yaml"]
        );
        assert_eq!(candidates("/project/db/master.xml", "/project", &inc("db/changes/001.xml", false)), ["/project/db/changes/001.xml"]);
        assert!(candidates(log, "/project", &inc("${base}/x.yaml", false)).is_empty());
        assert!(candidates(log, "/project", &inc("../../../../../../etc/passwd", true)).is_empty());
    }
}
