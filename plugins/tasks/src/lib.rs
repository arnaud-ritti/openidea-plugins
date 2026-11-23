//! Tasks: local tasks and issue trackers. One task is active at a time;
//! local tasks are created in the view, issues come from the trackers of
//! `trackers.rs`, and with git each task gets its branch (`git.rs`). The
//! task list of each project is kept in storage.

mod commit_message;
mod git;
mod fogbugz;
mod issue_context;
mod model;
mod search;
mod self_hosted;
mod state;
mod task_search;
mod trackers;
mod view_scope;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Error, Event, Invocation, LogLevel, NotificationLevel, PickItem, Plugin, Project, StatusItem, StatusMenuEntry, host, register_plugin,
};

use git::Git;
use model::{Choices, DEFAULT_TASK_ID, NamedContext, Task, TaskList, trimmed};

const VIEW: &str = "tasks";
/// Native view invalidation; no packaged frame or message bridge is involved.
fn refresh_tasks_view() {
    host::refresh_view(VIEW);
}

const STATUS_ITEM: &str = "active-task";

/// Shown in place of the task tree: the IDE has no dialogs.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Form {
    New {
        summary: String,
        description: String,
    },
    Edit {
        id: String,
        summary: String,
        description: String,
    },
    /// OpenTaskDialog, for an issue that isn't a task yet.
    Open {
        id: String,
    },
    Close {
        id: String,
    },
    CachedOpen { review: issue_context::Review },
}

#[derive(Default)]
struct Tasks {
    git: bool,
    git_notice: std::sync::atomic::AtomicBool,
    scope: view_scope::Scope,
    issue_search: search::Search,
    selected: Option<String>,
    form: Option<Form>,
    /// The checkboxes of the open form.
    choices: Choices,
    collapsed: BTreeSet<String>,
}

impl Plugin for Tasks {
    fn activate(&mut self, ctx: &Context) -> Result<(), Error> {
        self.git = ctx.has("process:git");
        host::remove_status_item(STATUS_ITEM);

        let _ = host::remove_window_status_item(STATUS_ITEM);
        let _ = host::subscribe(openidea_plugin::EventKind::EditorFocused);
        let _ = host::subscribe(openidea_plugin::EventKind::FileOpened);

        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        let _ = host::remove_window_status_item(STATUS_ITEM);
        if matches!(id, "open-issue-at-caret" | "activate-issue-at-caret") {
            issue_context::run(&self.issue_search, ctx, id == "activate-issue-at-caret")?;
            if let Some(root) = ctx.project().and_then(|p| p.root().ok()) {
                if let Ok(tasks) = load(&root) {
                    show_status(&root, &tasks);
                }
            }
            return Ok(None);
        }
        let project = ctx.project().ok_or("No project is open.")?;
        let root = project.root()?;
        if id == "reset-project-state" {
            state::recover(&root)?;
            self.scope.clear();
            self.form = None;
            self.selected = None;
            refresh_tasks_view();
            return Ok(None);
        }
        let (tasks, stamp) = state::snapshot(&root)?;
        if self.scope.bind(&root, stamp) {
            self.form = None;
            self.selected = None;
            self.collapsed.clear();
        }
        self.scope.rotate();
        // Even a cancelled command invalidates the old rendered action tokens.
        refresh_tasks_view();
        match id {
            "review-issue-branch-at-caret" => {
                if !self.git { return Err(ui_kit::denied::message("Reviewing branch options", "process:git", "Tasks").into()); }
                let review = issue_context::prepare(&self.issue_search, ctx)?;
                if tasks.find(&review.issue.id).is_some() {
                    return Err("This issue already has a local task; use its existing task switch/branch workflow.".into());
                }
                self.choices = tasks.choices.clone();
                self.choices.create_branch = true;
                self.form = Some(Form::CachedOpen { review });
                refresh_tasks_view();
                host::reveal_view(VIEW)?;
            }
            // Closing can merge and delete the branch and close the issue: the user reviews the choices first.
            "close" => {
                if tasks.active().is_default() {
                    return Err("The default task can't be closed.".into());
                }

                self.show_form(Form::Close { id: tasks.active.clone() }, &tasks);
                refresh_tasks_view();
                if host::reveal_view(VIEW).is_err() {
                    host::notify(NotificationLevel::Info, "Close the active task", "Review the close options in the Tasks tool window.");
                }
            }
            "switch" if arg.is_some() => {
                let id = arg.unwrap_or_default();
                if id != tasks.active {
                    tasks.find(id).filter(|task| !task.closed).ok_or("This task is closed or no longer listed. Choose another with Tasks: Switch Task.")?;
                    self.switch_task(&project, id, false, &tasks.choices)?;
                    refresh_tasks_view();
                }
            }
            "switch" => {
                let items: Vec<PickItem> = tasks
                    .tasks
                    .iter()
                    .filter(|t| !t.closed && t.id != tasks.active)
                    .map(|t| PickItem::new(&t.id, &format!("{} {}", t.id, t.summary)))
                    .collect();
                if items.is_empty() {
                    return Err("There is no other open task to switch to.".into());
                }

                if let Some(id) = host::pick("Switch Task", &items)? {
                    self.switch_task(&project, &id, false, &tasks.choices)?;
                    refresh_tasks_view();
                }
            }
            "open-in-browser" => {
                let url = tasks.active().url.as_deref().ok_or("The active task isn't an issue, so it has no page to open.")?;
                host::open_url(url)?;
            }
            "apply-commit-message" => commit_message::run(&project)?,
            "copy-commit-message" => {
                let message = tasks.commit_message(&commit_message_format())?;
                host::copy_to_clipboard(&message).map_err(|e| ui_kit::denied::map(e, "Copy Commit Message", "clipboard", "Tasks"))?;
                host::notify(NotificationLevel::Info, "Commit message copied", &message);
            }
            "new-task" => {
                self.button("new", ctx)?;
                refresh_tasks_view();
                let _ = host::reveal_view(VIEW);
            }
            "open-task" => {
                let previous = self.selected.take();
                let result = self.button("find", ctx);
                if self.selected.is_none() {
                    self.selected = previous;
                }
                result?;
                refresh_tasks_view();
                if self.form.is_some() {
                    let _ = host::reveal_view(VIEW);
                }
            }
            "refresh" => refresh_issues(&project, false)?,
            "search-issues" => self.search_issues(&project, &tasks, ctx)?,
            "save-context" => save_context(&project, &tasks, ctx)?,
            "load-context" => load_context(&project, &tasks)?,
            "remove-context" => remove_context(&project, &tasks)?,
            "add-repository" => add_repository(&project)?,
            "remove-repository" => remove_repository(&project, &tasks)?,
            _ => return Err(Error::NotFound(format!("command {id}"))),
        }

        show_status(&root, &load(&root)?);
        Ok(None)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        let mut opened = Vec::new();

        for event in events {
            match event {
                Event::EditorFocused(path) => {
                    if let Some(root) = ctx.project().and_then(|p| p.root().ok()).filter(|root| {
                        path.as_ref().is_some_and(|p| host::project_of(p).and_then(|p| p.root().ok()).as_deref() == Some(root))
                    }) {
                        if let Ok(tasks) = load(&root) {
                            show_status(&root, &tasks);
                        } else {
                            let _ = host::remove_window_status_item(STATUS_ITEM);
                        }
                    } else {
                        let _ = host::remove_window_status_item(STATUS_ITEM);
                    }
                }
                Event::FileOpened(path) => opened.push(path.clone()),
                Event::Scheduled => update_all_issues(),
                _ => {}
            }
        }

        if opened.is_empty() {
            return;
        }

        let Some(root) = ctx.project().and_then(|p| p.root().ok()) else {
            return;
        };

        opened.retain(|path| state::owns(&root, path) && host::project_of(path).and_then(|p| p.root().ok()).as_deref() == Some(&root));
        if opened.is_empty() {
            return;
        }
        let recorded = update(&root, |tasks, _| {
            opened
                .iter()
                .filter(|path| state::owns(&root, path) && host::project_of(path).and_then(|p| p.root().ok()).as_deref() == Some(&root))
                .for_each(|path| tasks.record_file(path));
            Ok(())
        });

        if recorded.is_ok() && self.scope.owner_is(&root) {
            self.scope.after(ctx);
        }
        if let Err(e) = recorded {
            host::log(LogLevel::Warn, &format!("recording opened files: {e}"));
        }
    }
}

impl Views for Tasks {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            let Some(project) = ctx.project() else {
                self.scope.clear();
                self.form = None;
                self.selected = None;
                self.collapsed.clear();
                let _ = host::remove_window_status_item(STATUS_ITEM);
                return Ok(View::message("Open a project to work on tasks."));
            };

