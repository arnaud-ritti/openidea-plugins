//! Per-project file indexes read a little at a time, so that every editor
//! provider call stays within its 50 ms.

use openidea_plugin::{Error, Event, Project, host};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const MAX_FILES: u32 = 10_000;
/// How long a full pass over the project is trusted; open documents are always re-read.
const TTL: Duration = Duration::from_secs(120);
/// Reading time per editor provider call.
pub const PROVIDER_BUDGET: Duration = Duration::from_millis(30);
/// Reading time for a command, an activation or a batch of file events. Every
/// call gets 5×10⁹ fuel, which a few seconds of parsing can use up: the call
/// then traps and drops the instance. A call that began a task gets 12 times
/// more, refilled by each `report` or `is_cancelled`, and can read on in steps.
pub const COMMAND_BUDGET: Duration = Duration::from_secs(1);
/// Discovery and reading time for a view render or an `endpoints` export. The
/// host gives an export 2 s and every call 5×10⁹ fuel; a call that runs out
/// traps and drops the instance with its index, so a large project would start
/// over on every attempt. Views refresh themselves while scanning instead.
pub const VIEW_BUDGET: Duration = Duration::from_millis(300);
/// Reading time a diagnostics call may spend advancing a cold or stale index.
pub const DIAGNOSTICS_WARM: Duration = Duration::from_millis(10);

/// The project holding `path` (the innermost when projects nest).
pub fn project_of(path: &str) -> Option<Project> {
    host::project_of(path)
}

/// The root of `project` when it is canonical (POSIX or drive-letter), as written.
pub fn scoped_root(project: &Project) -> Option<String> {
    let root = project.root().ok()?;
    crate::scope::scope_path(&root)?;
    Some(root)
}

/// The root of the open project holding `path`, when both are canonical
/// (POSIX or drive-letter) and the path is below the root.
pub fn owning_root(path: &str) -> Option<String> {
    let root = scoped_root(&project_of(path)?)?;
    (crate::scope::current_owner(path, std::slice::from_ref(&root)).as_deref() == Some(root.as_str())).then_some(root)
}

/// What a project's files matching some globs say, by absolute path. The
/// first pass fills `files` as it goes; later passes replace it when done.
pub struct Index<T> {
    files: BTreeMap<String, T>,
    next: BTreeMap<String, T>,
    unread: Vec<String>,
    /// Discovery under way: the next glob to list and the paths listed so far.
    discovering: Option<(usize, Vec<String>)>,
    scanned: Option<Instant>,
    /// A pass has started and not finished.
    reading: bool,
    passes: u32,
    /// Unreadable files as of the last finished pass, kept current by events.
    failed: BTreeSet<String>,
    discovery_incomplete: bool,
    /// What the pass in progress found so far; published when it finishes.
    pass_failed: BTreeSet<String>,
    pass_discovery_incomplete: bool,
    /// Changes were missed: the last pass no longer counts.
    stale: bool,
    globs: &'static [&'static str],
    parse: fn(&str, &str) -> Option<T>,
    /// Files larger than this are not parsed (see [`Index::with_limits`]).
    max_file: usize,
    /// Bytes of kept files' text the index may hold.
    max_total: usize,
    /// Text bytes of each kept file, when `max_total` is set.
    weights: BTreeMap<String, usize>,
    total: usize,
    required: fn(&str, Option<&str>) -> bool,
    rejected: fn(&str, &str) -> bool,
    /// Discovery leaves files of nested open projects to their own index.
    exclude_nested: bool,
    id: u64,
    /// What [`Index::update`] last applied per path, so the same text again changes nothing.
    applied: BTreeMap<String, u64>,
    generation: u64,
}

fn every(_: &str, _: Option<&str>) -> bool {
    true
}

