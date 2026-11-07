//! Commands that ask the user with `pick`, `pick_many` and `input`; they only
//! work from a command, never from a provider.

use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, host};

use crate::{HELM, KUBECTL, KubeExplorer, REQUEST_TIMEOUT, DEFAULT_DEBUG_IMAGE, is_dns_label, is_dns_subdomain};

const MAX_RECENT_IMAGES: usize = 10;

fn stored_lines(key: &str) -> Vec<String> {
    let text = host::storage::get(key).ok().flatten().unwrap_or_default();
    text.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn store_lines(key: &str, lines: &[String]) {
    let _ = match lines.is_empty() {
        true => host::storage::delete(key),
        false => host::storage::set(key, &lines.join("\n")),
    };
}

fn stored_value(key: &str) -> Option<String> {
    host::storage::get(key).ok().flatten().filter(|value| !value.is_empty())
}

fn store_value(key: &str, value: &str) {
    let _ = match value.is_empty() {
        true => host::storage::delete(key),
        false => host::storage::set(key, value),
    };
}

fn ask(prompt: &str, current: Option<&str>) -> Result<Option<String>, Error> {
    Ok(host::input(prompt, current)?.map(|text| text.trim().to_owned()))
}

pub fn recent_debug_images() -> Vec<String> {
    stored_lines("debug-images")
}

pub fn helm_flags() -> Vec<String> {
    let mut flags = Vec::new();
    flags.extend(stored_value("helm-values").map(|file| format!("--values={file}")));
    flags.extend(stored_value("helm-set").map(|set| format!("--set={set}")));
    flags
}

pub fn helm_release() -> Option<String> {
    stored_value("helm-release")
}

fn refuse_option_like(what: &str, value: &str) -> Result<(), Error> {
    if value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("The {what} \"{value}\" starts with '-' or has control characters.").into());
    }
    Ok(())
}

impl KubeExplorer {
    pub fn helm_create(&self, ctx: &Invocation) -> Result<(), Error> {
        let project = ctx.project().ok_or("No project is open.")?;
        let Some(name) = ask("Chart name", None)? else { return Ok(()) };
        if !is_dns_label(&name) {
            return Err(format!("\"{name}\" is not a chart name: use lowercase letters, digits and '-'.").into());
        }
        let _task = host::begin_task(&format!("Creating chart {name}"), false);
        let out = project.run_process(HELM, &["create".to_owned(), name.clone()], None)?;
        if out.status != Some(0) {
            return Err(crate::failure(HELM, &out.stderr, out.status));
        }
        crate::config::notify_success("Helm chart created", &format!("{name}: {}", out.stdout.trim()));
        Ok(())
    }

    pub fn helm_parameters(&self) -> Result<(), Error> {
        let Some(release) = ask("Helm release name (empty: the chart folder's name)", helm_release().as_deref())? else {
            return Ok(());
        };
        let Some(values) = ask("Helm values file (empty: none)", stored_value("helm-values").as_deref())? else {
            return Ok(());
        };
        let Some(set) = ask("Helm --set overrides, e.g. image.tag=1.2,replicas=2 (empty: none)", stored_value("helm-set").as_deref())?
        else {
            return Ok(());
        };
        refuse_option_like("release name", &release)?;
        refuse_option_like("values file", &values)?;
        refuse_option_like("--set value", &set)?;
        store_value("helm-release", &release);
        store_value("helm-values", &values);
        store_value("helm-set", &set);
        host::notify(NotificationLevel::Info, "Helm parameters saved", "Preview Helm Template and Lint Helm Chart use them.");
        Ok(())
    }

    pub fn ask_debug_image(&self) -> Result<Option<String>, Error> {
        let setting = host::setting_string("debugImage").filter(|image| !image.trim().is_empty());
        let default = recent_debug_images()
            .into_iter()
            .next()
            .or(setting)
            .unwrap_or_else(|| DEFAULT_DEBUG_IMAGE.to_owned());
        Ok(ask("Debug image", Some(&default))?.filter(|image| !image.is_empty()))
    }

    pub fn remember_debug_image(&self, image: &str) {
        let mut images = recent_debug_images();
        images.retain(|recent| recent != image);
        images.insert(0, image.to_owned());
        images.truncate(MAX_RECENT_IMAGES);
        store_lines("debug-images", &images);
    }

    pub fn edit_debug_images(&self) -> Result<(), Error> {
        let images = recent_debug_images();
        if images.is_empty() {
            host::notify(NotificationLevel::Info, "Recent debug images", "No debug image has been used yet.");
            return Ok(());
        }
        let items: Vec<PickItem> = images.iter().map(|image| PickItem::new(image, image)).collect();
        let Some(removed) = host::pick_many("Remove recent debug images", &items)? else { return Ok(()) };
        let kept: Vec<String> = images.into_iter().filter(|image| !removed.contains(image)).collect();
        store_lines("debug-images", &kept);
        Ok(())
    }