            let root = match project.root() {
                Ok(root) => root,
                Err(error) => {
                    self.scope.clear(); self.form = None; self.selected = None; self.collapsed.clear();
                    let _ = host::remove_window_status_item(STATUS_ITEM);
                    return Err(error);
                }
            };
            let (tasks, stamp) = match state::snapshot(&root) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.scope.clear();
                    self.form = None;
                    self.selected = None;
                    self.collapsed.clear();
                    let _ = host::remove_window_status_item(STATUS_ITEM);
                    return Err(error);
                }
            };
            show_status(&root, &tasks);
            if self.scope.bind(&root, stamp) {
                self.form = None;
                self.selected = None;
                self.collapsed.clear();
            }
            if let Some(form) = &self.form {
                return Ok(self.scope.view(self.render_form(form, &tasks)));
            }

            let now = now();
            let selected = self.selected.as_deref().and_then(|key| selected_task(&tasks, key));

            let mut items = vec![self.group("open", &format!("Tasks ({})", tasks.tasks.iter().filter(|t| !t.closed).count()))];

            for task in tasks.tasks.iter().filter(|t| !t.closed) {
                let spent = duration(tasks.seconds_spent(task, now));
                let detail = match (task.id == tasks.active, &task.branch) {
                    (true, Some(branch)) => format!("active · {spent} · {branch}"),
                    (true, None) => format!("active · {spent}"),
                    (false, Some(branch)) => format!("{spent} · {branch}"),
                    (false, None) => spent,
                };

                let icon = match (task.id == tasks.active, task.is_issue()) {
                    (true, _) => "star",
                    (false, true) => "github-issue-open",
                    (false, false) => "todo",
                };

                items.push(TreeItem::new(&format!("task:{}", task.id), &task.label()).parent("open").icon(icon).detail(&detail));
            }

            if tasks.tasks.iter().any(|t| t.closed) {
                items.push(self.group("closed", &format!("Closed ({})", tasks.tasks.iter().filter(|t| t.closed).count())));

                for task in tasks.tasks.iter().filter(|t| t.closed) {
                    let detail = duration(task.seconds_spent);
                    let item = TreeItem::new(&format!("task:{}", task.id), &task.label()).parent("closed");
                    items.push(item.icon("github-issue-closed").detail(&detail));
                }
            }

            items.push(self.group("issues", &format!("Issues ({})", tasks.pending_issues().count())));

            for issue in tasks.pending_issues() {
                let item = TreeItem::new(&format!("issue:{}", issue.id), &issue.label()).parent("issues");
                items.push(item.icon("github-issue-open").detail(trackers::name(issue)));
            }

            if tasks.pending_issues().next().is_none() {
                let hint = no_issues_hint(&root);
                items.push(TreeItem::new("no-issues", hint).parent("issues").icon("info"));
            }

            let (open_label, open_enabled) = match selected {
                Some(task) if task.id == tasks.active => ("Active Task", false),
                Some(task) if task.closed => ("Reopen Task", true),
                Some(task) if tasks.find(&task.id).is_some() => ("Switch Task", true),
                _ => ("Open Task…", true),
            };
            Ok(self.scope.view(
                View::new(Body::Tree(items))
                    .button(Button::new("new", "New Task…").icon("plus"))
                    .button(Button::new("find", open_label).icon("run").enabled(open_enabled))
                    .button(Button::new("close-active", "Close Active Task…").icon("check").enabled(!tasks.active().is_default()))
                    .button(Button::new("actions", "Task Actions…").icon("edit").enabled(selected.is_some()))
                    .button(Button::new("more", "More…").icon("more"))
                    .status(&format!("Active: {} · {}", tasks.active().label(), selected.map(|task| format!("Selected: {}", task.label())).unwrap_or_else(|| "Select a task; Enter opens it".into()))),
            ))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if _view != VIEW || matches!(&event, ViewEvent::Message(_) | ViewEvent::Other { .. }) {
            return Err(Error::Invalid("Tasks accepts native view events only.".into()));
        }

        state::clear_receipt();
        let event = match self.scope.decode(event, ctx) {
            Ok(event) => event,
            Err(error) => {
                refresh_tasks_view();
                return Err(error);
            }
        };
        if self.form.is_some()
            && matches!(&event, ViewEvent::Select(_) | ViewEvent::Activate(_) | ViewEvent::Expand(_) | ViewEvent::Collapse(_))
        {
            return Err(Error::Stale);
        }
        let rotate = matches!(&event, ViewEvent::Select(_) | ViewEvent::Activate(_) | ViewEvent::Button(_));
        let result = match event {
            ViewEvent::Select(key) => {
                self.selected = Some(key);
                Ok(())
            }
            ViewEvent::Activate(key) if key.starts_with("task:") || key.starts_with("issue:") => {
                self.selected = Some(key);
                self.start_selected(ctx)
            }
            ViewEvent::Expand(key) => {
                self.collapsed.remove(&key);
                Ok(())
            }
            ViewEvent::Collapse(key) => {
                self.collapsed.insert(key);
                Ok(())
            }
            ViewEvent::Field(key, value) => {
                self.field(&key, value);
                Ok(())
            }
            ViewEvent::Button(button) => self.button(&button, ctx),
            _ => Ok(()),
        };

        if rotate {
            self.scope.rotate();
        }
        self.scope.after(ctx);
        if let Some(root) = ctx.project().and_then(|p| p.root().ok()) {
            if let Ok(tasks) = load(&root) {
                show_status(&root, &tasks);
            }
        }
        refresh_tasks_view();

        result
    }
}

impl Tasks {
    fn group(&self, key: &str, label: &str) -> TreeItem {
        TreeItem::new(key, label).expandable(!self.collapsed.contains(key))
    }

    fn render_form(&self, form: &Form, tasks: &TaskList) -> View {
        let checkbox = |key: &str, label: &str, value: bool| Field::new(key, label, FieldValue::Checkbox(value));
        let vcs_fields = || match self.git {
            true => vec![
                checkbox("create_branch", "Create branch", self.choices.create_branch),
                checkbox("shelve_changes", "Shelve changes when switching tasks", self.choices.shelve_changes),
            ],
            false => Vec::new(),
        };

        let (mut fields, ok, enabled, status) = match form {
            Form::New { summary, description } => (
                vec![
                    Field::new("summary", "Summary", FieldValue::Text(summary.clone())),
                    Field::new("description", "Description", FieldValue::Multiline(description.clone())),
                ],
                "Create Task",
                !summary.trim().is_empty(),
                "Create New Task".to_owned(),
            ),
            Form::Edit { summary, description, .. } => (
                vec![
                    Field::new("summary", "Summary", FieldValue::Text(summary.clone())),
                    Field::new("description", "Description", FieldValue::Multiline(description.clone())),
                ],
                "Save",
                !summary.trim().is_empty(),
                "Edit task".to_owned(),
            ),
            Form::Open { id } => {
                let label = tasks.find_any(id).map(Task::label).unwrap_or_default();
                (vec![Field::new("review-task", "Task", FieldValue::label(&label))], "Open Task", true, "Open Task · review VCS options".into())
            }
            Form::CachedOpen { review } => (
                vec![Field::new("review-task", "Task", FieldValue::label(&review.issue.label()))], "Open Task", true,
                format!("Review cached issue {} — {}. Branch: {} (create from HEAD or switch existing). Source/configuration rechecked on Open Task.", review.issue.id, review.issue.summary, git::branch_name(&review.issue, &review.branch_format)),
            ),
            Form::Close { id } => {
                let task = tasks.find(id);
                let mut fields = vec![Field::new("review-task", "Task to close", FieldValue::label(&task.map(Task::label).unwrap_or_default()))];

                if task.is_some_and(trackers::can_update_state) {
                    fields.push(checkbox("update_state", "Update issue state in the tracker", self.choices.update_state));
                }

                if let Some((branch, base)) = task.and_then(|t| Some((t.branch.as_ref()?, t.base_branch.as_ref()?))).filter(|_| self.git) {
                    let label = format!("Merge branch {branch} into {base}, then delete {branch}");
                    fields.push(checkbox("merge_branch", &label, self.choices.merge_branch));
                }

                (fields, "Close Task", true, task.map(Task::label).unwrap_or_default())
            }
        };

        if matches!(form, Form::New { .. } | Form::Open { .. } | Form::CachedOpen { .. }) {
            fields.extend(vcs_fields());
        }

        View::new(Body::Form(fields))
            .button(Button::new("ok", ok).icon("check").enabled(enabled))
            .button(Button::new("cancel", "Cancel").icon("close"))
            .status(&status)
    }

    fn field(&mut self, key: &str, value: String) {
        let checked = value == "true";

        match (key, &mut self.form) {
            ("summary", Some(Form::New { summary, .. } | Form::Edit { summary, .. })) => *summary = value,
            ("description", Some(Form::New { description, .. } | Form::Edit { description, .. })) => *description = value,
            ("create_branch", _) => self.choices.create_branch = checked,
            ("shelve_changes", _) => self.choices.shelve_changes = checked,
            ("merge_branch", _) => self.choices.merge_branch = checked,
            ("update_state", _) => self.choices.update_state = checked,
            _ => {}
        }
    }

    fn button(&mut self, button: &str, ctx: &Invocation) -> Result<(), Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let tasks = load(&project.root()?)?;