thread_local! {
    static INDEX_IDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A number no earlier index of this plugin instance had, even after [`Indexes::reset`].
fn next_id() -> u64 {
    INDEX_IDS.with(|ids| {
        ids.set(ids.get() + 1);
        ids.get()
    })
}

fn never(_: &str, _: &str) -> bool {
    false
}

impl<T: Clone> Index<T> {
    /// `parse(path, text)` keeps what matters of a file (`None`: nothing).
    pub fn new(globs: &'static [&'static str], parse: fn(&str, &str) -> Option<T>) -> Self {
        Self {
            files: BTreeMap::new(),
            next: BTreeMap::new(),
            unread: Vec::new(),
            discovering: None,
            scanned: None,
            reading: false,
            passes: 0,
            failed: BTreeSet::new(),
            discovery_incomplete: false,
            pass_failed: BTreeSet::new(),
            pass_discovery_incomplete: false,
            stale: false,
            globs,
            parse,
            max_file: usize::MAX,
            max_total: usize::MAX,
            weights: BTreeMap::new(),
            total: 0,
            required: every,
            rejected: never,
            exclude_nested: false,
            id: next_id(),
            applied: BTreeMap::new(),
            generation: 0,
        }
    }

    /// Files over `max_file` bytes are not parsed, and a file whose text
    /// would take the kept files over `max_total` bytes is not kept: both
    /// count as unreadable.
    pub fn with_limits(mut self, max_file: usize, max_total: usize) -> Self {
        self.max_file = max_file;
        self.max_total = max_total;
        self
    }

    /// Only unreadable files `required(path, text)` accepts make the index
    /// incomplete; the others are skipped. `text` is `None` when the file
    /// could not be read at all.
    pub fn requiring(mut self, required: fn(&str, Option<&str>) -> bool) -> Self {
        self.required = required;
        self
    }

    /// A file that parses to nothing counts as unreadable when
    /// `rejected(path, text)`: broken rather than irrelevant.
    pub fn rejecting(mut self, rejected: fn(&str, &str) -> bool) -> Self {
        self.rejected = rejected;
        self
    }

    /// Discovery skips the files of open projects nested inside this one:
    /// they belong to the innermost project, as [`project_of`] answers.
    pub fn excluding_nested_projects(mut self) -> Self {
        self.exclude_nested = true;
        self
    }

    /// What `text` gives within the limits; `Err` when it counts as unreadable.
    fn parsed(&mut self, path: &str, text: &str) -> Result<Option<T>, ()> {
        let old = self.weights.get(path).copied().unwrap_or(0);
        if text.len() > self.max_file || self.total - old + text.len() > self.max_total {
            return Err(());
        }
        match (self.parse)(path, text) {
            Some(value) => {
                if self.max_total != usize::MAX {
                    self.weights.insert(path.to_owned(), text.len());
                    self.total = self.total - old + text.len();
                }
                Ok(Some(value))
            }
            None if (self.rejected)(path, text) => Err(()),
            None => Ok(None),
        }
    }

    fn unweigh(&mut self, path: &str) {
        if let Some(weight) = self.weights.remove(path) {
            self.total -= weight;
        }
    }

    /// Discovers and reads files for at most `budget`. Starting a pass (cold,
    /// or past its TTL) first lists files with one `find_files` per glob; each
    /// call lists at least one glob, then more while `budget` lasts, so a pass
    /// moves on however small the budget.
    pub fn scan(&mut self, project: &Project, budget: Duration) {
        let start = Instant::now();
        if self.discovering.is_none() && self.unread.is_empty() && self.scanned.is_none_or(|at| at.elapsed() > TTL) {
            self.pass_failed.clear();
            self.pass_discovery_incomplete = false;
            self.discovering = Some((0, Vec::new()));
            self.reading = true;
        }
        if let Some((mut next, mut paths)) = self.discovering.take() {
            let mut listed = false;
            while let Some(glob) = self.globs.get(next)
                && (!listed || start.elapsed() < budget)
            {
                match project.find_files(glob, MAX_FILES) {
                    Ok(found) => {
                        self.pass_discovery_incomplete |= found.len() >= MAX_FILES as usize;
                        paths.extend(found);
                    }
                    Err(_) => self.pass_discovery_incomplete = true,
                }
                next += 1;
                listed = true;
            }
            if next < self.globs.len() {
                self.discovering = Some((next, paths));
                return;
            }
            paths.sort();
            paths.dedup();
            if self.exclude_nested {
                let nested = nested_roots(project);
                paths.retain(|path| !nested.iter().any(|root| path.starts_with(root)));
            }
            paths.reverse(); // popped from the end: read in path order
            self.unread = paths;
            self.scanned = Some(Instant::now());
        }
        let first = self.passes == 0;
        while start.elapsed() < budget
            && let Some(path) = self.unread.pop()
        {
            self.generation += 1;
            self.applied.remove(&path);
            let Ok(text) = project.read_file(&path) else {
                if (self.required)(&path, None) {
                    self.pass_failed.insert(path);
                }
                continue;
            };
            let parsed = self.parsed(&path, &text);
            let target = if first { &mut self.files } else { &mut self.next };
            match parsed {
                Ok(Some(value)) => {
                    target.insert(path, value);
                }
                Ok(None) => {
                    target.remove(&path);
                }
                Err(()) => {
                    target.remove(&path);
                    if (self.required)(&path, Some(&text)) {
                        self.pass_failed.insert(path);
                    }
                }
            }
        }
        if self.reading && self.unread.is_empty() {
            self.generation += 1;
            if !first {
                self.files = std::mem::take(&mut self.next);
                let files = &self.files;
                self.weights.retain(|path, _| files.contains_key(path));
                self.total = self.weights.values().sum();
            }
            self.failed = std::mem::take(&mut self.pass_failed);
            self.discovery_incomplete = self.pass_discovery_incomplete;
            self.stale = false;
            self.reading = false;
            self.passes += 1;
        }
    }

    /// Replaces what a file says with its current text (an open document).
    pub fn update(&mut self, path: &str, text: &str) {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let hash = hasher.finish();
        if !self.reading && self.applied.get(path) == Some(&hash) && self.files.contains_key(path) {
            return;
        }
        self.remove(path);
        self.applied.insert(path.to_owned(), hash);
        let value = match self.parsed(path, text) {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(()) => return self.fail_with(path, Some(text)),
        };
        if self.reading && self.passes > 0 {
            self.next.insert(path.to_owned(), value.clone());
        }
        self.files.insert(path.to_owned(), value);
    }

    /// Whether the last pass finished (some reads may have failed) and no
    /// change was missed since. A refresh in progress keeps the last pass.
    pub fn walked(&self) -> bool {
        self.passes > 0 && !self.stale
    }

    /// Required files the last pass, or a later change, could not read or
    /// keep within the limits.
    pub fn unreadable(&self) -> usize {
        self.failed.len()
    }

    /// Every path the index knows of: kept, queued or unreadable.
    pub fn inventory(&self) -> BTreeSet<&str> {
        let kept = self.files.keys().chain(self.next.keys());
        let discovered = self.discovering.iter().flat_map(|(_, paths)| paths);
        kept.chain(&self.unread).chain(discovered).chain(&self.failed).chain(&self.pass_failed).map(String::as_str).collect()
    }

    /// Queues `paths` (such as an inventory saved by an earlier session) to
    /// read before the first discovery. The pass reading them stays partial:
    /// only the discovery that follows can complete the index. Ignored once
    /// the index has started reading.
    pub fn seed(&mut self, mut paths: Vec<String>) {
        if self.passes > 0 || self.reading || self.discovering.is_some() || self.scanned.is_some() || paths.is_empty() {
            return;
        }
        paths.sort();
        paths.dedup();
        paths.reverse();
        self.unread = paths;
        self.reading = true;
        self.pass_failed.clear();
        self.pass_discovery_incomplete = true;
        self.generation += 1;
    }

    /// Unique among all indexes made so far: with [`Self::generation`] it names
    /// one state of one index, which a reset or a new index never repeats.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The files that have something for `project` (`None`: not this file), as a read-only view.
    pub fn view<'a, U>(&'a self, project: impl Fn(&'a T) -> Option<&'a U>) -> View<'a, U> {
        View {
            entries: self.files.iter().filter_map(|(path, value)| Some((path.as_str(), project(value)?))).collect(),
            complete: self.complete(),
            fresh: self.fresh(),
            scanning: self.scanning(),
            id: self.id,
            generation: self.generation,
        }
    }

    /// Changes whenever what the index holds may have changed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether the last pass was whole and uncapped and every file was read.
    /// Absence-based diagnostics must not trust partial discovery or failed reads.
    pub fn complete(&self) -> bool {
        self.walked() && self.unreadable() == 0 && !self.discovery_incomplete
    }

    /// Changes were missed: `complete()` and `fresh()` stay false until the
    /// next pass, which starts over, finishes.
    pub fn mark_stale(&mut self) {
        self.generation += 1;
        self.stale = true;
        self.scanned = None;
        self.unread.clear();
        self.discovering = None;
        self.next.clear();
    }

    /// A complete source pass still inside its trust interval. Cache-only live
    /// checks use this for absence claims without triggering a refresh themselves.
    pub fn fresh(&self) -> bool {
        self.complete() && self.scanned.is_some_and(|at| at.elapsed() <= TTL)
    }

    /// Whether more queued work can be advanced by another render/provider call.
    pub fn scanning(&self) -> bool {
        self.reading
    }

    /// Forgets the files so the next scan starts over.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.applied.clear();
        self.weights.clear();
        self.total = 0;
        self.files.clear();
        self.next.clear();
        self.unread.clear();
        self.discovering = None;
        self.scanned = None;
        self.reading = false;
        self.passes = 0;
        self.failed.clear();
        self.discovery_incomplete = false;
        self.pass_failed.clear();
        self.pass_discovery_incomplete = false;
        self.stale = false;
    }

    pub fn remove(&mut self, path: &str) {
        self.generation += 1;
        self.applied.remove(path);
        self.unweigh(path);
        self.failed.remove(path);
        self.pass_failed.remove(path);
        self.files.remove(path);
        self.next.remove(path);
        self.unread.retain(|p| p != path);
        if let Some((_, paths)) = &mut self.discovering {
            paths.retain(|p| p != path);
        }
    }

    /// Forgets the files of projects nested inside `project`, when this index
    /// excludes them: one opened after the scan still took them over.
    fn drop_nested(&mut self, project: &Project) {
        if !self.exclude_nested {
            return;
        }
        let nested = nested_roots(project);
        if nested.is_empty() {
            return;
        }
        let foreign: Vec<String> =
            self.files.keys().chain(self.next.keys()).filter(|path| nested.iter().any(|root| path.starts_with(root))).cloned().collect();
        for path in foreign {
            self.remove(&path);
        }
    }

    /// Forgets what a file said because it could not be read.
    pub fn fail(&mut self, path: &str) {
        self.fail_with(path, None);
    }

    fn fail_with(&mut self, path: &str, text: Option<&str>) {
        self.remove(path);
        if !(self.required)(path, text) {
            return;
        }
        self.failed.insert(path.to_owned());
        if self.reading {
            self.pass_failed.insert(path.to_owned());
        }
    }

    pub fn files(&self) -> &BTreeMap<String, T> {
        &self.files
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.files.iter().map(|(p, v)| (p.as_str(), v))
    }
}

