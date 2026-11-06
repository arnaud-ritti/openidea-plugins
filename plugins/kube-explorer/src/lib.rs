//! Kube Explorer for OpenIDEA: a tool window that browses a cluster with
//! `kubectl` (contexts, namespaces, workloads, config, storage, nodes) and
//! shows `describe` / diff output in place, YAML in an editor that applies on
//! save, terminals for logs, shells, attach and port forwards, commands for
//! the open manifest (apply, dry-run, diff, delete), Kustomize and Helm.
//!
//! Every cluster call is a `kubectl` run with the chosen context and
//! namespace as flags, so the user's kubeconfig is never changed.

mod checks;
mod convert;
mod manifest;
mod schema;
mod secrets;

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, RandomState};
use std::path::Path;

use openidea_plugin::providers::{CodeAction, Document, Providers};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Error, Event, EventKind, Invocation, LogLevel, NotificationLevel, Plugin, Range, host, register_plugin,
};

pub const TREE: &str = "kube";
const KUBECTL: &str = "kubectl";
/// Writes the YAML to edit: the plugin API has no file writes.
const TEE: &str = "tee";
const HELM: &str = "helm";
const DEFAULT_DEBUG_IMAGE: &str = "busybox:stable";
/// Keeps reads under the 5 s call budget on an unreachable cluster.
const REQUEST_TIMEOUT: &str = "--request-timeout=4s";
/// Shows `describe`, YAML and diff output, highlighted; YAML can be edited.
const PAGE: &str = include_str!("page.html");
/// Shown in the YAML panel when the IDE blocks the page's script, which Apply and Save to File need.
const NO_SCRIPTS: &str = "<p id=\"nojs\">Apply and Save to File don't work in this panel: this version of the IDE blocks scripts in plugin pages. \
    To edit, allow tee for Kube Explorer in Settings &rsaquo; Plugins. The YAML then opens in an editor tab, and saving the file applies it.</p>";

/// A Kubernetes icon of the package (a `fileIcons` entry of `plugin.toml`).
macro_rules! k8s {
    ($id:literal) => {
        concat!("ext:fileIcon/aritti.kube-explorer/", $id)
    };
}

/// `kubectl get` resource, tree label and icon.
const KINDS: &[(&str, &str, &str)] = &[
    ("pods", "Pods", k8s!("pod")),
    ("deployments", "Deployments", k8s!("deploy")),
    ("statefulsets", "Stateful Sets", k8s!("sts")),
    ("daemonsets", "Daemon Sets", k8s!("ds")),
    ("jobs", "Jobs", k8s!("job")),
    ("cronjobs", "Cron Jobs", k8s!("cronjob")),
    ("services", "Services", k8s!("svc")),
    ("ingresses", "Ingresses", k8s!("ing")),
    ("configmaps", "Config Maps", k8s!("cm")),
    ("secrets", "Secrets", k8s!("secret")),
    ("persistentvolumeclaims", "Persistent Volume Claims", k8s!("pvc")),
    ("events", "Events", "bell"),
    ("nodes", "Nodes", k8s!("node")),
    ("persistentvolumes", "Persistent Volumes", k8s!("pv")),
];

/// Exact API groups Kubernetes itself serves. Others, even `*.k8s.io` ones
/// like `gateway.networking.k8s.io`, are custom resources.
const BUILT_IN_GROUPS: &[&str] = &[
    "apps", "batch", "autoscaling", "policy",
    "networking.k8s.io", "events.k8s.io", "storage.k8s.io", "rbac.authorization.k8s.io",
    "coordination.k8s.io", "discovery.k8s.io", "apiextensions.k8s.io",
    // Served by the metrics-server add-on; read-only node and pod metrics, not user-managed.
    "metrics.k8s.io",
    "scheduling.k8s.io", "node.k8s.io", "certificates.k8s.io", "admissionregistration.k8s.io",
    "flowcontrol.apiserver.k8s.io", "apiregistration.k8s.io", "internal.apiserver.k8s.io",
    "resource.k8s.io", "storagemigration.k8s.io", "authentication.k8s.io", "authorization.k8s.io",
];

/// Kinds whose pods `kubectl logs --all-pods` follows.
const WORKLOADS: &[&str] = &["deployments", "statefulsets", "daemonsets", "jobs"];
/// Kinds `kubectl port-forward` takes, and where their ports are.
const FORWARDABLE: &[(&str, &str)] = &[
    ("pods", "{.spec.containers[*].ports[*].containerPort}"),
    ("services", "{.spec.ports[*].port}"),
    ("deployments", "{.spec.template.spec.containers[*].ports[*].containerPort}"),
    ("statefulsets", "{.spec.template.spec.containers[*].ports[*].containerPort}"),
];

#[derive(Default)]
pub struct KubeExplorer {
    /// `None`: the kubeconfig's current context / the context's namespace.
    context: Option<String>,
    namespace: Option<String>,
    current_context: Option<String>,
    /// Children of the tree's expandable keys (`ctx`, `ns`, `kind:<kind>`).
    children: BTreeMap<String, Result<Vec<String>, String>>,
    expanded: BTreeSet<String>,
    /// Kind and name of the selected resource.
    selected: Option<(String, String)>,
    confirm_delete: bool,
    /// Shown instead of the tree.
    details: Option<Details>,
    /// Port forward being set up (shown instead of the tree): target and ports.
    forward: Option<(String, String)>,
    /// The manifest whose resources the shown dry run would delete.
    pending_delete: Option<String>,
    /// A revealed Secret: name and decoded values. Dropped by Back.
    revealed: Option<(String, Vec<(String, String)>)>,
    /// A Secret being created. Dropped by Back and by submitting.
    draft: Option<Draft>,
}

#[derive(Default)]
pub struct Draft {
    name: String,
    key: String,
    value: String,
    entries: Vec<(String, String)>,
}

/// `describe`, YAML or diff output.
pub struct Details {
    title: String,
    text: String,
    /// `yaml`, `diff` or `text` (highlighting).
    lang: &'static str,
    /// Kind and name of the object an editable YAML is of.
    editing: Option<(String, String)>,
    /// What the last apply printed.
    note: Option<String>,
}