        match button {
            "new" => self.show_form(Form::New { summary: String::new(), description: String::new() }, &tasks),
            "edit" => {
                let task = self.selected_task(&tasks)?;
                if task.is_default() || task.is_issue() { return Err("Select a local task to edit.".into()); }
                let form = Form::Edit { id: task.id.clone(), summary: task.summary.clone(), description: task.description.clone() };
                self.show_form(form, &tasks);
            }
            "close-active" => {
                if tasks.active().is_default() { return Err("The default task cannot be closed.".into()); }
                self.show_form(Form::Close { id: tasks.active.clone() }, &tasks);
            }
            "close" => {
                let task = self.selected_task(&tasks)?;
                if task.is_default() || task.closed || tasks.find(&task.id).is_none() { return Err("Select an open local task to close.".into()); }
                let id = task.id.clone();
                self.show_form(Form::Close { id }, &tasks);
            }
            "switch" => self.start_selected(ctx)?,
            "post-time" => {
                let task = self.selected_task(&tasks)?;
                let seconds = unposted(&tasks, task, now());

                if seconds < 60 {
                    return Err("There is less than a minute of work time to post.".into());
                }

                trackers::post_time(task, seconds)?;

                let id = task.id.clone();
                update(&project.root()?, |tasks, _| {
                    tasks.find_mut(&id).ok_or("The task no longer exists.")?.seconds_posted += seconds;
                    Ok(())
                })?;
                host::notify(NotificationLevel::Info, "Work time posted", &format!("{} to {id}", duration(seconds)));
            }
            "browse" => {
                let url = self.selected_task(&tasks)?.url.clone().ok_or("The task isn't an issue, so it has no page to open.")?;
                host::open_url(&url)?;
            }
            "actions" => {
                let task = self.selected_task(&tasks)?;
                let mut actions = vec![PickItem::new("details", "Show Task Description")];
                if task.id != tasks.active { actions.push(PickItem::new("switch", "Switch to Task")); }
                if tasks.find(&task.id).is_some() && !task.is_default() && !task.closed { actions.push(PickItem::new("close", "Close Selected Task…")); }
                if !task.is_default() && !task.is_issue() { actions.push(PickItem::new("edit", "Edit Task…")); }
                if tasks.find(&task.id).is_some() && trackers::can_post_time(task) && unposted(&tasks, task, now()) >= 60 {
                    actions.push(PickItem::new("post-time", &format!("Post Work Time ({})", duration(unposted(&tasks, task, now())))));
                }
                if task.is_issue() { actions.push(PickItem::new("browse", "Open in Browser")); }
                if let Some(action) = host::pick("Task Actions", &actions)? {
                    if load(&project.root()?)? != tasks || !actions.iter().any(|allowed| allowed.key == action) { return Err(Error::Stale); }
                    self.button(&action, ctx)?;
                }
            }
            "details" => {
                let task = self.selected_task(&tasks)?;
                let identity = format!("{}:{}", project.root()?, task.id);
                let key = identity.bytes().fold(0xcbf29ce484222325u64, |h,b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
                host::open_content_tab(&openidea_plugin::ContentTab {
                    key: format!("task-description:{key:016x}"), title: task.label().chars().scan(0usize, |bytes, c| { *bytes += c.len_utf8(); (*bytes <= 256).then_some(c) }).collect(),
                    language: Some("plaintext".into()), content: format!("{}\n\n{}", task.label(), task.description), read_only: true,
                    ..Default::default()
                })?;
            }
            "find" => {
                // Selection already identifies the target: Enter and Open share the same review.
                if let Some(task) = self.selected.as_deref().and_then(|key| selected_task(&tasks, key)) {
                    if task.id != tasks.active { self.start_selected(ctx)?; }
                    return Ok(());
                }
                let candidates: Vec<_> = tasks.tasks.iter().filter(|t| !t.closed).map(|t| (format!("task:{}", t.id), t))
                    .chain(tasks.pending_issues().map(|t| (format!("issue:{}", t.id), t))).collect();
                let items: Vec<_> = candidates.iter().map(|(key, task)| PickItem::new(key, &task.label()).description(if task.id == tasks.active { "Active task" } else { &task.description })).collect();
                if let Some(key) = host::pick("Open Task", &items)? {
                    if load(&project.root()?)? != tasks || !candidates.iter().any(|(allowed, _)| allowed == &key) { return Err(Error::Stale); }
                    self.selected = Some(key);
                    self.start_selected(ctx)?;
                }
            }
            "search-issues" => self.search_issues(&project, &tasks, ctx)?,
            "more" => {
                let choices = [
                    PickItem::new("search-issues", "Search Issues…"),
                    PickItem::new("refresh", "Update Issues"),
                    PickItem::new("save-context", "Save Context…"),
                    PickItem::new("load-context", "Load Context…"),
                    PickItem::new("remove-context", "Remove Saved Context…"),
                ];
                if let Some(action) = host::pick("More Tasks actions", &choices)? {
                    if load(&project.root()?)? != tasks { return Err(Error::Stale); }
                    match action.as_str() {
                        "search-issues" => self.search_issues(&project, &tasks, ctx)?,
                        "refresh" => refresh_issues(&project, false)?,
                        "save-context" => save_context(&project, &tasks, ctx)?,
                        "load-context" => load_context(&project, &tasks)?,
                        "remove-context" => remove_context(&project, &tasks)?,
                        _ => return Err(Error::Stale),
                    }
                }
            }
            "refresh" => refresh_issues(&project, false)?,
            "ok" => self.submit(&project, ctx)?,
            "cancel" => self.form = None,
            _ => {}
        }

        Ok(())
    }

    fn show_form(&mut self, form: Form, tasks: &TaskList) {
        self.choices = tasks.choices.clone();
        self.form = Some(form);
    }

    fn submit(&mut self, project: &Project, ctx: &Invocation) -> Result<(), Error> {
        if let Some(Form::CachedOpen { review }) = &self.form {
            review.validate(&self.issue_search, ctx)?;
        }
        if let Some(Form::New { summary, description } | Form::Edit { summary, description, .. }) = &self.form {
            if summary.trim().is_empty() || summary.len() > 4096 || description.len() > 32 * 1024 {
                return Err("A task needs a nonempty summary (up to 4096 bytes) and a bounded description (32 KiB).".into());
            }
        }
        let Some(form) = self.form.take() else {
            return Ok(());
        };

        let root = project.root()?;
        let choices = self.choices.clone();

        if !matches!(form, Form::Edit { .. }) {
            update(&root, |tasks, _| {
                tasks.choices = choices.clone();
                Ok(())
            })?;
        }

        match form {
            Form::New { summary, description } => {
                if summary.trim().is_empty() {
                    self.form = Some(Form::New { summary, description });
                    return Err("A task needs a summary.".into());
                }

                let id = update(&root, |tasks, _| Ok(tasks.add_local(summary.trim(), &description)))?;
                self.switch_task(project, &id, true, &choices)?;
                self.selected = Some(format!("task:{id}"));
            }
            Form::Edit { id, summary, description } => {
                update(&root, |tasks, _| tasks.edit(&id, summary.trim(), &description))?;
            }
            Form::Open { id } => {
                self.switch_task(project, &id, true, &choices)?;
                self.selected = Some(format!("task:{id}"));
            }
            Form::CachedOpen { review } => {
                // The retained source/configuration ticket was checked before
                // changing choices, importing the issue, activating, or running Git.
                let id = review.issue.id.clone();
                update(&root, |tasks, _| {
                    if tasks.find(&id).is_some() { return Err(Error::Stale); }
                    tasks.issues.retain(|issue| issue.id != id);
                    tasks.issues.push(review.issue.clone());
                    Ok(())
                })?;
                self.switch_task(project, &id, true, &choices)?;
                self.selected = Some(format!("task:{id}"));
            }
            Form::Close { id } => self.close_task(project, &id, &choices)?,
        }

        Ok(())
    }

    fn selected_task<'a>(&self, tasks: &'a TaskList) -> Result<&'a Task, Error> {
        self.selected.as_deref().and_then(|key| selected_task(tasks, key)).ok_or_else(|| "Select a task first.".into())
    }

    fn search_issues(&mut self, project: &Project, _tasks: &TaskList, ctx: &Invocation) -> Result<(), Error> {
        let Some(issue) = self.issue_search.pick(project, ctx)? else {
            return Ok(());
        };
        let root = project.root()?;
        let current = load(&root)?;
        if let Some(existing) = current.find_any(&issue.id) {
            if existing.tracker != issue.tracker || existing.api != issue.api {
                return Err("This issue ID belongs to a different existing task/server. It cannot be replaced by search.".into());
            }
        }
        let id = issue.id.clone();

        update(&root, |tasks, _| {
            if let Some(existing) = tasks.find_any(&id) {
                if existing.tracker != issue.tracker || existing.api != issue.api {
                    return Err("The issue ID now belongs to a different task/server.".into());
                }
            } else {
                tasks.issues.push(issue.clone());
            }
            // A fresh explicit credential-bridge search renews its non-secret
            // event/profile proof while preserving local task time/branch/context.
            if matches!(issue.tracker.as_deref(),Some("fogbugz"|"redmine"|"mantis"|"generic")) {
                if let Some(existing)=tasks.tasks.iter_mut().chain(tasks.issues.iter_mut()).find(|t|t.id==id) {
                    existing.summary=issue.summary.clone(); existing.description=issue.description.clone();
                    existing.url=issue.url.clone(); existing.api=issue.api.clone(); existing.fogbugz=issue.fogbugz.clone(); existing.self_hosted=issue.self_hosted.clone();
                }
            }
            Ok(())
        })?;

        self.selected = Some(format!("{}:{id}", if current.find(&id).is_some() { "task" } else { "issue" }));

        self.start_selected(ctx)?;

        if self.form.is_some() {
            refresh_tasks_view();
            if host::reveal_view(VIEW).is_err() {
                host::notify(NotificationLevel::Info, "Open the issue", "Review the branch options in the Tasks tool window.");
            }
        }

        Ok(())
    }

    /// Activating an issue first asks, like OpenTaskDialog, whether to start a branch.
    fn start_selected(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let tasks = load(&project.root()?)?;
        let task = self.selected_task(&tasks)?;
        let id = task.id.clone();

        if tasks.find(&id).is_some() {
            return self.switch_task(&project, &id, false, &tasks.choices);
        }

        if self.git {
            self.show_form(Form::Open { id }, &tasks);
            return Ok(());
        }

        self.switch_task(&project, &id, true, &tasks.choices)?;
        self.selected = Some(format!("task:{id}"));

        Ok(())
    }

    /// Activating a task: shelve the changes of the task being
    /// left, check out the task's branch (or start one for a new task),
    /// restore its changes, its commit message and the files it had open.
    fn switch_task(&self, project: &Project, id: &str, new_task: bool, choices: &Choices) -> Result<(), Error> {
        let root = project.root()?;
        if let Some(task) = load(&root)?.find_any(id) {
            owned_files(project, &task.files)?;
        }
        let _progress = host::begin_task("Switching task", false);
        let git = self.git.then(|| Git::new(project));

        if git.is_none() && new_task && choices.create_branch {
            ui_kit::denied::notify_once(&self.git_notice, "Creating a branch per task", "process:git", "Tasks");
        }

        let mut warnings = Vec::new();

        let files = update(&root, |tasks, now| {
            let previous = tasks.active.clone();

            if previous == id {
                return Ok(Vec::new());
            }

            let mut branch_before = None;

            if let Some(git) = &git {
                if choices.shelve_changes {
                    note(&mut warnings, git.shelve(&previous));
                }

                branch_before = git.current_branch().ok().flatten();
            }

            tasks.activate(id, now)?;

            let Some(git) = &git else {
                return Ok(tasks.active().files.clone());
            };

            let task = tasks.active().clone();

            if new_task && choices.create_branch {
                let name = git::branch_name(&task, &setting_or("branchNameFormat", "{id}"));

                match git.start_branch(&name) {
                    Ok(()) => {
                        // The task being left keeps the branch it was on.
                        if let Some(left) = tasks.find_mut(&previous).filter(|t| t.branch.is_none()) {
                            left.branch = branch_before.clone();
                        }

                        let active = tasks.active_mut();
                        active.branch = Some(name);
                        active.base_branch = branch_before;
                    }
                    Err(e) => note(&mut warnings, Err(e)),
                }
            } else if let Some(branch) = &task.branch {
                note(&mut warnings, git.switch(branch));
            }

            if choices.shelve_changes {
                note(&mut warnings, git.unshelve(id));
            }

            if host::setting_bool("fillCommitTemplate") == Some(true) {
                let message = (!task.is_default()).then(|| task.format(&commit_message_format()));
                note(&mut warnings, git.set_commit_template(message.as_deref()));
            }

            Ok(tasks.active().files.clone())
        })?;

        for file in files {
            let _ = host::open_location(&file, 0, 0);
        }

        warn("Switched task, but some steps failed", &warnings);

        Ok(())
    }

    /// CloseTaskAction and CloseTaskDialog: optionally resolve the issue and
    /// merge the task's branch, then mark it closed and go back to the default task.
    fn close_task(&self, project: &Project, id: &str, choices: &Choices) -> Result<(), Error> {
        let root = project.root()?;
        let tasks = load(&root)?;
        let task = tasks.find(id).ok_or_else(|| format!("Task {id} not found."))?.clone();

        if task.is_default() {
            return Err("The default task can't be closed.".into());
        }

        let _progress = host::begin_task(&format!("Closing {}", task.label()), false);
        let mut warnings = Vec::new();

        if choices.update_state && trackers::can_update_state(&task) {
            match trackers::close_issue(&task) {
                Err(Error::Denied(_)) => warnings.push(format!("The issue state was not updated: allow network access to {} in Settings › Plugins › Tasks.", trackers::name(&task))),
                result => note(&mut warnings, result),
            }
        }

        if tasks.active == id {
            self.switch_task(project, DEFAULT_TASK_ID, false, choices)?;
        }

        if let (true, true, Some(branch), Some(base)) = (choices.merge_branch, self.git, &task.branch, &task.base_branch) {
            note(&mut warnings, Git::new(project).merge_and_delete(branch, base));
        }

        update(&root, |tasks, _| tasks.close(id))?;
        match warnings.is_empty() {
            true => host::notify(NotificationLevel::Info, "Task closed", &format!("Closed {}", task.label())),
            false => warn(&format!("Closed {}, but some steps failed", task.id), &warnings),
        }

        Ok(())
    }
}

