//! The task list of a project, after TaskManagerImpl and LocalTaskImpl.

use openidea_plugin::Error;
use serde::{Deserialize, Serialize};

pub const DEFAULT_TASK_ID: &str = "Default";

const CLOSED_TASKS_KEPT: usize = 50;
const CONTEXT_FILES_KEPT: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Web page of a tracker's issue; local tasks have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The tracker (`github`, `jira`…) and its API address of the issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub seconds_spent: u64,
    /// Time already posted to the tracker as work items.
    #[serde(default)]
    pub seconds_posted: u64,
    /// The task's branch and the one it was started from, merged into on close.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// Files opened while the task was active, most recent last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

impl Task {
    pub fn issue(tracker: &str, id: &str, summary: &str, url: &str, api: &str) -> Self {
        Self {
            id: id.into(),
            summary: summary.into(),
            url: Some(url.into()),
            tracker: Some(tracker.into()),
            api: Some(api.into()),
            ..Self::default()
        }
    }

    pub fn is_issue(&self) -> bool {
        self.url.is_some()
    }

    pub fn is_default(&self) -> bool {
        self.id == DEFAULT_TASK_ID
    }

    /// TaskUtil.getTrimmedSummary.
    pub fn label(&self) -> String {
        let text = match self.is_issue() {
            true => format!("{}: {}", self.id, self.summary),
            false => self.summary.clone(),
        };

        trimmed(&text, 60)
    }

    /// TaskUtil.formatTask: `{id}` and `{summary}`; local tasks have no id to show.
    pub fn format(&self, format: &str) -> String {
        let id = if self.is_issue() { self.id.as_str() } else { "" };

        format.replace("{id}", id).replace("{summary}", &self.summary).trim().to_owned()
    }
}

/// What the Open Task and Close Task forms last asked for (TaskManagerImpl.Config).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choices {
    pub create_branch: bool,
    pub shelve_changes: bool,
    pub merge_branch: bool,
    pub update_state: bool,
}

