//! GitLab for OpenIDEA, ported from JetBrains' GitLab plugin
//! (`org.jetbrains.plugins.gitlab`, intellij-community `plugins/gitlab`,
//! Apache-2.0), against gitlab.com with a personal access token: the merge
//! request list, timeline and review flow, the changes with diff comments and
//! pending reviews, the current branch's merge request in the status bar and
//! the editor, checkout, clone and share, Open on GitLab / Copy Link and
//! Create Snippet.

mod api;
mod branch;
mod ci;
mod forms;
mod repo;
mod review;
mod safe;

use std::collections::{BTreeMap, BTreeSet};

use api::{Approvals, Commit, Discussion, DraftNote, FileDiff, Job, MergeRequest, Note, Pipeline, ProjectInfo, TimelineEvent, User};
use branch::BranchReview;
use openidea_plugin::host::{self, storage};
use openidea_plugin::providers::{CodeAction, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, Plugin, Position, Range, RepositoryStatus, register_plugin, text,
};

const LIST: &str = "merge-requests";
const DETAILS: &str = "merge-request";
const CHANGES: &str = "merge-request-changes";
const PROJECTS: &str = "gitlab-projects";
const NEW_MR: &str = "new-merge-request";
const NEW_SNIPPET: &str = "new-snippet";
const SHARE: &str = "share-project-form";
const COMMITS: &str = "merge-request-commits";
const PIPELINES: &str = "merge-request-pipeline";

const FILTER_KEY: &str = "filter";
const SELECTED_KEY: &str = "selected";
const THREAD_KEY: &str = "thread";
/// The selected reply; without it a selected thread means its first note.
const NOTE_KEY: &str = "note";
const COMMENT_KEY: &str = "comment";
const AS_DRAFT_KEY: &str = "as-draft";
const EVENTS_KEY: &str = "show-events";

const DETACHED: &str = "HEAD is detached. Check out a branch first.";
const NO_TOKEN: &str = "Enter a gitlab.com personal access token (scopes api, read_user) under Settings › Plugins › GitLab.";

/// The original's quick filters, in its order; "assigned" is its default.
const FILTERS: [(&str, &str); 5] = [
    ("open", "Open"),
    ("mine", "Include my changes"),
    ("review", "Need my review"),
    ("assigned", "Assigned to me"),
    ("closed", "Closed"),
];

fn filter() -> &'static str {
    let saved = storage::get(FILTER_KEY).ok().flatten();

    FILTERS.iter().map(|(key, _)| *key).find(|key| saved.as_deref() == Some(*key)).unwrap_or("assigned")
}

fn filter_label(key: &str) -> &'static str {
    FILTERS.iter().find(|(k, _)| *k == key).map(|(_, label)| *label).unwrap_or("Assigned to me")
}

fn next_filter(key: &str) -> &'static str {
    let index = FILTERS.iter().position(|(k, _)| *k == key).unwrap_or(0);

    FILTERS[(index + 1) % FILTERS.len()].0
}

fn filter_query(key: &str, me: &User) -> String {
    match key {
        "open" => "state=opened&scope=all".into(),
        "mine" => "state=opened&scope=created_by_me".into(),
        "review" => format!("state=opened&scope=all&reviewer_username={}", api::encode(&me.username)),
        "closed" => "state=closed&scope=all".into(),
        _ => "state=opened&scope=assigned_to_me".into(),
    }
}

/// A stored on/off preference; `default` until the user toggles it.
fn preference(key: &str, default: bool) -> bool {
    match storage::get(key).ok().flatten().as_deref() {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

/// The window's repository and its gitlab.com project.
struct Repo {
    status: RepositoryStatus,
    /// `group/name`.
    path: String,
    /// The git remote pointing at it (`origin` first, as git does).
    remote: String,
}

fn gitlab_project(ctx: &Invocation) -> Result<Repo, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let status = project.vcs_status()?.ok_or("The project is not in a git repository.")?;

    let (remote, path) = project
        .vcs_remotes()?
        .into_iter()
        .filter_map(|(name, url)| api::project_path(&url).map(|path| (name, path)))
        .min_by_key(|(name, _)| name != "origin")
        .ok_or("The repository has no gitlab.com remote.")?;

    Ok(Repo { status, path, remote })
}

fn relative_path<'a>(root: &str, path: &'a str) -> Option<&'a str> {
    path.strip_prefix(root)?.strip_prefix(['/', '\\'])
}

fn first_line(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() <= 80 {
        return line.to_owned();
    }

    format!("{}…", line.chars().take(79).collect::<String>())
}

/// `words_like_this` as "Words like this".
fn sentence(words: &str) -> String {
    let words = words.replace('_', " ");
    let mut chars = words.chars();

    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn state_label(state: &str) -> String {
    match state {
        "opened" => "Open".into(),
        other => sentence(other),
    }
}

fn merge_status_label(status: &str) -> Option<String> {
    let label = match status {
        "mergeable" => "Ready to merge",
        "ci_must_pass" => "Pipeline must pass",
        "ci_still_running" => "Pipeline still running",
        "discussions_not_resolved" => "Unresolved threads",
        "not_approved" => "Approval required",
        "requested_changes" => "Changes requested",
        "need_rebase" => "Rebase needed",
        "conflict" => "Merge conflicts",
        "blocked_status" => "Blocked by another merge request",
        "external_status_checks" => "External status checks must pass",
        "checking" | "unchecked" | "preparing" | "approvals_syncing" => "Checking mergeability",
        // The state and draft badges say these already.
        "draft_status" | "not_open" => return None,
        other => return Some(sentence(other)),
    };

    Some(label.into())
}

fn pipeline_label(status: &str) -> String {
    let label = match status {
        "success" => "passed",
        "created" | "pending" | "preparing" | "waiting_for_resource" | "scheduled" => "pending",
        "manual" => "waiting for a manual action",
        other => return format!("Pipeline {}", other.replace('_', " ")),
    };

    format!("Pipeline {label}")
}

/// Mergeability and pipeline, next to the state badge.
fn merge_status(mr: &MergeRequest) -> String {
    let merge = mr.detailed_merge_status.as_deref().and_then(merge_status_label);
    let pipeline = mr.head_pipeline.as_ref().map(|p| pipeline_label(&p.status));

    merge.into_iter().chain(pipeline).collect::<Vec<_>>().join(" · ")
}

fn merge_request_icon(mr: &MergeRequest) -> &'static str {
    match mr.state.as_str() {
        _ if mr.draft => "github-pr-draft",
        "merged" => "github-pr-merged",
        "closed" => "github-pr-closed",
        _ => "github-pr-open",
    }
}

/// `2026-01-02T10:00:00Z` as `2026-01-02`.
fn date(timestamp: &str) -> &str {
    timestamp.split('T').next().unwrap_or(timestamp)
}

/// Buttons that run only on a second click, relabelled "Confirm …" in between.
const CONFIRMED: [&str; 4] = ["merge", "close", "delete-note", "cancel"];

const NOTE_ACTIONS: [&str; 3] = ["edit-note", "delete-note", "react"];

fn approvals_text(approvals: &Approvals) -> String {
    let names: Vec<&str> = approvals.approved_by.iter().map(|a| a.user.username.as_str()).collect();
    let approved = match names.is_empty() {
        true => "No approvals".to_owned(),
        false => format!("Approved by {}", names.join(", ")),
    };

    match approvals.approvals_left {
        0 => approved,
        left => format!("{approved} · {left} more required"),
    }
}

/// A key in the merge request tree: `mr:<iid>`, `d:<iid>:<discussion>`, `n:<iid>:<discussion>:<note>`
/// (`e:…` timeline events are not selectable).
#[derive(Debug, PartialEq)]
enum Key<'a> {
    MergeRequest(u64),
    Thread(u64, &'a str),
    Note(u64, &'a str, u64),
}

impl<'a> Key<'a> {
    fn parse(key: &'a str) -> Option<Self> {
        let mut parts = key.split(':');
        let kind = parts.next()?;
        let iid = parts.next()?.parse().ok()?;

        match kind {
            "mr" => Some(Self::MergeRequest(iid)),
            "d" => Some(Self::Thread(iid, parts.next()?)),
            "n" => Some(Self::Note(iid, parts.next()?, parts.next()?.parse().ok()?)),
            _ => None,
        }
    }
}

/// A key in the pipeline tree: `p:<id>`, `s:<pipeline>:<stage>`, `j:<id>`; anything but a `u64` id is no key.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PipelineKey {
    Pipeline(u64),
    Stage(u64),
    Job(u64),
}