/// Git or a tracker failing doesn't stop the switch; the user is told.
/// A missing capability is a choice of the user, not a problem.
fn note(warnings: &mut Vec<String>, result: Result<(), Error>) {
    match result {
        Ok(()) | Err(Error::Denied(_)) => {}
        Err(e) => warnings.push(e.to_string()),
    }
}

fn warn(title: &str, warnings: &[String]) {
    if !warnings.is_empty() {
        host::notify(NotificationLevel::Warning, title, &warnings.join("\n"));
    }
}

fn selected_task<'a>(tasks: &'a TaskList, key: &str) -> Option<&'a Task> {
    if let Some(id) = key.strip_prefix("task:") {
        return tasks.find(id);
    }

    let id = key.strip_prefix("issue:")?;

    tasks.issues.iter().find(|t| t.id == id)
}

/// Updates issues every 20 minutes (`on-schedule:1200`).
fn update_all_issues() {
    if host::setting_bool("updateIssuesAutomatically") == Some(false) {
        return;
    }

    for project in host::projects().unwrap_or_default() {
        if let Err(e) = refresh_issues(&project, true) {
            host::log(LogLevel::Warn, &format!("updating issues: {e}"));
        }
    }
}

fn capture_files(root: &str) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    for editor in host::open_editors()? {
        if state::owns(root, &editor.path)
            && host::project_of(&editor.path).and_then(|p| p.root().ok()).as_deref() == Some(root)
            && !files.contains(&editor.path)
        { files.push(editor.path); }
    }
    if files.len() > 10 { return Err("At most ten open project editors can be saved in a context.".into()); }
    Ok(files)
}

fn save_context(project: &Project, tasks: &TaskList, ctx: &Invocation) -> Result<(), Error> {
    let root = project.root()?;
    let files = capture_files(&root)?;
    let selection = ctx.editor().and_then(|editor| {
        let path = editor.path();
        if !files.contains(&path) { return None; }
        editor.selections().ok()?.first().copied().map(|range| (path, range))
    });

    if files.is_empty() {
        return Err("This window has no open files owned by the current project to save.".into());
    }

    let Some(name) = host::input("Save context as", None)?.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) else {
        return Ok(());
    };

    if name.len() > 128 || name.chars().any(char::is_control) {
        return Err("Use a context name of at most 128 bytes without control characters.".into());
    }
    if load(&root)? != *tasks || capture_files(&root)? != files {
        return Err(Error::Stale);
    }
    update(&root, |current, _| {
        if *current != *tasks {
            return Err(Error::Stale);
        }
        if current.contexts.len() >= 64 && !current.contexts.iter().any(|c| c.name == name) {
            return Err("Saved context limit (64) reached.".into());
        }
        let tasks = current;
        tasks.contexts.retain(|c| c.name != name);
        tasks.contexts.push(NamedContext { name: name.clone(), files, selection });
        Ok(())
    })?;
    host::notify(NotificationLevel::Info, "Context saved", &name);

    Ok(())
}

fn pick_context(tasks: &TaskList, title: &str) -> Result<Option<NamedContext>, Error> {
    if tasks.contexts.is_empty() {
        return Err("There is no saved context.".into());
    }

    let items: Vec<PickItem> =
        tasks.contexts.iter().map(|c| PickItem::new(&c.name, &c.name).description(&format!("{} files", c.files.len()))).collect();

    match host::pick(title, &items)? {
        None => Ok(None),
        Some(name) => tasks
            .contexts
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .map(Some)
            .ok_or_else(|| Error::Invalid("Unknown context selection.".into())),
    }
}

fn owned_files(project: &Project, files: &[String]) -> Result<(), Error> {
    let root = project.root()?;
    if files.len() > 10
        || files
            .iter()
            .any(|file| !state::owns(&root, file) || host::project_of(file).and_then(|p| p.root().ok()).as_deref() != Some(&root))
    {
        return Err("Recorded files no longer belong to this exact project.".into());
    }
    Ok(())
}
fn load_context(project: &Project, tasks: &TaskList) -> Result<(), Error> {
    let root = project.root()?;
    let Some(context) = pick_context(tasks, "Reopen Recorded Context Files")? else {
        return Ok(());
    };
    if load(&root)? != *tasks {
        return Err(Error::Stale);
    }
    owned_files(project, &context.files)?;
    for (opened, file) in context.files.iter().enumerate() {
        if let Err(error) = host::open_location(file, 0, 0) {
            return Err(Error::Failed(format!("Opened {opened} of {} recorded context files; {error}", context.files.len())));
        }
    }
    if let Some((path, range)) = context.selection {
        // A saved selection is advisory after edits: only restore valid current coordinates.
        let text = project.read_file(&path)?;
        if text.len() <= 1024 * 1024
            && openidea_plugin::text::offset(&text, range.start).is_some()
            && openidea_plugin::text::offset(&text, range.end).is_some()
        { host::open_editor(&path, Some(range))?; }
    }
    Ok(())
}

fn remove_context(project: &Project, tasks: &TaskList) -> Result<(), Error> {
    let Some(context) = pick_context(tasks, "Remove Context")? else {
        return Ok(());
    };

    if !host::confirm("Remove context", &format!("Remove the saved context \"{}\"?", context.name))? {
        return Ok(());
    }

    let root = project.root()?;
    if load(&root)? != *tasks {
        return Err(Error::Stale);
    }
    update(&root, |current, _| {
        if *current != *tasks {
            return Err(Error::Stale);
        }
        current.contexts.retain(|c| c.name != context.name);
        Ok(())
    })
}

fn add_repository(project: &Project) -> Result<(), Error> {
    let kinds = ["github", "gitlab", "forgejo"];
    let items: Vec<PickItem> = kinds.iter().map(|k| PickItem::new(k, trackers::tracker_name(k))).collect();

    let Some(kind) = host::pick("Add repository to", &items)? else {
        return Ok(());
    };

    let prompt = match kind.as_str() {
        "gitlab" => "GitLab project as group/name",
        _ => "Repository as owner/name",
    };

    let Some(repository) = host::input(prompt, None)?.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty()) else {
        return Ok(());
    };

    let valid = match kind.as_str() {
        "gitlab" => repository.contains('/'),
        _ => trackers::valid_repo(&repository),
    };

    if !valid {
        return Err("Enter the repository as owner/name.".into());
    }

    update(&project.root()?, |tasks, _| {
        let list = tasks.repositories.entry(kind).or_default();

        if !list.contains(&repository) {
            list.push(repository);
        }
        Ok(())
    })?;

    refresh_issues(project, false)
}

fn remove_repository(project: &Project, tasks: &TaskList) -> Result<(), Error> {
    let items: Vec<PickItem> = tasks
        .repositories
        .iter()
        .filter(|(kind, _)| trackers::is_repository_tracker(kind))
        .flat_map(|(kind, list)| {
            list.iter().map(move |r| PickItem::new(&format!("{kind} {r}"), r).description(trackers::tracker_name(kind)))
        })
        .collect();

    if items.is_empty() {
        return Err("No repository was added; issues come from the settings or the git remote.".into());
    }

    let Some(key) = host::pick("Remove repository", &items)? else {
        return Ok(());
    };

    let Some((kind, repository)) = key.split_once(' ') else {
        return Ok(());
    };

    update(&project.root()?, |tasks, _| {
        if let Some(list) = tasks.repositories.get_mut(kind) {
            list.retain(|r| r != repository);
        }
        tasks.repositories.retain(|_, list| !list.is_empty());
        Ok(())
    })?;

    refresh_issues(project, false)
}

thread_local! {
    /// Why the last issue update of a project root loaded nothing, when every tracker failed.
    static ISSUE_FAILURES: std::cell::RefCell<BTreeMap<String, &'static str>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The text of the Issues group's placeholder row.
fn no_issues_hint(root: &str) -> &'static str {
    if let Some(failure) = ISSUE_FAILURES.with_borrow(|failures| failures.get(root).copied()) {
        return failure;
    }
    if !trackers::any_configured() {
        return "No issue tracker set up: add a tracker token in Settings › Plugins › Tasks";
    }
    "No open issues"
}