impl Default for Choices {
    fn default() -> Self {
        Self { create_branch: true, shelve_changes: false, merge_branch: true, update_state: true }
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskList {
    /// Most recently activated first.
    pub tasks: Vec<Task>,
    pub active: String,
    pub active_since: u64,
    pub local_tasks_counter: u32,
    #[serde(default)]
    pub issues: Vec<Task>,
    #[serde(default)]
    pub choices: Choices,
}

impl TaskList {
    pub fn new(now: u64) -> Self {
        let default_task = Task { id: DEFAULT_TASK_ID.into(), summary: "Default task".into(), ..Task::default() };

        Self {
            tasks: vec![default_task],
            active: DEFAULT_TASK_ID.into(),
            active_since: now,
            local_tasks_counter: 1,
            issues: Vec::new(),
            choices: Choices::default(),
        }
    }

    pub fn find(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn active(&self) -> &Task {
        self.find(&self.active).unwrap_or(&self.tasks[0])
    }

    pub fn active_mut(&mut self) -> &mut Task {
        let index = self.tasks.iter().position(|t| t.id == self.active).unwrap_or(0);

        &mut self.tasks[index]
    }

    /// A task or a not yet started issue.
    pub fn find_any(&self, id: &str) -> Option<&Task> {
        self.find(id).or_else(|| self.issues.iter().find(|t| t.id == id))
    }

    pub fn add_local(&mut self, summary: &str, description: &str) -> String {
        let id = format!("LOCAL-{:05}", self.local_tasks_counter);
        self.local_tasks_counter += 1;

        self.tasks.push(Task { id: id.clone(), summary: summary.into(), description: description.into(), ..Task::default() });

        id
    }

    /// An issue becomes a task the first time it is activated.
    pub fn activate(&mut self, id: &str, now: u64) -> Result<(), Error> {
        if id == self.active {
            return Ok(());
        }

        let index = match self.tasks.iter().position(|t| t.id == id) {
            Some(index) => index,
            None => {
                let issue = self.issues.iter().find(|t| t.id == id).ok_or_else(|| format!("Task {id} not found."))?.clone();
                self.tasks.push(issue);
                self.tasks.len() - 1
            }
        };

        self.stop_clock(now);

        let mut task = self.tasks.remove(index);
        task.closed = false;
        self.active = task.id.clone();
        self.tasks.insert(0, task);

        Ok(())
    }

    // ponytail: counts wall-clock time while the task is active, idle time included; activity-based tracking would need `events.input`.
    fn stop_clock(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.active_since);
        self.active_mut().seconds_spent += elapsed;
        self.active_since = now;
    }

    /// Marks the task closed; the caller switches away from it first.
    pub fn close(&mut self, id: &str) -> Result<(), Error> {
        if id == DEFAULT_TASK_ID {
            return Err("The default task can't be closed.".into());
        }

        if id == self.active {
            return Err("Switch to another task before closing this one.".into());
        }

        let index = self.tasks.iter().position(|t| t.id == id).ok_or_else(|| format!("Task {id} not found."))?;
        let mut task = self.tasks.remove(index);
        task.closed = true;
        self.tasks.insert(0, task);

        let mut closed_seen = 0;
        self.tasks.retain(|t| {
            closed_seen += usize::from(t.closed);
            !t.closed || closed_seen <= CLOSED_TASKS_KEPT
        });

        Ok(())
    }

    pub fn edit(&mut self, id: &str, summary: &str, description: &str) -> Result<(), Error> {
        let task = self.find_mut(id).ok_or_else(|| format!("Task {id} not found."))?;
        task.summary = summary.into();
        task.description = description.into();

        Ok(())
    }

    pub fn seconds_spent(&self, task: &Task, now: u64) -> u64 {
        match task.id == self.active {
            true => task.seconds_spent + now.saturating_sub(self.active_since),
            false => task.seconds_spent,
        }
    }

    pub fn record_file(&mut self, path: &str) {
        let files = &mut self.active_mut().files;
        files.retain(|f| f != path);
        files.push(path.into());

        if files.len() > CONTEXT_FILES_KEPT {
            files.remove(0);
        }
    }

    pub fn commit_message(&self, format: &str) -> Result<String, Error> {
        let task = self.active();

        if task.is_default() {
            return Err("No task is active.".into());
        }

        Ok(task.format(format))
    }

    pub fn pending_issues(&self) -> impl Iterator<Item = &Task> {
        self.issues.iter().filter(|issue| self.find(&issue.id).is_none())
    }
}

pub fn trimmed(text: &str, max: usize) -> String {
    match text.chars().count() > max {
        true => format!("{}…", text.chars().take(max - 1).collect::<String>()),
        false => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str) -> Task {
        Task::issue("jira", id, "Crash on start", "https://acme.atlassian.net/browse/PROJ-7", "https://acme.atlassian.net/rest/api/3/issue/PROJ-7")
    }

    #[test]
    fn switches_tasks_and_counts_their_time() {
        let mut tasks = TaskList::new(0);

        let first = tasks.add_local("Fix login", "");
        assert_eq!(first, "LOCAL-00001");
        tasks.activate(&first, 100).unwrap();
        assert_eq!(tasks.find(DEFAULT_TASK_ID).unwrap().seconds_spent, 100);

        let second = tasks.add_local("Write docs", "");
        tasks.activate(&second, 400).unwrap();
        assert_eq!(tasks.find(&first).unwrap().seconds_spent, 300);

        tasks.activate(&first, 1000).unwrap();
        assert_eq!(tasks.tasks[0].id, first);
        assert_eq!(tasks.seconds_spent(tasks.active(), 1060), 360);

        assert!(tasks.close(&first).is_err());
        tasks.activate(DEFAULT_TASK_ID, 1100).unwrap();
        tasks.close(&first).unwrap();
        assert!(tasks.find(&first).unwrap().closed);
        assert!(tasks.close(DEFAULT_TASK_ID).is_err());
    }

    #[test]
    fn activating_an_issue_makes_it_a_task() {
        let mut tasks = TaskList::new(0);
        tasks.issues = vec![issue("PROJ-7")];

        tasks.activate("PROJ-7", 10).unwrap();

        assert_eq!(tasks.active().label(), "PROJ-7: Crash on start");
        assert_eq!(tasks.pending_issues().count(), 0);
        assert_eq!(tasks.commit_message("{id} {summary}").unwrap(), "PROJ-7 Crash on start");
    }

    #[test]
    fn local_tasks_have_no_id_in_the_commit_message() {
        let mut tasks = TaskList::new(0);
        assert!(tasks.commit_message("{id} {summary}").is_err());

        let id = tasks.add_local("Refactor parser", "");
        tasks.activate(&id, 0).unwrap();

        assert_eq!(tasks.commit_message("{id} {summary}").unwrap(), "Refactor parser");
    }

    #[test]
    fn keeps_the_fifty_most_recent_closed_tasks() {
        let mut tasks = TaskList::new(0);

        for i in 0..55 {
            let id = tasks.add_local(&format!("task {i}"), "");
            tasks.close(&id).unwrap();
        }

        assert_eq!(tasks.tasks.iter().filter(|t| t.closed).count(), CLOSED_TASKS_KEPT);
        assert!(tasks.find("LOCAL-00055").is_some());
        assert!(tasks.find("LOCAL-00001").is_none());
    }

    #[test]
    fn remembers_the_last_ten_files_of_the_active_task() {
        let mut tasks = TaskList::new(0);

        for i in 0..12 {
            tasks.record_file(&format!("/project/{i}.rs"));
        }
        tasks.record_file("/project/5.rs");

        let files = &tasks.active().files;
        assert_eq!(files.len(), 10);
        assert_eq!(files.first().unwrap(), "/project/2.rs");
        assert_eq!(files.last().unwrap(), "/project/5.rs");
    }
}
