//! Flyway Migrations for OpenIDEA: Flyway's SQL migrations, read from file
//! names and project configuration (there is no database to ask).
//!
//! - Migration file names are checked against the configured naming scheme.

mod native_runs;
mod native_browser;
mod checks;
mod comparison;
mod console;
mod complete;
mod config;
mod execution;
mod generate;
mod hibernate_schema;
mod info;
mod naming;
mod search;
pub(crate) const PLACEHOLDER_MARKER: &str = "icons/marker-placeholder.svg";
pub(crate) const TABLE_MARKER: &str = "icons/flyway.svg";
/// Markers one gutter button lists at most; the IDE merges a line's same-icon markers into one chooser.
const MAX_LINE_TARGETS: usize = 200;

/// One marker per target on `line`, all with `icon`: one target jumps, several open the chooser.
pub(crate) fn line_markers(line: u32, icon: &str, tooltip: &str, targets: Vec<openidea_plugin::Location>) -> Vec<openidea_plugin::providers::LineMarker> {
    let targets: Vec<_> = targets.into_iter().filter(|target| target.range.start != target.range.end).collect();
    let tooltip = match targets.len() > MAX_LINE_TARGETS {
        true => format!("{tooltip} (first {MAX_LINE_TARGETS} of {})", targets.len()),
        false => tooltip.to_owned(),
    };
    targets
        .into_iter()
        .take(MAX_LINE_TARGETS)
        .map(|target| openidea_plugin::providers::LineMarker { line, icon: Some(icon.into()), tooltip: tooltip.clone(), target: Some(target) })
        .collect()
}

mod navigation;
mod table_navigation;

use std::collections::{BTreeMap, BTreeSet};

use openidea_plugin::host;
use openidea_plugin::providers::{CompletionItem, Document, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Diagnostic, Error, Event, Invocation, NotificationLevel, Plugin, Position, Project, StatusItem, StatusMenuEntry};
use spring_text::config::is_config_file;
use spring_text::index::{glob_matches, project_of};

use config::{Config, is_flyway_config, load, location_glob, migration_files, vendor_prefixes};
use naming::{Kind, Name, next_version, numeric_key, parse, version_key};

const TITLE: &str = "Flyway";
const PLUGIN: &str = "Flyway Migrations";

#[derive(Default)]
pub struct Flyway {
    browser_filter: Option<(String, String)>,
    browser_selection: Option<native_browser::Selection>,
    browser_expansion: spring_text::tree::Expansion,
    navigation: navigation::State,
    table_navigation: table_navigation::State,
    generation: Option<generate::Draft>,
    hibernate_schema: Option<hibernate_schema::Candidate>,
    comparison: comparison::Review,
    /// Placeholder names by project root.
    placeholders: BTreeMap<String, Vec<String>>,
    placeholder_syntax: BTreeMap<String, config::PlaceholderSyntax>,
    /// Paths this instance published problems for.
    flagged: BTreeSet<String>,
    /// Project roots scanned by this instance; problems from before it started are unknown.
    scanned: BTreeSet<String>,
    execution: execution::Execution,
    info_cache: Option<(String, View)>,
    info_scope: Option<execution::Proof>,
    info_at: Option<std::time::Instant>,
    /// Pending migrations in the cached Info result.
    info_pending: Option<usize>,
    targets_root: Option<String>,
    /// Migration file by tree item key, from the last render of the migrations view.
    targets: BTreeMap<String, String>,
    /// Local migration inventory and checks by project root.
    local: BTreeMap<String, Local>,
    local_root: Option<String>,
    /// Migration file by local row key (`l:<n>`), from the last local render.
    local_targets: BTreeMap<String, String>,
    /// Whether migration files carry notes from the accepted Info result.
    info_noted: bool,
    /// Table cells by migrations row key (`l:` local, `m:` Info), for the packaged view's columns.
    row_cells: BTreeMap<String, Vec<String>>,
    /// Whether the console rules are set: from the first Flyway project seen on.
    console_rules: bool,
}

#[derive(Clone, PartialEq)]
struct Local {
    naming: naming::Naming,
    locations: Vec<String>,
    files: Vec<String>,
    results: BTreeMap<String, Vec<Diagnostic>>,
}

fn configured(project: &Project) -> bool {
    config::CONFIG_FILES.iter().any(|file| project.exists(file).unwrap_or(false))
}

/// Project-tree note set for Info states on migration files.
const INFO_NOTES: &str = "flyway-info";
/// Window status item showing the best-known target database.
const STATUS_ITEM: &str = "flyway-target";

/// The configured runner and its arguments for a goal; `json` asks for machine-readable output.
fn runner_args(goal: &str, json: bool) -> (&'static str, Vec<String>) {
    let maven = host::setting_string("runner").as_deref() == Some("mvn");
    let mut args = vec![if maven { format!("flyway:{goal}") } else { goal.to_owned() }];
    if json {
        args.push(if maven { "-Dflyway.outputType=json" } else { "-outputType=json" }.to_owned());
    }
    // ponytail: no shell quoting; quote-aware split when someone needs spaces in a value
    args.extend(host::setting_string("arguments").unwrap_or_default().split_whitespace().map(str::to_owned));
    (if maven { "mvn" } else { "flyway" }, args)
}

fn migration_description(text: &str) -> String {
    let words: Vec<String> = text.split(|c: char| !c.is_ascii_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_owned).collect();
    words.join("_")
}

fn root_relative<'a>(path: &'a str, root: &str) -> &'a str {
    path.strip_prefix(root).map_or(path, |rest| rest.trim_start_matches('/'))
}

