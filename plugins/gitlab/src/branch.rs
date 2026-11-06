//! The merge request of the current branch, as the original's branch widget
//! and editor review: a status bar item (or "Create Merge Request" when the pushed branch
//! has none), its unresolved threads on the lines of open files, and checking
//! out a merge request's branch with `git`.

use openidea_plugin::host::{self, storage};
use openidea_plugin::providers::{CodeAction, Document, LineMarker};
use openidea_plugin::{Diagnostic, Error, Invocation, LogLevel, Position, ProcessOutput, Project, Range, Severity, StatusItem, TextEdit};

use crate::api::{self, Discussion, MergeRequest};
use crate::{GitLab, SELECTED_KEY, first_line, relative_path};

pub const STATUS_ITEM: &str = "merge-request";
const RECHECK_SECS: u64 = 300;

pub struct BranchReview {
    /// Repository root, upstream and commit the lookup was made for.
    key: String,
    checked_at: u64,
    pub root: String,
    pub mr: Option<MergeRequest>,
    /// Unresolved threads on the new side, shown only while the branch has
    /// every commit of the merge request (the original hides them otherwise).
    pub threads: Vec<Discussion>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn editor_review_enabled() -> bool {
    host::setting_bool("editorReview").unwrap_or(true)
}

fn thread_text(d: &Discussion) -> String {
    let first = &d.notes[0];
    let replies = match d.notes.len() - 1 {
        0 => String::new(),
        1 => " (1 reply)".into(),
        n => format!(" ({n} replies)"),
    };

    format!("{}: {}{replies}", first.author.username, first_line(&first.body))
}

impl BranchReview {
    /// `(0-based line, thread)` of the threads on `path` (absolute).
    fn threads_on<'a>(&'a self, path: &str) -> Vec<(u32, &'a Discussion)> {
        let Some(relative) = relative_path(&self.root, path) else {
            return Vec::new();
        };

        let relative = relative.replace('\\', "/");
        self.threads
            .iter()
            .filter_map(|d| {
                let p = d.notes[0].position.as_ref()?;
                let line = p.new_line?;
                (p.new_path.as_deref() == Some(relative.as_str())).then_some((line.saturating_sub(1), d))
            })
            .collect()
    }
}

/// A GitLab suggestion block: replaces `above` lines before and `below` lines after the commented line with `text`.
#[derive(Debug, PartialEq)]
pub struct Suggestion {
    pub above: u32,
    pub below: u32,
    pub text: String,
}

/// The blocks opened by ```` ```suggestion ```` or ```` ```suggestion:-N+M ````.
pub fn suggestions(body: &str) -> Vec<Suggestion> {
    let mut found = Vec::new();
    let mut open: Option<(u32, u32, String)> = None;

    for line in body.lines() {
        let fence = line.trim_end();
        match open.take() {
            Some((above, below, text)) if fence == "```" => found.push(Suggestion { above, below, text }),
            Some((above, below, mut text)) => {
                text.push_str(line);
                text.push('\n');
                open = Some((above, below, text));
            }
            None => {
                let Some(range) = fence.strip_prefix("```suggestion") else {
                    continue;
                };
                let parsed = match range.strip_prefix(':') {
                    None if range.is_empty() => Some((0, 0)),
                    None => None,
                    Some(range) => range
                        .strip_prefix('-')
                        .and_then(|r| r.split_once('+'))
                        .and_then(|(above, below)| Some((above.parse().ok()?, below.parse().ok()?))),
                };
                open = parsed.map(|(above, below)| (above, below, String::new()));
            }
        }
    }

    found
}

/// The edit of `text` that applies `suggestion` to the 0-based commented `line`; `None` past the end of the file.
fn suggestion_edit(text: &str, line: u32, suggestion: &Suggestion) -> Option<TextEdit> {
    let first = line.checked_sub(suggestion.above)?;
    let last = line + suggestion.below;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let last_line = *lines.get(last as usize)?;

    let (end, replacement) = match last_line.ends_with('\n') {
        true => (Position::new(last + 1, 0), suggestion.text.clone()),
        false => (Position::new(last, last_line.chars().count() as u32), suggestion.text.trim_end_matches('\n').to_owned()),
    };

    Some(TextEdit { range: Range::new(Position::new(first, 0), end), text: replacement })
}

