//! Liquibase Changelogs: an OpenIDEA plugin.

mod catalog;
mod changelog;
mod checks;
mod complete;

use changelog::{Changelog, Include, Target};
use openidea_plugin::providers::{CompletionItem, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use spring_text::tree::Expansion;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};
use openidea_plugin::text::offset;
use openidea_plugin::{Error, Invocation, Location, Plugin, Position, Project, Range, host, register_plugin};
use spring_text::index::project_of;
use std::cell::OnceCell;

const MAX_FILES: usize = 2000;
const DISCOVERY_BUDGET: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct Liquibase {
    expansion: Expansion,
    /// Changelog count that cuts discovery short; `None`: the default.
    max_files: Option<usize>,
    /// File and 0-based line by tree item key, from the last render of the changesets view.
    targets: BTreeMap<String, (String, u32)>,
}

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

/// Whether a directory exists, judged by reading it as a file: the host answers `Failed` ("not a file")
/// for an existing directory and `Invalid` for a missing one; `None` for any other error.
fn dir_exists(read: Result<String, Error>) -> Option<bool> {
    match read {
        Ok(_) | Err(Error::Failed(_)) => Some(true),
        Err(Error::NotFound(_) | Error::Invalid(_)) => Some(false),
        Err(_) => None,
    }
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
    // ponytail: reads or probes each include to test it exists (no exists API; a directory is probed by a failed read, so an empty one is not flagged); cap or cache if master changelogs with hundreds of includes miss the 50 ms budget
    for candidate in candidates {
        let read = project.read_file(&candidate);
        let seen = if include.dir { dir_exists(read) } else { file_exists(read) };
        match seen {
            Some(true) => return Target::Found(candidate),
            Some(false) => {}
            None => unsure = true,
        }
    }
    if unsure { Target::Unknown } else { Target::Missing }
}

const MAX_LISTED_FILES: usize = 1000;
const MAX_CHANGELOGS: usize = 200;
const MAX_CHANGELOG_BYTES: usize = 256 * 1024;
const MAX_CONFIG_FILES: usize = 20;

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Whether a changelog at `candidate` is worth reading for property declarations: it sits beside the open file or in one of
/// its ancestor directories, or under a `db`, `changelog`, `liquibase` or `migrations` path.
fn likely_changelog(candidate: &str, open: &str) -> bool {
    let dir = directory(candidate);
    let open_dir = directory(open);
    dir == open_dir
        || open_dir.starts_with(&format!("{dir}/"))
        || ["/db/", "/changelog", "/liquibase", "/migrations"].iter().any(|part| candidate.contains(part))
}