// No model/identity data in the broadcast; each frame renders its own scoped cache.
fn refresh_migrations() {
    ui_kit::web::refresh_view("migrations", include_str!("ui_config.json"));
}
fn notify_runner_denied(goal: &str) {
    let (program, _) = runner_args(goal, false);
    host::notify(NotificationLevel::Warning, TITLE, &ui_kit::denied::message("Running Flyway", &format!("process:{program}"), PLUGIN));
}
/// Shows the confirmation's redacted target hint in the window's status bar while the
/// project has a Flyway config file; clicking opens a menu of the Flyway commands
/// (the panel on hosts without status bar menus).
fn update_status(project: &Project, pending: Option<usize>) {
    let item = open_root(project).and_then(|_| execution::status_target(project)).map(|target| match target {
        Some((short, hint)) => StatusItem {
            text: format!("Flyway: {short}"),
            tooltip: Some(format!("Target database: {hint}. Click for Flyway commands.")),
            command: Some("show-migrations".into()),
        },
        None => StatusItem {
            text: "Flyway".into(),
            tooltip: Some("Target database unknown from the project's Flyway config files. Click for Flyway commands.".into()),
            command: Some("show-migrations".into()),
        },
    });
    let Some(item) = item else {
        let _ = host::remove_window_status_item(STATUS_ITEM);
        return;
    };
    if host::set_window_status_item_menu(STATUS_ITEM, &item, &status_menu(pending)).is_err() {
        let item = StatusItem { tooltip: item.tooltip.map(|tip| tip.replace("Click for Flyway commands.", "Click to open the Flyway panel.")), ..item };
        let _ = host::set_window_status_item(STATUS_ITEM, &item);
    }
}
/// The status bar menu: the pending count from the last Info, when there is one, then the commands.
fn status_menu(pending: Option<usize>) -> Vec<StatusMenuEntry> {
    let mut menu = Vec::new();
    if let Some(pending) = pending {
        menu.push(StatusMenuEntry::Header(match pending {
            0 => "No pending migrations at the last Info".into(),
            1 => "1 pending migration at the last Info".into(),
            n => format!("{n} pending migrations at the last Info"),
        }));
    }
    menu.extend([
        StatusMenuEntry::action("Show Migrations", "show-migrations"),
        StatusMenuEntry::action("New Migration…", "new-migration"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Run Info…", "info"),
        StatusMenuEntry::action("Validate", "validate"),
        StatusMenuEntry::action("Migrate", "migrate").with_description("Asks first"),
        StatusMenuEntry::action("Repair", "repair").with_description("Asks first"),
    ]);
    menu
}
fn open_root(project: &Project) -> Option<String> {
    let root = project.root().ok()?;
    (root.starts_with('/') && !root.chars().any(char::is_control)
        && !root.split('/').any(|part| matches!(part,"."|".."))
        && host::projects().ok()?.iter().any(|project|project.root().ok().as_deref()==Some(root.as_str())))
        .then_some(root)
}
impl Flyway {
    /// Pending migrations in this project's current Info result.
    fn pending(&self, project: &Project) -> Option<usize> {
        let root = project.root().ok()?;
        let current = self.info_cache.as_ref().is_some_and(|(owner, _)| owner == &root)
            && self.info_scope.as_ref().is_some_and(|scope| scope.settings_current(&root));
        self.info_pending.filter(|_| current)
    }
    fn drop_info(&mut self) {
        self.info_cache = None;
        self.info_pending = None;
        self.info_scope = None;
        self.targets.clear();
        self.targets_root = None;
        self.clear_info_notes();
    }
    fn clear_info_notes(&mut self) {
        if std::mem::take(&mut self.info_noted) {
            let _ = host::clear_file_decorations(INFO_NOTES);
        }
    }
    fn show_info_notes(&mut self, notes: &BTreeMap<String, String>) {
        // ponytail: one shared set, matching the single cached Info result.
        self.clear_info_notes();
        self.info_noted = !notes.is_empty();
        for (path, note) in notes.iter().take(2_000) {
            if host::set_file_note(path, INFO_NOTES, note, Some("State from the last Flyway Info run")).is_err() {
                break;
            }
        }
    }
    fn scan(&mut self, project: &Project) -> (Config, Vec<String>, BTreeMap<String, Vec<Diagnostic>>) {
        let config = load(project);
        let files = migration_files(project, &config);
        self.analyse(project, config, files)
    }

    fn analyse(&mut self, project: &Project, config: Config, files: Vec<String>) -> (Config, Vec<String>, BTreeMap<String, Vec<Diagnostic>>) {
        self.navigation.warm(project, &config);
        self.table_navigation.warm(project, &files);
        let root = project.root().unwrap_or_default();
        self.placeholders.insert(root.clone(), config.placeholders.clone());
        self.placeholder_syntax.insert(root.clone(), config.placeholder_syntax.clone());
        let results = self.record_local(&root, &config, &files);
        (config, files, results)
    }

    fn record_local(&mut self, root: &str, config: &Config, files: &[String]) -> BTreeMap<String, Vec<Diagnostic>> {
        let vendors = vendor_prefixes(&config.locations, root);
        let results = checks::check(root, files, &config.naming, &vendors);
        let local = Local { naming: config.naming.clone(), locations: config.locations.clone(), files: files.to_vec(), results: results.clone() };
        self.local.insert(root.to_owned(), local);
        results
    }

    /// A Flyway project has a Flyway config file or migration files; the inventory is computed once, then kept current by events.
    pub(crate) fn flyway_project(&mut self, project: &Project) -> bool {
        if configured(project) {
            self.uses_flyway();
            return true;
        }
        let Ok(root) = project.root() else { return false };
        if !self.local.contains_key(&root) {
            let config = load(project);
            let files = migration_files(project, &config);
            self.record_local(&root, &config, &files);
        }
        let flyway = self.local.get(&root).is_some_and(|local| !local.files.is_empty());
        if flyway {
            self.uses_flyway();
        }
        flyway
    }

    /// Console rules go on once a project is seen to use Flyway and stay until the plugin stops.
    pub(crate) fn uses_flyway(&mut self) {
        if !std::mem::replace(&mut self.console_rules, true) {
            console::apply();
        }
    }

    /// Drops a cached "no migrations" result when a config file or a file in its locations changes.
    fn forget_empty_inventory(&mut self, path: &str) {
        let config = is_flyway_config(path) || is_config_file(path);
        self.local.retain(|root, local| {
            let Some(relative) = path.strip_prefix(&format!("{}/", root.trim_end_matches('/'))) else { return true };
            let in_location = || local.locations.iter().filter_map(|location| location_glob(location, root)).any(|glob| glob_matches(&glob, relative));
            !(local.files.is_empty() && (config || in_location()))
        });
    }

    /// The file behind a migrations tree row, still owned by `root`.
    fn target(&self, root: &str, key: &str) -> Option<String> {
        let path = if key.starts_with("l:") {
            self.local_targets.get(key).filter(|_| self.local_root.as_deref() == Some(root))
        } else {
            self.targets.get(key).filter(|_| {
                self.info_scope.as_ref().is_some_and(|scope| scope.settings_current(root)) && self.targets_root.as_deref() == Some(root)
            })
        }?;
        (host::project_of(path).and_then(|project| project.root().ok()).as_deref() == Some(root)).then(|| path.clone())
    }

    /// Render may poll the owned managed child (never start one) so a finished Info lands without another click.
    fn poll_owned(&mut self, project: &Project, root: &str) {
        for _ in 0..4 {
            if !self.execution.running(root) {
                return;
            }
            match self.execution.poll(root) {
                Ok(Some(finished)) => {
                    let _ = self.accept_finished(project, finished);
                    return;
                }
                Ok(None) => {}
                Err(_) => return,
            }
        }
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
                &format!(
                    "No Flyway migrations found in {locations}. Set the locations in Settings › Plugins › Flyway Migrations if they are elsewhere."
                ),
            );
            return;
        }

        let problems: usize = results.values().map(Vec::len).sum();
        host::notify(NotificationLevel::Info, TITLE, &format!("Checked {} migrations: {problems} problems.", results.len()));
    }

    fn next_migration(&mut self, project: &Project) -> Result<(String, String, String), Error> {
        let (config, files, _) = self.scan(project);
        let naming = &config.naming;
        let root = project.root()?;

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
        Ok((next, directory, suffix.to_owned()))
    }

    fn copy_next_version(&mut self, project: &Project) -> Result<(), Error> {
        let (next, directory, suffix) = self.next_migration(project)?;
        let suffix = suffix.as_str();

        match host::copy_to_clipboard(&next) {
            Ok(()) => host::notify(
                NotificationLevel::Info,
                TITLE,
                &format!("{next} copied to the clipboard: name the new migration {next}<description>{suffix} in {directory}."),
            ),
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                TITLE,
                &format!(
                    "Next migration: {next}<description>{suffix} in {directory}. {}",
                    ui_kit::denied::message("Copying it", "clipboard", PLUGIN)
                ),
            ),
            Err(e) => return Err(e),
        }
        Ok(())
    }

    fn run_flyway(&mut self, goal: &str, ctx: &Invocation) -> Result<Option<String>, Error> {
        match self.execution.start(ctx, goal, false, false) {
            Ok(()) | Err(Error::Cancelled) => {}
            Err(Error::Denied(_)) => notify_runner_denied(goal),
            Err(error) => return Err(error),
        }
        Ok(None)
    }
    fn accept_finished(&mut self, project: &Project, finished: execution::Finished) -> Result<(), Error> {
        let root = open_root(project).ok_or(Error::Stale)?;
        if finished.root != root {
            return Err(Error::Stale);
        }
        if finished.goal == "info" {
            // Accepting reads the bounded source proof (Status or a render poll); it never runs a command.
            if execution::info_scope(&project).as_ref().ok() != Some(&finished.proof) {
                self.drop_info();
                refresh_migrations();
                return Err(Error::Stale);
            }
            self.clear_info_notes();
            self.info_scope = Some(finished.proof.clone());
            self.info_at = Some(std::time::Instant::now());
            self.targets.clear();
            self.targets_root = Some(root.clone());
            self.info_pending = None;
            let view = if let Some(error) = finished.info_error {
                View::message(error).button(Button::new("refresh", "Run Info…").icon("refresh"))
            } else {
                self.accept_info(&project, finished.output, &finished.redactor)
            };
            self.info_cache = Some((root, view));
            update_status(project, self.pending(project));
            refresh_migrations();
        }
        Ok(())
    }
    fn poll_execution(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let project = ctx.project().ok_or(Error::Stale)?;
        let root = project.root()?;
        if let Some(finished) = self.execution.poll(&root)? { self.accept_finished(&project, finished)?; }
        ui_kit::web::refresh_view(execution::VIEW, include_str!("ui_config.json"));
        Ok(())
    }
}

