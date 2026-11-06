//! Manifest inspections. Each group is one function over the scanned
//! documents; a file with Helm actions (`{{ ... }}`) is only read by the Helm
//! group, because its keys and values are not final YAML.

use openidea_plugin::text::position;
use openidea_plugin::{Diagnostic, Severity};

use crate::manifest::{Doc, documents};
use crate::schema;

pub const DEPRECATED_API_VERSION: &str = "deprecated-api-version";
pub const DUPLICATE_ENV_NAME: &str = "duplicate-env-name";
pub const MISSING_NAME: &str = "missing-name";
pub const MISSING_SELECTOR: &str = "missing-selector";
pub const HELM_WHITESPACE_TRIM: &str = "helm-whitespace-trim";
pub const CHART_MISSING_KEY: &str = "chart-missing-key";

/// Setting (a boolean, default on) that switches each code off.
pub const SETTINGS: &[(&str, &str)] = &[
    (DEPRECATED_API_VERSION, "checkDeprecatedApiVersions"),
    (DUPLICATE_ENV_NAME, "checkDuplicateEnvNames"),
    (MISSING_NAME, "checkMissingName"),
    (MISSING_SELECTOR, "checkMissingSelector"),
    (HELM_WHITESPACE_TRIM, "checkHelmWhitespaceTrim"),
    (CHART_MISSING_KEY, "checkChartKeys"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
}

impl Finding {
    fn new(start: usize, end: usize, code: &'static str, message: impl Into<String>) -> Self {
        Self { start, end, code, message: message.into(), severity: Severity::Warning }
    }

    pub fn diagnostic(&self, text: &str) -> Diagnostic {
        Diagnostic {
            code: Some(self.code.into()),
            ..Diagnostic::new(
                openidea_plugin::Range::new(position(text, self.start), position(text, self.end)),
                self.severity,
                &self.message,
            )
        }
    }
}

pub fn is_helm_template(text: &str) -> bool {
    text.contains("{{")
}

/// Everything that applies to the file at `path`; `in_chart` says it sits in a chart's `templates/`.
pub fn for_file(path: &str, text: &str, in_chart: bool, patch_context: bool) -> Vec<Finding> {
    if path.rsplit('/').next() == Some("Chart.yaml") {
        return chart_keys(text);
    }
    let patch_context = patch_context || path.rsplit('/').next().is_some_and(|name| name.contains("patch"));
    let mut found = if in_chart { helm_whitespace_trim(text) } else { Vec::new() };
    if (path.ends_with(".yaml") || path.ends_with(".yml")) && text.contains("apiVersion") {
        found.extend(check_in(&documents(text), text, patch_context));
    }
    found
}

/// The manifest groups, for a file without Helm actions.
#[cfg(test)]
pub fn check(docs: &[Doc], text: &str) -> Vec<Finding> {
    check_in(docs, text, false)
}

/// `patch_context`: the file is a patch or sits next to a kustomization, so partial workloads are expected.
fn check_in(docs: &[Doc], text: &str, patch_context: bool) -> Vec<Finding> {
    if is_helm_template(text) {
        return Vec::new();
    }
    let manifests: Vec<&Doc> = docs.iter().filter(|doc| doc.is_manifest()).collect();
    let mut found = Vec::new();
    for doc in manifests {
        found.extend(deprecated_api_version(doc));
        found.extend(duplicate_env_names(doc));
        found.extend(missing_name(doc));
        if !patch_context {
            found.extend(missing_selector(doc));
        }
    }
    found
}

/// Removed or deprecated versions: api version, kinds, replacement, release that removed it.
const DEPRECATED: &[(&str, &[&str], &str, &str)] = &[
    ("extensions/v1beta1", &["Deployment", "DaemonSet", "ReplicaSet"], "apps/v1", "v1.16"),
    ("extensions/v1beta1", &["Ingress"], "networking.k8s.io/v1", "v1.22"),
    ("extensions/v1beta1", &["NetworkPolicy"], "networking.k8s.io/v1", "v1.16"),
    ("apps/v1beta1", &["Deployment", "StatefulSet", "ControllerRevision"], "apps/v1", "v1.16"),
    ("apps/v1beta2", &["Deployment", "StatefulSet", "DaemonSet", "ReplicaSet", "ControllerRevision"], "apps/v1", "v1.16"),
    ("batch/v1beta1", &["CronJob"], "batch/v1", "v1.25"),
    ("networking.k8s.io/v1beta1", &["Ingress", "IngressClass"], "networking.k8s.io/v1", "v1.22"),
    ("policy/v1beta1", &["PodDisruptionBudget"], "policy/v1", "v1.25"),
    (
        "rbac.authorization.k8s.io/v1beta1",
        &["Role", "RoleBinding", "ClusterRole", "ClusterRoleBinding"],
        "rbac.authorization.k8s.io/v1",
        "v1.22",
    ),
    (
        "admissionregistration.k8s.io/v1beta1",
        &["ValidatingWebhookConfiguration", "MutatingWebhookConfiguration"],
        "admissionregistration.k8s.io/v1",
        "v1.22",
    ),
    ("apiextensions.k8s.io/v1beta1", &["CustomResourceDefinition"], "apiextensions.k8s.io/v1", "v1.22"),
];

fn deprecated_api_version(doc: &Doc) -> Option<Finding> {
    let (api_version, kind) = (doc.api_version.as_ref()?, doc.kind.as_ref()?);
    let message = if kind.text == "PodSecurityPolicy" && api_version.text == "extensions/v1beta1" {
        "extensions/v1beta1 PodSecurityPolicy was removed in Kubernetes v1.16; its replacement apiVersion policy/v1beta1 was itself removed in v1.25, so use Pod Security Admission."
            .to_owned()
    } else if kind.text == "PodSecurityPolicy" && api_version.text == "policy/v1beta1" {
        "policy/v1beta1 PodSecurityPolicy was removed in Kubernetes v1.25; use Pod Security Admission.".to_owned()
    } else {
        let (_, _, replacement, removed) = DEPRECATED
            .iter()
            .find(|(version, kinds, ..)| *version == api_version.text && kinds.contains(&kind.text.as_str()))?;
        format!("{} {} was removed in Kubernetes {removed}; use apiVersion {replacement}.", api_version.text, kind.text)
    };
    Some(Finding::new(api_version.start, api_version.end, DEPRECATED_API_VERSION, message))
}

fn duplicate_env_names(doc: &Doc) -> Vec<Finding> {
    let mut found = Vec::new();
    for names in &doc.env_lists {
        for (i, name) in names.iter().enumerate() {
            if names[..i].iter().any(|earlier| earlier.text == name.text) {
                let message = format!("Environment variable `{}` is already defined in this env list.", name.text);
                found.push(Finding::new(name.start, name.end, DUPLICATE_ENV_NAME, message));
            }
        }
    }
    found
}

fn is_built_in(doc: &Doc) -> bool {
    let Some(api_version) = &doc.api_version else { return false };
    let group = api_version.text.rsplit_once('/').map_or("", |(group, _)| group);
    schema::GROUPS.contains(&group)
}

fn missing_name(doc: &Doc) -> Option<Finding> {
    let kind = doc.kind.as_ref()?;
    let exempt = kind.text.ends_with("List") || kind.text.ends_with("Review") || kind.text == "Config";
    if !is_built_in(doc) || exempt || doc.name_unknown || doc.name.is_some() || doc.has_generate_name {
        return None;
    }
    let message = format!("{} has no metadata.name (or metadata.generateName).", kind.text);
    Some(Finding::new(kind.start, kind.end, MISSING_NAME, message))
}

fn missing_selector(doc: &Doc) -> Option<Finding> {
    let (api_version, kind) = (doc.api_version.as_ref()?, doc.kind.as_ref()?);
    let selects = ["Deployment", "StatefulSet", "DaemonSet", "ReplicaSet"].contains(&kind.text.as_str());
    if api_version.text != "apps/v1" || !selects || doc.has_selector || !doc.has_template {
        return None;
    }
    let message = format!("apps/v1 {} requires spec.selector.", kind.text);
    Some(Finding::new(kind.start, kind.end, MISSING_SELECTOR, message))
}

/// `Chart.yaml` needs `apiVersion`, `name` and `version`.
pub fn chart_keys(text: &str) -> Vec<Finding> {
    let top_keys: Vec<String> = documents(text).into_iter().flat_map(|doc| doc.top_keys).collect();
    let first_line_end = text.find('\n').unwrap_or(text.len());
    ["apiVersion", "name", "version"]
        .into_iter()
        .filter(|key| !top_keys.iter().any(|present| present == key))
        .map(|key| Finding::new(0, first_line_end, CHART_MISSING_KEY, format!("Chart.yaml is missing the required `{key}` key.")))
        .collect()
}

/// A trim marker needs a space beside it: `{{-foo` is read as a number or fails, `foo-}}` fails.
pub fn helm_whitespace_trim(text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let mut action_start = 0;
        while let Some(open) = line[action_start..].find("{{").map(|i| i + action_start) {
            let close = line[open..].find("}}").map(|i| i + open);
            let body_end = close.unwrap_or(line.len());
            let body = &line[open + 2..body_end];
            if body.starts_with('-') && body[1..].starts_with(|c: char| c.is_ascii_alphabetic() || matches!(c, '.' | '$' | '(')) {
                let message = "Write `{{- ` with a space after the dash; `{{-` followed by text is not a trim marker.";
                found.push(Finding::new(line_start + open, line_start + open + 3, HELM_WHITESPACE_TRIM, message));
            }
            if let Some(close) = close {
                let before = &line[..close];
                let dash_glued = before.ends_with('-')
                    && before[..before.len() - 1].ends_with(|c: char| c.is_alphanumeric() || matches!(c, ')' | '"'));
                if dash_glued {
                    let message = "Write ` -}}` with a space before the dash; `-}}` after text is not a trim marker.";
                    found.push(Finding::new(line_start + close - 1, line_start + close + 2, HELM_WHITESPACE_TRIM, message));
                }
            }
            action_start = close.map_or(line.len(), |close| close + 2);
        }
        line_start += line.len();
    }
    found
}