/// Every property any changelog of the project declares: Liquibase properties are global unless declared otherwise, so a
/// master or a sibling may declare what the open file uses. `None` when that cannot be told: a scan cap was hit, a file is
/// too big or unreadable, a JSON changelog is around, or some changelog has an `includeAll`, an include the host cannot
/// resolve or a `property` loaded from a file.
// ponytail: lists and reads every likely changelog on each check that finds an unknown placeholder; cache per project if it misses the 50 ms budget
fn project_properties(project: &Project, root: &str, path: &str, open: &Changelog) -> Option<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for glob in ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.sql", "**/*.json"] {
        let listed = project.find_files(glob, MAX_LISTED_FILES as u32).ok()?;
        if listed.len() >= MAX_LISTED_FILES {
            return None;
        }
        paths.extend(listed.into_iter().filter(|candidate| likely_changelog(candidate, path)));
    }
    paths.remove(path);
    if paths.len() > MAX_CHANGELOGS {
        return None;
    }
    let mut names = BTreeSet::new();
    let mut absorb = |at: &str, log: &Changelog| -> Option<()> {
        if log.opaque_properties {
            return None;
        }
        names.extend(log.properties.iter().cloned());
        for include in &log.includes {
            if include.dir || target(project, root, at, include) == Target::Unknown {
                return None;
            }
        }
        Some(())
    };
    absorb(path, open)?;
    for file in paths {
        let text = match project.read_file(&file) {
            Ok(text) => text,
            Err(Error::NotFound(_) | Error::Invalid(_)) => continue,
            Err(_) => return None,
        };
        if text.len() > MAX_CHANGELOG_BYTES {
            return None;
        }
        if file.ends_with(".json") {
            if text.contains("databaseChangeLog") {
                return None;
            }
            continue;
        }
        if let Some(log) = changelog::parse(&file, &text) {
            absorb(&file, &log)?;
        }
    }
    Some(names)
}

/// `foo` from `liquibase.parameters.foo`, `spring.liquibase.parameters.foo`, `parameter.foo`…
fn parameter_name(key: &str) -> Option<&str> {
    ["parameters.", "parameter."].iter().find_map(|marker| {
        let rest = key.strip_prefix(marker).or_else(|| key.split_once(&format!(".{marker}")).map(|(_, rest)| rest))?;
        (!rest.is_empty()).then_some(rest)
    })
}

/// Configuration that points at more configuration (or at profiles) this check cannot follow.
fn follows_other_config(key: &str) -> bool {
    let plain = key.replace(['-', '_'], "").to_lowercase();
    plain.ends_with("defaultsfile")
        || ["spring.config.import", "spring.config.location", "spring.config.additionallocation", "spring.config.activate"]
            .iter()
            .any(|prefix| plain.starts_with(&prefix.replace('_', "")))
}

/// The parameters the project's `liquibase.properties` and Spring configuration files (profile and module files included) set;
/// `None` when none of those files can be read or the configuration cannot be followed completely.
fn project_parameters(project: &Project, root: &str) -> Option<BTreeSet<String>> {
    let mut files: BTreeSet<String> = ["liquibase.properties", "src/main/resources/liquibase.properties"]
        .iter()
        .map(|file| format!("{root}/{file}"))
        .collect();
    for ext in ["properties", "yaml", "yml"] {
        files.insert(format!("{root}/src/main/resources/application.{ext}"));
    }
    for glob in ["**/liquibase.properties", "**/application*.properties", "**/application*.yaml", "**/application*.yml"] {
        let listed = project.find_files(glob, MAX_CONFIG_FILES as u32 + 1).ok()?;
        files.extend(listed.into_iter().filter(|file| {
            let name = file.rsplit('/').next().unwrap_or(file);
            name == "liquibase.properties" || spring_text::config::is_config_file(file)
        }));
    }
    if files.len() > MAX_CONFIG_FILES + 5 {
        return None;
    }
    let mut names = BTreeSet::new();
    let mut readable = false;
    for path in files {
        match project.read_file(&path) {
            Ok(text) => {
                readable = true;
                for key in spring_text::config::keys(&path, &text) {
                    if follows_other_config(&key.key) {
                        return None;
                    }
                    names.extend(parameter_name(&key.key).map(str::to_owned));
                }
            }
            Err(Error::NotFound(_) | Error::Invalid(_)) => {}
            Err(_) => return None,
        }
    }
    readable.then_some(names)
}

/// Builds the changesets tree: a node per changelog, holding its changesets and the changelogs it includes, in execution order.
struct Walk<'a> {
    project: &'a Project,
    root: &'a str,
    found: BTreeMap<String, Rc<(String, Changelog)>>,
    items: Vec<TreeItem>,
    targets: BTreeMap<String, (String, u32)>,
    visited: BTreeSet<String>,
    resolved: BTreeMap<(String, usize), (Vec<String>, Target)>,
    max_files: usize,
    partial: bool,
}

impl<'a> Walk<'a> {
    fn new(project: &'a Project, root: &'a str, max_files: usize) -> Self {
        Self {
            project,
            root,
            found: BTreeMap::new(),
            items: Vec::new(),
            targets: BTreeMap::new(),
            visited: BTreeSet::new(),
            resolved: BTreeMap::new(),
            max_files,
            partial: false,
        }
    }

