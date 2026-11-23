//! Native read-only runs. Writing operations must keep their fresh confirmation path.
use crate::*;
use openidea_plugin::{PluginRunConfiguration, RunConfigurations};
pub(crate) fn configurations(root: &str) -> Vec<PluginRunConfiguration> {
    if root.is_empty() || !root.starts_with('/') || root == "/" || root.chars().any(char::is_control)
        || root.contains('\\') || root.split('/').any(|p| matches!(p, "." | "..")) { return Vec::new(); }
    let Some(project) = host::project_of(root) else { return Vec::new(); };
    if project.root().ok().as_deref() != Some(root)
        || !host::projects().unwrap_or_default().iter().any(|p| p.root().ok().as_deref() == Some(root)) { return Vec::new(); }
    if !FILES.iter().any(|file| project.exists(file).unwrap_or(false)) { return Vec::new(); }
    [
        ("status", vec!["status".to_owned(), "--verbose".to_owned()]),
        ("validate", vec!["validate".to_owned()]),
        ("update-sql", vec!["update-sql".to_owned()]),
    ].into_iter().map(|(goal, args)| {
        PluginRunConfiguration { id: format!("native-{goal}"), name: format!("Liquibase: {goal}"), program: "liquibase".into(), args,
            cwd: Some(root.into()), test: false, framework: None }
    }).collect()
}
const FILES: &[&str] = &["liquibase.properties"];
impl RunConfigurations for Liquibase {
    fn configurations(&mut self, project_root: &str) -> Vec<PluginRunConfiguration> {
        let runs = configurations(project_root);
        if !runs.is_empty() && !std::mem::replace(&mut self.live.console_rules, true) {
            crate::console::apply();
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
        host.add_file(FILES[0], "");
        let runs = configurations("/project");
        assert!(!runs.is_empty());
        assert!(runs.iter().all(|r| r.cwd.as_deref() == Some("/project") && !r.test && r.framework.is_none()));
        assert!(runs.iter().all(|r| !r.args.iter().any(|a| matches!(a.as_str(), "migrate" | "update" | "clean" | "repair" | "rollback-count"))));
        assert!(configurations("/other").is_empty());
        assert!(configurations("/project/../project").is_empty());
        host.set_project_root(None);
        assert!(configurations("/project").is_empty());
    }
}