/// `quiet`: problems go to the log instead of a notification.
fn refresh_issues(project: &Project, quiet: bool) -> Result<(), Error> {
    let root = project.root()?;
    let _progress = host::begin_task("Updating issues", false);
    let remotes = project.vcs_remotes().unwrap_or_default();
    let repositories = load(&root)?.repositories;
    let fetched = trackers::fetch_all(&trackers::Scope { remotes: &remotes, query: None, page: 1, repositories: &repositories });
    ISSUE_FAILURES.with_borrow_mut(|failures| failures.remove(&root));

    if fetched.trackers == 0 {
        if !quiet {
            host::notify(
                NotificationLevel::Info,
                "No issue tracker set up",
                "Add a tracker token in Settings › Plugins › Tasks. Trackers that use a credential profile are searched with Search Issues.",
            );
        }
        return Ok(());
    }

    if !fetched.errors.is_empty() {
        match quiet {
            true => host::log(LogLevel::Warn, &fetched.errors.join("; ")),
            false => warn("Some trackers couldn't be updated", &fetched.errors),
        }
    }

    // Offline (every tracker failed): keep the earlier issues rather than emptying the list.
    if fetched.errors.len() == fetched.trackers {
        let failure = match fetched.denied == fetched.trackers {
            true => "Issues couldn't be loaded: network access is not allowed (Settings › Plugins › Tasks)",
            false => "Issues couldn't be loaded: run Update Issues to see why",
        };
        ISSUE_FAILURES.with_borrow_mut(|failures| failures.insert(root.clone(), failure));
        return Ok(());
    }

    update(&root, |tasks, _| {
        // Tasks follow their issue's summary.
        for task in &mut tasks.tasks {
            if let Some(issue) = fetched.issues.iter().find(|i| i.tracker == task.tracker && trackers::same_issue(i, task)) {
                task.summary = issue.summary.clone();
                task.description = issue.description.clone();
                task.url = issue.url.clone();
                task.api = issue.api.clone();
            }
        }

        let retained: Vec<_> = tasks
            .issues
            .iter()
            .filter(|t| {
                let tracker = t.tracker.as_deref().unwrap_or_default();
                matches!(tracker, "fogbugz" | "redmine" | "mantis" | "generic") || fetched.failed_trackers.iter().any(|f| f == tracker)
            })
            .cloned()
            .collect();
        tasks.issues = fetched.issues;
        tasks.issues.extend(retained);
        Ok(())
    })
}

fn load(root: &str) -> Result<TaskList, Error> {
    state::load(root)
}
fn update<R>(root: &str, change: impl FnOnce(&mut TaskList, u64) -> Result<R, Error>) -> Result<R, Error> {
    let result = state::update(root, change)?;
    refresh_tasks_view();
    Ok(result)
}

fn show_status(_root: &str, tasks: &TaskList) {
    let item = StatusItem {
        text: trimmed(&tasks.active().label(), 40),
        tooltip: Some("Active task. Click to switch, close or copy its commit message.".into()),
        command: Some("copy-commit-message".into()),
    };
    if host::set_window_status_item_menu(STATUS_ITEM, &item, &status_menu(tasks)).is_err() {
        let item = StatusItem { tooltip: Some("Active task. Click to copy its commit message.".into()), ..item };
        let _ = host::set_window_status_item(STATUS_ITEM, &item);
    }
}

/// Tasks the status bar menu switches to directly; the rest are behind Switch Task.
const MENU_TASKS: usize = 20;

/// The active task's actions, then the other open tasks to switch to.
fn status_menu(tasks: &TaskList) -> Vec<StatusMenuEntry> {
    let active = tasks.active();
    let mut menu = vec![StatusMenuEntry::Header(active.label()), StatusMenuEntry::action("Copy Commit Message", "copy-commit-message")];
    if active.url.is_some() {
        menu.push(StatusMenuEntry::action("Open in Browser", "open-in-browser"));
    }
    if !active.is_default() {
        menu.push(StatusMenuEntry::action("Close Task…", "close"));
    }
    let others: Vec<&Task> = tasks.tasks.iter().filter(|task| !task.closed && task.id != tasks.active).collect();
    if !others.is_empty() {
        menu.push(StatusMenuEntry::Separator);
        menu.push(StatusMenuEntry::Header("Switch to".into()));
        for task in others.iter().take(MENU_TASKS) {
            let entry = StatusMenuEntry::action(task.label(), "switch").with_argument(task.id.clone());
            menu.push(match &task.branch {
                Some(branch) => entry.with_description(branch.clone()),
                None => entry,
            });
        }
        if others.len() > MENU_TASKS {
            menu.push(StatusMenuEntry::action(format!("All {} Open Tasks…", others.len()), "switch"));
        }
    }
    menu.push(StatusMenuEntry::Separator);
    menu.push(StatusMenuEntry::action("New Task…", "new-task"));
    menu.push(StatusMenuEntry::action("Open Task…", "open-task"));
    menu
}

fn setting_or(key: &str, default: &str) -> String {
    host::setting_string(key).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| default.into())
}