    pub fn visible_namespaces(&self, namespaces: Vec<String>) -> Vec<String> {
        let chosen = stored_lines("visible-namespaces");
        if chosen.is_empty() {
            return namespaces;
        }
        let current = self.namespace.as_deref();
        namespaces.into_iter().filter(|name| chosen.contains(name) || Some(name.as_str()) == current).collect()
    }

    pub fn customize_namespaces(&mut self) -> Result<(), Error> {
        let out = self.kubectl(&["get", "namespaces", "-o", "name", REQUEST_TIMEOUT], None)?;
        let chosen = stored_lines("visible-namespaces");
        let items: Vec<PickItem> = out
            .lines()
            .filter_map(|line| line.strip_prefix("namespace/"))
            .map(|name| match chosen.iter().any(|c| c == name) {
                true => PickItem::new(name, name).description("shown"),
                false => PickItem::new(name, name),
            })
            .collect();
        let Some(picked) = host::pick_many("Namespaces to list (none picked: all)", &items)? else {
            return Ok(());
        };
        store_lines("visible-namespaces", &picked);
        self.reload();
        Ok(())
    }

    pub fn select_log_sources(&self, kind: &str, row: &str) -> Result<(), Error> {
        let (namespace, name) = self.split_scope(kind, row)?;
        let namespace = namespace.as_deref();
        if kind == "pods" {
            let jsonpath = ["get", "pod", &name, "-o", "jsonpath={.spec.containers[*].name}", REQUEST_TIMEOUT];
            let out = self.kubectl_in(namespace, &jsonpath, None)?;
            let items: Vec<PickItem> = out.split_whitespace().filter(|c| is_dns_label(c)).map(|c| PickItem::new(c, c)).collect();
            let Some(containers) = host::pick_many(&format!("Containers of {name}"), &items)? else { return Ok(()) };
            for container in containers {
                let args = ["logs", "-f", "--tail=500", &name, &format!("--container={container}")];
                host::open_terminal(&format!("Logs: {row}/{container}"), KUBECTL, &self.args_in(namespace, &args))?;
            }
            return Ok(());
        }
        let target = format!("{kind}/{name}");
        let template = "go-template={{range $k,$v := .spec.selector.matchLabels}}{{$k}}={{$v}},{{end}}";
        let selector = self.kubectl_in(namespace, &["get", &target, "-o", template, REQUEST_TIMEOUT], None)?;
        let selector = selector.trim().trim_end_matches(',');
        if selector.is_empty() {
            return Err(format!("{target} has no label selector to find its pods.").into());
        }
        let selector = format!("--selector={selector}");
        let pods = self.kubectl_in(namespace, &["get", "pods", "-o", "name", &selector, REQUEST_TIMEOUT], None)?;
        let items: Vec<PickItem> = pods
            .lines()
            .filter_map(|line| line.strip_prefix("pod/"))
            .filter(|pod| is_dns_subdomain(pod))
            .map(|pod| PickItem::new(pod, pod))
            .collect();
        let Some(chosen) = host::pick_many(&format!("Pods of {target}"), &items)? else { return Ok(()) };
        for pod in chosen {
            let args = ["logs", "-f", "--all-containers", "--tail=200", &pod];
            host::open_terminal(&format!("Logs: {pod}"), KUBECTL, &self.args_in(namespace, &args))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{Plugin, ProcessOutput};
    use openidea_plugin::mock::MockHost;

    fn ok(stdout: &str) -> Result<ProcessOutput, Error> {
        Ok(ProcessOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "process:helm", "storage", "project.read", "editor.read"]);
        host.on_process(KUBECTL, |args, _| {
            let has = |needle: &str| args.iter().any(|a| a.contains(needle));
            match () {
                _ if has("go-template") => ok("app=web,tier=api,"),
                _ if has("--selector") => ok("pod/web-1\npod/web-2\n"),
                _ if has("containers[*].name") => ok("app sidecar"),
                _ if has("namespaces") => ok("namespace/default\nnamespace/dev\nnamespace/prod\n"),
                _ => ok(""),
            }
        });
        host.on_process(HELM, |_, _| ok("Creating web\n"));
        host
    }

    fn command(host: &MockHost, kube: &mut KubeExplorer, id: &str, arg: Option<&str>) -> Result<Option<String>, Error> {
        kube.run_command(id, &host.invocation(), arg)
    }

    #[test]
    fn create_chart_runs_helm_create_with_the_typed_name() {
        let host = host();
        host.answer_next_prompt(Some(&["web"]));
        command(&host, &mut KubeExplorer::default(), "helm-create", None).unwrap();
        assert_eq!(host.process_runs()[0].args, ["create", "web"]);
    }

    #[test]
    fn create_chart_dismissed_or_invalid_runs_nothing() {
        let host = host();
        command(&host, &mut KubeExplorer::default(), "helm-create", None).unwrap();
        host.answer_next_prompt(Some(&["--bad"]));
        assert!(command(&host, &mut KubeExplorer::default(), "helm-create", None).is_err());
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn helm_parameters_are_stored_and_reach_template_and_lint() {
        let host = host();
        host.add_file("/project/charts/web/Chart.yaml", "name: web");
        host.open_editor("/project/charts/web/templates/svc.yaml", "yaml", "kind: Service", &[]);
        let mut kube = KubeExplorer::default();
        for answer in ["prod", "values-prod.yaml", "image.tag=2"] {
            host.answer_next_prompt(Some(&[answer]));
        }
        command(&host, &mut kube, "helm-parameters", None).unwrap();

        command(&host, &mut kube, "helm-template", None).unwrap();
        command(&host, &mut kube, "helm-lint", None).unwrap();

        let runs = host.process_runs();
        assert_eq!(
            runs[0].args,
            ["template", "prod", "/project/charts/web", "--values=values-prod.yaml", "--set=image.tag=2"]
        );
        assert_eq!(runs[1].args, ["lint", "/project/charts/web", "--values=values-prod.yaml", "--set=image.tag=2"]);
    }

    #[test]
    fn dismissing_helm_parameters_keeps_the_stored_ones() {
        let host = host();
        host.answer_next_prompt(Some(&["prod"]));
        host.answer_next_prompt(Some(&[""]));
        host.answer_next_prompt(Some(&[""]));
        command(&host, &mut KubeExplorer::default(), "helm-parameters", None).unwrap();
        host.answer_next_prompt(Some(&["other"]));
        command(&host, &mut KubeExplorer::default(), "helm-parameters", None).unwrap();
        assert_eq!(helm_release().as_deref(), Some("prod"));
    }

    fn debug_pod(host: &MockHost, answer: Option<&[&str]>) -> Vec<String> {
        host.answer_next_prompt(answer);
        let mut kube = KubeExplorer::default();
        command(host, &mut kube, "resource-debug", Some("res:pods/web-1")).unwrap();
        host.terminals().first().map(|t| t.args.clone()).unwrap_or_default()
    }

    #[test]
    fn debug_asks_for_the_image_and_remembers_it() {
        let host = host();
        host.set_setting("debugImage", "\"alpine:3\"");
        let args = debug_pod(&host, Some(&["nicolaka/netshoot"]));
        assert!(args.contains(&"--image=nicolaka/netshoot".to_owned()));
        assert_eq!(recent_debug_images(), ["nicolaka/netshoot"]);
    }

    #[test]
    fn debug_dismissed_opens_no_terminal() {
        let host = host();
        assert!(debug_pod(&host, None).is_empty());
        assert!(recent_debug_images().is_empty());
    }

    #[test]
    fn recent_debug_images_can_be_removed() {
        let host = host();
        let kube = KubeExplorer::default();
        kube.remember_debug_image("a:1");
        kube.remember_debug_image("b:1");
        host.answer_next_prompt(Some(&["a:1"]));
        command(&host, &mut KubeExplorer::default(), "edit-debug-images", None).unwrap();
        assert_eq!(recent_debug_images(), ["b:1"]);
        host.answer_next_prompt(None);
        command(&host, &mut KubeExplorer::default(), "edit-debug-images", None).unwrap();
        assert_eq!(recent_debug_images(), ["b:1"]);
    }

    #[test]
    fn chosen_namespaces_filter_the_namespace_list() {
        let host = host();
        let mut kube = KubeExplorer::default();
        host.answer_next_prompt(Some(&["dev", "prod"]));
        command(&host, &mut kube, "customize-namespaces", None).unwrap();
        assert_eq!(kube.load("ns").unwrap(), ["dev", "prod"]);

        host.answer_next_prompt(None);
        command(&host, &mut kube, "customize-namespaces", None).unwrap();
        assert_eq!(kube.load("ns").unwrap(), ["dev", "prod"]);

        host.answer_next_prompt(Some(&[]));
        command(&host, &mut kube, "customize-namespaces", None).unwrap();
        assert_eq!(kube.load("ns").unwrap().len(), 3);
    }

    #[test]
    fn log_sources_of_a_pod_open_one_terminal_per_container() {
        let host = host();
        host.answer_next_prompt(Some(&["app", "sidecar"]));
        command(&host, &mut KubeExplorer::default(), "resource-log-sources", Some("res:pods/web-1")).unwrap();
        let terminals = host.terminals();
        assert_eq!(terminals.len(), 2);
        assert!(terminals[1].args.contains(&"--container=sidecar".to_owned()));
    }

    #[test]
    fn log_sources_of_a_workload_follow_the_chosen_pods() {
        let host = host();
        host.answer_next_prompt(Some(&["web-2"]));
        command(&host, &mut KubeExplorer::default(), "resource-log-sources", Some("res:deployments/web")).unwrap();
        let terminals = host.terminals();
        assert_eq!(terminals.len(), 1);
        assert_eq!(terminals[0].args, ["logs", "-f", "--all-containers", "--tail=200", "web-2"]);
        assert!(host.process_runs().iter().any(|r| r.args.contains(&"--selector=app=web,tier=api".to_owned())));
    }

    #[test]
    fn log_sources_dismissed_opens_nothing() {
        let host = host();
        command(&host, &mut KubeExplorer::default(), "resource-log-sources", Some("res:pods/web-1")).unwrap();
        assert!(host.terminals().is_empty());
    }
}