impl Plugin for Flyway {
    fn deactivate(&mut self) {self.comparison.cancel_owned();}

    fn activate(&mut self, _ctx: &openidea_plugin::Context) -> Result<(), Error> {
        let _ = host::subscribe_paths("files-changed", &["**/*.sql", "**/*.conf", "**/*.toml", "**/application*", "**/*.class", "**/*.jar", "**/*.java", "**/*.kt", "**/hibernate-schema.json"]);
        Ok(())
    }
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        if let Some(project) = ctx.project() {
            update_status(&project, self.pending(&project));
        }
        match id {
            "show-migrations" => {
                ui_kit::web::reveal_view("migrations", include_str!("ui_config.json"))?;
                Ok(None)
            }
            "hibernate-schema" => self.generate_hibernate_schema(ctx),
            "review-comparison" => {
                self.comparison.begin(ctx)?;
                ui_kit::web::reveal_view(comparison::VIEW, include_str!("ui_config.json"))?;
                comparison::refresh();
                Ok(None)
            }
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
            "new-migration" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                self.begin_generation(&project)?;
                ui_kit::web::reveal_view("migrations", include_str!("ui_config.json"))?;
                refresh_migrations();
                Ok(None)
            }
            "info" | "validate" | "migrate" | "repair" | "clean" => self.run_flyway(id, ctx),
            "status" => {
                self.poll_execution(ctx)?;
                ui_kit::web::reveal_view(execution::VIEW, include_str!("ui_config.json"))?;
                Ok(None)
            }
            "cancel" => {
                let root = ctx.project().ok_or(Error::Stale)?.root()?;
                self.execution.cancel(&root)?;
                ui_kit::web::refresh_view(execution::VIEW, include_str!("ui_config.json"));
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.execution.closed_roots();
        self.comparison.invalidate(events);
        if self.hibernate_schema.as_ref().is_some_and(|candidate| events.iter().any(|event| match event {
            Event::FileSaved(path) => candidate.affected(path),
            Event::FilesChanged(batch) => batch.created.iter().chain(&batch.modified).chain(&batch.removed).any(|path| candidate.affected(path)), _ => false,
        })) { self.hibernate_schema = None; }
        let affected = self.info_scope.as_ref().is_some_and(|proof| events.iter().any(|event| match event {
            Event::FileSaved(path) => proof.affected(path) || self.targets.values().any(|source|source==path),
            Event::FilesChanged(batch) => batch.created.iter().chain(&batch.modified).chain(&batch.removed).any(|path|proof.affected(path)
                || self.targets.values().any(|source|source==path || !path.is_empty() && source.starts_with(&format!("{}/",path.trim_end_matches('/'))))),
            _ => false,
        }));
        if affected {self.drop_info();refresh_migrations();}
        for event in events {
            if let Event::FileSaved(path) = event {
                self.table_navigation.invalidate(path);
                self.forget_empty_inventory(path);
            }
            if let Event::FilesChanged(batch) = event {
                for path in batch.created.iter().chain(&batch.modified).chain(&batch.removed) {
                    self.navigation.invalidate(path);
                    self.table_navigation.invalidate(path);
                    self.forget_empty_inventory(path);
                }
            }
        }
        // ponytail: custom sqlMigrationSuffixes are checked by Flyway: Check Migrations only
        let qualifying = events.iter().find_map(|event| match event {
            Event::FileOpened(path) | Event::FileSaved(path)
                if path.ends_with(".sql") || is_flyway_config(path) || is_config_file(path) =>
            {
                Some(path)
            }
            Event::FilesChanged(batch) => batch
                .created
                .iter()
                .chain(&batch.modified)
                .find(|path| path.ends_with(".sql") || is_flyway_config(path) || is_config_file(path)),
            _ => None,
        });
        let Some(path) = qualifying else { return };
        if let Some(project) = _ctx.project() {
            update_status(&project, self.pending(&project));
        }
        let Some(project) = project_of(path) else {
            return;
        };

        let root = project.root().unwrap_or_default();
        let prefix = format!("{}/", root.trim_end_matches('/'));
        let flyway = configured(&project) || self.flagged.iter().any(|path| path.starts_with(&prefix));
        let before = self.local.get(&root).cloned();
        // A project already found without migrations is not searched again until config or a location changes.
        if !flyway && before.as_ref().is_some_and(|local| local.files.is_empty()) {
            return;
        }
        let config = load(&project);
        let files = migration_files(&project, &config);
        if !flyway && files.is_empty() {
            self.record_local(&root, &config, &files);
            return;
        }

        self.uses_flyway();
        let (_, _, results) = self.analyse(&project, config, files);
        self.publish(&root, &results, false);
        if self.local.get(&root) != before.as_ref() {
            refresh_migrations();
        }
    }
}