impl KubeExplorer {
    fn args(&self, rest: &[&str]) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(path) = host::setting_string("kubeconfig").filter(|p| !p.is_empty()) {
            args.push(format!("--kubeconfig={path}"));
        }
        if let Some(context) = &self.context {
            args.push(format!("--context={context}"));
        }
        if let Some(namespace) = &self.namespace {
            args.push(format!("--namespace={namespace}"));
        }
        args.extend(rest.iter().map(|s| (*s).to_owned()));
        args
    }

    /// `kubectl debug` for a pod; names reach kubectl as arguments only, so one that
    /// looks like an option is refused.
    fn debug_args(&self, pod: &str, image: &str, target: Option<&str>) -> Result<Vec<String>, String> {
        let named = [("pod", pod), ("image", image)].into_iter().chain(target.map(|t| ("container", t)));
        for (what, value) in named {
            if value.is_empty() || value.starts_with('-') || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!(
                    "Refusing to debug: the {what} name \"{value}\" is empty, starts with '-' or has whitespace."
                ));
            }
        }
        let image = format!("--image={image}");
        let target = target.map(|t| format!("--target={t}"));
        let mut rest = vec!["debug", "-it", pod, image.as_str()];
        rest.extend(target.as_deref());
        Ok(self.args(&rest))
    }

    fn debug_pod(&self, pod: &str) -> Result<(), Error> {
        let image = host::setting_string("debugImage").filter(|i| !i.trim().is_empty());
        let image = image.as_deref().map_or(DEFAULT_DEBUG_IMAGE, str::trim);
        self.debug_args(pod, image, None)?;
        let first = self.kubectl(&["get", "pod", pod, "-o", "jsonpath={.spec.containers[0].name}", REQUEST_TIMEOUT], None);
        let target = first.as_deref().map(str::trim).ok().filter(|t| !t.is_empty());
        let args = self.debug_args(pod, image, target)?;
        host::open_terminal(&format!("Debug: {pod}"), KUBECTL, &args)
    }

    fn reveal_allowed() -> bool {
        host::setting_bool("allowRevealSecrets") == Some(true)
    }

    /// True when the secret's data may be read; otherwise tells the user how to allow it.
    fn secret_gate(kind: &str) -> bool {
        if kind != "secrets" || Self::reveal_allowed() {
            return true;
        }
        host::notify(
            NotificationLevel::Warning,
            "Secret values are hidden",
            "To show them, turn on \"Allow Show Secret\" (allowRevealSecrets) in Settings › Plugins › Kube Explorer.",
        );
        false
    }

    fn show_secret(&mut self, name: &str) -> Result<(), Error> {
        if !Self::secret_gate("secrets") {
            return Ok(());
        }
        if name.starts_with('-') {
            return Err("Refusing a secret name that starts with '-'.".into());
        }
        let entries = secrets::data_entries(&self.kubectl(&["get", "secret", name, "-o", "json", REQUEST_TIMEOUT], None)?)?;
        let values = entries.iter().map(|(key, bytes)| (key.clone(), secrets::display(bytes))).collect();
        self.revealed = Some((name.to_owned(), values));
        self.details = None;
        host::refresh_view(TREE);
        Ok(())
    }

    fn create_secret(&mut self) -> Result<(), Error> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        let mut entries = draft.entries.clone();
        if !draft.key.is_empty() || !draft.value.is_empty() {
            entries.push((draft.key.clone(), draft.value.clone()));
        }
        let name = draft.name.clone();
        let manifest = secrets::manifest(&name, &entries)?;
        self.draft = None;
        host::refresh_view(TREE);
        let out = host::run_process(KUBECTL, &self.args(&["create", "-f", "-"]), Some(&manifest))?;
        if out.status != Some(0) {
            if out.stderr.contains("AlreadyExists") || out.stderr.contains("already exists") {
                return Err(format!("A secret named \"{name}\" already exists; Create Secret does not overwrite it.").into());
            }
            return Err(failure(KUBECTL, &secrets::redact(&out.stderr, &entries), out.status));
        }
        host::notify(NotificationLevel::Info, "Secret created", &format!("{name} in {}", self.target()));
        self.reload();
        Ok(())
    }

    fn render_revealed(&self, name: &str, values: &[(String, String)]) -> View {
        let body = match values.is_empty() {
            true => Body::Message("This secret has no data.".into()),
            false => Body::Form(
                values.iter().map(|(key, value)| Field::new(key, key, FieldValue::Multiline(value.clone()))).collect(),
            ),
        };
        View::new(body)
            .status(&format!("Secret {name}: values are shown here only; Back clears them"))
            .button(Button::new("back", "Back").icon("arrow-left"))
    }

    fn render_draft(&self, draft: &Draft) -> View {
        // ponytail: the Form API has no masked field, so values are visible while typed.
        let fields = vec![
            Field::new("secret-name", "Secret name", FieldValue::Text(draft.name.clone())),
            Field::new("secret-key", "Key", FieldValue::Text(draft.key.clone())),
            Field::new("secret-value", "Value", FieldValue::Multiline(draft.value.clone())),
        ];
        let keys = draft.entries.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ");
        View::new(Body::Form(fields))
            .status(&format!("New Opaque Secret; keys added: {}", if keys.is_empty() { "none" } else { &keys }))
            .button(Button::new("back", "Back").icon("arrow-left"))
            .button(Button::new("add-secret-key", "Add Key").icon("add"))
            .button(Button::new("submit-secret", "Create").icon("check"))
    }

    fn kubectl(&self, rest: &[&str], stdin: Option<&str>) -> Result<String, Error> {
        let out = host::run_process(KUBECTL, &self.args(rest), stdin)?;
        if out.status == Some(0) {
            return Ok(out.stdout);
        }
        Err(failure(KUBECTL, &out.stderr, out.status))
    }

    fn load(&self, parent: &str) -> Result<Vec<String>, String> {
        if parent == "ctx" {
            let out = self.kubectl(&["config", "get-contexts", "-o", "name"], None)?;
            return Ok(out.lines().map(str::to_owned).collect());
        }
        if parent == "crds" {
            return self.custom_resources();
        }
        if parent.starts_with("crd:") {
            return Ok(Vec::new());
        }
        let kind = if parent == "ns" { "namespaces" } else { parent.trim_start_matches("kind:") };
        let label = KINDS.iter().find(|(k, ..)| *k == kind).map_or(kind.to_owned(), |(_, label, _)| label.to_lowercase());
        let _task = host::begin_task(&format!("Loading {label}"), false);
        let out = self.kubectl(&["get", kind, "-o", "name", REQUEST_TIMEOUT], None)?;
        Ok(out.lines().map(|line| line.split_once('/').map_or(line, |(_, name)| name).to_owned()).collect())
    }

    /// `kind.group` names of the cluster's custom resources.
    fn custom_resources(&self) -> Result<Vec<String>, String> {
        let _task = host::begin_task("Loading custom resources", false);
        let out = self.kubectl(&["api-resources", "-o", "name", "--verbs=list", REQUEST_TIMEOUT], None)?;
        Ok(custom_resource_names(out.lines().map(str::to_owned).collect()))
    }

    /// Drops cached children and reloads those of expanded nodes.
    fn reload(&mut self) {
        self.children.clear();
        self.current_context = None;
        for key in self.expanded.clone() {
            let loaded = self.load(&key);
            self.children.insert(key, loaded);
        }
        host::refresh_view(TREE);
    }

    /// `context/namespace` the cluster calls go to, for messages.
    fn target(&self) -> String {
        let current = || self.kubectl(&["config", "current-context"], None).ok().map(|out| out.trim().to_owned());
        let context = self.context.clone().or_else(|| self.current_context.clone()).or_else(current);
        let context = context.unwrap_or_else(|| "(kubectl unavailable)".into());
        format!("{context}/{}", self.namespace.as_deref().unwrap_or("(default)"))
    }

    fn save(&self) {
        for (key, value) in [("context", &self.context), ("namespace", &self.namespace)] {
            let _ = match value {
                Some(v) => host::storage::set(key, v),
                None => host::storage::delete(key),
            };
        }
    }

    fn show(&mut self, title: String, text: String, lang: &'static str) {
        self.details = Some(Details { title, text, lang, editing: None, note: None });
        self.forward = None;
        self.pending_delete = None;
        host::refresh_view(TREE);
    }

    /// The YAML in an editor on a temp file (saving applies it), else in the panel.
    fn edit_yaml(&mut self, kind: &str, name: &str) -> Result<(), Error> {
        if !Self::secret_gate(kind) {
            return Ok(());
        }
        let _task = host::begin_task(&format!("Loading {kind}/{name}"), false);
        let text = self.kubectl(&["get", kind, name, "-o", "yaml", REQUEST_TIMEOUT], None)?;
        let yaml = without_resource_version(&text);
        let yaml = format!("{}{yaml}", schema::modeline(&yaml).unwrap_or_default());
        if let Err(e) = self.open_temp_file(kind, name, &yaml) {
            host::log(LogLevel::Info, &format!("editing {kind}/{name} in the panel: {e}"));
            self.show_yaml(kind, name, text, None);
        }
        Ok(())
    }

    fn open_temp_file(&self, kind: &str, name: &str, yaml: &str) -> Result<(), Error> {
        // Unguessable, so another local user can't plant a symlink at it in /tmp.
        let nonce = RandomState::new().hash_one(0u8) as u32;
        let path = format!("/tmp/kube-explorer-{}-{}-{nonce:08x}.yaml", file_safe(kind), file_safe(name));
        let out = host::run_process(TEE, &[path.clone()], Some(yaml))?;
        if out.status != Some(0) {
            return Err(failure(TEE, &out.stderr, out.status));
        }
        // ponytail: one storage entry per opened file, never pruned; prune by age if 1 MiB gets near.
        let origin = [self.context.as_deref().unwrap_or(""), self.namespace.as_deref().unwrap_or(""), kind, name];
        host::storage::set(&format!("edit:{path}"), &origin.join("\n"))?;
        host::subscribe(EventKind::FileSaved)?;
        host::open_location(&path, 0, 0)
    }

    /// Replaces the object a saved temp file was opened for, in the context it came from.
    fn apply_saved(path: &str) {
        let Ok(Some(origin)) = host::storage::get(&format!("edit:{path}")) else {
            return;
        };
        let parts: Vec<&str> = origin.split('\n').collect();
        let [context, namespace, kind, name] = parts[..] else {
            return;
        };
        let given = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let origin = KubeExplorer { context: given(context), namespace: given(namespace), ..KubeExplorer::default() };
        let _task = host::begin_task(&format!("Applying {kind}/{name}"), false);
        match origin.kubectl(&["replace", "-f", path], None) {
            Ok(out) => host::notify(NotificationLevel::Info, &format!("Applied {kind}/{name}"), out.trim()),
            Err(e) => host::notify(NotificationLevel::Error, &format!("Could not apply {kind}/{name}"), &e.to_string()),
        }
    }

    fn open_yaml(&mut self, kind: &str, name: &str, note: Option<String>) -> Result<(), Error> {
        if !Self::secret_gate(kind) {
            return Ok(());
        }
        let _task = host::begin_task(&format!("Loading {kind}/{name}"), false);
        let text = self.kubectl(&["get", kind, name, "-o", "yaml", REQUEST_TIMEOUT], None)?;
        self.show_yaml(kind, name, text, note);
        Ok(())
    }

    fn describe(&mut self, kind: &str, name: &str) -> Result<(), Error> {
        let _task = host::begin_task(&format!("Describing {kind}/{name}"), false);
        let text = self.kubectl(&["describe", kind, name, REQUEST_TIMEOUT], None)?;
        self.show(format!("{kind}/{name}"), text, "yaml");
        Ok(())
    }

    fn show_yaml(&mut self, kind: &str, name: &str, text: String, note: Option<String>) {
        let editing = Some((kind.to_owned(), name.to_owned()));
        self.details = Some(Details { title: format!("{kind}/{name}"), text, lang: "yaml", editing, note });
        host::refresh_view(TREE);
    }

    /// Replaces the edited object; on failure the page keeps the edits and shows why.
    fn apply_yaml(&mut self, yaml: &str) -> Result<(), Error> {
        let Some((kind, name)) = self.details.as_ref().and_then(|d| d.editing.clone()) else {
            return Ok(());
        };
        let _task = host::begin_task(&format!("Applying {kind}/{name}"), false);
        match self.kubectl(&["replace", "-f", "-"], Some(yaml)) {
            Ok(out) => self.open_yaml(&kind, &name, Some(out.trim().to_owned())),
            Err(e) => host::post_view_message(TREE, &format!("error:{e}")),
        }
    }

    fn render_details(&self, details: &Details) -> View {
        let page = PAGE
            .replace("{{lang}}", details.lang)
            .replace("{{name}}", &escape_html(&details.title.replace('/', "-")))
            .replace("{{readonly}}", if details.editing.is_some() { "" } else { "readonly" })
            .replace("{{nojs}}", if details.editing.is_some() { NO_SCRIPTS } else { "" })
            .replace("{{highlighted}}", &highlight(&details.text, details.lang))
            .replace("{{text}}", &escape_html(&details.text));
        let status = match &details.note {
            Some(note) => format!("{} · {note}", details.title),
            None => details.title.clone(),
        };
        let mut view = View::new(Body::Html(page)).status(&status).button(Button::new("back", "Back").icon("arrow-left"));
        if details.editing.is_some() {
            view = view
                .button(Button::new("apply", "Apply").icon("check"))
                .button(Button::new("save-file", "Save to File").icon("export"))
                .button(Button::new("reload-yaml", "Refresh").icon("refresh"));
        }
        if self.pending_delete.is_some() {
            return view.button(Button::new("confirm-delete-manifest", "Confirm Delete").icon("trash"));
        }
        self.actions(view)
    }

    fn start_port_forward(&mut self, kind: &str, name: &str) -> Result<(), Error> {
        let Some((_, ports_path)) = FORWARDABLE.iter().find(|(k, _)| *k == kind) else {
            return Ok(());
        };
        let found = self.kubectl(&["get", kind, name, "-o", &format!("jsonpath={ports_path}"), REQUEST_TIMEOUT], None)?;
        // A port served over TCP and UDP is listed twice; forwarding it twice fails.
        let mut ports: Vec<&str> = Vec::new();
        for port in found.split_whitespace() {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        let ports = ports.iter().map(|p| format!("{p}:{p}")).collect::<Vec<_>>().join(" ");
        self.forward = Some((format!("{kind}/{name}"), ports));
        self.details = None;
        host::refresh_view(TREE);
        Ok(())
    }

    fn render_forward(&self, target: &str, ports: &str) -> View {
        let field = Field::new("ports", "Ports (local:remote, separated by spaces)", FieldValue::Text(ports.into()));
        View::new(Body::Form(vec![field]))
            .status(&format!("Port forward to {target}"))
            .button(Button::new("back", "Back").icon("arrow-left"))
            .button(Button::new("start-forward", "Start").icon("run"))
    }

    fn render_tree(&mut self) -> Result<View, Error> {
        if let Some((target, ports)) = &self.forward {
            return Ok(self.render_forward(target, ports));
        }
        if let Some(draft) = &self.draft {
            return Ok(self.render_draft(draft));
        }
        if !Self::reveal_allowed() {
            self.revealed = None;
        }
        if let Some((name, values)) = &self.revealed {
            return Ok(self.render_revealed(name, values));
        }
        if let Some(details) = &self.details {
            return Ok(self.render_details(details));
        }
        if self.context.is_none() && self.current_context.is_none() {
            self.current_context = match self.kubectl(&["config", "current-context"], None) {
                Ok(out) => Some(out.trim().to_owned()),
                Err(Error::Denied(_)) => {
                    return Ok(View::message("Kube Explorer needs permission to run kubectl. Grant it in Settings › Plugins."));
                }
                Err(_) => Some("(kubectl unavailable)".into()),
            };
        }
        let context = self.context.as_ref().or(self.current_context.as_ref()).cloned().unwrap_or_default();
        let namespace = self.namespace.clone().unwrap_or_else(|| "(default)".into());

        let mut items = Vec::new();
        self.push_node(&mut items, "ctx", &format!("Context: {context}"), k8s!("cluster"), Some(&context));
        self.push_node(&mut items, "ns", &format!("Namespace: {namespace}"), k8s!("ns"), self.namespace.as_ref());
        for (kind, label, icon) in KINDS {
            self.push_node(&mut items, &format!("kind:{kind}"), label, icon, None);
        }

        self.push_custom_resources(&mut items);

        let view = View::new(Body::Tree(items)).status("Double-click a context or namespace to switch");
        Ok(self.actions(view.button(Button::new("refresh", "Refresh").icon("refresh"))))
    }

    fn push_custom_resources(&self, items: &mut Vec<TreeItem>) {
        let expanded = self.expanded.contains("crds");
        items.push(TreeItem::new("crds", "Custom Resources").icon("folder-library").expandable(expanded));
        if !expanded {
            return;
        }
        let names = match self.children.get("crds") {
            Some(Ok(names)) => names,
            Some(Err(e)) => {
                let reason = e.lines().last().unwrap_or(e);
                items.push(TreeItem::new("crds!", reason).parent("crds").icon("error"));
                return;
            }
            None => return,
        };
        if names.is_empty() {
            items.push(TreeItem::new("crds!", "No custom resources").parent("crds").icon("info"));
        }
        let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for name in names {
            if let Some((kind, group)) = name.split_once('.') {
                groups.entry(group).or_default().push(kind);
            }
        }
        for (group, kinds) in groups {
            let group_key = format!("crd:{group}");
            let group_expanded = self.expanded.contains(&group_key);
            items.push(TreeItem::new(&group_key, group).parent("crds").icon("folder-resources").expandable(group_expanded));
            if !group_expanded {
                continue;
            }
            for kind in kinds {
                let at = items.len();
                self.push_node(items, &format!("kind:{kind}.{group}"), kind, "file-yaml", None);
                items[at].parent = Some(group_key.clone());
            }
        }
    }

    /// The buttons acting on the selected resource.
    fn actions(&self, view: View) -> View {
        let kind = self.selected.as_ref().map_or("", |(kind, _)| kind.as_str());
        let pod_selected = kind == "pods";
        let logs = pod_selected || WORKLOADS.contains(&kind);
        let forwardable = FORWARDABLE.iter().any(|(k, _)| *k == kind);
        let delete = match (&self.selected, self.confirm_delete) {
            (Some((kind, name)), true) => format!("Confirm Delete {kind}/{name}"),
            _ => "Delete".into(),
        };
        view.button(Button::new("describe", "Describe").icon("info").enabled(self.selected.is_some()))
            .button(Button::new("yaml", "YAML").icon("file-yaml").enabled(self.selected.is_some()))
            .button(Button::new("logs", "Logs").icon("file-log").enabled(logs))
            .button(Button::new("shell", "Shell").icon("terminal").enabled(pod_selected))
            .button(Button::new("attach", "Attach").icon("attach-to-process").enabled(pod_selected))
            .button(Button::new("show-secret", "Show Secret").icon("eye").enabled(kind == "secrets"))
            .button(Button::new("create-secret", "Create Secret").icon("add"))
            .button(Button::new("debug", "Debug").icon("debug").enabled(pod_selected))
            .button(Button::new("port-forward", "Port Forward").icon("external-link").enabled(forwardable))
            .button(Button::new("delete", &delete).icon("trash").enabled(self.selected.is_some()))
    }

    fn push_node(&self, items: &mut Vec<TreeItem>, key: &str, label: &str, icon: &str, active: Option<&String>) {
        let expanded = self.expanded.contains(key);
        items.push(TreeItem::new(key, label).icon(icon).expandable(expanded));
        if !expanded {
            return;
        }
        match self.children.get(key) {
            // An expandable node without children is drawn as still loading.
            Some(Ok(names)) if names.is_empty() => {
                items.push(TreeItem::new(&format!("{key}!"), "No resources").parent(key).icon("info"));
            }
            Some(Ok(names)) => {
                for name in names {
                    let child = TreeItem::new(&format!("{key}/{name}"), name).parent(key).icon(icon);
                    items.push(if Some(name) == active { child.detail("active") } else { child });
                }
            }
            // kubectl repeats client-side noise first; its last line is the reason.
            Some(Err(e)) => {
                let reason = e.lines().last().unwrap_or(e);
                items.push(TreeItem::new(&format!("{key}!"), reason).parent(key).icon("error"));
            }
            None => {}
        }
    }

    fn handle_tree(&mut self, event: ViewEvent) -> Result<(), Error> {
        match event {
            ViewEvent::Expand(key) => {
                if !self.children.contains_key(&key) {
                    let loaded = self.load(&key);
                    self.children.insert(key.clone(), loaded);
                }
                self.expanded.insert(key);
                host::refresh_view(TREE);
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
            }
            ViewEvent::Select(key) => {
                self.selected = resource(&key);
                self.confirm_delete = false;
                host::refresh_view(TREE);
            }
            ViewEvent::Activate(key) => match key.split_once('/') {
                Some(("ctx", name)) => {
                    self.context = Some(name.to_owned());
                    self.namespace = None;
                    self.selected = None;
                    self.save();
                    self.reload();
                }
                Some(("ns", name)) => {
                    self.namespace = Some(name.to_owned());
                    self.selected = None;
                    self.save();
                    self.reload();
                }
                _ => {
                    if let Some((kind, name)) = resource(&key) {
                        self.describe(&kind, &name)?;
                    }
                }
            },
            ViewEvent::Button(button) => self.button(&button)?,
            ViewEvent::Field(key, value) if key.starts_with("secret-") => {
                if let Some(draft) = &mut self.draft {
                    match key.as_str() {
                        "secret-name" => draft.name = value,
                        "secret-key" => draft.key = value,
                        _ => draft.value = value,
                    }
                }
            }
            ViewEvent::Field(key, value) if key == "ports" => {
                if let Some((_, ports)) = &mut self.forward {
                    *ports = value;
                }
            }
            ViewEvent::Message(message) => {
                if let Some(yaml) = message.strip_prefix("apply\n") {
                    self.apply_yaml(yaml)?;
                }
                if let Some(problem) = message.strip_prefix("diag:") {
                    host::log(LogLevel::Warn, &format!("details page: {problem}"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn button(&mut self, button: &str) -> Result<(), Error> {
        if button == "refresh" {
            self.reload();
            return Ok(());
        }
        match button {
            "back" => {
                self.details = None;
                self.forward = None;
                self.pending_delete = None;
                self.revealed = None;
                self.draft = None;
                host::refresh_view(TREE);
                return Ok(());
            }
            "create-secret" => {
                self.draft = Some(Draft::default());
                host::refresh_view(TREE);
                return Ok(());
            }
            "add-secret-key" => {
                if let Some(draft) = &mut self.draft {
                    let entry = (std::mem::take(&mut draft.key), std::mem::take(&mut draft.value));
                    if !secrets::valid_key(&entry.0) {
                        draft.key = entry.0;
                        draft.value = entry.1;
                        return Err("Enter a key (letters, digits, '-', '_' and '.') before adding it.".into());
                    }
                    draft.entries.push(entry);
                    host::refresh_view(TREE);
                }
                return Ok(());
            }
            "submit-secret" => return self.create_secret(),
            "start-forward" => {
                let Some((target, ports)) = &self.forward else {
                    return Ok(());
                };
                let mut args = vec!["port-forward", target.as_str()];
                args.extend(ports.split_whitespace());
                return host::open_terminal(&format!("Port Forward: {target}"), KUBECTL, &self.args(&args));
            }
            "confirm-delete-manifest" => {
                let Some(manifest) = self.pending_delete.take() else {
                    return Ok(());
                };
                let _task = host::begin_task("Deleting the manifest's resources", false);
                let out = self.kubectl(&["delete", "-f", "-"], Some(&manifest))?;
                self.show("Deleted".into(), out, "text");
                self.reload();
                return Ok(());
            }
            "apply" => return host::post_view_message(TREE, "collect:apply"),
            "save-file" => return host::post_view_message(TREE, "collect:save"),
            "reload-yaml" => {
                if let Some((kind, name)) = self.details.as_ref().and_then(|d| d.editing.clone()) {
                    self.open_yaml(&kind, &name, None)?;
                }
                return Ok(());
            }
            _ => {}
        }
        let Some((kind, name)) = self.selected.clone() else {
            return Ok(());
        };
        let target = format!("{kind}/{name}");
        match button {
            "describe" => self.describe(&kind, &name)?,
            "yaml" => self.edit_yaml(&kind, &name)?,
            "logs" if kind == "pods" => {
                host::open_terminal(&format!("Logs: {name}"), KUBECTL, &self.args(&["logs", "-f", "--tail=500", &name]))?
            }
            "logs" => host::open_terminal(
                &format!("Logs: {target}"),
                KUBECTL,
                &self.args(&["logs", "-f", &target, "--all-pods=true", "--all-containers", "--max-log-requests=20", "--tail=200"]),
            )?,
            "attach" => host::open_terminal(&format!("Attach: {name}"), KUBECTL, &self.args(&["attach", "-it", &name]))?,
            "debug" => self.debug_pod(&name)?,
            "show-secret" if kind == "secrets" => self.show_secret(&name)?,
            "port-forward" => self.start_port_forward(&kind, &name)?,
            "shell" => host::open_terminal(
                &format!("Shell: {name}"),
                KUBECTL,
                &self.args(&["exec", "-it", &name, "--", "sh", "-c", "command -v bash >/dev/null && exec bash || exec sh"]),
            )?,
            "delete" if !self.confirm_delete => {
                self.confirm_delete = true;
                host::refresh_view(TREE);
            }
            "delete" => {
                let _task = host::begin_task(&format!("Deleting {target}"), false);
                self.kubectl(&["delete", &kind, &name, "--wait=false"], None)?;
                self.selected = None;
                self.confirm_delete = false;
                self.details = None;
                self.reload();
            }
            _ => {}
        }
        Ok(())
    }
}

/// Sorted `kind.group` names without the core and built-in groups, deduplicated.
fn custom_resource_names(names: Vec<String>) -> Vec<String> {
    let mut custom: Vec<String> = names
        .into_iter()
        .filter(|name| name.split_once('.').is_some_and(|(_, group)| !BUILT_IN_GROUPS.contains(&group)))
        .collect();
    custom.sort();
    custom.dedup();
    custom
}

/// `(kind, name)` of a resource node key (`kind:pods/web-1`).
fn resource(key: &str) -> Option<(String, String)> {
    let (parent, name) = key.split_once('/')?;
    let kind = parent.strip_prefix("kind:")?;
    Some((kind.to_owned(), name.to_owned()))
}

fn failure(program: &str, stderr: &str, status: Option<i32>) -> Error {
    let message = stderr.trim();
    if !message.is_empty() {
        return Error::Failed(message.to_owned());
    }
    match status {
        Some(code) => Error::Failed(format!("{program} exited with status {code}.")),
        None => Error::Failed(format!("{program} stopped without an exit status.")),
    }
}

/// Drops `metadata.resourceVersion`, so each save of an edited object replaces
/// it whatever changed in between (a kept one fails every save after the first).
fn without_resource_version(yaml: &str) -> String {
    let mut in_metadata = false;
    let mut out = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        if !line.starts_with(' ') {
            in_metadata = line.trim_end() == "metadata:";
        }
        if in_metadata && line.starts_with("  resourceVersion:") {
            continue;
        }
        out.push_str(line);
    }
    out
}

/// The directory of a file path.
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(".", |(dir, _)| dir)
}

/// The innermost Helm chart (a directory with `Chart.yaml`) holding `file`.
fn chart_dir(ctx: &Invocation, file: &str) -> Result<String, Error> {
    let project = ctx.project().ok_or("No project is open.")?;
    let charts = project.find_files("**/Chart.yaml", 1000)?;
    charts
        .iter()
        .filter_map(|chart| chart.strip_suffix("/Chart.yaml"))
        .filter(|dir| file.starts_with(&format!("{dir}/")))
        .max_by_key(|dir| dir.len())
        .map(str::to_owned)
        .ok_or_else(|| "The open file is not in a Helm chart (no Chart.yaml in its folder or above).".into())
}

fn file_safe(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect()
}

/// The page's highlighting (`page.html` does the same while editing), done here so it shows when
/// the IDE blocks the page's script. `lang` is `yaml`, `diff` or `text`.
fn highlight(text: &str, lang: &str) -> String {
    let line: fn(&str) -> String = match lang {
        "diff" => diff_line,
        "text" => escape_html,
        _ => yaml_line,
    };
    // The extra line keeps a trailing newline as tall as in the textarea.
    text.split('\n').map(line).collect::<Vec<_>>().join("\n") + "\n "
}

fn span(class: &str, text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    format!("<span class=\"{class}\">{}</span>", escape_html(text))
}

fn yaml_value(v: &str) -> String {
    let t = v.trim();
    let quoted = t.len() >= 2 && (t.starts_with('"') && t.ends_with('"') || t.starts_with('\'') && t.ends_with('\''));
    let number = t.strip_prefix('-').unwrap_or(t).split_once('.').map_or(is_digits(t.strip_prefix('-').unwrap_or(t)), |(a, b)| is_digits(a) && is_digits(b));
    match () {
        _ if t.starts_with('#') => span("m", v),
        _ if quoted => span("s", v),
        _ if number || matches!(t, "true" | "false" | "null" | "~") => span("n", v),
        _ => escape_html(v),
    }
}

fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn yaml_line(line: &str) -> String {
    if line.trim_start().starts_with('#') {
        return span("m", line);
    }

    let mut prefix = line.len() - line.trim_start().len();
    while let Some(rest) = line[prefix..].strip_prefix('-')
        && rest.starts_with(char::is_whitespace)
    {
        prefix = line.len() - rest.trim_start().len();
    }
    let (lead, rest) = line.split_at(prefix);

    let key_end = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => rest[1..].find(q).map(|i| i + 2),
        Some(c) if !c.is_whitespace() && c != '#' && c != '-' => rest.find([':', '#']).filter(|&i| rest[i..].starts_with(':')),
        _ => None,
    };
    if let Some(end) = key_end
        && let Some(after) = rest[end..].strip_prefix(':')
        && (after.is_empty() || after.starts_with(char::is_whitespace))
    {
        return format!("{}{}:{}", span("m", lead), span("k", &rest[..end]), yaml_value(after));
    }

    match lead.contains('-') {
        true => span("m", lead) + &yaml_value(rest),
        false => yaml_value(line),
    }
}

fn diff_line(line: &str) -> String {
    match () {
        _ if line.starts_with("@@") => span("hunk", line),
        _ if line.starts_with('+') => span("add", line),
        _ if line.starts_with('-') => span("del", line),
        _ => escape_html(line),
    }
}

/// The `Chart.yaml` above the nearest `templates` directory of `path`.
fn chart_file(path: &str) -> Option<String> {
    let dir = path.rsplit_once('/')?.0;
    let (at, _) = dir.rmatch_indices("templates").find(|(i, _)| {
        dir[..*i].ends_with('/') && (dir.len() == i + 9 || dir[i + 9..].starts_with('/'))
    })?;
    Some(format!("{}/Chart.yaml", dir[..at].trim_end_matches('/')))
}

const MAX_BYTES: usize = 1 << 20;

/// Publishes the manifest problems of an opened or saved YAML file of an open project.
fn inspect(path: &str) {
    if !(path.ends_with(".yaml") || path.ends_with(".yml") || path.ends_with(".tpl")) {
        return;
    }
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
    if text.len() > MAX_BYTES {
        let _ = host::publish_diagnostics(path, &[]);
        return;
    }
    let in_chart = chart_file(path).is_some_and(|chart| project.read_file(&chart).is_ok());
    let directory = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let patch_context = ["kustomization.yaml", "kustomization.yml", "Kustomization"]
        .iter()
        .any(|name| project.read_file(&format!("{directory}/{name}")).is_ok());
    let enabled = |code: &str| {
        let setting = checks::SETTINGS.iter().find(|(c, _)| *c == code).map(|(_, s)| *s);
        setting.and_then(host::setting_bool) != Some(false)
    };
    let diagnostics: Vec<_> = checks::for_file(path, &text, in_chart, patch_context)
        .iter()
        .filter(|finding| enabled(finding.code))
        .map(|finding| finding.diagnostic(&text))
        .collect();
    let _ = host::publish_diagnostics(path, &diagnostics);
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

impl Plugin for KubeExplorer {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileOpened, EventKind::FileSaved] {
            if let Err(error) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("Manifest inspections need the {} event: {error}", kind.name()));
            }
        }
        self.context = host::storage::get("context").ok().flatten();
        self.namespace = host::storage::get("namespace").ok().flatten();
        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for event in events {
            match event {
                Event::FileSaved(path) => {
                    Self::apply_saved(path);
                    inspect(path);
                }
                Event::FileOpened(path) => inspect(path),
                _ => {}
            }
        }
    }

    /// The IDE drops a command's result text: it is shown in a notification.
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let (working, done) = match id {
            "apply" => ("Applying manifest", "Manifest applied"),
            "dry-run" => ("Validating manifest", "Manifest validated"),
            "diff" => ("Comparing manifest with cluster", "Manifest diff"),
            "delete-manifest" => ("Previewing deletion", "Deletion preview"),
            "kustomize-render" => ("Rendering Kustomization", "Kustomization rendered"),
            "kustomize-apply" => ("Applying Kustomization", "Kustomization applied"),
            "helm-template" => ("Rendering Helm chart", "Helm chart rendered"),
            "helm-lint" => ("Linting Helm chart", "Helm chart linted"),
            "helm-dependency-update" => ("Updating Helm dependencies", "Helm dependencies updated"),
            _ => return Err(Error::NotFound(format!("command {id}"))),
        };
        let _task = host::begin_task(working, false);
        let text = self.command(id, ctx)?;
        host::notify(NotificationLevel::Info, done, &text);
        Ok(None)
    }
}

