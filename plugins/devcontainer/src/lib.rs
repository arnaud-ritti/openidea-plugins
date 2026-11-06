//! Dev Container CLI: drives the project's development container with the
//! `devcontainer` CLI (https://containers.dev).

mod checks;

use std::path::Path;

use openidea_plugin::text::position;
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, LogLevel, NotificationLevel, Plugin, Project, Range, host, register_plugin,
};

use checks::{FEATURES_MALFORMED, UNKNOWN_PROPERTY};

const MAX_BYTES: usize = 256 * 1024;

/// Where the CLI looks for a configuration, in order. The host's `find_files`
/// skips hidden directories, so these are probed with `read_file`; configs in
/// `.devcontainer/<folder>/` are picked with the `configFile` setting.
const DEFAULT_CONFIGS: [&str; 2] = [".devcontainer/devcontainer.json", ".devcontainer.json"];

const LOGIN_SHELL: &str = "command -v bash >/dev/null && exec bash -l; exec sh -l";

#[derive(Default)]
pub struct Main;

impl Plugin for Main {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileOpened, EventKind::FileSaved] {
            if let Err(error) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("Dev Container inspections need the {} event: {error}", kind.name()));
            }
        }

        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            if is_devcontainer_file(path) {
                inspect(path);
            }
        }
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let root = project.root()?;

        if id == "stop" {
            return stop(&project, &root);
        }

        if id == "info" {
            return info(&project, &root);
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

fn is_devcontainer_file(path: &str) -> bool {
    let path = path.replace('\\', "/");

    path.ends_with("/.devcontainer.json") || (path.ends_with("/devcontainer.json") && path.contains("/.devcontainer/"))
}

fn inspect(path: &str) {
    let Some(project) = host::projects()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.root().ok()?, p)))
        .filter(|(root, _)| Path::new(path).starts_with(root))
        .max_by_key(|(root, _)| root.len())
        .map(|(_, project)| project)
    else {
        return;
    };

    let Ok(text) = project.read_file(path) else { return };
    let findings = match text.len() > MAX_BYTES {
        true => vec![],
        false => enabled_findings(&text),
    };
    let diagnostics: Vec<Diagnostic> = findings
        .iter()
        .map(|f| Diagnostic {
            code: Some(f.code.into()),
            ..Diagnostic::new(Range::new(position(&text, f.start), position(&text, f.end)), f.severity, &f.message)
        })
        .collect();

    let _ = host::publish_diagnostics(path, &diagnostics);
}

