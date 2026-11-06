//! Settings that are lists or switches: the kubeconfig files, success
//! notifications, files that are not Kubernetes, and Telepresence.

use openidea_plugin::{Error, NotificationLevel, PickItem, host};

use crate::KubeExplorer;

const TELEPRESENCE: &str = "telepresence";
const CHOSEN_KUBECONFIG: &str = "kubeconfig-chosen";

fn setting_list(key: &str) -> Vec<String> {
    host::setting_string(key)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn kubeconfigs() -> Vec<String> {
    setting_list("kubeconfig")
}

/// One file, never a merge: the stored choice while it is still listed, else the first.
pub fn kubeconfig() -> Option<String> {
    let files = kubeconfigs();
    let chosen = host::storage::get(CHOSEN_KUBECONFIG).ok().flatten();
    chosen.filter(|file| files.contains(file)).or_else(|| files.into_iter().next())
}

pub fn notify_success(title: &str, body: &str) {
    if host::setting_bool("notifySuccess") == Some(false) {
        return;
    }
    host::notify(NotificationLevel::Info, title, body);
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            let (rest, crosses_slash) = match rest.split_first() {
                Some((b'*', deeper)) => (deeper, true),
                _ => (rest, false),
            };
            let rest = if crosses_slash { rest.strip_prefix(b"/").unwrap_or(rest) } else { rest };
            (0..=text.len())
                .take_while(|&at| crosses_slash || !text[..at].contains(&b'/'))
                .any(|at| glob_match(rest, &text[at..]))
        }
        Some((b'?', rest)) => text.split_first().is_some_and(|(c, tail)| *c != b'/' && glob_match(rest, tail)),
        Some((c, rest)) => text.split_first().is_some_and(|(t, tail)| t == c && glob_match(rest, tail)),
    }
}

/// A pattern with a slash matches the end of the path, one without matches the file name.
fn matches_glob(pattern: &str, path: &str) -> bool {
    let target = match pattern.contains('/') {
        true => path.trim_start_matches('/'),
        false => path.rsplit('/').next().unwrap_or(path),
    };
    let pattern = pattern.trim_start_matches('/');
    let bytes = target.as_bytes();
    let starts = std::iter::once(0).chain(bytes.iter().enumerate().filter(|(_, c)| **c == b'/').map(|(i, _)| i + 1));
    match pattern.contains('/') {
        true => starts.into_iter().any(|at| glob_match(pattern.as_bytes(), &bytes[at..])),
        false => glob_match(pattern.as_bytes(), bytes),
    }
}

pub fn is_non_kubernetes(path: &str) -> bool {
    setting_list("nonKubernetesGlobs").iter().any(|pattern| matches_glob(pattern, path))
}

impl KubeExplorer {
    pub fn choose_kubeconfig(&mut self) -> Result<(), Error> {
        let files = kubeconfigs();
        if files.len() < 2 {
            host::notify(
                NotificationLevel::Info,
                "Choose Kubeconfig",
                "List two or more files, separated by commas, in the kubeconfig setting.",
            );
            return Ok(());
        }
        let current = kubeconfig();
        let items: Vec<PickItem> = files
            .iter()
            .map(|file| match current.as_ref() == Some(file) {
                true => PickItem::new(file, file).description("in use"),
                false => PickItem::new(file, file),
            })
            .collect();
        let Some(file) = host::pick("Kubeconfig file", &items)? else { return Ok(()) };
        host::storage::set(CHOSEN_KUBECONFIG, &file)?;
        self.context = None;
        self.namespace = None;
        let _ = host::storage::delete("context");
        let _ = host::storage::delete("namespace");
        self.reload();
        Ok(())
    }

    pub fn telepresence(&self, subcommand: &str) -> Result<(), Error> {
        let args = vec![subcommand.to_owned()];
        match host::open_terminal(&format!("Telepresence {subcommand}"), TELEPRESENCE, &args) {
            Err(Error::Denied(_)) => {
                host::notify(
                    NotificationLevel::Info,
                    "Telepresence",
                    "Allow the optional process:telepresence capability to run telepresence from here.",
                );
                Ok(())
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Plugin, ProcessOutput};

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "process:telepresence", "storage", "editor.read"]);
        host.on_process(crate::KUBECTL, |_, _| Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() }));
        host
    }

    fn run(host: &MockHost, id: &str) -> Result<Option<String>, Error> {
        KubeExplorer::default().run_command(id, &host.invocation(), None)
    }

    #[test]
    fn glob_matches_names_and_paths() {
        assert!(matches_glob("*.tpl.yaml", "/p/a/b.tpl.yaml"));
        assert!(matches_glob("charts/**/*.yaml", "/p/charts/x/y/z.yaml"));
        assert!(matches_glob("ci/*.yml", "/p/ci/build.yml"));
        assert!(!matches_glob("ci/*.yml", "/p/ci/sub/build.yml"));
        assert!(!matches_glob("*.tpl.yaml", "/p/a/b.yaml"));
        assert!(matches_glob("docker-compose?.yml", "/p/docker-compose2.yml"));
    }

    #[test]
    fn globs_come_from_a_comma_list() {
        let host = host();
        host.set_setting("nonKubernetesGlobs", "\"*.compose.yaml, ci/*.yml\"");
        assert!(is_non_kubernetes("/p/a.compose.yaml"));
        assert!(is_non_kubernetes("/p/ci/x.yml"));
        assert!(!is_non_kubernetes("/p/k8s/x.yml"));
        host.set_setting("nonKubernetesGlobs", "\"\"");
        assert!(!is_non_kubernetes("/p/a.compose.yaml"));
    }

    #[test]
    fn the_first_listed_kubeconfig_is_used_until_one_is_chosen() {
        let host = host();
        host.set_setting("kubeconfig", "\"/a/config, /b/config\"");
        let kube = KubeExplorer::default();
        assert_eq!(kube.args(&["get"])[0], "--kubeconfig=/a/config");

        host.answer_next_prompt(Some(&["/b/config"]));
        run(&host, "choose-kubeconfig").unwrap();
        assert_eq!(kube.args(&["get"])[0], "--kubeconfig=/b/config");

        host.set_setting("kubeconfig", "\"/a/config\"");
        assert_eq!(kube.args(&["get"])[0], "--kubeconfig=/a/config");
    }

    #[test]
    fn dismissing_the_kubeconfig_chooser_keeps_the_current_one() {
        let host = host();
        host.set_setting("kubeconfig", "\"/a/config,/b/config\"");
        run(&host, "choose-kubeconfig").unwrap();
        assert_eq!(kubeconfig().as_deref(), Some("/a/config"));
    }

    #[test]
    fn success_notifications_follow_the_setting() {
        let host = host();
        notify_success("Done", "x");
        assert_eq!(host.notifications().len(), 1);
        host.set_setting("notifySuccess", "false");
        notify_success("Done", "x");
        assert_eq!(host.notifications().len(), 1);
    }

    #[test]
    fn telepresence_opens_a_terminal_and_a_denied_process_only_notifies() {
        let host = host();
        run(&host, "telepresence-connect").unwrap();
        run(&host, "telepresence-quit").unwrap();
        let terminals = host.terminals();
        assert_eq!(terminals[0].program, "telepresence");
        assert_eq!(terminals[1].args, ["quit"]);

        let denied = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        assert!(run(&denied, "telepresence-connect").is_ok());
        assert!(denied.terminals().is_empty());
        assert_eq!(denied.notifications().len(), 1);
    }
}