impl KubeExplorer {
    fn command(&mut self, id: &str, ctx: &Invocation) -> Result<String, Error> {
        let editor = ctx.editor().ok_or("No manifest is open.")?;
        let manifest = editor.document()?.text;
        let path = editor.path();
        match id {
            "delete-manifest" => {
                let out = self.kubectl(&["delete", "--dry-run=server", "-f", "-"], Some(&manifest))?;
                self.show("Delete these resources?".into(), out, "text");
                self.pending_delete = Some(manifest);
                Ok("Review the resources in Kube Explorer, then click Confirm Delete.".into())
            }
            "kustomize-render" => {
                let dir = parent(&path);
                let out = self.kubectl(&["kustomize", dir], None)?;
                self.show(format!("Kustomize: {dir}"), out, "yaml");
                Ok("The rendered YAML is shown in Kube Explorer.".into())
            }
            "kustomize-apply" => {
                let out = self.kubectl(&["apply", "-k", parent(&path)], None)?;
                let target = self.target();
                self.reload();
                Ok(format!("{target}: {}", out.trim()))
            }
            "helm-template" | "helm-lint" | "helm-dependency-update" => {
                let chart = chart_dir(ctx, &path)?;
                let release = chart.rsplit('/').next().unwrap_or("release");
                let args: &[&str] = match id {
                    "helm-template" => &["template", release, &chart],
                    "helm-lint" => &["lint", &chart],
                    _ => &["dependency", "update", &chart],
                };
                let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
                let out = host::run_process(HELM, &args, None)?;
                // `helm lint` reports problems on stdout and fails; show them either way.
                if id == "helm-lint" {
                    self.show(format!("Helm lint: {release}"), format!("{}{}", out.stdout, out.stderr), "text");
                    return Ok(if out.status == Some(0) { "The chart passed lint. The output is shown in Kube Explorer." } else { "The chart has lint errors. The output is shown in Kube Explorer." }.into());
                }
                if out.status != Some(0) {
                    return Err(failure(HELM, &out.stderr, out.status));
                }
                if id == "helm-template" {
                    self.show(format!("Helm template: {release}"), out.stdout, "yaml");
                    return Ok("The rendered YAML is shown in Kube Explorer.".into());
                }
                Ok(out.stdout.trim().to_owned())
            }
            "apply" => {
                let out = self.kubectl(&["apply", "-f", "-"], Some(&manifest))?;
                let target = self.target();
                self.reload();
                Ok(format!("{target}: {}", out.trim()))
            }
            "dry-run" => {
                let out = self.kubectl(&["apply", "--dry-run=server", "-f", "-"], Some(&manifest))?;
                Ok(out.trim().to_owned())
            }
            "diff" => {
                let out = host::run_process(KUBECTL, &self.args(&["diff", "-f", "-"]), Some(&manifest))?;
                match out.status {
                    Some(0) => Ok("No differences with the cluster.".into()),
                    Some(1) => {
                        self.show(format!("Diff: {}", editor.path()), out.stdout, "diff");
                        Ok("The differences are shown in Kube Explorer.".into())
                    }
                    status => Err(failure(KUBECTL, &out.stderr, status)),
                }
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

impl Providers for KubeExplorer {
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let mut actions = schema::code_actions(doc);
        actions.extend(convert::code_actions(doc, selection));
        actions
    }
}

impl Views for KubeExplorer {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        self.render_tree()
    }

    fn handle(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        self.handle_tree(event)
    }
}

register_plugin!(KubeExplorer, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::FieldValue;

    fn ok(stdout: &str) -> Result<ProcessOutput, Error> {
        Ok(ProcessOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        host.on_process(KUBECTL, |args, stdin| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("config") if args.contains(&"current-context".to_owned()) => ok("minikube\n"),
            Some("config") => ok("minikube\narn:aws:eks:eu-west-1:1:cluster/prod\n"),
            Some("get") if args.iter().any(|a| a.starts_with("jsonpath=")) => ok("80 80 443"),
            Some("get") if args.contains(&"yaml".to_owned()) => ok("apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-1\n"),
            Some("get") if args.contains(&"pods".to_owned()) => ok("pod/web-1\npod/web-2\n"),
            Some("describe") => ok("Name: web-1\n"),
            Some("kustomize") => ok("kind: ConfigMap\n"),
            Some("replace") if stdin.is_some_and(|s| s.contains("conflict")) => {
                Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "the object has been modified\n".into() })
            }
            Some("replace") => ok("pod/web-1 replaced\n"),
            Some("delete") => ok("pod \"web-1\" deleted\n"),
            Some("apply") if stdin == Some("kind: Deployment") => ok("deployment.apps/web configured\n"),
            _ => ok(""),
        });
        host
    }