fn commit_message_format() -> String {
    setting_or("commitMessageFormat", "{id} {summary}")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn unposted(tasks: &TaskList, task: &Task, now: u64) -> u64 {
    tasks.seconds_spent(task, now).saturating_sub(task.seconds_posted)
}

fn duration(seconds: u64) -> String {
    let minutes = seconds / 60;

    match minutes {
        0 => "<1m".into(),
        m if m < 60 => format!("{m}m"),
        m => format!("{}h {:02}m", m / 60, m % 60),
    }
}

impl openidea_plugin::hover::HoverProvider for Tasks {
    fn hover(
        &mut self,
        doc: &openidea_plugin::providers::Document,
        at: openidea_plugin::Position,
    ) -> Option<openidea_plugin::hover::Hover> {
        issue_context::hover(&self.issue_search, doc, at)
    }
}

register_plugin!(Tasks, views, hover, search);

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{HttpResponse, ProcessOutput};

    const ALL: &[&str] = &["project.read", "storage", "vcs.read", "secrets", "events", "clipboard", "process:git", "net:api.github.com"];

    fn plugin(host: &MockHost) -> Tasks {
        let mut plugin = Tasks::default();
        plugin.activate(&host.context("aritti.tasks")).unwrap();
        plugin
    }

    /// Records git calls; `git rev-parse --abbrev-ref HEAD` answers `main`.
    fn fake_git(host: &MockHost) -> Rc<RefCell<Vec<String>>> {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = calls.clone();
        host.on_process("git", move |args, _| {
            let command = args.join(" ");
            seen.borrow_mut().push(command.clone());
            let (status, stdout) = match command.as_str() {
                "rev-parse --abbrev-ref HEAD" => (0, "main\n"),
                c if c.starts_with("rev-parse --verify") => (1, ""),
                c if c.starts_with("rev-parse --path-format=absolute --git-path") => (0, "/project/.git/openidea-task-message\n"),
                c if c.starts_with("config --local --get") => (1, ""),
                _ => (0, ""),
            };
            Ok(ProcessOutput { status: Some(status), stdout: stdout.into(), stderr: String::new() })
        });
        calls
    }

    fn press(plugin: &mut Tasks, host: &MockHost, event: ViewEvent) {
        plugin.render(VIEW, None, &host.invocation()).unwrap();
        let event = match event {
            ViewEvent::Button(k) => ViewEvent::Button(plugin.scope.key(&k, true)),
            ViewEvent::Field(k, v) => ViewEvent::Field(plugin.scope.key(&k, true), v),
            ViewEvent::Select(k) => ViewEvent::Select(plugin.scope.key(&k, false)),
            ViewEvent::Activate(k) => ViewEvent::Activate(plugin.scope.key(&k, false)),
            ViewEvent::Expand(k) => ViewEvent::Expand(plugin.scope.key(&k, false)),
            ViewEvent::Collapse(k) => ViewEvent::Collapse(plugin.scope.key(&k, false)),
            event => event,
        };
        plugin.handle(VIEW, event, &host.invocation()).unwrap();
    }

    fn seeded() -> (MockHost, Tasks) {
        let host=MockHost::install().with_capabilities(&["project.read","storage"]);
        update("/project",|tasks,now| { let id=tasks.add_local("Private project task","");tasks.activate(&id,now)?;Ok(()) }).unwrap();
        (host,Tasks::default())
    }
    fn notified(host:&MockHost, after:usize) -> bool {
        host.refreshed_views()[after..].iter().any(|view| view == VIEW)
    }
    #[test]
    fn copy_commit_message_denied_names_permission() {
        let (host, mut plugin) = seeded();
        let error = plugin.run_command("copy-commit-message", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("needs the clipboard permission"), "{error}");
        assert!(error.contains("Settings › Plugins › Tasks"), "{error}");
    }

    #[test]
    fn branch_per_task_notice_shown_once() {
        let (host, plugin) = seeded();
        update("/project", |tasks, _| { tasks.add_local("Second task", ""); tasks.add_local("Third task", ""); Ok(()) }).unwrap();
        let project = host.invocation().project().unwrap();
        plugin.switch_task(&project, "LOCAL-00002", true, &Choices::default()).unwrap();
        plugin.switch_task(&project, "LOCAL-00003", true, &Choices::default()).unwrap();
        let notices: Vec<_> = host.notifications().into_iter().filter(|n| n.body.contains("needs the process:git permission")).collect();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("Settings › Plugins › Tasks"));
    }

    #[test]
    fn native_primary_controls_distinguish_selection_from_active_task() {
        let (host, mut plugin) = seeded();
        update("/project", |tasks, _| { tasks.add_local("Selected next task", ""); Ok(()) }).unwrap();
        press(&mut plugin, &host, ViewEvent::Select("task:LOCAL-00002".into()));
        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        assert!(view.toolbar.iter().any(|b| b.label == "Switch Task" && b.enabled));
        let labels: Vec<_> = view.toolbar.iter().map(|b| b.label.as_str()).collect();
        assert_eq!(labels, ["New Task…", "Switch Task", "Close Active Task…", "Task Actions…", "More…"]);
        press(&mut plugin, &host, ViewEvent::Button("close-active".into()));
        assert!(matches!(&plugin.form, Some(Form::Close { id }) if id == "LOCAL-00001"));
        let form = plugin.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Form(fields) = form.body else { panic!("review form") };
        assert!(fields.iter().any(|field| field.label == "Task to close" && field.value == FieldValue::label("Private project task")));
        press(&mut plugin, &host, ViewEvent::Button("cancel".into()));
        // No picker answer is supplied: a known selection opens directly.
        press(&mut plugin, &host, ViewEvent::Button("find".into()));
        assert_eq!(load("/project").unwrap().active, "LOCAL-00002");
        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        assert!(!view.toolbar.iter().any(|b| b.label == "Active Task"));
        assert!(view.status.unwrap().contains("Selected: Selected next task"));
        assert!(host.process_runs().is_empty());
        assert!(host.requests().is_empty());
    }

    #[test]
    fn native_task_picker_and_action_review_preserve_activation_and_editing() {
        let (host, mut plugin) = seeded();
        update("/project", |tasks, _| { tasks.add_local("Second task", "Read the task description"); Ok(()) }).unwrap();
        host.answer_next_prompt(Some(&["task:LOCAL-00002"]));
        press(&mut plugin, &host, ViewEvent::Button("find".into()));
        assert_eq!(load("/project").unwrap().active, "LOCAL-00002");
        host.answer_next_prompt(Some(&["edit"]));
        press(&mut plugin, &host, ViewEvent::Button("actions".into()));
        assert!(matches!(plugin.form, Some(Form::Edit { .. })));
        press(&mut plugin, &host, ViewEvent::Field("summary".into(), "Reviewed summary".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));
        assert_eq!(load("/project").unwrap().find("LOCAL-00002").unwrap().summary, "Reviewed summary");
        assert!(host.view_messages().is_empty());
        assert!(host.process_runs().is_empty());
        assert!(host.requests().is_empty());
    }

    #[test]
    fn external_close_form_refreshes_native_view_without_closing_task() {
        let (host,mut plugin)=seeded();let before=host.refreshed_views().len();
        plugin.run_command("close",&host.invocation(),None).unwrap();
        assert!(notified(&host,before));assert!(matches!(plugin.form,Some(Form::Close{..})));
        assert_eq!(load("/project").unwrap().active,"LOCAL-00001");assert!(host.requests().is_empty());
    }
    #[test]
    fn new_task_command_opens_the_new_task_form() {
        let (host,mut plugin)=seeded();
        plugin.run_command("new-task",&host.invocation(),None).unwrap();
        assert!(matches!(plugin.form,Some(Form::New{..})));
    }
    #[test]
    fn open_task_command_reveals_the_window_for_the_branch_form() {
        let host=MockHost::install().with_capabilities(&["project.read","storage","process:git"]);
        let _git=fake_git(&host);
        let mut plugin=plugin(&host);
        update("/project",|tasks,_| { tasks.issues=vec![Task::issue("github","tool-12","Crash","https://github.com/x","https://api.github.com/x")]; Ok(()) }).unwrap();
        host.answer_next_prompt(Some(&["issue:tool-12"]));
        plugin.run_command("open-task",&host.invocation(),None).unwrap();
        assert!(matches!(plugin.form,Some(Form::Open{..})));
        assert_eq!(host.revealed_views(),["tasks"]);
    }
    #[test]
    fn cancelled_open_task_picker_keeps_the_selection() {
        let (host,mut plugin)=seeded();
        plugin.run_command("refresh",&host.invocation(),None).ok();
        plugin.selected=Some("task:LOCAL-00001".into());
        host.answer_next_prompt(Some(&[]));
        plugin.run_command("open-task",&host.invocation(),None).unwrap();
        assert_eq!(plugin.selected.as_deref(),Some("task:LOCAL-00001"));
    }
    #[test]
    fn open_task_command_picks_and_activates_a_task() {
        let (host,mut plugin)=seeded();
        update("/project",|tasks,_| { tasks.add_local("Second task",""); Ok(()) }).unwrap();
        host.answer_next_prompt(Some(&["task:LOCAL-00002"]));
        plugin.run_command("open-task",&host.invocation(),None).unwrap();
        assert_eq!(load("/project").unwrap().active,"LOCAL-00002");
    }
    #[test]
    fn external_switch_and_cancelled_switch_refresh_current_action_tokens() {
        let (host,mut plugin)=seeded();plugin.render(VIEW,None,&host.invocation()).unwrap();
        let old=plugin.scope.key("new",true);let before=host.refreshed_views().len();
        plugin.run_command("switch",&host.invocation(),None).unwrap();
        assert!(notified(&host,before));assert!(plugin.handle(VIEW,ViewEvent::Button(old),&host.invocation()).is_err());
        host.answer_next_prompt(Some(&[DEFAULT_TASK_ID]));let before=host.refreshed_views().len();
        plugin.run_command("switch",&host.invocation(),None).unwrap();
        assert!(notified(&host,before));assert_eq!(load("/project").unwrap().active,DEFAULT_TASK_ID);
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
    #[test]
    fn status_menu_lists_the_active_task_actions_and_switches_directly() {
        let (host,mut plugin)=seeded();
        update("/project",|tasks,_| { tasks.add_local("Second task",""); Ok(()) }).unwrap();
        plugin.render(VIEW,None,&host.invocation()).unwrap();
        assert_eq!(host.status_menu(STATUS_ITEM), [
            StatusMenuEntry::Header("Private project task".into()),
            StatusMenuEntry::action("Copy Commit Message","copy-commit-message"),
            StatusMenuEntry::action("Close Task…","close"),
            StatusMenuEntry::Separator,
            StatusMenuEntry::Header("Switch to".into()),
            StatusMenuEntry::action("Default task","switch").with_argument(DEFAULT_TASK_ID),
            StatusMenuEntry::action("Second task","switch").with_argument("LOCAL-00002"),
            StatusMenuEntry::Separator,
            StatusMenuEntry::action("New Task…","new-task"),
            StatusMenuEntry::action("Open Task…","open-task"),
        ]);
        plugin.run_command("switch",&host.invocation(),Some("LOCAL-00002")).unwrap();
        assert_eq!(load("/project").unwrap().active,"LOCAL-00002");
        plugin.run_command("switch",&host.invocation(),Some("LOCAL-00002")).unwrap();
        assert_eq!(load("/project").unwrap().active,"LOCAL-00002","switching to the active task changes nothing");
        assert!(plugin.run_command("switch",&host.invocation(),Some("LOCAL-09999")).is_err());
    }
    #[test]
    fn status_menu_keeps_to_the_host_limit() {
        let (_host,_plugin)=seeded();
        update("/project",|tasks,_| { for n in 0..40 { tasks.add_local(&format!("Task {n}"),""); } Ok(()) }).unwrap();
        let menu=status_menu(&load("/project").unwrap());
        assert!(menu.len()<=64);
        assert_eq!(menu[menu.len()-4], StatusMenuEntry::action("All 41 Open Tasks…","switch"));
    }
    #[test]
    fn search_tab_lists_open_tasks_that_switch_when_chosen() {
        use openidea_plugin::search::{SearchProvider, SearchTarget};
        let (host,mut plugin)=seeded();
        update("/project",|tasks,_| { tasks.add_local("Fix login page",""); Ok(()) }).unwrap();
        let page=plugin.search("tasks","login",50,&host.invocation()).unwrap();
        assert_eq!(page.items.len(),1);
        assert_eq!(page.items[0].title,"Fix login page");
        assert_eq!(page.items[0].description.as_deref(),Some("Switch to this task"));
        assert_eq!(page.items[0].target,SearchTarget::Command{id:"switch".into(),arg:Some("LOCAL-00002".into())});
        let all=plugin.search("tasks","",2,&host.invocation()).unwrap();
        assert_eq!((all.items.len(),all.has_more),(2,true));
        let active=plugin.search("tasks","private",50,&host.invocation()).unwrap();
        assert_eq!(active.items[0].description.as_deref(),Some("Active"));
    }
    #[test]
    fn external_reset_and_committed_update_notify_only_tasks_view() {
        let (host,mut plugin)=seeded();let before=host.refreshed_views().len();
        host.answer_next_prompt(Some(&[]));plugin.run_command("reset-project-state",&host.invocation(),None).unwrap();
        assert!(notified(&host,before));assert_eq!(load("/project").unwrap().tasks.len(),1);
        let before=host.refreshed_views().len();update("/project",|tasks,_|{tasks.add_local("Updated","");Ok(())}).unwrap();
        assert!(notified(&host,before));assert!(host.refreshed_views()[before..].iter().all(|view|view==VIEW));
    }
    #[test]
    fn failed_update_and_stable_render_do_not_create_refresh_loop() {
        let (host,mut plugin)=seeded();let before=host.refreshed_views().len();
        plugin.render(VIEW,None,&host.invocation()).unwrap();plugin.render(VIEW,None,&host.invocation()).unwrap();
        assert_eq!(host.refreshed_views().len(),before);
        let failed:Result<(),Error>=update("/project",|_,_|Err(Error::Stale));assert!(failed.is_err());
        assert_eq!(host.refreshed_views().len(),before);
        let before = host.view_messages().len();
        let request = serde_json::json!({"channel":"ui-kit","op":"render"});
        assert!(plugin.handle(VIEW,ViewEvent::Message(request.to_string()),&host.invocation()).is_err());
        assert_eq!(host.view_messages().len(), before);
    }
    #[test]
    fn automatic_foreign_model_drops_private_form_and_old_callbacks() {
        let (host,mut plugin)=seeded();plugin.render(VIEW,None,&host.invocation()).unwrap();
        let old=plugin.scope.key("ok",true);plugin.form=Some(Form::New{summary:"Private form content".into(),description:String::new()});
        host.set_project_root(Some("/other"));let model=plugin.render(VIEW,None,&host.invocation()).unwrap();
        assert!(!format!("{:?}",model).contains("Private form content"));assert!(plugin.form.is_none());
        assert!(plugin.handle(VIEW,ViewEvent::Button(old),&host.invocation()).is_err());assert_eq!(load("/other").unwrap().tasks.len(),1);
    }
    #[test]
    fn lost_project_read_clears_form_before_render_error() {
        let (host,mut plugin)=seeded();plugin.render(VIEW,None,&host.invocation()).unwrap();
        plugin.form=Some(Form::New{summary:"Private form content".into(),description:String::new()});
        let host=host.with_capabilities(&["storage"]);assert!(plugin.render(VIEW,None,&host.invocation()).is_err());
        assert!(plugin.form.is_none());assert!(plugin.selected.is_none());assert!(host.requests().is_empty());
    }

    #[test]
    fn a_new_task_gets_its_branch_and_closing_merges_it() {
        let host = MockHost::install().with_capabilities(ALL);
        let git = fake_git(&host);
        let mut plugin = plugin(&host);

        press(&mut plugin, &host, ViewEvent::Button("new".into()));
        press(&mut plugin, &host, ViewEvent::Field("summary".into(), "Fix login page".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));

        assert_eq!(host.status_items()[STATUS_ITEM].text, "Fix login page");
        assert!(git.borrow().contains(&"switch -c Fix-login".to_owned()));

        let tasks = load("/project").unwrap();
        assert_eq!(tasks.active().branch.as_deref(), Some("Fix-login"));
        assert_eq!(tasks.active().base_branch.as_deref(), Some("main"));
        assert_eq!(tasks.find(DEFAULT_TASK_ID).unwrap().branch.as_deref(), Some("main"));

        press(&mut plugin, &host, ViewEvent::Button("close".into()));
        press(&mut plugin, &host, ViewEvent::Field("merge_branch".into(), "true".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));

        let calls = git.borrow();
        let tail: Vec<_> = calls.iter().rev().take(4).rev().cloned().collect();
        assert_eq!(tail, ["switch main", "switch main", "merge --no-edit Fix-login", "branch -d Fix-login"]);
        assert_eq!(host.status_items()[STATUS_ITEM].text, "Default task");
        assert!(load("/project").unwrap().choices.merge_branch);
        assert!(host.notifications().iter().any(|n| n.body == "Closed Fix login page"));
    }

    #[test]
    fn the_close_command_opens_the_close_form_instead_of_closing() {
        let host = MockHost::install().with_capabilities(ALL);
        let git = fake_git(&host);
        let mut plugin = plugin(&host);

        press(&mut plugin, &host, ViewEvent::Button("new".into()));
        press(&mut plugin, &host, ViewEvent::Field("summary".into(), "Fix login".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));
        let calls_before = git.borrow().len();

        plugin.run_command("close", &host.invocation(), None).unwrap();

        assert_eq!(plugin.form, Some(Form::Close { id: "LOCAL-00001".into() }));
        assert_eq!(load("/project").unwrap().active, "LOCAL-00001");
        assert_eq!(git.borrow().len(), calls_before);
        assert_eq!(host.revealed_views(), ["tasks"]);
        assert!(host.notifications().is_empty());
    }

    #[test]
    fn the_switch_command_activates_the_picked_task() {
        let host = MockHost::install().with_capabilities(ALL);
        fake_git(&host);
        let mut plugin = plugin(&host);

        press(&mut plugin, &host, ViewEvent::Button("new".into()));
        press(&mut plugin, &host, ViewEvent::Field("summary".into(), "Fix login".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));
        host.answer_next_prompt(Some(&[DEFAULT_TASK_ID]));

        plugin.run_command("switch", &host.invocation(), None).unwrap();

        assert_eq!(load("/project").unwrap().active, DEFAULT_TASK_ID);
    }

    #[test]
    fn the_switch_command_without_another_task_says_so() {
        let host = MockHost::install().with_capabilities(ALL);
        let mut plugin = plugin(&host);

        assert!(plugin.run_command("switch", &host.invocation(), None).is_err());
    }

    #[test]
    fn the_tree_always_shows_the_issues_group_and_ignores_activating_a_group() {
        let host = MockHost::install().with_capabilities(ALL);
        let mut plugin = plugin(&host);

        press(&mut plugin, &host, ViewEvent::Activate("issues".into()));

        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("not a tree") };
        let hint = items.iter().find(|i| i.parent.as_deref().is_some_and(|k| k.ends_with(":issues"))).unwrap();
        assert!(items.iter().any(|i| i.key.ends_with(":issues")));
        assert_eq!(hint.icon.as_deref(), Some("info"));
        assert!(plugin.form.is_none());

        update("/project", |tasks, _| {
            tasks.issues = vec![Task::issue("github", "tool-12", "Crash", "https://github.com/x", "https://api.github.com/x")];
            Ok(())
        })
        .unwrap();

        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("not a tree") };
        let issue = items.iter().find(|i| i.key.ends_with(":issue:tool-12")).unwrap();
        assert_eq!(issue.detail.as_deref(), Some("GitHub"));
        assert!(!items.iter().any(|i| i.key.ends_with(":no-issues")));
    }

    #[test]
    fn the_empty_issues_row_says_why_there_are_no_issues() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "secrets"]);
        let mut plugin = plugin(&host);
        let hint = |plugin: &mut Tasks| {
            let Body::Tree(items) = plugin.render(VIEW, None, &host.invocation()).unwrap().body else { panic!("not a tree") };
            items.into_iter().find(|i| i.key.ends_with(":no-issues")).unwrap().label
        };
        assert!(hint(&mut plugin).starts_with("No issue tracker set up"));

        host.set_secret("youtrack-token", "token");
        host.set_setting("youtrackHost", "\"tenant.youtrack.cloud\"");
        refresh_issues(&host.invocation().project().unwrap(), true).unwrap();
        assert!(hint(&mut plugin).contains("network access is not allowed"));
    }

    #[test]
    fn works_without_git_and_reopens_the_files_of_a_task() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events"]);
        let mut plugin = plugin(&host);

        press(&mut plugin, &host, ViewEvent::Button("new".into()));
        press(&mut plugin, &host, ViewEvent::Field("summary".into(), "Fix login".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));
        plugin.handle_events(&[Event::FileOpened("/project/src/login.rs".into())], &host.invocation());

        plugin.selected = Some(format!("task:{DEFAULT_TASK_ID}"));
        press(&mut plugin, &host, ViewEvent::Button("switch".into()));
        plugin.selected = Some("task:LOCAL-00001".into());
        press(&mut plugin, &host, ViewEvent::Button("switch".into()));

        assert_eq!(host.opened_locations(), [("/project/src/login.rs".to_owned(), 0, 0)]);
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn updates_issues_from_github_using_the_origin_remote() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_secret("github-token", "t0ken");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        host.on_fetch(|_| {
            let body = r#"{"items":[{"number":12,"title":"Crash","html_url":"https://github.com/acme/tool/issues/12","url":"https://api.github.com/repos/acme/tool/issues/12"}]}"#;
            Ok(HttpResponse { status: 200, headers: vec![], body: body.into() })
        });

        plugin(&host).run_command("refresh", &host.invocation(), None).unwrap();

        let request = &host.requests()[0];
        assert!(request.url.contains("q=repo%3Aacme%2Ftool%20is%3Aissue%20is%3Aopen"));
        assert!(request.headers.contains(&("Authorization".into(), "Bearer t0ken".into())));

        let tasks = load("/project").unwrap();
        assert_eq!(tasks.issues[0].id, "tool-12");
        assert_eq!(tasks.issues[0].api.as_deref(), Some("https://api.github.com/repos/acme/tool/issues/12"));
    }

    #[test]
    fn partial_refresh_keeps_failed_tracker_issues() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "vcs.read", "secrets", "net:api.github.com", "net:api.trello.com"]);
        host.set_secret("github-token", "t0ken");
        host.set_secret("trello-key", "k");
        host.set_secret("trello-token", "t");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        host.on_fetch(|request| {
            if request.url.contains("trello") {
                return Err(Error::Invalid("offline".into()));
            }
            let body = r#"{"items":[{"number":7,"title":"Crash","html_url":"https://github.com/acme/tool/issues/7","url":"https://api.github.com/repos/acme/tool/issues/7","repository_url":"https://api.github.com/repos/acme/tool"}]}"#;
            Ok(HttpResponse { status: 200, headers: vec![], body: body.into() })
        });
        update("/project", |tasks, _| {
            tasks.issues = vec![
                Task::issue("trello", "5f3a", "Cached card", "https://trello.com/c/abc", "https://api.trello.com/1/cards/5f3a"),
                Task::issue("github", "old-1", "Gone", "https://github.com/acme/tool/issues/1", "https://api.github.com/repos/acme/tool/issues/1"),
            ];
            Ok(())
        })
        .unwrap();
        let project = host::project_of("/project").unwrap();

        refresh_issues(&project, true).unwrap();

        let ids: Vec<_> = load("/project").unwrap().issues.into_iter().map(|t| t.id).collect();
        assert_eq!(ids, ["tool-7", "5f3a"]);
    }

    #[test]
    fn opening_an_issue_with_the_commit_template_on_writes_its_message() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_setting("fillCommitTemplate", "true");
        let git = fake_git(&host);
        let mut plugin = plugin(&host);
        update("/project", |tasks, _| {
            tasks.issues = vec![Task::issue("github", "tool-12", "100% crash", "https://github.com/x", "https://api.github.com/x")];
            Ok(())
        })
        .unwrap();

        press(&mut plugin, &host, ViewEvent::Activate("issue:tool-12".into()));
        press(&mut plugin, &host, ViewEvent::Field("create_branch".into(), "false".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));

        assert_eq!(load("/project").unwrap().active, "tool-12");
        assert!(git.borrow().iter().any(|c| c.starts_with("config --local commit.template ")));
        let template = git.borrow().iter().find(|c| c.starts_with("config --local commit.template ")).cloned().unwrap();
        assert_eq!(template, "config --local commit.template /project/.git/openidea-task-message");
        assert!(git.borrow().iter().any(|c| c == "log -1 --format=tool-12 100%% crash --output=/project/.git/openidea-task-message"));
        assert!(!git.borrow().iter().any(|c| c.starts_with("switch -c")));
    }

    #[test]
    fn commit_template_set_in_empty_repo() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "process:git"]);
        host.set_setting("fillCommitTemplate", "true");
        let calls = Rc::new(RefCell::new(Vec::new()));
        let seen = calls.clone();
        host.on_process("git", move |args, _| {
            let command = args.join(" ");
            seen.borrow_mut().push(command.clone());
            let (status, stdout) = match command.as_str() {
                "rev-parse --abbrev-ref HEAD" => (0, "main\n"),
                c if c.starts_with("log ") => (128, ""),
                c if c.starts_with("rev-parse --path-format=absolute --git-path") => (0, "/project/.git/openidea-task-message\n"),
                c if c.starts_with("config --local --get") => (1, ""),
                _ => (0, ""),
            };
            Ok(ProcessOutput { status: Some(status), stdout: stdout.into(), stderr: String::new() })
        });
        let mut plugin = plugin(&host);
        update("/project", |tasks, _| {
            tasks.issues = vec![Task::issue("github", "tool-12", "Crash", "https://github.com/x", "https://api.github.com/x")];
            Ok(())
        })
        .unwrap();

        press(&mut plugin, &host, ViewEvent::Activate("issue:tool-12".into()));
        press(&mut plugin, &host, ViewEvent::Field("create_branch".into(), "false".into()));
        press(&mut plugin, &host, ViewEvent::Button("ok".into()));

        assert!(host.notifications().is_empty(), "{:?}", host.notifications());
        let configure = calls.borrow().iter().find(|c| c.starts_with("config --local commit.template ")).cloned().unwrap();
        let path = configure.trim_start_matches("config --local commit.template ");
        assert!(!path.contains(".git"), "{path}");
        assert!(path.starts_with(&host::fs::plugin_dir().unwrap()), "{path}");
        assert!(host::fs::read_file(path).unwrap().starts_with("tool-12 Crash"));
    }

    fn github_hit(host: &MockHost) {
        host.on_fetch(|_| {
            let body = r#"{"items":[{"number":7,"title":"Crash","html_url":"https://github.com/acme/tool/issues/7","url":"https://api.github.com/repos/acme/tool/issues/7","repository_url":"https://api.github.com/repos/acme/tool"}]}"#;
            Ok(HttpResponse { status: 200, headers: vec![], body: body.into() })
        });
    }

    #[test]
    fn searching_issues_activates_the_picked_hit() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_secret("github-token", "t0ken");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        github_hit(&host);
        let mut plugin = plugin(&host);

        host.answer_next_prompt(Some(&["crash"]));
        host.answer_next_prompt(Some(&["0"]));
        plugin.run_command("search-issues", &host.invocation(), None).unwrap();

        assert!(host.requests()[0].url.contains("is%3Aopen%20crash"));
        assert_eq!(plugin.form, Some(Form::Open { id: "tool-7".into() }));
        assert_eq!(host.revealed_views(), ["tasks"]);
    }

    #[test]
    fn more_button_runs_search_issues_from_its_picker() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_secret("github-token", "t0ken");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        github_hit(&host);
        let mut plugin = plugin(&host);
        host.answer_next_prompt(Some(&["search-issues"]));
        host.answer_next_prompt(Some(&["crash"]));
        host.answer_next_prompt(Some(&["0"]));
        press(&mut plugin, &host, ViewEvent::Button("more".into()));

        assert!(host.requests()[0].url.contains("is%3Aopen%20crash"));
    }

    #[test]
    fn searching_issues_without_git_switches_straight_to_the_hit() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "vcs.read", "secrets", "net:api.github.com"]);
        host.set_secret("github-token", "t0ken");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        github_hit(&host);
        let mut plugin = plugin(&host);

        host.answer_next_prompt(Some(&["crash"]));
        host.answer_next_prompt(Some(&["0"]));
        plugin.run_command("search-issues", &host.invocation(), None).unwrap();

        assert_eq!(load("/project").unwrap().active, "tool-7");
    }

    #[test]
    fn search_reuses_only_the_matching_server_task_and_refuses_id_collisions() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_secret("github-token", "token");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        github_hit(&host);
        update("/project", |tasks, _| {
            tasks.tasks.push(Task::issue(
                "github",
                "tool-7",
                "Owned",
                "https://github.com/acme/tool/issues/7",
                "https://api.github.com/repos/acme/tool/issues/7",
            ));
            Ok(())
        })
        .unwrap();
        let mut plugin = plugin(&host);
        plugin.git = false;
        host.answer_next_prompt(Some(&["crash"]));
        host.answer_next_prompt(Some(&["0"]));
        plugin.run_command("search-issues", &host.invocation(), None).unwrap();
        assert_eq!(load("/project").unwrap().active, "tool-7");
        assert_eq!(load("/project").unwrap().find("tool-7").unwrap().summary, "Owned");
        update("/project", |tasks, _| {
            tasks.active = DEFAULT_TASK_ID.into();
            tasks.tasks.iter_mut().find(|task| task.id == "tool-7").unwrap().api =
                Some("https://api.github.com/repos/other/tool/issues/7".into());
            Ok(())
        })
        .unwrap();
        host.answer_next_prompt(Some(&["crash"]));
        host.answer_next_prompt(Some(&["0"]));
        assert!(plugin.run_command("search-issues", &host.invocation(), None).is_err());
        assert_eq!(load("/project").unwrap().active, DEFAULT_TASK_ID);
    }

    #[test]
    fn dismissing_the_issue_search_does_nothing() {
        let host = MockHost::install().with_capabilities(ALL);
        let mut plugin = plugin(&host);

        plugin.run_command("search-issues", &host.invocation(), None).unwrap();

        host.answer_next_prompt(Some(&["crash"]));
        host.set_secret("github-token", "t0ken");
        host.set_vcs_remotes(&[("origin", "git@github.com:acme/tool.git")]);
        github_hit(&host);
        plugin.run_command("search-issues", &host.invocation(), None).unwrap();

        assert_eq!(load("/project").unwrap().active, DEFAULT_TASK_ID);
        assert!(host.requests().len() == 1);
    }

    #[test]
    fn native_context_uses_current_tabs_and_restores_active_selection() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events", "editor.read"]);
        let mut plugin = plugin(&host);
        plugin.handle_events(&[Event::FileOpened("/project/closed.rs".into())], &host.invocation());
        host.open_editor("/project/current.rs", "rust", "abcdef", &[(0, 2, 0, 4)]);
        host.add_file("current.rs", "abcdef");
        host.answer_next_prompt(Some(&["Current"]));
        plugin.run_command("save-context", &host.invocation(), None).unwrap();
        let saved = &load("/project").unwrap().contexts[0];
        assert_eq!(saved.files, ["/project/current.rs"]);
        assert_eq!(saved.selection.as_ref().unwrap().1.start.character, 2);
        host.answer_next_prompt(Some(&["Current"]));
        plugin.run_command("load-context", &host.invocation(), None).unwrap();
        assert_eq!(host.invocation().editor().unwrap().selections().unwrap()[0].end.character, 4);
    }

    #[test]
    fn named_context_capture_excludes_nested_project_and_denial_preserves_storage() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "editor.read"]);
        let mut plugin = plugin(&host);
        host.add_project("/project/nested");
        host.open_editor("/project/nested/private.rs", "rust", "secret", &[]);
        assert!(plugin.run_command("save-context", &host.invocation(), None).is_err());
        assert!(state::raw("/project").unwrap().is_none());
        let host = host.with_capabilities(&["project.read", "storage"]);
        assert!(plugin.run_command("save-context", &host.invocation(), None).is_err());
        assert!(state::raw("/project").unwrap().is_none());
    }

    #[test]
    fn active_task_status_is_available_with_multiple_projects() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage"]);
        host.add_project("/other");
        show_status("/project", &TaskList::new(0));
        assert_eq!(host.status_items()[STATUS_ITEM].text, "Default task");
    }

    #[test]
    fn contexts_are_saved_loaded_and_removed_by_name() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events", "editor.read"]);
        let mut plugin = plugin(&host);
        plugin.handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());

        host.open_editor("/project/a.rs", "rust", "abc", &[]);
        host.answer_next_prompt(Some(&["login"]));
        plugin.run_command("save-context", &host.invocation(), None).unwrap();
        assert_eq!(load("/project").unwrap().contexts[0].files, ["/project/a.rs"]);

        host.answer_next_prompt(Some(&["login"]));
        plugin.run_command("load-context", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_locations(), [("/project/a.rs".to_owned(), 0, 0)]);

        host.answer_next_prompt(Some(&["login"]));
        host.answer_next_prompt(None);
        plugin.run_command("remove-context", &host.invocation(), None).unwrap();
        assert_eq!(load("/project").unwrap().contexts.len(), 1);

        host.answer_next_prompt(Some(&["login"]));
        host.answer_next_prompt(Some(&["ok"]));
        plugin.run_command("remove-context", &host.invocation(), None).unwrap();
        assert!(load("/project").unwrap().contexts.is_empty());
    }

    #[test]
    fn dismissing_save_context_stores_nothing() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events", "editor.read"]);
        let mut plugin = plugin(&host);
        plugin.handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());

        host.open_editor("/project/a.rs", "rust", "abc", &[]);
        plugin.run_command("save-context", &host.invocation(), None).unwrap();

        assert!(load("/project").unwrap().contexts.is_empty());
    }

    #[test]
    fn added_repositories_are_all_searched_and_can_be_removed() {
        let host = MockHost::install().with_capabilities(ALL);
        host.set_secret("github-token", "t0ken");
        github_hit(&host);
        let mut plugin = plugin(&host);

        for repo in ["acme/tool", "acme/other"] {
            host.answer_next_prompt(Some(&["github"]));
            host.answer_next_prompt(Some(&[repo]));
            plugin.run_command("add-repository", &host.invocation(), None).unwrap();
        }

        assert!(host.requests().last().unwrap().url.contains("repo%3Aacme%2Ftool%20repo%3Aacme%2Fother"));

        host.answer_next_prompt(Some(&["github acme/tool"]));
        plugin.run_command("remove-repository", &host.invocation(), None).unwrap();
        assert_eq!(load("/project").unwrap().repositories["github"], ["acme/other"]);

        host.answer_next_prompt(None);
        plugin.run_command("add-repository", &host.invocation(), None).unwrap();
        assert_eq!(load("/project").unwrap().repositories["github"].len(), 1);
    }
}
