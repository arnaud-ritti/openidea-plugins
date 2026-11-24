//! Liquibase Changelogs: an OpenIDEA plugin.

mod array_actions;
mod strict_json;
mod generated_changelog;
mod execution_review;
mod catalog;
mod changelog;
mod console;
mod native_runs;
mod native_browser;
mod checks;
mod complete;
mod file_actions;
mod generate;
mod json;
pub(crate) const INCLUDE_MARKER: &str = "icons/marker-include.svg";

mod live;
mod local_references;
mod file_references;
mod selection;
mod offline_diff;
mod search;
mod status;

use changelog::{Changelog, Format, Include, Target};
use openidea_plugin::PickItem;
use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, Location, NotificationLevel, Plugin, Position, Project, Range, host};
use spring_text::index::project_of;
use spring_text::tree::Expansion;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

const MAX_FILES: usize = 2000;
/// Reading time of a command's changelog walk, under a task (12 times the fuel of a plain call).
const DISCOVERY_BUDGET: Duration = Duration::from_secs(3);
/// Reading time of a render, which runs without a task: its 5×10⁹ fuel must last the whole call.
const VIEW_BUDGET: Duration = Duration::from_secs(1);

/// Refresh the native browser without touching CLI/output views.
fn refresh_browser() {
    ui_kit::web::refresh_view("changesets", include_str!("ui_config.json"));
}

#[derive(Default)]
pub struct Liquibase {
    /// Set while a command resolves an action, so its failure is reported once by the command.
    command_resolving: bool,
    browser_filter: Option<(String, String)>,
    browser_selection: Option<native_browser::Selection>,
    expansion: Expansion,
    offline: offline_diff::State,
    live: live::State,
    /// Changelog count that cuts discovery short; `None`: the default.
    max_files: Option<usize>,
    /// File and 0-based line by tree item key, from the last render of the changesets view.
    targets: BTreeMap<String, (String, u32)>,
    targets_root: Option<String>,
    /// `git config user.name`, asked once; the outer `None`: not asked yet.
    git_user: Option<Option<String>>,
    /// The last explicit `status` run, kept until its sources change.
    status: status::State,
    search: search::Index,
}

impl Plugin for Liquibase {
    fn activate(&mut self, _ctx: &openidea_plugin::Context) -> Result<(), Error> {
        let subscriptions = [
            host::subscribe(openidea_plugin::EventKind::FileOpened),
            host::subscribe(openidea_plugin::EventKind::FileSaved),
            host::subscribe_paths("files-changed", &["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.sql", "**/liquibase.properties"]),
        ];
        if subscriptions.iter().any(|result| matches!(result, Err(Error::Denied(_)))) {
            self.live.events_denied = true;
            host::notify(NotificationLevel::Warning, TITLE, "Allow events for Liquibase Changelogs in Settings › Plugins so include and property checks follow your edits.");
        }
        Ok(())
    }
    fn handle_events(&mut self, events: &[openidea_plugin::Event], _ctx: &Invocation) {
        self.handle_events_until(events, Instant::now() + EVENT_BUDGET);
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "offline-diff"=>{ui_kit::web::reveal_view(offline_diff::VIEW, include_str!("ui_config.json"))?;Ok(None)},
            "offline-diff-run"=>{self.start_offline_diff(ctx)?;Ok(None)},
            "choose-sources" => self.choose_sources(ctx),
            "clear-sources" => self.clear_sources(ctx),
            "update-confirmed" => {
                execution_review::run(ctx, vec!["update".to_owned()], "Update {db}?", "Applies every pending changeset to the database. To see them first, cancel and run Liquibase: Show Pending Changesets…")?;
                Ok(None)
            }
            "rollback-count" => {
                let Some(answer) = host::input("Number of changesets to roll back", Some("1"))? else {
                    return Ok(None);
                };
                let Ok(count) = answer.trim().parse::<u32>() else {
                    host::notify(NotificationLevel::Warning, TITLE, &format!("\"{}\" is not a number of changesets.", answer.trim()));
                    return Ok(None);
                };
                if count == 0 {
                    host::notify(NotificationLevel::Warning, TITLE, "Roll back at least one changeset.");
                    return Ok(None);
                }
                let body = format!("Rolls back the last {count} deployed changeset(s).");
                execution_review::run(ctx, vec!["rollback-count".to_owned(), count.to_string()], &format!("Roll back {count} changeset(s) in {{db}}?"), &body)?;
                Ok(None)
            }
            status::COMMAND => {
                self.show_pending(ctx)?;
                Ok(None)
            }
            "new-changeset" => self.generator_entry(ctx, "new-changeset"),
            "new-changelog" => self.generator_entry(ctx, "new-changelog"),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

const TITLE: &str = "Liquibase Changelogs";
/// One event batch, all of it: the host stops a handler at 5 s.
const EVENT_BUDGET: Duration = Duration::from_secs(1);

impl Liquibase {
    /// Applies a batch of events by `deadline`. Changelogs it does not reach are forgotten, so their
    /// checks stay quiet until the next event or the provider's own bounded warm.
    fn handle_events_until(&mut self, events: &[openidea_plugin::Event], deadline: Instant) {
        let mut changed = false;
        for event in events {
            match event {
                openidea_plugin::Event::FilesChanged(batch) => {
                    for path in batch.created.iter().chain(&batch.modified).chain(&batch.removed) {
                        self.live.invalidate(path);
                        self.search.invalidate(path);
                    }
                    for path in &batch.modified {
                        self.status_touched(path, false);
                    }
                    for path in batch.created.iter().chain(&batch.removed) {
                        self.status_touched(path, true);
                    }
                    changed = true;
                }
                openidea_plugin::Event::FileOpened(path) | openidea_plugin::Event::FileSaved(path) => {
                    if matches!(event, openidea_plugin::Event::FileSaved(_)) {
                        self.status_touched(path, false);
                        self.search.invalidate(path);
                    }
                    self.live.forget(path);
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if !remaining.is_zero() {
                        self.warm_from_disk(path, remaining);
                    }
                }
                _ => {}
            }
        }
        if changed {
            self.rewarm(deadline);
        }
    }

    fn warm_from_disk(&mut self, path: &str, budget: Duration) {
        let Some(project) = host::project_of(path) else {
            return;
        };
        let (Ok(root), Ok(text)) = (project.root(), project.read_file(path)) else {
            self.live.forget(path);
            return;
        };
        self.live.update(&root, path, &text);
        self.live.warm(&project, &root, path, &text, budget, None, true);
    }

    fn rewarm(&mut self, deadline: Instant) {
        for path in self.live.warmed_paths() {
            match deadline.saturating_duration_since(Instant::now()) {
                remaining if remaining.is_zero() => self.live.forget(&path),
                remaining => self.warm_from_disk(&path, remaining),
            }
        }
    }
}

fn root_relative<'a>(path: &'a str, root: &str) -> &'a str {
    path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches('/'))
}

