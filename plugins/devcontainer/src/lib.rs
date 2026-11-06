//! Dev Container CLI: drives the project's development container with the
//! `devcontainer` CLI (https://containers.dev).

use openidea_plugin::{Error, Invocation, NotificationLevel, Plugin, Project, host, register_plugin};

/// Where the CLI looks for a configuration, in order. The host's `find_files`
/// skips hidden directories, so these are probed with `read_file`; configs in
/// `.devcontainer/<folder>/` are picked with the `configFile` setting.
const DEFAULT_CONFIGS: [&str; 2] = [".devcontainer/devcontainer.json", ".devcontainer.json"];

const LOGIN_SHELL: &str = "command -v bash >/dev/null && exec bash -l; exec sh -l";

#[derive(Default)]
pub struct Main;

impl Plugin for Main {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let root = project.root()?;

        if id == "stop" {
            return stop(&project, &root);
        }

        let mut args = vec!["--workspace-folder".to_owned(), root.clone(), "--config".to_owned(), config_file(&project)?];

        let (title, command, extra): (&str, &str, &[&str]) = match id {
            "up" => ("Dev Container: Start Container", "up", &[]),
            "rebuild" => ("Dev Container: Rebuild Container", "up", &["--remove-existing-container"]),
            "shell" => ("Dev Container: Open Shell in Container", "exec", &["sh", "-c", LOGIN_SHELL]),
            _ => return Err(Error::NotFound(format!("command {id}"))),
        };

        args.insert(0, command.to_owned());
        args.extend(extra.iter().map(|a| a.to_string()));
        host::open_terminal(title, "devcontainer", &args)?;

        Ok(None)
    }
}

fn config_file(project: &Project) -> Result<String, Error> {
    let configured = host::setting_string("configFile").unwrap_or_default();

    if !configured.trim().is_empty() {
        return Ok(configured.trim().to_owned());
    }

    for path in DEFAULT_CONFIGS {
        match project.read_file(path) {
            Ok(_) => return Ok(path.to_owned()),
            Err(Error::Denied(cap)) => return Err(Error::Denied(cap)),
            Err(_) => continue,
        }
    }

    Err("No .devcontainer/devcontainer.json or .devcontainer.json found in this project. To use another file, set it in Settings › Plugins › Dev Container CLI › Configuration file.".into())
}

/// The CLI has no stop command; its containers carry the workspace folder as a label.
fn stop(project: &Project, root: &str) -> Result<Option<String>, Error> {
    let label = format!("label=devcontainer.local_folder={root}");
    let listed = project.run_process("docker", &["ps".into(), "-q".into(), "--filter".into(), label], None)?;

    if listed.status != Some(0) {
        return Err(format!("docker ps failed: {}", listed.stderr.trim()).into());
    }

    let ids: Vec<String> = listed.stdout.split_whitespace().map(str::to_owned).collect();

    if ids.is_empty() {
        host::notify(NotificationLevel::Info, "Dev Container", "No running container for this project.");
        return Ok(None);
    }

    let stopped = project.run_process("docker", &[vec!["stop".to_owned()], ids.clone()].concat(), None)?;

    if stopped.status != Some(0) {
        return Err(format!("docker stop failed: {}", stopped.stderr.trim()).into());
    }

    let stopped = match ids.len() {
        1 => "Stopped 1 container.".to_owned(),
        count => format!("Stopped {count} containers."),
    };
    host::notify(NotificationLevel::Info, "Dev Container", &stopped);

    Ok(None)
}

register_plugin!(Main);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::mock::MockHost;

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read", "process:devcontainer", "process:docker"]);
        host.set_project_root(Some("/work/app"));
        host
    }

    fn ok(stdout: &str) -> Result<ProcessOutput, Error> {
        Ok(ProcessOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    #[test]
    fn up_uses_the_default_config() {
        let host = host();
        host.add_file(".devcontainer/devcontainer.json", "{}");
        host.add_file(".devcontainer.json", "{}");

        Main.run_command("up", &host.invocation(), None).unwrap();

        let terminal = &host.terminals()[0];
        assert_eq!(terminal.title, "Dev Container: Start Container");
        assert_eq!(terminal.program, "devcontainer");
        assert_eq!(terminal.args, ["up", "--workspace-folder", "/work/app", "--config", ".devcontainer/devcontainer.json"]);
    }

    #[test]
    fn rebuild_falls_back_to_the_root_config() {
        let host = host();
        host.add_file(".devcontainer.json", "{}");

        Main.run_command("rebuild", &host.invocation(), None).unwrap();

        assert_eq!(
            host.terminals()[0].args,
            ["up", "--workspace-folder", "/work/app", "--config", ".devcontainer.json", "--remove-existing-container"]
        );
    }

    #[test]
    fn shell_uses_the_configured_file() {
        let host = host();
        host.set_setting("configFile", "\".devcontainer/python/devcontainer.json\"");

        Main.run_command("shell", &host.invocation(), None).unwrap();

        assert_eq!(
            host.terminals()[0].args,
            ["exec", "--workspace-folder", "/work/app", "--config", ".devcontainer/python/devcontainer.json", "sh", "-c", LOGIN_SHELL]
        );
    }

    #[test]
    fn up_without_config_fails() {
        let host = host();

        let error = Main.run_command("up", &host.invocation(), None).unwrap_err();

        assert!(error.to_string().starts_with("No .devcontainer/devcontainer.json"));
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn stop_stops_the_project_containers() {
        let host = host();
        host.on_process("docker", |args, _| match args[0].as_str() {
            "ps" => {
                assert_eq!(args[3], "label=devcontainer.local_folder=/work/app");
                ok("abc\ndef\n")
            }
            "stop" => {
                assert_eq!(args, ["stop", "abc", "def"]);
                ok("")
            }
            other => panic!("unexpected docker {other}"),
        });

        Main.run_command("stop", &host.invocation(), None).unwrap();

        assert_eq!(host.notifications()[0].body, "Stopped 2 containers.");
    }

    #[test]
    fn stop_without_docker_is_denied() {
        let host = host().with_capabilities(&["project.read", "process:devcontainer"]);

        let error = Main.run_command("stop", &host.invocation(), None).unwrap_err();

        assert!(matches!(error, Error::Denied(cap) if cap == "process:docker"));
    }

    fn strip_line_comments(text: &str) -> String {
        text.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn all_templates_registered() {
        let manifest = include_str!("../plugin.toml");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut registered = 0;

        for entry in std::fs::read_dir(&dir).unwrap() {
            let name = entry.unwrap().file_name().into_string().unwrap();
            let path = format!("path = \"templates/{name}\"");
            assert!(manifest.contains(&path), "{name} is not registered");
            registered += 1;
        }

        assert_eq!(manifest.matches("[[contributes.fileTemplates]]").count(), registered);

        for line in manifest.lines().filter(|l| l.starts_with("path = \"templates/")) {
            let relative = line.trim_start_matches("path = \"").trim_end_matches('"');
            let text = std::fs::read_to_string(dir.parent().unwrap().join(relative)).unwrap();
            let json: serde_json::Value = serde_json::from_str(&strip_line_comments(&text)).unwrap_or_else(|e| panic!("{relative}: {e}"));
            assert!(json.get("$schema").is_some(), "{relative} lacks $schema");
            assert!(json.get("image").is_some() || json.get("dockerComposeFile").is_some(), "{relative} lacks image or dockerComposeFile");
        }

        assert!(registered >= 7);
    }
}
