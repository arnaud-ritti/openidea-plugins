//! Dev Container CLI: drives the project's development container with the
//! `devcontainer` CLI (https://containers.dev).

mod checks;
mod dashboard;
mod features;

use std::path::Path;

use openidea_plugin::providers::{CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{
    Context as PluginContext, Diagnostic, Error, Event, EventKind, Invocation, LogLevel, NotificationLevel, PickItem, Plugin, Position, Project, Range, host, register_plugin,
};

use checks::{Context, FEATURES_MALFORMED, UNKNOWN_PROPERTY};

const MAX_BYTES: usize = 256 * 1024;

/// Where the CLI looks for a configuration, in order. The host's `find_files`
/// skips hidden directories, so these are probed with `read_file`; configs in
/// `.devcontainer/<folder>/` are picked with the `configFile` setting.
const DEFAULT_CONFIGS: [&str; 2] = [".devcontainer/devcontainer.json", ".devcontainer.json"];

const LOGIN_SHELL: &str = "command -v bash >/dev/null && exec bash -l; exec sh -l";

#[derive(Default)]
pub struct Main {
    /// Container id picked in the Dev Containers view.
    selected: Option<String>,
    running: std::collections::BTreeSet<String>,
}

impl Plugin for Main {
    fn activate(&mut self, _ctx: &PluginContext) -> Result<(), Error> {
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

        if id == "refresh-features" {
            return features::refresh();
        }

        if id == "clone" {
            return clone(&project, &root);
        }

        if id == "rebuild" && !host::confirm("Rebuild Dev Container", "The existing container is removed and built again.").unwrap_or(true) {
            return Ok(None);
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

    matches!(path.rsplit('/').next(), Some("devcontainer.json" | ".devcontainer.json"))
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
        false => enabled_findings(path, &text),
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

fn enabled_findings(path: &str, text: &str) -> Vec<checks::Finding> {
    let properties = host::setting_bool("checkProperties").unwrap_or(true);
    let features = host::setting_bool("checkFeatures").unwrap_or(true);

    if !checks::in_valid_location(path) {
        let hint = checks::folder_structure(path).filter(|_| host::setting_bool("checkFolderStructure").unwrap_or(true));

        let token = checks::first_token(text);

        return hint.map(|hint| checks::Finding { start: token.start, end: token.end, ..hint }).into_iter().collect();
    }

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

    let mut found = Vec::new();
    for path in DEFAULT_CONFIGS {
        match project.read_file(path) {
            Ok(_) => found.push(path.to_owned()),
            Err(Error::Denied(cap)) => return Err(Error::Denied(cap)),
            Err(_) => continue,
        }
    }

    if found.len() > 1 {
        let items: Vec<PickItem> = found.iter().map(|path| PickItem::new(path, path)).collect();
        if let Ok(Some(chosen)) = host::pick("Dev Container configuration", &items) {
            return Ok(chosen);
        }
    }

    if let Some(path) = found.into_iter().next() {
        return Ok(path);
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

fn repository_name(url: &str) -> &str {
    let name = url.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or("");

    name.strip_suffix(".git").unwrap_or(name)
}

fn clone(project: &Project, root: &str) -> Result<Option<String>, Error> {
    let Some(url) = host::input("Repository URL to clone", None)? else { return Ok(None) };
    let url = url.trim();
    let name = repository_name(url);
    if url.starts_with('-') || url.contains(char::is_whitespace) || name.is_empty() {
        host::notify(NotificationLevel::Warning, "Dev Container", &format!("\"{url}\" is not a repository URL."));
        return Ok(None);
    }

    let parent = Path::new(root).parent().map_or(root.to_owned(), |p| p.to_string_lossy().into_owned());
    let suggested = format!("{}/{name}", parent.trim_end_matches('/'));
    let Some(target) = host::input("Clone into (a new folder)", Some(&suggested))? else { return Ok(None) };
    let target = target.trim();
    if target.is_empty() {
        return Ok(None);
    }

    let cloned = {
        let _task = host::begin_task("Cloning the repository", false);
        project.run_process("git", &["clone".into(), "--".into(), url.into(), target.into()], None)?
    };
    if cloned.status != Some(0) {
        return Err(format!("git clone failed: {}", cloned.stderr.trim()).into());
    }

    let args = ["up".to_owned(), "--workspace-folder".to_owned(), target.to_owned()];
    host::open_terminal("Dev Container: Start Cloned Container", "devcontainer", &args)?;

    Ok(None)
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

impl Providers for Main {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !is_devcontainer_file(&doc.path) || doc.text.len() > MAX_BYTES || !host::setting_bool("completeProperties").unwrap_or(true) {
            return vec![];
        }

        let Some(offset) = offset(&doc.text, at) else { return vec![] };
        if let Some(key) = checks::feature_key_at(&doc.text, offset) {
            return feature_items(&doc.text, key);
        }
        let Some(context) = checks::context_at(&doc.text, offset) else { return vec![] };
        let replace = checks::word_range(&doc.text, offset);
        let replace = Range::new(position(&doc.text, replace.start), position(&doc.text, replace.end));

        let mut items = match context {
            Context::Key => checks::missing_properties(&doc.text, offset)
                .into_iter()
                .map(|name| {
                    let mut item = CompletionItem::new(name, CompletionKind::Property);
                    item.insert_text = format!("\"{name}\": $0");
                    item.snippet = true;
                    item
                })
                .collect(),
            Context::Value { key } => checks::value_choices(&key)
                .iter()
                .map(|(value, quoted)| {
                    let mut item = CompletionItem::new(value, CompletionKind::EnumMember);
                    if *quoted {
                        item.insert_text = format!("\"{value}\"");
                    }
                    item
                })
                .collect::<Vec<_>>(),
        };

        for item in &mut items {
            item.replace = Some(replace);
        }

        items
    }
}

fn feature_items(text: &str, key: std::ops::Range<usize>) -> Vec<CompletionItem> {
    let has_value = text[key.end..].trim_start().starts_with(':');
    let replace = Range::new(position(text, key.start), position(text, key.end));

    features::cached()
        .into_iter()
        .map(|(reference, name)| {
            let mut item = CompletionItem::new(&reference, CompletionKind::Module);
            item.detail = Some(name).filter(|n| !n.is_empty());
            item.insert_text = if has_value { format!("\"{reference}\"") } else { format!("\"{reference}\": {{$0}}") };
            item.snippet = !has_value;
            item.filter_text = Some(format!("\"{reference}\""));
            item.replace = Some(replace);
            item
        })
        .collect()
}

register_plugin!(Main, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::{Body, ViewEvent, Views};

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

        Main::default().run_command("up", &host.invocation(), None).unwrap();

        let terminal = &host.terminals()[0];
        assert_eq!(terminal.title, "Dev Container: Start Container");
        assert_eq!(terminal.program, "devcontainer");
        assert_eq!(terminal.args, ["up", "--workspace-folder", "/work/app", "--config", ".devcontainer/devcontainer.json"]);
    }

    #[test]
    fn rebuild_falls_back_to_the_root_config() {
        let host = host();
        host.add_file(".devcontainer.json", "{}");
        host.answer_next_prompt(Some(&[""]));

        Main::default().run_command("rebuild", &host.invocation(), None).unwrap();

        assert_eq!(
            host.terminals()[0].args,
            ["up", "--workspace-folder", "/work/app", "--config", ".devcontainer.json", "--remove-existing-container"]
        );
    }

    #[test]
    fn declining_the_rebuild_confirm_starts_nothing() {
        let host = host();
        host.add_file(".devcontainer.json", "{}");
        host.answer_next_prompt(None);

        Main::default().run_command("rebuild", &host.invocation(), None).unwrap();

        assert!(host.terminals().is_empty());
    }

    #[test]
    fn several_default_configs_let_the_user_pick() {
        let host = host();
        host.add_file(".devcontainer/devcontainer.json", "{}");
        host.add_file(".devcontainer.json", "{}");
        host.answer_next_prompt(Some(&[".devcontainer.json"]));

        Main::default().run_command("up", &host.invocation(), None).unwrap();

        assert_eq!(host.terminals()[0].args[4], ".devcontainer.json");
    }

    #[test]
    fn shell_uses_the_configured_file() {
        let host = host();
        host.set_setting("configFile", "\".devcontainer/python/devcontainer.json\"");

        Main::default().run_command("shell", &host.invocation(), None).unwrap();

        assert_eq!(
            host.terminals()[0].args,
            ["exec", "--workspace-folder", "/work/app", "--config", ".devcontainer/python/devcontainer.json", "sh", "-c", LOGIN_SHELL]
        );
    }

    #[test]
    fn up_without_config_fails() {
        let host = host();

        let error = Main::default().run_command("up", &host.invocation(), None).unwrap_err();

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

        Main::default().run_command("stop", &host.invocation(), None).unwrap();

        assert_eq!(host.notifications()[0].body, "Stopped 2 containers.");
    }

    #[test]
    fn clone_clones_next_to_the_project_then_starts_the_container() {
        let host = host().with_capabilities(&["project.read", "process:devcontainer", "process:git"]);
        host.on_process("git", |args, _| {
            assert_eq!(args, ["clone", "--", "https://example.com/acme/tool.git", "/work/tool"]);
            ok("")
        });

        host.answer_next_prompt(Some(&["https://example.com/acme/tool.git"]));
        host.answer_next_prompt(Some(&["/work/tool"]));
        Main::default().run_command("clone", &host.invocation(), None).unwrap();

        assert_eq!(host.terminals()[0].args, ["up", "--workspace-folder", "/work/tool"]);
    }

    #[test]
    fn clone_stops_on_dismissal_bad_url_or_failed_git() {
        let host = host().with_capabilities(&["project.read", "process:devcontainer", "process:git"]);
        host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "fatal: nope".into() }));

        host.answer_next_prompt(None);
        Main::default().run_command("clone", &host.invocation(), None).unwrap();
        host.answer_next_prompt(Some(&["--upload-pack=x"]));
        Main::default().run_command("clone", &host.invocation(), None).unwrap();
        host.answer_next_prompt(Some(&["https://example.com/acme/tool.git"]));
        host.answer_next_prompt(None);
        Main::default().run_command("clone", &host.invocation(), None).unwrap();
        assert!(host.process_runs().is_empty() && host.terminals().is_empty());
        assert_eq!(host.notifications().len(), 1);

        host.answer_next_prompt(Some(&["https://example.com/acme/tool.git"]));
        host.answer_next_prompt(Some(&["/work/tool"]));
        let error = Main::default().run_command("clone", &host.invocation(), None).unwrap_err();
        assert!(error.to_string().contains("fatal: nope"));
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn stop_without_docker_is_denied() {
        let host = host().with_capabilities(&["project.read", "process:devcontainer"]);

        let error = Main::default().run_command("stop", &host.invocation(), None).unwrap_err();

        assert!(matches!(error, Error::Denied(cap) if cap == "process:docker"));
    }

    fn strip_line_comments(text: &str) -> String {
        text.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
    }

    fn open(host: &MockHost, path: &str, text: &str) {
        host.add_file(path, text);
        Main::default().handle_events(&[Event::FileOpened(format!("/work/app/{path}"))], &host.invocation());
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

        open(&host, "devcontainer.json", "\r\n// x\r\n{}");
        assert_eq!(published(&host, "devcontainer.json")[0].range.start.line, 2);
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
    fn manifest_activates_on_file_events() {
        let manifest = include_str!("../plugin.toml");

        assert!(manifest.contains("activation = [\"on-event:file-opened\", \"on-event:file-saved\", \"on-view:containers\"]"));
    }

    #[test]
    fn utf8_bom_is_skipped() {
        let host = host();
        open(&host, FILE, "\u{feff}{\"imge\": 1}");

        assert_eq!(codes(&published(&host, FILE)), ["unknown-property"]);
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

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn misplaced_config_gets_only_the_structure_hint() {
        let host = host();
        open(&host, "other/devcontainer.json", "{\"imge\": 1}");

        assert_eq!(codes(&published(&host, "other/devcontainer.json")), ["folder-structure"]);
    }

    #[test]
    fn devcontainer_json_outside_a_devcontainer_folder_gets_a_hint() {
        let host = host();
        open(&host, "devcontainer.json", "// c\n{}");

        let found = published(&host, "devcontainer.json");

        assert_eq!(codes(&found), ["folder-structure"]);
        assert_eq!(found[0].severity, openidea_plugin::Severity::Info);
        assert_eq!(found[0].range, Range::new(openidea_plugin::Position::new(1, 0), openidea_plugin::Position::new(1, 1)));
    }

    #[test]
    fn config_two_levels_below_devcontainer_gets_a_hint() {
        let host = host();
        open(&host, ".devcontainer/a/b/devcontainer.json", "{}");

        assert_eq!(codes(&published(&host, ".devcontainer/a/b/devcontainer.json")), ["folder-structure"]);
    }

    #[test]
    fn config_one_folder_below_devcontainer_is_quiet() {
        let host = host();
        open(&host, ".devcontainer/python/devcontainer.json", "{}");

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn root_config_files_are_quiet() {
        let host = host();
        open(&host, ".devcontainer.json", "{}");
        open(&host, "sub/.devcontainer.json", "{}");

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn windows_separators_are_normalised() {
        assert!(checks::folder_structure("C:\\work\\.devcontainer\\devcontainer.json").is_none());
        assert!(checks::folder_structure("C:\\work\\devcontainer.json").is_some());
    }

    #[test]
    fn folder_structure_is_quiet_for_library_or_dynamic_name() {
        assert!(checks::folder_structure("/work/app/devcontainer.json.bak").is_none());
        assert!(checks::folder_structure("/work/app/my-devcontainer.json").is_none());
        assert!(!is_devcontainer_file("/work/app/my-devcontainer.json"));
    }

    #[test]
    fn moving_the_file_clears_the_hint() {
        let host = host();
        open(&host, "devcontainer.json", "{}");
        assert_eq!(host.diagnostics().len(), 1);

        open(&host, ".devcontainer/devcontainer.json", "{}");
        assert!(published(&host, ".devcontainer/devcontainer.json").is_empty());
    }

    #[test]
    fn folder_structure_can_be_switched_off() {
        let host = host();
        host.set_setting("checkFolderStructure", "false");
        open(&host, "devcontainer.json", "{}");

        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn structure_hint_ignores_the_bom() {
        let host = host();
        open(&host, "devcontainer.json", "\u{feff}{}");

        assert_eq!(published(&host, "devcontainer.json")[0].range.start.line, 0);
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

        assert!(Main::default().activate(&host.context("aritti.devcontainer")).is_ok());
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

        Main::default().run_command("info", &host.invocation(), None).unwrap();

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

        Main::default().run_command("info", &host.invocation(), None).unwrap();

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

            assert!(Main::default().run_command("info", &host.invocation(), None).is_err());
            assert!(host.notifications().is_empty());
        }
    }

    #[test]
    fn info_without_cli_capability_is_denied() {
        let host = info_host(ok("{}")).with_capabilities(&["project.read"]);

        let error = Main::default().run_command("info", &host.invocation(), None).unwrap_err();

        assert!(matches!(error, Error::Denied(cap) if cap == "process:devcontainer"));
    }

    fn complete_in(path: &str, marked: &str) -> Vec<openidea_plugin::providers::CompletionItem> {
        let text = marked.replacen('|', "", 1);
        let caret = marked.find('|').unwrap();
        let doc = openidea_plugin::providers::Document { path: path.into(), language: "jsonc".into(), version: 1, text: text.clone() };

        Main::default().completion(&doc, position(&text, caret), None)
    }

    fn complete(marked: &str) -> Vec<openidea_plugin::providers::CompletionItem> {
        complete_in("/work/app/.devcontainer/devcontainer.json", marked)
    }

    fn labels(items: &[openidea_plugin::providers::CompletionItem]) -> Vec<&str> {
        items.iter().map(|i| i.label.as_str()).collect()
    }

    #[test]
    fn suggests_missing_top_level_properties() {
        let _host = host();

        let items = complete("{\n  \"image\": \"x\",\n  |\n}");

        assert!(labels(&items).contains(&"remoteUser"));
        assert!(!labels(&items).contains(&"image"));
        let item = items.iter().find(|i| i.label == "remoteUser").unwrap();
        assert_eq!(item.insert_text, "\"remoteUser\": $0");
        assert!(item.snippet);
    }

    #[test]
    fn suggests_enum_values_for_shutdown_action() {
        let _host = host();

        let items = complete("{\"shutdownAction\": \"|\"}");

        assert_eq!(labels(&items), ["none", "stopContainer", "stopCompose"]);
        assert_eq!(items[0].insert_text, "\"none\"");
        assert!(!items[0].snippet);
    }

    #[test]
    fn suggests_booleans_for_privileged() {
        let _host = host();

        assert_eq!(labels(&complete("{\"privileged\": |}")), ["true", "false"]);
        assert_eq!(labels(&complete("{\"privileged\": tr|,\n\"image\": 1}")), ["true", "false"]);
    }

    #[test]
    fn completion_is_quiet_inside_nested_objects_and_arrays() {
        let _host = host();

        for marked in [
            "{\"features\": {|}}",
            "{\"customizations\": {\"vscode\": {\"x\": |}}}",
            "{\"forwardPorts\": [|]}",
            "{\"forwardPorts\": [3000, \"|\"]}",
            "{\"image\": \"x\"}|",
            "|{\"image\": \"x\"}",
        ] {
            assert!(complete(marked).is_empty(), "{marked}");
        }
    }

    #[test]
    fn completion_is_quiet_inside_comments_and_for_other_files() {
        let _host = host();

        assert!(complete("{\n  // rem|\n}").is_empty());
        assert!(complete("{\n  /* rem| */\n}").is_empty());
        assert!(complete("{\"privileged\": true, // x|\n}").is_empty());
        assert!(!complete("{\n  // c\n  |\n}").is_empty());

        for path in ["/work/app/package.json", "/work/app/tsconfig.json", "/work/app/.vscode/settings.json"] {
            assert!(complete_in(path, "{|}").is_empty(), "{path}");
        }
    }

    #[test]
    fn completion_works_in_misplaced_files() {
        let _host = host();

        assert!(!complete_in("/work/app/devcontainer.json", "{|}").is_empty());
        assert!(!complete_in("/work/app/.devcontainer.json", "{|}").is_empty());
    }

    #[test]
    fn completion_survives_an_unterminated_string_at_the_caret() {
        let _host = host();

        let items = complete("{\"image\": \"x\",\n  \"remo|");
        assert!(labels(&items).contains(&"remoteUser"));
        let replace = items[0].replace.clone().unwrap();
        assert_eq!((replace.start.line, replace.start.character, replace.end.character), (1, 2, 7));

        assert_eq!(labels(&complete("{\"waitFor\": \"|")).len(), 6);
    }

    #[test]
    fn completion_reads_jsonc_comments_and_trailing_commas() {
        let _host = host();

        let items = complete("{\n  // \"name\": 1,\n  /* \"init\": */ \"image\": \"x\", // end\n  |\n}");

        assert!(labels(&items).contains(&"name"));
        assert!(labels(&items).contains(&"init"));
        assert!(!labels(&items).contains(&"image"));
        assert_eq!(labels(&complete("{\"init\": |, }")), ["true", "false"]);
    }

    #[test]
    fn completion_ranges_use_utf16_columns_with_multibyte_text() {
        let _host = host();

        let items = complete("{\"name\": \"é😀\", \"pri|\": 1}");

        let replace = items.iter().find(|i| i.label == "privileged").unwrap().replace.clone().unwrap();
        assert_eq!((replace.start.character, replace.end.character), (16, 21));
    }

    #[test]
    fn completion_can_be_switched_off() {
        let host = host();
        host.set_setting("completeProperties", "false");

        assert!(complete("{|}").is_empty());
    }

    #[test]
    fn completion_is_fast_on_a_256_kib_file() {
        let _host = host();

        let oversized = format!("{{\"name\": \"{}\", |}}", "x".repeat(MAX_BYTES));
        assert!(complete(&oversized).is_empty());

        let near_limit = format!("{{\"name\": \"{}\", |}}", "x".repeat(MAX_BYTES - 100));
        assert!(!complete(&near_limit).is_empty());
    }

    #[test]
    fn completion_at_the_end_of_unfinished_text_does_not_panic() {
        let _host = host();

        for marked in ["{\n|", "{ |", "{\"a\": 1,\n|", "{\"a\": |", "{\"a\":\n|"] {
            let _ = complete(marked);
        }

        assert!(!complete("{\n|").is_empty());
        assert!(!complete("{ |").is_empty());
        assert!(!complete("{\"image\": 1,\n|").is_empty());
    }

    #[test]
    fn structure_hint_ranges_use_utf16_columns_with_multibyte_first_character() {
        let host = host();
        open(&host, "devcontainer.json", "/* é😀 */ 日{}");

        let found = published(&host, "devcontainer.json");

        assert_eq!(found[0].range.start.character, 10);
        assert_eq!(found[0].range.end.character, 11);

        open(&host, "other/devcontainer.json", "é{}");
        assert_eq!(published(&host, "other/devcontainer.json")[0].range.end.character, 1);
    }

    #[test]
    fn manifest_registers_the_completion_provider() {
        let manifest = include_str!("../plugin.toml");

        assert!(manifest.contains("kinds = [\"completion\"]"));
        assert!(manifest.contains("languages = [\"json\", \"jsonc\"]"));
    }

    const INDEX: &str = r#"{"collections": [{"features": [
        {"id": "ghcr.io/devcontainers/features/node", "name": "Node.js", "majorVersion": "1"},
        {"id": "ghcr.io/x/features/plain", "name": "Plain"},
        {"name": "No id"}
    ]}]}"#;

    fn feature_labels(marked: &str) -> Vec<String> {
        complete(marked).into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn refresh_stores_the_index_and_completion_reads_it() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "net:containers.dev"]);
        host.on_fetch(|r| {
            assert_eq!(r.url, features::INDEX_URL);
            Ok(openidea_plugin::HttpResponse { status: 200, headers: vec![], body: INDEX.as_bytes().to_vec() })
        });
        Main::default().run_command("refresh-features", &host.invocation(), None).unwrap();

        let items = complete("{\n  \"features\": {\n    \"ghcr|\n  }\n}");
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["ghcr.io/devcontainers/features/node:1", "ghcr.io/x/features/plain"]);
        assert_eq!(items[0].insert_text, "\"ghcr.io/devcontainers/features/node:1\": {$0}");
        assert_eq!(items[0].detail.as_deref(), Some("Node.js"));

        let after_comma = "{\"features\": {\"a/b\": {}, \"|\": {}}}";
        assert_eq!(feature_labels(after_comma).len(), 2);
        let existing = complete("{\"features\": {\"gh|cr\": {}}}");
        assert_eq!(existing[0].insert_text, "\"ghcr.io/devcontainers/features/node:1\"");
    }

    #[test]
    fn feature_ids_are_quiet_outside_feature_keys() {
        let _host = MockHost::install().with_capabilities(&["project.read", "storage"]);
        host::storage::set("feature-index", "ghcr.io/a/b:1\tB\n").unwrap();
        assert_eq!(feature_labels("{\"features\": {\"|\": {}}}"), ["ghcr.io/a/b:1"]);
        assert!(feature_labels("{\"features\": {\"ghcr.io/a/b:1\": {\"version\": \"|\"}}}").is_empty());
        assert!(feature_labels("{\"features\": {\"ghcr.io/a/b:1\": {\"x\": 1, \"|\": 2}}}").is_empty());
        assert!(feature_labels("{\"image\": \"|\"}").is_empty());
        assert!(feature_labels("{\"features\": [\"|\"]}").is_empty());
    }

    #[test]
    fn empty_cache_and_denied_storage_offer_nothing() {
        let _host = MockHost::install().with_capabilities(&["project.read"]);
        assert!(feature_labels("{\"features\": {\"|\": {}}}").is_empty());
    }

    #[test]
    fn refresh_reports_a_bad_status_and_an_odd_index() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "net:containers.dev"]);
        host.on_fetch(|_| Ok(openidea_plugin::HttpResponse { status: 503, headers: vec![], body: vec![] }));
        assert!(Main::default().run_command("refresh-features", &host.invocation(), None).is_err());
        host.on_fetch(|_| Ok(openidea_plugin::HttpResponse { status: 200, headers: vec![], body: b"[]".to_vec() }));
        assert!(Main::default().run_command("refresh-features", &host.invocation(), None).is_err());
        assert!(host.storage().is_empty());
    }

    fn docker_host(output: &'static str) -> MockHost {
        let host = host();
        host.on_process("docker", move |args, _| match args[0].as_str() {
            "ps" => {
                assert_eq!(args[1..4], ["-a", "--filter", "label=devcontainer.local_folder=/work/app"]);
                ok(output)
            }
            _ => ok(""),
        });
        host
    }

    const PS: &str = "{\"ID\":\"abc123\",\"Image\":\"node:20\",\"Names\":\"happy_fox\",\"Status\":\"Up 2 minutes\"}\nnot json\n{\"Names\":\"no id\"}\n";

    #[test]
    fn parses_docker_ps_json_lines() {
        let containers = dashboard::parse_containers(PS);
        assert_eq!(containers.len(), 1);
        assert_eq!((containers[0].id.as_str(), containers[0].name.as_str(), containers[0].image.as_str()), ("abc123", "happy_fox", "node:20"));
    }

    #[test]
    fn view_lists_containers_and_acts_on_the_selection() {
        let host = docker_host(PS);
        let mut plugin = Main::default();
        let view = plugin.render("containers", None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("a list") };
        assert_eq!(items[0].key, "abc123");
        assert_eq!(items[0].detail.as_deref(), Some("node:20 - Up 2 minutes"));
        assert!(!view.toolbar[1].enabled);

        plugin.handle("containers", ViewEvent::Select("abc123".into()), &host.invocation()).unwrap();
        assert!(plugin.render("containers", None, &host.invocation()).unwrap().toolbar[2].enabled);
        plugin.handle("containers", ViewEvent::Button("shell".into()), &host.invocation()).unwrap();
        assert_eq!(host.terminals()[0].args, ["exec", "-it", "abc123", "sh", "-c", LOGIN_SHELL]);

        plugin.handle("containers", ViewEvent::Button("stop".into()), &host.invocation()).unwrap();
        assert!(host.process_runs().iter().any(|run| run.args == ["stop", "abc123"]));
        assert!(plugin.selected.is_none());
    }

    const PS_ALL: &str = "{\"ID\":\"abc123\",\"Image\":\"node:20\",\"Names\":\"up\",\"State\":\"running\",\"Status\":\"Up 2 minutes\"}\n{\"ID\":\"def456\",\"Image\":\"node:18\",\"Names\":\"old\",\"State\":\"exited\",\"Status\":\"Exited (0) 3 days ago\"}\n";

    #[test]
    fn view_lists_stopped_containers_and_starts_them() {
        let host = docker_host(PS_ALL);
        let mut plugin = Main::default();
        let view = plugin.render("containers", None, &host.invocation()).unwrap();
        let Body::List(items) = &view.body else { panic!("a list") };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].detail.as_deref(), Some("node:18 - Exited (0) 3 days ago"));
        assert_eq!(view.status.as_deref(), Some("1 running, 1 stopped"));

        plugin.handle("containers", ViewEvent::Select("def456".into()), &host.invocation()).unwrap();
        let toolbar = plugin.render("containers", None, &host.invocation()).unwrap().toolbar;
        assert!(!toolbar[1].enabled && !toolbar[2].enabled && toolbar[3].enabled);

        plugin.handle("containers", ViewEvent::Button("start".into()), &host.invocation()).unwrap();
        assert!(host.process_runs().iter().any(|run| run.args == ["start", "def456"]));

        plugin.handle("containers", ViewEvent::Activate("abc123".into()), &host.invocation()).unwrap();
        assert_eq!(host.terminals()[0].args[2], "abc123");
    }

    #[test]
    fn view_explains_empty_denied_and_failed_listings() {
        let host = docker_host("");
        let message = |host: &MockHost| match Main::default().render("containers", None, &host.invocation()).unwrap().body {
            Body::Message(text) => text,
            _ => panic!("a message"),
        };
        assert!(message(&host).starts_with("No dev container"));

        let denied = MockHost::install().with_capabilities(&["project.read"]);
        assert!(message(&denied).contains("docker permission"));

        let failing = host_failing();
        assert!(message(&failing).starts_with("Cannot list containers"));
    }

    fn host_failing() -> MockHost {
        let host = host();
        host.on_process("docker", |_, _| Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "daemon down".into() }));
        host
    }

    #[test]
    fn stop_button_without_a_selection_does_nothing() {
        let host = docker_host(PS);
        Main::default().handle("containers", ViewEvent::Button("stop".into()), &host.invocation()).unwrap();
        assert!(host.process_runs().is_empty());
    }
}