impl PipelineKey {
    fn parse(key: &str) -> Option<Self> {
        let (kind, rest) = key.split_once(':')?;

        match kind {
            "p" => Some(Self::Pipeline(rest.parse().ok()?)),
            "s" => Some(Self::Stage(rest.split_once(':')?.0.parse().ok()?)),
            "j" => Some(Self::Job(rest.parse().ok()?)),
            _ => None,
        }
    }
}

/// Most urgent first: a stage shows the first of these that one of its jobs is in.
const STAGE_ORDER: [&str; 12] =
    ["failed", "running", "pending", "preparing", "waiting_for_resource", "created", "scheduled", "canceled", "manual", "warning", "success", "skipped"];
const RETRYABLE_JOB: [&str; 3] = ["failed", "canceled", "success"];
const RETRYABLE_PIPELINE: [&str; 2] = ["failed", "canceled"];
const ACTIVE_PIPELINE: [&str; 6] = ["created", "waiting_for_resource", "preparing", "pending", "running", "scheduled"];

/// A job's status, with an allowed failure as `warning`.
fn job_state(job: &Job) -> &str {
    match job.allow_failure && job.status == "failed" {
        true => "warning",
        false => &job.status,
    }
}

fn status_icon(state: &str) -> &'static str {
    match state {
        "success" => "success",
        "failed" => "error",
        "warning" => "warning",
        "running" => "run",
        "canceled" => "stop",
        "manual" => "pause",
        "skipped" => "minus",
        "created" | "pending" | "preparing" | "waiting_for_resource" | "scheduled" => "clock",
        _ => "info",
    }
}

fn pipelines_enabled() -> bool {
    host::setting_bool("pipelineView").unwrap_or(true)
}

struct Details {
    mr: MergeRequest,
    approvals: Approvals,
    drafts: Vec<DraftNote>,
    remove_source_branch: bool,
    squash: bool,
}

#[derive(Default)]
pub struct GitLab {
    me: Option<User>,
    /// The project and filter `list` was loaded for.
    loaded: Option<(String, String)>,
    list: Vec<MergeRequest>,
    threads: BTreeMap<u64, Vec<Discussion>>,
    events: BTreeMap<u64, Vec<TimelineEvent>>,
    /// Threads whose replies are shown.
    open_threads: BTreeSet<String>,
    details: Option<Details>,
    diffs: Option<(u64, Vec<FileDiff>)>,
    /// The diff row to scroll back to after the changes view is redrawn.
    anchor: Option<String>,
    status: Option<String>,
    branch: Option<BranchReview>,
    default_branches: BTreeMap<String, String>,
    /// Files this plugin published review threads for.
    published: BTreeSet<String>,
    /// The `.gitlab-ci.yml` problems found on the last open or save of each file.
    ci_problems: BTreeMap<String, Vec<Diagnostic>>,
    projects: Option<Vec<ProjectInfo>>,
    /// The `CONFIRMED` button clicked once, waiting for its second click.
    confirm: Option<String>,
    new_merge_request: forms::MergeRequestForm,
    /// The project's branches, the only values the target branch can take.
    branches: Option<Vec<String>>,
    form_status: Option<String>,
    new_snippet: forms::SnippetForm,
    share: forms::ShareForm,
    /// The namespaces the share form offers, loaded when it is first drawn.
    namespaces: Option<Vec<api::Namespace>>,
    /// The reaction picked in the details form, checked against `safe::EMOJIS` when used.
    emoji: Option<String>,
    /// The commit whose diff the changes view shows instead of the merge request's; see [`GitLab::active_commit`].
    commit: Option<String>,
    commits: Option<(u64, Vec<Commit>)>,
    commit_diff: Option<(String, Vec<FileDiff>)>,
    /// The selected merge request's pipelines, newest first, and the jobs of the expanded ones.
    pipelines: Option<(u64, Vec<Pipeline>)>,
    jobs: BTreeMap<u64, Vec<Job>>,
    /// Pipeline tree nodes the user expanded or collapsed; by default the newest pipeline and every stage are open.
    pipeline_open: BTreeMap<String, bool>,
    pipeline_selection: Option<PipelineKey>,
    /// The pipeline the first Cancel click was for; the second click cancels only that one, still selected.
    pipeline_confirm: Option<u64>,
    pipeline_status: Option<String>,
}

impl GitLab {
    fn me(&mut self) -> Result<User, Error> {
        if let Some(me) = &self.me {
            return Ok(me.clone());
        }

        let me = api::current_user()?;
        self.me = Some(me.clone());

        Ok(me)
    }

    fn reset(&mut self) {
        self.loaded = None;
        self.threads.clear();
        self.events.clear();
        self.details = None;
        self.diffs = None;
    }

    fn selected() -> Option<u64> {
        storage::get(SELECTED_KEY).ok().flatten()?.parse().ok()
    }

    fn thread(&self, iid: u64) -> Option<&Discussion> {
        let id = storage::get(THREAD_KEY).ok().flatten()?;

        self.threads.get(&iid)?.iter().find(|d| d.id == id)
    }

    fn selected_note(&self, iid: u64) -> Option<&Note> {
        let thread = self.thread(iid)?;
        let Some(id) = storage::get(NOTE_KEY).ok().flatten() else {
            return thread.notes.first();
        };
        let id: u64 = id.parse().ok()?;

        thread.notes.iter().find(|n| n.id == id)
    }

    /// The selected note's id when a thread is selected, `None` for the merge request itself.
    fn reaction_target(&self, iid: u64) -> Result<Option<u64>, Error> {
        if storage::get(THREAD_KEY)?.is_none() {
            return Ok(None);
        }

        let note = self.selected_note(iid).ok_or("The selected comment is gone. Refresh the merge request.")?;

        Ok(Some(note.id))
    }

    fn own_note(&mut self, iid: u64) -> Result<u64, Error> {
        let me = self.me()?;

        self.selected_note(iid).filter(|n| is_own(n, &me)).map(|n| n.id).ok_or_else(|| "Select one of your own comments first.".into())
    }

    /// What a confirm click is bound to: a delete stays with the note, a merge or close with the merge request
    /// it was first clicked on.
    fn confirm_token(&self, button: &str) -> String {
        let iid = Self::selected();
        let note = iid.and_then(|iid| Some((self.thread(iid)?.id.clone(), self.selected_note(iid)?.id)));

        match (button, iid, note) {
            ("delete-note", _, Some((thread, note))) => format!("{button}:{thread}:{note}"),
            ("merge" | "close", Some(iid), _) => format!("{button}:{iid}"),
            _ => button.to_owned(),
        }
    }

    /// Switches every merge request view to `iid`, dropping what belonged to the previous one.
    fn select_merge_request(&mut self, iid: u64) -> Result<(), Error> {
        storage::set(SELECTED_KEY, &iid.to_string())?;
        storage::delete(COMMENT_KEY)?;
        self.details = None;
        self.anchor = None;
        self.commit = None;
        self.confirm = None;
        self.pipeline_selection = None;
        self.pipeline_confirm = None;
        self.pipeline_status = None;
        self.pipeline_open.clear();
        host::refresh_view(CHANGES);
        host::refresh_view(COMMITS);
        host::refresh_view(PIPELINES);

        Ok(())
    }

    /// Threads and, when shown, the timeline events of `iid`.
    fn load_timeline(&mut self, project: &str, iid: u64) -> Result<(), Error> {
        let api = api::Project(project);
        if !self.threads.contains_key(&iid) {
            self.threads.insert(iid, api.discussions(iid)?);
        }

        if preference(EVENTS_KEY, true) && !self.events.contains_key(&iid) {
            self.events.insert(iid, api.events(iid)?);
        }

        Ok(())
    }

    fn ensure_details(&mut self, project: &str, iid: u64) -> Result<(), Error> {
        if self.details.as_ref().map(|d| d.mr.iid) == Some(iid) {
            return Ok(());
        }

        let _task = host::begin_task("Loading merge request", false);
        let api = api::Project(project);
        let mr = api.merge_request(iid)?;
        let approvals = api.approvals(iid)?;
        let drafts = api.draft_notes(iid)?;
        if !self.threads.contains_key(&iid) {
            self.threads.insert(iid, api.discussions(iid)?);
        }

        self.details = Some(Details {
            remove_source_branch: mr.force_remove_source_branch.unwrap_or(false),
            squash: mr.squash,
            mr,
            approvals,
            drafts,
        });

        Ok(())
    }