/// A host that cannot show the dialog (`Invalid`) leaves a command that writes to the database unconfirmed, so it does not run.
fn confirmed(title: &str, body: &str) -> Result<bool, Error> {
    match ui_kit::prompts::confirm_exact(title, body) {
        Err(Error::Invalid(_)) => {
            host::notify(NotificationLevel::Warning, TITLE, "This host cannot ask for confirmation. Run the command from a terminal instead.");
            Ok(false)
        }
        other => other,
    }
}

fn run_liquibase(args: Vec<String>) -> Result<(), Error> {
    let title = format!("Liquibase {}", args.join(" "));
    match host::open_terminal(&title, "liquibase", &args) {
        Err(Error::Denied(_)) => {
            host::notify(NotificationLevel::Warning, TITLE, &ui_kit::denied::message("Running Liquibase", "process:liquibase", TITLE));
            Ok(())
        }
        other => other,
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
#[cfg(test)]
fn target(project: &Project, root: &str, changelog: &str, include: &Include) -> Target {
    target_until(project, root, changelog, include, None)
}

/// As `target`, but `Unknown` once `deadline` has passed.
fn target_until(project: &Project, root: &str, changelog: &str, include: &Include, deadline: Option<Instant>) -> Target {
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
        if deadline.is_some_and(|deadline| Instant::now() > deadline) {
            return Target::Unknown;
        }
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
fn project_properties(project: &Project, root: &str, path: &str, open: &Changelog, deadline: Instant) -> Option<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for glob in ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.sql", "**/*.json"] {
        if Instant::now() > deadline {
            return None;
        }
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
            if include.dir || target_until(project, root, at, include, Some(deadline)) == Target::Unknown {
                return None;
            }
        }
        Some(())
    };
    absorb(path, open)?;
    for file in paths {
        if Instant::now() > deadline {
            return None;
        }
        let text = match project.read_file(&file) {
            Ok(text) => text,
            Err(Error::NotFound(_) | Error::Invalid(_)) => continue,
            Err(_) => return None,
        };
        if text.len() > MAX_CHANGELOG_BYTES {
            return None;
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

const BUILD_PARAMETER_MARKERS: [&str; 6] =
    ["expressionVariables", "expressionVars", "changelogParameters", "changeLogParameters", "liquibaseProperties", "propertyFile"];

/// Whether a build file between the changelog and the project root passes parameters to the Liquibase Maven or Gradle plugin.
fn build_passes_parameters(project: &Project, root: &str, changelog: &str) -> bool {
    let mut dir = changelog;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        if !parent.starts_with(root) {
            break;
        }
        for file in ["pom.xml", "build.gradle", "build.gradle.kts"] {
            let Ok(text) = project.read_file(&format!("{parent}/{file}")) else {
                continue;
            };
            if BUILD_PARAMETER_MARKERS.iter().any(|marker| text.contains(marker)) {
                return true;
            }
        }
        dir = parent;
    }
    false
}

/// The parameters the project's `liquibase.properties` and Spring configuration files (profile and module files included) set;
/// `None` when none of those files can be read or the configuration cannot be followed completely.
fn project_parameters(project: &Project, root: &str, changelog: &str, deadline: Instant) -> Option<BTreeSet<String>> {
    if build_passes_parameters(project, root, changelog) {
        return None;
    }
    let mut files: BTreeSet<String> =
        ["liquibase.properties", "src/main/resources/liquibase.properties"].iter().map(|file| format!("{root}/{file}")).collect();
    for ext in ["properties", "yaml", "yml"] {
        files.insert(format!("{root}/src/main/resources/application.{ext}"));
    }
    for glob in ["**/liquibase.properties", "**/application*.properties", "**/application*.yaml", "**/application*.yml"] {
        if Instant::now() > deadline {
            return None;
        }
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
        if Instant::now() > deadline {
            return None;
        }
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
    /// Changeset tree key → (file, id, author).
    sets: BTreeMap<String, (String, String, String)>,
    visited: BTreeSet<String>,
    resolved: BTreeMap<(String, usize), (Vec<String>, Target)>,
    max_files: usize,
    partial: bool,
    budget: Duration,
    deadline: Instant,
    reads: usize,
    discovery_done: bool,
}

impl<'a> Walk<'a> {
    fn new(project: &'a Project, root: &'a str, max_files: usize) -> Self {
        Self {
            project,
            root,
            found: BTreeMap::new(),
            items: Vec::new(),
            targets: BTreeMap::new(),
            sets: BTreeMap::new(),
            visited: BTreeSet::new(),
            resolved: BTreeMap::new(),
            max_files,
            partial: false,
            budget: DISCOVERY_BUDGET,
            deadline: Instant::now() + DISCOVERY_BUDGET,
            reads: 0,
            discovery_done: false,
        }
    }

    fn within(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self.deadline = Instant::now() + budget;
        self
    }

    // ponytail: reads every xml/yaml/sql file the host lists, up to MAX_FILES per extension and the walk's budget (checked between reads), on every render; index them (spring_text::index) if big projects hit the cap
    fn discover(&mut self) {
        if self.discovery_done {
            return;
        }
        self.discovery_done = true;
        let start = Instant::now();
        let mut paths = BTreeSet::new();
        for glob in ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.sql"] {
            if Instant::now() > self.deadline {
                self.partial = true;
                break;
            }
            let listed = match self.project.find_files(glob, self.max_files as u32) {
                Ok(files) => files,
                Err(_) => {
                    self.partial = true;
                    Vec::new()
                }
            };
            self.partial |= listed.len() >= self.max_files;
            paths.extend(listed);
        }
        for path in paths {
            if self.found.len() >= self.max_files || start.elapsed() > self.budget {
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
        if self.reads >= self.max_files || Instant::now() > self.deadline {
            self.partial = true;
            return None;
        }
        self.reads += 1;
        if !self.project.stat(path).is_ok_and(|metadata| metadata.kind == openidea_plugin::FileKind::File && metadata.size <= 512 * 1024) {
            self.partial = true;
            return None;
        }
        let text = match self.project.read_file(path) {
            Ok(text) if text.len() <= 512 * 1024 => text,
            _ => {
                self.partial = true;
                return None;
            }
        };
        let log = changelog::parse(path, &text)?;
        let loaded = Rc::new((text, log));
        self.found.insert(path.to_owned(), loaded.clone());
        Some(loaded)
    }

    /// Changelogs nothing includes, then any left over (an include cycle has no root).
    fn walk_all(&mut self) {
        let paths: Vec<String> = self.found.keys().cloned().collect();
        let mut included = BTreeSet::new();
        'roots: for path in &paths {
            let log = self.found[path].clone();
            for index in 0..log.1.includes.len() {
                if Instant::now() > self.deadline || self.resolved.len() >= 4096 {
                    self.partial = true;
                    break 'roots;
                }
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
        if Instant::now() > self.deadline || self.resolved.len() >= 4096 {
            self.partial = true;
            return (Vec::new(), Target::Unknown);
        }
        if let Some(known) = self.resolved.get(&key) {
            return known.clone();
        }
        let include = self.found[path].clone().1.includes[index].clone();
        if include.dir && !self.discovery_done {
            self.discover();
        }
        let found = selection::browse_target(self.project, self.root, path, &include);
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
        if self.items.len() >= 4096 || stack.len() >= 64 || Instant::now() > self.deadline {
            self.partial = true;
            return;
        }
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
            self.items.push(item.detail("already shown"));
            return;
        }
        self.items.push(item);
        self.visited.insert(path.to_owned());
        let Some(loaded) = self.load(path) else {
            if let Some(item) = self.items.last_mut() {
                item.detail = Some("source unavailable or discovery limit".into());
            }
            return;
        };
        let (text, log) = (&loaded.0, &loaded.1);

        let mut steps: Vec<(usize, bool, usize)> = Vec::new();
        steps.extend(log.change_sets.iter().enumerate().map(|(i, set)| (set.at, false, i)));
        steps.extend(log.includes.iter().enumerate().map(|(i, include)| (include.at, true, i)));
        steps.sort();

        stack.push(path.to_owned());
        for (_, is_include, index) in steps {
            if self.items.len() >= 4096 || Instant::now() > self.deadline {
                self.partial = true;
                break;
            }
            if !is_include {
                let set = &log.change_sets[index];
                let label = format!("{} ({})", set.id.as_deref().unwrap_or("?"), set.author.as_deref().unwrap_or("?"));
                let child = format!("{key}/c{index}");
                let mut item = TreeItem::new(&child, &label).parent(&key).icon("database");
                if let Some(description) = changelog::describe(log, text, index) {
                    item = item.detail(&description);
                }
                self.targets.insert(child.clone(), (path.to_owned(), changelog::line_of(text, set.at)));
                self.sets.insert(child, (path.to_owned(), set.id.clone().unwrap_or_default(), set.author.clone().unwrap_or_default()));
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

impl Liquibase {
    fn author(&mut self) -> Option<String> {
        let configured = host::setting_string("author").filter(|a| !a.trim().is_empty());
        if configured.is_some() {
            return configured;
        }
        self.git_user
            .get_or_insert_with(|| {
                let output = host::run_process("git", &["config".into(), "user.name".into()], None).ok()?;
                let name = output.stdout.trim().to_owned();
                (!name.is_empty()).then_some(name)
            })
            .clone()
    }

    fn include_files(doc: &Document, include: &Include, from: usize, to: usize) -> Vec<CompletionItem> {
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root) = project.root() else {
            return Vec::new();
        };
        let mut files = Vec::new();
        for glob in ["**/*.xml", "**/*.yaml", "**/*.yml", "**/*.json", "**/*.sql"] {
            files.extend(project.find_files(glob, MAX_LISTED_FILES as u32).unwrap_or_default());
        }
        let paths = complete::file_paths(&doc.path, &root, include.relative, &files);
        complete::value_items(&doc.text, from, to, &paths, CompletionKind::File)
    }
}

impl Providers for Liquibase {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(caret) = offset(&doc.text, at) else {
            return Vec::new();
        };
        let Some(log) = changelog::parse(&doc.path, &doc.text) else {
            return if doc.path.ends_with(".sql") { complete::sql(&doc.text, caret, false) } else { Vec::new() };
        };
        if let Some((from, to)) = complete::author_value(&doc.path, &doc.text, caret) {
            let authors: Vec<String> = self.author().into_iter().collect();
            return complete::value_items(&doc.text, from, to, &authors, CompletionKind::Value);
        }
        let blank_value = |i: &Include| i.path.is_empty() && caret >= i.at && doc.text[i.at..caret].trim().is_empty();
        if let Some(include) = log.includes.iter().find(|i| !i.dir && (i.at <= caret && caret <= i.end || blank_value(i))) {
            let (from, to) = if include.path.is_empty() { (caret, caret) } else { (include.at, include.end) };
            return Self::include_files(doc, include, from, to);
        }
        if doc.path.ends_with(".sql") {
            return complete::sql(&doc.text, caret, true);
        }
        if doc.path.ends_with(".json") {
            return complete::json(&doc.text, caret);
        }
        if doc.path.ends_with(".xml") {
            return complete::xml(&doc.text, caret);
        }
        if doc.path.ends_with(".yaml") || doc.path.ends_with(".yml") {
            return complete::yaml(&doc.text, caret);
        }
        Vec::new()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let Some(caret) = offset(&doc.text, at) else { return Vec::new(); };
        let local = local_references::definition(doc, caret);
        if !local.is_empty() { return local; }
        let Some(log) = live::parse(&doc.path, &doc.text) else { return Vec::new(); };
        if matches!(log.format, changelog::Format::Yaml | changelog::Format::Json)
            && generate::parent_log(&doc.path, &doc.text).is_err() { return Vec::new(); }
        if log.format == changelog::Format::Xml
            && generate::navigation_xml_log(&doc.text).map_or(true, |parent| parent.opaque) { return Vec::new(); }
        let Some(include) = file_references::reference(doc, caret)
            .or_else(|| log.includes.iter().find(|i| !i.dir && i.at <= caret && caret <= i.end).cloned()) else {
            return Vec::new();
        };
        let Some(project) = project_of(&doc.path) else {
            return Vec::new();
        };
        let Ok(root) = project.root() else {
            return Vec::new();
        };
        if include.path.len() > 4096 || include.path.chars().any(char::is_control)
            || include.path.contains('\\') || include.path.contains("://")
            || include.path.starts_with('/') { return Vec::new(); }
        let mut found = std::collections::BTreeSet::new();
        for path in changelog::candidates(&doc.path, &root, &include).into_iter().take(16) {
            if project_of(&path).and_then(|p| p.root().ok()).as_deref() == Some(root.as_str())
                && project.stat(&path).is_ok_and(|s| s.kind == openidea_plugin::FileKind::File) { found.insert(path); }
        }
        found.into_iter().map(|path| Location { path, range: Range::new(Position::new(0,0),Position::new(0,0)) }).collect()
    }

    /// Navigation markers use only current text and cached positive include facts.
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.live.markers(doc)
    }
}

impl Views for Liquibase {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            if _view==offline_diff::VIEW{return self.render_offline_diff(_viewport,ctx);}
            self.targets.clear();
            self.targets_root = None;
            let refresh = Button::new("refresh", "Refresh").icon("refresh");
            let choose = || Button::new("choose-sources", "Choose Sources…").icon("folder");
            let clear = || Button::new("clear-sources", "Browse All Changelogs").icon("list");
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its Liquibase changesets."));
            };
            let Ok(root) = selection::project_root(&project) else {
                return Ok(View::message("The project folder could not be read. Reopen the project, then press Refresh."));
            };
            let (selected, source_status) = match self.selection_state(&project) {
                selection::State::All => (None, None),
                selection::State::Selected { path, summary } => (Some(path), Some(summary)),
                selection::State::Unavailable(message) => (None, Some(message)),
                selection::State::Invalid(message) => return Ok(View::message(&message).button(choose()).button(clear())),
            };
            let mut walk = Walk::new(&project, &root, self.max_files.unwrap_or(MAX_FILES)).within(VIEW_BUDGET);
            if let Some(selected) = &selected {
                if walk.load(selected).is_none() {
                    return Ok(View::message("The changelog you chose was changed, moved or deleted. Pick another with Choose Sources…, or press Browse All Changelogs.")
                        .button(choose())
                        .button(clear()));
                }
            }
            if selected.is_none() {
                walk.discover();
            }
            if walk.found.is_empty() {
                return Ok(View::message(if walk.partial {
                    "No Liquibase changelog was found before the search time ran out. Press Refresh to search again, or pick one with Choose Sources…"
                } else {
                    "No Liquibase changelogs in this project yet. Press New Changelog… to create one."
                })
                .button(Button::new("new-changelog", "New Changelog…").icon("plus"))
                .button(refresh)
                .button(choose()));
            }
            if let Some(selected) = &selected {
                walk.file(selected, None, &mut Vec::new());
            } else {
                walk.walk_all();
            }

            let warm_started = Instant::now();
            let mut warm_bytes = 0usize;
            for (path, fact) in walk.found.iter().take(128) {
                warm_bytes = warm_bytes.saturating_add(fact.0.len());
                if warm_bytes > 4 * 1024 * 1024 || warm_started.elapsed() > Duration::from_millis(30) {
                    break;
                }
                self.live.update(&root, path, &fact.0);
            }
            let partial = walk.partial;
            let mut items = std::mem::take(&mut walk.items);
            for item in items.iter_mut() {
                item.expanded = true;
            }
            self.expansion.apply(&mut items);
            let checked = self.status_line(&project, &root);
            self.mark_status(&root, &mut items, &walk.sets);
            self.targets = std::mem::take(&mut walk.targets);
            self.targets_root = selection::project_root(&project).ok();
            let sets = self.targets.keys().filter(|key| key.contains("/c")).count();
            let mut status = format!("{sets} changesets");
            if partial {
                status = format!("{status}; partial: showing the first {} changelogs (time budget)", walk.found.len());
            }
            if let Some(source) = source_status {
                status = format!("{status}; {source}");
            }
            if let Some(checked) = checked {
                status = format!("{checked} · {status}");
            }
            let shown = View::new(Body::Tree(items)).button(refresh).status(&status);
            Ok(self.native_browser_view(&project, shown))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, _ctx, include_str!("ui_config.json"))? { return Ok(()); }
        if view==offline_diff::VIEW{return self.handle_offline_diff(event,_ctx);}
        if view == "changesets" && self.native_browser_event(&event, _ctx)? {
            return Ok(());
        }

        if self.expansion.handle(&event) {
            refresh_browser();
            return Ok(());
        }
        match event {
            ViewEvent::Button(button) if button == "refresh" => refresh_browser(),
            ViewEvent::Button(button) if button == "choose-sources" => {
                self.choose_sources(_ctx)?;
            }
            ViewEvent::Button(button) if button == "clear-sources" => {
                self.clear_sources(_ctx)?;
            }
            ViewEvent::Button(button) if button == "new-changelog" => {
                self.generator_entry(_ctx, "new-changelog")?;
                refresh_browser();
            }
            ViewEvent::Activate(key) => {
                let current = _ctx.project().and_then(|project| selection::project_root(&project).ok());
                if current.is_none() || current != self.targets_root {
                    return Err(Error::Stale);
                }
                if let Some((path, line)) = self.targets.get(&key) {
                    host::open_location(path, *line, 0)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// API 1.3 registers the optional exports on the same component.
impl openidea_plugin::diagnostics::DiagnosticsProvider for Liquibase {
    fn diagnostics(&mut self, doc: &Document) -> Vec<openidea_plugin::Diagnostic> {
        self.live.diagnostics(doc)
    }
}
impl openidea_plugin::hover::HoverProvider for Liquibase {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<openidea_plugin::hover::Hover> {
        self.live.hover(doc, at)
    }
}
openidea_plugin::register_plugin!(Liquibase, providers, views, diagnostics, hover, file_actions, run_configurations, search);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::Document;

    #[test]
    fn update_runs_only_when_confirmed() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:liquibase"]);
        let mut liquibase = Liquibase::default();

        host.answer_next_prompt(None);
        liquibase.run_command("update-confirmed", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());

        host.answer_next_prompt(Some(&[]));
        host.answer_next_prompt(Some(&["yes"]));
        liquibase.run_command("update-confirmed", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());

        host.answer_next_prompt(Some(&[]));
        host.answer_next_prompt(Some(&["update"]));
        liquibase.run_command("update-confirmed", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["update"]);
    }

    #[test]
    fn empty_browser_offers_new_changelog() {
        let host = MockHost::install().with_capabilities(&["project.read", "project.write"]);
        let mut liquibase = Liquibase::default();
        let view = liquibase.render("changesets", None, &host.invocation()).unwrap();
        assert!(matches!(&view.body, Body::Message(text) if text.contains("New Changelog…")));
        assert!(view.toolbar.iter().any(|button| button.id == "new-changelog"));
        host.answer_next_prompt(Some(&["db/changelog.xml"]));
        liquibase.handle("changesets", ViewEvent::Button("new-changelog".into()), &host.invocation()).unwrap();
        assert!(host.invocation().project().unwrap().exists("db/changelog.xml").unwrap());
    }

    #[test]
    fn update_with_a_readable_database_needs_only_the_confirm() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:liquibase"]);
        host.add_file("liquibase.properties", "url=jdbc:mysql://db.example:3306/shop\n");
        host.answer_next_prompt(Some(&[]));
        Liquibase::default().run_command("update-confirmed", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["update"]);
    }

    #[test]
    fn rollback_count_asks_for_a_count_then_confirms() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:liquibase"]);
        let mut liquibase = Liquibase::default();

        host.answer_next_prompt(None);
        liquibase.run_command("rollback-count", &host.invocation(), None).unwrap();
        host.answer_next_prompt(Some(&["two"]));
        liquibase.run_command("rollback-count", &host.invocation(), None).unwrap();
        host.answer_next_prompt(Some(&["0"]));
        liquibase.run_command("rollback-count", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());
        assert_eq!(host.notifications().len(), 2);

        host.answer_next_prompt(Some(&["3"]));
        host.answer_next_prompt(None);
        liquibase.run_command("rollback-count", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());

        host.answer_next_prompt(Some(&["3"]));
        host.answer_next_prompt(Some(&[]));
        host.answer_next_prompt(Some(&["rollback-count"]));
        liquibase.run_command("rollback-count", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["rollback-count", "3"]);
    }

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
            Some("json") => "json",
            _ => "yaml",
        };
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn provider_diagnostics(path: &str, text: &str) -> Vec<openidea_plugin::Diagnostic> {
        let mut liquibase = Liquibase::default();
        if let Some(project) = project_of(path) {
            if let Ok(root) = project.root() {
                liquibase.live.warm(&project, &root, path, text, Duration::from_secs(2), None, true);
            }
        }
        openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut liquibase, &doc(path, text))
    }

    fn checked(_host: &MockHost, path: &str, text: &str) -> Vec<(String, String)> {
        provider_diagnostics(path, text).into_iter().map(|d| (d.code.unwrap_or_default(), d.message)).collect()
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
        let host = MockHost::install().with_capabilities(&["project.read", "process:liquibase"]);
        let mut liquibase = Liquibase::default();
        let Body::Message(text) = liquibase.render("changesets", None, &host.invocation()).unwrap().body else {
            panic!("message expected")
        };
        assert_eq!(text, "No Liquibase changelogs in this project yet. Press New Changelog… to create one.");

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
    fn a_changelog_reached_twice_is_already_shown() {
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
                ("db/c.xml".to_owned(), Some("already shown".to_owned())),
            ]
        );
    }

    #[test]
    fn direct_definitions_refuse_foreign_namespace_and_nested_project_targets() {
        let host=MockHost::install();
        host.add_file("db/nested/child.xml","<databaseChangeLog/>");
        host.add_project("/project/db/nested");
        let text="<databaseChangeLog><include file='nested/child.xml' relativeToChangelogFile='true'/></databaseChangeLog>";
        assert!(Liquibase::default().definition(&doc("/project/db/a.xml",text),openidea_plugin::text::position(text,text.find("nested/").unwrap()+2)).is_empty());
        host.add_file("db/child.xml","<databaseChangeLog/>");
        let foreign="<databaseChangeLog xmlns='urn:foreign'><include file='child.xml' relativeToChangelogFile='true'/></databaseChangeLog>";
        assert!(Liquibase::default().definition(&doc("/project/db/a.xml",foreign),openidea_plugin::text::position(foreign,foreign.find("child.xml").unwrap()+2)).is_empty());
    }

    #[test]
    fn an_include_found_under_several_classpath_roots_offers_each_file() {
        let host = MockHost::install();
        host.add_file("src/test/resources/db/child.xml", "<databaseChangeLog/>");
        let text = "<databaseChangeLog>\n<include file=\"db/child.xml\"/>\n</databaseChangeLog>";
        let d = doc("/project/src/test/resources/db/master.xml", text);
        let at = Position::new(1, 17);
        assert_eq!(Liquibase::default().definition(&d, at).len(), 1);
        host.add_file("src/main/resources/db/child.xml", "<databaseChangeLog/>");
        let found: Vec<_> = Liquibase::default().definition(&d, at).into_iter().map(|l| l.path).collect();
        assert_eq!(found, ["/project/src/main/resources/db/child.xml", "/project/src/test/resources/db/child.xml"]);
    }

    #[test]
    fn goes_to_included_changelogs() {
        let host = MockHost::install();
        host.add_file("db/changes/001.xml", "<databaseChangeLog/>");
        let text = "<databaseChangeLog>\n    <include file=\"changes/001.xml\" relativeToChangelogFile=\"true\"/>\n    <changeSet id=\"1\" author=\"a\"/>\n</databaseChangeLog>";
        let d = doc("/project/db/master.xml", text);
        let start = Position::new(0, 0);
        assert_eq!(
            Liquibase::default().definition(&d, Position::new(1, 25)),
            [Location { path: "/project/db/changes/001.xml".into(), range: Range::new(start, start) }]
        );
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
        let duplicate = provider_diagnostics(MASTER, MASTER_YAML).into_iter().find(|d| d.code.as_deref() == Some("duplicate-changeset")).unwrap();
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
    fn reports_structural_xml_rules() {
        let host = host();
        let text = r#"<databaseChangeLog>
    <include/>
    <changeSet id="1" author="bob">
        <preConditions onFail="STOP"><tableExistz tableName="t"/><dbms type="h2"/></preConditions>
        <sqlFile path="nope.sql" relativeToChangelogFile="true"/>
        <sqlFile/>
    </changeSet>
</databaseChangeLog>"#;
        let codes: Vec<String> = checked(&host, "/project/db/master.xml", text).into_iter().map(|(c, _)| c).collect();
        assert_eq!(codes, ["include-without-path", "invalid-precondition-action", "unknown-precondition", "unresolved-sql-file", "sqlfile-without-path"]);
    }

    #[test]
    fn xsd_changelog_level_preconditions_accept_only_halt_or_warn_changeset_level_four_values() {
        let host = host();
        let codes = |text: &str| -> Vec<String> { checked(&host, "/project/db/master.xml", text).into_iter().map(|(c, _)| c).collect() };
        let root = |on_fail: &str| format!(r#"<databaseChangeLog><preConditions onFail="{on_fail}" onError="WARN"><tableExists tableName="t"/></preConditions></databaseChangeLog>"#);
        let set = |on_fail: &str| format!(r#"<databaseChangeLog><changeSet id="1" author="a"><preConditions onFail="{on_fail}"><tableExists tableName="t"/></preConditions></changeSet></databaseChangeLog>"#);
        assert!(codes(&root("HALT")).is_empty() && codes(&root("WARN")).is_empty());
        assert_eq!(codes(&root("MARK_RAN")), ["invalid-precondition-action"]);
        assert_eq!(codes(&root("CONTINUE")), ["invalid-precondition-action"]);
        for ok in ["HALT", "CONTINUE", "MARK_RAN", "WARN"] {
            assert!(codes(&set(ok)).is_empty(), "{ok}");
        }
        assert_eq!(codes(&set("STOP")), ["invalid-precondition-action"]);
        let yaml = "databaseChangeLog:\n  - preConditions:\n      - onFail: MARK_RAN\n  - changeSet:\n      id: a\n      author: b\n      preConditions:\n        - onFail: MARK_RAN\n";
        assert_eq!(checked(&host, "/project/db/master.yaml", yaml).into_iter().map(|(c, _)| c).collect::<Vec<_>>(), ["invalid-precondition-action"]);
    }

    #[test]
    fn xsd_precondition_children_list_includes_unique_constraint_exists_and_nested_containers() {
        let host = host();
        let names = ["and", "or", "not", "dbms", "runningAs", "changeSetExecuted", "tableExists", "columnExists", "sequenceExists", "foreignKeyConstraintExists", "indexExists", "uniqueConstraintExists", "primaryKeyExists", "viewExists", "tableIsEmpty", "rowCount", "sqlCheck", "changeLogPropertyDefined", "expectedQuotingStrategy", "customPrecondition"];
        for name in names {
            let text = format!(r#"<databaseChangeLog><changeSet id="1" author="a"><preConditions><not><{name}/></not></preConditions></changeSet></databaseChangeLog>"#);
            assert!(checked(&host, "/project/db/master.xml", &text).is_empty(), "{name}");
        }
    }

    #[test]
    fn reports_structural_yaml_rules() {
        let host = host();
        let text = "databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: bob\n      preConditions:\n        dbms:\n          type: h2\n      changes:\n        - sqlFile:\n            path: nope.sql\n            relativeToChangelogFile: true\n";
        let codes: Vec<String> = checked(&host, "/project/db/master.yaml", text).into_iter().map(|(c, _)| c).collect();
        assert_eq!(codes, ["preconditions-shape", "unresolved-sql-file"]);
        let ok = "databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: bob\n      preConditions:\n        - onFail: WARN\n        - dbms:\n            type: h2\n";
        assert!(checked(&host, "/project/db/master.yaml", ok).is_empty());
    }

    #[test]
    fn reports_formatted_sql_comment_syntax() {
        let host = host();
        let text = "--liquibase formatted sql\n--rollback drop table x;\n--changeset bob:1 runOnChange:yes\nselect 1;\n--rollback select 2;\n";
        let codes: Vec<String> = checked(&host, "/project/db/a.sql", text).into_iter().map(|(c, _)| c).collect();
        assert_eq!(codes, ["rollback-outside-changeset", "invalid-changeset-attribute"]);
    }

    #[test]
    fn stays_silent_outside_changelogs() {
        let host = host();
        assert!(checked(&host, "/project/pom.xml", "<project><changeSet/></project>").is_empty());
        assert!(checked(&host, "/project/src/main/resources/application.yml", "spring:\n  liquibase:\n    change-log: x\n").is_empty());
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
        assert_eq!(checked(&host, MASTER, text), [("unresolved-include".into(), "Cannot find directory `nope/`.".into())]);
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
    fn json_changelog_offers_change_keys() {
        let _host = MockHost::install();
        let prefix = r#"{"databaseChangeLog":[{"changeSet":{"id":"a","author":"ann","changes":[{"createTab"#;
        let items = complete_at_end("/project/db/changelog.json", prefix);
        assert!(labels(&items).contains(&"createTable"), "{:?}", labels(&items));
    }

    #[test]
    fn completion_provider_is_declared_for_json() {
        let manifest = include_str!("../plugin.toml");
        let completion = manifest.split("[[contributes.providers]]").find(|block| block.contains("kinds = [\"completion\"]")).unwrap();
        assert!(completion.lines().find(|l| l.starts_with("languages")).unwrap().contains("\"json\""));
    }

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
        assert_eq!(
            item(&items, "changeSet").insert_text,
            "changeSet:\n      id: ${1:id}\n      author: ${2:author}\n      changes:\n        - $0"
        );
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
    fn json_completion_uses_current_parent_and_preserves_existing_values() {
        let _host = MockHost::install();
        let path = "/project/db/a.json";
        let prefix = r#"{"databaseChangeLog":[{"changeSet":{"id":"a","author":"ann","changes":[{"alterSequence":{"seq"#;
        let items = complete_at_end(path, prefix);
        assert!(labels(&items).contains(&"sequenceName"));
        assert_eq!(item(&items, "sequenceName").insert_text, "sequenceName\": $0");
        let text = format!("{prefix}\": \"😀\"}}}}]}}}}]}}");
        let caret = prefix.len();
        let items = Liquibase::default().completion(&doc(path, &text), openidea_plugin::text::position(&text, caret), None);
        assert_eq!(item(&items, "sequenceName").insert_text, "sequenceName");
        assert!(!item(&items, "sequenceName").snippet);
        assert!(complete_at_end(path, r#"{"databaseChangeLog":[{"changeSet":{"comment":"seq"#).is_empty());
        assert!(complete_at_end(path, r#"{"databaseChangeLog":[],"other":{"seq"#).is_empty());
        let xml = "<databaseChangeLog><changeSet id='a' author='ann'><mergeColumns ";
        assert!(labels(&complete_at_end("/project/db/a.xml", xml)).contains(&"finalColumnName"));
    }

    #[test]
    fn formatted_sql_changeset_attributes_keep_values_and_ignore_other_sql() {
        let _host = MockHost::install();
        let path = "/project/db/a.sql";
        let text = "--liquibase formatted sql\n--changeset ann:1 dbms:postgresql run";
        let items = complete_at_end(path, text);
        assert!(labels(&items).contains(&"runOnChange"));
        assert!(!labels(&items).contains(&"dbms"));
        assert_eq!(item(&items, "runOnChange").insert_text, "runOnChange:$0");
        let text = "--liquibase formatted sql\n--changeset ann:1 endDelim:$$";
        let caret = text.len() - 3;
        let items = Liquibase::default().completion(&doc(path, text), openidea_plugin::text::position(text, caret), None);
        assert_eq!(item(&items, "endDelimiter").insert_text, "endDelimiter");
        assert!(!item(&items, "endDelimiter").snippet);
        assert!(complete_at_end(path, "--liquibase formatted sql\n--changeset ann:1 dbms:post").is_empty());
        assert!(complete_at_end(path, "--liquibase formatted sql\nselect 1; --changeset ann:1 run").is_empty());
        assert!(complete_at_end(path, "--liquibase formatted sql\n--changeset ann: ").is_empty());
    }

    #[test]
    fn completion_survives_every_prefix() {
        let _host = MockHost::install();
        let text = format!(
            "{SET}        - createTable:\n            columns:\n              - column:\n                  constraints:\n                    nullable: false\n"
        );
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
        let range = provider_diagnostics(PROPERTY_PATH, PLACEHOLDER_XML)[0].range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (2, 32, 41));
    }

    #[test]
    fn declared_property_is_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let own = PLACEHOLDER_XML.replacen("<changeSet", "<property name=\"prefix\" value=\"t_\"/>\n    <changeSet", 1);
        assert!(property_codes(&host, &own).is_empty());

        host.add_file("db/props.yaml", "databaseChangeLog:\n  - property:\n      name: prefix\n      value: t_\n");
        let included =
            PLACEHOLDER_XML.replacen("<changeSet", "<include file=\"props.yaml\" relativeToChangelogFile=\"true\"/>\n    <changeSet", 1);
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
    fn maven_plugin_expression_variables_make_the_check_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        assert_eq!(property_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
        host.add_file("pom.xml", "<project><build><plugins><plugin><configuration><expressionVariables><prefix>t_</prefix></expressionVariables></configuration></plugin></plugins></build></project>");
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn gradle_changelog_parameters_make_the_check_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        host.add_file(
            "build.gradle.kts",
            "liquibase { activities.register(\"main\") { arguments = mapOf(\"changelogParameters\" to \"prefix=t_\") } }",
        );
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());
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
        let range = provider_diagnostics(MASTER, text)[0].range;
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
    fn check_properties_false_silences_placeholder_warning() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        assert_eq!(property_codes(&host, PLACEHOLDER_XML), ["unresolved-property"]);
        host.set_setting("checkProperties", "false");
        assert!(property_codes(&host, PLACEHOLDER_XML).is_empty());
    }

    #[test]
    fn provider_answers_from_warmed_facts_without_probing_the_project() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let mut liquibase = Liquibase::default();
        let text = "<databaseChangeLog>\n<include file=\"later.xml\" relativeToChangelogFile=\"true\"/>\n<changeSet id=\"${prefix}\" author=\"a\"/>\n</databaseChangeLog>";
        host.add_file("db/master.xml", text);
        liquibase.handle_events(&[openidea_plugin::Event::FileOpened(PROPERTY_PATH.into())], &host.invocation());
        let provider = |liquibase: &mut Liquibase| {
            let codes: Vec<_> = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(liquibase, &doc(PROPERTY_PATH, text))
                .into_iter()
                .filter_map(|d| d.code)
                .collect();
            codes
        };
        assert_eq!(provider(&mut liquibase), ["unresolved-include", "unresolved-property"]);
        host.add_file("db/later.xml", "<databaseChangeLog/>");
        host.add_file("db/props.xml", "<databaseChangeLog><property name=\"prefix\" value=\"t\"/></databaseChangeLog>");
        assert_eq!(provider(&mut liquibase), ["unresolved-include", "unresolved-property"]);
        liquibase.handle_events(&[openidea_plugin::Event::FileSaved(PROPERTY_PATH.into())], &host.invocation());
        assert!(provider(&mut liquibase).is_empty());
    }

    #[test]
    fn files_changed_rewarms_the_facts_of_warmed_changelogs() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let text = "<databaseChangeLog><include file=\"later.xml\" relativeToChangelogFile=\"true\"/></databaseChangeLog>";
        host.add_file("db/master.xml", text);
        let mut liquibase = Liquibase::default();
        liquibase.handle_events(&[openidea_plugin::Event::FileOpened(PROPERTY_PATH.into())], &host.invocation());
        let count = |liquibase: &mut Liquibase| {
            openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(liquibase, &doc(PROPERTY_PATH, text)).len()
        };
        assert_eq!(count(&mut liquibase), 1);
        host.add_file("db/later.xml", "<databaseChangeLog/>");
        let changed = openidea_plugin::FilesChanged { created: vec!["/project/db/later.xml".into()], ..Default::default() };
        liquibase.handle_events(&[openidea_plugin::Event::FilesChanged(changed)], &host.invocation());
        assert_eq!(count(&mut liquibase), 0);
    }

    #[test]
    fn an_event_batch_past_its_deadline_forgets_instead_of_warming() {
        let host = host();
        let text = "<databaseChangeLog><include file=\"later.xml\" relativeToChangelogFile=\"true\"/></databaseChangeLog>";
        host.add_file("db/master.xml", text);
        let mut liquibase = Liquibase::default();
        liquibase.handle_events(&[openidea_plugin::Event::FileOpened(PROPERTY_PATH.into())], &host.invocation());
        assert_eq!(liquibase.live.warmed_paths(), [PROPERTY_PATH]);
        let changed = openidea_plugin::FilesChanged { created: vec!["/project/db/later.xml".into()], ..Default::default() };
        let past = Instant::now().checked_sub(Duration::from_millis(1)).unwrap();
        liquibase.handle_events_until(&[openidea_plugin::Event::FilesChanged(changed), openidea_plugin::Event::FileSaved("/project/db/other.xml".into())], past);
        assert!(liquibase.live.warmed_paths().is_empty(), "unreached changelogs are forgotten, not left stale");
    }

    #[test]
    fn project_listings_stop_at_the_deadline() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let project = project_of(PROPERTY_PATH).unwrap();
        let past = Instant::now().checked_sub(Duration::from_millis(1)).unwrap();
        assert_eq!(project_parameters(&project, "/project", PROPERTY_PATH, past), None);
        let log = changelog::parse(PROPERTY_PATH, "<databaseChangeLog/>").unwrap();
        assert_eq!(project_properties(&project, "/project", PROPERTY_PATH, &log, past), None);
    }

    #[test]
    fn denied_event_subscription_notifies_once() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut liquibase = Liquibase::default();
        liquibase.activate(&host.context("aritti.liquibase")).unwrap();
        let notes = host.notifications();
        assert_eq!(notes.len(), 1);
        assert!(format!("{notes:?}").contains("Allow events for Liquibase Changelogs in Settings"));
    }

    #[test]
    fn missing_include_of_the_open_file_is_flagged_without_events() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut liquibase = Liquibase::default();
        liquibase.activate(&host.context("aritti.liquibase")).unwrap();
        let text = "<databaseChangeLog><include file=\"nope.xml\" relativeToChangelogFile=\"true\"/></databaseChangeLog>";
        let codes: Vec<_> = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut liquibase, &doc(PROPERTY_PATH, text))
            .into_iter()
            .filter_map(|d| d.code)
            .collect();
        assert_eq!(codes, ["unresolved-include"]);
    }

    #[test]
    fn a_file_open_before_the_plugin_started_is_warmed_by_its_first_diagnostics() {
        let _host = host();
        let mut liquibase = Liquibase::default();
        let text = "<databaseChangeLog><include file=\"nope.xml\" relativeToChangelogFile=\"true\"/></databaseChangeLog>";
        let codes: Vec<_> = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut liquibase, &doc(PROPERTY_PATH, text))
            .into_iter()
            .filter_map(|d| d.code)
            .collect();
        assert_eq!(codes, ["unresolved-include"]);
    }

    #[test]
    fn exhausted_warm_budget_leaves_facts_unknown_and_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let mut liquibase = Liquibase::default();
        let project = project_of(PROPERTY_PATH).unwrap();
        let text = "<databaseChangeLog><include file=\"nope.xml\" relativeToChangelogFile=\"true\"/><changeSet id=\"${p}\" author=\"a\"/></databaseChangeLog>";
        liquibase.live.warm(&project, "/project", PROPERTY_PATH, text, Duration::ZERO, None, true);
        let found = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut liquibase, &doc(PROPERTY_PATH, text));
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn fallback_with_many_includes_skips_project_listings_and_stays_quiet() {
        let host = host();
        host.add_file("liquibase.properties", "url=x\n");
        let mut liquibase = Liquibase::default();
        liquibase.live.events_denied = true;
        let includes = (0..300).map(|n| format!("<include file=\"c{n}.xml\" relativeToChangelogFile=\"true\"/>")).collect::<String>();
        let text = format!("<databaseChangeLog>{includes}<changeSet id=\"${{p}}\" author=\"a\"/></databaseChangeLog>");
        let codes: Vec<_> = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut liquibase, &doc(PROPERTY_PATH, &text))
            .into_iter()
            .filter_map(|d| d.code)
            .collect();
        assert!(!codes.contains(&"unresolved-property".to_owned()), "{codes:?}");
        assert!(codes.iter().all(|c| c == "unresolved-include"), "{codes:?}");
    }

    #[test]
    fn plugin_publishes_no_document_indexer() {
        assert!(!include_str!("../plugin.toml").contains("documentIndexers"));
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

    #[test]
    fn completes_include_files_authors_and_xml_names() {
        let host = MockHost::install().with_capabilities(&["project.read", "process:git"]);
        host.add_file("db/master.yaml", "databaseChangeLog:\n  - include:\n      file: \n");
        host.add_file("db/changes/001.yaml", "databaseChangeLog: []\n");
        host.add_file("db/changes/002.sql", "--liquibase formatted sql\n");
        host.add_file("pom.xml", "<project/>");
        host.on_process("git", |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "Ann\n".into(), stderr: String::new() })
        });

        let text = "databaseChangeLog:\n  - include:\n      relativeToChangelogFile: true\n      file: ch";
        let items = complete_at_end("/project/db/master.yaml", text);
        assert_eq!(labels(&items), ["changes/001.yaml", "changes/002.sql"]);
        assert_eq!(item(&items, "changes/001.yaml").replace, Some(spring_text::range(text, text.len() - 2, text.len())));
        let absolute = complete_at_end("/project/db/master.yaml", "databaseChangeLog:\n  - include:\n      file: ");
        assert_eq!(labels(&absolute), ["db/changes/001.yaml", "db/changes/002.sql", "pom.xml"]);

        let xml = "<databaseChangeLog>\n<include file=\"\" relativeToChangelogFile=\"true\"/>\n</databaseChangeLog>";
        let caret = xml.find("\"\"").unwrap() + 1;
        let items = Liquibase::default().completion(&doc("/project/db/master.xml", xml), openidea_plugin::text::position(xml, caret), None);
        assert_eq!(labels(&items), ["changes/001.yaml", "changes/002.sql", "master.yaml"]);

        let author = "databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: ";
        assert_eq!(labels(&complete_at_end("/project/db/a.yaml", author)), ["Ann"]);
        let sql = "--liquibase formatted sql\n--changeset ";
        assert_eq!(labels(&complete_at_end("/project/db/a.sql", sql)), ["Ann"]);
        assert!(!labels(&complete_at_end("/project/db/a.sql", "--liquibase formatted sql\n--changeset ann:")).contains(&"Ann"));
        let xml_author = "<databaseChangeLog>\n<changeSet id=\"1\" author=\"\">\n</changeSet>\n</databaseChangeLog>";
        let caret = xml_author.find("author=\"").unwrap() + 8;
        let items = Liquibase::default().completion(
            &doc("/project/db/a.xml", xml_author),
            openidea_plugin::text::position(xml_author, caret),
            None,
        );
        assert_eq!(labels(&items), ["Ann"]);

        let tag = "<databaseChangeLog>\n<changeSet id=\"1\" author=\"a\">\n<creat";
        let items = complete_at_end("/project/db/a.xml", tag);
        assert!(labels(&items).contains(&"createTable") && labels(&items).contains(&"comment"));
        assert_eq!(item(&items, "createTable").replace, Some(spring_text::range(tag, tag.len() - 5, tag.len())));
        let in_table = "<databaseChangeLog>\n<changeSet id=\"1\" author=\"a\">\n<createTable tableName=\"t\">\n<";
        assert_eq!(labels(&complete_at_end("/project/db/a.xml", in_table)), ["column"]);
        assert!(complete_at_end("/project/db/a.xml", "<databaseChangeLog>\n<").is_empty());

        let attrs = "<databaseChangeLog>\n<changeSet id=\"1\" ru";
        let items = complete_at_end("/project/db/a.xml", attrs);
        assert!(labels(&items).contains(&"runOnChange") && !labels(&items).contains(&"id") && !labels(&items).contains(&"changes"));
        assert_eq!(item(&items, "runOnChange").insert_text, "runOnChange=\"$0\"");
        assert!(complete_at_end("/project/db/a.xml", "<databaseChangeLog>\n<changeSet id=\"1\" author=\"x").is_empty());

        let schema = "<databaseChangeLog xsi:schemaLocation=\"x y\">\n<changeSet id=\"1\">\n<creat";
        assert!(complete_at_end("/project/db/a.xml", schema).is_empty());
    }

    #[test]
    fn author_is_a_setting_or_nothing_without_git() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let author = "databaseChangeLog:\n  - changeSet:\n      author: ";
        assert!(complete_at_end("/project/db/a.yaml", author).is_empty());
        host.set_setting("author", "\"Bo\"");
        assert_eq!(labels(&complete_at_end("/project/db/a.yaml", author)), ["Bo"]);
    }

    const JSON_LOG: &str = "{\"databaseChangeLog\": [\n{\"include\": {\"file\": \"nope.json\"}},\n{\"changeSet\": {\"id\": \"1\", \"author\": \"a\", \"changes\": [{\"frobnicate\": {}}]}},\n{\"changeSet\": {\"id\": \"1\", \"author\": \"a\"}},\n{\"changeSet\": {\"id\": \"2\"}}\n]}";

    #[test]
    fn checks_json_changelogs_and_stays_silent_on_other_json() {
        let host = MockHost::install();
        let codes: Vec<String> = checked(&host, "/project/db/log.json", JSON_LOG).into_iter().map(|(code, _)| code).collect();
        assert_eq!(codes, ["unresolved-include", "unknown-change-type", "duplicate-changeset", "incomplete-changeset"]);
        assert!(checked(&host, "/project/package.json", "{\"name\": \"x\"}").is_empty());
    }

    #[test]
    fn goes_to_json_includes_and_lists_json_changesets() {
        let host = MockHost::install();
        host.add_file("db/master.json", "{\"databaseChangeLog\": [{\"include\": {\"file\": \"db/child.json\"}}]}");
        host.add_file(
            "db/child.json",
            "{\"databaseChangeLog\": [{\"changeSet\": {\"id\": \"7\", \"author\": \"x\", \"changes\": [{\"dropTable\": {}}]}}]}",
        );
        host.add_file("package.json", "{\"name\": \"x\"}");
        let text = "{\"databaseChangeLog\": [{\"include\": {\"file\": \"db/child.json\"}}]}";
        let caret = text.find("db/child").unwrap() + 2;
        let found = Liquibase::default().definition(&doc("/project/db/master.json", text), openidea_plugin::text::position(text, caret));
        assert_eq!(found.iter().map(|l| l.path.as_str()).collect::<Vec<_>>(), ["/project/db/child.json"]);

        let rows = tree_rows(&host, &mut Liquibase::default());
        assert!(rows.iter().any(|(_, label, detail)| label == "7 (x)" && detail.as_deref() == Some("dropTable")), "{rows:?}");
    }

}

