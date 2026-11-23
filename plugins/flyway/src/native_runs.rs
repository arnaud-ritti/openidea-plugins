//! Native read-only runs. Writing operations must keep their fresh confirmation path.
use crate::*;
use openidea_plugin::{PluginRunConfiguration, RunConfigurations};
pub(crate) fn configurations(root: &str) -> Vec<PluginRunConfiguration> {
    if root.is_empty() || !root.starts_with('/') || root == "/" || root.chars().any(char::is_control)
        || root.contains('\\') || root.split('/').any(|p| matches!(p, "." | "..")) { return Vec::new(); }
    let Some(project) = host::project_of(root) else { return Vec::new(); };
    if project.root().ok().as_deref() != Some(root)
        || !host::projects().unwrap_or_default().iter().any(|p| p.root().ok().as_deref() == Some(root)) { return Vec::new(); }
    if !config::CONFIG_FILES.iter().any(|file| project.exists(file).unwrap_or(false)) { return Vec::new(); }
    if !host::setting_string("arguments").unwrap_or_default().trim().is_empty() { return Vec::new(); }
    let program = if host::setting_string("runner").as_deref() == Some("mvn") { "mvn" } else { "flyway" };

    [        ("info", vec!["info".to_owned()]),
        ("validate", vec!["validate".to_owned()]),
    ].into_iter().map(|(goal, mut args)| {
        if program == "mvn" { args[0] = format!("flyway:{goal}"); }
        PluginRunConfiguration { id: format!("native-{goal}"), name: format!("Flyway: {goal}"), program: program.into(), args,
            cwd: Some(root.into()), test: false, framework: None }
    }).collect()
}
impl RunConfigurations for Flyway {
    fn configurations(&mut self, project_root: &str) -> Vec<PluginRunConfiguration> {
        let runs = configurations(project_root);
        if !runs.is_empty() {
            self.uses_flyway();
        }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn runs_require_owned_project_and_observed_configuration() {
        let host = MockHost::install();
        assert!(configurations("/project").is_empty());
        host.add_file(config::CONFIG_FILES[0], "");
        let runs = configurations("/project");
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| r.cwd.as_deref() == Some("/project") && !r.test && r.framework.is_none()));
        assert!(runs.iter().all(|r| !r.args.iter().any(|a| matches!(a.as_str(), "migrate" | "update" | "clean" | "repair" | "rollback-count"))));
        assert!(configurations("/other").is_empty());
        assert!(configurations("/project/../project").is_empty());
        host.set_project_root(None);
        assert!(configurations("/project").is_empty());
    }
    #[test]
    fn runner_selection_and_custom_argument_withdrawal() {
        let host = MockHost::install();
        host.add_file("flyway.toml", "");
        host.set_setting("runner", "\"mvn\"");
        let runs = configurations("/project");
        assert_eq!(runs.len(), 2);
        assert!(runs.iter().all(|r| r.program == "mvn" && r.args[0].starts_with("flyway:")));
        host.set_setting("arguments", "\"-password=private\"");
        assert!(configurations("/project").is_empty());
    }
}