    fn timeline_items(&self, mr: &MergeRequest, parent: &str) -> Vec<TreeItem> {
        let mut entries: Vec<(&str, Vec<TreeItem>)> = Vec::new();
        let show_events = preference(EVENTS_KEY, true);

        for d in self.threads.get(&mr.iid).into_iter().flatten() {
            let first = &d.notes[0];
            if d.is_system() {
                if show_events {
                    let key = format!("e:{}:n{}", mr.iid, first.id);
                    let label = format!("{} {}", first.author.username, first_line(&first.body));
                    let item = TreeItem::new(&key, &label).parent(parent).icon("history").detail(date(&first.created_at));
                    entries.push((&first.created_at, vec![item]));
                }
                continue;
            }

            let thread_key = format!("d:{}:{}", mr.iid, d.id);
            let mut detail = vec![first.author.username.clone()];
            match d.notes.len() - 1 {
                0 => {}
                1 => detail.push("1 reply".into()),
                n => detail.push(format!("{n} replies")),
            }

            if d.resolvable() {
                detail.push(if d.resolved() { "resolved" } else { "unresolved" }.into());
            }

            if let Some((path, line)) = first.location() {
                detail.push(format!("{path}:{line}"));
            }

            let icon = if d.resolvable() && d.resolved() { "green-checkmark" } else { "review-comment" };
            let mut item = TreeItem::new(&thread_key, &first_line(&first.body)).parent(parent).icon(icon).detail(&detail.join(" · "));
            if d.notes.len() > 1 {
                item = item.expandable(self.open_threads.contains(&thread_key));
            }

            let mut items = vec![item];
            for n in &d.notes[1..] {
                let note_key = format!("n:{}:{}:{}", mr.iid, d.id, n.id);
                items.push(TreeItem::new(&note_key, &first_line(&n.body)).parent(&thread_key).icon("review-comment").detail(&n.author.username));
            }
            entries.push((&first.created_at, items));
        }

        if show_events {
            for (index, e) in self.events.get(&mr.iid).into_iter().flatten().enumerate() {
                let key = format!("e:{}:{index}", mr.iid);
                let item = TreeItem::new(&key, &format!("{} {}", e.user, e.text)).parent(parent).icon("history").detail(date(&e.created_at));
                entries.push((&e.created_at, vec![item]));
            }
        }

        entries.sort_by(|a, b| a.0.cmp(b.0));
        entries.into_iter().flat_map(|(_, items)| items).collect()
    }

    fn render_list(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let repo = gitlab_project(ctx)?;
        let filter = filter();
        let wanted = (repo.path.clone(), filter.to_owned());

        if self.loaded.as_ref() != Some(&wanted) {
            let _task = host::begin_task("Loading merge requests", false);
            let query = filter_query(filter, &self.me()?);
            self.list = api::Project(&repo.path).merge_requests(&query)?;
            self.loaded = Some(wanted);
        }

        let mut items = Vec::new();
        for mr in &self.list {
            let key = format!("mr:{}", mr.iid);
            let draft = if mr.draft { "Draft · " } else { "" };
            let detail = format!("{draft}{} · {} → {}", mr.author.username, mr.source_branch, mr.target_branch);
            let expanded = self.threads.contains_key(&mr.iid);
            let item = TreeItem::new(&key, &format!("!{} {}", mr.iid, mr.title)).icon(merge_request_icon(mr)).detail(&detail);
            items.push(item.expandable(expanded));

            if !expanded {
                continue;
            }

            let timeline = self.timeline_items(mr, &key);
            // An expanded item without children shows as loading forever.
            if timeline.is_empty() {
                let text = if preference(EVENTS_KEY, true) { "No comments or events." } else { "No comments. Events are hidden." };
                items.push(TreeItem::new(&format!("{key}:none"), text).parent(&key).icon("info"));
            }
            items.extend(timeline);
        }

        let body = match items.is_empty() {
            true => Body::Message(format!("No merge requests match the filter \"{}\".", filter_label(filter))),
            false => Body::Tree(items),
        };
        let events = if preference(EVENTS_KEY, true) { "Hide Events" } else { "Show Events" };

        Ok(View::new(body)
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .button(Button::new("filter", &format!("Filter: {}", filter_label(filter))).icon("filter"))
            .button(Button::new("events", events).icon("github-timeline"))
            .status(&format!("{} · {}", repo.path, filter_label(filter))))
    }

    fn handle_list(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Button(id) if id == "refresh" => {
                self.reset();
                host::refresh_view(DETAILS);
                host::refresh_view(CHANGES);
            }
            ViewEvent::Button(id) if id == "filter" => storage::set(FILTER_KEY, next_filter(filter()))?,
            ViewEvent::Button(id) if id == "events" => {
                storage::set(EVENTS_KEY, &(!preference(EVENTS_KEY, true)).to_string())?;
                let repo = gitlab_project(ctx)?;
                let expanded: Vec<u64> = self.threads.keys().copied().collect();
                for iid in expanded {
                    self.load_timeline(&repo.path, iid)?;
                }
            }
            ViewEvent::Select(key) => {
                let (iid, thread, note) = match Key::parse(&key) {
                    Some(Key::MergeRequest(iid)) => (iid, None, None),
                    Some(Key::Thread(iid, d)) => (iid, Some(d), None),
                    Some(Key::Note(iid, d, n)) => (iid, Some(d), Some(n)),
                    None => return Ok(()),
                };

                if Self::selected() != Some(iid) {
                    self.select_merge_request(iid)?;
                }

                match thread {
                    Some(d) => storage::set(THREAD_KEY, d)?,
                    None => storage::delete(THREAD_KEY)?,
                }

                match note {
                    Some(n) => storage::set(NOTE_KEY, &n.to_string())?,
                    None => storage::delete(NOTE_KEY)?,
                }

                self.status = None;
                self.confirm = None;
                host::refresh_view(DETAILS);
                return Ok(());
            }
            ViewEvent::Expand(key) => {
                let Some(Key::MergeRequest(iid)) = Key::parse(&key) else {
                    self.open_threads.insert(key);
                    host::refresh_view(LIST);
                    return Ok(());
                };

                let repo = gitlab_project(ctx)?;
                let _task = host::begin_task("Loading timeline", false);
                self.load_timeline(&repo.path, iid)?;
            }
            ViewEvent::Collapse(key) => {
                let Some(Key::MergeRequest(iid)) = Key::parse(&key) else {
                    self.open_threads.remove(&key);
                    host::refresh_view(LIST);
                    return Ok(());
                };

                self.threads.remove(&iid);
                self.events.remove(&iid);
            }
            ViewEvent::Activate(key) => return self.activate(&key, ctx),
            _ => return Ok(()),
        }

        host::refresh_view(LIST);