    fn keys(view: View) -> Vec<String> {
        match view.body {
            Body::Tree(items) => items.into_iter().map(|i| i.key).collect(),
            other => panic!("not a tree: {other:?}"),
        }
    }

    #[test]
    fn expanding_a_kind_lists_its_resources_and_a_pod_opens_logs() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand("kind:pods".into()), &host.invocation()).unwrap();
        let tree = keys(kube.render(TREE, None, &host.invocation()).unwrap());
        assert!(tree.contains(&"kind:pods/web-1".to_owned()));

        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("logs".into()), &host.invocation()).unwrap();
        assert_eq!(host.terminals()[0].args, ["logs", "-f", "--tail=500", "web-1"]);
    }

    #[test]
    fn choosing_a_context_with_a_slash_persists_it_and_passes_it_to_kubectl() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand("ctx".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Activate("ctx/arn:aws:eks:eu-west-1:1:cluster/prod".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand("kind:pods".into()), &host.invocation()).unwrap();

        assert_eq!(host.storage()["context"], "arn:aws:eks:eu-west-1:1:cluster/prod");
        assert!(host.process_runs().last().unwrap().args.contains(&"--context=arn:aws:eks:eu-west-1:1:cluster/prod".to_owned()));
    }

    #[test]
    fn describe_shows_its_output_in_the_tree_view_until_back() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button("describe".into()), &host.invocation()).unwrap();
        let shown = kube.render(TREE, None, &host.invocation()).unwrap().body;
        kube.handle(TREE, ViewEvent::Button("back".into()), &host.invocation()).unwrap();
        let back = kube.render(TREE, None, &host.invocation()).unwrap().body;

        assert!(matches!(shown, Body::Html(page) if page.contains("Name: web-1")));
        assert!(matches!(back, Body::Tree(_)));
        assert!(host.tasks().iter().any(|t| t.title == "Describing pods/web-1" && t.ended));
    }

    fn page(kube: &mut KubeExplorer, host: &MockHost) -> String {
        match kube.render(TREE, None, &host.invocation()).unwrap().body {
            Body::Html(page) => page,
            other => panic!("not a page: {other:?}"),
        }
    }

    #[test]
    fn highlights_yaml_and_diffs_like_the_page() {
        assert_eq!(yaml_line("  name: web # x"), r#"<span class="m">  </span><span class="k">name</span>: web # x"#);
        assert_eq!(yaml_line(r#"- image: "nginx""#), r#"<span class="m">- </span><span class="k">image</span>:<span class="s"> &quot;nginx&quot;</span>"#);
        assert_eq!(yaml_line("replicas: 3"), r#"<span class="k">replicas</span>:<span class="n"> 3</span>"#);
        assert_eq!(yaml_line("  - 10.0.0.1"), r#"<span class="m">  - </span>10.0.0.1"#);
        assert_eq!(yaml_line("url: http://x"), r#"<span class="k">url</span>: http://x"#);
        assert_eq!(yaml_line("# note"), r#"<span class="m"># note</span>"#);
        assert_eq!(diff_line("+  replicas: 2"), r#"<span class="add">+  replicas: 2</span>"#);
        assert_eq!(highlight("a <b>", "text"), "a &lt;b&gt;\n ");
    }

    #[test]
    fn yaml_is_editable_and_describe_is_read_only() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button("yaml".into()), &host.invocation()).unwrap();
        let yaml = page(&mut kube, &host);
        kube.handle(TREE, ViewEvent::Button("describe".into()), &host.invocation()).unwrap();
        let describe = page(&mut kube, &host);

        assert!(yaml.contains("name: web-1") && !yaml.contains("readonly"));
        assert!(describe.contains("readonly"));
        assert!(describe.contains(r#"<pre id="hl"><span class="k">Name</span>: web-1"#), "highlighted without the script");
        assert!(yaml.contains(r#"id="nojs""#) && !describe.contains(r#"id="nojs""#));
    }

    #[test]
    fn applying_edited_yaml_replaces_the_object_and_shows_it_again() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("yaml".into()), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Message("apply\nkind: Pod # edited".into()), &host.invocation()).unwrap();

        assert!(host.process_runs().iter().any(|r| r.args.ends_with(&["replace".into(), "-f".into(), "-".into()])));
        assert!(page(&mut kube, &host).contains("name: web-1"));
    }

    #[test]
    fn a_rejected_apply_keeps_the_edits_and_reports_why() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("yaml".into()), &host.invocation()).unwrap();
        let refreshes = host.refreshed_views().len();

        kube.handle(TREE, ViewEvent::Message("apply\nkind: Pod # conflict".into()), &host.invocation()).unwrap();

        assert_eq!(host.view_messages().last().unwrap().1, "error:the object has been modified");
        assert_eq!(host.refreshed_views().len(), refreshes);
    }

    fn host_with_tee() -> (MockHost, std::rc::Rc<std::cell::RefCell<String>>) {
        let host = host().with_capabilities(&["process:kubectl", "process:tee", "editor.read", "storage", "events", "project.read"]);
        let written = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let sink = written.clone();
        host.on_process("tee", move |_, stdin| {
            *sink.borrow_mut() = stdin.unwrap_or_default().to_owned();
            ok("")
        });
        (host, written)
    }

    fn open_in_editor(kube: &mut KubeExplorer, host: &MockHost) -> String {
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("yaml".into()), &host.invocation()).unwrap();
        host.opened_locations()[0].0.clone()
    }

    #[test]
    fn yaml_opens_in_an_editor_on_a_temp_file_when_tee_is_granted() {
        let (host, written) = host_with_tee();
        let mut kube = KubeExplorer::default();

        let path = open_in_editor(&mut kube, &host);

        assert!(path.starts_with("/tmp/kube-explorer-pods-web-1-") && path.ends_with(".yaml"));
        let tee = host.process_runs().into_iter().find(|r| r.program == "tee").unwrap();
        assert_eq!(tee.args, [path]);
        assert!(written.borrow().contains("name: web-1"));
        assert!(host.subscriptions().contains(&openidea_plugin::EventKind::FileSaved));
    }

    #[test]
    fn saving_the_temp_file_replaces_the_object_in_the_context_it_came_from() {
        let (host, _) = host_with_tee();
        let mut kube = KubeExplorer { context: Some("prod".into()), ..KubeExplorer::default() };
        let path = open_in_editor(&mut kube, &host);
        kube.context = Some("dev".into());

        KubeExplorer::default().handle_events(&[openidea_plugin::Event::FileSaved(path.clone())], &host.invocation());

        let replace = host.process_runs().into_iter().find(|r| r.args.contains(&"replace".to_owned())).unwrap();
        assert!(replace.args.contains(&"--context=prod".to_owned()));
        assert!(replace.args.ends_with(&["replace".into(), "-f".into(), path]));
    }

    #[test]
    fn saving_another_file_does_nothing() {
        let (host, _) = host_with_tee();

        KubeExplorer::default().handle_events(&[openidea_plugin::Event::FileSaved("/project/a.yaml".into())], &host.invocation());

        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn only_the_objects_own_resource_version_is_dropped() {
        let yaml = "kind: Event\ninvolvedObject:\n  resourceVersion: \"7\"\nmetadata:\n  name: x\n  resourceVersion: \"206\"\n  uid: u\nspec: {}\n";

        assert_eq!(
            without_resource_version(yaml),
            "kind: Event\ninvolvedObject:\n  resourceVersion: \"7\"\nmetadata:\n  name: x\n  uid: u\nspec: {}\n"
        );
    }

    #[test]
    fn the_temp_file_names_the_kubernetes_schema() {
        let (host, written) = host_with_tee();

        open_in_editor(&mut KubeExplorer::default(), &host);

        assert!(written.borrow().starts_with("# yaml-language-server: $schema="));
    }

    #[test]
    fn a_manifest_offers_to_add_its_schema() {
        let doc = Document { path: "/project/pod.yaml".into(), language: "yaml".into(), version: 1, text: "apiVersion: v1\nkind: Pod\n".into() };

        let actions = KubeExplorer::default().code_actions(&doc, Default::default());

        assert_eq!(actions[0].edits[0].1[0].text.lines().next().unwrap(), format!("# yaml-language-server: $schema={}", schema::url("v1", "Pod").unwrap()));
    }

    #[test]
    fn port_forward_asks_for_ports_then_opens_a_terminal() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button("port-forward".into()), &host.invocation()).unwrap();
        let form = kube.render(TREE, None, &host.invocation()).unwrap().body;
        kube.handle(TREE, ViewEvent::Field("ports".into(), "8080:80".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("start-forward".into()), &host.invocation()).unwrap();

        assert!(matches!(form, Body::Form(fields) if fields[0].value == FieldValue::Text("80:80 443:443".into())));
        assert_eq!(host.terminals()[0].args, ["port-forward", "pods/web-1", "8080:80"]);
    }

    #[test]
    fn workload_logs_follow_all_its_pods_and_pods_can_be_attached() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Select("kind:deployments/web".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("logs".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button("attach".into()), &host.invocation()).unwrap();

        assert!(host.terminals()[0].args.starts_with(&["logs".into(), "-f".into(), "deployments/web".into(), "--all-pods=true".into()]));
        assert_eq!(host.terminals()[1].args, ["attach", "-it", "web-1"]);
    }

    #[test]
    fn deleting_a_manifests_resources_previews_them_first() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "kind: Deployment", &[]);
        let mut kube = KubeExplorer::default();

        kube.run_command("delete-manifest", &host.invocation(), None).unwrap();
        let deletes = || host.process_runs().into_iter().filter(|r| r.args.contains(&"delete".to_owned())).collect::<Vec<_>>();
        assert!(deletes()[0].args.contains(&"--dry-run=server".to_owned()));

        kube.handle(TREE, ViewEvent::Button("confirm-delete-manifest".into()), &host.invocation()).unwrap();
        assert!(deletes()[1].args.ends_with(&["delete".into(), "-f".into(), "-".into()]));
    }

    #[test]
    fn kustomize_renders_the_directory_of_the_open_file() {
        let host = host();
        host.open_editor("/project/k/kustomization.yaml", "yaml", "resources: []", &[]);
        let mut kube = KubeExplorer::default();

        kube.run_command("kustomize-render", &host.invocation(), None).unwrap();

        assert!(host.process_runs()[0].args.ends_with(&["kustomize".into(), "/project/k".into()]));
        assert!(page(&mut kube, &host).contains("kind: ConfigMap"));
    }

    #[test]
    fn helm_templates_the_chart_around_the_open_file() {
        let host = host().with_capabilities(&["process:kubectl", "process:helm", "editor.read", "storage", "project.read"]);
        host.on_process("helm", |_, _| ok("kind: Service\n"));
        host.add_file("/project/charts/web/Chart.yaml", "name: web");
        host.open_editor("/project/charts/web/templates/svc.yaml", "yaml", "kind: Service", &[]);
        let mut kube = KubeExplorer::default();

        kube.run_command("helm-template", &host.invocation(), None).unwrap();

        assert_eq!(host.process_runs()[0].args, ["template", "web", "/project/charts/web"]);
        assert!(page(&mut kube, &host).contains("kind: Service"));
    }

    #[test]
    fn resources_carry_the_icon_of_their_kind() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand("kind:pods".into()), &host.invocation()).unwrap();
        let Body::Tree(items) = kube.render(TREE, None, &host.invocation()).unwrap().body else {
            panic!("not a tree");
        };
        let icon = |key: &str| items.iter().find(|i| i.key == key).and_then(|i| i.icon.clone());

        assert_eq!(icon("ctx").as_deref(), Some(k8s!("cluster")));
        assert_eq!(icon("kind:pods").as_deref(), Some(k8s!("pod")));
        assert_eq!(icon("kind:pods/web-1").as_deref(), Some(k8s!("pod")));
    }

    #[test]
    fn an_expanded_kind_without_resources_has_a_placeholder_child() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand("kind:jobs".into()), &host.invocation()).unwrap();
        let tree = keys(kube.render(TREE, None, &host.invocation()).unwrap());

        assert!(tree.contains(&"kind:jobs!".to_owned()));
    }

    #[test]
    fn delete_runs_only_after_confirmation() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select("kind:pods/web-1".into()), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button("delete".into()), &host.invocation()).unwrap();
        assert!(!host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));

        kube.handle(TREE, ViewEvent::Button("delete".into()), &host.invocation()).unwrap();
        assert!(host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));
    }

    const API_RESOURCES: &str = "pods\ndeployments.apps\njobs.batch\ningresses.networking.k8s.io\ncertificates.cert-manager.io\nissuers.cert-manager.io\nservices\n";

    #[test]
    fn api_resources_output_becomes_crd_nodes() {
        let host = host();
        host.on_process(KUBECTL, |args, _| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("api-resources") => {
                assert!(args.contains(&REQUEST_TIMEOUT.to_owned()));
                ok(API_RESOURCES)
            }
            _ => ok(""),
        });
        let mut kube = KubeExplorer::default();
        kube.current_context = Some("minikube".into());

        kube.handle(TREE, ViewEvent::Expand("crds".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand("crd:cert-manager.io".into()), &host.invocation()).unwrap();
        let tree = keys(kube.render(TREE, None, &host.invocation()).unwrap());

        assert!(tree.contains(&"crds".to_owned()));
        assert!(tree.contains(&"crd:cert-manager.io".to_owned()));
        assert!(tree.contains(&"kind:certificates.cert-manager.io".to_owned()));
        assert!(tree.contains(&"kind:issuers.cert-manager.io".to_owned()));
        assert!(!tree.iter().any(|k| k.starts_with("crd:") && k != "crd:cert-manager.io"));
        assert!(!tree.contains(&"kind:deployments.apps".to_owned()));
    }

    #[test]
    fn crd_kind_lists_instances() {
        let host = host();
        host.on_process(KUBECTL, |args, _| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("api-resources") => ok("certificates.cert-manager.io\n"),
            _ => ok("certificate.cert-manager.io/web-tls\ncertificate.cert-manager.io/api-tls\n"),
        });
        let mut kube = KubeExplorer::default();
        kube.current_context = Some("minikube".into());

        kube.handle(TREE, ViewEvent::Expand("crds".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand("crd:cert-manager.io".into()), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand("kind:certificates.cert-manager.io".into()), &host.invocation()).unwrap();
        let tree = keys(kube.render(TREE, None, &host.invocation()).unwrap());
        assert!(tree.contains(&"kind:certificates.cert-manager.io/web-tls".to_owned()));

        kube.handle(TREE, ViewEvent::Select("kind:certificates.cert-manager.io/web-tls".into()), &host.invocation()).unwrap();
        assert_eq!(kube.selected, Some(("certificates.cert-manager.io".into(), "web-tls".into())));
    }

    #[test]
    fn built_in_groups_are_not_custom_resources() {
        let names = [
            "storageclasses.storage.k8s.io", "roles.rbac.authorization.k8s.io", "poddisruptionbudgets.policy",
            "horizontalpodautoscalers.autoscaling", "leases.coordination.k8s.io", "customresourcedefinitions.apiextensions.k8s.io",
            "pods.metrics.k8s.io", "storageversionmigrations.storagemigration.k8s.io", "gateways.gateway.networking.k8s.io", "certificates.cert-manager.io",
            "clusters.cluster.x-k8s.io", "volumesnapshots.snapshot.storage.k8s.io", "pods",
        ];

        let custom = custom_resource_names(names.iter().map(|n| (*n).to_owned()).collect());

        assert_eq!(custom, [
            "certificates.cert-manager.io",
            "clusters.cluster.x-k8s.io",
            "gateways.gateway.networking.k8s.io",
            "volumesnapshots.snapshot.storage.k8s.io",
        ]);
    }

    fn crd_tree(api_resources: Result<ProcessOutput, Error>) -> Vec<TreeItem> {
        let host = host();
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = calls.clone();
        host.on_process(KUBECTL, move |_, _| {
            seen.set(seen.get() + 1);
            api_resources.clone()
        });
        let mut kube = KubeExplorer::default();
        kube.current_context = Some("minikube".into());
        for key in ["crds", "crd:b.io", "crd:a.io"] {
            kube.handle(TREE, ViewEvent::Expand(key.into()), &host.invocation()).unwrap();
        }
        assert_eq!(calls.get(), 1);
        let Body::Tree(items) = kube.render(TREE, None, &host.invocation()).unwrap().body else {
            panic!("not a tree");
        };
        items
    }

    #[test]
    fn groups_are_sorted_and_kinds_labelled_by_plural() {
        let items = crd_tree(ok("zeta.b.io\nalpha.a.io\nbeta.a.io\n"));
        let nodes: Vec<(&str, &str)> = items.iter().skip_while(|i| i.key != "crds").map(|i| (i.key.as_str(), i.label.as_str())).collect();

        assert_eq!(nodes, [
            ("crds", "Custom Resources"),
            ("crd:a.io", "a.io"),
            ("kind:alpha.a.io", "alpha"),
            ("kind:beta.a.io", "beta"),
            ("crd:b.io", "b.io"),
            ("kind:zeta.b.io", "zeta"),
        ]);
    }

    #[test]
    fn a_failing_api_resources_shows_an_error_node() {
        let items = crd_tree(Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Error from server (Forbidden): nope\n".into() }));

        let node = items.iter().find(|i| i.key == "crds!").unwrap();
        assert!(node.label.contains("Forbidden"));
    }

    #[test]
    fn empty_api_resources_output_shows_a_placeholder() {
        let items = crd_tree(ok(""));

        assert!(items.iter().any(|i| i.key == "crds!" && i.label == "No custom resources"));
    }

    #[test]
    fn no_custom_resources_shows_a_placeholder() {
        let items = crd_tree(ok("pods\ndeployments.apps\n"));

        assert!(items.iter().any(|i| i.key == "crds!" && i.label == "No custom resources"));
    }

    #[test]
    fn apply_sends_the_open_manifest() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "kind: Deployment", &[]);

        KubeExplorer::default().run_command("apply", &host.invocation(), None).unwrap();

        assert_eq!(host.notifications()[0].body, "minikube/(default): deployment.apps/web configured");
    }

    fn registered_templates() -> Vec<(String, String, String)> {
        let toml = include_str!("../plugin.toml");
        let field = |block: &str, key: &str| {
            let prefix = format!("{key} = \"");
            let line = block.lines().find_map(|l| l.strip_prefix(prefix.as_str())).unwrap_or_default();
            line.trim_end_matches('"').to_owned()
        };
        toml.split("[[contributes.fileTemplates]]")
            .skip(1)
            .map(|b| {
                let b = b.split("\n[").next().unwrap_or(b);
                (field(b, "id"), field(b, "name"), field(b, "path"))
            })
            .collect()
    }

    #[test]
    fn every_template_is_registered_and_exists() {
        let templates = registered_templates();
        assert_eq!(templates.len(), 15);
        let ids: BTreeSet<_> = templates.iter().map(|t| &t.0).collect();
        let names: BTreeSet<_> = templates.iter().map(|t| &t.1).collect();
        assert_eq!((ids.len(), names.len()), (15, 15));
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/");
        let mut on_disk = std::fs::read_dir(format!("{dir}templates")).unwrap().count();
        for (id, _, path) in &templates {
            let text = std::fs::read_to_string(format!("{dir}{path}")).unwrap_or_else(|_| panic!("{id}: {path} missing"));
            let mut rest = text.as_str();
            while let Some((_, after)) = rest.split_once('$') {
                let (placeholder, tail) = after.split_once('$').unwrap_or_else(|| panic!("{id}: lone $"));
                assert!(["NAME", "END"].contains(&placeholder), "{id}: placeholder ${placeholder}$");
                rest = tail;
            }
            on_disk -= 1;
        }
        assert_eq!(on_disk, 0, "a template file is not registered");
    }

    #[test]
    fn templates_pass_the_manifest_checks() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/");
        for (id, _, path) in registered_templates() {
            let text = std::fs::read_to_string(format!("{dir}{path}")).unwrap();
            let found = checks::for_file(&path, &text, false, false);
            assert!(found.is_empty(), "{id}: {:?}", found.iter().map(|f| &f.message).collect::<Vec<_>>());
            for deprecated in ["extensions/v1beta1", "v1beta1", "v2beta"] {
                assert!(!text.contains(deprecated), "{id}");
            }
        }
    }

    #[test]
    fn debug_args_include_context_namespace_and_target() {
        let _host = host();
        let kube = KubeExplorer { context: Some("prod".into()), namespace: Some("shop".into()), ..Default::default() };
        let args = kube.debug_args("web-1", "busybox:stable", Some("app")).unwrap();
        assert_eq!(
            args,
            ["--context=prod", "--namespace=shop", "debug", "-it", "web-1", "--image=busybox:stable", "--target=app"]
        );
        let args = kube.debug_args("web-1", "busybox:stable", None).unwrap();
        assert!(!args.iter().any(|a| a.starts_with("--target")));
    }

    #[test]
    fn debug_args_reject_option_like_names() {
        let _host = host();
        let kube = KubeExplorer::default();
        assert!(kube.debug_args("--rm", "busybox", None).is_err());
        assert!(kube.debug_args("web", "-v", None).is_err());
        assert!(kube.debug_args("web", "busybox", Some("--privileged")).is_err());
        assert!(kube.debug_args("", "busybox", None).is_err());
    }

    #[test]
    fn debug_args_reject_whitespace_image() {
        let _host = host();
        let kube = KubeExplorer::default();
        assert!(kube.debug_args("web", "busybox --privileged", None).is_err());
        assert!(kube.debug_args("web", "busy\tbox", None).is_err());
        assert!(kube.debug_args("web", "busybox\n", None).is_err());
        assert!(kube.debug_args("we b", "busybox", None).is_err());
        assert!(kube.debug_args("web", "busybox", Some("a b")).is_err());
        let host = host();
        host.set_setting("debugImage", "\"busybox --privileged\"");
        let kube = KubeExplorer { selected: Some(("pods".into(), "web-1".into())), ..Default::default() };
        assert!(kube.debug_pod("web-1").is_err());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn the_debug_button_opens_a_terminal_on_the_pods_first_container() {
        let host = host();
        host.on_process(KUBECTL, |args, _| match args.iter().any(|a| a.starts_with("jsonpath=")) {
            true => ok("app"),
            false => ok(""),
        });
        host.set_setting("debugImage", "\"alpine:3\"");
        let mut kube = KubeExplorer { selected: Some(("pods".into(), "web-1".into())), ..Default::default() };
        kube.button("debug").unwrap();
        let terminal = &host.terminals()[0];
        assert_eq!(terminal.args, ["debug", "-it", "web-1", "--image=alpine:3", "--target=app"]);
    }

    type Runs = std::rc::Rc<std::cell::RefCell<Vec<(Vec<String>, Option<String>)>>>;

    fn secret_json(entries: &[(&str, &[u8])]) -> String {
        let data: Vec<String> = entries.iter().map(|(k, v)| format!("\"{k}\":\"{}\"", secrets::b64_encode(v))).collect();
        format!(
            "{{\"apiVersion\":\"v1\",\"data\":{{{}}},\"kind\":\"Secret\",\"metadata\":{{\"annotations\":{{\"a\":\"{{\\\"data\\\":{{\\\"x\\\":\\\"!\\\"}}}}\"}},\"name\":\"db\"}},\"type\":\"Opaque\"}}",
            data.join(",")
        )
    }

    fn secret_host(allow: bool, json: String, apply_stderr: &'static str) -> (MockHost, Runs) {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        if allow {
            host.set_setting("allowRevealSecrets", "true");
        }
        let runs: Runs = Default::default();
        let seen = runs.clone();
        host.on_process(KUBECTL, move |args, stdin| {
            seen.borrow_mut().push((args.to_vec(), stdin.map(str::to_owned)));
            match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
                Some("get") => ok(&json),
                Some("create") if !apply_stderr.is_empty() => {
                    Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: apply_stderr.into() })
                }
                _ => ok("secret/db created\n"),
            }
        });
        (host, runs)
    }

    fn selected_secret() -> KubeExplorer {
        KubeExplorer { selected: Some(("secrets".into(), "db".into())), ..Default::default() }
    }

    fn drafting(name: &str, key: &str, value: &str) -> KubeExplorer {
        let draft = Draft { name: name.into(), key: key.into(), value: value.into(), entries: Vec::new() };
        KubeExplorer { draft: Some(draft), namespace: Some("shop".into()), ..Default::default() }
    }

    #[test]
    fn secret_values_are_base64_decoded() {
        let (_host, _) = secret_host(true, secret_json(&[("password", b"pa\"ss\nw\xc3\xa9"), ("user", b"admin")]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        let Body::Form(fields) = kube.render_tree().unwrap().body else { panic!("not a form") };
        let shown: Vec<_> = fields.iter().map(|f| (f.key.as_str(), f.value.clone())).collect();
        assert_eq!(
            shown,
            [
                ("password", FieldValue::Multiline("pa\"ss\nwé".into())),
                ("user", FieldValue::Multiline("admin".into())),
            ]
        );
        kube.button("back").unwrap();
        assert!(kube.revealed.is_none());
        assert!(matches!(kube.render_tree().unwrap().body, Body::Tree(_)));
    }

    #[test]
    fn secret_reveal_is_denied_when_setting_off() {
        let (host, runs) = secret_host(false, secret_json(&[("password", b"hunter2")]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        assert!(kube.revealed.is_none());
        let notes = host.notifications();
        assert_eq!(notes.len(), 1);
        assert!(notes[0].body.contains("allowRevealSecrets"));
        assert!(runs.borrow().is_empty());
    }

    #[test]
    fn create_secret_passes_values_on_stdin_not_args() {
        let value = "s3cr\"et\n\u{2713} 'x'";
        let (_host, runs) = secret_host(true, String::new(), "");
        let mut kube = drafting("db", "password", value);
        kube.button("submit-secret").unwrap();
        let runs = runs.borrow();
        let (args, stdin) = &runs[0];
        assert_eq!(args, &["--namespace=shop", "create", "-f", "-"]);
        let encoded = secrets::b64_encode(value.as_bytes());
        assert!(args.iter().all(|a| !a.contains(value) && !a.contains(&encoded)));
        let manifest = stdin.as_deref().unwrap();
        assert!(manifest.contains(&format!("\"password\": {encoded}\n")));
        assert!(manifest.contains("kind: Secret") && manifest.contains("type: Opaque"));
        assert!(kube.draft.is_none());
    }

    #[test]
    fn secret_values_never_reach_notifications_or_logs() {
        let value = "hunter2-top-secret";
        let encoded = secrets::b64_encode(value.as_bytes());
        let (host, runs) = secret_host(true, secret_json(&[("password", value.as_bytes())]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        kube.button("back").unwrap();
        let (failing, _) = secret_host(true, String::new(), "invalid: hunter2-top-secret rejected");
        let mut creating = drafting("db", "password", value);
        let error = format!("{:?}", creating.button("submit-secret").unwrap_err());
        assert!(!error.contains(value) && !error.contains(&encoded) && error.contains("***"));
        let mut creating = drafting("db", "password", value);
        let (ok_host, _) = secret_host(true, String::new(), "");
        creating.button("submit-secret").unwrap();
        for host in [&host, &failing, &ok_host] {
            let seen = format!(
                "{:?} {:?} {:?} {:?} {:?} {:?}",
                host.notifications(),
                host.logs(),
                host.storage(),
                host.clipboard(),
                host.terminals(),
                host.process_runs()
            );
            assert!(!seen.contains(value) && !seen.contains(&encoded), "{seen}");
        }
        assert!(runs.borrow().iter().all(|(args, _)| args.iter().all(|a| !a.contains(value))));
    }

    #[test]
    fn create_secret_rejects_invalid_keys() {
        let (_host, runs) = secret_host(true, String::new(), "");
        for key in ["", "bad key", "a/b", "..", ".", "k\u{e9}y", "k=v"] {
            assert!(drafting("db", key, "v").button("submit-secret").is_err(), "{key:?}");
        }
        for name in ["", "Db", "db_1", "-db", "db-", "a b"] {
            assert!(drafting(name, "k", "v").button("submit-secret").is_err(), "{name:?}");
        }
        let mut twice = drafting("db", "k", "v");
        twice.draft.as_mut().unwrap().entries.push(("k".into(), "w".into()));
        assert!(twice.button("submit-secret").is_err());
        assert!(drafting("db", "-k_1.x", "v").button("add-secret-key").is_ok());
        assert!(runs.borrow().is_empty());
    }

    #[test]
    fn binary_secret_value_is_shown_as_base64_not_garbage() {
        let bytes: &[u8] = &[0xff, 0xfe, 0x00, 0x80];
        let (_host, _) = secret_host(true, secret_json(&[("blob", bytes)]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        let (_, values) = kube.revealed.unwrap();
        assert_eq!(values, [("blob".to_owned(), "(binary, base64) //4AgA==".to_owned())]);
    }

    fn tee_host(allow: bool) -> (MockHost, Runs) {
        let (host, runs) = secret_host(allow, "apiVersion: v1\nkind: Secret\ndata:\n  p: cGFzcw==\n".into(), "");
        let host = host.with_capabilities(&["process:kubectl", "process:tee", "editor.read", "storage"]);
        (host, runs)
    }

    #[test]
    fn yaml_of_a_secret_is_refused_when_reveal_is_off() {
        let (host, runs) = tee_host(false);
        let mut kube = selected_secret();
        kube.button("yaml").unwrap();
        kube.open_yaml("secrets", "db", None).unwrap();
        assert!(runs.borrow().is_empty());
        assert!(host.process_runs().is_empty());
        assert!(kube.details.is_none());
        let notes = host.notifications();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].body.contains("allowRevealSecrets"));
    }

    #[test]
    fn yaml_of_a_secret_works_when_reveal_is_on() {
        let (host, runs) = tee_host(true);
        let mut kube = selected_secret();
        kube.button("yaml").unwrap();
        assert!(runs.borrow().iter().any(|(args, _)| args.contains(&"yaml".to_owned())));
        assert!(host.notifications().is_empty());
    }

    #[test]
    fn create_secret_does_not_overwrite_an_existing_secret_message() {
        let (_host, _) = secret_host(true, String::new(), "Error from server (AlreadyExists): secrets \"db\" already exists");
        let mut kube = drafting("db", "password", "hunter2");
        let error = format!("{:?}", kube.button("submit-secret").unwrap_err());
        assert!(error.contains("already exists") && !error.contains("hunter2"));
    }

    #[test]
    fn malformed_json_does_not_hang() {
        for bad in ["[1}", "{\"a\":[1}]}", "{\"data\":[}", "{\"a\":}", "{", "{\"a\":1,}x"] {
            let _ = secrets::data_entries(bad);
        }
        assert!(secrets::data_entries("{\"a\":[1}]}").is_err());
    }

    #[test]
    fn invalid_key_error_does_not_quote_the_key() {
        let error = secrets::manifest("db", &[("hunter2 secret!".into(), "v".into())]).unwrap_err();
        assert!(!error.contains("hunter2"));
        assert!(error.contains("letters, digits"));
    }

    #[test]
    fn turning_the_setting_off_hides_revealed_values() {
        let (host, _) = secret_host(true, secret_json(&[("password", b"hunter2")]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        assert!(matches!(kube.render_tree().unwrap().body, Body::Form(_)));
        host.set_setting("allowRevealSecrets", "false");
        let body = kube.render_tree().unwrap().body;
        assert!(!format!("{body:?}").contains("hunter2"));
        assert!(kube.revealed.is_none());
    }
}
