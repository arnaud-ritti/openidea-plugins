//! Tasks: a port of the Issue Trackers plugin (com.intellij.tasks) of
//! intellij-community. One task is active at a time; local tasks are created
//! in the view, issues come from the trackers of `trackers.rs`, and with git
//! each task gets its branch (`git.rs`). The task list of each project is kept
//! in storage, as TaskManagerImpl keeps it in the workspace.

mod git;
mod model;
mod trackers;

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Error, Event, Invocation, LogLevel, NotificationLevel, PickItem, Plugin, Project, StatusItem, host, register_plugin,
};

use git::Git;
use model::{Choices, DEFAULT_TASK_ID, NamedContext, Task, TaskList, trimmed};

const VIEW: &str = "tasks";
const STATUS_ITEM: &str = "active-task";

/// Shown in place of the task tree: the IDE has no dialogs.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Form {
    New { summary: String, description: String },
    Edit { id: String, summary: String, description: String },
    /// OpenTaskDialog, for an issue that isn't a task yet.
    Open { id: String },
    Close { id: String },
}

#[derive(Default)]
struct Tasks {
    git: bool,
    selected: Option<String>,
    form: Option<Form>,
    /// The checkboxes of the open form.
    choices: Choices,
    collapsed: BTreeSet<String>,
}

impl Plugin for Tasks {
    fn activate(&mut self, ctx: &Context) -> Result<(), Error> {
        self.git = ctx.has("process:git");

        if let Some(project) = host::projects().ok().and_then(|p| p.into_iter().next()) {
            if let Ok(tasks) = project.root().and_then(|root| load(&root)) {
                show_status(&tasks);
            }
        }

        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let tasks = load(&project.root()?)?;

        match id {
            // Closing can merge and delete the branch and close the issue: the user reviews the choices first.
            "close" => {
                if tasks.active().is_default() {
                    return Err("The default task can't be closed.".into());
                }

                self.show_form(Form::Close { id: tasks.active.clone() }, &tasks);
                host::refresh_view(VIEW);
                if host::reveal_view(VIEW).is_err() {
                    host::notify(NotificationLevel::Info, "Close the active task", "Review the close options in the Tasks tool window.");
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
                    host::refresh_view(VIEW);
                }
            }
            "open-in-browser" => {
                let url = tasks.active().url.as_deref().ok_or("The active task isn't an issue, so it has no page to open.")?;
                host::open_url(url)?;
            }
            "copy-commit-message" => {
                let message = tasks.commit_message(&commit_message_format())?;
                host::copy_to_clipboard(&message)?;
                host::notify(NotificationLevel::Info, "Commit message copied", &message);
            }
            "refresh" => refresh_issues(&project, false)?,
            "search-issues" => self.search_issues(&project, &tasks, ctx)?,
            "save-context" => save_context(&project, &tasks)?,
            "load-context" => load_context(&tasks)?,
            "remove-context" => remove_context(&project, &tasks)?,
            "add-repository" => add_repository(&project)?,
            "remove-repository" => remove_repository(&project, &tasks)?,
            _ => return Err(Error::NotFound(format!("command {id}"))),
        }

        Ok(None)
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        let mut opened = Vec::new();

        for event in events {
            match event {
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

        let recorded = update(&root, |tasks, _| {
            opened.iter().filter(|path| path.starts_with(&root)).for_each(|path| tasks.record_file(path));
            Ok(())
        });

        if let Err(e) = recorded {
            host::log(LogLevel::Warn, &format!("recording opened files: {e}"));
        }
    }
}

impl Views for Tasks {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to work on tasks."));
        };

        let tasks = load(&project.root()?)?;

        if let Some(form) = &self.form {
            return Ok(self.render_form(form, &tasks));
        }

        let now = now();
        let selected = self.selected.as_deref().and_then(|key| selected_task(&tasks, key));

        let mut items = vec![self.group("open", "Tasks")];

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
            items.push(self.group("closed", "Closed"));

            for task in tasks.tasks.iter().filter(|t| t.closed) {
                let detail = duration(task.seconds_spent);
                let item = TreeItem::new(&format!("task:{}", task.id), &task.label()).parent("closed");
                items.push(item.icon("github-issue-closed").detail(&detail));
            }
        }

