//! Git for tasks, after GitTaskHandler (git4idea) and the VCS options of
//! OpenTaskDialog / CloseTaskDialog: a branch per task, shelving as stashes,
//! and the commit message as `commit.template`, which the IDE's commit field
//! starts from. Needs `process:git`; without it tasks have no branches.

use openidea_plugin::{Error, Project};

use crate::model::Task;

const TEMPLATE_FILE: &str = "openidea-task-message";

pub struct Git<'a> {
    project: &'a Project,
}

impl<'a> Git<'a> {
    pub fn new(project: &'a Project) -> Self {
        Self { project }
    }

    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let output = self.project.run_process("git", &args, None)?;

        if output.status != Some(0) {
            return Err(format!("git {}: {}", args.join(" "), output.stderr.trim()).into());
        }

        Ok(output.stdout.trim_end().to_owned())
    }

    /// `None` on a detached HEAD.
    pub fn current_branch(&self) -> Result<Option<String>, Error> {
        let branch = self.run(&["rev-parse", "--abbrev-ref", "HEAD"])?;

        Ok((branch != "HEAD").then_some(branch))
    }

    pub fn switch(&self, branch: &str) -> Result<(), Error> {
        self.run(&["switch", branch]).map(drop)
    }

    /// Checks out `name`, creating it from HEAD unless it exists.
    pub fn start_branch(&self, name: &str) -> Result<(), Error> {
        let exists = self.run(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{name}")]).is_ok();

        match exists {
            true => self.switch(name),
            false => self.run(&["switch", "-c", name]).map(drop),
        }
    }

    /// GitTaskHandler.mergeAndClose: merges the task branch into its base, then deletes it.
    pub fn merge_and_delete(&self, branch: &str, base: &str) -> Result<(), Error> {
        self.switch(base)?;
        self.run(&["merge", "--no-edit", branch])?;
        self.run(&["branch", "-d", branch]).map(drop)
    }

    /// Stashes the working tree changes of the task being left (nothing when clean).
    pub fn shelve(&self, task_id: &str) -> Result<(), Error> {
        if self.run(&["status", "--porcelain"])?.is_empty() {
            return Ok(());
        }

        self.run(&["stash", "push", "--include-untracked", "-m", &stash_message(task_id)]).map(drop)
    }

    /// Restores what `shelve` stashed for the task, if anything.
    pub fn unshelve(&self, task_id: &str) -> Result<(), Error> {
        let suffix = format!(": {}", stash_message(task_id));
        let stashes = self.run(&["stash", "list", "--format=%gd %s"])?;
        let stash = stashes.lines().find(|line| line.ends_with(&suffix)).and_then(|line| line.split(' ').next());

        match stash {
            Some(stash) => self.run(&["stash", "pop", stash]).map(drop),
            None => Ok(()),
        }
    }

    /// Points `commit.template` at a file holding `message` (`None`: removes
    /// it). A template the user set up is left alone.
    pub fn set_commit_template(&self, message: Option<&str>) -> Result<(), Error> {
        let path = self.run(&["rev-parse", "--path-format=absolute", "--git-path", TEMPLATE_FILE])?;
        let current = self.run(&["config", "--local", "--get", "commit.template"]).ok();

        if current.as_deref().is_some_and(|template| template != path) {
            return Ok(());
        }

        let Some(message) = message else {
            if current.is_some() {
                self.run(&["config", "--local", "--unset", "commit.template"])?;
            }
            return Ok(());
        };

        // The plugin can't write files; git can, with log's --output.
        self.run(&["log", "-1", &format!("--format={}", message.replace('%', "%%")), &format!("--output={path}")])?;
        self.run(&["config", "--local", "commit.template", &path]).map(drop)
    }
}

fn stash_message(task_id: &str) -> String {
    format!("openidea task {task_id}")
}

/// TaskManagerImpl.suggestBranchName with `format` (default `{id}`): the first
/// two words of the summary when that leaves nothing, as for local tasks;
/// then what git refuses in a branch name is replaced.
pub fn branch_name(task: &Task, format: &str) -> String {
    let mut name = task.format(format);

    if name.is_empty() {
        name = task.summary.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).take(2).collect::<Vec<_>>().join("-");
    }

    let name: String = name
        .chars()
        .map(|c| match c {
            ' ' => '-',
            c if c.is_alphanumeric() || "-_./".contains(c) => c,
            _ => '_',
        })
        .collect();
    let name = name.replace("..", "_");
    let name = name.trim_matches(|c| c == '-' || c == '.' || c == '/');
    let name = name.strip_suffix(".lock").unwrap_or(name);

    match name.is_empty() {
        true => task.id.clone(),
        false => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_branches_like_intellij() {
        let issue = Task::issue("jira", "PROJ-7", "Crash on start", "u", "a");
        let local = Task { id: "LOCAL-00003".into(), summary: "Fix the: login page".into(), ..Task::default() };

        assert_eq!(branch_name(&issue, "{id}"), "PROJ-7");
        assert_eq!(branch_name(&issue, "feature/{id} {summary}"), "feature/PROJ-7-Crash-on-start");
        assert_eq!(branch_name(&local, "{id}"), "Fix-the");
        assert_eq!(branch_name(&local, "{summary}"), "Fix-the_-login-page");
    }
}
