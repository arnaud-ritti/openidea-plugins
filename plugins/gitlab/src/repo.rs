//! Clone (a list of the user's projects) and Share Project, both running the
//! approved `git` (`process:git`).

use openidea_plugin::host;
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent};
use openidea_plugin::{Error, Invocation, NotificationLevel};

use crate::api::{self, ProjectInfo};
use crate::branch::git_ok;
use crate::prompts::{self, ShareRequest};
use crate::{GitLab, PROJECTS, first_line, safe};

fn use_ssh() -> bool {
    host::setting_bool("cloneUsingSsh").unwrap_or(false)
}

fn clone_url(project: &ProjectInfo) -> &str {
    match use_ssh() {
        true => &project.ssh_url_to_repo,
        false => &project.http_url_to_repo,
    }
}

/// Where clones go: the `cloneDirectory` setting, else next to the window's project.
fn clone_directory(ctx: &Invocation) -> Option<String> {
    let configured = host::setting_string("cloneDirectory").unwrap_or_default();
    if !configured.trim().is_empty() {
        return Some(configured.trim().trim_end_matches(['/', '\\']).to_owned());
    }

    let root = ctx.project()?.root().ok()?;
    let (parent, _) = root.trim_end_matches(['/', '\\']).rsplit_once(['/', '\\'])?;

    Some(parent.to_owned())
}

/// A GitLab project name from a folder name (`[a-zA-Z0-9_.-]+`, as the original validates).
pub fn project_name(folder: &str) -> String {
    folder.chars().map(|c| if c.is_ascii_alphanumeric() || "_.-".contains(c) { c } else { '-' }).collect()
}

impl GitLab {
    pub(crate) fn render_projects(&mut self, ctx: &Invocation) -> Result<View, Error> {
        if self.projects.is_none() {
            let _task = host::begin_task("Loading GitLab projects", false);
            self.projects = Some(api::member_projects()?);
        }

        let items: Vec<ListItem> = self
            .projects
            .iter()
            .flatten()
            .map(|p| {
                ListItem::new(&p.path_with_namespace, &p.name_with_namespace).icon("module").detail(&first_line(p.description.as_deref().unwrap_or("")))
            })
            .collect();
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        if items.is_empty() {
            return Ok(View::message("You aren't a member of any project.").button(refresh));
        }

        let status = match clone_directory(ctx) {
            Some(dir) => format!("Double-click a project to clone it into {dir}"),
            None => "Set the clone directory under Settings › Plugins › GitLab".into(),
        };

        Ok(View::new(Body::List(items)).button(refresh).status(&status))
    }

    pub(crate) fn handle_projects(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        let path = match event {
            ViewEvent::Button(id) if id == "refresh" => {
                self.projects = None;
                host::refresh_view(PROJECTS);
                return Ok(());
            }
            ViewEvent::Activate(path) => path,
            _ => return Ok(()),
        };

        let project = self.projects.iter().flatten().find(|p| p.path_with_namespace == path).ok_or("The project is no longer in the list. Refresh it.")?;
        let directory = clone_directory(ctx).ok_or("Set the clone directory under Settings › Plugins › GitLab.")?;
        let target = format!("{directory}/{}", safe::remote_name(&project.path)?);
        let url = safe::clone_url(clone_url(project))?;

        // A terminal, not a one-off process: clones outlast the 60 s call limit and may ask for credentials.
        host::open_terminal(&format!("Clone {}", project.name), "git", &["clone".into(), "--".into(), url.into(), target.clone()])?;
        host::notify(NotificationLevel::Info, "Cloning from GitLab", &format!("{} → {target}\nWhen the clone finishes, open it with File › Open.", project.path_with_namespace));

        Ok(())
    }
}

/// The command: asks for name and visibility. A repository without a commit is initialised and committed
/// only after a confirm that lists the files, and the project is created only after that commit succeeded.
pub fn share(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let status = project.vcs_status()?;
    let root = match &status {
        Some(s) => s.root.clone(),
        None => project.root()?,
    };
    let folder = root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("project");

    let Some(mut request) = prompts::ask_share(&project_name(folder))? else {
        return Ok(None);
    };

    let has_commit = status.as_ref().is_some_and(|s| s.commit.is_some());
    if !has_commit {
        if !host::confirm("Share Project", "The project has no commit yet. Create the repository if needed and commit all its files first?")? {
            return Ok(None);
        }

        request.init = status.is_none();
        request.commit_all = true;
    }

    let mut confirmed: Option<Vec<String>> = None;
    while let Some(files) = share_with(ctx, &request, confirmed.as_deref())? {
        let list = files.join("\n");
        if !host::confirm("Share Project", &format!("Commit these {} files as \"Initial commit\"?\n{list}", files.len()))? {
            return Ok(None);
        }

        confirmed = Some(files);
    }

    Ok(None)
}