impl Providers for Flyway {
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<openidea_plugin::Location> {
        self.source_definition(doc, at)
    }
    fn line_markers(&mut self, doc: &Document) -> Vec<openidea_plugin::providers::LineMarker> {
        self.source_markers(doc)
    }
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(at) = offset(&doc.text, at) else {
            return Vec::new();
        };
        match doc.language.as_str() {
            "sql" => {
                let project = self
                    .placeholders
                    .iter()
                    .filter(|(root, _)| doc.path.starts_with(&format!("{}/", root.trim_end_matches('/'))))
                    .max_by_key(|(root, _)| root.len())
                    .map(|(root, names)| (names.as_slice(), self.placeholder_syntax.get(root)));
                let default = config::PlaceholderSyntax::default();
                let (names, syntax) = project.map(|(names, syntax)| (names, syntax.unwrap_or(&default))).unwrap_or((&[], &default));
                complete::configured_placeholders(&doc.text, at, names, syntax)
            }
            "ini" if is_flyway_config(&doc.path) => complete::conf_keys(&doc.text, at),
            "toml" if is_flyway_config(&doc.path) => complete::toml_keys(&doc.text, at),
            _ => Vec::new(),
        }
    }
}

impl Flyway {
    fn migration_file(&self, project: &Project, files: &[String], migration: &info::Migration, naming: &naming::Naming) -> Option<String> {
        let description = migration.description.trim();
        if description.starts_with("<<") && description.ends_with(">>") {
            return None;
        }

        let normalized = |text: &str| text.replace('_', " ").to_lowercase();
        let same_description = |description: &str| normalized(description) == normalized(&migration.description);
        let found = files
            .iter()
            .filter_map(|path| {
                let file_name = path.rsplit('/').next().unwrap_or(path);
                let Name::Migration { kind, version, description } = parse(file_name, naming) else {
                    return None;
                };
                if migration.kind == Kind::Repeatable {
                    return (kind == Kind::Repeatable && same_description(&description)).then_some((path, (false, false)));
                }
                let compatible = kind == migration.kind || (migration.kind == Kind::Baseline && kind == Kind::Versioned);
                (compatible && version_key(&version) == version_key(&migration.version))
                    .then(|| (path, (kind != migration.kind, !same_description(&description))))
            })
            .min_by_key(|(_, rank)| *rank)
            .map(|(path, _)| path);
        if let Some(path) = found {
            return Some(path.clone());
        }

        let root = project.root().ok()?;
        let path = migration.filepath.as_deref()?;
        let path = if path.starts_with('/') { path.to_owned() } else { format!("{}/{path}", root.trim_end_matches('/')) };
        if path.split('/').any(|part| part == "." || part == "..")
            || host::project_of(&path).and_then(|project| project.root().ok()).as_deref() != Some(root.as_str())
        {
            return None;
        }
        let Name::Migration { kind, version, description } = parse(path.rsplit('/').next()?, naming) else {
            return None;
        };
        let compatible = kind == migration.kind || migration.kind == Kind::Baseline && kind == Kind::Versioned;
        if !compatible
            || if kind == Kind::Repeatable {
                !same_description(&description)
            } else {
                version_key(&version) != version_key(&migration.version)
            }
        {
            return None;
        }
        project.read_file(&path).ok().map(|_| path)
    }

    fn migrations_view(&mut self, project: &Project) -> View {
        let Some(root) = open_root(project) else {return View::message("This project is not open in this window. Reopen it to see its Flyway migrations.");};
        if self.info_cache.as_ref().is_some_and(|(owner, _)| owner == &root) && !self.info_scope.as_ref().is_some_and(|scope| scope.settings_current(&root)) {
            self.clear_info_notes();
        }
        let shown = match self.info_cache.as_ref().filter(|(owner, _)| owner == &root && self.info_scope.as_ref().is_some_and(|scope|scope.settings_current(&root))) {
            Some((_, cached)) => {
                let mut shown = cached.clone();
                shown.status = Some(format!("Flyway Info from {}s ago · {}", self.info_at.map(|at|at.elapsed().as_secs()).unwrap_or(0), shown.status.as_deref().unwrap_or("Run Info… to update")));
                shown
            }
            None => self.local_view(project, &root),
        };
        let mut shown = shown;
        if let Some(status) = self.execution.migration_status(&root) {
            shown.status = Some(format!("{} · Command status: {status}",shown.status.as_deref().unwrap_or("No Info result yet")));
        }
        shown.button(Button::new("execution-output", "Output").icon("terminal"))
    }
    /// Local migrations from the last scan (version, description, kind, problems); Info replaces it once captured.
    fn local_view(&mut self, project: &Project, root: &str) -> View {
        if !self.local.contains_key(root) && self.flyway_project(project) {
            self.scan(project);
        }
        let refresh = Button::new("refresh", "Run Info…").icon("refresh");
        let Some(local) = self.local.get(root) else { return View::message("Could not list local migrations.").button(refresh) };
        let mut rows = Vec::new();
        for path in &local.files {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            let (rank, order, label, kind_name, version, description) = match parse(file_name, &local.naming) {
                Name::Migration { kind, version, description } => {
                    let (prefix, kind_name) = match kind {
                        Kind::Versioned => (&local.naming.versioned, "Versioned"),
                        Kind::Undo => (&local.naming.undo, "Undo"),
                        Kind::Baseline => (&local.naming.baseline, "Baseline"),
                        Kind::Repeatable => (&local.naming.repeatable, "Repeatable"),
                    };
                    let text = format!("{prefix}{version} {}", description.replace('_', " ")).trim_end().to_owned();
                    (usize::from(kind == Kind::Repeatable), numeric_key(&version), text, kind_name, version, description.replace('_', " "))
                }
                Name::Invalid(_) => (2, Vec::new(), file_name.to_owned(), "Invalid", String::new(), file_name.to_owned()),
                Name::Callback | Name::NotMigration => continue,
            };
            let detail = match local.results.get(path).and_then(|problems| problems.first()) {
                Some(problem) => format!("Problems · {}", problem.message),
                None => kind_name.to_owned(),
            };
            let problem = local.results.get(path).and_then(|problems| problems.first()).map_or(String::new(), |problem| problem.message.clone());
            rows.push(((rank, order, path.clone()), label, detail, vec![version, description, kind_name.to_owned(), problem]));
        }
        if rows.is_empty() {
            return View::message(&format!(
                "No local Flyway migrations in {}. Create one with New…, or press Run Info… to read the database's migration history.",
                local.locations.join(", ")
            ))
            .button(refresh);
        }
        rows.sort();
        let problems = rows.iter().filter(|(_, _, detail, _)| detail.starts_with("Problems · ")).count();
        let mut status = format!("{} local migrations, {problems} with problems", rows.len());
        if let Some((indexed, total)) = self.table_navigation.coverage(root).filter(|(indexed, total)| indexed < total) {
            status.push_str(&format!(" · table navigation covers {indexed} of {total} migrations"));
        }
        status.push_str(" · Run Info… adds the database state");
        self.local_targets.clear();
        self.local_root = Some(root.to_owned());
        self.row_cells.retain(|key, _| !key.starts_with("l:"));
        let mut items = Vec::new();
        for (index, ((_, _, path), label, detail, cells)) in rows.into_iter().enumerate() {
            let key = format!("l:{index}");
            self.row_cells.insert(key.clone(), cells);
            let icon = if detail.starts_with("Problems · ") { "warning" } else { "file" };
            items.push(TreeItem::new(&key, &label).detail(&detail).icon(icon));
            self.local_targets.insert(key, path);
        }
        View::new(Body::Tree(items)).button(refresh).status(&status)
    }