    // ponytail: reads every xml/yaml/sql file the host lists, up to MAX_FILES per extension and DISCOVERY_BUDGET (checked between reads), on every render; the plugin has no `events` capability to invalidate a cache on save; index them (spring_text::index) if big projects hit the cap
    fn discover(&mut self) {
        let start = Instant::now();
        let mut paths = BTreeSet::new();
        for glob in ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.sql"] {
            let listed = self.project.find_files(glob, self.max_files as u32).unwrap_or_default();
            self.partial |= listed.len() >= self.max_files;
            paths.extend(listed);
        }
        for path in paths {
            if self.found.len() >= self.max_files || start.elapsed() > DISCOVERY_BUDGET {
                self.partial = true;
                break;
            }
            self.load(&path);
        }
    }

    fn load(&mut self, path: &str) -> Option<Rc<(String, Changelog)>> {
        if let Some(known) = self.found.get(path) {
            return Some(known.clone());
        }
        let text = self.project.read_file(path).ok()?;
        let log = changelog::parse(path, &text)?;
        let loaded = Rc::new((text, log));
        self.found.insert(path.to_owned(), loaded.clone());
        Some(loaded)
    }

    /// Changelogs nothing includes, then any left over (an include cycle has no root).
    fn walk_all(&mut self) {
        let paths: Vec<String> = self.found.keys().cloned().collect();
        let mut included = BTreeSet::new();
        for path in &paths {
            let log = self.found[path].clone();
            for index in 0..log.1.includes.len() {
                included.extend(self.included_files(path, index).0);
            }
        }
        for path in paths.iter().filter(|path| !included.contains(*path)) {
            self.file(path, None, &mut Vec::new());
        }
        for path in &paths {
            if !self.visited.contains(path) {
                self.file(path, None, &mut Vec::new());
            }
        }
    }

    /// The changelogs `include` (written in `path`) pulls in, and where it points.
    fn included_files(&mut self, path: &str, index: usize) -> (Vec<String>, Target) {
        let key = (path.to_owned(), index);
        if let Some(known) = self.resolved.get(&key) {
            return known.clone();
        }
        let include = &self.found[path].clone().1.includes[index];
        let found = target(self.project, self.root, path, include);
        let files = match &found {
            Target::Found(at) if include.dir => {
                let prefix = format!("{}/", at.trim_end_matches('/'));
                self.found.keys().filter(|file| file.starts_with(&prefix)).cloned().collect()
            }
            Target::Found(at) => vec![at.clone()],
            _ => Vec::new(),
        };
        self.resolved.insert(key, (files.clone(), found.clone()));
        (files, found)
    }

    fn file(&mut self, path: &str, parent: Option<&str>, stack: &mut Vec<String>) {
        let key = format!("f:{}", self.items.len());
        let label = path.strip_prefix(&format!("{}/", self.root.trim_end_matches('/'))).unwrap_or(path);
        let mut item = TreeItem::new(&key, label).icon("file");
        if let Some(parent) = parent {
            item = item.parent(parent);
        }
        self.targets.insert(key.clone(), (path.to_owned(), 0));
        if stack.iter().any(|open| open == path) {
            self.items.push(item.detail("include cycle").icon("warning"));
            return;
        }
        if self.visited.contains(path) {
            self.items.push(item.detail("already run"));
            return;
        }
        self.items.push(item);
        self.visited.insert(path.to_owned());
        let Some(loaded) = self.load(path) else {
            return;
        };
        let (text, log) = (&loaded.0, &loaded.1);

        let mut steps: Vec<(usize, bool, usize)> = Vec::new();
        steps.extend(log.change_sets.iter().enumerate().map(|(i, set)| (set.at, false, i)));
        steps.extend(log.includes.iter().enumerate().map(|(i, include)| (include.at, true, i)));
        steps.sort();

        stack.push(path.to_owned());
        for (_, is_include, index) in steps {
            if !is_include {
                let set = &log.change_sets[index];
                let label = format!("{} ({})", set.id.as_deref().unwrap_or("?"), set.author.as_deref().unwrap_or("?"));
                let child = format!("{key}/c{index}");
                let mut item = TreeItem::new(&child, &label).parent(&key).icon("database");
                if let Some(description) = changelog::describe(log, text, index) {
                    item = item.detail(&description);
                }
                self.targets.insert(child, (path.to_owned(), changelog::line_of(text, set.at)));
                self.items.push(item);
                continue;
            }
            let include = &log.includes[index];
            let (files, found) = self.included_files(path, index);
            for file in &files {
                self.file(file, Some(&key), stack);
            }
            let problem = match found {
                Target::Found(_) => continue,
                Target::Missing => "missing",
                Target::Unknown => "unresolved",
            };
            let child = format!("{key}/i{index}");
            self.targets.insert(child.clone(), (path.to_owned(), changelog::line_of(text, include.at)));
            self.items.push(TreeItem::new(&child, &include.path).parent(&key).detail(problem).icon("warning"));
        }
        stack.pop();
    }
}

impl Providers for Liquibase {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(caret) = offset(&doc.text, at) else {
            return Vec::new();
        };
        let changelog = changelog::parse(&doc.path, &doc.text).is_some();
        if doc.path.ends_with(".sql") {
            return complete::sql(&doc.text, caret, changelog);
        }
        if changelog && (doc.path.ends_with(".yaml") || doc.path.ends_with(".yml")) {
            return complete::yaml(&doc.text, caret);
        }
        Vec::new()
    }

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
                let context = || {
                    context.get_or_init(|| {
                        let project = project_of(&doc.path)?;
                        let root = project.root().ok()?;
                        Some((project, root))
                    })
                };
                let resolve = |include: &Include| match context() {
                    Some((project, root)) => target(project, root, &doc.path, include),
                    None => Target::Unknown,
                };
                let declared = OnceCell::new();
                let from_project = OnceCell::new();
                let defined = |name: &str| {
                    let Some((project, root)) = context().as_ref() else {
                        return true;
                    };
                    let Some(configured) = from_project.get_or_init(|| project_parameters(project, root)) else {
                        return true;
                    };
                    if configured.contains(name) {
                        return true;
                    }
                    match declared.get_or_init(|| project_properties(project, root, &doc.path, &log)) {
                        Some(declared) => declared.contains(name),
                        None => true,
                    }
                };
                let check = host::setting_bool("checkProperties").unwrap_or(true);
                let defined: Option<&dyn Fn(&str) -> bool> = check.then_some(&defined);
                checks::problems(&doc.path, &doc.text, &log, &resolve, defined)
            }
            None => Vec::new(),
        };
        let _ = host::publish_diagnostics(&doc.path, &problems);
        Vec::new()
    }
}