        items.push(self.group("issues", "Issues"));

        for issue in tasks.pending_issues() {
            let item = TreeItem::new(&format!("issue:{}", issue.id), &issue.label()).parent("issues");
            items.push(item.icon("github-issue-open").detail(trackers::name(issue)));
        }

        if tasks.pending_issues().next().is_none() {
            let hint = "No issues (trackers are set up in Settings › Plugins › Tasks)";
            items.push(TreeItem::new("no-issues", hint).parent("issues").icon("info"));
        }

        let is_task = |t: &&Task| tasks.find(&t.id).is_some();
        let can_switch = selected.is_some_and(|t| t.id != tasks.active);
        let can_close = selected.filter(is_task).is_some_and(|t| !t.is_default() && !t.closed);
        let can_edit = selected.is_some_and(|t| !t.is_default() && !t.is_issue());
        let can_post = selected.filter(is_task).is_some_and(trackers::can_post_time);
        let can_browse = selected.is_some_and(Task::is_issue);
        let post_label = match selected.filter(|_| can_post) {
            Some(task) => format!("Post Work Time ({})", duration(unposted(&tasks, task, now))),
            None => "Post Work Time".to_owned(),
        };

        Ok(View::new(Body::Tree(items))
            .button(Button::new("new", "New Task").icon("plus"))
            .button(Button::new("switch", "Switch to Task").icon("run").enabled(can_switch))
            .button(Button::new("edit", "Edit Task").icon("edit").enabled(can_edit))
            .button(Button::new("close", "Close Task").icon("check").enabled(can_close))
            .button(Button::new("post-time", &post_label).icon("clock").enabled(can_post))
            .button(Button::new("browse", "Open in Browser").icon("web").enabled(can_browse))
            .button(Button::new("refresh", "Update Issues").icon("refresh"))
            .status(&format!("Active: {}", tasks.active().label())))
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
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