    fn accept_info(&mut self, project: &Project, output: openidea_plugin::ProcessOutput, redactor: &execution::Redactor) -> View {
        let refresh = Button::new("refresh", "Run Info…").icon("refresh");
        if output.status != Some(0) {
            let reason = output.stderr.trim();
            let reason = if reason.is_empty() { output.stdout.trim() } else { reason };
            let reason = reason.lines().last().unwrap_or_default();
            let code = output.status.map_or("killed".to_owned(), |code| format!("exit {code}"));
            return View::message(&format!("Flyway info failed ({code}): {}", redactor.literals(reason))).button(refresh);
        }

        let migrations = match info::parse(&output.stdout) {
            Ok(migrations) => migrations,
            Err(message) => return View::message(&message).button(refresh),
        };
        if migrations.is_empty() {
            return View::message("Flyway reports no migrations. Create one with Flyway: New Migration…, then Run Info… again.").button(refresh);
        }

        let config = load(project);
        let files = migration_files(project, &config);
        self.targets.clear();
        self.row_cells.retain(|key, _| !key.starts_with("m:"));
        let mut items = Vec::new();
        let mut notes = BTreeMap::new();
        for (index, migration) in migrations.iter().enumerate() {
            let key = format!("m:{index}");
            if let Some(path) = self.migration_file(project, &files, migration, &config.naming) {
                // Info lists history in order, so a file's latest row wins.
                match migration.note() {
                    Some(note) => notes.insert(path.clone(), note),
                    None => notes.remove(&path),
                };
                self.targets.insert(key.clone(), path);
            }
            items.push(TreeItem::new(&key, &redactor.label(&migration.label)).detail(migration.state).icon(migration.icon));
            let kind = match migration.kind { Kind::Versioned => "Versioned", Kind::Undo => "Undo", Kind::Baseline => "Baseline", Kind::Repeatable => "Repeatable" };
            self.row_cells.insert(key, vec![redactor.label(&migration.version), redactor.label(&migration.description), kind.into(), migration.state.into(), migration.installed_on.clone().unwrap_or_default()]);
        }

        self.show_info_notes(&notes);
        let pending = migrations.iter().filter(|m| m.state == "Pending").count();
        self.info_pending = Some(pending);
        View::new(Body::Tree(items)).button(refresh).status(&format!("{} migrations, {pending} pending", migrations.len()))
    }
}