/// What one part of a combined index says per file, with the index's state.
pub struct View<'a, U> {
    entries: Vec<(&'a str, &'a U)>,
    complete: bool,
    fresh: bool,
    scanning: bool,
    id: u64,
    generation: u64,
}

impl<'a, U> View<'a, U> {
    /// The files in path order.
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a U)> + '_ {
        self.entries.iter().copied()
    }

    pub fn get(&self, path: &str) -> Option<&'a U> {
        self.entry(path).map(|(_, value)| value)
    }

    /// The file's path as the view holds it, with what it says.
    pub fn entry(&self, path: &str) -> Option<(&'a str, &'a U)> {
        let at = self.entries.binary_search_by(|(p, _)| (*p).cmp(path)).ok()?;
        Some(self.entries[at])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn complete(&self) -> bool {
        self.complete
    }

    pub fn fresh(&self) -> bool {
        self.fresh
    }

    pub fn scanning(&self) -> bool {
        self.scanning
    }

    /// [`Index::id`] of the index viewed.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Prefixes (`root/`) of the open projects nested inside `project`: their
/// files belong to them, as [`project_of`] answers.
fn nested_roots(project: &Project) -> Vec<String> {
    let projects = host::projects().unwrap_or_default();
    if projects.len() < 2 {
        return Vec::new();
    }
    let Ok(root) = project.root() else { return Vec::new() };
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let roots = projects.into_iter().filter_map(|other| other.root().ok());
    roots.map(|other| format!("{}/", other.trim_end_matches('/'))).filter(|other| other.starts_with(&prefix) && *other != prefix).collect()
}

/// What an index covers when it is not the whole project: "scanning…" while
/// reading, "not indexed yet" before any pass, "partial index (N unreadable)"
/// or "partial index" after a pass that skipped files or hit the discovery
/// cap; `None` when whole.
pub fn coverage_note<T: Clone>(index: &Index<T>) -> Option<String> {
    if index.scanning() {
        return Some(SCANNING_NOTE.to_owned());
    }
    if !index.walked() {
        return Some("not indexed yet".to_owned());
    }
    if index.complete() {
        return None;
    }
    Some(match index.unreadable() {
        0 => "partial index".to_owned(),
        unreadable => format!("partial index ({unreadable} unreadable)"),
    })
}

pub const SCANNING_NOTE: &str = "scanning…";

/// The message of a view with nothing to list: `none_found` is e.g. "No STOMP
/// destinations found"; `scanning` is the index's [`Index::scanning`].
pub fn empty_message(none_found: &str, scanning: bool, note: Option<&str>) -> String {
    match note {
        _ if scanning => "Scanning…".to_owned(),
        None => format!("{none_found}."),
        Some(note) => format!("{none_found} · {note}"),
    }
}

/// What [`Indexes::on_events`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventOutcome {
    /// Changed paths applied to a cached index.
    pub updated: usize,
    /// The deadline cut the batch short; the affected indexes were marked stale.
    pub stale: bool,
}

