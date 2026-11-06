//! The Dev Containers view: the running containers of the project, found by the label the CLI puts on them.

use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, NotificationLevel, Project, host};

use crate::{LOGIN_SHELL, Main};

pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
}

/// The lines of `docker ps --format '{{json .}}'`; lines that are not JSON objects are skipped.
pub fn parse_containers(stdout: &str) -> Vec<Container> {
    let text = |value: &serde_json::Value, key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_owned();

    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| {
            let id = text(&value, "ID");
            (!id.is_empty()).then(|| Container { name: text(&value, "Names"), image: text(&value, "Image"), status: text(&value, "Status"), id })
        })
        .collect()
}

fn list(project: &Project, root: &str) -> Result<Vec<Container>, Error> {
    let args = ["ps", "--filter", &format!("label=devcontainer.local_folder={root}"), "--format", "{{json .}}"].map(str::to_owned);
    let output = project.run_process("docker", &args, None)?;
    if output.status != Some(0) {
        return Err(format!("docker ps failed: {}", output.stderr.trim()).into());
    }

    Ok(parse_containers(&output.stdout))
}

impl Views for Main {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let toolbar = |view: View| {
            let selected = self.selected.is_some();
            view.button(Button::new("refresh", "Refresh").icon("refresh"))
                .button(Button::new("shell", "Open Shell").icon("run").enabled(selected))
                .button(Button::new("stop", "Stop").icon("stop").enabled(selected))
        };
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to see its dev containers."));
        };
        let containers = match list(&project, &project.root()?) {
            Ok(containers) => containers,
            Err(Error::Denied(_)) => return Ok(View::message("Allow the docker permission in Settings > Plugins to list the containers.")),
            Err(error) => return Ok(View::message(&format!("Cannot list containers: {error}"))),
        };
        if containers.is_empty() {
            return Ok(toolbar(View::message("No running dev container for this project. Run Dev Container: Start Container.")));
        }

        let items = containers
            .iter()
            .map(|c| ListItem::new(&c.id, &c.name).icon("database").detail(&format!("{} - {}", c.image, c.status)))
            .collect();

        Ok(toolbar(View::new(Body::List(items))).status(&format!("{} running", containers.len())))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Select(id) => self.selected = Some(id),
            ViewEvent::Activate(id) => open_shell(&id)?,
            ViewEvent::Button(button) if button == "refresh" => host::refresh_view(view),
            ViewEvent::Button(button) if button == "shell" => {
                if let Some(id) = &self.selected {
                    open_shell(id)?;
                }
            }
            ViewEvent::Button(button) if button == "stop" => {
                let Some(id) = self.selected.take() else {
                    return Ok(());
                };
                let project = ctx.project().ok_or("No project is open.")?;
                let stopped = project.run_process("docker", &["stop".to_owned(), id], None)?;
                if stopped.status != Some(0) {
                    return Err(format!("docker stop failed: {}", stopped.stderr.trim()).into());
                }
                host::notify(NotificationLevel::Info, "Dev Container", "Stopped the container.");
                host::refresh_view(view);
            }
            _ => {}
        }

        Ok(())
    }
}

fn open_shell(id: &str) -> Result<(), Error> {
    let args = ["exec", "-it", id, "sh", "-c", LOGIN_SHELL].map(str::to_owned);

    host::open_terminal("Dev Container: Shell", "docker", &args)
}