impl Views for Flyway {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            if view == comparison::VIEW {
                return self.comparison.view(ctx);
            }
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to see its Flyway migrations."));
            };
            let Some(root) = open_root(&project) else {return Ok(View::message("This project is not open in this window. Reopen it to see its Flyway migrations."));};
            self.poll_owned(&project, &root);
            if view == execution::VIEW {
                return Ok(self.execution.view(&root));
            }
            if let Some(view) = self.generation_view(&project)? {
                return Ok(view);
            }
            let shown = self.migrations_view(&project);
            Ok(self.native_browser_view(&project, shown))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, _ctx, include_str!("ui_config.json"))? { return Ok(()); }
        if view == comparison::VIEW {
            return self.comparison.handle(event, _ctx);
        }
        if view == "migrations" && self.native_browser_event(&event, _ctx)? {
            return Ok(());
        }

        if self.generation_event(&event, _ctx)? {
            return Ok(());
        }
        match event {
            ViewEvent::Button(button) if button == "refresh" => {
                match self.execution.start(_ctx, "info", true, true) {
                    Ok(()) => {
                        self.poll_execution(_ctx)?;
                    }
                    Err(Error::Cancelled) => {}
                    Err(Error::Denied(_)) => notify_runner_denied("info"),
                    Err(error) => return Err(error),
                }
                ui_kit::web::refresh_view(view, include_str!("ui_config.json"));
            }
            ViewEvent::Button(button) if button == "execution-output" => ui_kit::web::reveal_view(execution::VIEW, include_str!("ui_config.json"))?,
            ViewEvent::Button(button) if button == "execution-status" => self.poll_execution(_ctx)?,
            ViewEvent::Button(button) if button == "execution-cancel" => {
                let root = _ctx.project().ok_or(Error::Stale)?.root()?;
                self.execution.cancel(&root)?;
                ui_kit::web::refresh_view(execution::VIEW, include_str!("ui_config.json"));
            }
            ViewEvent::Activate(key) => {
                if let Some(path) = _ctx.project().and_then(|project| project.root().ok()).and_then(|root| self.target(&root, &key)) {
                    if let Err(e) = host::open_location(&path, 0, 0) {
                        host::notify(NotificationLevel::Warning, TITLE, &format!("Could not open {path}: {e}"));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

// API 1.3 registers the optional exports on the same component.
openidea_plugin::register_plugin!(Flyway, providers, views, file_actions, run_configurations, search);

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
        assert_eq!(
            diagnostics[&format!("{DIR}/V1__init.sql")][0].message,
            "Version 1 is also used by src/main/resources/db/migration/V1_0__again.sql."
        );
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
    fn non_flyway_project_event_does_no_scan() {
        let host = MockHost::install();
        host.add_file("scripts/seed.sql", "CREATE TABLE seed(id int);");
        host.add_file("src/main/resources/application.properties", "server.port=8080\n");
        let mut flyway = Flyway::default();
        flyway.handle_events(&[Event::FileOpened("/project/scripts/seed.sql".into())], &host.invocation());
        flyway.handle_events(&[Event::FileSaved("/project/src/main/resources/application.properties".into())], &host.invocation());
        assert!(flyway.scanned.is_empty() && flyway.placeholders.is_empty());
        assert!(flyway.table_navigation.coverage("/project").is_none());
        assert!(host.diagnostics().is_empty());
        host.add_file("src/main/resources/db/custom/V1__a.sql", "CREATE TABLE a(id int);");
        host.add_file("src/main/resources/application.properties", "spring.flyway.locations=classpath:db/custom\n");
        flyway.handle_events(&[Event::FileOpened("/project/scripts/seed.sql".into())], &host.invocation());
        assert!(flyway.local["/project"].files.is_empty(), "the cached empty result skips the migration search");
        flyway.handle_events(&[Event::FileSaved("/project/src/main/resources/application.properties".into())], &host.invocation());
        assert!(flyway.scanned.contains("/project"), "a config file event drops the cached empty result");
        let mut flyway = Flyway::default();
        host.add_file("src/main/resources/application.properties", "server.port=8080\n");
        flyway.handle_events(&[Event::FileOpened("/project/scripts/seed.sql".into())], &host.invocation());
        host.add_file("src/main/resources/db/migration/V1__init.sql", "CREATE TABLE b(id int);");
        flyway.handle_events(&[Event::FileOpened("/project/scripts/seed.sql".into())], &host.invocation());
        assert!(flyway.scanned.is_empty());
        let created = openidea_plugin::FilesChanged { created: vec!["/project/src/main/resources/db/migration/V1__init.sql".into()], ..Default::default() };
        flyway.handle_events(&[Event::FilesChanged(created)], &host.invocation());
        assert!(flyway.scanned.contains("/project"), "a new file in a migration location drops the cached empty result");
    }

    #[test]
    fn view_lists_local_migrations_without_info() {
        let host = host();
        host.add_file("src/main/resources/db/migration/V3__function.sql", "CREATE FUNCTION f() RETURNS int AS $$ select 1 $$;");
        host.on_process("flyway", |_, _| panic!("rendering never runs Flyway"));
        let mut flyway = Flyway::default();
        let view = flyway.render("migrations", None, &host.invocation()).unwrap();
        let status = view.status.clone().unwrap();
        assert!(status.contains("4 local migrations, 3 with problems"), "{status}");
        assert!(status.contains("table navigation covers 3 of 4 migrations"), "{status}");
        let Body::Tree(items) = view.body else { panic!("local migrations tree") };
        let row = |label: &str| items.iter().find(|item| item.key.starts_with("l:") && item.label == label).unwrap_or_else(|| panic!("{label}: {items:?}"));
        assert_eq!(row("V1 init").detail.as_deref(), Some("Problems · Version 1 is also used by src/main/resources/db/migration/V1_0__again.sql."));
        assert!(row("V1_0 again").detail.as_deref().unwrap().starts_with("Problems · "));
        assert!(row("V2_bad.sql").detail.as_deref().unwrap().starts_with("Problems · "));
        assert_eq!(row("V3 function").detail.as_deref(), Some("Versioned"));
        assert_eq!(row("V3 function").parent.as_deref(), Some("state:Versioned"));
        assert_eq!(row("V1 init").parent.as_deref(), Some("state:Problems"));
        let key = row("V3 function").key.clone();
        flyway.handle("migrations", ViewEvent::Activate(key.clone()), &host.invocation()).unwrap();
        flyway.handle("migrations", ViewEvent::Select(key), &host.invocation()).unwrap();
        flyway.handle("migrations", ViewEvent::Button("browser-open".into()), &host.invocation()).unwrap();
        let opened: Vec<String> = host.opened_locations().into_iter().map(|location| location.0).collect();
        assert_eq!(opened, [format!("{DIR}/V3__function.sql"), format!("{DIR}/V3__function.sql")]);
        assert!(host.process_runs().is_empty());
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
            "Next migration: V8__<description>.sql in src/main/resources/db/migration. Copying it needs the clipboard permission. Allow it in Settings › Plugins › Flyway Migrations."
        );
    }

    #[test]
    fn runs_flyway_goals_in_a_terminal() {
        let host = host();
        host.set_setting("arguments", "\"-environment=dev\"");
        host.answer_next_prompt(Some(&["yes"]));
        Flyway::default().run_command("migrate", &host.invocation(), None).unwrap();
        host.set_setting("runner", "\"mvn\"");
        host.answer_next_prompt(Some(&["yes"]));
        Flyway::default().run_command("info", &host.invocation(), None).unwrap();

        let runs: Vec<(String, String, Vec<String>)> = host.terminals().into_iter().map(|t| (t.title, t.program, t.args)).collect();
        assert_eq!(
            runs,
            [
                ("Flyway migrate".to_owned(), "flyway".to_owned(), vec!["migrate".to_owned(), "-environment=dev".to_owned()]),
                ("Flyway info".to_owned(), "mvn".to_owned(), vec!["flyway:info".to_owned(), "-environment=dev".to_owned()]),
            ]
        );
    }

    #[test]
    fn clean_runs_only_when_confirmed() {
        let host = host();
        let mut flyway = Flyway::default();

        host.answer_next_prompt(None);
        flyway.run_command("clean", &host.invocation(), None).unwrap();
        assert!(host.terminals().is_empty());

        host.answer_next_prompt(Some(&[]));
        host.answer_next_prompt(Some(&["clean"]));
        flyway.run_command("clean", &host.invocation(), None).unwrap();
        assert_eq!(host.terminals()[0].args, ["clean"]);
    }

    #[test]
    fn repeated_clean_without_confirmation_never_runs() {
        let host = host();
        let mut flyway = Flyway::default();
        for _ in 0..2 {
            flyway.run_command("clean", &host.invocation(), None).unwrap();
        }
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn asks_for_the_program_when_denied() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.answer_next_prompt(Some(&["yes"]));
        assert_eq!(Flyway::default().run_command("validate", &host.invocation(), None).unwrap(), None);
        assert_eq!(host.notifications()[0].body, "Running Flyway needs the process:flyway permission. Allow it in Settings › Plugins › Flyway Migrations.");
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

    #[test]
    fn configured_placeholder_completion_refreshes_and_is_project_scoped() {
        let host = host();
        host.add_file("flyway.conf", "flyway.placeholderPrefix=<<\nflyway.placeholderSuffix=>>\nflyway.placeholders.tenant=acme\n");
        let mut flyway = Flyway::default();
        flyway.handle_events(&[Event::FileOpened(format!("{DIR}/V1__init.sql"))], &host.invocation());
        let doc = Document { path: format!("{DIR}/V3__x.sql"), language: "sql".into(), version: 1, text: "<<te".into() };
        assert!(flyway.completion(&doc, Position::new(0, 4), None).iter().any(|item| item.insert_text == "tenant>>"));
        host.add_file("flyway.conf", "flyway.placeholderReplacement=false\nflyway.placeholders.tenant=acme\n");
        flyway.handle_events(&[Event::FileSaved("/project/flyway.conf".into())], &host.invocation());
        let disabled = Document { text: "${te".into(), ..doc };
        assert!(flyway.completion(&disabled, Position::new(0, 4), None).is_empty());
    }

    const INFO_JSON: &str = r#"{"schemaName":"public","schemaVersion":"2","flywayVersion":"10.17.0","migrations":[
      {"category":"Versioned","version":"1","description":"init","type":"SQL","installedOn":"2024-01-01 10:00:00","state":"Success","filepath":"/project/src/main/resources/db/migration/V1__init.sql"},
      {"category":"Versioned","version":"2","description":"add users","type":"SQL","state":"Failed"},
      {"category":"Versioned","version":"3","description":"gone","type":"SQL","installedOn":"2024-01-02 10:00:00","state":"Missing (Success)"},
      {"category":"Repeatable","description":"views","type":"SQL","state":"Pending"},
      {"category":"Versioned","version":"4","description":"later","type":"SQL","state":"Pending"}
    ]}"#;

    fn info_host() -> MockHost {
        let host = host();
        host.add_file("src/main/resources/db/migration/V4__later.sql", "");
        host.add_file("src/main/resources/db/migration/V2__add_users.sql", "");
        host.add_file("src/main/resources/db/migration/R__views.sql", "");
        host
    }

    // Feed explicitly mocked completed-process output. Rendering itself stays cold/cache-only.
    fn completed_info_fixture(host: &MockHost, flyway: &mut Flyway) -> View {
        let project = host.invocation().project().unwrap();
        let (runner, args) = runner_args("info", true);
        flyway.targets.clear();
        flyway.targets_root = Some(project.root().unwrap());
        flyway.info_scope = execution::info_scope(&project).ok();
        flyway.info_at = Some(std::time::Instant::now());
        match project.run_process(runner, &args, None) {
            Ok(output) => flyway.accept_info(&project, output, &execution::Redactor::default()),
            Err(Error::Denied(_)) => {
                View::message(&ui_kit::denied::message("Listing migrations", &format!("process:{runner}"), PLUGIN))
            }
            Err(error) => View::message(&format!("Could not run {runner}: {error}")),
        }
    }
    fn tree(host: &MockHost, flyway: &mut Flyway) -> Vec<TreeItem> {
        let view = completed_info_fixture(host, flyway);
        flyway.info_cache = Some((host.invocation().project().unwrap().root().unwrap(), view));
        let Body::Tree(items) = flyway.render("migrations", None, &host.invocation()).unwrap().body else { panic!("tree expected") };
        items.into_iter().filter(|item|item.key.starts_with("m:")).collect()
    }

    #[test]
    fn info_json_parses_into_tree_items_pending_applied_failed() {
        let host = info_host();
        host.on_process("flyway", |args, _| {
            assert_eq!(args, ["info", "-outputType=json"]);
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: INFO_JSON.into(), stderr: String::new() })
        });
        let mut flyway = Flyway::default();
        let items = tree(&host, &mut flyway);

        let rows: Vec<(&str, Option<&str>, Option<&str>)> =
            items.iter().map(|i| (i.label.as_str(), i.detail.as_deref(), i.icon.as_deref())).collect();
        assert_eq!(
            rows,
            [
                ("V1 init", Some("Applied"), Some("success")),
                ("V2 add users", Some("Failed"), Some("error")),
                ("V3 gone", Some("Missing"), Some("warning")),
                ("R views", Some("Pending"), Some("ci-pending")),
                ("V4 later", Some("Pending"), Some("ci-pending")),
            ]
        );

        flyway.handle("migrations", ViewEvent::Activate(items[0].key.clone()), &host.invocation()).unwrap();
        flyway.handle("migrations", ViewEvent::Activate(items[1].key.clone()), &host.invocation()).unwrap();
        flyway.handle("migrations", ViewEvent::Activate(items[2].key.clone()), &host.invocation()).unwrap();
        flyway.handle("migrations", ViewEvent::Activate(items[3].key.clone()), &host.invocation()).unwrap();
        let opened: Vec<String> = host.opened_locations().into_iter().map(|l| l.0).collect();
        assert_eq!(opened, [format!("{DIR}/V1__init.sql"), format!("{DIR}/V2__add_users.sql"), format!("{DIR}/R__views.sql")]);
    }

    fn message(host: &MockHost) -> String {
        let Body::Message(text) = completed_info_fixture(host, &mut Flyway::default()).body else { panic!("message expected") };
        text
    }

    #[test]
    fn the_migrations_view_explains_why_it_is_empty() {
        let host = host();
        host.on_process("flyway", |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "{\"migrations\":[]}".into(), stderr: String::new() })
        });
        assert_eq!(message(&host), "Flyway reports no migrations. Create one with Flyway: New Migration…, then Run Info… again.");

        host.on_process("flyway", |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: "not json".into(), stderr: String::new() })
        });
        assert!(message(&host).starts_with("Flyway info did not return JSON"));

        host.on_process("flyway", |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(1), stdout: String::new(), stderr: "ERROR: no url\n".into() })
        });
        assert_eq!(message(&host), "Flyway info failed (exit 1): ERROR: no url");

        host.on_process("flyway", |_, _| Err(Error::NotFound("flyway".into())));
        assert!(message(&host).contains("flyway"));

        let denied = MockHost::install().with_capabilities(&["project.read", "events"]);
        assert_eq!(message(&denied), "Listing migrations needs the process:flyway permission. Allow it in Settings › Plugins › Flyway Migrations.");
    }

    #[test]
    fn info_runs_through_mvn_with_the_maven_output_flag() {
        let host = info_host();
        host.set_setting("runner", "\"mvn\"");
        host.set_setting("arguments", "\"-Dflyway.url=x\"");
        host.on_process("mvn", |args, _| {
            assert_eq!(args, ["flyway:info", "-Dflyway.outputType=json", "-Dflyway.url=x"]);
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: INFO_JSON.into(), stderr: String::new() })
        });
        assert_eq!(tree(&host, &mut Flyway::default()).len(), 5);
    }

    #[test]
    fn undo_rows_open_u_files_and_relative_paths_join_the_root() {
        let host = host();
        host.add_file("src/main/resources/db/migration/U2__x.sql", "");
        host.add_file("sql/V9__elsewhere.sql", "");
        host.on_process("flyway", |_, _| {
            let json = r#"{"migrations":[
              {"category":"Undo","version":"2","description":"x","state":"Pending"},
              {"category":"Versioned","version":"9","description":"zzz","state":"Success","filepath":"sql/V9__elsewhere.sql"},
              {"category":"Versioned","version":"8","description":"nope","state":"Success","filepath":"sql/V8__gone.sql"}]}"#;
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: json.into(), stderr: String::new() })
        });
        host.set_setting("locations", "\"filesystem:none\"");
        let mut flyway = Flyway::default();
        let items = tree(&host, &mut flyway);
        host.set_setting("locations", "\"\"");
        let items2 = tree(&host, &mut flyway);
        assert_eq!(items.len(), 3);
        for item in &items2 {
            flyway.handle("migrations", ViewEvent::Activate(item.key.clone()), &host.invocation()).unwrap();
        }
        let opened: Vec<String> = host.opened_locations().into_iter().map(|l| l.0).collect();
        assert_eq!(opened, [format!("{DIR}/U2__x.sql"), "/project/sql/V9__elsewhere.sql".to_owned()]);
    }

    fn run_info(host: &MockHost, json: &'static str) {
        host.on_process("flyway", move |_, _| {
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: json.into(), stderr: String::new() })
        });
    }

    #[test]
    fn baseline_row_opens_b_or_v_file() {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:flyway"]);
        host.add_file("src/main/resources/db/migration/V5__old.sql", "");
        host.add_file("src/main/resources/db/migration/B7__real.sql", "");
        host.add_file("src/main/resources/db/migration/V7__real.sql", "");
        run_info(
            &host,
            r#"{"migrations":[
          {"category":"Baseline","version":"5","description":"old","state":"Baseline"},
          {"category":"Baseline","version":"7","description":"real","state":"Baseline"},
          {"category":"Baseline","version":"1","description":"<< Flyway Baseline >>","state":"Baseline"}]}"#,
        );
        let mut flyway = Flyway::default();
        let items = tree(&host, &mut flyway);
        for item in &items {
            flyway.handle("migrations", ViewEvent::Activate(item.key.clone()), &host.invocation()).unwrap();
        }
        let opened: Vec<String> = host.opened_locations().into_iter().map(|l| l.0).collect();
        assert_eq!(opened, [format!("{DIR}/V5__old.sql"), format!("{DIR}/B7__real.sql")]);
    }

    #[test]
    fn activation_without_project_permission_cannot_open_cached_targets() {
        let host = info_host();
        run_info(&host, INFO_JSON);
        let mut flyway = Flyway::default();
        let items = tree(&host, &mut flyway);
        let denied = host.with_capabilities(&["events"]);
        let result = flyway.handle("migrations", ViewEvent::Activate(items[0].key.clone()), &denied.invocation());
        assert!(result.is_ok());
        let notes = denied.notifications();
        assert!(notes.is_empty());
        assert!(denied.opened_locations().is_empty());
    }

    #[test]
    fn synthetic_baseline_row_has_no_target_even_when_v1_exists() {
        let host = MockHost::install().with_capabilities(&["project.read", "events", "process:flyway"]);
        host.add_file("src/main/resources/db/migration/V1__init.sql", "");
        host.add_file("src/main/resources/db/migration/B1__base.sql", "");
        run_info(
            &host,
            r#"{"migrations":[
          {"category":"Baseline","version":"1","description":"<< Flyway Baseline >>","type":"BASELINE","state":"Baseline"}]}"#,
        );
        let mut flyway = Flyway::default();
        let items = tree(&host, &mut flyway);
        flyway.handle("migrations", ViewEvent::Activate(items[0].key.clone()), &host.invocation()).unwrap();
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn every_declared_template_and_icon_exists() {
        let manifest = include_str!("../plugin.toml");
        let paths: Vec<&str> = manifest
            .lines()
            .filter_map(|l| l.strip_prefix("path = \"").or_else(|| l.strip_prefix("icon = \"")))
            .map(|l| l.trim_end_matches('"'))
            .collect();
        assert!(paths.iter().filter(|p| p.starts_with("templates/")).count() == 7);
        for path in paths {
            let full = format!("{}/{path}", env!("CARGO_MANIFEST_DIR"));
            if path.contains('/') || path.ends_with(".svg") {
                assert!(std::path::Path::new(&full).is_file(), "{path}");
            }
        }
    }
    #[test]
    fn runtime_reported_paths_must_be_owned_matching_migration_files() {
        let host = host();
        host.add_file(".env", "PASSWORD=not-a-migration");
        host.add_file("sql/V8__wrong.sql", "");
        let project = host.invocation().project().unwrap();
        let plugin = Flyway::default();
        for path in [".env", "sql/V8__wrong.sql", "../project/sql/V9__outside.sql", "/other/V9__outside.sql"] {
            let json=serde_json::json!({"migrations":[{"category":"Versioned","version":"9","description":"x","state":"Success","filepath":path}]}).to_string();
            let migration = info::parse(&json).unwrap().remove(0);
            assert!(plugin.migration_file(&project, &[], &migration, &naming::Naming::default()).is_none(), "{path}");
        }
    }
}

