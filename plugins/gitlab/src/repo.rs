//! Clone (a list of the user's projects) and Share Project, both running the
//! approved `git` (`process:git`).

use openidea_plugin::host;
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent};
use openidea_plugin::{Error, Invocation, NotificationLevel};

use crate::api::{self, ProjectInfo};
use crate::branch::git_ok;
use crate::{GitLab, PROJECTS, first_line};

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
        let target = format!("{directory}/{}", project.path);

        // A terminal, not a one-off process: clones outlast the 60 s call limit and may ask for credentials.
        host::open_terminal(&format!("Clone {}", project.name), "git", &["clone".into(), clone_url(project).into(), target.clone()])?;
        host::notify(NotificationLevel::Info, "Cloning from GitLab", &format!("{} → {target}\nWhen the clone finishes, open it with File › Open.", project.path_with_namespace));

        Ok(())
    }
}

/// Creates a private project in the user's namespace named after the folder,
/// adds it as `origin` (or `gitlab`) and pushes the current branch.
pub fn share(ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let status = project.vcs_status()?.ok_or("Create a git repository and commit your files first.")?;
    let branch = status.branch.clone().ok_or("Check out a branch first.")?;
    if status.commit.is_none() {
        return Err("There is nothing to push yet. Commit your files first.".into());
    }

    let remotes = project.vcs_remotes()?;
    if remotes.iter().any(|(_, url)| api::project_path(url).is_some()) {
        return Err("The repository already has a gitlab.com remote.".into());
    }

    let remote = ["origin", "gitlab"]
        .into_iter()
        .find(|name| remotes.iter().all(|(existing, _)| existing != name))
        .ok_or("The remotes origin and gitlab both exist already.")?;

    git_ok(&project, &["--version"])?;

    let folder = status.root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or("project");
    let created = {
        let _task = host::begin_task("Creating GitLab project", false);
        api::create_project(&project_name(folder))?
    };

    let url = match use_ssh() {
        true => &created.ssh_url_to_repo,
        false => &created.http_url_to_repo,
    };
    git_ok(&project, &["remote", "add", remote, url])?;
    host::open_terminal("Push to GitLab", "git", &["push".into(), "-u".into(), remote.into(), branch])?;
    host::notify(NotificationLevel::Info, "Shared on GitLab", &created.web_url);

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