        Ok(())
    }

    /// Double click: a diff thread opens its line, anything else its GitLab page.
    fn activate(&mut self, key: &str, ctx: &Invocation) -> Result<(), Error> {
        let (iid, thread, note) = match Key::parse(key) {
            Some(Key::MergeRequest(iid)) => (iid, None, None),
            Some(Key::Thread(iid, d)) => (iid, Some(d), None),
            Some(Key::Note(iid, d, n)) => (iid, Some(d), Some(n)),
            None => return Ok(()),
        };

        let mr = self.list.iter().find(|mr| mr.iid == iid).ok_or("The merge request is no longer in the list. Refresh it.")?;
        let Some(thread) = thread.and_then(|d| self.threads.get(&iid)?.iter().find(|t| t.id == d)) else {
            return host::open_url(&mr.web_url);
        };

        let first = &thread.notes[0];
        if let Some((path, line)) = first.location() {
            let repo = gitlab_project(ctx)?;
            let opened = host::open_location(&format!("{}/{path}", repo.status.root), line.saturating_sub(1), 0);
            if opened.is_ok() {
                return Ok(());
            }
        }

        host::open_url(&format!("{}#note_{}", mr.web_url, note.unwrap_or(first.id)))
    }

    fn render_details(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the Merge Requests tool window."));
        };

        let repo = gitlab_project(ctx)?;
        self.ensure_details(&repo.path, iid)?;

        let me = self.me()?;
        let thread = self.thread(iid).cloned();
        let details = self.details.as_ref().ok_or("The merge request is not loaded; refresh it.")?;
        let mr = &details.mr;
        let opened = mr.state == "opened";
        let comment = storage::get(COMMENT_KEY)?.unwrap_or_default();
        let as_draft = preference(AS_DRAFT_KEY, true);
        let emoji = note_actions_enabled().then(|| self.emoji.clone().unwrap_or_else(|| safe::EMOJIS[0].to_owned()));
        let fields = details_fields(details, thread.as_ref(), comment, as_draft, emoji);
        let mut view = View::new(Body::Form(fields)).button(Button::new("comment", comment_label(as_draft, thread.is_some())).icon("review-comment"));

        if !details.drafts.is_empty() {
            view = view.button(Button::new("submit", &format!("Submit Review ({})", details.drafts.len())).icon("send"));
        }

        if let Some(thread) = thread.as_ref().filter(|t| t.resolvable()) {
            view = view.button(Button::new("resolve", if thread.resolved() { "Unresolve Thread" } else { "Resolve Thread" }).icon("check"));
        }

        if note_actions_enabled() {
            if self.selected_note(iid).is_some_and(|n| is_own(n, &me)) {
                let delete = match self.confirm == Some(self.confirm_token("delete-note")) {
                    true => "Confirm Delete Comment",
                    false => "Delete Comment",
                };
                view = view.button(Button::new("edit-note", "Save Edit").icon("edit")).button(Button::new("delete-note", delete).icon("trash"));
            }

            let react = if thread.is_some() { "React to Comment" } else { "React" };
            view = view.button(Button::new("react", react).icon("star"));
        }

        if opened {
            if details.approvals.user_has_approved {
                view = view.button(Button::new("unapprove", "Revoke Approval").icon("undo"));
            }

            if details.approvals.user_can_approve && !details.approvals.user_has_approved {
                view = view.button(Button::new("approve", "Approve").icon("github-review-accepted"));
            }

            let can_merge = mr.user.as_ref().is_some_and(|u| u.can_merge);
            let mergeable = mr.detailed_merge_status.as_deref() == Some("mergeable");
            let merge = match self.confirm == Some(format!("merge:{}", mr.iid)) {
                true => format!("Confirm Merge !{} into {}", mr.iid, mr.target_branch),
                false => "Merge".into(),
            };
            view = view.button(Button::new("merge", &merge).icon("github-pr-merged").enabled(can_merge && mergeable));

            if mr.detailed_merge_status.as_deref() == Some("need_rebase") {
                view = view.button(Button::new("rebase", "Rebase"));
            }

            if !mr.reviewers.iter().any(|u| u.id == me.id) {
                view = view.button(Button::new("review", "Add Me as Reviewer").icon("github-review"));
            }

            let (draft, draft_icon) = match mr.draft {
                true => ("Mark as Ready", "github-pr-open"),
                false => ("Mark as Draft", "github-pr-draft"),
            };
            let close = match self.confirm == Some(format!("close:{}", mr.iid)) {
                true => format!("Confirm Close !{}", mr.iid),
                false => "Close".into(),
            };
            view = view
                .button(Button::new("draft", draft).icon(draft_icon))
                .button(Button::new("close", &close).icon("github-pr-closed"));
        }

        if mr.state == "closed" {
            view = view.button(Button::new("reopen", "Reopen").icon("github-pr-open"));
        }

        if repo.status.branch.as_deref() != Some(branch::local_branch(mr).as_str()) && mr.state != "merged" {
            view = view.button(Button::new("checkout", "Check Out Branch").icon("branch"));
        }

        view = view
            .button(Button::new("browser", "Open on GitLab").icon("external-link"))
            .button(Button::new("copy", "Copy Link").icon("copy"))
            .button(Button::new("refresh", "Refresh").icon("refresh"));

        let summary = details_summary(details);
        Ok(match &self.status {
            Some(status) => view.status(&format!("{status} · {summary}")),
            None => view.status(&summary),
        })
    }

    fn handle_details(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let button = match event {
            ViewEvent::Button(button) => button,
            ViewEvent::Field(key, value) => return self.field(&key, &value),
            _ => return Ok(()),
        };

        if NOTE_ACTIONS.contains(&button.as_str()) && !note_actions_enabled() {
            return Ok(());
        }

        let pending = self.confirm.take();
        let token = self.confirm_token(&button);
        if CONFIRMED.contains(&button.as_str()) && pending.as_deref() != Some(token.as_str()) {
            self.confirm = Some(token);
            host::refresh_view(DETAILS);
            return Ok(());
        }

        if pending.is_some() {
            host::refresh_view(DETAILS);
        }

        if button == "refresh" {
            if let Some(iid) = Self::selected() {
                self.threads.remove(&iid);
            }

            self.details = None;
            self.status = None;
            host::refresh_view(DETAILS);
            return Ok(());
        }

        let Some(details) = &self.details else {
            return Ok(());
        };

        let mr = details.mr.clone();
        match button.as_str() {
            "browser" => return host::open_url(&mr.web_url),
            "copy" => return copy_link(&mr.web_url),
            _ => {}
        }

        let repo = gitlab_project(ctx)?;
        let _task = host::begin_task("Updating merge request", false);
        let result = self.act(&repo, &button, &mr, ctx);

        self.status = Some(match result {
            Ok(done) => done,
            Err(e) => e.to_string(),
        });
        self.loaded = None;
        self.reload_merge_request(mr.iid);

        Ok(())
    }

    fn field(&mut self, key: &str, value: &str) -> Result<(), Error> {
        let checked = value == "true";

        match (key, self.details.as_mut()) {
            (COMMENT_KEY, _) => storage::set(COMMENT_KEY, value)?,
            (AS_DRAFT_KEY, _) => {
                storage::set(AS_DRAFT_KEY, value)?;
                host::refresh_view(DETAILS);
            }
            ("remove-source-branch", Some(d)) => d.remove_source_branch = checked,
            ("squash", Some(d)) => d.squash = checked,
            ("emoji", _) => self.emoji = Some(value.to_owned()),
            _ => {}
        }

        Ok(())
    }

    fn act(&mut self, repo: &Repo, button: &str, mr: &MergeRequest, ctx: &Invocation) -> Result<String, Error> {
        let api = api::Project(&repo.path);
        let details = self.details.as_ref().ok_or("The merge request is not loaded; refresh it.")?;
        let done = |message: &str| Ok(message.to_owned());

        match button {
            "comment" => {
                let body = storage::get(COMMENT_KEY)?.unwrap_or_default();
                if body.trim().is_empty() {
                    return Err("Write a comment first.".into());
                }

                let thread = self.thread(mr.iid).map(|t| t.id.clone());
                match (preference(AS_DRAFT_KEY, true), thread) {
                    (true, thread) => api.create_draft(mr.iid, &body, None, thread.as_deref())?,
                    (false, Some(thread)) => api.reply(mr.iid, &thread, &body)?,
                    (false, None) => api.discuss(mr.iid, &body, None)?,
                }

                storage::delete(COMMENT_KEY)?;
                done(if preference(AS_DRAFT_KEY, true) { "Added to your review." } else { "Comment posted." })
            }
            "submit" => {
                api.publish_drafts(mr.iid)?;
                let summary = storage::get(COMMENT_KEY)?.unwrap_or_default();
                if !summary.trim().is_empty() {
                    api.discuss(mr.iid, &summary, None)?;
                    storage::delete(COMMENT_KEY)?;
                }

                done("Review submitted.")
            }
            "edit-note" => {
                let note = self.own_note(mr.iid)?;
                let body = storage::get(COMMENT_KEY)?.unwrap_or_default();
                if body.trim().is_empty() {
                    return Err("Write the new text in the comment box first.".into());
                }

                api.edit_note(mr.iid, note, &body)?;
                storage::delete(COMMENT_KEY)?;
                done("Comment edited.")
            }
            "delete-note" => {
                let note = self.own_note(mr.iid)?;
                api.delete_note(mr.iid, note).and_then(|_| done("Comment deleted."))
            }
            "react" => {
                let emoji = safe::emoji(self.emoji.as_deref().unwrap_or(safe::EMOJIS[0]))?;
                let note = self.reaction_target(mr.iid)?;
                let me = self.me()?;
                let mine = api.awards(mr.iid, note)?.into_iter().find(|a| a.name == emoji && me.id != 0 && a.user.id == me.id);

                match mine {
                    Some(award) => api.remove_award(mr.iid, note, award.id).map(|_| format!("Removed your :{emoji}:.")),
                    None => api.add_award(mr.iid, note, emoji).map(|_| format!("Reacted with :{emoji}:.")),
                }
            }
            "resolve" => {
                let thread = self.thread(mr.iid).ok_or("Select a thread first.")?;
                let resolve = !thread.resolved();
                api.resolve(mr.iid, &thread.id, resolve)?;
                done(if resolve { "Thread resolved." } else { "Thread unresolved." })
            }
            "approve" => api.approve(mr.iid).and_then(|_| done("Approved.")),
            "unapprove" => api.unapprove(mr.iid).and_then(|_| done("Approval revoked.")),
            "merge" => api.merge(mr, details.remove_source_branch, details.squash).and_then(|_| done("Merged.")),
            "rebase" => api.rebase(mr.iid).and_then(|_| done("Rebase started.")),
            "draft" => api.set_draft(mr.iid, !mr.draft).and_then(|_| done(if mr.draft { "Marked as ready." } else { "Marked as draft." })),
            "close" => api.set_state(mr.iid, "close").and_then(|_| done("Closed.")),
            "reopen" => api.set_state(mr.iid, "reopen").and_then(|_| done("Reopened.")),
            "review" => {
                let me = self.me()?;
                let ids: Vec<u64> = mr.reviewers.iter().map(|u| u.id).chain([me.id]).collect();
                api.set_reviewers(mr.iid, &ids).and_then(|_| done("Added you as a reviewer."))
            }
            "checkout" => {
                let project = ctx.project().ok_or("No project is open.")?;
                branch::checkout(&project, &repo.remote, mr).map_err(|e| match e {
                    Error::Denied(_) => "Allow git for GitLab in Settings › Plugins to check out branches.".into(),
                    e => e,
                })
            }
            _ => Err(Error::NotFound(format!("button {button}"))),
        }
    }

    fn handle_changes(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Message(message) => self.handle_changes_message(&message, ctx),
            ViewEvent::Button(id) if id == "refresh" => {
                self.diffs = None;
                self.commit_diff = None;
                self.status = None;
                match Self::selected() {
                    Some(iid) => self.reload_merge_request(iid),
                    None => host::refresh_view(CHANGES),
                }
                Ok(())
            }
            ViewEvent::Button(id) if id == "submit" || id == "browser" => self.handle_details(ViewEvent::Button(id), ctx),
            ViewEvent::Button(id) if id == "all-changes" => self.handle_commits(ViewEvent::Button(id)),
            _ => Ok(()),
        }
    }
}