#[cfg(test)]
mod custom_group_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use serde_json::{json,Value};
    fn request(plugin: &mut Flyway, host: &MockHost, mut value: Value) -> Value {
        value["channel"]=json!("ui-kit");value["sequence"]=json!(1);value["project"]=json!("/project");
        plugin.handle("migrations",ViewEvent::Message(value.to_string()),&host.invocation()).unwrap();
        serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap()
    }
    #[test]
    fn packaged_migrations_view_shows_local_migrations_as_columns() {
        let host=MockHost::install().with_capabilities(&["storage","project.read"]);
        host.add_file("flyway.conf","flyway.locations=filesystem:db/migration");
        host.add_file("db/migration/V1__create_users.sql","select 1;");
        let mut plugin=Flyway::default();
        let rendered=request(&mut plugin,&host,json!({"op":"render"}));
        let body=&rendered["model"]["body"];
        assert_eq!(body["kind"],"resources","{rendered}");
        assert_eq!(body["columns"],json!(["Version","Description","Type","Problems"]));
        assert_eq!(body["items"][0]["cells"],json!(["1","create users","Versioned",""]));
    }
    #[test]
    fn one_custom_container_routes_tabs_and_rejects_hidden_tab_mutations_without_cli() {
        let host=MockHost::install().with_capabilities(&["storage","project.read"]);
        let mut plugin=Flyway::default();
        let first=request(&mut plugin,&host,json!({"op":"render"}));
        assert_eq!(first["model"]["activeView"],"migrations");
        assert_eq!(first["model"]["viewTabs"].as_array().unwrap().len(),3);
        let second=request(&mut plugin,&host,json!({"op":"select-view","targetView":"migrations","target":"command-output"}));
        assert_eq!(second["model"]["activeView"],"command-output");
        let stale=request(&mut plugin,&host,json!({"op":"event","targetView":"migrations","event":{"kind":"button","key":"refresh"}}));
        assert!(stale.get("error").is_some());
        let active=request(&mut plugin,&host,json!({"op":"render"}));
        assert_eq!(active["model"]["activeView"],"command-output");
        assert!(host.process_runs().is_empty()&&host.terminals().is_empty()&&host.requests().is_empty());
        assert!(host.view_messages().iter().all(|(view,_)|view=="migrations"));
    }
}

#[cfg(test)]
mod indexer_guard {
    /// Table/entity/migration links stay definition navigation: as Rename anchors they would rewrite
    /// applied migrations (immutable Flyway history) and dialect-folded or class-derived names.
    #[test]
    fn no_document_indexer_is_contributed() {
        let manifest = include_str!("../plugin.toml");
        assert!(!manifest.contains("[[contributes.documentIndexers]]"));
        assert!(!include_str!("lib.rs").contains(concat!("views, ", "indexing")));
    }
}

#[cfg(test)]
mod marker_icon_guard {
    #[test]
    fn marker_icons_are_declared_in_the_manifest() {
        let manifest = include_str!("../plugin.toml");
        for icon in [crate::PLACEHOLDER_MARKER, crate::TABLE_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")), "{icon}");
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file(), "{icon}");
        }
    }
}