        host::refresh_view(VIEW);

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
                "Create",
                !summary.trim().is_empty(),
                "New task".to_owned(),
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
                (Vec::new(), "Open Task", true, label)
            }
            Form::Close { id } => {
                let mut fields = Vec::new();
                let task = tasks.find(id);

                if task.is_some_and(trackers::can_update_state) {
                    fields.push(checkbox("update_state", "Update issue state in the tracker", self.choices.update_state));
                }

                if let Some((branch, base)) = task.and_then(|t| Some((t.branch.as_ref()?, t.base_branch.as_ref()?))).filter(|_| self.git) {
                    let label = format!("Merge branch {branch} into {base}");
                    fields.push(checkbox("merge_branch", &label, self.choices.merge_branch));
                }

                (fields, "Close Task", true, task.map(Task::label).unwrap_or_default())
            }
        };

        if matches!(form, Form::New { .. } | Form::Open { .. }) {
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
                let form = Form::Edit { id: task.id.clone(), summary: task.summary.clone(), description: task.description.clone() };
                self.show_form(form, &tasks);
            }
            "close" => {
                let id = self.selected_task(&tasks)?.id.clone();
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
            "refresh" => refresh_issues(&project, false)?,
            "ok" => self.submit(&project)?,
            "cancel" => self.form = None,
            _ => {}
        }

        Ok(())
    }

    fn show_form(&mut self, form: Form, tasks: &TaskList) {
        self.choices = tasks.choices.clone();
        self.form = Some(form);
    }

    fn submit(&mut self, project: &Project) -> Result<(), Error> {
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
            Form::Close { id } => self.close_task(project, &id, &choices)?,
        }

        Ok(())
    }

    fn selected_task<'a>(&self, tasks: &'a TaskList) -> Result<&'a Task, Error> {
        self.selected.as_deref().and_then(|key| selected_task(tasks, key)).ok_or_else(|| "Select a task first.".into())
    }

    fn search_issues(&mut self, project: &Project, tasks: &TaskList, ctx: &Invocation) -> Result<(), Error> {
        let Some(query) = host::input("Search issues", None)?.map(|q| q.trim().to_owned()).filter(|q| !q.is_empty()) else {
            return Ok(());
        };

        let _progress = host::begin_task("Searching issues", false);
        let remotes = project.vcs_remotes().unwrap_or_default();
        let scope = trackers::Scope { remotes: &remotes, query: Some(&query), repositories: &tasks.repositories };
        let found = trackers::fetch_all(&scope);

        warn("Some trackers couldn't be searched", &found.errors);

        if found.trackers == 0 {
            return Err("No issue tracker set up. Enter a token in Settings › Plugins › Tasks.".into());
        }

        if found.issues.is_empty() {
            return Err(format!("No issue matches \"{query}\".").into());
        }

        let items: Vec<PickItem> = found
            .issues
            .iter()
            .enumerate()
            .map(|(i, t)| PickItem::new(&i.to_string(), &t.label()).description(trackers::name(t)))
            .collect();

        let Some(index) = host::pick("Go to Issue", &items)?.and_then(|key| key.parse::<usize>().ok()) else {
            return Ok(());
        };

        let issue = found.issues[index].clone();
        let id = issue.id.clone();

        update(&project.root()?, |tasks, _| {
            if tasks.find_any(&id).is_none() {
                tasks.issues.push(issue);
            }
            Ok(())
        })?;

        self.selected = Some(format!("issue:{id}"));

        self.start_selected(ctx)?;

        if self.form.is_some() {
            host::refresh_view(VIEW);
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

    /// TaskManagerImpl.activateTask: shelve the changes of the task being
    /// left, check out the task's branch (or start one for a new task),
    /// restore its changes, its commit message and the files it had open.
    fn switch_task(&self, project: &Project, id: &str, new_task: bool, choices: &Choices) -> Result<(), Error> {
        let root = project.root()?;
        let _progress = host::begin_task("Switching task", false);
        let git = self.git.then(|| Git::new(project));
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
                        // TaskManagerImpl.createBranch: the task being left keeps the branch it was on.
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
            note(&mut warnings, trackers::close_issue(&task));
        }

        if tasks.active == id {
            self.switch_task(project, DEFAULT_TASK_ID, false, choices)?;
        }

        if let (true, true, Some(branch), Some(base)) = (choices.merge_branch, self.git, &task.branch, &task.base_branch) {
            note(&mut warnings, Git::new(project).merge_and_delete(branch, base));
        }

        update(&root, |tasks, _| tasks.close(id))?;
        warn(&format!("Closed {}, but some steps failed", task.id), &warnings);

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

/// TaskManagerImpl.updateIssues, every 20 minutes (`on-schedule:1200`).
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

fn save_context(project: &Project, tasks: &TaskList) -> Result<(), Error> {
    let files = tasks.active().files.clone();

    if files.is_empty() {
        return Err("The active task has no recorded files to save.".into());
    }

    let Some(name) = host::input("Save context as", None)?.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()) else {
        return Ok(());
    };

    update(&project.root()?, |tasks, _| {
        tasks.contexts.retain(|c| c.name != name);
        tasks.contexts.push(NamedContext { name: name.clone(), files });
        Ok(())
    })?;
    host::notify(NotificationLevel::Info, "Context saved", &name);

    Ok(())
}

fn pick_context(tasks: &TaskList, title: &str) -> Result<Option<NamedContext>, Error> {
    if tasks.contexts.is_empty() {
        return Err("There is no saved context.".into());
    }

    let items: Vec<PickItem> = tasks
        .contexts
        .iter()
        .map(|c| PickItem::new(&c.name, &c.name).description(&format!("{} files", c.files.len())))
        .collect();

    Ok(host::pick(title, &items)?.and_then(|name| tasks.contexts.iter().find(|c| c.name == name).cloned()))
}

fn load_context(tasks: &TaskList) -> Result<(), Error> {
    let Some(context) = pick_context(tasks, "Load Context")? else {
        return Ok(());
    };

    for file in &context.files {
        let _ = host::open_location(file, 0, 0);
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

    update(&project.root()?, |tasks, _| {
        tasks.contexts.retain(|c| c.name != context.name);
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

    if !repository.contains('/') {
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
        .flat_map(|(kind, list)| list.iter().map(move |r| PickItem::new(&format!("{kind} {r}"), r).description(trackers::tracker_name(kind))))
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

/// `quiet`: problems go to the log instead of a notification.
fn refresh_issues(project: &Project, quiet: bool) -> Result<(), Error> {
    let root = project.root()?;
    let _progress = host::begin_task("Updating issues", false);
    let remotes = project.vcs_remotes().unwrap_or_default();
    let repositories = load(&root)?.repositories;
    let fetched = trackers::fetch_all(&trackers::Scope { remotes: &remotes, query: None, repositories: &repositories });

    if fetched.trackers == 0 {
        if !quiet {
            host::notify(
                NotificationLevel::Info,
                "No issue tracker set up",
                "Enter a GitHub, GitLab, Codeberg, YouTrack, Jira or Trello token in Settings › Plugins › Tasks.",
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
        return Ok(());
    }

    update(&root, |tasks, _| {
        // TaskManagerImpl.updateIssue: tasks follow their issue's summary.
        for task in &mut tasks.tasks {
            if let Some(issue) = fetched.issues.iter().find(|i| i.id == task.id && i.tracker == task.tracker) {
                task.summary = issue.summary.clone();
                task.url = issue.url.clone();
                task.api = issue.api.clone();
            }
        }

        tasks.issues = fetched.issues;
        Ok(())
    })
}

fn storage_key(root: &str) -> String {
    format!("tasks:{root}")
}

fn load(root: &str) -> Result<TaskList, Error> {
    let stored = host::storage::get(&storage_key(root))?;

    Ok(stored.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_else(|| TaskList::new(now())))
}

fn update<R>(root: &str, change: impl FnOnce(&mut TaskList, u64) -> Result<R, Error>) -> Result<R, Error> {
    let mut tasks = load(root)?;
    let result = change(&mut tasks, now())?;
    let json = serde_json::to_string(&tasks).map_err(|e| e.to_string())?;

    host::storage::set(&storage_key(root), &json)?;
    show_status(&tasks);
    host::refresh_view(VIEW);

    Ok(result)
}

fn show_status(tasks: &TaskList) {
    let item = StatusItem {
        text: trimmed(&tasks.active().label(), 40),
        tooltip: Some("Active task. Click to copy its commit message.".into()),
        command: Some("copy-commit-message".into()),
    };

    host::set_status_item(STATUS_ITEM, &item);
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

register_plugin!(Tasks, views);

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
                _ => (0, ""),
            };
            Ok(ProcessOutput { status: Some(status), stdout: stdout.into(), stderr: String::new() })
        });
        calls
    }

    fn press(plugin: &mut Tasks, host: &MockHost, event: ViewEvent) {
        plugin.handle(VIEW, event, &host.invocation()).unwrap();
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
        let hint = items.iter().find(|i| i.parent.as_deref() == Some("issues")).unwrap();
        assert!(items.iter().any(|i| i.key == "issues"));
        assert_eq!(hint.icon.as_deref(), Some("info"));
        assert!(plugin.form.is_none());

        update("/project", |tasks, _| {
            tasks.issues = vec![Task::issue("github", "tool-12", "Crash", "https://github.com/x", "https://api.github.com/x")];
            Ok(())
        })
        .unwrap();

        let view = plugin.render(VIEW, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("not a tree") };
        let issue = items.iter().find(|i| i.key == "issue:tool-12").unwrap();
        assert_eq!(issue.detail.as_deref(), Some("GitHub"));
        assert!(!items.iter().any(|i| i.key == "no-issues"));
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
        assert!(git.borrow().iter().any(|c| c.starts_with("log -1 --format=tool-12 100%% crash --output=")));
        assert!(!git.borrow().iter().any(|c| c.starts_with("switch -c")));
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
    fn contexts_are_saved_loaded_and_removed_by_name() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events"]);
        let mut plugin = plugin(&host);
        plugin.handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());

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
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "events"]);
        let mut plugin = plugin(&host);
        plugin.handle_events(&[Event::FileOpened("/project/a.rs".into())], &host.invocation());

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