/// One [`Index`] per project root.
pub struct Indexes<T> {
    by_root: BTreeMap<String, Index<T>>,
    unidentified: Index<T>,
    globs: &'static [&'static str],
    parse: fn(&str, &str) -> Option<T>,
    exclude_nested: bool,
    limits: (usize, usize),
}

impl<T: Clone> Indexes<T> {
    pub fn new(globs: &'static [&'static str], parse: fn(&str, &str) -> Option<T>) -> Self {
        Self {
            by_root: BTreeMap::new(),
            unidentified: Index::new(globs, parse),
            globs,
            parse,
            exclude_nested: false,
            limits: (usize::MAX, usize::MAX),
        }
    }

    /// Every project's index applies [`Index::with_limits`].
    pub fn with_limits(mut self, max_file: usize, max_total: usize) -> Self {
        self.limits = (max_file, max_total);
        self.unidentified.max_file = max_file;
        self.unidentified.max_total = max_total;
        self
    }

    /// Every project's index leaves out the files of projects nested inside it.
    pub fn excluding_nested_projects(mut self) -> Self {
        self.exclude_nested = true;
        self.unidentified.exclude_nested = true;
        self
    }

    /// The index of `root`, created empty when there is none yet.
    fn entry(&mut self, root: String) -> &mut Index<T> {
        let (globs, parse, nested, (max_file, max_total)) = (self.globs, self.parse, self.exclude_nested, self.limits);
        self.by_root.entry(root).or_insert_with(|| {
            let mut index = Index::new(globs, parse).with_limits(max_file, max_total);
            index.exclude_nested = nested;
            index
        })
    }

    /// The index of `project`, discovered and read for at most `budget` more.
    /// A pass lists at least one glob per call (`find_files`), which the budget
    /// cannot cut short: providers use [`Self::advance`] instead.
    pub fn get(&mut self, project: &Project, budget: Duration) -> &mut Index<T> {
        let Ok(root) = project.root() else {
            self.unidentified.reset();
            return &mut self.unidentified;
        };
        if root.is_empty() {
            self.unidentified.reset();
            return &mut self.unidentified;
        }
        let index = self.entry(root);
        index.scan(project, budget);
        index.drop_nested(project);
        index
    }

    /// Reads on for at most `budget` when `project`'s pass is already under
    /// way; never discovers files, so a provider may call it.
    pub fn advance(&mut self, project: &Project, budget: Duration) {
        let Ok(root) = project.root() else { return };
        if let Some(index) = self.by_root.get_mut(&root).filter(|index| index.reading && !index.unread.is_empty()) {
            index.scan(project, budget);
        }
    }

    /// [`Self::get`] when `discover` (events, views, commands), else [`Self::advance`] (providers).
    pub fn warm(&mut self, project: &Project, budget: Duration, discover: bool) {
        match discover {
            true => {
                self.get(project, budget);
            }
            false => self.advance(project, budget),
        }
    }

    /// The index of the project holding `path`, with the open document's text applied.
    pub fn for_document(&mut self, path: &str, text: &str, budget: Duration) -> Option<&mut Index<T>> {
        let project = project_of(path)?;
        let index = self.get(&project, budget);
        if index.globs.iter().any(|g| glob_matches(g, path)) {
            index.update(path, text);
        }
        Some(index)
    }

    /// Current-document facts plus already cached project facts, without file
    /// discovery or scanning. A cold cache stays incomplete until a background,
    /// navigation or explicit command path warms it.
    pub fn for_cached_document(&mut self, path: &str, text: &str) -> Option<&mut Index<T>> {
        let project = project_of(path)?;
        let root = project.root().ok().filter(|root| !root.is_empty())?;
        let index = self.entry(root);
        index.drop_nested(&project);
        if index.globs.iter().any(|g| glob_matches(g, path)) {
            index.update(path, text);
        }
        Some(index)
    }

    /// The cache of the project holding `path`, as it is: the document is not
    /// applied, so the generation does not move.
    pub fn cached(&self, path: &str) -> Option<&Index<T>> {
        let root = project_of(path)?.root().ok().filter(|root| !root.is_empty())?;
        self.by_root.get(&root)
    }

    /// Like [`Self::for_cached_document`], but a cold, stale, expired or
    /// refreshing index reads on for at most `budget` first. A cold, stale or
    /// expired index first lists its files, at least one glob (`find_files`)
    /// per call, which the budget cannot cut short.
    pub fn for_cached_document_warm(&mut self, path: &str, text: &str, budget: Duration) -> Option<&mut Index<T>> {
        let project = project_of(path)?;
        let root = project.root().ok().filter(|root| !root.is_empty())?;
        let index = self.entry(root);
        if !index.fresh() || index.scanning() {
            index.scan(&project, budget);
        }
        index.drop_nested(&project);
        if index.globs.iter().any(|g| glob_matches(g, path)) {
            index.update(path, text);
        }
        Some(index)
    }

    /// Apply a watched change to a cache already owned by this project. This
    /// does not start a scan; a missing cache will discover the file normally.
    pub fn update_existing(&mut self, project: &Project, path: &str, text: Option<&str>) -> bool {
        let Some(index) = self.owned(project, path) else { return false };
        match text {
            Some(text) => index.update(path, text),
            None => index.remove(path),
        }
        true
    }

    /// Applies file events path by path to the caches they touch, reading each
    /// changed file once. A missing file is removed; an unreadable one counts
    /// against completeness on its own. Paths left when `deadline` passes mark
    /// their index stale instead.
    pub fn on_events(&mut self, events: &[Event], deadline: Instant) -> EventOutcome {
        let mut outcome = EventOutcome::default();
        for (path, removed) in changed_paths(events) {
            let Some(project) = project_of(path) else { continue };
            let Some(index) = self.owned(&project, path) else { continue };
            if Instant::now() >= deadline {
                index.mark_stale();
                outcome.stale = true;
                continue;
            }
            outcome.updated += 1;
            if removed {
                index.remove(path);
                continue;
            }
            match project.read_file(path) {
                Ok(text) => index.update(path, &text),
                // A file that exists but is not text may be reported as missing.
                Err(Error::NotFound(_)) if project.read_bytes(path).is_ok() => index.fail(path),
                Err(Error::NotFound(_)) => index.remove(path),
                Err(_) => index.fail(path),
            }
        }
        outcome
    }

    /// The cache of `project` that watches `path`, when `path` is inside its root.
    fn owned(&mut self, project: &Project, path: &str) -> Option<&mut Index<T>> {
        let root = project.root().ok()?;
        let prefix = format!("{}/", root.trim_end_matches('/'));
        let relative = path.strip_prefix(&prefix)?;
        if relative.split('/').any(|part| matches!(part, "." | "..")) {
            return None;
        }
        self.by_root.get_mut(&root).filter(|index| index.globs.iter().any(|g| glob_matches(g, path)))
    }

    /// Forgets every project's files.
    pub fn reset(&mut self) {
        self.by_root.clear();
        self.unidentified.reset();
    }
}