fn enabled_findings(text: &str) -> Vec<checks::Finding> {
    let properties = host::setting_bool("checkProperties").unwrap_or(true);
    let features = host::setting_bool("checkFeatures").unwrap_or(true);

    checks::check(text)
        .into_iter()
        .filter(|f| match f.code {
            UNKNOWN_PROPERTY => properties,
            FEATURES_MALFORMED => features,
            _ => true,
        })
        .collect()
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

fn info(project: &Project, root: &str) -> Result<Option<String>, Error> {
    let args = vec!["read-configuration".to_owned(), "--workspace-folder".to_owned(), root.to_owned(), "--config".to_owned(), config_file(project)?];
    let _task = host::begin_task("Reading the Dev Container configuration", false);
    let output = project.run_process("devcontainer", &args, None)?;

    if output.status != Some(0) {
        return Err(format!("devcontainer read-configuration failed: {}", output.stderr.trim()).into());
    }

    let summary = summarise(&output.stdout).ok_or("devcontainer read-configuration printed no JSON configuration.")?;
    host::notify(NotificationLevel::Info, "Dev Container", &summary);

    Ok(None)
}

/// The CLI may print log noise before its JSON, so the first parseable object wins.
fn summarise(stdout: &str) -> Option<String> {
    let value = stdout
        .match_indices('{')
        .find_map(|(start, _)| serde_json::Deserializer::from_str(&stdout[start..]).into_iter::<serde_json::Value>().next()?.ok())?;
    let config = value.get("configuration")?;
    let mut lines = vec![];

    if let Some(image) = config.get("image").and_then(|v| v.as_str()) {
        lines.push(format!("Image: {image}"));
    }

    let compose = match config.get("dockerComposeFile") {
        Some(serde_json::Value::String(file)) => Some(file.clone()),
        Some(serde_json::Value::Array(files)) => Some(files.iter().filter_map(|f| f.as_str()).collect::<Vec<_>>().join(", ")),
        _ => None,
    };
    if let Some(files) = compose {
        lines.push(format!("Compose file: {files}"));
    }

    if let Some(user) = config.get("remoteUser").and_then(|v| v.as_str()) {
        lines.push(format!("Remote user: {user}"));
    }

    let ports: Vec<String> = config
        .get("forwardPorts")
        .and_then(|v| v.as_array())
        .map(|ports| ports.iter().map(|p| p.as_str().map(str::to_owned).unwrap_or_else(|| p.to_string())).collect())
        .unwrap_or_default();
    lines.push(match ports.is_empty() {
        true => "Forwarded ports: none".to_owned(),
        false => format!("Forwarded ports: {}", ports.join(", ")),
    });

    let features = config.get("features").and_then(|v| v.as_object()).map_or(0, |f| f.len());
    lines.push(format!("Features: {features}"));

    let has_post_create = config.get("postCreateCommand").is_some_and(|v| !v.is_null() && v.as_str() != Some(""));
    lines.push(format!("postCreateCommand: {}", if has_post_create { "yes" } else { "no" }));

    Some(lines.join("\n"))
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

    fn open(host: &MockHost, path: &str, text: &str) {
        host.add_file(path, text);
        Main.handle_events(&[Event::FileOpened(format!("/work/app/{path}"))], &host.invocation());
    }

    fn published(host: &MockHost, path: &str) -> Vec<Diagnostic> {
        host.diagnostics().get(&format!("/work/app/{path}")).cloned().unwrap_or_default()
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics.iter().map(|d| d.code.as_deref().unwrap()).collect()
    }

    const FILE: &str = ".devcontainer/devcontainer.json";

    #[test]
    fn unknown_top_level_property_warns() {
        let host = host();
        open(&host, FILE, "{\n  \"image\": \"x\",\n  \"postCreateCommnad\": \"ls\"\n}\n");

        let found = published(&host, FILE);

        assert_eq!(codes(&found), ["unknown-property"]);
        assert_eq!(found[0].range, Range::new(openidea_plugin::Position::new(2, 2), openidea_plugin::Position::new(2, 21)));
        assert!(found[0].message.contains("postCreateCommand"));
    }

    #[test]
    fn unknown_property_is_quiet_for_vendor_and_unrelated_names() {
        let host = host();
        open(&host, FILE, "{\"$schema\": \"s\", \"x-vendor\": 1, \"completelyDifferent\": 2, \"image\": \"i\", \"customizations\": {\"vscode\": {\"extensons\": []}}}");

        assert!(published(&host, FILE).is_empty());
    }

    #[test]
    fn features_must_be_an_object() {
        let host = host();
        open(&host, FILE, "{\"features\": [\"a\"]}");

        assert_eq!(codes(&published(&host, FILE)), ["features-malformed"]);
    }

    #[test]
    fn feature_id_with_whitespace_warns() {
        let host = host();
        open(&host, FILE, "{\"features\": {\"ghcr.io/x/y node:1\": {}, \"\": {}, \"ok:1\": \"latest\", \"b\": true, \"c\": 3}}");

        assert_eq!(codes(&published(&host, FILE)), ["features-malformed"; 3]);
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_are_read() {
        let host = host();
        open(
            &host,
            FILE,
            "{\n // \"fake\": 1,\n /* \"imagee\": */ \"features\": { \"a b\": {}, /* c */ },\n \"imge\": \"x\", // end\n}",
        );

        let found = published(&host, FILE);

        assert_eq!(codes(&found), ["features-malformed", "unknown-property"]);
        assert_eq!(found[1].range.start.line, 3);
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let host = host();
        open(&host, FILE, "{\"name\": \"é😀\", \"imge\": 1}");

        let found = published(&host, FILE);

        assert_eq!(found[0].range.start.character, 16);
        assert_eq!(found[0].range.end.character, 22);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let host = host();
        open(&host, FILE, "{\r\n  \"image\": \"x\",\r\n  \"imge\": 1\r\n}\r\n");

        let found = published(&host, FILE);

        assert_eq!(found[0].range.start.line, 2);
        assert_eq!(found[0].range.start.character, 2);
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = host();
        open(&host, ".devcontainer.json", "{\"imge\": 1}");
        assert_eq!(host.diagnostics().len(), 1);

        open(&host, ".devcontainer.json", "{\"image\": \"x\"}");
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn oversized_file_publishes_empty_list() {
        let host = host();
        open(&host, FILE, &format!("{{\"imge\": \"{}\"}}", "x".repeat(MAX_BYTES)));

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn non_devcontainer_json_is_ignored() {
        let host = host();
        open(&host, "package.json", "{\"imge\": 1}");
        open(&host, "other/devcontainer.json", "{\"imge\": 1}");

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn nested_folder_config_is_checked() {
        let host = host();
        open(&host, ".devcontainer/python/devcontainer.json", "{\"imge\": 1}");

        assert_eq!(host.diagnostics().len(), 1);
    }

    #[test]
    fn each_group_can_be_switched_off() {
        let host = host();
        host.set_setting("checkProperties", "false");
        open(&host, FILE, "{\"imge\": 1, \"features\": 3}");
        assert_eq!(codes(&published(&host, FILE)), ["features-malformed"]);

        host.set_setting("checkFeatures", "false");
        open(&host, FILE, "{\"imge\": 1, \"features\": 3}");
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn denied_events_log_a_warning_and_still_activate() {
        let host = MockHost::install().with_capabilities(&["project.read"]);

        assert!(Main.activate(&host.context("aritti.devcontainer")).is_ok());
        assert!(host.logs().iter().any(|(level, _)| *level == LogLevel::Warn));
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
    const CONFIGURATION: &str = r#"npm notice hello
{"configuration": {"image": "mcr.microsoft.com/devcontainers/base:ubuntu", "remoteUser": "vscode",
 "forwardPorts": [3000, 8080], "features": {"a": {}, "b": {}}, "postCreateCommand": "npm i"}, "workspace": {}}"#;

    fn info_host(result: Result<ProcessOutput, Error>) -> MockHost {
        let host = host();
        host.add_file(".devcontainer/devcontainer.json", "{}");
        host.on_process("devcontainer", move |args, _| {
            assert_eq!(args, ["read-configuration", "--workspace-folder", "/work/app", "--config", ".devcontainer/devcontainer.json"]);
            result.clone()
        });
        host
    }

    #[test]
    fn read_configuration_json_is_summarised_in_notification() {
        let host = info_host(ok(CONFIGURATION));

        Main.run_command("info", &host.invocation(), None).unwrap();

        let body = &host.notifications()[0].body;
        assert!(body.contains("Image: mcr.microsoft.com/devcontainers/base:ubuntu"), "{body}");
        assert!(body.contains("Remote user: vscode"));
        assert!(body.contains("Forwarded ports: 3000, 8080"));
        assert!(body.contains("Features: 2"));
        assert!(body.contains("postCreateCommand: yes"));
    }

    #[test]
    fn read_configuration_compose_file_is_summarised() {
        let host = info_host(ok(r#"{"configuration": {"dockerComposeFile": ["docker-compose.yml"], "service": "app"}}"#));

        Main.run_command("info", &host.invocation(), None).unwrap();

        let body = &host.notifications()[0].body;
        assert!(body.contains("Compose file: docker-compose.yml"), "{body}");
        assert!(body.contains("postCreateCommand: no"));
    }

    #[test]
    fn read_configuration_quiet_failures() {
        for result in [
            ok("not json"),
            Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "boom".into() }),
            Err(Error::NotFound("devcontainer".into())),
        ] {
            let host = info_host(result);

            assert!(Main.run_command("info", &host.invocation(), None).is_err());
            assert!(host.notifications().is_empty());
        }
    }

    #[test]
    fn info_without_cli_capability_is_denied() {
        let host = info_host(ok("{}")).with_capabilities(&["project.read"]);

        let error = Main.run_command("info", &host.invocation(), None).unwrap_err();

        assert!(matches!(error, Error::Denied(cap) if cap == "process:devcontainer"));
    }
}
