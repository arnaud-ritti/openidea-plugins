//! GitLab for OpenIDEA, against gitlab.com with a personal access token: the merge
//! request list, timeline and review flow, the changes with diff comments and
//! pending reviews, the current branch's merge request in the status bar and
//! the editor, checkout, clone and share, Open on GitLab / Copy Link and
//! Create Snippet.

mod api;
mod markdown;
mod attributes;
mod branch;
mod fork_checkout;
mod ci;
mod ci_data;
mod content_tabs;
mod prompts;
mod repo;
mod review;
mod safe;
mod sections;
mod timeline;
mod local_drafts;
mod workspace;

use std::collections::{BTreeMap, BTreeSet};

use api::{Approvals, Commit, Discussion, DraftNote, FileDiff, Job, MergeRequest, Note, Pipeline, ProjectInfo, TimelineEvent, User};
use branch::BranchReview;
use openidea_plugin::host::{self, storage};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, PickItem, Plugin, Position, Range, RepositoryStatus,
    register_plugin, text,
};
use sections::{Badges, Section};

const LIST: &str = "merge-requests";
const DETAILS: &str = "merge-request";
/// Content-free invalidation; each page fetches its own correlated model.
fn refresh_mr_view(view: &str) {
    debug_assert!(matches!(view, LIST | DETAILS));
    ui_kit::web::refresh_view(view, include_str!("ui_config.json"));
}

const PROJECTS: &str = "gitlab-projects";

const FILTER_KEY: &str = "filter";
/// Typed filters (author, reviewer, label, text) as a query-string fragment added to the quick filter's.
const EXTRA_FILTER_KEY: &str = "filter-extra";
const SELECTED_KEY: &str = "selected";
const SELECTED_SCOPE_KEY: &str = "selected-scope";
const THREAD_KEY: &str = "thread";
/// The selected reply; without it a selected thread means its first note.
const NOTE_KEY: &str = "note";
const COMMENT_KEY: &str = "comment";
const AS_DRAFT_KEY: &str = "as-draft";
const EVENTS_KEY: &str = "show-events";

const DETACHED: &str = "HEAD is detached. Check out a branch first.";
const NO_TOKEN: &str = "Enter a GitLab personal access token (scopes api, read_user) under Settings › Plugins › GitLab.";

/// The original's quick filters, in its order; "assigned" is its default.
const FILTERS: [(&str, &str); 5] =
    [("open", "Open"), ("mine", "Include my changes"), ("review", "Need my review"), ("assigned", "Assigned to me"), ("closed", "Closed")];

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

fn extra_filter() -> String {
    storage::get(EXTRA_FILTER_KEY).ok().flatten().unwrap_or_default()
}

fn set_extra_filter(key: &str, value: Option<&str>) -> Result<(), Error> {
    let kept = extra_filter();
    let mut pairs: Vec<String> = kept.split('&').filter(|p| !p.is_empty() && p.split('=').next() != Some(key)).map(str::to_owned).collect();

    if let Some(value) = value {
        pairs.push(format!("{key}={}", api::encode(value)));
    }

    storage::set(EXTRA_FILTER_KEY, &pairs.join("&"))
}

fn full_filter_query(key: &str, me: &User) -> String {
    let query = filter_query(key, me);

    match extra_filter().as_str() {
        "" => query,
        extra => format!("{query}&{extra}"),
    }
}

/// Asks which filter to change: the quick filter, or a typed one (author, reviewer and label from the project's lists).
/// Where the IDE refuses the picker the quick filter just moves on to the next.
fn edit_filter(ctx: &Invocation) -> Result<(), Error> {
    let dimensions = [
        PickItem::new("quick", "Quick filter"),
        PickItem::new("author_username", "Author"),
        PickItem::new("reviewer_username", "Reviewer"),
        PickItem::new("labels", "Label"),
        PickItem::new("search", "Text in title or description"),
        PickItem::new("clear", "Clear author, reviewer, label and text"),
    ];

    let dimension = match host::pick("Filter merge requests", &dimensions) {
        Err(Error::Invalid(_)) => return storage::set(FILTER_KEY, next_filter(filter())),
        Err(e) => return Err(e),
        Ok(None) => return Ok(()),
        Ok(Some(dimension)) => dimension,
    };

    match dimension.as_str() {
        "quick" => {
            let items: Vec<PickItem> = FILTERS.iter().map(|(key, label)| PickItem::new(key, label)).collect();
            if let Some(key) = host::pick("Quick filter", &items)? {
                storage::set(FILTER_KEY, &key)?;
            }
        }
        "clear" => storage::delete(EXTRA_FILTER_KEY)?,
        "search" => {
            if let Some(text) = host::input("Search merge requests", None)? {
                set_extra_filter("search", Some(text.trim()).filter(|t| !t.is_empty()))?;
            }
        }
        key => {
            let repo = gitlab_project(ctx)?;
            let _task = host::begin_task("Loading choices", false);
            let api = api::Project(&repo.path);
            let names = match key {
                "labels" => api.labels()?,
                _ => api.members()?.into_iter().map(|u| u.username).collect(),
            };
            let items: Vec<PickItem> = names.iter().map(|name| PickItem::new(name, name)).collect();

            if let Some(name) = host::pick("Filter merge requests", &items)? {
                set_extra_filter(key, Some(&name))?;
            }
        }
    }

    Ok(())
}