/// Changed paths with whether they were removed: removals first, so a path
/// removed and created again in one batch ends up read.
pub fn changed_paths(events: &[Event]) -> Vec<(&str, bool)> {
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for event in events {
        match event {
            Event::FileSaved(path) => changed.push((path.as_str(), false)),
            Event::FilesChanged(files) => {
                removed.extend(files.removed.iter().map(|path| (path.as_str(), true)));
                changed.extend(files.created.iter().chain(&files.modified).map(|path| (path.as_str(), false)));
            }
            _ => {}
        }
    }
    removed.extend(changed);
    removed
}

/// Whether `path` (absolute or relative) matches a project glob such as
/// `**/*.java` or `**/src/main/resources/**/*.yml`. `**` spans directories,
/// `*` stays within one; the glob is matched against the path's tail.
pub fn glob_matches(glob: &str, path: &str) -> bool {
    fn go(g: &[u8], p: &[u8]) -> bool {
        match g {
            [] => p.is_empty(),
            [b'*', b'*', b'/', rest @ ..] => (0..=p.len()).any(|i| (i == 0 || p[i - 1] == b'/') && go(rest, &p[i..])),
            [b'*', b'*', rest @ ..] => (0..=p.len()).any(|i| go(rest, &p[i..])),
            [b'*', rest @ ..] => (0..=p.len()).take_while(|&i| i == 0 || p[i - 1] != b'/').any(|i| go(rest, &p[i..])),
            [c, rest @ ..] => p.first() == Some(c) && go(rest, &p[1..]),
        }
    }
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches('/');
    let glob = glob.trim_start_matches('/');
    // A relative glob may match from any directory of an absolute path.
    (0..=path.len()).any(|i| (i == 0 || path.as_bytes()[i - 1] == b'/') && go(glob.as_bytes(), &path.as_bytes()[i..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::FilesChanged;
    use openidea_plugin::mock::MockHost;

    fn words(_: &str, text: &str) -> Option<usize> {
        let n = text.split_whitespace().count();
        (n > 0).then_some(n)
    }

    #[test]
    fn updating_a_file_with_the_text_it_already_has_keeps_the_generation() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a b c");
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&host.invocation().project().unwrap(), COMMAND_BUDGET);
        index.update("/project/src/A.java", "a b");
        let generation = index.generation();
        index.update("/project/src/A.java", "a b");
        assert_eq!(index.generation(), generation);
        index.update("/project/src/A.java", "a b c d");
        assert_ne!(index.generation(), generation);
        assert_eq!(index.files()["/project/src/A.java"], 4);
    }

    #[test]
    fn owning_root_accepts_drive_letter_roots() {
        let host = MockHost::install();
        host.set_project_root(Some("C:/work/app"));
        assert_eq!(owning_root("C:/work/app/src/A.java").as_deref(), Some("C:/work/app"));
        assert_eq!(owning_root("C:/work/app/../x/A.java"), None);
    }

    #[test]
    fn reads_the_project_and_applies_open_documents() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a b c");
        host.add_file("src/B.java", "");
        host.add_file("README.md", "x");
        let mut indexes = Indexes::new(&["**/*.java"], words);
        let project = host.invocation().project().unwrap();
        let index = indexes.get(&project, COMMAND_BUDGET);
        assert!(index.complete());
        assert_eq!(index.iter().map(|(p, n)| (p.rsplit('/').next().unwrap(), *n)).collect::<Vec<_>>(), [("A.java", 3)]);

        let index = indexes.for_document("/project/src/B.java", "one two", PROVIDER_BUDGET).unwrap();
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/A.java", "/project/src/B.java"]);
        let index = indexes.for_document("/project/README.md", "x y", PROVIDER_BUDGET).unwrap();
        assert_eq!(index.files().len(), 2, "not one of the globs");
    }

    #[test]
    fn denied_discovery_is_partial_and_does_not_request_more_work() {
        let host = MockHost::install().with_capabilities(&[]);
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&project, COMMAND_BUDGET);
        assert!(!index.complete());
        assert!(!index.scanning());
        index.update("/project/A.java", "current document");
        assert!(!index.complete(), "one open document cannot prove project-wide absence");
    }

    #[test]
    fn completed_but_expired_facts_cannot_prove_current_absence() {
        let host = MockHost::install();
        host.add_file("src/A.java", "known");
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&host.invocation().project().unwrap(), COMMAND_BUDGET);
        assert!(index.fresh());
        index.scanned = Some(Instant::now() - TTL - Duration::from_secs(1));
        assert!(index.complete(), "a historical pass still completed");
        assert!(!index.fresh(), "cache-only absence checks need current provenance");
        index.update("/project/src/A.java", "unsaved");
        assert!(!index.fresh(), "an open document cannot refresh the rest of the project");
    }

    #[test]
    fn matches_globs() {
        assert!(glob_matches("**/*.java", "/p/src/A.java"));
        assert!(glob_matches("**/*.java", "A.java"));
        assert!(!glob_matches("**/*.java", "/p/A.javax"));
        assert!(glob_matches("**/application*.yml", "/p/src/main/resources/application-dev.yml"));
        assert!(!glob_matches("src/*.java", "/p/src/a/A.java"));
        assert!(glob_matches("**/resources/**/*.graphqls", "/p/src/main/resources/graphql/schema.graphqls"));
        assert!(glob_matches("**/resources/**/*.graphqls", "/p/src/main/resources/schema.graphqls"));
    }
    #[test]
    fn watched_changes_update_only_the_owned_cache_and_remove_old_paths() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        indexes.get(&project, COMMAND_BUDGET);
        assert!(indexes.update_existing(&project, "/project/src/A.java", None));
        assert!(indexes.update_existing(&project, "/project/src/B.java", Some("b c")));
        assert!(!indexes.get(&project, Duration::ZERO).files().contains_key("/project/src/A.java"));
        assert_eq!(indexes.get(&project, Duration::ZERO).files()["/project/src/B.java"], 2);
        assert!(!indexes.update_existing(&project, "/other/src/B.java", Some("bad")));
        assert!(!indexes.update_existing(&project, "/project/../other/B.java", Some("bad")));
        host.set_project_root(Some("/second"));
        let second = host.invocation().project().unwrap();
        assert!(!indexes.update_existing(&second, "/project/src/B.java", None));
        assert_eq!(indexes.get(&project, Duration::ZERO).files()["/project/src/B.java"], 2);
    }
    #[test]
    fn denied_project_identity_cannot_reuse_an_anonymous_cache() {
        let host = MockHost::install().with_capabilities(&[]);
        let mut indexes = Indexes::new(&["**/*.java"], words);
        let first = host.invocation().project().unwrap();
        indexes.get(&first, Duration::ZERO).update("/project/A.java", "private first");
        host.set_project_root(Some("/second"));
        let second = host.invocation().project().unwrap();
        assert!(indexes.get(&second, Duration::ZERO).files().is_empty());
        assert!(indexes.by_root.is_empty());
    }
    #[test]
    fn cached_document_requests_never_begin_discovery_and_keep_cold_provenance() {
        let host = MockHost::install();
        host.add_file("src/Disk.java", "disk facts");
        let mut indexes = Indexes::new(&["**/*.java"], words);
        let index = indexes.for_cached_document("/project/src/Open.java", "current unsaved facts").unwrap();
        assert!(!index.complete());
        assert!(index.scanned.is_none() && index.unread.is_empty() && !index.reading);
        assert_eq!(index.files().len(), 1);
        assert_eq!(index.files()["/project/src/Open.java"], 3);
        let project = host.invocation().project().unwrap();
        indexes.get(&project, COMMAND_BUDGET);
        let warm = indexes.for_cached_document("/project/src/Open.java", "new").unwrap();
        assert!(warm.complete());
        assert!(warm.files().contains_key("/project/src/Disk.java"));
        assert_eq!(warm.files()["/project/src/Open.java"], 1);
        host.set_project_root(Some("/second"));
        let second = indexes.for_cached_document("/second/src/Next.java", "other").unwrap();
        assert_eq!(second.files().len(), 1);
        assert!(!second.files().contains_key("/project/src/Disk.java"));
    }

    #[test]
    fn cached_leaves_the_generation_alone() {
        let _host = MockHost::install();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        assert!(indexes.cached("/project/src/Open.java").is_none());
        indexes.for_cached_document("/project/src/Open.java", "one two");
        let generation = indexes.cached("/project/src/Open.java").unwrap().generation();
        assert_eq!(indexes.cached("/project/src/Open.java").unwrap().generation(), generation);
        indexes.for_cached_document("/project/src/Open.java", "one two three");
        assert_ne!(indexes.cached("/project/src/Open.java").unwrap().generation(), generation);
    }

    fn changed(created: &[&str], modified: &[&str], removed: &[&str]) -> Vec<Event> {
        let owned = |paths: &[&str]| paths.iter().map(|p| (*p).to_owned()).collect();
        vec![Event::FilesChanged(FilesChanged { created: owned(created), modified: owned(modified), removed: owned(removed) })]
    }

    #[test]
    fn coverage_note_and_empty_message() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&project, Duration::ZERO);
        assert_eq!(coverage_note(&index).as_deref(), Some("scanning…"));
        assert_eq!(empty_message("None found", index.scanning(), coverage_note(&index).as_deref()), "Scanning…");
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(coverage_note(&index), None);
        assert_eq!(empty_message("None found", false, None), "None found.");
        assert_eq!(coverage_note(&Index::new(&["**/*.java"], words)).as_deref(), Some("not indexed yet"));
        host.add_file_bytes("src/Latin.java", b"caf\xe9");
        let mut indexes = Indexes::new(&["**/*.java"], words);
        indexes.get(&project, COMMAND_BUDGET);
        indexes.on_events(&changed(&["/project/src/Latin.java"], &[], &[]), Instant::now() + COMMAND_BUDGET);
        let index = indexes.get(&project, Duration::ZERO);
        let note = coverage_note(index);
        assert_eq!(note.as_deref(), Some("partial index (1 unreadable)"));
        assert_eq!(empty_message("None found", false, note.as_deref()), "None found · partial index (1 unreadable)");
    }

    #[test]
    fn complete_survives_ttl_refresh() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/B.java", "b");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&project, COMMAND_BUDGET);
        assert!(index.complete());
        index.scanned = Some(Instant::now() - TTL - Duration::from_secs(1));
        index.scan(&project, Duration::ZERO);
        assert!(index.scanning(), "the refresh is reading");
        assert!(index.complete(), "the previous pass stays complete while refreshing");
        assert!(index.walked());
        index.scan(&project, COMMAND_BUDGET);
        assert!(!index.scanning() && index.complete());
    }

    #[test]
    fn on_events_notfound_is_removal() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        indexes.get(&project, COMMAND_BUDGET);
        indexes.update_existing(&project, "/project/src/Gone.java", Some("gone"));
        let outcome = indexes.on_events(&changed(&[], &["/project/src/Gone.java"], &[]), Instant::now() + COMMAND_BUDGET);
        assert_eq!((outcome.updated, outcome.stale), (1, false));
        let index = indexes.get(&project, Duration::ZERO);
        assert!(!index.files().contains_key("/project/src/Gone.java"));
        assert!(index.files().contains_key("/project/src/A.java"));
        assert!(index.complete());
    }

    #[test]
    fn on_events_read_error_fails_only_that_path() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/B.java", "b c");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        indexes.get(&project, COMMAND_BUDGET);
        host.add_file_bytes("src/Latin.java", b"caf\xe9");
        let events = changed(&["/project/src/Latin.java"], &["/project/src/B.java"], &[]);
        let outcome = indexes.on_events(&events, Instant::now() + COMMAND_BUDGET);
        assert_eq!((outcome.updated, outcome.stale), (2, false));
        let index = indexes.get(&project, Duration::ZERO);
        assert_eq!(index.unreadable(), 1);
        assert!(index.walked());
        assert!(!index.complete());
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/A.java", "/project/src/B.java"]);
        host.add_file("src/Latin.java", "now text");
        indexes.on_events(&changed(&[], &["/project/src/Latin.java"], &[]), Instant::now() + COMMAND_BUDGET);
        let index = indexes.get(&project, Duration::ZERO);
        assert_eq!(index.unreadable(), 0);
        assert!(index.complete());
    }

    #[test]
    fn on_events_deadline_marks_stale() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        assert!(indexes.get(&project, COMMAND_BUDGET).fresh());
        host.add_file("src/A.java", "a b");
        let past = Instant::now() - Duration::from_secs(1);
        let outcome = indexes.on_events(&changed(&[], &["/project/src/A.java"], &[]), past);
        assert_eq!((outcome.updated, outcome.stale), (0, true));
        let index = indexes.by_root.get_mut("/project").unwrap();
        assert!(!index.fresh() && !index.complete());
        index.scan(&project, COMMAND_BUDGET);
        assert!(index.fresh(), "the next finished pass clears the stale mark");
        assert_eq!(index.files()["/project/src/A.java"], 2);
    }

    fn kept(_: &str, text: &str) -> Option<usize> {
        (!text.contains("nothing")).then_some(text.len())
    }

    fn java_only(path: &str, _: Option<&str>) -> bool {
        path.ends_with(".java")
    }

    fn broken(_: &str, text: &str) -> bool {
        text.contains("broken")
    }

    #[test]
    fn limits_fail_oversized_and_over_budget_files() {
        let host = MockHost::install();
        host.add_file("src/A.java", "aaaa");
        host.add_file("src/B.java", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        host.add_file("src/C.java", "cccccc");
        host.add_file("src/D.java", "dddddd");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], kept).with_limits(16, 12);
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.files().keys().map(|p| p.rsplit('/').next().unwrap()).collect::<Vec<_>>(), ["A.java", "C.java"]);
        assert_eq!(index.unreadable(), 2, "B is over the file limit, D over the total");
        assert!(!index.complete());
        index.remove("/project/src/C.java");
        index.update("/project/src/D.java", "dddddd");
        assert!(index.files().contains_key("/project/src/D.java"), "removing C freed its bytes");
        assert_eq!(index.unreadable(), 1);
    }

    #[test]
    fn only_required_failures_leave_the_index_incomplete() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("seed.sql", &"x".repeat(64));
        host.add_file_bytes("latin.sql", b"caf\xe9");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java", "**/*.sql"], kept).with_limits(32, 1 << 20).requiring(java_only);
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.unreadable(), 0);
        assert!(index.complete() && index.fresh());
        assert_eq!(coverage_note(&index), None);
        index.update("/project/src/A.java", &"y".repeat(64));
        assert_eq!(index.unreadable(), 1);
        assert!(!index.complete());
    }

    #[test]
    fn rejected_texts_that_parse_to_nothing_count_as_unreadable() {
        let host = MockHost::install();
        host.add_file("src/A.java", "nothing broken");
        host.add_file("src/B.java", "nothing");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], kept).rejecting(broken);
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.unreadable(), 1);
        assert!(index.files().is_empty());
        index.update("/project/src/A.java", "fixed");
        assert!(index.complete());
        assert!(index.files().contains_key("/project/src/A.java"));
    }

    #[test]
    fn seeded_paths_are_read_before_discovery_and_stay_partial() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/New.java", "new");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], words);
        let before = index.generation();
        index.seed(vec!["/project/src/A.java".into()]);
        assert!(index.generation() > before);
        assert_eq!(index.inventory().into_iter().collect::<Vec<_>>(), ["/project/src/A.java"]);
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/A.java"]);
        assert!(index.walked() && !index.complete(), "a restored inventory is not a discovery");
        index.scan(&project, COMMAND_BUDGET);
        assert!(index.complete());
        assert_eq!(index.inventory().into_iter().collect::<Vec<_>>(), ["/project/src/A.java", "/project/src/New.java"]);
        let settled = index.generation();
        index.seed(vec!["/project/src/Gone.java".into()]);
        assert_eq!(index.generation(), settled, "only a cold index takes a seed");
    }

    #[test]
    fn discovery_leaves_nested_projects_to_their_own_index_only_when_asked() {
        let host = MockHost::install();
        host.add_project("/project/inner");
        host.add_file("/project/src/Outer.java", "outer");
        host.add_file("/project/inner/src/Inner.java", "inner");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java"], words);
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.files().len(), 2, "the default keeps nested projects' files");
        let mut index = Index::new(&["**/*.java"], words).excluding_nested_projects();
        index.scan(&project, COMMAND_BUDGET);
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/Outer.java"]);
        assert!(index.complete());
    }

    #[test]
    fn indexes_limits_apply_to_every_project() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/Big.java", "a b c d e f g");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words).with_limits(4, usize::MAX);
        let index = indexes.get(&project, COMMAND_BUDGET);
        assert_eq!(index.files().keys().collect::<Vec<_>>(), ["/project/src/A.java"]);
        assert_eq!(index.unreadable(), 1);
    }

    #[test]
    fn advance_reads_on_without_starting_discovery() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/B.java", "b");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java"], words);
        indexes.advance(&project, COMMAND_BUDGET);
        assert!(indexes.cached("/project/src/A.java").is_none(), "a cold project is not discovered");
        indexes.get(&project, Duration::ZERO);
        host.add_file("src/C.java", "c");
        indexes.advance(&project, COMMAND_BUDGET);
        let index = indexes.cached("/project/src/A.java").unwrap();
        assert!(index.walked() && !index.scanning());
        assert_eq!(index.files().len(), 2, "the pass already under way reads what it discovered");
        indexes.warm(&project, COMMAND_BUDGET, false);
        assert_eq!(indexes.cached("/project/src/A.java").unwrap().files().len(), 2, "a finished pass is not restarted");
    }

    fn slow_words(path: &str, text: &str) -> Option<usize> {
        std::thread::sleep(Duration::from_millis(1));
        words(path, text)
    }

    #[test]
    fn a_large_project_fills_over_several_calls_each_within_its_budget() {
        let host = MockHost::install();
        for n in 0..400 {
            host.add_file(&format!("module{}/src/F{n}.java", n % 20), "a b");
        }
        host.add_file("module0/src/K.kt", "k");
        let project = host.invocation().project().unwrap();
        let mut indexes = Indexes::new(&["**/*.java", "**/*.kt"], slow_words);
        let budget = Duration::from_millis(20);
        let (mut calls, mut read) = (0, 0);
        loop {
            calls += 1;
            let started = Instant::now();
            let index = indexes.get(&project, budget);
            assert!(started.elapsed() < budget + Duration::from_millis(30), "call {calls} took {:?}", started.elapsed());
            assert!(index.files().len() >= read, "call {calls} lost files read earlier");
            read = index.files().len();
            if !index.scanning() {
                break;
            }
            assert!(!index.walked() && !index.complete());
            assert!(calls < 1_000);
        }
        assert!(calls >= 5, "{calls} calls");
        let index = indexes.get(&project, budget);
        assert!(index.complete());
        assert_eq!(index.files().len(), 401);
    }

    #[test]
    fn discovery_lists_one_glob_per_call_once_the_budget_is_spent() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/K.kt", "k");
        let project = host.invocation().project().unwrap();
        let mut index = Index::new(&["**/*.java", "**/*.kt"], words);
        index.scan(&project, Duration::ZERO);
        assert!(index.scanning() && !index.walked());
        assert_eq!(index.inventory().into_iter().collect::<Vec<_>>(), ["/project/src/A.java"]);
        index.scan(&project, Duration::ZERO);
        assert_eq!(index.inventory().len(), 2);
        index.scan(&project, COMMAND_BUDGET);
        assert!(index.complete());
        assert_eq!(index.files().len(), 2);
    }

    #[test]
    fn warm_advances_when_not_fresh() {
        let host = MockHost::install();
        host.add_file("src/A.java", "a");
        host.add_file("src/B.java", "b");
        host.add_file("src/C.java", "c");
        let mut indexes = Indexes::new(&["**/*.java"], words);
        let index = indexes.for_cached_document_warm("/project/src/A.java", "open a", DIAGNOSTICS_WARM).unwrap();
        assert_eq!(index.files().len(), 3);
        assert!(index.fresh());
        assert_eq!(index.files()["/project/src/A.java"], 2, "the open document wins");
    }
}