fn git(project: &Project, args: &[&str]) -> Result<ProcessOutput, Error> {
    let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();

    project.run_process("git", &args, None)
}

pub fn git_ok(project: &Project, args: &[&str]) -> Result<String, Error> {
    let output = git(project, args)?;
    if output.status != Some(0) {
        return Err(Error::Failed(format!("Git {} failed: {}", args[0], output.stderr.trim())));
    }

    Ok(output.stdout)
}

/// The local branch a merge request is checked out to; fork branches get the original's `fork/<owner>/` prefix.
pub fn local_branch(mr: &MergeRequest) -> String {
    match mr.source_project_id == mr.target_project_id {
        true => mr.source_branch.clone(),
        false => format!("fork/{}/{}", mr.author.username, mr.source_branch),
    }
}

/// Fetches and checks out `mr`'s branch from `remote` (the target project).
pub fn checkout(project: &Project, remote: &str, mr: &MergeRequest) -> Result<String, Error> {
    let local = local_branch(mr);

    if mr.source_project_id != mr.target_project_id {
        // ponytail: forks come from the target's refs/merge-requests/<iid>/head, never a fork remote.
        git_ok(project, &["fetch", remote, &format!("+refs/merge-requests/{}/head:refs/heads/{local}", mr.iid)])?;
        git_ok(project, &["checkout", &local])?;
        return Ok(format!("Checked out {local}."));
    }

    let tracking = format!("{remote}/{local}");
    git_ok(project, &["fetch", remote, &format!("+refs/heads/{local}:refs/remotes/{tracking}")])?;

    let exists = git(project, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{local}")])?.status == Some(0);
    if !exists {
        git_ok(project, &["checkout", "-b", &local, "--track", &tracking])?;
        return Ok(format!("Checked out {local}."));
    }

    git_ok(project, &["checkout", &local])?;
    if git_ok(project, &["merge", "--ff-only", &tracking]).is_err() {
        return Ok(format!("Checked out {local}. It has diverged from {tracking}, so it was not fast-forwarded."));
    }

    Ok(format!("Checked out {local}."))
}

impl GitLab {
    fn clear_branch(&mut self) {
        if self.branch.take().is_some() {
            host::remove_status_item(STATUS_ITEM);
            self.republish();
        }
    }

    fn default_branch(&mut self, project: &str) -> Result<String, Error> {
        if let Some(branch) = self.default_branches.get(project) {
            return Ok(branch.clone());
        }

        let branch = api::Project(project).info()?.default_branch.unwrap_or_default();
        self.default_branches.insert(project.to_owned(), branch.clone());

        Ok(branch)
    }

    /// Finds the open merge request of the window's branch (its upstream on a
    /// gitlab.com remote; not forks, as in the original) and shows it.
    pub(crate) fn update_branch(&mut self, ctx: &Invocation, force: bool) -> Result<(), Error> {
        let Some(project) = ctx.project() else {
            return Ok(());
        };

        let Some(status) = project.vcs_status()? else {
            self.clear_branch();
            return Ok(());
        };

        let upstream = status.upstream.clone().unwrap_or_default();
        let remote_url = upstream.split_once('/').and_then(|(remote, _)| {
            project.vcs_remotes().ok()?.into_iter().find(|(name, _)| name == remote).map(|(_, url)| url)
        });
        let (Some((_, branch)), Some(path)) = (upstream.split_once('/'), remote_url.as_deref().and_then(api::project_path)) else {
            self.clear_branch();
            return Ok(());
        };

        let key = format!("{}|{upstream}|{}", status.root, status.commit.as_deref().unwrap_or(""));
        let fresh = self.branch.as_ref().is_some_and(|b| b.key == key && now_secs() - b.checked_at < RECHECK_SECS);
        if fresh && !force {
            return Ok(());
        }

        let api = api::Project(&path);
        let query = format!("state=opened&source_branch={}", api::encode(branch));
        let mr = api.merge_requests(&query)?.into_iter().find(|mr| mr.source_project_id == mr.target_project_id);

        let threads = match &mr {
            Some(mr) if status.behind == 0 && editor_review_enabled() => api
                .discussions(mr.iid)?
                .into_iter()
                .filter(|d| !d.is_system() && !(d.resolvable() && d.resolved()))
                .collect(),
            _ => Vec::new(),
        };

        let item = match &mr {
            Some(mr) => {
                let mut tooltip = mr.title.clone();
                if status.behind > 0 {
                    tooltip.push_str("\nThis branch is behind its upstream. Pull to see the review threads in the editor.");
                }

                Some(StatusItem {
                    text: format!("!{} on {branch}", mr.iid),
                    tooltip: Some(tooltip),
                    command: Some("show-branch-merge-request".into()),
                })
            }
            None if self.default_branch(&path)? != branch => Some(StatusItem {
                text: "GitLab: Create Merge Request".into(),
                tooltip: Some(format!("{branch} has no open merge request")),
                command: Some("create-merge-request".into()),
            }),
            None => None,
        };

        match item {
            Some(item) => host::set_status_item(STATUS_ITEM, &item),
            None => host::remove_status_item(STATUS_ITEM),
        }

        self.branch = Some(BranchReview { key, checked_at: now_secs(), root: status.root, mr, threads });
        self.republish();

        Ok(())
    }

    /// Shows the review threads of `path` as problems (or clears them).
    pub(crate) fn publish_review(&mut self, path: &str) {
        let diagnostics: Vec<Diagnostic> = self
            .branch
            .iter()
            .flat_map(|b| b.threads_on(path))
            .map(|(line, d)| {
                let range = Range::new(Position::new(line, 0), Position::new(line + 1, 0));
                let mut diagnostic = Diagnostic::new(range, Severity::Info, thread_text(d));
                diagnostic.source = Some("GitLab".into());
                diagnostic
            })
            .collect();

        if diagnostics.is_empty() && !self.published.contains(path) {
            return;
        }

        match host::publish_diagnostics(path, &diagnostics) {
            Ok(()) if diagnostics.is_empty() => {
                self.published.remove(path);
            }
            Ok(()) => {
                self.published.insert(path.to_owned());
            }
            Err(e) => host::log(LogLevel::Debug, &format!("editor review for {path}: {e}")),
        }
    }

    fn republish(&mut self) {
        let paths: Vec<String> = self.published.iter().cloned().collect();
        for path in paths {
            self.publish_review(&path);
        }
    }

    pub(crate) fn branch_line_markers(&self, doc: &Document) -> Vec<LineMarker> {
        self.branch
            .iter()
            .flat_map(|b| b.threads_on(&doc.path))
            .map(|(line, d)| LineMarker { line, icon: None, tooltip: thread_text(d), target: None })
            .collect()
    }

    /// "Apply suggestion" for the suggestion blocks of the threads on the selected lines of the open file.
    /// The file may have changed since the comment: the lines are replaced as they are now.
    pub(crate) fn suggestion_actions(&self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(branch) = &self.branch else {
            return Vec::new();
        };

        branch
            .threads_on(&doc.path)
            .into_iter()
            .filter(|(line, _)| (selection.start.line..=selection.end.line).contains(line))
            .flat_map(|(line, d)| d.notes.iter().flat_map(|n| suggestions(&n.body)).map(move |s| (line, s)))
            .filter_map(|(line, suggestion)| {
                let edit = suggestion_edit(&doc.text, line, &suggestion)?;

                Some(CodeAction {
                    title: "GitLab: Apply suggestion".into(),
                    kind: Some("quickfix".into()),
                    preferred: false,
                    edits: vec![(doc.path.clone(), vec![edit])],
                    data: None,
                })
            })
            .collect()
    }

    pub(crate) fn show_branch_merge_request(&mut self) -> Result<Option<String>, Error> {
        let mr = self.branch.as_ref().and_then(|b| b.mr.as_ref()).ok_or("The current branch has no open merge request.")?;
        let iid = mr.iid;

        storage::set(SELECTED_KEY, &iid.to_string())?;
        self.reload_merge_request(iid);

        Ok(Some(format!("!{iid} is open in the Merge Request and Merge Request Changes tool windows.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_branches_get_a_prefix() {
        let mut mr = MergeRequest { source_branch: "fix".into(), source_project_id: 1, target_project_id: 1, ..MergeRequest::default() };
        assert_eq!(local_branch(&mr), "fix");

        mr.source_project_id = 2;
        mr.author.username = "ann".into();
        assert_eq!(local_branch(&mr), "fork/ann/fix");
    }
}