#[cfg(test)]
mod tests {
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Event, LogLevel, Plugin};

    use super::*;
    use crate::KubeExplorer;

    fn codes(text: &str) -> Vec<&'static str> {
        check(&documents(text), text).iter().map(|f| f.code).collect()
    }

    fn highlighted<'a>(text: &'a str, code: &str) -> Vec<&'a str> {
        check(&documents(text), text).iter().filter(|f| f.code == code).map(|f| &text[f.start..f.end]).collect()
    }

    const INGRESS: &str = "apiVersion: extensions/v1beta1\nkind: Ingress\nmetadata:\n  name: web\n";

    #[test]
    fn non_kubernetes_yaml_is_ignored() {
        for text in [
            "services:\n  web:\n    image: nginx\n    environment:\n      - A=1\n",
            "stages: [build]\nbuild:\n  script:\n    - make\n",
            "openapi: 3.0.0\ninfo:\n  title: x\n",
            "apiVersion: v1\nname: only-one-key\n",
            "kind: Deployment\nspec: {}\n",
        ] {
            assert!(codes(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn deprecated_ingress_api_version_warns() {
        assert_eq!(codes(INGRESS), [DEPRECATED_API_VERSION]);
        assert_eq!(highlighted(INGRESS, DEPRECATED_API_VERSION), ["extensions/v1beta1"]);
        let message = &check(&documents(INGRESS), INGRESS)[0].message;
        assert!(message.contains("v1.22") && message.contains("networking.k8s.io/v1"), "{message}");
    }

    #[test]
    fn deprecated_table_names_each_replacement() {
        let cronjob = "apiVersion: batch/v1beta1\nkind: CronJob\nmetadata:\n  name: a\n";
        let psp = "apiVersion: policy/v1beta1\nkind: PodSecurityPolicy\nmetadata:\n  name: a\n";
        let pdb = "apiVersion: policy/v1beta1\nkind: PodDisruptionBudget\nmetadata:\n  name: a\n";
        let role = "apiVersion: rbac.authorization.k8s.io/v1beta1\nkind: Role\nmetadata:\n  name: a\n";
        assert!(check(&documents(cronjob), cronjob)[0].message.contains("batch/v1"));
        assert!(check(&documents(psp), psp)[0].message.contains("Pod Security Admission"));
        assert!(check(&documents(pdb), pdb)[0].message.contains("policy/v1"));
        assert_eq!(codes(role), [DEPRECATED_API_VERSION]);
    }

    #[test]
    fn deprecated_api_version_is_quiet_for_library_or_dynamic_name() {
        for text in [
            "apiVersion: batch/v1\nkind: CronJob\nmetadata:\n  name: a\n",
            "apiVersion: example.com/v1beta1\nkind: Ingress\nmetadata:\n  name: a\n",
            "apiVersion: policy/v1beta1\nkind: Widget\nmetadata:\n  name: a\n",
            "apiVersion: {{ .Values.api }}\nkind: Ingress\nmetadata:\n  name: a\n",
        ] {
            assert!(codes(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn extensions_pod_security_policy_was_removed_in_1_16() {
        let text = "apiVersion: extensions/v1beta1\nkind: PodSecurityPolicy\nmetadata:\n  name: a\n";
        let message = &check(&documents(text), text)[0].message;
        assert!(message.contains("v1.16") && message.contains("v1.25") && message.contains("Pod Security Admission"), "{message}");
    }

    #[test]
    fn patch_without_template_is_quiet() {
        let text = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  replicas: 3\n";
        assert!(codes(text).is_empty());
    }

    #[test]
    fn kustomize_directory_patch_is_quiet() {
        let full = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  template:\n    spec: {}\n";
        assert_eq!(for_file("/p/dep.yaml", full, false, false).len(), 1);
        assert!(for_file("/p/dep.yaml", full, false, true).is_empty());
        assert!(for_file("/p/web-patch.yaml", full, false, false).is_empty());

        let host = project(&[("k/kustomization.yaml", "resources: []\n"), ("k/dep.yaml", full), ("j/dep.yaml", full)]);
        publish(&host, Event::FileOpened("/p/k/dep.yaml".into()));
        publish(&host, Event::FileOpened("/p/j/dep.yaml".into()));
        assert_eq!(host.diagnostics().keys().collect::<Vec<_>>(), ["/p/j/dep.yaml"]);
    }

    #[test]
    fn kustomization_yaml_is_quiet() {
        assert!(codes("apiVersion: kustomize.config.k8s.io/v1beta1\nkind: Kustomization\nresources:\n  - a.yaml\n").is_empty());
    }

    #[test]
    fn init_container_duplicate_env_warns() {
        let text = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\nspec:\n  initContainers:\n    - name: i\n      env:\n        - name: X\n          value: a\n        - name: X\n          value: b\n";
        assert_eq!(codes(text), [DUPLICATE_ENV_NAME]);
    }

    #[test]
    fn huge_yaml_is_skipped_and_clears_stale_diagnostics() {
        let host = project(&[("ing.yaml", INGRESS)]);
        publish(&host, Event::FileOpened("/p/ing.yaml".into()));
        assert_eq!(host.diagnostics().len(), 1);

        host.add_file("ing.yaml", &format!("{INGRESS}# {}\n", "x".repeat(crate::MAX_BYTES)));
        publish(&host, Event::FileSaved("/p/ing.yaml".into()));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn non_manifest_yaml_is_not_scanned() {
        let host = project(&[("data.yaml", "key: value\nlist:\n  - a\n")]);
        let before = crate::manifest::SCANS.with(|s| s.get());
        publish(&host, Event::FileOpened("/p/data.yaml".into()));
        assert_eq!(crate::manifest::SCANS.with(|s| s.get()), before);
        assert!(host.diagnostics().is_empty());
    }

    const POD: &str = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\nspec:\n  containers:\n    - name: a\n      env:\n        - name: A\n          value: \"1\"\n        - name: B\n          value: \"2\"\n        - name: A\n          valueFrom:\n            secretKeyRef: {name: s, key: k}\n    - name: b\n      env:\n        - name: A\n          value: \"3\"\n";

    #[test]
    fn duplicate_env_name_in_container_warns() {
        assert_eq!(codes(POD), [DUPLICATE_ENV_NAME]);
        assert_eq!(highlighted(POD, DUPLICATE_ENV_NAME), ["A"]);
        let second = POD.match_indices("name: A").nth(1).unwrap().0 + 6;
        assert_eq!(check(&documents(POD), POD)[0].start, second);
    }

    #[test]
    fn duplicate_env_name_is_quiet_for_library_or_dynamic_name() {
        let other_containers = POD.replace("        - name: A\n          valueFrom:\n            secretKeyRef: {name: s, key: k}\n", "");
        let script = "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: c\ndata:\n  run.sh: |\n    env:\n    - name: A\n    - name: A\n";
        let flow = "apiVersion: v1\nkind: Pod\nmetadata:\n  name: p\nspec:\n  containers:\n  - name: a\n    env: [{name: A}, {name: A}]\n";
        let templated = POD.replace("name: p", "name: {{ .Release.Name }}");
        for text in [other_containers.as_str(), script, flow, templated.as_str()] {
            assert!(!codes(text).contains(&DUPLICATE_ENV_NAME), "{text}");
        }
    }

    #[test]
    fn deployment_without_selector_warns() {
        let text = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  replicas: 2\n  template:\n    metadata:\n      labels: {a: b}\n";
        assert_eq!(codes(text), [MISSING_SELECTOR]);
        assert_eq!(highlighted(text, MISSING_SELECTOR), ["Deployment"]);
    }

    #[test]
    fn missing_selector_is_quiet_for_library_or_dynamic_name() {
        let with = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  selector:\n    matchLabels: {a: b}\n";
        let crd = "apiVersion: example.com/v1\nkind: Deployment\nmetadata:\n  name: web\nspec:\n  replicas: 1\n";
        let job = "apiVersion: batch/v1\nkind: Job\nmetadata:\n  name: j\nspec:\n  template: {}\n";
        let inline = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\nspec: {selector: {}}\n";
        for text in [with, crd, job, inline] {
            assert!(codes(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn missing_name_warns_for_built_in_kinds() {
        let text = "apiVersion: v1\nkind: ConfigMap\ndata:\n  a: b\n";
        assert_eq!(codes(text), [MISSING_NAME]);
        assert_eq!(highlighted(text, MISSING_NAME), ["ConfigMap"]);
        assert_eq!(codes("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  labels: {a: b}\n"), [MISSING_NAME]);
    }

    #[test]
    fn missing_name_is_quiet_for_library_or_dynamic_name() {
        for text in [
            "apiVersion: v1\nkind: List\nitems: []\n",
            "apiVersion: v1\nkind: ConfigMapList\nitems: []\n",
            "apiVersion: example.com/v1\nkind: Widget\nspec: {}\n",
            "apiVersion: v1\nkind: Pod\nmetadata:\n  generateName: job-\n",
            "apiVersion: v1\nkind: Pod\nmetadata: {name: x}\n",
            "apiVersion: v1\nkind: Config\nclusters: []\n",
            "apiVersion: v1\nkind: Pod\nmetadata:\n  name: {{ .Values.name }}\n",
        ] {
            assert!(codes(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn multi_document_file_reports_per_document() {
        let text = format!("{INGRESS}---\napiVersion: v1\nkind: Service\nspec: {{}}\n---\n# nothing\n---\n{POD}");
        let found = check(&documents(&text), &text);
        let kinds: Vec<_> = found.iter().map(|f| f.code).collect();
        assert_eq!(kinds, [DEPRECATED_API_VERSION, MISSING_NAME, DUPLICATE_ENV_NAME]);
        assert_eq!(documents(&text).iter().filter(|d| d.is_manifest()).count(), 3);
    }

    #[test]
    fn chart_yaml_missing_version_warns() {
        let found = for_file("/p/chart/Chart.yaml", "apiVersion: v2\nname: web\n", false, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, CHART_MISSING_KEY);
        assert!(found[0].message.contains("`version`"));
    }

    #[test]
    fn chart_missing_key_is_quiet_for_library_or_dynamic_name() {
        let complete = "apiVersion: v2\nname: web\nversion: 0.1.0\ndependencies:\n  - name: redis\n    version: 1.0.0\n";
        assert!(for_file("/p/chart/Chart.yaml", complete, false, false).is_empty());
        assert!(for_file("/p/chart/values.yaml", "replicas: 1\n", false, false).is_empty());
    }

    #[test]
    fn helm_trim_marker_without_space_warns() {
        let text = "metadata:\n  name: é{{-.Values.name }}\n  x: {{ .Values.x-}}\n";
        let found = helm_whitespace_trim(text);
        assert_eq!(found.len(), 2);
        assert_eq!(&text[found[0].start..found[0].end], "{{-");
        assert_eq!(&text[found[1].start..found[1].end], "-}}");
        assert_eq!(found[0].diagnostic(text).range.start.character, 9);
    }

    #[test]
    fn helm_whitespace_trim_is_quiet_for_library_or_dynamic_name() {
        let text = "{{- if .Values.a }}\n{{-  if .Values.b  -}}\n{{ x -}}\n{{- 3 }}\n{{-3}}\nk: v-}}\n{{ \"a-\" }}\n{{- /* c */ -}}\n{{- end }}\n";
        assert!(helm_whitespace_trim(text).is_empty());
    }

    #[test]
    fn helm_template_is_only_read_by_the_helm_group() {
        let text = "{{-.Values.x }}\napiVersion: extensions/v1beta1\nkind: Ingress\n";
        assert!(for_file("/p/chart/templates/a.yaml", text, false, false).is_empty());
        let found = for_file("/p/chart/templates/a.yaml", text, true, false);
        assert_eq!(found.iter().map(|f| f.code).collect::<Vec<_>>(), [HELM_WHITESPACE_TRIM]);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let text = INGRESS.replace('\n', "\r\n");
        let found = &check(&documents(&text), &text)[0];
        let range = found.diagnostic(&text).range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (0, 12, 30));
        assert_eq!(&text[found.start..found.end], "extensions/v1beta1");
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let text = "apiVersion: batch/v1beta1\nkind: CronJob\nmetadata:\n  name: a\n  annotations: {note: \"é😀\"}\n---\n# 😀é\napiVersion: v1\nkind: ConfigMap\n";
        let found = check(&documents(text), text);
        let missing = found.iter().find(|f| f.code == MISSING_NAME).unwrap();
        let range = missing.diagnostic(text).range;
        assert_eq!((range.start.line, range.start.character, range.end.character), (8, 6, 15));
    }

    const PROJECT: &str = "/p";

    fn project(files: &[(&str, &str)]) -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage", "project.read", "events"]);
        host.set_project_root(Some(PROJECT));
        for (path, text) in files {
            host.add_file(path, text);
        }
        host
    }

    fn publish(host: &MockHost, event: Event) {
        KubeExplorer::default().handle_events(&[event], &host.invocation());
    }

    #[test]
    fn publishes_manifest_problems_on_open_and_save() {
        let host = project(&[("k8s/ing.yaml", INGRESS)]);
        publish(&host, Event::FileOpened("/p/k8s/ing.yaml".into()));

        let published = &host.diagnostics()["/p/k8s/ing.yaml"];
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].code.as_deref(), Some(DEPRECATED_API_VERSION));
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = project(&[("ing.yaml", INGRESS)]);
        publish(&host, Event::FileOpened("/p/ing.yaml".into()));
        assert_eq!(host.diagnostics().len(), 1);

        host.add_file("ing.yaml", &INGRESS.replace("extensions/v1beta1", "networking.k8s.io/v1"));
        publish(&host, Event::FileSaved("/p/ing.yaml".into()));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn missing_file_is_treated_as_absent() {
        let host = project(&[]);
        publish(&host, Event::FileOpened("/p/gone.yaml".into()));
        publish(&host, Event::FileOpened("/p/chart/templates/gone.yaml".into()));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn non_kubernetes_yaml_publishes_nothing() {
        let host = project(&[("docker-compose.yml", "services:\n  web:\n    image: nginx\n"), ("notes.txt", INGRESS)]);
        publish(&host, Event::FileOpened("/p/docker-compose.yml".into()));
        publish(&host, Event::FileOpened("/p/notes.txt".into()));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn each_group_can_be_switched_off() {
        let host = project(&[("ing.yaml", INGRESS)]);
        host.set_setting("checkDeprecatedApiVersions", "false");
        publish(&host, Event::FileOpened("/p/ing.yaml".into()));
        assert!(host.diagnostics().is_empty());
    }

    #[test]
    fn helm_group_needs_a_chart_above_templates() {
        let text = "a: {{-.Values.x }}\n";
        let host = project(&[("chart/Chart.yaml", "apiVersion: v2\nname: c\nversion: 1\n"), ("chart/templates/a.yaml", text), ("other/templates/a.yaml", text)]);
        publish(&host, Event::FileOpened("/p/chart/templates/a.yaml".into()));
        publish(&host, Event::FileOpened("/p/other/templates/a.yaml".into()));

        let diagnostics = host.diagnostics();
        assert_eq!(diagnostics.keys().collect::<Vec<_>>(), ["/p/chart/templates/a.yaml"]);
        assert_eq!(diagnostics["/p/chart/templates/a.yaml"][0].code.as_deref(), Some(HELM_WHITESPACE_TRIM));
    }

    #[test]
    fn denied_events_log_a_warning_and_still_activate() {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        assert!(KubeExplorer::default().activate(&host.context("aritti.kube-explorer")).is_ok());
        assert!(host.logs().iter().any(|(level, _)| *level == LogLevel::Warn));
    }
}