fn comment_label(as_draft: bool, reply: bool) -> &'static str {
    match (as_draft, reply) {
        (true, _) => "Add to Review",
        (false, true) => "Reply",
        (false, false) => "Comment",
    }
}

/// State, branches, author, mergeability, pipeline and approvals on one line.
fn details_summary(details: &Details) -> String {
    let mr = &details.mr;
    let state = match mr.draft {
        true => format!("{} · Draft", state_label(&mr.state)),
        false => state_label(&mr.state),
    };
    let parts = [
        state,
        format!("{} → {}", mr.source_branch, mr.target_branch),
        mr.author.username.clone(),
        merge_status(mr),
        approvals_text(&details.approvals),
    ];

    parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ")
}

/// The comment and merge options as inputs; the description and replied-to note are shown, and edits to them ignored.
fn details_fields(details: &Details, thread: Option<&Discussion>, comment: String, as_draft: bool, emoji: Option<String>) -> Vec<Field> {
    let mut fields = Vec::new();

    if let Some(thread) = thread {
        let first = &thread.notes[0];
        let note = format!("{}: {}", first.author.username, first_line(&first.body));
        fields.push(Field::new("replying-to", "Replying to (read-only)", FieldValue::Text(note)));
    }

    let label = if thread.is_some() { "Reply" } else { "Comment" };
    fields.push(Field::new(COMMENT_KEY, label, FieldValue::Multiline(comment)));
    fields.push(Field::new(AS_DRAFT_KEY, "Add to my review (published when you submit the review)", FieldValue::Checkbox(as_draft)));

    if let Some(emoji) = emoji {
        fields.push(Field::new("emoji", "Reaction", FieldValue::Choice(safe::EMOJIS.map(str::to_owned).to_vec(), emoji)));
    }

    if details.mr.state == "opened" {
        fields.push(Field::new("remove-source-branch", "Delete source branch on merge", FieldValue::Checkbox(details.remove_source_branch)));
        fields.push(Field::new("squash", "Squash commits on merge", FieldValue::Checkbox(details.squash)));
    }

    if let Some(description) = details.mr.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        fields.push(Field::new("description", "Description (read-only)", FieldValue::Multiline(description.to_owned())));
    }

    fields
}

/// Only your own comments can be edited or deleted; GitLab writes system notes. An unknown id (0) owns nothing.
fn is_own(note: &Note, me: &User) -> bool {
    me.id != 0 && note.author.id == me.id && !note.system
}

fn note_actions_enabled() -> bool {
    host::setting_bool("noteActions").unwrap_or(true)
}

fn commits_enabled() -> bool {
    host::setting_bool("commitsView").unwrap_or(true)
}

fn forms_enabled() -> bool {
    host::setting_bool("forms").unwrap_or(true)
}

fn copy_link(url: &str) -> Result<(), Error> {
    match host::copy_to_clipboard(url) {
        Err(Error::Denied(_)) => {
            host::notify(NotificationLevel::Info, "GitLab link", url);
            Ok(())
        }
        other => other,
    }
}

/// The selection's 0-based line range; a selection ending at a line start stops on the line before.
fn selected_lines(editor: &openidea_plugin::TextEditor) -> Option<(u32, u32)> {
    let selection = *editor.selections().ok()?.first()?;
    let (start, end) = (selection.start.line, selection.end.line);

    match end > start && selection.end.character == 0 {
        true => Some((start, end - 1)),
        false => Some((start, end)),
    }
}

fn file_link(ctx: &Invocation) -> Result<String, Error> {
    let editor = ctx.editor().ok_or("No file is open.")?;
    let repo = gitlab_project(ctx)?;
    let path = editor.path();
    let relative = relative_path(&repo.status.root, &path).ok_or("The file is not in the repository.")?;
    let revision = repo.status.commit.as_deref().ok_or("The repository has no commits.")?;

    Ok(api::blob_url(&repo.path, revision, &relative.replace('\\', "/"), selected_lines(&editor)))
}

/// The focused file's repository path (its name outside the repository) and the selection, or the whole file.
fn snippet_content(ctx: &Invocation) -> Result<(String, String), Error> {
    let editor = ctx.editor().ok_or("No file is open.")?;
    let document = editor.document()?;
    let selected = editor
        .selections()?
        .first()
        .and_then(|r| document.text_in(*r))
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    let content = selected.unwrap_or(document.text);
    if content.is_empty() {
        return Err("The file is empty.".into());
    }

    let path = editor.path();
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path);
    let root = ctx.project().and_then(|p| p.vcs_status().ok().flatten()).map(|s| s.root);
    let file_path = root.as_deref().and_then(|r| relative_path(r, &path)).unwrap_or(name).replace('\\', "/");

    Ok((file_path, content))
}

fn post_snippet(draft: &api::SnippetDraft) -> Result<(), Error> {
    let _task = host::begin_task("Creating snippet", false);
    let url = api::create_snippet(draft)?;
    let copied = host::copy_to_clipboard(&url).is_ok();
    let body = if copied { format!("{url}\nLink copied to the clipboard.") } else { url };
    host::notify(NotificationLevel::Info, "Snippet created", &body);

    Ok(())
}

fn create_snippet(ctx: &Invocation) -> Result<Option<String>, Error> {
    if !host::secret_is_set(api::TOKEN) {
        return Err(NO_TOKEN.into());
    }

    let (file_path, content) = snippet_content(ctx)?;
    let title = file_path.rsplit('/').next().unwrap_or(&file_path).to_owned();
    post_snippet(&api::SnippetDraft { title, description: String::new(), visibility: "private".into(), file_path, content, project: None })?;

    Ok(None)
}

/// Unverified that the host sets `ctx.editor()` while a view button is clicked; without it the form says so.
fn focused_snippet(ctx: &Invocation) -> Result<(String, String), Error> {
    if ctx.editor().is_none() {
        return Err("Focus a file first.".into());
    }

    snippet_content(ctx)
}