#[cfg(test)]
mod custom_group_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use serde_json::{json,Value};
    fn request(plugin: &mut Liquibase, host: &MockHost, mut value: Value) -> Value {
        value["channel"]=json!("ui-kit");value["sequence"]=json!(1);value["project"]=json!("/project");
        plugin.handle("changesets",ViewEvent::Message(value.to_string()),&host.invocation()).unwrap();
        serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap()
    }
    #[test]
    fn one_custom_container_routes_tabs_and_rejects_hidden_tab_mutations_without_cli() {
        let host=MockHost::install().with_capabilities(&["storage","project.read"]);
        let mut plugin=Liquibase::default();
        let first=request(&mut plugin,&host,json!({"op":"render"}));
        assert_eq!(first["model"]["activeView"],"changesets");
        assert_eq!(first["model"]["viewTabs"].as_array().unwrap().len(),2);
        let second=request(&mut plugin,&host,json!({"op":"select-view","targetView":"changesets","target":"offline-diff"}));
        assert_eq!(second["model"]["activeView"],"offline-diff");
        let stale=request(&mut plugin,&host,json!({"op":"event","targetView":"changesets","event":{"kind":"button","key":"choose-sources"}}));
        assert!(stale.get("error").is_some());
        let active=request(&mut plugin,&host,json!({"op":"render"}));
        assert_eq!(active["model"]["activeView"],"offline-diff");
        assert!(host.process_runs().is_empty()&&host.terminals().is_empty()&&host.requests().is_empty());
        assert!(host.view_messages().iter().all(|(view,_)|view=="changesets"));
    }
}

#[cfg(test)]
mod marker_icon_guard {
    #[test]
    fn marker_icons_are_declared_in_the_manifest() {
        let manifest = include_str!("../plugin.toml");
        for icon in [crate::INCLUDE_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")), "{icon}");
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file(), "{icon}");
        }
    }
}
