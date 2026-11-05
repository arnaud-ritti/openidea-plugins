//! GitLab for OpenIDEA, ported from JetBrains' GitLab plugin
//! (`org.jetbrains.plugins.gitlab`, intellij-community `plugins/gitlab`,
//! Apache-2.0), against gitlab.com with a personal access token: the merge
//! request list, timeline and review flow, the changes with diff comments and
//! pending reviews, the current branch's merge request in the status bar and
//! the editor, checkout, clone and share, Open on GitLab / Copy Link and
//! Create Snippet.

mod api;
mod branch;
mod repo;
mod review;

use std::collections::{BTreeMap, BTreeSet};

use api::{Approvals, Discussion, DraftNote, FileDiff, MergeRequest, ProjectInfo, TimelineEvent, User};
use branch::BranchReview;
use openidea_plugin::host::{self, storage};
use openidea_plugin::providers::{Document, LineMarker, Providers};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Event, Invocation, LogLevel, NotificationLevel, Plugin, RepositoryStatus, register_plugin};

const LIST: &str = "merge-requests";
const DETAILS: &str = "merge-request";
const CHANGES: &str = "merge-request-changes";
const PROJECTS: &str = "gitlab-projects";

const FILTER_KEY: &str = "filter";
const SELECTED_KEY: &str = "selected";
const THREAD_KEY: &str = "thread";
const COMMENT_KEY: &str = "comment";
const AS_DRAFT_KEY: &str = "as-draft";
const EVENTS_KEY: &str = "show-events";

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
const CONFIRMED: [&str; 2] = ["merge", "close"];

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
    projects: Option<Vec<ProjectInfo>>,
    /// The `CONFIRMED` button clicked once, waiting for its second click.
    confirm: Option<String>,
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
                let (iid, thread) = match Key::parse(&key) {
                    Some(Key::MergeRequest(iid)) => (iid, None),
                    Some(Key::Thread(iid, d) | Key::Note(iid, d, _)) => (iid, Some(d)),
                    None => return Ok(()),
                };

                if Self::selected() != Some(iid) {
                    storage::set(SELECTED_KEY, &iid.to_string())?;
                    storage::delete(COMMENT_KEY)?;
                    self.details = None;
                    self.anchor = None;
                    host::refresh_view(CHANGES);
                }

                match thread {
                    Some(d) => storage::set(THREAD_KEY, d)?,
                    None => storage::delete(THREAD_KEY)?,
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
        let mut view = View::new(Body::Form(details_fields(details, thread.as_ref(), comment, as_draft))).button(Button::new("comment", comment_label(as_draft, thread.is_some())).icon("review-comment"));

        if !details.drafts.is_empty() {
            view = view.button(Button::new("submit", &format!("Submit Review ({})", details.drafts.len())).icon("send"));
        }

        if let Some(thread) = thread.as_ref().filter(|t| t.resolvable()) {
            view = view.button(Button::new("resolve", if thread.resolved() { "Unresolve Thread" } else { "Resolve Thread" }).icon("check"));
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
            let merge = match self.confirm.as_deref() {
                Some("merge") => format!("Confirm Merge !{} into {}", mr.iid, mr.target_branch),
                _ => "Merge".into(),
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
            let close = match self.confirm.as_deref() {
                Some("close") => format!("Confirm Close !{}", mr.iid),
                _ => "Close".into(),
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

        let pending = self.confirm.take();
        if CONFIRMED.contains(&button.as_str()) && pending.as_deref() != Some(button.as_str()) {
            self.confirm = Some(button);
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
                self.status = None;
                match Self::selected() {
                    Some(iid) => self.reload_merge_request(iid),
                    None => host::refresh_view(CHANGES),
                }
                Ok(())
            }
            ViewEvent::Button(id) if id == "submit" || id == "browser" => self.handle_details(ViewEvent::Button(id), ctx),
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
fn details_fields(details: &Details, thread: Option<&Discussion>, comment: String, as_draft: bool) -> Vec<Field> {
    let mut fields = Vec::new();

    if let Some(thread) = thread {
        let first = &thread.notes[0];
        let note = format!("{}: {}", first.author.username, first_line(&first.body));
        fields.push(Field::new("replying-to", "Replying to (read-only)", FieldValue::Text(note)));
    }

    let label = if thread.is_some() { "Reply" } else { "Comment" };
    fields.push(Field::new(COMMENT_KEY, label, FieldValue::Multiline(comment)));
    fields.push(Field::new(AS_DRAFT_KEY, "Add to my review (published when you submit the review)", FieldValue::Checkbox(as_draft)));

    if details.mr.state == "opened" {
        fields.push(Field::new("remove-source-branch", "Delete source branch on merge", FieldValue::Checkbox(details.remove_source_branch)));
        fields.push(Field::new("squash", "Squash commits on merge", FieldValue::Checkbox(details.squash)));
    }

    if let Some(description) = details.mr.description.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        fields.push(Field::new("description", "Description (read-only)", FieldValue::Multiline(description.to_owned())));
    }

    fields
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

fn create_snippet(ctx: &Invocation) -> Result<Option<String>, Error> {
    if !host::secret_is_set(api::TOKEN) {
        return Err(NO_TOKEN.into());
    }

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
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_owned();
    let root = ctx.project().and_then(|p| p.vcs_status().ok().flatten()).map(|s| s.root);
    let file_path = root.as_deref().and_then(|r| relative_path(r, &path)).unwrap_or(&name).replace('\\', "/");

    let _task = host::begin_task("Creating snippet", false);
    let url = api::create_snippet(&name, &file_path, &content)?;
    let copied = host::copy_to_clipboard(&url).is_ok();
    let body = if copied { format!("{url}\nLink copied to the clipboard.") } else { url };
    host::notify(NotificationLevel::Info, "Snippet created", &body);

    Ok(None)
}

impl Plugin for GitLab {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
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

impl Providers for GitLab {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        self.branch_line_markers(doc)
    }
}

impl Views for GitLab {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if !host::secret_is_set(api::TOKEN) {
            return Ok(View::message(NO_TOKEN).button(Button::new("new-token", "Create Access Token")));
        }

        let rendered = match view {
            LIST => self.render_list(ctx),
            DETAILS => self.render_details(ctx),
            CHANGES => self.render_changes(ctx),
            PROJECTS => self.render_projects(ctx),
            _ => return Err(Error::NotFound(format!("view {view}"))),
        };

        Ok(rendered.unwrap_or_else(|e| View::message(&e.to_string()).button(Button::new("refresh", "Refresh").icon("refresh"))))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
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
            _ => Ok(()),
        }
    }
}

register_plugin!(GitLab, providers, views);

#[cfg(test)]
mod tests;