impl Views for Liquibase {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to see its Liquibase changesets."));
        };
        let Ok(root) = project.root() else {
            return Ok(View::message("Open a project to see its Liquibase changesets."));
        };
        let mut walk = Walk::new(&project, &root, self.max_files.unwrap_or(MAX_FILES));
        walk.discover();
        if walk.found.is_empty() {
            return Ok(View::message("No Liquibase changelogs found in this project.").button(refresh));
        }
        walk.walk_all();

        let partial = walk.partial;
        let mut items = std::mem::take(&mut walk.items);
        for item in items.iter_mut() {
            item.expanded = true;
        }
        self.expansion.apply(&mut items);
        self.targets = std::mem::take(&mut walk.targets);
        let sets = self.targets.keys().filter(|key| key.contains("/c")).count();
        let mut status = format!("{sets} changesets");
        if partial {
            status = format!("{status}; partial: showing the first {} changelogs (time budget)", walk.found.len());
        }
        Ok(View::new(Body::Tree(items)).button(refresh).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if self.expansion.handle(&event) {
            host::refresh_view(view);
            return Ok(());
        }
        match event {
            ViewEvent::Button(button) if button == "refresh" => host::refresh_view(view),
            ViewEvent::Activate(key) => {
                if let Some((path, line)) = self.targets.get(&key) {
                    host::open_location(path, *line, 0)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

register_plugin!(Liquibase, providers, views);

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
      errorIfMissingOrEmpty: false
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
        Liquibase::default().line_markers(&doc(path, text));
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

    fn tree_rows(host: &MockHost, liquibase: &mut Liquibase) -> Vec<(Option<String>, String, Option<String>)> {
        let Body::Tree(items) = liquibase.render("changesets", None, &host.invocation()).unwrap().body else { panic!("tree expected") };
        items.into_iter().map(|i| (i.parent, i.label, i.detail)).collect()
    }

    fn fixture() -> MockHost {
        let host = MockHost::install();
        host.add_file(
            "db/master.xml",
            "<databaseChangeLog>\n    <changeSet id=\"1\" author=\"ann\">\n        <comment>Create users</comment>\n        <createTable tableName=\"users\"/>\n    </changeSet>\n    <include file=\"changes/002.yaml\" relativeToChangelogFile=\"true\"/>\n    <include file=\"changes/003.sql\" relativeToChangelogFile=\"true\"/>\n    <changeSet id=\"4\" author=\"bob\">\n        <addColumn tableName=\"users\"/>\n    </changeSet>\n</databaseChangeLog>\n",
        );
        host.add_file(
            "db/changes/002.yaml",
            "databaseChangeLog:\n  - changeSet:\n      id: 2\n      author: cy\n      changes:\n        - createIndex:\n            tableName: users\n",
        );
        host.add_file(
            "db/changes/003.sql",
            "--liquibase formatted sql\n\n--changeset dee:3\n--comment: seed admin\ninsert into users values (1);\n",
        );
        host
    }

    #[test]
    fn tree_lists_changesets_across_includes_in_order() {
        let host = fixture();
        let mut liquibase = Liquibase::default();
        let tree = tree_rows(&host, &mut liquibase);
        assert_eq!(tree[0], (None, "db/master.xml".to_owned(), None));
        let rows: Vec<_> = tree[1..].iter().map(|(p, l, d)| (p.is_some(), l.as_str(), d.as_deref())).collect();
        assert_eq!(
            rows,
            [
                (true, "1 (ann)", Some("Create users")),
                (true, "db/changes/002.yaml", None),
                (true, "2 (cy)", Some("createIndex")),
                (true, "db/changes/003.sql", None),
                (true, "3 (dee)", Some("seed admin")),
                (true, "4 (bob)", Some("addColumn")),
            ]
        );

        let Body::Tree(items) = liquibase.render("changesets", None, &host.invocation()).unwrap().body else { panic!() };
        for i in &items {
            liquibase.handle("changesets", ViewEvent::Activate(i.key.clone()), &host.invocation()).unwrap();
        }
        let opened: Vec<_> = host.opened_locations().into_iter().map(|(p, l, _)| (p, l)).collect();
        assert_eq!(
            opened,
            [
                ("/project/db/master.xml".to_owned(), 0),
                ("/project/db/master.xml".to_owned(), 1),
                ("/project/db/changes/002.yaml".to_owned(), 0),
                ("/project/db/changes/002.yaml".to_owned(), 1),
                ("/project/db/changes/003.sql".to_owned(), 0),
                ("/project/db/changes/003.sql".to_owned(), 2),
                ("/project/db/master.xml".to_owned(), 7),
            ]
        );
    }

    #[test]
    fn tree_survives_cycles_and_missing_includes_and_empty_projects() {
        let host = MockHost::install();
        let mut liquibase = Liquibase::default();
        let Body::Message(text) = liquibase.render("changesets", None, &host.invocation()).unwrap().body else { panic!("message expected") };
        assert_eq!(text, "No Liquibase changelogs found in this project.");

        host.add_file("db/a.xml", "<databaseChangeLog>\n<include file=\"b.xml\" relativeToChangelogFile=\"true\"/>\n<include file=\"gone.xml\" relativeToChangelogFile=\"true\"/>\n</databaseChangeLog>");
        host.add_file("db/b.xml", "<databaseChangeLog>\n<changeSet id=\"1\" author=\"x\"/>\n<include file=\"a.xml\" relativeToChangelogFile=\"true\"/>\n</databaseChangeLog>");
        let rows: Vec<_> = tree_rows(&host, &mut liquibase).into_iter().map(|(_, label, detail)| (label, detail)).collect();
        assert_eq!(
            rows,
            [
                ("db/a.xml".to_owned(), None),
                ("db/b.xml".to_owned(), None),
                ("1 (x)".to_owned(), None),
                ("db/a.xml".to_owned(), Some("include cycle".to_owned())),
                ("gone.xml".to_owned(), Some("missing".to_owned())),
            ]
        );
    }

    #[test]
    fn every_view_activates_the_plugin() {
        let manifest = include_str!("../plugin.toml");
        let activation = manifest.lines().find(|line| line.starts_with("activation")).unwrap();
        let mut views = 0;
        let mut lines = manifest.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "[[contributes.views]]" {
                continue;
            }
            let id = lines.next().unwrap().trim().strip_prefix("id = ").unwrap().trim_matches('"');
            assert!(activation.contains(&format!("\"on-view:{id}\"")), "no on-view:{id}");
            views += 1;
        }
        assert!(views > 0);
    }

    #[test]
    fn partial_discovery_is_flagged() {
        let host = fixture();
        let mut liquibase = Liquibase::default();
        liquibase.max_files = Some(2);
        let status = liquibase.render("changesets", None, &host.invocation()).unwrap().status.unwrap();
        assert!(status.contains("partial: showing the first 2 changelogs"), "{status}");

        liquibase.max_files = None;
        let status = liquibase.render("changesets", None, &host.invocation()).unwrap().status.unwrap();
        assert!(!status.contains("partial"), "{status}");
    }

    #[test]
    fn a_changelog_reached_twice_is_already_run() {
        let host = MockHost::install();
        let inc = |f: &str| format!("<include file=\"{f}\" relativeToChangelogFile=\"true\"/>");
        host.add_file("db/m.xml", &format!("<databaseChangeLog>{}{}</databaseChangeLog>", inc("a.xml"), inc("b.xml")));
        host.add_file("db/a.xml", &format!("<databaseChangeLog>{}</databaseChangeLog>", inc("c.xml")));
        host.add_file("db/b.xml", &format!("<databaseChangeLog>{}</databaseChangeLog>", inc("c.xml")));
        host.add_file("db/c.xml", "<databaseChangeLog><changeSet id=\"1\" author=\"x\"/></databaseChangeLog>");
        let rows: Vec<_> = tree_rows(&host, &mut Liquibase::default()).into_iter().map(|(_, label, detail)| (label, detail)).collect();
        assert_eq!(
            rows,
            [
                ("db/m.xml".to_owned(), None),
                ("db/a.xml".to_owned(), None),
                ("db/c.xml".to_owned(), None),
                ("1 (x)".to_owned(), None),
                ("db/b.xml".to_owned(), None),
                ("db/c.xml".to_owned(), Some("already run".to_owned())),
            ]
        );
    }

    #[test]
    fn goes_to_included_changelogs() {
        let host = MockHost::install();
        host.add_file("db/changes/001.xml", "<databaseChangeLog/>");
        let text = "<databaseChangeLog>\n    <include file=\"changes/001.xml\" relativeToChangelogFile=\"true\"/>\n    <changeSet id=\"1\" author=\"a\"/>\n</databaseChangeLog>";
        let d = doc("/project/db/master.xml", text);
        let start = Position::new(0, 0);
        assert_eq!(Liquibase::default().definition(&d, Position::new(1, 25)), [Location { path: "/project/db/changes/001.xml".into(), range: Range::new(start, start) }]);
        assert!(Liquibase::default().definition(&d, Position::new(2, 8)).is_empty());
        let yaml = "databaseChangeLog:\n  - include:\n      file: db/changes/001.xml\n";
        assert_eq!(Liquibase::default().definition(&doc("/project/master.yaml", yaml), Position::new(2, 14)).len(), 1);
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
        let include = Include { path: String::new(), relative: false, dir: false, optional: false, at: 0, end: 0 };
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
    fn classifies_directory_probes() {
        assert_eq!(dir_exists(Err(Error::Failed("/p/changes: not a file".into()))), Some(true));
        assert_eq!(dir_exists(Ok(String::new())), Some(true));
        assert_eq!(dir_exists(Err(Error::Invalid("changes: no such file".into()))), Some(false));
        assert_eq!(dir_exists(Err(Error::NotFound("x".into()))), Some(false));
        assert_eq!(dir_exists(Err(Error::Denied("x".into()))), None);
    }

    #[test]
    fn missing_include_all_directories_are_flagged_unless_optional() {
        let host = host();
        let text = "databaseChangeLog:\n  - includeAll:\n      path: nope/\n  - includeAll:\n      path: later/\n      errorIfMissingOrEmpty: false\n  - include:\n      file: gone.yaml\n      errorIfMissing: false\n";
        assert_eq!(
            checked(&host, MASTER, text),
            [("unresolved-include".into(), "Cannot find directory `nope/`.".into())]
        );
    }

    fn complete_at_end(path: &str, text: &str) -> Vec<CompletionItem> {
        Liquibase::default().completion(&doc(path, text), openidea_plugin::text::position(text, text.len()), None)
    }

    fn labels(items: &[CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    fn item<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
        items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"))
    }

    const SET: &str = "databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: bob\n      changes:\n";

    #[test]
    fn completes_yaml_changelogs() {
        let _host = MockHost::install();
        let y = "/project/db/master.yaml";

        let text = format!("{SET}        - creat");
        let items = complete_at_end(y, &text);
        assert!(labels(&items).contains(&"createTable") && labels(&items).contains(&"createIndex"));
        assert!(!labels(&items).contains(&"createTrigger"), "Pro types are not offered");
        assert_eq!(item(&items, "createTable").insert_text, "createTable:\n            ");
        assert_eq!(item(&items, "createTable").replace, Some(spring_text::range(&text, text.len() - 5, text.len())));

        let items = complete_at_end(y, &format!("{SET}        - createTable:\n            tab"));
        assert_eq!(item(&items, "tableName").insert_text, "tableName: ");
        assert_eq!(item(&items, "columns").insert_text, "columns:\n              - ");

        let in_columns = format!("{SET}        - createTable:\n            columns:\n              - ");
        assert_eq!(labels(&complete_at_end(y, &in_columns)), ["column"]);
        let in_column = format!("{in_columns}column:\n                  ");
        let items = complete_at_end(y, &in_column);
        assert!(labels(&items).contains(&"type"));
        assert_eq!(item(&items, "constraints").insert_text, "constraints:\n                    ");
        assert!(labels(&complete_at_end(y, &format!("{in_column}constraints:\n                    "))).contains(&"primaryKey"));

        let items = complete_at_end(y, "databaseChangeLog:\n  - changeSet:\n      ");
        assert!(labels(&items).contains(&"runOnChange"));
        assert_eq!(item(&items, "changes").insert_text, "changes:\n        - ");

        let items = complete_at_end(y, "databaseChangeLog:\n  - ");
        assert_eq!(item(&items, "changeSet").insert_text, "changeSet:\n      id: ${1:id}\n      author: ${2:author}\n      changes:\n        - $0");
        assert!(item(&items, "changeSet").snippet);
        assert!(labels(&items).contains(&"includeAll"));

        let existing = format!("{SET}        - createTable:\n            tab: x");
        let caret = existing.len() - ": x".len();
        let items = Liquibase::default().completion(&doc(y, &existing), openidea_plugin::text::position(&existing, caret), None);
        assert_eq!(item(&items, "tableName").insert_text, "tableName");

        assert!(complete_at_end(y, "databaseChangeLog:\n  - changeSet:\n      author: ").is_empty());
        assert!(complete_at_end("/project/src/main/resources/application.yml", "spring:\n  ").is_empty());
    }

    #[test]
    fn completes_formatted_sql_comments() {
        let _host = MockHost::install();
        assert_eq!(labels(&complete_at_end("/project/db/a.sql", "--")), ["--liquibase formatted sql"]);

        let text = "--liquibase formatted sql\n\n--ch";
        let items = complete_at_end("/project/db/a.sql", text);
        assert_eq!(labels(&items), ["--changeset", "--rollback", "--comment"]);
        assert_eq!(item(&items, "--changeset").insert_text, "--changeset ${1:author}:${2:id}\n$0");
        assert_eq!(item(&items, "--changeset").replace, Some(spring_text::range(text, text.len() - 4, text.len())));

        assert!(complete_at_end("/project/db/a.sql", "--liquibase formatted sql\nselect 1 -- ").is_empty());
        assert!(complete_at_end("/project/schema.sql", "select 1;\n--").is_empty());
    }

    #[test]
    fn completion_survives_every_prefix() {
        let _host = MockHost::install();
        let text = format!("{SET}        - createTable:\n            columns:\n              - column:\n                  constraints:\n                    nullable: false\n");
        for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
            complete_at_end("/project/db/master.yaml", &text[..end]);
        }
        let sql = "--liquibase formatted sql\n--changeset a:1\n--rollback x\n";
        for end in 0..=sql.len() {
            complete_at_end("/project/db/a.sql", &sql[..end]);
        }
    }

    const PLACEHOLDER_XML: &str = "<databaseChangeLog>\n    <changeSet id=\"1\" author=\"ann\">\n        <createTable tableName=\"${prefix}users\"/>\n    </changeSet>\n</databaseChangeLog>\n";
    const PROPERTY_PATH: &str = "/project/db/master.xml";

    fn property_codes(host: &MockHost, text: &str) -> Vec<String> {
        checked(host, PROPERTY_PATH, text).into_iter().map(|(code, _)| code).filter(|code| code == "unresolved-property").collect()
    }

    #[test]
    fn unresolved_property_placeholder_warns() {
        let host = host();
        host.add_file("liquibase.properties", "url=jdbc:h2:mem:x\n");
        assert_eq!(
            checked(&host, PROPERTY_PATH, PLACEHOLDER_XML),
            [(
                "unresolved-property".into(),
                "Property `prefix` is not declared in this changelog or the changelogs it includes, nor set as a parameter in the project's Liquibase configuration.".into()
            )]
        );
        let range = host.diagnostics()[PROPERTY_PATH][0].range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (2, 32, 41));
    }

    #[test]
    fn declared_property_is_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let own = PLACEHOLDER_XML.replacen("<changeSet", "<property name=\"prefix\" value=\"t_\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &own).is_empty());

        host.add_file("db/props.yaml", "databaseChangeLog:\n  - property:\n      name: prefix\n      value: t_\n");
        let included = PLACEHOLDER_XML.replacen("<changeSet", "<include file=\"props.yaml\" relativeToChangelogFile=\"true\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &included).is_empty());

        let sql = "--liquibase formatted sql\n--property name:prefix value:t_\n--changeset a:1\ncreate table ${prefix}x (id int);\n";
        assert!(checked(&host, "/project/db/a.sql", sql).is_empty());
    }

    #[test]
    fn property_from_liquibase_properties_is_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "liquibase.parameters.prefix=t_\n");
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());

        let host = MockHost::install();
        host.add_file("src/main/resources/application.yaml", "spring:\n  liquibase:\n    parameters:\n      prefix: t_\n");
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());

        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "spring.liquibase.parameters.other=1\n");
        assert_eq!(property_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
    }

    #[test]
    fn unknown_project_config_makes_the_check_quiet() {
        let host = host();
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());
        host.add_file("liquibase.properties", "url=x\n");
        let unresolved = PLACEHOLDER_XML.replacen("<changeSet", "<include file=\"${dir}/x.xml\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &unresolved).is_empty());
        let from_file = PLACEHOLDER_XML.replacen("<changeSet", "<property file=\"p.properties\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &from_file).is_empty());
        let include_all = PLACEHOLDER_XML.replacen("<changeSet", "<includeAll path=\"changes/\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &include_all).is_empty());
    }

    #[test]
    fn builtin_database_property_is_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let text = PLACEHOLDER_XML.replace("${prefix}users", "${database.defaultSchemaName}");
        assert!(property_codes(&host, &text).is_empty());
    }

    #[test]
    fn placeholder_is_quiet_for_library_or_dynamic_name() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        for name in ["${:prefix}", "${}", "${a b}"] {
            assert!(property_codes(&host, &PLACEHOLDER_XML.replace("${prefix}", name)).is_empty(), "{name}");
        }
        let yaml = "databaseChangeLog:\n  # ${commented}\n  - changeSet:\n      id: 1\n      author: a\n";
        assert!(checked(&host, MASTER, yaml).is_empty());
    }

    #[test]
    fn property_ranges_are_utf16_and_survive_crlf() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let text = "databaseChangeLog:\r\n  - changeSet:\r\n      id: \"é😀\"\r\n      author: a\r\n      comment: \"😀 ${nope}\"\r\n";
        assert_eq!(property_codes_at(&host, text), ["unresolved-property"]);
        let range = host.diagnostics()[MASTER][0].range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (4, 19, 26));
    }

    fn property_codes_at(host: &MockHost, text: &str) -> Vec<String> {
        checked(host, MASTER, text).into_iter().map(|(code, _)| code).filter(|code| code == "unresolved-property").collect()
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        assert_eq!(property_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
        assert!(property_codes(&host, &PLACEHOLDER_XML.replace("${prefix}", "")).is_empty());
    }

    #[test]
    fn setting_turns_the_property_check_off() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        host.set_setting("checkProperties", "false");
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    const CHILD: &str = "/project/db/changes/child.xml";
    const MASTER_WITH_PROPERTY: &str = "<databaseChangeLog>\n    <property name=\"prefix\" value=\"t_\"/>\n    <include file=\"changes/child.xml\" relativeToChangelogFile=\"true\"/>\n</databaseChangeLog>\n";

    fn child_codes(host: &MockHost, text: &str) -> Vec<String> {
        checked(host, CHILD, text).into_iter().map(|(code, _)| code).filter(|code| code == "unresolved-property").collect()
    }

    #[test]
    fn property_declared_in_the_master_is_quiet_in_a_child_changelog() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        host.add_file("db/changes/child.xml", PLACEHOLDER_XML);
        assert_eq!(child_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
        host.add_file("db/master.xml", MASTER_WITH_PROPERTY);
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn property_declared_in_a_sibling_included_by_the_master_is_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        host.add_file("db/changes/child.xml", PLACEHOLDER_XML);
        host.add_file("db/changes/props.xml", "<databaseChangeLog><property name=\"prefix\" value=\"t_\"/></databaseChangeLog>");
        host.add_file(
            "db/master.xml",
            "<databaseChangeLog>\n<include file=\"changes/props.xml\" relativeToChangelogFile=\"true\"/>\n<include file=\"changes/child.xml\" relativeToChangelogFile=\"true\"/>\n</databaseChangeLog>",
        );
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn scan_cap_makes_the_check_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        for n in 0..=MAX_CHANGELOGS {
            host.add_file(&format!("db/many/{n}.yaml"), "databaseChangeLog: []\n");
        }
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn include_all_anywhere_makes_the_check_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        host.add_file("db/master.xml", "<databaseChangeLog><includeAll path=\"changes/\"/></databaseChangeLog>");
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn parameter_in_a_profile_file_is_quiet() {
        let host = host();
        host.add_file("src/main/resources/application.yaml", "spring:\n  application:\n    name: x\n");
        assert_eq!(child_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
        host.add_file("src/main/resources/application-dev.yaml", "spring:\n  liquibase:\n    parameters:\n      prefix: t_\n");
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn parameter_in_a_module_application_yaml_is_quiet() {
        let host = host();
        host.add_file("src/main/resources/application.yaml", "spring:\n  application:\n    name: x\n");
        host.add_file("api/src/main/resources/application.yaml", "spring:\n  liquibase:\n    parameters:\n      prefix: t_\n");
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn config_that_cannot_be_followed_makes_the_check_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\ndefaultsFile=../shared.properties\n");
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
        let host = MockHost::install();
        host.add_file("src/main/resources/application.properties", "spring.config.import=optional:file:./extra.properties\n");
        assert!(child_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    fn placeholder_in(path: &str, text: &str) -> Vec<String> {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        checked(&host, path, text).into_iter().map(|(code, _)| code).filter(|code| code == "unresolved-property").collect()
    }

    #[test]
    fn doubled_dollar_is_quiet() {
        assert!(placeholder_in(PROPERTY_PATH, &PLACEHOLDER_XML.replace("${prefix}", "$${prefix}")).is_empty());
    }

    #[test]
    fn backslash_escaped_placeholder_is_quiet() {
        assert!(placeholder_in(PROPERTY_PATH, &PLACEHOLDER_XML.replace("${prefix}", "\\${prefix}")).is_empty());
    }

    #[test]
    fn placeholder_in_xml_comment_is_quiet() {
        let text = "<databaseChangeLog>\n<!-- uses ${prefix}\n and more -->\n</databaseChangeLog>";
        assert!(placeholder_in(PROPERTY_PATH, text).is_empty());
        assert_eq!(placeholder_in(PROPERTY_PATH, &format!("{text}<!-- x --><a b=\"${{late}}\"/>")).len(), 1);
    }

    #[test]
    fn placeholder_in_sql_line_comment_is_quiet() {
        let text = "--liquibase formatted sql\n-- run ${prefix}\n--changeset a:1\nselect 1;\n";
        assert!(placeholder_in("/project/db/a.sql", text).is_empty());
        let live = "--liquibase formatted sql\n--changeset a:1\nselect ${prefix};\n";
        assert_eq!(placeholder_in("/project/db/a.sql", live), ["unresolved-property"]);
    }

    #[test]
    fn numeric_reference_is_quiet() {
        assert!(placeholder_in(PROPERTY_PATH, &PLACEHOLDER_XML.replace("${prefix}", "${1}")).is_empty());
    }
}