/// Posts and returns `None`, or returns why it waits or refused. A public snippet posts only on a confirm
/// of the same file and content its warning named, so a focus change between the clicks publishes nothing.
fn snippet_click(form: &mut forms::SnippetForm, confirming: Option<(String, String)>, ctx: &Invocation) -> Result<Option<String>, Error> {
    let focused = focused_snippet(ctx)?;
    if form.visibility == "public" {
        match confirming {
            None => {
                let warning = format!("Publish {} publicly? Anyone can see a public snippet, signed in or not. Click Confirm public.", focused.0);
                form.confirm_public = Some(focused);
                return Ok(Some(warning));
            }
            Some(shown) if shown != focused => {
                return Ok(Some("The focused file or selection changed since the warning. Nothing was published; click Create Snippet again.".into()));
            }
            Some(_) => {}
        }
    }

    let (path, content) = focused;
    post_snippet(&form.draft(path, content, &snippet_scopes(ctx))?)?;

    Ok(None)
}

/// `Personal`, and `Project <group/name>` when the repository has a gitlab.com remote.
fn snippet_scopes(ctx: &Invocation) -> Vec<String> {
    let project = gitlab_project(ctx).ok().map(|repo| format!("Project {}", repo.path));

    std::iter::once(forms::PERSONAL.to_owned()).chain(project).collect()
}

impl Plugin for GitLab {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        api::begin_budget();
        match id {
            "new-token" => host::open_url(&api::new_token_url())?,
            "open-in-browser" => host::open_url(&file_link(ctx)?)?,
            "copy-link" => copy_link(&file_link(ctx)?)?,
            "create-snippet" => return create_snippet(ctx),
            "share-project" => {
                if !host::secret_is_set(api::TOKEN) {
                    return Err(NO_TOKEN.into());
                }

                return repo::share(ctx);
            }
            "show-branch-merge-request" => return self.show_branch_merge_request(),
            "open-merge-request-in-worktree" => return self.open_worktree(ctx),
            "update-merge-request-branch" => return self.update_merge_request_branch(ctx),
            "create-merge-request" => {
                let repo = gitlab_project(ctx)?;
                let branch = repo.status.branch.ok_or("HEAD is detached. Check out a branch first.")?;
                host::open_url(&api::new_merge_request_url(&repo.path, &branch))?;
            }
            _ => return Err(Error::NotFound(format!("command {id}"))),
        }

        Ok(None)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        api::begin_budget();
        self.check_ci_files(events, ctx);
        if !host::secret_is_set(api::TOKEN) {
            return;
        }

        let repository_changed = events.iter().any(|e| matches!(e, Event::VcsChanged(_)));
        if let Err(e) = self.update_branch(ctx, repository_changed) {
            host::log(LogLevel::Warn, &format!("merge request of the current branch: {e}"));
        }

        for event in events {
            if let Event::FileOpened(path) = event {
                self.publish_review(path);
            }
        }
    }
}

impl GitLab {
    /// The merge request the commands act on: the selected one, else the current branch's.
    fn command_target(&self) -> Result<u64, Error> {
        let branch_iid = self.branch.as_ref().and_then(|b| b.mr.as_ref()).map(|mr| mr.iid);

        Self::selected().or(branch_iid).ok_or_else(|| "Select a merge request first.".into())
    }

    fn open_worktree(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::TOKEN) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        let project = ctx.project().ok_or("No project is open.")?;
        let mr = api::Project(&repo.path).merge_request(self.command_target()?)?;
        let plan = branch::worktree_plan(&repo.status.root, &repo.remote, &mr)?;

        let _task = host::begin_task("Adding worktree", false);
        let run = |args: &Vec<String>| branch::git_ok(&project, &args.iter().map(String::as_str).collect::<Vec<_>>());
        let added = run(&plan.fetch).and_then(|_| run(&plan.add));

        match added {
            Ok(_) => host::notify(NotificationLevel::Info, "Worktree added", &format!("!{} is checked out in {}", mr.iid, plan.path)),
            Err(Error::Denied(_)) => host::notify(NotificationLevel::Warning, "Worktree not added", "Allow git for GitLab in Settings › Plugins to add worktrees."),
            Err(e) => return Err(e),
        }