fn filter_button_label(filter: &str, extra: &str) -> String {
    let typed = extra.split('&').filter(|p| !p.is_empty()).count();

    match typed {
        0 => filter_label(filter).to_owned(),
        n => format!("{} + {n} typed", filter_label(filter)),
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

/// The repository of the workspace the merge request views show (the window's own,
/// or an independent one in a child folder) and its GitLab project.
struct Repo {
    status: RepositoryStatus,
    /// `group/name`.
    path: String,
    /// The git remote pointing at it (`origin` first, as git does).
    remote: String,
}

fn gitlab_project(ctx: &Invocation) -> Result<Repo, Error> {
    let project = ctx.project().ok_or("No project is open.")?;

    workspace::resolve(&project)
}

/// The repository of an editor file or selection: the workspace repository containing it,
/// else the window's own (which then rejects the path with the usual message).
fn file_repository(ctx: &Invocation, path: &str) -> Result<Repo, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let repo = match workspace::containing(&project, path) {
        Some(repo) => repo,
        None => gitlab_project(ctx)?,
    };

    Ok(workspace::with_live_commit(&project, repo))
}

/// Whether `repo` is the repository the window itself has open (local `git` runs there).
fn is_window_repository(repo: &Repo, ctx: &Invocation) -> bool {
    ctx.project().is_some_and(|project| project.vcs_status().is_ok_and(|status| status.is_some_and(|status| workspace::same_root(&status.root, &repo.status.root))))
}

/// Local `git` actions run in the window's repository; a picked workspace repository needs its own window.
fn window_repository(repo: &Repo, ctx: &Invocation) -> Result<(), Error> {
    match is_window_repository(repo, ctx) {
        true => Ok(()),
        false => Err("The selected merge request belongs to another workspace repository. Open its folder in its own window to run local git actions.".into()),
    }
}

const NO_REPOSITORY: &str = "The project is not in a git repository.";
const NO_REMOTE: &str = "The repository has no remote on the configured GitLab server. \
Share it on GitLab, clone a project from the Projects tab, or check the GitLab server in Settings › Plugins › GitLab.";

/// A missing repository or GitLab remote offers Share Project instead of a dead end.
fn with_share_offer(view: View, message: &str) -> View {
    if message != NO_REPOSITORY && message != NO_REMOTE {
        return view;
    }
    view.button(Button::new("share-project", "Share Project on GitLab…").icon("upload"))
}

fn selection_scope(repo: &Repo) -> String {
    serde_json::to_string(&(&repo.path, &repo.status.root, &repo.remote, api::token())).unwrap_or_default()
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

const MORE: &str = "more-actions";

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
const STAGE_ORDER: [&str; 12] = [
    "failed",
    "running",
    "pending",
    "preparing",
    "waiting_for_resource",
    "created",
    "scheduled",
    "canceled",
    "manual",
    "warning",
    "success",
    "skipped",
];
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
    timeline: timeline::State,
    draft_selected: Option<u64>,
    /// The project and filter `list` was loaded for.
    loaded: Option<(String, String)>,
    /// Root, GitLab project and remote owning all in-memory MR caches.
    cache_scope: Option<String>,
    account_scope: Option<String>,
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
    content_snapshots: BTreeMap<String, content_tabs::Snapshot>,
    /// Oldest accepted capture first, independent of opaque key sorting.
    content_snapshot_order: std::collections::VecDeque<String>,
    content_origin_frame: Option<String>,
    /// Native activation contract only; never inferred from persisted state or SDK constant.
    trace_transport:bool,
    /// The `CONFIRMED` button clicked once, waiting for its second click.
    confirm: Option<String>,
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
    fn ensure_account(&mut self) -> Result<(), Error> {
        let account=api::token();
        if self.account_scope.as_deref()==Some(account) {return Ok(());}
        if self.account_scope.is_none() {self.account_scope=Some(account.into());return Ok(());}
        if self.account_scope.is_some() && storage::get(COMMENT_KEY)?.is_some_and(|draft|!draft.trim().is_empty()) {
            return Err("Finish or clear the current account's unsent comment before switching account slots. Restore its account setting to continue the draft.".into());
        }
        self.me=None;self.projects=None;self.default_branches.clear();self.content_snapshots.clear();self.content_snapshot_order.clear();self.content_origin_frame=None;self.cache_scope=None;
        self.account_scope=Some(account.into());Ok(())
    }

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
        self.timeline = timeline::State::default();
        self.draft_selected = None;
        self.threads.clear();
        self.events.clear();
        self.details = None;
        self.diffs = None;
    }

    fn ensure_view_owner(&mut self, ctx: &Invocation) -> Result<(), Error> {
        self.ensure_account()?;
        let repo = gitlab_project(ctx);
        let scope = repo.as_ref().ok().map(selection_scope);
        if self.cache_scope != scope {
            self.reset(); self.list.clear(); self.commits = None; self.commit_diff = None;
            self.pipelines = None; self.jobs.clear(); self.pipeline_selection = None;
            self.pipeline_confirm = None; self.pipeline_status = None; self.pipeline_open.clear();
            self.open_threads.clear(); self.anchor = None; self.commit = None; self.confirm = None;
            self.status = None; self.branch = None;
            self.cache_scope = scope;
        }
        repo.map(|_| ())
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
        let _ = host::set_context("mrSelected", true);
        self.timeline = timeline::State::default();
        storage::delete(COMMENT_KEY)?;
        self.details = None;
        self.anchor = None;
        self.commit = None;
        self.confirm = None;
        self.pipeline_selection = None;
        self.pipeline_confirm = None;
        self.pipeline_status = None;
        self.pipeline_open.clear();
        refresh_mr_view(DETAILS);

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
                items.push(
                    TreeItem::new(&note_key, &first_line(&n.body)).parent(&thread_key).icon("review-comment").detail(&n.author.username),
                );
            }
            entries.push((&first.created_at, items));
        }

        if show_events {
            for (index, e) in self.events.get(&mr.iid).into_iter().flatten().enumerate() {
                let key = format!("e:{}:{index}", mr.iid);
                let item =
                    TreeItem::new(&key, &format!("{} {}", e.user, e.text)).parent(parent).icon("history").detail(date(&e.created_at));
                entries.push((&e.created_at, vec![item]));
            }
        }

        entries.sort_by(|a, b| a.0.cmp(b.0));
        entries.into_iter().flat_map(|(_, items)| items).collect()
    }

    fn render_list(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let repo = match gitlab_project(ctx) {
            Ok(repo) => repo,
            Err(error) => return Ok(self.unavailable(error, ctx)),
        };
        let filter = filter();
        let extra = extra_filter();
        let wanted = (repo.path.clone(), format!("{filter}&{extra}"));

        if self.loaded.as_ref() != Some(&wanted) {
            let _task = host::begin_task("Loading merge requests", false);
            let query = full_filter_query(filter, &self.me()?);
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
            true => Body::Message(format!("No merge requests match the filter \"{}\".", filter_button_label(filter, &extra))),
            false => Body::Tree(items),
        };
        let events = if preference(EVENTS_KEY, true) { "Hide Events" } else { "Show Events" };

        Ok(View::new(body)
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .button(Button::new("repo", "Repository").icon("module"))
            .button(Button::new("filter", &filter_button_label(filter, &extra)).icon("filter"))
            .button(Button::new("events", events).icon("github-timeline"))
            .status(&format!("{} · {}", repo.path, filter_label(filter))))
    }

    /// The error as a message, with the repository picker when the workspace still has repositories to offer.
    fn unavailable(&self, error: Error, ctx: &Invocation) -> View {
        let message = error.to_string();
        let mut view = with_share_offer(View::message(&message).button(Button::new("refresh", "Refresh").icon("refresh")), &message);
        if ctx.project().is_some_and(|project| workspace::pickable(&project)) {
            view = view.button(Button::new("repo", "Repository").icon("module"));
        }

        view
    }

    fn handle_list(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Button(id) if id == "refresh" => {
                self.reset();
                workspace::forget();
                refresh_mr_view(DETAILS);
            }
            ViewEvent::Button(id) if id == "repo" => {
                let project = ctx.project().ok_or("No project is open.")?;
                let root = project.root()?;
                workspace::forget();
                let items = workspace::pick_items(&project, &root);
                if items.is_empty() {
                    return Err("This workspace has no GitLab repositories to pick from.".into());
                }

                let window_root = project.vcs_status()?.map(|status| status.root).unwrap_or_default();
                if let Some(picked) = host::pick("Repository merge requests", &items)? {
                    match workspace::same_root(&picked, &window_root) {
                        true => workspace::select(&root, None)?,
                        false => workspace::select(&root, Some(&picked))?,
                    }
                }

                self.reset();
                refresh_mr_view(DETAILS);
            }
            ViewEvent::Button(id) if id == "filter" => edit_filter(ctx)?,
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

                let repo = gitlab_project(ctx)?;
                if self.loaded.as_ref().is_some_and(|(project, _)| project != &repo.path) {
                    return Err("The project changed. Refresh the merge request list before selecting an item.".into());
                }
                let scope = selection_scope(&repo);
                let old_scope = storage::get(SELECTED_SCOPE_KEY)?;
                let changed_scope = old_scope.as_deref() != Some(scope.as_str());
                if changed_scope && storage::get(COMMENT_KEY)?.is_some_and(|text| !text.is_empty()) {
                    return Err("An unsent comment belongs to the previously selected project. Finish or clear it there before switching projects.".into());
                }
                storage::set(SELECTED_SCOPE_KEY, &scope)?;
                if changed_scope || Self::selected() != Some(iid) {
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
                refresh_mr_view(DETAILS);
                return Ok(());
            }
            ViewEvent::Expand(key) => {
                let Some(Key::MergeRequest(iid)) = Key::parse(&key) else {
                    self.open_threads.insert(key);
                    refresh_mr_view(LIST);
                    return Ok(());
                };

                let repo = gitlab_project(ctx)?;
                let _task = host::begin_task("Loading timeline", false);
                self.load_timeline(&repo.path, iid)?;
            }
            ViewEvent::Collapse(key) => {
                let Some(Key::MergeRequest(iid)) = Key::parse(&key) else {
                    self.open_threads.remove(&key);
                    refresh_mr_view(LIST);
                    return Ok(());
                };

                self.threads.remove(&iid);
                self.events.remove(&iid);
            }
            ViewEvent::Activate(key) => return self.activate(&key, ctx),
            _ => return Ok(()),
        }

        refresh_mr_view(LIST);

        Ok(())
    }

    /// Open a listed merge request in the native Details tab; diff threads keep source navigation.
    fn activate(&mut self, key: &str, ctx: &Invocation) -> Result<(), Error> {
        let (iid, thread, note) = match Key::parse(key) {
            Some(Key::MergeRequest(iid)) => (iid, None, None),
            Some(Key::Thread(iid, d)) => (iid, Some(d), None),
            Some(Key::Note(iid, d, n)) => (iid, Some(d), Some(n)),
            None => return Ok(()),
        };

        if matches!(Key::parse(key), Some(Key::MergeRequest(_))) {
            if !self.list.iter().any(|mr|mr.iid == iid) { return Err("The merge request is no longer in the list. Refresh it.".into()); }
            self.handle_list(ViewEvent::Select(key.to_owned()), ctx)?;
            return ui_kit::web::reveal_view(DETAILS, include_str!("ui_config.json"));
        }
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

    fn set_section(&self, section: Section) -> Result<(), Error> {
        storage::set(sections::SECTION_KEY, section.id())?;
        refresh_mr_view(DETAILS);

        Ok(())
    }

    fn refresh_section(&self, section: Section) {
        if Section::stored() == section {
            refresh_mr_view(DETAILS);
        }
    }

    fn badges(&self) -> Badges {
        let diff_count = self.diffs.as_ref().map(|(_, diffs)| diffs.len());

        Badges {
            changes: diff_count,
            files: diff_count,
            commits: self.commits.as_ref().map(|(_, commits)| commits.len()),
            pipeline: self.details.as_ref().and_then(|d| d.mr.head_pipeline.as_ref()).map(|p| p.status.clone()),
        }
    }

    fn render_section(&mut self, ctx: &Invocation) -> Result<View, Error> {
        if Self::selected().is_some() {
            let repo = gitlab_project(ctx)?;
            self.checked_selection(&repo, ctx)?;
        }
        let section = Section::stored();
        let view = match section {
            Section::Overview => self.render_details(ctx),
            Section::Description => self.render_markdown_description(ctx),
            Section::Changes => self.render_changes(ctx),
            Section::Commits => self.render_commits(ctx),
            Section::Files => self.render_files(ctx),
            Section::Pipelines => self.render_pipelines(ctx),
            Section::Timeline => self.render_timeline(ctx),
            Section::Drafts => self.render_local_drafts(ctx),
        }
        .unwrap_or_else(|e| View::message(&e.to_string()).button(Button::new("refresh", "Refresh").icon("refresh")));

        Ok(sections::with_sections(view, section, &self.badges()))
    }

    fn handle_section(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if Self::selected().is_some() {
            let repo = gitlab_project(ctx)?;
            self.checked_selection(&repo, ctx)?;
        }
        if let ViewEvent::Button(id) = &event {
            if id == MORE {
                return self.pick_more_action(ctx);
            }
            if id == "open-overview-snapshot" {
                return self.open_content_snapshot(ctx, content_tabs::Kind::Overview);
            }
            if id == "open-diff-snapshot" {
                return self.open_content_snapshot(ctx, content_tabs::Kind::Diff);
            }
        }
        if let ViewEvent::Button(id) = &event {
            if let Some(section) = Section::from_button(id) {
                return self.set_section(section);
            }
        }

        if let ViewEvent::Message(raw)=&event{if raw.len()<=8192{if let Ok(message)=serde_json::from_str::<serde_json::Value>(raw){if message["type"]=="markdown-link"{return self.open_markdown_link(ctx,message["url"].as_str().ok_or(Error::Stale)?);}}}}
        match Section::stored() {
            Section::Overview | Section::Description => self.handle_details(event, ctx),
            Section::Changes => self.handle_changes(event, ctx),
            Section::Commits => self.handle_commits(event),
            Section::Files => self.handle_files(event, ctx),
            Section::Pipelines => self.handle_pipelines(event, ctx),
            Section::Timeline => self.handle_timeline(event, ctx),
            Section::Drafts => self.handle_local_drafts(event, ctx),
        }
    }

    fn open_markdown_link(&self,ctx:&Invocation,url:&str)->Result<(),Error>{
        let repo=gitlab_project(ctx)?;let iid=self.checked_selection(&repo,ctx)?;
        let details=self.details.as_ref().filter(|details|details.mr.iid==iid).ok_or(Error::Stale)?;
        let base=format!("{}/{}/-/merge_requests/{iid}",api::server(),repo.path);
        let mut links=markdown::links(details.mr.description.as_deref().unwrap_or_default(),&base);
        for note in self.threads.get(&iid).into_iter().flatten().flat_map(|thread|&thread.notes){links.extend(markdown::links(&note.body,&api::server_prefix()));links.extend(markdown::links(&note.body,&base));}
        for draft in &details.drafts{links.extend(markdown::links(&draft.note,&api::server_prefix()));}
        if !links.iter().any(|known|known==url){return Err("Markdown link no longer belongs to the selected merge request snapshot.".into());}
        host::open_url(url)
    }

    fn render_markdown_description(&mut self,ctx:&Invocation)->Result<View,Error>{
        let repo=gitlab_project(ctx)?;let iid=self.checked_selection(&repo,ctx)?;self.ensure_details(&repo.path,iid)?;
        let details=self.details.as_ref().ok_or("Refresh merge request details first.")?;
        let base=format!("{}/{}/-/merge_requests/{iid}",api::server(),repo.path);
        let mut text=details.mr.description.clone().unwrap_or_default();
        if let Some(thread)=self.thread(iid){for note in &thread.notes{text.push_str(&format!("\n\n---\n\n### @{}\n\n{}",note.author.username,note.body));}}
        Ok(View::new(Body::Html(markdown::document(&text,&base))).button(Button::new("refresh","Refresh Description").icon("refresh")).status("Cached GitLab Markdown; images are shown as text and links open only on explicit activation."))
    }

    fn render_details(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the GitLab window."));
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
        let mut merge_hint = None;
        let mut view =
            View::new(Body::Form(fields)).button(Button::new("comment", comment_label(as_draft, thread.is_some())).icon("review-comment"));

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
                view = view
                    .button(Button::new("edit-note", "Save Edit").icon("edit"))
                    .button(Button::new("delete-note", delete).icon("trash"));
            }

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
                true => {
                    let options: Vec<&str> = [(details.squash, "squash"), (details.remove_source_branch, "delete source branch")]
                        .into_iter()
                        .filter_map(|(on, option)| on.then_some(option))
                        .collect();
                    match options.is_empty() {
                        true => format!("Confirm Merge !{} into {}", mr.iid, mr.target_branch),
                        false => format!("Confirm Merge !{} into {} ({})", mr.iid, mr.target_branch, options.join(", ")),
                    }
                }
                false => "Merge".into(),
            };
            view = view.button(Button::new("merge", &merge).icon("github-pr-merged").enabled(can_merge && mergeable));
            // The merge status already explains other blocks; a hidden Merge on a ready request means no permission.
            if mergeable && !can_merge {
                merge_hint = Some("You cannot merge this merge request");
            }

            if mr.detailed_merge_status.as_deref() == Some("need_rebase") {
                view = view.button(Button::new("rebase", "Rebase").icon("branch"));
            }

            view = view.button(Button::new("edit", "Edit Labels, People, Milestone…").icon("edit"));

            let close = match self.confirm == Some(format!("close:{}", mr.iid)) {
                true => format!("Confirm Close !{}", mr.iid),
                false => "Close".into(),
            };
            view = view.button(Button::new("close", &close).icon("github-pr-closed"));
        }

        if mr.state == "closed" {
            view = view.button(Button::new("reopen", "Reopen").icon("github-pr-open"));
        }

        if repo.status.branch.as_deref() != Some(branch::local_branch(mr).as_str()) && mr.state != "merged" && is_window_repository(&repo, ctx) {
            view = view.button(Button::new("checkout", "Check Out Branch").icon("branch"));
        }

        view = view
            .button(Button::new("browser", "Open on GitLab").icon("external-link"))
            .button(Button::new(MORE, "More…").icon("more"))
            .button(Button::new("refresh", "Refresh").icon("refresh"));

        let mut summary = details_summary(details);
        if let Some(hint) = merge_hint {
            summary.push_str(&format!(" · {hint}"));
        }
        Ok(match &self.status {
            Some(status) => view.status(&format!("{status} · {summary}")),
            None => view.status(&summary),
        })
    }

    /// Secondary Overview actions behind More…; only those available for the loaded merge request.
    fn more_actions(&mut self) -> Result<Vec<(&'static str, &'static str)>, Error> {
        let me = self.me()?;
        let details = self.details.as_ref().ok_or("The merge request is not loaded; refresh it.")?;
        let mr = &details.mr;
        let mut actions = Vec::new();

        if note_actions_enabled() {
            actions.push(("react", if self.thread(mr.iid).is_some() { "React to Comment" } else { "React" }));
        }

        if mr.state == "opened" {
            if !mr.reviewers.iter().any(|u| u.id == me.id) {
                actions.push(("review", "Add Me as Reviewer"));
            }

            if !mr.reviewers.is_empty() {
                actions.push(("rerequest-review", "Re-request Review…"));
            }

            actions.push(("draft", if mr.draft { "Mark as Ready" } else { "Mark as Draft" }));
        }

        actions.extend([
            ("open-overview-snapshot", "Open MR Snapshot"),
            ("open-diff-snapshot", "Open Diff Snapshot"),
            ("open-existing-worktree", "Open Existing Worktree"),
            ("copy", "Copy Link"),
        ]);
        Ok(actions)
    }

    fn pick_more_action(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let actions = self.more_actions()?;
        let items: Vec<PickItem> = actions.iter().map(|(id, label)| PickItem::new(id, label)).collect();
        let Some(chosen) = host::pick("More merge request actions", &items)? else {
            return Ok(());
        };

        if !actions.iter().any(|(id, _)| *id == chosen) {
            return Err(Error::Stale);
        }

        self.handle_section(ViewEvent::Button(chosen), ctx)
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
            refresh_mr_view(DETAILS);
            return Ok(());
        }

        if pending.is_some() {
            refresh_mr_view(DETAILS);
        }

        if let Some(attribute) = button.strip_prefix("edit-attribute:") {
            return self.edit_context_attribute(ctx, attribute);
        }
        if button == "rerequest-review" { return self.rerequest_review(ctx); }
        if button == "open-existing-worktree" {
            self.open_existing_worktree(ctx)?;
            return Ok(());
        }
        if button == "refresh" {
            if let Some(iid) = Self::selected() {
                self.threads.remove(&iid);
            }

            self.details = None;
            self.status = None;
            refresh_mr_view(DETAILS);
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
                refresh_mr_view(DETAILS);
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
            "edit" => match attributes::edit(&api, mr) {
                Err(Error::Invalid(_)) => {
                    Err("The IDE shows pickers only for commands. Run GitLab: Edit Merge Request from the command palette.".into())
                }
                result => result.map(|message| message.unwrap_or_else(|| "Nothing changed.".into())),
            },
            "checkout" => {
                window_repository(repo, ctx)?;
                let project = ctx.project().ok_or("No project is open.")?;
                branch::checkout(&project, &repo.remote, mr).map_err(|e| match e {
                    Error::Denied(_) => ui_kit::denied::message("Checking out a branch", "process:git", "GitLab").into(),
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
                    None => self.refresh_section(Section::Changes),
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

/// Only comment and merge options are editable; metadata is selectable text.
fn details_fields(details: &Details, thread: Option<&Discussion>, comment: String, as_draft: bool, emoji: Option<String>) -> Vec<Field> {
    let mut fields = Vec::new();

    if let Some(thread) = thread {
        let first = &thread.notes[0];
        let note = format!("{}: {}", first.author.username, first_line(&first.body));
        fields.push(Field::new("replying-to", "Replying to", FieldValue::readonly_text(&note, false)));
    }

    let label = if thread.is_some() { "Reply" } else { "Comment" };
    fields.push(Field::new(COMMENT_KEY, label, FieldValue::Multiline(comment)));
    fields.push(Field::new(AS_DRAFT_KEY, "Add to my review (published when you submit the review)", FieldValue::Checkbox(as_draft)));

    if let Some(emoji) = emoji {
        fields.push(Field::new("emoji", "Reaction", FieldValue::Choice(safe::EMOJIS.map(str::to_owned).to_vec(), emoji)));
    }

    if details.mr.state == "opened" {
        fields.push(Field::new(
            "remove-source-branch",
            "Delete source branch on merge",
            FieldValue::Checkbox(details.remove_source_branch),
        ));
        fields.push(Field::new("squash", "Squash commits on merge", FieldValue::Checkbox(details.squash)));
    }

    if let Some(description) = details.mr.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        fields.push(Field::new("description", "Description", FieldValue::readonly_text(description, true)));
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

fn first_listed<'a>(first: &str, branches: impl Iterator<Item = &'a str>) -> Vec<String> {
    let rest = branches.filter(|b| *b != first).map(str::to_owned);

    std::iter::once(first.to_owned()).filter(|b| !b.is_empty()).chain(rest).collect()
}

fn copy_link(url: &str) -> Result<(), Error> {
    match host::copy_to_clipboard(url) {
        Err(Error::Denied(_)) => {
            let denied = ui_kit::denied::message("Copying the link", "clipboard", "GitLab");
            host::notify(NotificationLevel::Info, "GitLab link", &format!("{denied}\n{url}"));
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
    let path = editor.path();
    let repo = file_repository(ctx, &path)?;
    let relative = relative_path(&repo.status.root, &path).ok_or("The file is not in the repository.")?;
    let revision = repo.status.commit.as_deref().ok_or("The repository has no commits.")?;

    Ok(api::blob_url(&repo.path, revision, &relative.replace('\\', "/"), selected_lines(&editor)))
}

fn selected_link(ctx: &Invocation, path: &str) -> Result<String, Error> {
    let repo = file_repository(ctx, path)?;
    let relative = match path == repo.status.root {
        true => "",
        false => relative_path(&repo.status.root, path).ok_or("The selection is not in the repository.")?,
    };
    let revision = repo.status.commit.as_deref().ok_or("The repository has no commits.")?;
    let relative = relative.replace('\\', "/");
    let is_file = ctx.project().is_some_and(|p| p.read_file(path).is_ok());

    Ok(match is_file {
        true => api::blob_url(&repo.path, revision, &relative, None),
        false => api::tree_url(&repo.path, revision, &relative),
    })
}

/// The Project view selection's link, or the focused editor's.
fn current_link(ctx: &Invocation) -> Result<String, Error> {
    match ctx.selected_paths().first() {
        Some(path) => selected_link(ctx, path),
        None => file_link(ctx),
    }
}

/// The focused file's repository path (its name outside the repository) and the selection, or the whole file.
fn snippet_content(ctx: &Invocation) -> Result<(String, String), Error> {
    let editor = ctx.editor().ok_or("No file is open.")?;
    let document = editor.document()?;
    let selected = editor.selections()?.first().and_then(|r| document.text_in(*r)).filter(|t| !t.is_empty()).map(str::to_owned);
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

fn selected_files(ctx: &Invocation) -> Vec<(String, String)> {
    let Some(project) = ctx.project() else {
        return Vec::new();
    };
    let root = project.vcs_status().ok().flatten().map(|s| s.root);

    ctx.selected_paths()
        .into_iter()
        .filter_map(|path| {
            let content = project.read_file(&path).ok().filter(|c| !c.is_empty())?;
            let name = path.rsplit(['/', '\\']).next().unwrap_or(&path);
            let file_path = root.as_deref().and_then(|r| relative_path(r, &path)).unwrap_or(name).replace('\\', "/");

            Some((file_path, content))
        })
        .collect()
}

fn create_snippet(ctx: &Invocation) -> Result<Option<String>, Error> {
    if !host::secret_is_set(api::token()) {
        return Err(NO_TOKEN.into());
    }

    let mut files = selected_files(ctx);
    if files.is_empty() {
        files = vec![snippet_content(ctx)?];
    }

    let default_title = match files.as_slice() {
        [(path, _)] => path.rsplit('/').next().unwrap_or(path).to_owned(),
        _ => format!("{} files", files.len()),
    };
    let paths: Vec<String> = files.iter().map(|(path, _)| path.clone()).collect();
    let Some(answers) = prompts::ask_new_snippet(&default_title, &paths)? else {
        return Ok(None);
    };

    files.extend(extra_snippet_files(&answers.extra_paths, ctx)?);
    post_snippet(&api::SnippetDraft {
        title: answers.title,
        description: String::new(),
        visibility: answers.visibility,
        files,
        project: None,
    })?;

    Ok(None)
}

fn extra_snippet_files(paths: &[String], ctx: &Invocation) -> Result<Vec<(String, String)>, Error> {
    let paths = paths.iter().map(|p| safe::repo_file_path(p)).collect::<Result<Vec<_>, _>>()?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let project = ctx.project().ok_or("Open a project first.")?;
    let root = project.root()?;

    paths
        .into_iter()
        .map(|path| {
            let content = project.read_file(&format!("{root}/{path}")).map_err(|_| Error::from(format!("Cannot read {path}.")))?;

            Ok((path.to_owned(), content))
        })
        .collect()
}

impl Plugin for GitLab {
    fn activate(&mut self,context:&openidea_plugin::Context)->Result<(),Error> {
        let mut parts=context.api_version.split('.');let major=parts.next();let minor=parts.next();let patch=parts.next();
        let numeric=|s:&str|!s.is_empty()&&s.bytes().all(|b|b.is_ascii_digit());
        self.trace_transport=major==Some("1")&&minor.is_some_and(|m|numeric(m)&&m.parse::<u32>().is_ok_and(|n|n>=4))&&patch.is_none_or(numeric)&&parts.next().is_none()&&context.has("net:gitlab.com");
        self.retire_job_snapshots();
        let _ = host::set_context("mrSelected", Self::selected().is_some());
        let _ = host::set_context("pendingFolder", repo::has_pending_folder());
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let _interactive = api::Interactive::start();
        self.ensure_account()?;
        api::begin_budget();
        match id {
            "new-token" => host::open_url(&api::new_token_url())?,
            "open-in-browser" => match arg.filter(|url| self.is_branch_link(url)) {
                Some(url) => host::open_url(url)?,
                None => host::open_url(&current_link(ctx)?)?,
            },
            "copy-link" => copy_link(&current_link(ctx)?)?,
            "create-snippet" => return create_snippet(ctx),
            "share-project" => {
                if !host::secret_is_set(api::token()) {
                    return Err(NO_TOKEN.into());
                }

                return repo::share(ctx);
            }
            "edit-merge-request" => return self.edit_merge_request(ctx),
            "checkout-source-cli" => return self.checkout_source_cli(ctx),
            "rerequest-review" => { self.rerequest_review(ctx)?; return Ok(None); },
            "show-branch-merge-request" => {
                // The command is about the window's branch, so the panel follows the window's repository again.
                // Without the optional project.read there is no workspace pick to clear.
                let project = ctx.project().ok_or("No project is open.")?;
                if let Ok(root) = project.root() {
                    workspace::select(&root, None)?;
                }
                self.ensure_view_owner(ctx)?;
                self.update_branch(ctx, true)?;
                let scope = selection_scope(&gitlab_project(ctx)?);
                if storage::get(SELECTED_SCOPE_KEY)?.as_deref() != Some(scope.as_str())
                    && storage::get(COMMENT_KEY)?.is_some_and(|text| !text.is_empty()) {
                    return Err("Finish or clear the previous project's unsent comment before switching projects.".into());
                }
                storage::set(SELECTED_SCOPE_KEY, &scope)?;
                return self.show_branch_merge_request();
            },
            "open-merge-request-in-worktree" => return self.open_worktree(ctx),
            "open-pending-folder" => return repo::open_pending_folder(),
            "open-existing-merge-request-worktree" => return self.open_existing_worktree(ctx),
            "update-merge-request-branch" => return self.update_merge_request_branch(ctx),
            "create-merge-request" => return self.create_merge_request(ctx),
            _ => return Err(Error::NotFound(format!("command {id}"))),
        }

        Ok(None)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        // Token-free commands (Open on GitLab, Copy Link) use the workspace repositories too.
        let repository_changed = events.iter().any(|e| matches!(e, Event::VcsChanged(_)));
        if repository_changed {
            workspace::forget();
        }
        if self.ensure_account().is_err() { return; }
        api::begin_budget();
        self.check_ci_files(events, ctx);
        if !host::secret_is_set(api::token()) {
            return;
        }

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
    /// The merge request the commands act on in `repo`: the one selected in its list, else the
    /// current branch's when `repo` is the window's repository. A selection made in another
    /// repository is never reused, since equal iids name different merge requests there.
    fn command_target(&self, repo: &Repo) -> Result<u64, Error> {
        let in_scope = storage::get(SELECTED_SCOPE_KEY)?.as_deref() == Some(selection_scope(repo).as_str());
        let selected = Self::selected().filter(|_| in_scope);
        let branch_iid = self.branch.as_ref().filter(|b| workspace::same_root(&b.root, &repo.status.root)).and_then(|b| b.mr.as_ref()).map(|mr| mr.iid);

        selected.or(branch_iid).ok_or_else(|| "Select a merge request in the GitLab window first.".into())
    }

    fn edit_merge_request(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        let api = api::Project(&repo.path);
        let mr = api.merge_request(self.command_target(&repo)?)?;
        let message = attributes::edit(&api, &mr)?;

        if message.is_some() {
            self.loaded = None;
            self.reload_merge_request(mr.iid);
        }

        Ok(message)
    }

    fn rerequest_review(&mut self, ctx: &Invocation) -> Result<(), Error> {
        if !host::secret_is_set(api::token()) {return Err(NO_TOKEN.into());}
        let repo=gitlab_project(ctx)?;let iid=self.checked_selection(&repo,ctx)?;
        let mr=self.checked_merge_request(&repo,ctx,iid)?;
        if mr.state!="opened" || mr.reviewers.is_empty() {return Err("Select an open merge request with assigned reviewers.".into());}
        let mut ids=std::collections::HashSet::new();
        if mr.reviewers.iter().any(|user|user.id==0 || user.username.is_empty() || !ids.insert(user.id)) {return Err("Refresh the ambiguous reviewer list before requesting review.".into());}
        let items:Vec<_>=mr.reviewers.iter().map(|user|PickItem::new(&user.id.to_string(),&format!("@{} — {}",user.username,user.name))).collect();
        let Some(choice)=host::pick("Re-request review from an assigned reviewer",&items)? else {return Ok(());};
        let reviewer=mr.reviewers.iter().find(|user|user.id.to_string()==choice).ok_or(Error::Stale)?;
        if !host::confirm("Re-request review",&format!("Request another review from @{} for {} !{iid}?",reviewer.username,repo.path))? {return Ok(());}
        let current=self.checked_merge_request(&repo,ctx,iid)?;
        if current.state!="opened" || current.sha!=mr.sha || !current.reviewers.iter().any(|user|user.id==reviewer.id&&user.username==reviewer.username) {return Err(Error::Stale);}
        api::Project(&repo.path).rerequest_review(iid,reviewer.id)?;
        self.loaded=None;self.reload_merge_request(iid);self.status=Some(format!("Review re-requested from @{}.",reviewer.username));refresh_mr_view(DETAILS);Ok(())
    }

    fn edit_context_attribute(&mut self, ctx: &Invocation, attribute: &str) -> Result<(), Error> {
        if !attributes::ATTRIBUTES.iter().any(|(key, _)| *key == attribute) {
            return Err(Error::NotFound(format!("attribute {attribute}")));
        }
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }
        let repo = gitlab_project(ctx)?;
        let iid = self.checked_selection(&repo, ctx)?;
        let mr = self.checked_merge_request(&repo, ctx, iid)?;
        if self.details.as_ref().is_none_or(|details| {
            details.mr.iid != iid
                || details.mr.target_project_id != mr.target_project_id
                || details.mr.source_project_id != mr.source_project_id
                || details.mr.source_branch != mr.source_branch
                || details.mr.author.username != mr.author.username
        }) || mr.state != "opened"
        {
            return Err("Refresh the selected open merge request before editing it.".into());
        }
        let task = host::begin_task("Editing merge request attributes", true);
        let message = attributes::edit_one(&api::Project(&repo.path), &mr, attribute, || {
            if task.is_cancelled() {
                return Err("Attribute edit cancelled.".into());
            }
            let current = self.checked_merge_request(&repo, ctx, iid)?;
            if task.is_cancelled() {
                return Err("Attribute edit cancelled.".into());
            }
            if current.state != "opened"
                || current.source_project_id != mr.source_project_id
                || current.source_branch != mr.source_branch
                || current.author.username != mr.author.username
            {
                return Err("The merge request changed; refresh before editing.".into());
            }
            Ok(())
        })?;
        if let Some(message) = message {
            self.status = Some(message);
            self.loaded = None;
            self.reload_merge_request(iid);
        }
        Ok(())
    }

    fn checked_selection(&self, repo: &Repo, ctx: &Invocation) -> Result<u64, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        if !workspace::within(&project.root()?, &repo.status.root)
            || storage::get(SELECTED_SCOPE_KEY)?.as_deref() != Some(selection_scope(repo).as_str())
        {
            return Err("Select a merge request in this project's refreshed list first.".into());
        }
        Self::selected().ok_or_else(|| "Select a merge request first.".into())
    }

    fn checked_merge_request(&self, repo: &Repo, ctx: &Invocation, iid: u64) -> Result<MergeRequest, Error> {
        if self.checked_selection(repo, ctx)? != iid {
            return Err("The selected merge request changed.".into());
        }
        let current = gitlab_project(ctx)?;
        if selection_scope(&current) != selection_scope(repo) {
            return Err("The repository changed; refresh the merge request.".into());
        }
        let api = api::Project(&repo.path);
        let info = api.info()?;
        let mr = api.merge_request(iid)?;
        if info.id == 0 || mr.iid != iid || mr.target_project_id != info.id || mr.source_project_id == 0 {
            return Err("The merge request does not belong to this repository's GitLab project.".into());
        }
        Ok(mr)
    }

    /// Opens a registered exact-head checkout without fetching, adding or switching branches.
    fn open_existing_worktree(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }
        let repo = gitlab_project(ctx)?;
        let iid = self.checked_selection(&repo, ctx)?;
        let mr = self.checked_merge_request(&repo, ctx, iid)?;
        window_repository(&repo, ctx)?;
        let project = ctx.project().ok_or("No project is open.")?;
        let task = host::begin_task("Finding existing merge request worktrees", true);
        let list = branch::git_ok(&project, &["worktree", "list", "--porcelain", "-z"])?;
        if task.is_cancelled() {
            return Ok(None);
        }
        let candidates = branch::matching_worktrees(&list, &mr)?;
        if candidates.is_empty() {
            return Err("No available local worktree has this merge request's exact current head. No checkout was created.".into());
        }
        let items: Vec<_> = candidates.iter().map(|tree| PickItem::new(&tree.path, &tree.path)).collect();
        let Some(path) = host::pick(&format!("Open existing worktree for !{iid} in a new window"), &items)? else {
            return Ok(None);
        };
        if task.is_cancelled() {
            return Ok(None);
        }
        if !candidates.iter().any(|tree| tree.path == path) {
            return Err("The chosen worktree is no longer available.".into());
        }
        let current = self.checked_merge_request(&repo, ctx, iid)?;
        if current.sha != mr.sha
            || current.source_project_id != mr.source_project_id
            || current.source_branch != mr.source_branch
            || current.author.username != mr.author.username
        {
            return Err("The merge request changed; select its worktree again.".into());
        }
        let list = branch::git_ok(&project, &["worktree", "list", "--porcelain", "-z"])?;
        if task.is_cancelled() {
            return Ok(None);
        }
        if !branch::matching_worktrees(&list, &current)?.iter().any(|tree| tree.path == path) {
            return Err("The worktree changed or disappeared; refresh and try again.".into());
        }
        self.checked_selection(&repo, ctx)?;
        let latest = gitlab_project(ctx)?;
        if selection_scope(&latest) != selection_scope(&repo) {
            return Err("The repository changed before opening the worktree.".into());
        }
        if task.is_cancelled() {
            return Ok(None);
        }
        host::open_project(&path, true)?;
        Ok(Some(format!("Requested opening !{iid}'s existing worktree in a new window.")))
    }

    fn open_worktree(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        window_repository(&repo, ctx)?;
        let project = ctx.project().ok_or("No project is open.")?;
        let mr = api::Project(&repo.path).merge_request(self.command_target(&repo)?)?;
        let plan = branch::worktree_plan(&repo.status.root, &repo.remote, &mr)?;

        let _task = host::begin_task("Adding worktree", false);
        let run = |args: &Vec<String>| branch::git_ok(&project, &args.iter().map(String::as_str).collect::<Vec<_>>());
        let added = run(&plan.fetch).and_then(|_| run(&plan.add));

        match added {
            Ok(_) => repo::offer_open("Worktree added", &format!("!{} is checked out in {}", mr.iid, plan.path), "Open Worktree", &plan.path),
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                "Worktree not added",
                &ui_kit::denied::message("Adding a worktree", "process:git", "GitLab"),
            ),
            Err(e) => return Err(e),
        }

        Ok(None)
    }

    fn update_merge_request_branch(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        let iid = self.command_target(&repo)?;
        let api = api::Project(&repo.path);
        let target = api.merge_request(iid)?.target_branch;
        if !host::confirm(&format!("Rebase !{iid} onto {target}?"), "GitLab rewrites the source branch on the server. Your local branches are not changed.")? {
            return Ok(None);
        }
        api.rebase(iid)?;

        let state = api.rebase_state(iid)?;
        let error = state.merge_error.filter(|_| !state.rebase_in_progress);
        if let Some(error) = error.filter(|e| !e.is_empty()) {
            return Err(Error::Failed(format!("Rebase of !{iid} failed: {error}")));
        }

        Ok(Some(format!("Rebase of !{iid} requested.")))
    }

    fn merge_request_defaults(&mut self, ctx: &Invocation, repo: &Repo) -> Result<prompts::MrDefaults, Error> {
        let current = safe::ref_name(repo.status.branch.as_deref().ok_or(DETACHED)?)?.to_owned();
        let project = ctx.project().ok_or("No project is open.")?;
        let local = branch::git_ok(&project, &["branch", "--format=%(refname:short)"]).unwrap_or_default();
        let sources = first_listed(&current, local.lines().map(str::trim).filter(|b| safe::ref_name(b).is_ok()));
        let title = branch::git_ok(&project, &["log", "-1", "--format=%s"]).unwrap_or_default().trim().to_owned();

        let default = self.default_branch(&repo.path)?;
        let targets = first_listed(&default, api::Project(&repo.path).branches()?.iter().map(String::as_str));

        Ok(prompts::MrDefaults { title, sources, targets })
    }

    fn create_merge_request(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        if !host::secret_is_set(api::token()) {
            return Err(NO_TOKEN.into());
        }

        let repo = gitlab_project(ctx)?;
        window_repository(&repo, ctx)?;
        let defaults = {
            let _task = host::begin_task("Loading branches", false);
            self.merge_request_defaults(ctx, &repo)?
        };
        let Some(answers) = prompts::ask_new_merge_request(&defaults)? else {
            return Ok(None);
        };

        let source = answers.source.clone();
        let created = {
            let _task = host::begin_task("Creating merge request", false);
            api::Project(&repo.path).create_merge_request(&answers.into_request()?)
        };
        match created {
            Ok(mr) => {
                host::notify(NotificationLevel::Info, "Merge request created", &format!("!{} {}\n{}", mr.iid, mr.title, mr.web_url));
                self.loaded = None;
                refresh_mr_view(LIST);
            }
            Err(error) => {
                host::notify(NotificationLevel::Error, "Merge request not created", &error.to_string());
                if host::confirm("Create Merge Request", "Open on GitLab?")? {
                    host::open_url(&api::new_merge_request_url(&repo.path, &source))?;
                }
            }
        }

        Ok(None)
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
            return Ok(View::message("Select a merge request in the GitLab window."));
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
            Some(c) => view.button(Button::new("all-changes", "All Changes").icon("arrow-left")).status(&format!(
                "Showing {} {} in the Changes section",
                c.short_id,
                first_line(&c.title)
            )),
            None => view.status(&format!("!{iid} · double-click a commit to see its changes")),
        })
    }

    /// Only a hex SHA listed for the selected merge request is ever picked, so a row key never reaches a URL unchecked.
    fn handle_commits(&mut self, event: ViewEvent) -> Result<(), Error> {
        if !commits_enabled() {
            return Ok(());
        }

        let mut show_changes = true;
        match event {
            ViewEvent::Activate(key) => {
                let Ok(sha) = safe::hex_sha(&key) else {
                    return Ok(());
                };
                let listed = self
                    .commits
                    .as_ref()
                    .is_some_and(|(iid, commits)| Self::selected() == Some(*iid) && commits.iter().any(|c| c.id == sha));
                if !listed {
                    return Ok(());
                }

                self.commit = Some(sha.to_owned());
            }
            ViewEvent::Button(id) if id == "all-changes" => self.commit = None,
            ViewEvent::Button(id) if id == "refresh" => {
                self.commits = None;
                show_changes = false;
            }
            _ => return Ok(()),
        }

        self.anchor = None;
        self.refresh_section(Section::Commits);
        if show_changes {
            return self.set_section(Section::Changes);
        }
        self.refresh_section(Section::Changes);

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

    fn pipeline_button_key(&self, ctx: &Invocation, kind: &str) -> Result<String, Error> {
        let context = self.review_context(ctx)?;
        let (pipeline, job) = self.pipeline_target();
        let source = serde_json::to_string(&(context, pipeline.map(|pipeline| pipeline.id), job.map(|job| job.id), self.pipeline_confirm, kind))
            .map_err(|_| Error::Stale)?;
        Ok(format!("pipeline-action:{source}"))
    }

    fn render_pipelines(&mut self, ctx: &Invocation) -> Result<View, Error> {
        if !pipelines_enabled() {
            return Ok(View::message("The pipeline view is off. Turn it on under Settings › Plugins › GitLab."));
        }

        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the GitLab window."));
        };

        let repo = gitlab_project(ctx)?;
        self.ensure_details(&repo.path, iid)?;
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
            items.push(
                TreeItem::new(&key, &label).icon(status_icon(&pipeline.status)).detail(&pipeline_label(&pipeline.status)).expandable(open),
            );
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
            (_, Some(j)) => Button::new(&self.pipeline_button_key(ctx, "retry")?, "Retry Job").enabled(RETRYABLE_JOB.contains(&j.status.as_str())),
            (Some(p), None) => Button::new(&self.pipeline_button_key(ctx, "retry")?, "Retry Pipeline").enabled(RETRYABLE_PIPELINE.contains(&p.status.as_str())),
            _ => Button::new(&self.pipeline_button_key(ctx, "retry")?, "Retry").enabled(false),
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
            .button(Button::new(&self.pipeline_button_key(ctx, "open-job-trace-snapshot")?,"Open Job Trace Snapshot").icon("file").enabled(self.trace_transport&&job.is_some()))
            .button(Button::new(&self.pipeline_button_key(ctx, "follow-job-log")?, "Follow Job Log (glab)").icon("console").enabled(job.is_some()))
            .button(Button::new(&self.pipeline_button_key(ctx, "cancel")?, &cancel).icon("stop").enabled(cancellable))
            .button(Button::new(&self.pipeline_button_key(ctx, "browser")?, "Open on GitLab").icon("external-link").enabled(pipeline.is_some()))
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
            let state =
                STAGE_ORDER.into_iter().find(|s| in_stage.iter().any(|j| job_state(j) == *s)).unwrap_or_else(|| job_state(in_stage[0]));
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
                items.push(
                    TreeItem::new(&format!("j:{}", job.id), &job.name).parent(&key).icon(status_icon(job_state(job))).detail(&detail),
                );
            }
        }

        items
    }

    /// Acts only on a listed pipeline or job; a stale or unparsable selection sends nothing.
    fn handle_pipelines(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if !pipelines_enabled() {
            return Ok(());
        }

        let event = if let ViewEvent::Button(key) = event {
            if let Some(payload) = key.strip_prefix("pipeline-action:").filter(|payload| payload.len() <= 2 * 1024 * 1024) {
                let (_, _, _, _, kind): (String, Option<u64>, Option<u64>, Option<u64>, String) = serde_json::from_str(payload).map_err(|_| Error::Stale)?;
                if !matches!(kind.as_str(), "retry" | "cancel" | "browser" | "open-job-trace-snapshot" | "follow-job-log") || self.pipeline_button_key(ctx, &kind)? != key {
                    return Err("This pipeline control belongs to another source row; refresh Pipelines.".into());
                }
                ViewEvent::Button(kind)
            } else if matches!(key.as_str(), "retry" | "cancel" | "browser" | "open-job-trace-snapshot" | "follow-job-log") {
                return Ok(());
            } else { ViewEvent::Button(key) }
        } else { event };
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
            ViewEvent::Button(id) if id == "follow-job-log" => {
                return self.follow_job_log(ctx);
            }
            ViewEvent::Button(id) if id == "open-job-trace-snapshot" => {
                let (pipeline,job)=self.pipeline_target();let (Some(pipeline),Some(job))=(pipeline,job) else{return Err("Select one listed pipeline job first.".into());};
                return self.open_content_snapshot(ctx,content_tabs::Kind::Job{pipeline:pipeline.id,job:job.id});
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
                if pipeline.is_none() && job.is_none() {
                    self.pipeline_status = Some("Select one listed pipeline or job first.".into());
                    self.refresh_section(Section::Pipelines);
                    return Ok(());
                }
                let cancellable = pipeline.as_ref().filter(|p| ACTIVE_PIPELINE.contains(&p.status.as_str())).map(|p| p.id);
                if CONFIRMED.contains(&id.as_str()) && cancellable.is_some() && pending != cancellable {
                    self.pipeline_confirm = cancellable;
                    self.refresh_section(Section::Pipelines);
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

        self.refresh_section(Section::Pipelines);

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
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else {
                continue;
            };
            if !ci::is_ci_path(path) {
                continue;
            }

            let Ok(source) = project.read_file(path) else {
                if self.ci_problems.remove(path).is_some() {
                    self.publish_review(path);
                }
                continue;
            };
            let mut file = ci::parse(&source);
            resolve_local_includes(&mut file, &project);
            let problems = ci::check(&file)
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

fn resolve_local_includes(file: &mut ci::CiFile, project: &host::Project) {
    let Some(root) = project.vcs_status().ok().flatten().map(|s| s.root) else {
        file.has_include |= !file.local_includes.is_empty();
        file.local_includes.clear();
        return;
    };

    ci::resolve_includes(file, &|path| project.read_file(&format!("{root}/{}", path.trim_start_matches('/'))).ok());
}

fn ci_group_enabled(code: &str) -> bool {
    let key = match code {
        "undefined-job" | "undefined-reference" => "ciJobReferences",
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

        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        let Some(target) = ci::target_at(&ci::parse(&doc.text), offset) else {
            return Vec::new();
        };

        let range = Range::new(text::position(&doc.text, target.start), text::position(&doc.text, target.end));
        vec![Location { path: doc.path.clone(), range }]
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        match ci::is_ci_path(&doc.path) {
            true => ci::completions(&doc.text, text::offset(&doc.text, at)),
            false => Vec::new(),
        }
        .into_iter()
        .map(|c| {
            let kind = if c.variable { CompletionKind::Variable } else { CompletionKind::Keyword };
            let mut item = CompletionItem::new(&c.label, kind);
            item.insert_text = c.insert;
            item.replace = Some(Range::new(text::position(&doc.text, c.from), at));
            item
        })
        .collect()
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
        self.ensure_account()?;
        (|| -> Result<View, Error> {
            api::begin_budget();
            if !host::secret_is_set(api::token()) {
                return Ok(View::message(NO_TOKEN).button(Button::new("new-token", "Create Access Token").icon("external-link")));
            }

            if matches!(view, LIST | DETAILS) {
                if let Err(error) = self.ensure_view_owner(ctx) { return Ok(self.unavailable(error, ctx)); }
            }

            let rendered = match view {
                LIST => self.render_list(ctx),
                DETAILS => self.render_section(ctx),
                PROJECTS => self.render_projects(ctx),
                _ => return Err(Error::NotFound(format!("view {view}"))),
            };

            Ok(rendered.unwrap_or_else(|e| {
                let message = e.to_string();
                with_share_offer(View::message(&message).button(Button::new("refresh", "Refresh").icon("refresh")), &message)
            }))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let _interactive = api::Interactive::start();
        self.ensure_account()?;
        if self.handle_content_action(view, &event, ctx)? {
            return Ok(());
        }
        let incoming_frame = content_tabs::origin_frame(&event);
        let previous_frame = incoming_frame.map(|frame| self.content_origin_frame.replace(frame));
        let handled = ui_kit::web::handle(self, view, &event, ctx, include_str!("ui_config.json"));
        if let Some(previous) = previous_frame {
            self.content_origin_frame = previous;
        }
        if handled? {
            return Ok(());
        }
        if matches!(&event, ViewEvent::Button(id) if id == "new-token") {
            return host::open_url(&api::new_token_url());
        }
        if matches!(&event, ViewEvent::Button(id) if id == "share-project") {
            return self.run_command("share-project", ctx, None).map(|_| ());
        }
        if matches!(view, LIST | DETAILS) && let Err(error) = self.ensure_view_owner(ctx) {
            // Refresh and Repository are the way out of an unresolved repository in an open project; everything else needs one.
            return match &event {
                ViewEvent::Button(id) if (id == "refresh" || id == "repo") && ctx.project().is_some() => self.handle_list(event, ctx),
                _ => Err(error),
            };
        }

        api::begin_budget();
        match view {
            LIST => self.handle_list(event, ctx),
            DETAILS => self.handle_section(event, ctx),
            PROJECTS => self.handle_projects(event, ctx),
            _ => Ok(()),
        }
    }
}

register_plugin!(GitLab, providers, views);

#[cfg(test)]
mod tests;