/// `.env*`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`, matched on the file name, any case.
fn looks_secret(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();

    name.starts_with(".env") || name.ends_with(".pem") || name.ends_with(".key") || name.starts_with("id_rsa") || name.starts_with("id_ed25519")
}

/// The paths of `git status --porcelain -z` entries (`XY path`); a rename or copy is followed by its source.
fn porcelain_paths(stdout: &str) -> Vec<String> {
    let mut entries = stdout.split('\0').filter(|e| !e.is_empty());
    let mut paths = Vec::new();

    while let Some(entry) = entries.next() {
        let Some(path) = entry.get(3..) else { continue };
        paths.push(path.to_owned());

        if entry[..2].contains(['R', 'C']) {
            paths.extend(entries.next().map(str::to_owned));
        }
    }

    paths
}

fn same_files(a: &[String], b: &[String]) -> bool {
    let sorted = |files: &[String]| {
        let mut files = files.to_vec();
        files.sort();
        files.dedup();
        files
    };

    sorted(a) == sorted(b)
}

/// Shares the project as the request says. Every field is checked before the first `git` run and the local
/// steps (`git init`, the commit) succeed before the project is created. With commit-all it returns the files
/// `git status` lists, and commits only when called again with exactly those files as `confirmed`.
pub fn share_with(ctx: &Invocation, request: &ShareRequest, confirmed: Option<&[String]>) -> Result<Option<Vec<String>>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    if !api::VISIBILITIES.contains(&request.visibility.as_str()) {
        return Err("Choose a visibility.".into());
    }

    let status = project.vcs_status()?;
    if status.is_none() && !request.init {
        return Err("Create a git repository and commit your files first.".into());
    }

    let has_commit = status.as_ref().is_some_and(|s| s.commit.is_some());
    if !has_commit && !request.commit_all {
        return Err("There is nothing to push yet. Commit your files first.".into());
    }

    let branch = status.as_ref().and_then(|s| s.branch.as_deref()).map(safe::ref_name).transpose()?.map(str::to_owned);
    if branch.is_none() && has_commit {
        return Err("Check out a branch first.".into());
    }

    let remotes = match status {
        Some(_) => project.vcs_remotes()?,
        None => Vec::new(),
    };
    if remotes.iter().any(|(_, url)| api::project_path(url).is_some()) {
        return Err("The repository already has a gitlab.com remote.".into());
    }

    let remote = ["origin", "gitlab"]
        .into_iter()
        .find(|name| remotes.iter().all(|(existing, _)| existing != name))
        .ok_or("The remotes origin and gitlab both exist already. Remove one first.")?;

    let name = request.name.trim();

    git_ok(&project, &["--version"])?;
    if status.is_none() {
        git_ok(&project, &["init"])?;
    }

    let branch = match branch {
        Some(branch) => branch,
        None => safe::ref_name(git_ok(&project, &["symbolic-ref", "--short", "HEAD"])?.trim())?.to_owned(),
    };

    if request.commit_all {
        let files = porcelain_paths(&git_ok(&project, &["status", "--porcelain", "-z", "--untracked-files=all"])?);
        let secret: Vec<&str> = files.iter().map(String::as_str).filter(|f| looks_secret(f)).collect();
        if !secret.is_empty() {
            return Err(format!("Nothing was committed: {} look secret. Ignore them in .gitignore or commit by hand.", secret.join(", ")).into());
        }

        if files.is_empty() && !has_commit {
            return Err("There is nothing to commit.".into());
        }

        if !files.is_empty() {
            if !confirmed.is_some_and(|c| same_files(c, &files)) {
                return Ok(Some(files));
            }

            git_ok(&project, &["add", "-A"])?;
            // A file created between the listing and `add -A` is staged too; only the confirmed set gets committed.
            let staged: Vec<String> =
                git_ok(&project, &["diff", "--cached", "--name-only", "-z", "--no-renames"])?.split('\0').filter(|f| !f.is_empty()).map(str::to_owned).collect();
            if !same_files(&staged, &files) || staged.iter().any(|f| looks_secret(f)) {
                git_ok(&project, &["reset", "-q"])?;
                return Ok(Some(staged));
            }

            git_ok(&project, &["commit", "-m", "Initial commit"])?;
        }
    }

    let created = {
        let _task = host::begin_task("Creating GitLab project", false);
        api::create_project(name, &request.visibility)?
    };

    let url = safe::clone_url(match use_ssh() {
        true => &created.ssh_url_to_repo,
        false => &created.http_url_to_repo,
    })?;
    let web_url = safe::gitlab_url(&created.web_url)?;
    git_ok(&project, &["remote", "add", "--", remote, url])?;
    host::open_terminal("Push to GitLab", "git", &["push".into(), "-u".into(), "--".into(), remote.into(), branch])?;
    host::notify(NotificationLevel::Info, "Shared on GitLab", web_url);

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names_from_folders() {
        assert_eq!(project_name("my app (v2)"), "my-app--v2-");
        assert_eq!(project_name("lib_x.rs"), "lib_x.rs");
    }
}