        Ok(None)
    }

    fn update_merge_request_branch(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::TOKEN) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        let iid = self.command_target()?;
        let api = api::Project(&repo.path);
        api.rebase(iid)?;

        let state = api.rebase_state(iid)?;
        let error = state.merge_error.filter(|_| !state.rebase_in_progress);
        if let Some(error) = error.filter(|e| !e.is_empty()) {
            return Err(Error::Failed(format!("Rebase of !{iid} failed: {error}")));
        }

        Ok(Some(format!("Rebase of !{iid} requested.")))
    }

    fn render_new_merge_request(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let repo = gitlab_project(ctx)?;
        let source = safe::ref_name(repo.status.branch.as_deref().ok_or(DETACHED)?)?.to_owned();
        let branches = match self.branches.clone() {
            Some(branches) => branches,
            None => {
                let _task = host::begin_task("Loading branches", false);
                let branches = api::Project(&repo.path).branches()?;
                self.branches = Some(branches.clone());
                branches
            }
        };

        if !branches.contains(&self.new_merge_request.target) {
            let default = self.default_branch(&repo.path)?;
            self.new_merge_request.target = match branches.contains(&default) {
                true => default,
                false => branches.first().cloned().unwrap_or_default(),
            };
        }

        let mut view = self.new_merge_request.render(&branches, &source);
        if let (Some(status), Some(summary)) = (&self.form_status, &mut view.status) {
            *summary = format!("{status} · {summary}");
        }

        Ok(view)
    }

    fn handle_new_merge_request(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Field(key, value) => {
                let listed = self.branches.iter().flatten().any(|b| *b == value);
                if key != "target-branch" || listed {
                    self.new_merge_request.field(&key, &value);
                }
                return Ok(());
            }
            ViewEvent::Button(id) if id == "refresh" => {
                self.branches = None;
                self.form_status = None;
            }
            ViewEvent::Button(id) if id == "create" => self.form_status = self.create_merge_request(ctx).err().map(|e| e.to_string()),
            _ => return Ok(()),
        }

        host::refresh_view(NEW_MR);

        Ok(())
    }

    fn create_merge_request(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let repo = gitlab_project(ctx)?;
        let source = repo.status.branch.as_deref().ok_or(DETACHED)?;
        let branches = self.branches.as_deref().ok_or("The branches are not loaded. Refresh the form.")?;

        let _task = host::begin_task("Creating merge request", false);
        let new = self.new_merge_request.request(source, branches)?;
        let mr = api::Project(&repo.path).create_merge_request(&new)?;
        host::notify(NotificationLevel::Info, "Merge request created", &format!("!{} {}\n{}", mr.iid, mr.title, mr.web_url));

        self.new_merge_request = forms::MergeRequestForm::default();
        self.loaded = None;
        host::refresh_view(LIST);

        Ok(())
    }

    fn handle_new_snippet(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let form = &mut self.new_snippet;
        let pending = form.confirm_public.take();
        match event {
            ViewEvent::Field(key, value) => {
                let was_pending = pending.is_some();
                form.confirm_public = pending;
                if key != "scope" || snippet_scopes(ctx).contains(&value) {
                    form.field(&key, &value);
                }
                if form.confirm_public.is_some() || !was_pending {
                    return Ok(());
                }
                form.status = None;
            }
            ViewEvent::Button(id) if id == "create" || (id == "confirm-public" && pending.is_some()) => {
                let confirming = pending.filter(|_| id == "confirm-public");
                match snippet_click(form, confirming, ctx) {
                    Ok(None) => self.new_snippet = forms::SnippetForm::default(),
                    Ok(Some(text)) => form.status = Some(text),
                    Err(e) => form.status = Some(e.to_string()),
                }
            }
            _ => {
                form.confirm_public = pending;
                return Ok(());
            }
        }

        host::refresh_view(NEW_SNIPPET);

        Ok(())
    }

    fn render_share(&mut self) -> Result<View, Error> {
        if self.namespaces.is_none() {
            let _task = host::begin_task("Loading GitLab namespaces", false);
            self.namespaces = Some(api::namespaces()?);
        }

        Ok(self.share.render(self.namespaces.as_deref().unwrap_or_default()))
    }

    fn handle_share(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let namespaces = self.namespaces.as_deref().unwrap_or_default();
        let form = &mut self.share;
        match event {
            ViewEvent::Field(key, value) => {
                let was_pending = form.confirm_commit.is_some();
                if key != "namespace" || forms::namespace_choices(namespaces).contains(&value) {
                    form.field(&key, &value);
                }
                if !was_pending || form.confirm_commit.is_some() {
                    return Ok(());
                }
                form.status = None;
            }
            ViewEvent::Button(id) if id == "refresh" => {
                self.namespaces = None;
                form.status = None;
                form.confirm_commit = None;
            }
            ViewEvent::Button(id) if id == "share" || (id == "confirm-commit" && form.confirm_commit.is_some()) => {
                let shown = form.confirm_commit.take().filter(|_| id == "confirm-commit");
                let shared = form.namespace_id(namespaces).and_then(|namespace| repo::share_with(ctx, form, namespace, shown.as_deref()));
                match shared {
                    Ok(None) => *form = forms::ShareForm::default(),
                    Ok(Some(files)) => {
                        let count = match files.len() {
                            1 => "1 file".to_owned(),
                            n => format!("{n} files"),
                        };
                        form.status = Some(match shown {
                            Some(_) => format!("The files changed since the list was shown; nothing was committed. Check the {count} below and confirm again."),
                            None => format!("Commit {count} as \"Initial commit\"? Check the list below, then click Confirm commit."),
                        });
                        form.confirm_commit = Some(files);
                    }
                    Err(e) => form.status = Some(e.to_string()),
                }
            }
            _ => return Ok(()),
        }

        host::refresh_view(SHARE);

        Ok(())
    }

    /// The picked commit while it belongs to the selected merge request and the view is on.
    fn active_commit(&self) -> Option<&Commit> {
        let sha = self.commit.as_deref().filter(|_| commits_enabled())?;
        let (iid, commits) = self.commits.as_ref()?;
        if Self::selected() != Some(*iid) {
            return None;
        }

        commits.iter().find(|c| c.id == sha)
    }

    fn render_commits(&mut self, ctx: &Invocation) -> Result<View, Error> {
        if !commits_enabled() {
            return Ok(View::message("The commit list is off. Turn it on under Settings › Plugins › GitLab."));
        }

        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the Merge Requests tool window."));
        };

        if self.commits.as_ref().map(|(i, _)| *i) != Some(iid) {
            let repo = gitlab_project(ctx)?;
            let _task = host::begin_task("Loading commits", false);
            self.commits = Some((iid, api::Project(&repo.path).mr_commits(iid)?));
        }

        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let commits = self.commits.as_ref().map(|(_, c)| c.as_slice()).unwrap_or_default();
        if commits.is_empty() {
            return Ok(View::message(&format!("!{iid} has no commits.")).button(refresh));
        }

        let items = commits
            .iter()
            .map(|c| TreeItem::new(&c.id, &format!("{} {}", c.short_id, first_line(&c.title))).icon("commit").detail(&c.author_name))
            .collect();
        let view = View::new(Body::Tree(items)).button(refresh);

        Ok(match self.active_commit() {
            Some(c) => view
                .button(Button::new("all-changes", "All Changes").icon("arrow-left"))
                .status(&format!("Showing {} {} in Merge Request Changes", c.short_id, first_line(&c.title))),
            None => view.status(&format!("!{iid} · double-click a commit to see its changes")),
        })
    }

    /// Only a hex SHA listed for the selected merge request is ever picked, so a row key never reaches a URL unchecked.
    fn handle_commits(&mut self, event: ViewEvent) -> Result<(), Error> {
        if !commits_enabled() {
            return Ok(());
        }

        match event {
            ViewEvent::Activate(key) => {
                let Ok(sha) = safe::hex_sha(&key) else { return Ok(()) };
                let listed = self.commits.as_ref().is_some_and(|(iid, commits)| Self::selected() == Some(*iid) && commits.iter().any(|c| c.id == sha));
                if !listed {
                    return Ok(());
                }

                self.commit = Some(sha.to_owned());
            }
            ViewEvent::Button(id) if id == "all-changes" => self.commit = None,
            ViewEvent::Button(id) if id == "refresh" => self.commits = None,
            _ => return Ok(()),
        }

        self.anchor = None;
        host::refresh_view(COMMITS);
        host::refresh_view(CHANGES);

        Ok(())
    }

    /// The selected merge request's listed pipelines; none while another one's are loaded.
    fn listed_pipelines(&self) -> &[Pipeline] {
        match &self.pipelines {
            Some((iid, pipelines)) if Self::selected() == Some(*iid) => pipelines,
            _ => &[],
        }
    }

    /// The selected pipeline, or the selected job and its pipeline, while they are listed.
    fn pipeline_target(&self) -> (Option<Pipeline>, Option<Job>) {
        let listed = self.listed_pipelines();

        match self.pipeline_selection {
            Some(PipelineKey::Pipeline(id)) => (listed.iter().find(|p| p.id == id).cloned(), None),
            Some(PipelineKey::Job(id)) => listed
                .iter()
                .find_map(|p| Some((p.clone(), self.jobs.get(&p.id)?.iter().find(|j| j.id == id)?.clone())))
                .map_or((None, None), |(p, j)| (Some(p), Some(j))),
            _ => (None, None),
        }
    }

    fn render_pipelines(&mut self, ctx: &Invocation) -> Result<View, Error> {
        if !pipelines_enabled() {
            return Ok(View::message("The pipeline view is off. Turn it on under Settings › Plugins › GitLab."));
        }

        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the Merge Requests tool window."));
        };

        let repo = gitlab_project(ctx)?;
        let api = api::Project(&repo.path);
        if self.pipelines.as_ref().map(|(i, _)| *i) != Some(iid) {
            let _task = host::begin_task("Loading pipelines", false);
            self.jobs.clear();
            self.pipelines = Some((iid, api.mr_pipelines(iid)?));
        }

        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let pipelines = self.listed_pipelines().to_vec();
        if pipelines.is_empty() {
            return Ok(View::message(&format!("!{iid} has no pipelines.")).button(refresh));
        }

        let mut items = Vec::new();
        for (n, pipeline) in pipelines.iter().enumerate() {
            let key = format!("p:{}", pipeline.id);
            let open = self.pipeline_open.get(&key).copied().unwrap_or(n == 0);
            let label = format!("#{}", pipeline.id);
            items.push(TreeItem::new(&key, &label).icon(status_icon(&pipeline.status)).detail(&pipeline_label(&pipeline.status)).expandable(open));
            if !open {
                continue;
            }

            if !self.jobs.contains_key(&pipeline.id) {
                let _task = host::begin_task("Loading jobs", false);
                self.jobs.insert(pipeline.id, api.pipeline_jobs(pipeline.id)?);
            }

            items.extend(self.job_items(pipeline.id, &key));
        }

        let (pipeline, job) = self.pipeline_target();
        let retry = match (&pipeline, &job) {
            (_, Some(j)) => Button::new("retry", "Retry Job").enabled(RETRYABLE_JOB.contains(&j.status.as_str())),
            (Some(p), None) => Button::new("retry", "Retry Pipeline").enabled(RETRYABLE_PIPELINE.contains(&p.status.as_str())),
            _ => Button::new("retry", "Retry").enabled(false),
        };
        let cancel = match self.pipeline_confirm.filter(|id| pipeline.as_ref().map(|p| p.id) == Some(*id)) {
            Some(id) => format!("Confirm Cancel Pipeline #{id}"),
            None => "Cancel Pipeline".into(),
        };
        let cancellable = pipeline.as_ref().is_some_and(|p| ACTIVE_PIPELINE.contains(&p.status.as_str()));
        let status = match &self.pipeline_status {
            Some(status) => status.clone(),
            None => format!("!{iid} · select a pipeline or job"),
        };

        Ok(View::new(Body::Tree(items))
            .button(retry.icon("rerun"))
            .button(Button::new("cancel", &cancel).icon("stop").enabled(cancellable))
            .button(Button::new("browser", "Open on GitLab").icon("external-link").enabled(pipeline.is_some()))
            .button(refresh)
            .status(&status))
    }

    /// Stages in the order their first job was created, an `info` row when there are no jobs.
    fn job_items(&self, pipeline: u64, parent: &str) -> Vec<TreeItem> {
        let mut jobs: Vec<&Job> = self.jobs.get(&pipeline).into_iter().flatten().collect();
        if jobs.is_empty() {
            return vec![TreeItem::new(&format!("i:{pipeline}"), "No jobs").parent(parent).icon("info")];
        }

        jobs.sort_by_key(|j| j.id);
        let mut stages: Vec<&str> = Vec::new();
        for job in &jobs {
            if !stages.contains(&job.stage.as_str()) {
                stages.push(&job.stage);
            }
        }

        let mut items = Vec::new();
        for stage in stages {
            let key = format!("s:{pipeline}:{stage}");
            let in_stage: Vec<&Job> = jobs.iter().copied().filter(|j| j.stage == stage).collect();
            let state = STAGE_ORDER.into_iter().find(|s| in_stage.iter().any(|j| job_state(j) == *s)).unwrap_or_else(|| job_state(in_stage[0]));
            let open = self.pipeline_open.get(&key).copied().unwrap_or(true);
            items.push(TreeItem::new(&key, stage).parent(parent).icon(status_icon(state)).expandable(open));
            if !open {
                continue;
            }

            for job in in_stage {
                let detail = match job_state(job) {
                    "warning" => "Failed (allowed to fail)".to_owned(),
                    _ => sentence(&job.status),
                };
                items.push(TreeItem::new(&format!("j:{}", job.id), &job.name).parent(&key).icon(status_icon(job_state(job))).detail(&detail));
            }
        }

        items
    }

    /// Acts only on a listed pipeline or job; a stale or unparsable selection sends nothing.
    fn handle_pipelines(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if !pipelines_enabled() {
            return Ok(());
        }

        let pending = self.pipeline_confirm.take();
        match event {
            ViewEvent::Select(key) => {
                let selection = PipelineKey::parse(&key);
                if selection == self.pipeline_selection {
                    self.pipeline_confirm = pending;
                    return Ok(());
                }

                self.pipeline_selection = selection;
                self.pipeline_status = None;
            }
            ViewEvent::Expand(key) | ViewEvent::Collapse(key) if PipelineKey::parse(&key).is_none() => self.pipeline_confirm = pending,
            ViewEvent::Expand(key) => {
                self.pipeline_open.insert(key, true);
                self.pipeline_confirm = pending;
            }
            ViewEvent::Collapse(key) => {
                self.pipeline_open.insert(key, false);
                self.pipeline_confirm = pending;
            }
            ViewEvent::Button(id) if id == "refresh" => {
                self.pipelines = None;
                self.pipeline_status = None;
            }
            ViewEvent::Button(id) if id == "browser" => {
                let (pipeline, job) = self.pipeline_target();
                let url = job.map(|j| j.web_url).or(pipeline.map(|p| p.web_url));
                let opened = match &url {
                    Some(url) => safe::gitlab_url(url).and_then(host::open_url),
                    None => Err("Select a pipeline or job first.".into()),
                };
                self.pipeline_status = opened.err().map(|e| e.to_string());
            }
            ViewEvent::Button(id) if id == "retry" || id == "cancel" => {
                let (pipeline, job) = self.pipeline_target();
                let cancellable = pipeline.as_ref().filter(|p| ACTIVE_PIPELINE.contains(&p.status.as_str())).map(|p| p.id);
                if CONFIRMED.contains(&id.as_str()) && cancellable.is_some() && pending != cancellable {
                    self.pipeline_confirm = cancellable;
                    host::refresh_view(PIPELINES);
                    return Ok(());
                }

                self.pipeline_status = Some(match Self::pipeline_action(&id, pipeline, job, ctx) {
                    Ok(done) => done,
                    Err(e) => e.to_string(),
                });
                self.pipelines = None;
                if let Some(iid) = Self::selected() {
                    self.loaded = None;
                    self.reload_merge_request(iid);
                }
            }
            _ => {}
        }

        host::refresh_view(PIPELINES);

        Ok(())
    }

    /// A selected job is retried itself, never its pipeline.
    fn pipeline_action(button: &str, pipeline: Option<Pipeline>, job: Option<Job>, ctx: &Invocation) -> Result<String, Error> {
        let selected_job = job.is_some();
        let retryable_job = job.filter(|j| RETRYABLE_JOB.contains(&j.status.as_str()));
        let pipeline = pipeline.filter(|p| match button {
            "retry" => !selected_job && RETRYABLE_PIPELINE.contains(&p.status.as_str()),
            _ => ACTIVE_PIPELINE.contains(&p.status.as_str()),
        });
        let repo = gitlab_project(ctx)?;
        let api = api::Project(&repo.path);
        let _task = host::begin_task("Updating pipeline", false);

        match (button, retryable_job, pipeline) {
            ("retry", Some(job), _) => api.retry_job(job.id).map(|_| "Job retried.".into()),
            ("retry", None, Some(p)) => api.retry_pipeline(p.id).map(|_| "Pipeline retried.".into()),
            ("retry", ..) => Err("Select a finished job or a failed pipeline first.".into()),
            ("cancel", _, Some(p)) => api.cancel_pipeline(p.id).map(|_| "Pipeline canceled.".into()),
            _ => Err("Select a running pipeline first.".into()),
        }
    }

    fn check_ci_files(&mut self, events: &[Event], ctx: &Invocation) {
        let Some(project) = ctx.project() else { return };

        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            if !ci::is_ci_path(path) {
                continue;
            }

            let Ok(source) = project.read_file(path) else {
                if self.ci_problems.remove(path).is_some() {
                    self.publish_review(path);
                }
                continue;
            };
            let problems = ci::check(&ci::parse(&source))
                .into_iter()
                .filter(|f| ci_group_enabled(f.code))
                .map(|f| {
                    let range = Range::new(text::position(&source, f.start), text::position(&source, f.end));
                    let mut diagnostic = Diagnostic::new(range, f.severity, f.message);
                    diagnostic.code = Some(f.code.into());
                    diagnostic.source = Some("GitLab CI".into());
                    diagnostic
                })
                .collect();

            self.ci_problems.insert(path.clone(), problems);
            self.publish_review(path);
        }
    }
}

