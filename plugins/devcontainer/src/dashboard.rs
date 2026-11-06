//! The Dev Containers view: the containers of the project, found by the label the CLI puts on them.

use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, Invocation, NotificationLevel, Project, host};

use crate::{LOGIN_SHELL, Main};

pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub running: bool,
}

/// The lines of `docker ps -a --format '{{json .}}'`; lines that are not JSON objects are skipped.
pub fn parse_containers(stdout: &str) -> Vec<Container> {
    let text = |value: &serde_json::Value, key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_owned();

    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| {
            let id = text(&value, "ID");
            let status = text(&value, "Status");
            let running = match text(&value, "State").as_str() {
                "" => status.starts_with("Up"),
                state => state == "running",
            };
            (!id.is_empty()).then(|| Container { name: text(&value, "Names"), image: text(&value, "Image"), status, running, id })
        })
        .collect()
}

fn list(project: &Project, root: &str) -> Result<Vec<Container>, Error> {
    let args = ["ps", "-a", "--filter", &format!("label=devcontainer.local_folder={root}"), "--format", "{{json .}}"].map(str::to_owned);
    let output = project.run_process("docker", &args, None)?;
    if output.status != Some(0) {
        return Err(format!("docker ps failed: {}", output.stderr.trim()).into());
    }

    Ok(parse_containers(&output.stdout))
}

impl Views for Main {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let selected = self.selected.clone();
        let toolbar = |view: View, running_ids: &std::collections::BTreeSet<String>| {
            let selected = selected.as_ref();
            let running = selected.is_some_and(|id| running_ids.contains(id));
            view.button(Button::new("refresh", "Refresh").icon("refresh"))
                .button(Button::new("shell", "Open Shell").icon("run").enabled(running))
                .button(Button::new("stop", "Stop").icon("stop").enabled(running))
                .button(Button::new("start", "Start").icon("run").enabled(selected.is_some() && !running))
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
            self.running.clear();
            return Ok(toolbar(View::message("No dev container for this project. Run Dev Container: Start Container."), &self.running));
        }

        let running: std::collections::BTreeSet<String> = containers.iter().filter(|c| c.running).map(|c| c.id.clone()).collect();
        let items = containers
            .iter()
            .map(|c| ListItem::new(&c.id, &c.name).icon("database").detail(&format!("{} - {}", c.image, c.status)))
            .collect();

        let status = format!("{} running, {} stopped", running.len(), containers.len() - running.len());
        let view = toolbar(View::new(Body::List(items)), &running).status(&status);
        self.running = running;

        Ok(view)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Select(id) => self.selected = Some(id),
            ViewEvent::Activate(id) if self.running.contains(&id) => open_shell(&id)?,
            ViewEvent::Activate(id) => self.start(view, ctx, id)?,
            ViewEvent::Button(button) if button == "refresh" => host::refresh_view(view),
            ViewEvent::Button(button) if button == "shell" => {
                if let Some(id) = &self.selected {
                    open_shell(id)?;
                }
            }
            ViewEvent::Button(button) if button == "start" => {
                if let Some(id) = self.selected.clone() {
                    self.start(view, ctx, id)?;
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

impl Main {
    fn start(&mut self, view: &str, ctx: &Invocation, id: String) -> Result<(), Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let started = project.run_process("docker", &["start".to_owned(), id], None)?;
        if started.status != Some(0) {
            return Err(format!("docker start failed: {}", started.stderr.trim()).into());
        }
        host::notify(NotificationLevel::Info, "Dev Container", "Started the container.");
        host::refresh_view(view);
        Ok(())
    }
}

fn open_shell(id: &str) -> Result<(), Error> {
    let args = ["exec", "-it", id, "sh", "-c", LOGIN_SHELL].map(str::to_owned);

    host::open_terminal("Dev Container: Shell", "docker", &args)
}