fn ci_group_enabled(code: &str) -> bool {
    let key = match code {
        "undefined-job" => "ciJobReferences",
        "undefined-stage" => "ciStages",
        "duplicate-job-usage" => "ciDuplicateJobUsage",
        _ => "ciSingleQuotedVariables",
    };

    host::setting_bool(key).unwrap_or(true)
}

impl Providers for GitLab {
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if !ci::is_ci_path(&doc.path) {
            return Vec::new();
        }

        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let Some(target) = ci::target_at(&ci::parse(&doc.text), offset) else { return Vec::new() };

        let range = Range::new(text::position(&doc.text, target.start), text::position(&doc.text, target.end));
        vec![Location { path: doc.path.clone(), range }]
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.branch_line_markers(doc)
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        self.suggestion_actions(doc, selection)
    }
}

impl Views for GitLab {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        api::begin_budget();
        if !host::secret_is_set(api::TOKEN) {
            return Ok(View::message(NO_TOKEN).button(Button::new("new-token", "Create Access Token")));
        }

        let rendered = match view {
            LIST => self.render_list(ctx),
            DETAILS => self.render_details(ctx),
            CHANGES => self.render_changes(ctx),
            PROJECTS => self.render_projects(ctx),
            NEW_MR if !forms_enabled() => return Ok(View::message("Form views are off. Turn them on under Settings › Plugins › GitLab.")),
            NEW_MR => self.render_new_merge_request(ctx),
            NEW_SNIPPET if !forms_enabled() => return Ok(View::message("Form views are off. Turn them on under Settings › Plugins › GitLab.")),
            NEW_SNIPPET => Ok(self.new_snippet.render(&snippet_scopes(ctx))),
            SHARE if !forms_enabled() => return Ok(View::message("Form views are off. Turn them on under Settings › Plugins › GitLab.")),
            SHARE => self.render_share(),
            COMMITS => self.render_commits(ctx),
            PIPELINES => self.render_pipelines(ctx),
            _ => return Err(Error::NotFound(format!("view {view}"))),
        };

        Ok(rendered.unwrap_or_else(|e| View::message(&e.to_string()).button(Button::new("refresh", "Refresh").icon("refresh"))))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        api::begin_budget();
        if let ViewEvent::Button(id) = &event {
            if id == "new-token" {
                return host::open_url(&api::new_token_url());
            }
        }

        match view {
            LIST => self.handle_list(event, ctx),
            DETAILS => self.handle_details(event, ctx),
            CHANGES => self.handle_changes(event, ctx),
            PROJECTS => self.handle_projects(event, ctx),
            NEW_MR if forms_enabled() => self.handle_new_merge_request(event, ctx),
            NEW_SNIPPET if forms_enabled() => self.handle_new_snippet(event, ctx),
            SHARE if forms_enabled() => self.handle_share(event, ctx),
            COMMITS => self.handle_commits(event),
            PIPELINES => self.handle_pipelines(event, ctx),
            _ => Ok(()),
        }
    }
}

register_plugin!(GitLab, providers, views);

#[cfg(test)]
mod tests;
