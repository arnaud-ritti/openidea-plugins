//! Kube Explorer for OpenIDEA: a tool window that browses a cluster with
//! `kubectl` (contexts, namespaces, workloads, config, storage, nodes) and
//! shows `describe` / diff output in place, YAML in an editor that applies on
//! save, terminals for logs, shells, attach and port forwards, commands for
//! the open manifest (apply, dry-run, diff, delete), Kustomize and Helm.
//!
//! Every cluster call is a `kubectl` run with the chosen context and
//! namespace as flags, so the user's kubeconfig is never changed.

pub(crate) const KUSTOMIZE_MARKER: &str = "icons/marker-kustomize.svg";
pub(crate) const LABEL_MARKER: &str = "icons/marker-label.svg";

mod ui_group;
mod chart_actions;
mod checks;
mod complete;
mod kustomize;
mod helm_values;
mod kustomize_graph;
mod crd_schema;
mod crd_export;
mod helm_assistance;
mod json_assistance;
mod label_refs;
mod lru;
mod cluster_completion;
mod watch;
mod process_scope;
mod context_actions;
mod services;
mod rich_controls;
mod yaml_editor;
mod config;
mod convert;
mod forward;
mod helm;
mod indexer;
mod kinds;
mod kubeconfig_import;
mod logs;
mod manifest;
mod navigation;
mod prompts;
mod reload;
mod schema;
mod secrets;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use openidea_plugin::providers::{CodeAction, CompletionItem, Document, Providers};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Context, Error, Event, EventKind, FileDiff, Invocation, LogLevel, NotificationAction, NotificationLevel, PickItem, Plugin, Position, Range, StatusItem, StatusMenuEntry, host};

pub const TREE: &str = "kube";
const PANEL: &str = "resource-details";
/// Native typed views consume viewChanged; packaged pages consume this content-free
/// invalidation and request their own frame/project-correlated ui-kit model.
fn refresh_panel() {
    crate::ui_group::refresh_view(PANEL);
}

fn refresh_views() {
    crate::ui_group::refresh_view(TREE);
    refresh_panel();
}

const KUBECTL: &str = "kubectl";
/// Status bar item naming the context and namespace kubectl calls go to.
const CONTEXT_STATUS: &str = "kube-context";

/// The context item's menu: switching first, then the cluster-wide commands.
fn status_menu() -> Vec<StatusMenuEntry> {
    vec![
        StatusMenuEntry::action("Switch Context…", "tree-switch").with_argument("ctx"),
        StatusMenuEntry::action("Switch Namespace…", "tree-switch").with_argument("ns"),
        StatusMenuEntry::Separator,
        StatusMenuEntry::action("Show Cluster Tree", SHOW_TREE),
        StatusMenuEntry::action("Start Resource Watch…", "watch-resources"),
        StatusMenuEntry::action("Apply Copied YAML…", "apply-copied-yaml"),
        StatusMenuEntry::action("Choose Kubeconfig", "choose-kubeconfig"),
    ]
}
/// Reveals the cluster tree: the status item's and Apply notification's action.
const SHOW_TREE: &str = "show-tree";
/// The context shown when kubectl runs but no current context is set.
const NO_CONTEXT: &str = "(no current context)";
const KUBECTL_UNAVAILABLE: &str = "(kubectl unavailable)";
const POD_SHELL: &str = "command -v bash >/dev/null && exec bash || exec sh";
const HELM: &str = "helm";
const DEFAULT_DEBUG_IMAGE: &str = "busybox:stable";
/// Keeps reads under the 5 s call budget on an unreachable cluster.
const REQUEST_TIMEOUT: &str = "--request-timeout=4s";
/// Shows `describe`, YAML and diff output, highlighted; YAML can be edited.
const PAGE: &str = include_str!("page.html");
/// Shown in the YAML panel when the IDE blocks the page's script, which Apply and Save to File need.
const NO_SCRIPTS: &str = "<p id=\"nojs\">Apply and Save to File don't work in this panel: this version of the IDE blocks scripts in plugin pages. \
    For supported built-ins, enable YAML in Editor and use the explicit Review Private YAML Apply and Apply Private YAML commands. Saving is local only.</p>";

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
    "apps",
    "batch",
    "autoscaling",
    "policy",
    "networking.k8s.io",
    "events.k8s.io",
    "storage.k8s.io",
    "rbac.authorization.k8s.io",
    "coordination.k8s.io",
    "discovery.k8s.io",
    "apiextensions.k8s.io",
    // Served by the metrics-server add-on; read-only node and pod metrics, not user-managed.
    "metrics.k8s.io",
    "scheduling.k8s.io",
    "node.k8s.io",
    "certificates.k8s.io",
    "admissionregistration.k8s.io",
    "flowcontrol.apiserver.k8s.io",
    "apiregistration.k8s.io",
    "internal.apiserver.k8s.io",
    "resource.k8s.io",
    "storagemigration.k8s.io",
    "authentication.k8s.io",
    "authorization.k8s.io",
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
    process_scope: process_scope::Binding,
    resource_scope: process_scope::Resources,
    navigation: navigation::Cache,
    label_refs: label_refs::Cache,
    kustomize_paths: kustomize::Cache,
    kustomize_graph: kustomize_graph::Cache,
    crd_schema: crd_schema::Cache,
    helm_assistance: helm_assistance::Cache,
    cluster_completion: cluster_completion::Cache,
    watch: watch::Watch,
    yaml_editors: yaml_editor::Editors,
    /// `None`: the kubeconfig's current context / the context's namespace.
    context: Option<String>,
    namespace: Option<String>,
    current_context: Option<String>,
    /// The context lookup failed for another reason than a denied permission.
    kubectl_failed: bool,
    /// Children of the tree's expandable keys (`ctx`, `ns`, `kind:<kind>`).
    children: BTreeMap<String, Result<Vec<String>, String>>,
    expanded: BTreeSet<String>,
    /// Synthetic Services parent only; no resource identity or discovery.
    cluster_collapsed: bool,
    /// Kind and row of the selected resource: its name, or `ns/name` in all namespaces.
    selected: Option<(String, String)>,
    confirm_delete: bool,
    /// Shown instead of the tree.
    details: Option<Details>,
    details_nonce: u64,
    rich_callback: Option<String>,
    rich_fields: std::cell::RefCell<BTreeMap<String,String>>,
    /// Owner of cached rich details / Secret plaintext and drafts, never anonymous.
    rich_owner: Option<String>,
    rich_scope: (Option<String>, Option<String>, Option<String>),
    /// Port forward being set up (shown instead of the tree): namespace, target and ports.
    forward: Option<forward::Draft>,
    forward_session: Option<forward::Session>,
    show_forward_session: bool,
    /// The manifest whose resources the shown dry run would delete.
    pending_delete: Option<String>,
    /// A revealed Secret: name and decoded values. Dropped by Back.
    revealed: Option<(String, Vec<(String, String)>)>,
    /// A Secret being created. Dropped by Back and by submitting.
    draft: Option<Draft>,
    /// A log snapshot shown instead of the tree.
    log_view: Option<logs::LogView>,
    /// A Secret being edited. Dropped by Back and by saving.
    editing_secret: Option<secrets::SecretEdit>,
    /// Chosen by Debug Resource's prompt, used by the next debug.
    debug_image: Option<String>,
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
    callback: String,
    title: String,
    text: String,
    /// `yaml`, `diff` or `text` (highlighting).
    lang: &'static str,
    /// Kind and row (as in `selected`) of the object an editable YAML is of.
    editing: Option<(String, String)>,
    /// What the last apply printed.
    note: Option<String>,
}

impl KubeExplorer {
    fn all_namespaces(&self) -> bool {
        self.namespace.as_deref() == Some(ALL_NAMESPACES)
    }

    /// Arguments of a list or manifest call: no namespace for all namespaces.
    fn args(&self, rest: &[&str]) -> Vec<String> {
        self.args_in(self.namespace.as_deref().filter(|ns| *ns != ALL_NAMESPACES), rest)
    }

    fn args_in(&self, namespace: Option<&str>, rest: &[&str]) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(path) = config::kubeconfig() {
            args.push(format!("--kubeconfig={path}"));
        }
        if let Some(context) = &self.context {
            args.push(format!("--context={context}"));
        }
        if let Some(namespace) = namespace {
            args.push(format!("--namespace={namespace}"));
        }
        args.extend(rest.iter().map(|s| (*s).to_owned()));
        args
    }

    /// The namespace and bare name a row's calls run with. In all namespaces a row is `ns/name`, or a
    /// bare name for a cluster-scoped kind; both parts must be Kubernetes names, so neither reaches
    /// kubectl as an option or a path, and no row falls back to the kubeconfig's namespace.
    fn split_scope(&self, kind: &str, row: &str) -> Result<(Option<String>, String), Error> {
        if !self.all_namespaces() {
            if row.is_empty() || row.starts_with('-') || matches!(row,"."|"..") || row.chars().any(|c|c.is_control()||c.is_whitespace()||matches!(c,'/'|'%'|',')) {
                return Err(format!("Refusing the name \"{row}\": it is not a single resource name.").into());
            }
            return Ok((self.namespace.clone(), row.to_owned()));
        }
        let cluster_scoped = CLUSTER_SCOPED.contains(&kind);
        match row.split_once('/') {
            None if cluster_scoped && is_dns_subdomain(row) => Ok((None, row.to_owned())),
            Some((namespace, name)) if !cluster_scoped && is_dns_label(namespace) && is_dns_subdomain(name) => {
                Ok((Some(namespace.to_owned()), name.to_owned()))
            }
            _ => Err(format!(
                "Refusing {kind} \"{row}\": in All namespaces a row needs a valid namespace and name. Pick its namespace to act on it."
            )
            .into()),
        }
    }

    /// `kubectl debug` for a pod; names reach kubectl as arguments only, so one that
    /// looks like an option is refused.
    fn debug_args(&self, row: &str, image: &str, target: Option<&str>) -> Result<Vec<String>, String> {
        let (namespace, pod) = self.split_scope("pods", row).map_err(|e| e.to_string())?;
        let pod = pod.as_str();
        let named = [("pod", pod), ("image", image)].into_iter().chain(target.map(|t| ("container", t)));
        for (what, value) in named {
            if value.is_empty() || value.starts_with('-') || value.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!("Refusing to debug: the {what} name \"{value}\" is empty, starts with '-' or has whitespace."));
            }
        }
        let image = format!("--image={image}");
        let target = target.map(|t| format!("--target={t}"));
        let mut rest = vec!["debug", "-it", pod, image.as_str()];
        rest.extend(target.as_deref());
        Ok(self.args_in(namespace.as_deref(), &rest))
    }

    fn debug_pod(&mut self, row: &str) -> Result<(), Error> {
        let setting = host::setting_string("debugImage").filter(|i| !i.trim().is_empty());
        let image = self.debug_image.take().or(setting);
        let image = image.as_deref().map_or(DEFAULT_DEBUG_IMAGE, str::trim);
        self.debug_args(row, image, None)?;
        let (namespace, pod) = self.split_scope("pods", row)?;
        let jsonpath = ["get", "pod", &pod, "-o", "jsonpath={.spec.containers[0].name}", REQUEST_TIMEOUT];
        let first = self.kubectl_in(namespace.as_deref(), &jsonpath, None);
        let target = first.as_deref().map(str::trim).ok().filter(|t| !t.is_empty());
        let args = self.debug_args(row, image, target)?;
        host::open_terminal(&format!("Debug: {pod}"), KUBECTL, &args)?;
        self.remember_debug_image(image);
        Ok(())
    }

    fn reveal_allowed() -> bool {
        host::setting_bool("allowRevealSecrets") == Some(true)
    }

    /// Off by default: private source-owned copy, with explicit reviewed apply.
    fn yaml_in_editor() -> bool {
        host::setting_bool("yamlInEditor") == Some(true)
    }

    /// True when the secret's data may be read; otherwise tells the user how to allow it.
    fn secret_gate(kind: &str) -> bool {
        if kind != "secrets" || Self::reveal_allowed() {
            return true;
        }
        host::notify(
            NotificationLevel::Warning,
            "Secret values are hidden",
            "To show them, turn on \"Allow Show Secret and YAML on a secret\" in Settings › Plugins › Kube Explorer.",
        );
        false
    }

    fn expire_secret_visibility(&mut self)->bool {
        if Self::reveal_allowed(){return false;}
        let mut changed=self.revealed.take().is_some()|self.editing_secret.take().is_some();
        if self.details.as_ref().is_some_and(|details|details.editing.as_ref().is_some_and(|(kind,_)|kind=="secrets")) {
            self.details=None;changed=true;
        }
        if changed {crate::refresh_panel();}
        changed
    }

    pub(crate) fn clear_panel_data(&mut self) {
        self.details=None;self.pending_delete=None;self.revealed=None;
        self.draft=None;self.log_view=None;self.editing_secret=None;
    }
    pub(crate) fn ensure_panel_replace(&self)->Result<(),Error> {
        if self.editing_secret.as_ref().is_some_and(|edit|edit.is_dirty())
            || self.draft.as_ref().is_some_and(|draft|!draft.name.is_empty()||!draft.key.is_empty()||!draft.value.is_empty()||!draft.entries.is_empty()) {
            crate::refresh_panel();
            return Err(Error::Invalid("Save or close the current Secret form first. Unsaved values were retained.".into()));
        }
        Ok(())
    }
    pub(crate) fn begin_panel(&mut self)->Result<String,Error> {
        self.ensure_panel_replace()?;
        self.clear_panel_data();self.forward=None;
        self.show_forward_session=false;Ok(self.new_rich_callback())
    }

    fn has_rich_cache(&self) -> bool {
        self.details.is_some() || self.revealed.is_some() || self.editing_secret.is_some()
            || self.draft.is_some() || self.pending_delete.is_some()
    }

    fn expire_rich_context(&mut self, ctx: Option<&Invocation>) -> bool {
        let hidden=self.expire_secret_visibility();
        let current = forward::owner(ctx).ok().map(|(_, root)| root);
        let scope = (self.context.clone(), self.namespace.clone(), config::kubeconfig());
        let stale = self.has_rich_cache() && (current.is_none() || self.rich_owner != current || self.rich_scope != scope);
        if stale {
            self.details = None; self.revealed = None; self.editing_secret = None;
            self.draft = None; self.pending_delete = None;
            crate::refresh_panel();
        }
        if !self.has_rich_cache() { self.rich_owner = current; self.rich_scope = scope; }
        stale||hidden
    }

    fn show_secret(&mut self, row: &str) -> Result<(), Error> {
        if !Self::secret_gate("secrets") {
            return Ok(());
        }
        let (namespace, name) = self.split_scope("secrets", row)?;
        let json = self.kubectl_in(namespace.as_deref(), &["get", "secret", &name, "-o", "json", REQUEST_TIMEOUT], None).map_err(|error|secrets::command_error("read",error))?;
        secrets::validate_response(&json,&name,namespace.as_deref(),false)?;
        let entries = secrets::data_entries(&json)?;
        let values = entries.iter().map(|(key, bytes)| (key.clone(), secrets::display(bytes))).collect();
        self.begin_panel()?;
        self.revealed = Some((row.to_owned(), values));
        self.details = None;
        crate::refresh_views();
        Ok(())
    }

    /// Writes without a row's own namespace (Create Secret, manifest apply and delete) need one
    /// namespace: in all namespaces kubectl would send them to the kubeconfig's.
    fn single_namespace_gate(&self, action: &str) -> Result<(), Error> {
        if self.all_namespaces() {
            return Err(format!("{action} works in one namespace: pick a namespace first.").into());
        }
        Ok(())
    }

    fn create_secret(&mut self) -> Result<(), Error> {
        self.single_namespace_gate("Create Secret")?;
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
        crate::refresh_views();
        let out = self.process(KUBECTL, &self.args(&["create", "-f", "-"]), Some(&manifest)).map_err(|error|secrets::command_error("create",error))?;
        if out.status != Some(0) {
            if out.stderr.contains("AlreadyExists") || out.stderr.contains("already exists") {
                return Err(format!("A secret named \"{name}\" already exists; Create Secret does not overwrite it.").into());
            }
            return Err(secrets::command_failure("create",out.status));
        }
        config::notify_success("Secret created", &format!("{name} in {}", self.target()));
        self.reload();
        Ok(())
    }

    fn render_revealed(&self, name: &str, values: &[(String, String)]) -> View {
        // Pinned native Multiline fields clip at1MiB. Numbered parts preserve
        // the whole shown value, including large binary base64 and Unicode.
        let mut split=false;
        let body = if values.is_empty() {Body::Message("This secret has no data.".into())}else{
            let mut fields=Vec::new();
            for (index,(key,value)) in values.iter().enumerate() {
                let mut parts=Vec::new();let mut start=0;
                while start<value.len() {
                    let mut end=(start+(1<<20)).min(value.len());
                    while !value.is_char_boundary(end){end-=1;}
                    parts.push(&value[start..end]);start=end;
                }
                if parts.is_empty(){parts.push("");}
                split|=parts.len()>1;
                for (part,text) in parts.iter().enumerate() {
                    let (id,label)=if parts.len()==1 {(format!("revealed-{index}"),key.clone())}else{
                        (format!("revealed-{index}-{part}"),format!("{key} (part {} of {}; join without separators)",part+1,parts.len()))
                    };
                    fields.push(Field::new(&id,&label,FieldValue::Multiline((*text).into())));
                }
            }
            Body::Form(fields)
        };
        let note=if split {"; numbered parts contain one value: join each value's parts in order without separators"}else{""};
        View::new(body)
            .status(&format!("Secret {name}: values are shown here only{note}; Back clears them"))
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
        self.run_kubectl(self.args(rest), stdin)
    }

    fn kubectl_in(&self, namespace: Option<&str>, rest: &[&str], stdin: Option<&str>) -> Result<String, Error> {
        self.run_kubectl(self.args_in(namespace, rest), stdin)
    }

    fn run_kubectl(&self, args: Vec<String>, stdin: Option<&str>) -> Result<String, Error> {
        let out = self.process(KUBECTL, &args, stdin)?;
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
        if !resource_type_name(kind) {return Err("Unsupported resource type from discovery; lookup was not run.".into());}
        let label = KINDS.iter().find(|(k, ..)| *k == kind).map_or(kind.to_owned(), |(_, label, _)| label.to_lowercase());
        let _task = host::begin_task(&format!("Loading {label}"), false);
        if self.all_namespaces() && !CLUSTER_SCOPED.contains(&kind) {
            let out = self.kubectl(&["get", kind, "--all-namespaces", "-o", ALL_ROWS, REQUEST_TIMEOUT], None)?;
            return Ok(out.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect());
        }
        let out = self.kubectl(&["get", kind, "-o", "name", REQUEST_TIMEOUT], None)?;
        let names: Vec<String> = out.lines().map(|line| line.split_once('/').map_or(line, |(_, name)| name).to_owned()).collect();
        Ok(if kind == "namespaces" { self.visible_namespaces(names) } else { names })
    }

    /// `kind.group` names of the cluster's custom resources.
    fn custom_resources(&self) -> Result<Vec<String>, String> {
        let _task = host::begin_task("Loading custom resources", false);
        let out = self.kubectl(&["api-resources", "-o", "name", "--verbs=list", REQUEST_TIMEOUT], None)?;
        if out.len()>256<<10 {return Err("Resource discovery exceeds its 256 KiB bound; no inventory was accepted.".into());}
        let names:Vec<_>=out.lines().filter(|line|!line.is_empty()).take(4097).map(str::to_owned).collect();
        if names.len()>4096||names.iter().any(|name|!resource_type_name(name)) {
            return Err("Unsupported or oversized resource discovery; no resource-type inventory was accepted.".into());
        }
        Ok(custom_resource_names(names))
    }

    /// Drops cached children and reloads those of expanded nodes; only an unreachable cluster stops the rest.
    fn reload(&mut self) {
        self.watch.settings_changed(&self.context,&self.namespace);
        self.cluster_completion.clear();
        self.children.clear();
        self.current_context = None;
        self.kubectl_failed = false;
        self.update_context_status();
        let mut first_failure: Option<String> = None;
        for key in self.expanded.clone() {
            let loaded = match &first_failure {
                Some(failure) => Err(failure.clone()),
                None => self.load(&key),
            };
            if let Err(failure) = &loaded {
                if connection_failure(failure) {
                    first_failure.get_or_insert_with(|| failure.clone());
                }
            }
            self.children.insert(key, loaded);
        }
        crate::refresh_views();
    }

    fn confirm_apply(&self, what: &str) -> Result<(), Error> {
        if host::confirm("Apply to the cluster", &self.apply_question(what))? {
            return Ok(());
        }
        Err(Error::Cancelled)
    }

    /// Shows `⎈ context / namespace` in the status bar, or nothing while the context is unknown.
    fn update_context_status(&self) {
        let context = self.context.as_deref().or(self.current_context.as_deref())
            .filter(|context| !context.is_empty() && ![NO_CONTEXT, KUBECTL_UNAVAILABLE].contains(context));
        let Some(context) = context else {
            host::remove_status_item(CONTEXT_STATUS);
            return;
        };
        let namespace = match self.namespace.as_deref() {
            Some(ALL_NAMESPACES) => "(all namespaces)",
            Some(namespace) => namespace,
            None => "(default)",
        };
        let item = |click: &str| StatusItem {
            text: format!("⎈ {context} / {namespace}"),
            tooltip: Some(format!("Kube Explorer runs kubectl against context {context}, namespace {namespace}. {click}")),
            command: Some(SHOW_TREE.into()),
        };
        // Older IDEs refuse the menu: the item then shows the cluster tree, as before.
        if host::set_status_item_menu(CONTEXT_STATUS, &item("Click to switch."), &status_menu()).is_err() {
            host::set_status_item(CONTEXT_STATUS, &item("Click to show the cluster tree and switch."));
        }
    }

    /// Switches from a picker over the contexts (`ctx`), the namespaces (`ns`) or both (`None`):
    /// Switch Context or Namespace from the palette or the status bar.
    fn pick_context_or_namespace(&mut self, scope: Option<&str>) -> Result<(), Error> {
        let mut items = Vec::new();
        if scope != Some("ns") {
            let current = self.context.as_deref().or(self.current_context.as_deref());
            for name in self.load("ctx")? {
                let note = if Some(name.as_str()) == current { "current context" } else { "context" };
                items.push(PickItem::new(&format!("ctx/{name}"), &name).description(note));
            }
        }
        if scope != Some("ctx") {
            let note = |name: &str| if self.namespace.as_deref() == Some(name) { "current namespace" } else { "namespace" };
            items.push(PickItem::new(&format!("ns/{ALL_NAMESPACES}"), "All namespaces").description(note(ALL_NAMESPACES)));
            for name in self.load("ns")? {
                items.push(PickItem::new(&format!("ns/{name}"), &name).description(note(&name)));
            }
        }
        let title = match scope {
            Some("ctx") => "Switch Kubernetes context",
            Some("ns") => "Switch namespace",
            _ => "Switch context or namespace",
        };
        let Some(choice) = host::pick(title, &items)? else {
            return Ok(());
        };
        let (parent, name) = choice.split_once('/').ok_or(Error::Stale)?;
        self.switch_to(parent, name);
        Ok(())
    }

    /// Makes `name` the context (`parent` = `ctx`) or the namespace (`ns`) of every kubectl call.
    fn switch_to(&mut self, parent: &str, name: &str) {
        if parent == "ctx" {
            if self.context.as_deref() != Some(name) { self.retire_forward("Kubernetes context changed"); }
            self.context = Some(name.to_owned());
            self.namespace = None;
        } else {
            if self.namespace.as_deref() != Some(name) { self.retire_forward("Kubernetes namespace changed"); }
            self.namespace = Some(name.to_owned());
        }
        self.selected = None;
        self.save();
        self.reload();
    }

    /// Kube Explorer: Apply Copied YAML…: applies the Kubernetes manifest on the clipboard after
    /// naming its resources and the target, with the same Apply / Show Diff first choice as the
    /// open-file Apply.
    fn apply_copied_yaml(&mut self) -> Result<String, Error> {
        const FEATURE: &str = "Apply Copied YAML";
        self.single_namespace_gate(FEATURE)?;
        let clipboard = host::read_clipboard().map_err(|error| ui_kit::denied::map(error, FEATURE, "clipboard.read", "Kube Explorer"))?;
        let manifest = clipboard
            .text
            .filter(|text| !text.trim().is_empty())
            .ok_or("The clipboard holds no text. Copy a Kubernetes manifest, then run Kube Explorer: Apply Copied YAML… again.")?;
        if manifest.len() > 512 << 10 {
            return Err("The copied text is over 512 KiB. Save it to a file and run Kube Explorer: Apply Manifest to Cluster.".into());
        }
        let docs = manifest::documents(&manifest);
        if docs.is_empty() || !docs.iter().all(manifest::Doc::is_manifest) {
            return Err("The clipboard doesn't hold a Kubernetes manifest: each YAML document needs apiVersion and kind.".into());
        }
        let resources: Vec<String> = docs
            .iter()
            .map(|doc| {
                let kind = doc.kind.as_ref().map_or("", |kind| kind.text.as_str());
                format!("{kind}/{}", doc.name.as_ref().map_or("(no name)", |name| name.text.as_str()))
            })
            .collect();
        let what = match resources.len() {
            1..=3 => resources.join(", "),
            count => format!("{count} resources ({}, …)", resources[..2].join(", ")),
        };
        let choices = [PickItem::new("apply", "Apply"), PickItem::new("diff", "Show Diff with Cluster First")];
        match host::pick(&self.apply_question(&what), &choices)?.as_deref() {
            Some("apply") => {}
            Some("diff") => {
                let shown = self.diff_manifest(&manifest, "copied YAML")?;
                if !host::confirm(&format!("Apply {what}?"), &format!("{shown}\n{}", self.apply_question(&what)))? {
                    return Err(Error::Cancelled);
                }
            }
            _ => return Err(Error::Cancelled),
        }
        let _task = host::begin_task("Applying copied YAML", false);
        let out = self.kubectl(&["apply", "-f", "-"], Some(&manifest))?;
        let target = self.target();
        self.reload();
        Ok(format!("{target}: {}", out.trim()))
    }

    /// One terminal per action and target: running it again focuses the open one.
    fn open_pod_terminal(&self, action: &str, title: &str, args: Vec<String>) -> Result<(), Error> {
        let context = self.context.as_deref().or(self.current_context.as_deref()).unwrap_or_default();
        let key = terminal_key(action, context, &args);
        let command = host::Command::new(KUBECTL).args(&args);
        // A window without a project may have no keyed terminal; fall back to a plain one.
        host::open_terminal_session(&key, title, &command).map(drop).or_else(|_| host::open_terminal(title, KUBECTL, &args))
    }

    #[cfg(test)]
    fn shell_terminal_key(&self, namespace: Option<&str>, pod: &str) -> String {
        let context = self.context.as_deref().or(self.current_context.as_deref()).unwrap_or_default();
        terminal_key("shell", context, &self.args_in(namespace, &["exec", "-it", pod, "--", "sh", "-c", POD_SHELL]))
    }

    /// Opens the live objects next to the open manifest; Secrets stay masked unless Show Secret is allowed.
    fn diff_manifest(&mut self, manifest: &str, path: &str) -> Result<String, Error> {
        if !Self::reveal_allowed() && !secret_free(manifest) {
            let out = self.process(KUBECTL, &self.args(&["diff", "-f", "-"]), Some(manifest))?;
            return match out.status {
                Some(0) => Ok("No differences with the cluster.".into()),
                Some(1) => {
                    self.show(format!("Diff: {path}"), out.stdout, "diff")?;
                    Ok("The differences are shown in Kube Explorer Details (Secret values masked).".into())
                }
                status => Err(failure(KUBECTL, &out.stderr, status)),
            };
        }
        let live = self.kubectl(&["get", "-f", "-", "-o", "yaml", "--ignore-not-found", REQUEST_TIMEOUT], Some(manifest))?;
        let planned = self.process(KUBECTL, &self.args(&DIFF_DRY_RUN), Some(manifest))?;
        let name = path.rsplit('/').next().unwrap_or(path);
        let new = live.trim().is_empty();
        let (right, right_label, fallback) = match planned.status {
            Some(0) => (cluster_objects(&planned.stdout).unwrap_or(planned.stdout), format!("After apply, server dry run ({name})"), String::new()),
            // Validation, an admission webhook or read-only RBAC can refuse the dry run; the file still compares.
            _ => {
                let local = convert::source_documents(manifest).and_then(normalized_objects).unwrap_or_else(|| manifest.to_owned());
                let reason = planned.stderr.lines().find(|line| !line.trim().is_empty()).unwrap_or("no details").trim().to_owned();
                (local, format!("Local ({name})"), format!(" The server dry run failed ({reason}), so the file is shown as written."))
            }
        };
        let diff = FileDiff {
            left: (!new).then(|| cluster_objects(&live).unwrap_or(live)),
            right: Some(right),
            left_label: Some(format!("Cluster ({})", self.target())),
            right_label: Some(right_label),
            title: Some(format!("{name}: cluster and local")),
        };
        host::open_file_diff(&diff, Some(name))?;
        let opened = if new { "These resources are not in the cluster yet." } else { "The diff with the cluster is open." };
        Ok(format!("{opened}{fallback}"))
    }

    /// `context/namespace` the cluster calls go to, for messages.
    fn target(&self) -> String {
        let (context, namespace) = self.target_parts();
        format!("{context}/{namespace}")
    }

    fn target_parts(&self) -> (String, &str) {
        let current = || self.kubectl(&["config", "current-context"], None).ok().map(|out| out.trim().to_owned());
        let context = self.context.clone().or_else(|| self.current_context.clone()).or_else(current);
        let context = context.unwrap_or_else(|| KUBECTL_UNAVAILABLE.into());
        let namespace = match self.namespace.as_deref() {
            Some(ALL_NAMESPACES) => "(all namespaces)",
            Some(namespace) => namespace,
            None => "(default)",
        };
        (context, namespace)
    }

    /// The question asked before applying `what` (a file or Kustomization directory).
    fn apply_question(&self, what: &str) -> String {
        let (context, namespace) = self.target_parts();
        format!("Apply {what} to context {context}, namespace {namespace}?")
    }

    /// The question asked before deleting a resource row: kind, name, namespace and context.
    fn delete_question(&self, kind: &str, row: &str) -> Result<String, Error> {
        let (namespace, name) = self.split_scope(kind, row)?;
        let context = self.context.as_ref().or(self.current_context.as_ref()).map_or("the current context", String::as_str);
        let scope = match namespace.as_deref() {
            _ if CLUSTER_SCOPED.contains(&kind) => String::new(),
            Some(namespace) => format!(" in namespace {namespace}"),
            None => " in the default namespace".into(),
        };
        Ok(format!("Delete {kind}/{name}{scope} on context {context}?"))
    }

    /// Asks before `kubectl replace` overwrites the live object; the body names kind, name, namespace and context.
    fn confirm_replace(&self, kind: &str, row: &str) -> Result<(), Error> {
        let question = self.delete_question(kind, row)?.replacen("Delete ", "Replace the live ", 1);
        let (_, name) = self.split_scope(kind, row)?;
        if host::confirm(&format!("Replace {kind}/{name}?"), &format!("{question}\nYour edits overwrite the object in the cluster; the previous version is not kept."))? {
            return Ok(());
        }
        Err(Error::Cancelled)
    }

    fn save(&self) {
        for (key, value) in [("context", &self.context), ("namespace", &self.namespace)] {
            let _ = match value {
                Some(v) => host::storage::set(key, v),
                None => host::storage::delete(key),
            };
        }
    }

    fn show(&mut self, title: String, text: String, lang: &'static str)->Result<(),Error> {
        self.begin_panel()?;
        self.details = Some(Details { title, text, lang, editing: None, note: None, callback:String::new() });
        self.show_forward_session = false;
        self.forward = None;
        self.pending_delete = None;
        crate::refresh_views();
        Ok(())
    }

    fn resource_yaml_text(&self,kind:&str,name:&str,namespace:Option<&str>)->Result<String,Error> {
        let read=self.kubectl_in(namespace,&["get",kind,name,"-o","yaml",REQUEST_TIMEOUT],None);
        if kind=="secrets" {read.map_err(|error|secrets::command_error("read",error))}else{read}
    }

    /// Private editor for supported source-owned built-ins; otherwise the panel.
    /// Secrets never go to a file. Save alone never executes a cluster mutation.
    fn edit_yaml(&mut self, kind: &str, row: &str) -> Result<(), Error> {
        if !Self::secret_gate(kind) { return Ok(()); }
        let (namespace, name) = self.split_scope(kind, row)?;
        if kind != "secrets" && Self::yaml_in_editor() {
            if let Some(root) = self.rich_owner.clone() {
                match self.yaml_editors.open(&root,self.context.as_deref(),namespace.as_deref(),kind,&name) {
                    Ok(()) => return Ok(()),
                    Err(_) => host::notify(NotificationLevel::Info,"Edit YAML","This resource can't be edited in the editor, so it opens in Kube Explorer Details. Nothing was applied."),
                }
            }
        }
        let _task = host::begin_task(&format!("Loading {kind}/{row}"), false);
        let text = self.resource_yaml_text(kind,&name,namespace.as_deref())?;
        self.show_yaml(kind,row,text,None)?; Ok(())
    }

    fn open_yaml(&mut self, kind: &str, row: &str, note: Option<String>) -> Result<(), Error> {
        if !Self::secret_gate(kind) {
            return Ok(());
        }
        let (namespace, name) = self.split_scope(kind, row)?;
        let _task = host::begin_task(&format!("Loading {kind}/{row}"), false);
        let text = self.resource_yaml_text(kind,&name,namespace.as_deref())?;
        self.show_yaml(kind, row, text, note)?;
        Ok(())
    }

    fn describe(&mut self, kind: &str, row: &str) -> Result<(), Error> {
        let (namespace, name) = self.split_scope(kind, row)?;
        let _task = host::begin_task(&format!("Describing {kind}/{row}"), false);
        let text = self.kubectl_in(namespace.as_deref(), &["describe", kind, &name, REQUEST_TIMEOUT], None)?;
        self.show(format!("{kind}/{row}"), text, "yaml")?;
        Ok(())
    }

    fn show_yaml(&mut self, kind: &str, name: &str, text: String, note: Option<String>)->Result<(),Error> {
        let editing = Some((kind.to_owned(), name.to_owned()));
        let callback=self.begin_panel()?;
        self.details = Some(Details { title: format!("{kind}/{name}"), text, lang: "yaml", editing, note, callback });
        crate::refresh_views();
        Ok(())
    }

    /// Replaces the edited object; on failure the page keeps the edits and shows why.
    fn apply_yaml(&mut self, yaml: &str) -> Result<(), Error> {
        let Some((kind, row)) = self.details.as_ref().and_then(|d| d.editing.clone()) else {
            return Ok(());
        };
        if kind=="secrets"&&!Self::reveal_allowed(){self.expire_secret_visibility();return Err(Error::Stale);}
        let (namespace, _) = self.split_scope(&kind, &row)?;
        self.confirm_replace(&kind, &row)?;
        let _task = host::begin_task(&format!("Applying {kind}/{row}"), false);
        let result=self.kubectl_in(namespace.as_deref(), &["replace", "-f", "-"], Some(yaml));
        let result=if kind=="secrets" {result.map_err(|error|secrets::command_error("save",error))}else{result};
        match result {
            Ok(out) => self.open_yaml(&kind, &row, Some(out.trim().to_owned())),
            Err(e) => crate::ui_group::post_view_message(PANEL, &format!("error:{e}")),
        }
    }

    fn render_details(&self, details: &Details) -> View {
        let page = fill(
            PAGE,
            &[
                ("callback", &details.callback),
                ("lang", details.lang),
                ("name", &escape_html(&details.title.replace('/', "-"))),
                ("readonly", if details.editing.is_some() { "" } else { "readonly" }),
                ("nojs", if details.editing.is_some() { NO_SCRIPTS } else { "" }),
                ("highlighted", &highlight(&details.text, details.lang)),
                ("text", &escape_html(&details.text)),
            ],
        );
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

    fn start_port_forward(&mut self, kind: &str, row: &str, ctx: Option<&Invocation>) -> Result<(), Error> {
        let Some((_, ports_path)) = FORWARDABLE.iter().find(|(k, _)| *k == kind) else {
            return Ok(());
        };
        let mut draft = self.prepare_forward(kind, row, ctx)?;
        let jsonpath = format!("jsonpath={ports_path}");
        let found = self.forward_ports(&draft, &jsonpath)?;
        // A port served over TCP and UDP is listed twice; forwarding it twice fails.
        let mut ports: Vec<&str> = Vec::new();
        for port in found.split_whitespace() {
            if !ports.contains(&port) {
                ports.push(port);
            }
        }
        let ports = ports.iter().map(|p| format!("{p}:{p}")).collect::<Vec<_>>().join(" ");
        draft.ports = ports;
        self.begin_panel()?;
        self.forward = Some(draft);
        self.show_forward_session = false;
        self.details = None;
        crate::refresh_views();
        Ok(())
    }

    fn render_forward(&self, target: &str, ports: &str) -> View {
        let field = Field::new("ports", "Ports (local:remote, separated by spaces)", FieldValue::Text(ports.into()));
        let owner = self.forward.as_ref().and_then(|draft| draft.owner_root.as_deref());
        let managed = match owner {
            Some(root) => format!("Start Managed Session runs it for the project {root} and stops it when you switch project, context, namespace or resource, after about five idle minutes, or when the window closes."),
            None => "Start Managed Session needs an open project and the project.read permission.".to_owned(),
        };
        let lifetime = Field::new(
            "forward-lifetime",
            "How long it runs",
            FieldValue::readonly_text(&format!("Start in Terminal keeps forwarding until you close the terminal. {managed}"), true),
        );
        View::new(Body::Form(vec![field, lifetime]))
            .status(&format!("Port forward to {target}"))
            .button(Button::new("back", "Back").icon("arrow-left"))
            .button(Button::new("start-forward", "Start in Terminal").icon("run"))
            .button(Button::new("start-managed-forward", "Start Managed Session").icon("run").enabled(owner.is_some()))
    }

    fn render_tree_in(&mut self, ctx: Option<&Invocation>) -> Result<View, Error> {
        if self.show_forward_session {
            let view=self.forward_status(ctx);return Ok(self.protect_rich_view(view));
        }
        if let Some(draft) = &self.forward {
            return Ok(self.protect_rich_view(self.render_forward(&draft.target, &draft.ports)));
        }
        if let Some(draft) = &self.draft {
            return Ok(self.protect_rich_view(self.render_draft(draft)));
        }
        self.expire_secret_visibility();
        if let Some(edit) = &self.editing_secret {
            return Ok(self.protect_rich_view(self.render_secret_edit(edit)));
        }
        if let Some(log_view) = &self.log_view {
            return Ok(self.protect_rich_view(log_view.render()));
        }
        if let Some((name, values)) = &self.revealed {
            return Ok(self.protect_rich_view(self.render_revealed(name, values)));
        }
        if let Some(details) = &self.details {
            return Ok(self.protect_rich_view(self.render_details(details)));
        }
        self.render_resources()
    }

    fn render_resources(&mut self) -> Result<View, Error> {
        if self.context.is_none() && self.current_context.is_none() {
            // A non-zero exit (no current context yet) means kubectl is installed: only a
            // failed spawn offers the install hint.
            self.current_context = match self.process(KUBECTL, &self.args(&["config", "current-context"]), None) {
                Ok(out) if out.status == Some(0) => Some(out.stdout.trim().to_owned()),
                Ok(_) => Some(NO_CONTEXT.into()),
                Err(Error::Denied(_)) => {
                    host::remove_status_item(CONTEXT_STATUS);
                    return Ok(View::new(Body::Tree(vec![
                        TreeItem::new("kube:status", "Kubernetes — permission required")
                            .icon(k8s!("cluster"))
                            .detail(&format!("{} Private import/review needs no connection.", ui_kit::denied::message("Browsing Kubernetes", "process:kubectl", "Kube Explorer"))),
                    ])));
                }
                Err(_) => {
                    self.kubectl_failed = true;
                    Some(KUBECTL_UNAVAILABLE.into())
                }
            };
        }
        self.update_context_status();
        let context = self.context.as_ref().or(self.current_context.as_ref()).cloned().unwrap_or_default();
        let namespace = match self.namespace.as_deref() {
            Some(ALL_NAMESPACES) => "All namespaces",
            Some(namespace) => namespace,
            None => "(default)",
        };

        let mut items = Vec::new();
        let imported = kubeconfig_import::selected_name().map(|name| format!(" · imported kubeconfig {name}")).unwrap_or_default();
        self.push_node(&mut items, "ctx", &format!("Context: {context}{imported}"), k8s!("cluster"), Some(&context));
        let at = items.len();
        self.push_node(&mut items, "ns", &format!("Namespace: {namespace}"), k8s!("ns"), self.namespace.as_ref());
        if self.expanded.contains("ns") {
            let all = TreeItem::new(&self.choice_key("ns",ALL_NAMESPACES), "All namespaces").parent("ns").icon(k8s!("ns"));
            items.insert(at + 1, if self.all_namespaces() { all.detail("active") } else { all });
        }
        for (kind, label, icon) in KINDS {
            self.push_node(&mut items, &format!("kind:{kind}"), label, icon, None);
        }

        self.push_custom_resources(&mut items);

        // Group only former roots; keep all existing child/resource keys and owners.
        let parent = TreeItem::new("kube:cluster", &format!("Kubernetes — {context}"))
            .icon(k8s!("cluster")).detail(&format!("Current context · namespace {namespace}. Refresh to update."))
            .expandable(!self.cluster_collapsed);
        if self.cluster_collapsed { items.clear(); }
        else {
            for item in &mut items {
                if item.parent.is_none() { item.parent = Some("kube:cluster".into()); }
            }
        }
        items.insert(0, parent);

        let mut view = View::new(Body::Tree(items)).status("Double-click a context or namespace to switch");
        if self.forward_session.is_some() {
            view = view.button(Button::new("forward-status", "Port Forward Status").icon("external-link"));
        }
        view = view.button(Button::new("refresh", "Refresh").icon("refresh"));
        if self.kubectl_failed {
            view = view.button(Button::new("how-to-install-kubectl", "How to Install kubectl").icon("help"));
        }
        if context == NO_CONTEXT {
            view = view.button(Button::new("choose-kubeconfig", "Choose Kubeconfig…").icon("folder-open"));
        }
        Ok(self.actions(view))
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
        self.protect_rich_view(view.button(Button::new("describe", "Describe").icon("info").enabled(self.selected.is_some()))
            .button(Button::new("yaml", "YAML").icon("file-yaml").enabled(self.selected.is_some()))
            .button(Button::new("logs", "Follow Logs (Terminal)").icon("file-log").enabled(logs))
            .button(Button::new("log-view", "Log Snapshot…").icon("list-filter").enabled(logs))
            .button(Button::new("shell", "Shell").icon("terminal").enabled(pod_selected))
            .button(Button::new("attach", "Attach").icon("attach-to-process").enabled(pod_selected))
            .button(Button::new("show-secret", "Show Secret").icon("eye").enabled(kind == "secrets"))
            .button(Button::new("edit-secret", "Edit Secret").icon("edit").enabled(kind == "secrets"))
            .button(Button::new("create-secret", "Create Secret").icon("add").enabled(!self.all_namespaces()))
            .button(Button::new("debug", "Debug").icon("debug").enabled(pod_selected))
            .button(Button::new("port-forward", "Port Forward").icon("external-link").enabled(forwardable))
            .button(Button::new("delete", &delete).icon("trash").enabled(self.selected.is_some())))
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
                    let child_key = match key.strip_prefix("kind:") {
                        Some(kind) => self.resource_key(kind,name),
                        None => self.choice_key(key,name),
                    };
                    if child_key.len()>1024 {
                        items.push(TreeItem::new(&format!("unavailable:{}",items.len()),"Name is too long for this view").parent(key).icon("error").detail("No action is available because this name is too long."));
                        continue;
                    }
                    let child = TreeItem::new(&child_key, name).parent(key).icon(icon);
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

    fn handle_tree(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Expand(key) if key == "kube:cluster" => {
                self.cluster_collapsed = false;
                crate::refresh_views();
            }
            ViewEvent::Collapse(key) if key == "kube:cluster" => {
                self.cluster_collapsed = true;
                crate::refresh_views();
            }
            ViewEvent::Expand(key) => {
                if !self.children.contains_key(&key) {
                    let loaded = self.load(&key);
                    self.children.insert(key.clone(), loaded);
                }
                self.expanded.insert(key);
                crate::refresh_views();
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
                crate::refresh_views();
            }
            ViewEvent::Select(key) => {
                let selected=if self.checked_choice(&key)?.is_some(){None}else{self.checked_resource(&key)?};
                if self.selected != selected { self.retire_forward("Resource selection changed"); }
                self.selected = selected;
                self.new_rich_callback();
                self.confirm_delete = false;
                crate::refresh_views();
            }
            ViewEvent::Activate(key) => match self.checked_choice(&key)? {
                Some((parent,name)) => self.switch_to(&parent, &name),
                _ => {
                    if let Some((kind, name)) = self.checked_resource(&key)? {
                        if self.rich_owner.is_none() { return Err(rich_details_unavailable()); }
                        self.describe(&kind, &name)?;
                    }
                }
            },
            ViewEvent::Button(button) => self.button_in(&button, Some(ctx))?,
            ViewEvent::Field(key, value) if key.starts_with("secret-") => {
                if let Some(edit) = &mut self.editing_secret {
                    edit.set_field(&key, value);
                } else if let Some(draft) = &mut self.draft {
                    match key.as_str() {
                        "secret-name" => draft.name = value,
                        "secret-key" => draft.key = value,
                        _ => draft.value = value,
                    }
                }
                crate::refresh_panel();
            }
            ViewEvent::Field(key, value) if key == "ports" => {
                if let Some(draft) = &mut self.forward {
                    draft.ports = value;
                }
                crate::refresh_panel();
            }
            ViewEvent::Message(message) => {
                if let Some(payload) = message.strip_prefix("apply\n") {
                    let (callback,yaml)=payload.split_once('\n').ok_or(Error::Stale)?;
                    let valid=self.details.as_ref().is_some_and(|d|d.editing.is_some()&&!d.callback.is_empty()&&d.callback==callback);
                    if !valid || self.rich_owner.as_deref() != forward::owner(Some(ctx)).ok().as_ref().map(|(_, root)| root.as_str()) {
                        return Err(Error::Stale);
                    }
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

    #[cfg(test)]
    fn test_apply_message(&self,yaml:&str)->String {format!("apply\n{}\n{yaml}",self.details.as_ref().expect("editable details").callback)}
    #[cfg(test)]
    fn button(&mut self, button: &str) -> Result<(), Error> { self.button_in(button, None) }
    #[cfg(test)]
    fn render_tree(&mut self) -> Result<View, Error> { self.render_tree_in(None) }

    fn button_in(&mut self, button: &str, ctx: Option<&Invocation>) -> Result<(), Error> {
        self.expire_forward_context(ctx);
        self.expire_log_context(ctx);
        if ctx.is_some() {
            self.expire_rich_context(ctx);
            if matches!(button, "show-secret" | "edit-secret" | "create-secret" | "describe" | "yaml") && self.rich_owner.is_none() {
                return Err(rich_details_unavailable());
            }
            let missing = match button {
                "add-secret-key" | "submit-secret" => self.draft.is_none(),
                "submit-secret-edit" => self.editing_secret.is_none(),
                "apply" | "save-file" | "reload-yaml" => self.details.is_none(),
                "confirm-delete-manifest" => self.pending_delete.is_none(),
                _ => false,
            };
            if missing { return Err(Error::Stale); }
        }
        if self.forward_button(button, ctx)? {
            return Ok(());
        }
        if button != "refresh" {
            self.show_forward_session = false;
        }
        if button == "refresh" {
            self.reload();
            return Ok(());
        }
        if button == "how-to-install-kubectl" {
            notify_install_hint();
            return Ok(());
        }
        if button == "choose-kubeconfig" {
            return self.choose_kubeconfig();
        }
        if button.starts_with("log-") && button != "log-view" {
            return self.log_button(button, ctx);
        }
        match button {
            "back" => {
                self.show_forward_session = false;
                self.details = None;
                self.forward = None;
                self.pending_delete = None;
                self.revealed = None;
                self.draft = None;
                self.editing_secret = None;
                self.log_view = None;
                crate::refresh_views();
                return Ok(());
            }
            "create-secret" => {
                self.single_namespace_gate("Create Secret")?;
                self.begin_panel()?;
                self.draft = Some(Draft::default());
                crate::refresh_views();
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
                    crate::refresh_views();
                }
                return Ok(());
            }
            "submit-secret" => return self.create_secret(),
            "submit-secret-edit" => return self.save_secret_edit(),
            "start-forward" => return self.start_terminal_forward(ctx),
            "start-managed-forward" => return self.start_managed_forward(ctx.ok_or_else(|| Error::Invalid("Managed forwarding needs a project invocation.".into()))?),
            "confirm-delete-manifest" => {
                self.single_namespace_gate("Delete Manifest Resources")?;
                if self.pending_delete.is_none() {
                    return Ok(());
                }
                let listed = self.details.as_ref().map_or(0, |d| d.text.lines().filter(|line| !line.trim().is_empty()).count());
                let (context, namespace) = self.target_parts();
                let body = format!("Context: {context}\nNamespace: {namespace}\n{listed} resource(s) listed in the preview are deleted from the cluster. This cannot be undone.");
                if !host::confirm(&format!("Delete {listed} resource(s) on {context}/{namespace}?"), &body)? {
                    return Err(Error::Cancelled);
                }
                let Some(manifest) = self.pending_delete.take() else {
                    return Ok(());
                };
                let _task = host::begin_task("Deleting the manifest's resources", false);
                let out = self.kubectl(&["delete", "-f", "-"], Some(&manifest))?;
                self.show("Deleted".into(), out, "text")?;
                self.reload();
                return Ok(());
            }
            "apply" => return crate::ui_group::post_view_message(PANEL, "collect:apply"),
            "save-file" => return crate::ui_group::post_view_message(PANEL, "collect:save"),
            "reload-yaml" => {
                if let Some((kind, name)) = self.details.as_ref().and_then(|d| d.editing.clone()) {
                    self.open_yaml(&kind, &name, None)?;
                }
                return Ok(());
            }
            _ => {}
        }
        let Some((kind, row)) = self.selected.clone() else {
            return Ok(());
        };
        let (namespace, name) = self.split_scope(&kind, &row)?;
        let args = |rest: &[&str]| self.args_in(namespace.as_deref(), rest);
        let target = format!("{kind}/{name}");
        match button {
            "describe" => self.describe(&kind, &row)?,
            "yaml" => self.edit_yaml(&kind, &row)?,
            "logs" if kind == "pods" => host::open_terminal(&format!("Logs: {row}"), KUBECTL, &args(&["logs", "-f", "--tail=500", &name]))?,
            "logs" => host::open_terminal(
                &format!("Logs: {kind}/{row}"),
                KUBECTL,
                &args(&["logs", "-f", &target, "--all-pods=true", "--all-containers", "--max-log-requests=20", "--tail=200"]),
            )?,
            "attach" => self.open_pod_terminal("attach", &format!("Attach: {row}"), args(&["attach", "-it", &name]))?,
            "debug" => self.debug_pod(&row)?,
            "show-secret" if kind == "secrets" => self.show_secret(&row)?,
            "edit-secret" if kind == "secrets" => self.edit_secret(&row)?,
            "log-view" => self.open_log_view(&kind, &row, ctx)?,
            "port-forward" => self.start_port_forward(&kind, &row, ctx)?,
            "shell" => self.open_pod_terminal(
                "shell",
                &format!("Shell: {row}"),
                args(&["exec", "-it", &name, "--", "sh", "-c", POD_SHELL]),
            )?,
            "delete" if !self.confirm_delete => {
                self.new_rich_callback();
                self.confirm_delete = true;
                crate::refresh_views();
            }
            "delete" => {
                let _task = host::begin_task(&format!("Deleting {kind}/{row}"), false);
                self.kubectl_in(namespace.as_deref(), &["delete", &kind, &name, "--wait=false"], None)?;
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

/// Kubernetes API-resource arguments must be literal DNS plural/group names,
/// never CLI options or output annotations.
fn resource_type_name(value:&str)->bool {
    let (plural,group)=value.split_once('.').map_or((value,None),|(plural,group)|(plural,Some(group)));
    is_dns_label(plural)&&group.is_none_or(is_dns_subdomain)
}

/// kubectl could not reach the cluster at all, so every further call would fail the same way.
fn connection_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    ["unable to connect", "connection refused", "i/o timeout", "dial tcp", "no such host", "context deadline exceeded"].iter().any(|m| message.contains(m))
}

/// Sorted `kind.group` names without the core and built-in groups, deduplicated.
fn custom_resource_names(names: Vec<String>) -> Vec<String> {
    let mut custom: Vec<String> = names.into_iter().filter(|name|resource_type_name(name)
        && name.split_once('.').is_some_and(|(_,group)|!BUILT_IN_GROUPS.contains(&group))).collect();
    custom.sort();
    custom.dedup();
    custom
}

/// `(kind, name)` of a resource node key (`res:pods/web-1`).
fn resource(key: &str) -> Option<(String, String)> {
    let (parent, name) = key.split_once('/')?;
    let kind = parent.strip_prefix("res:")?;
    resource_type_name(kind).then(||(kind.to_owned(),name.to_owned()))
}

#[cfg(test)]
fn test_tree_key(key:&str)->&str {
    let key=if key.starts_with("svc-"){key.split_once(':').map_or(key,|(_,key)|key)}else{key};
    let key=key.strip_prefix("node:").or_else(||key.strip_prefix("choice:")).unwrap_or(key);
    key.rsplit_once('~').filter(|(_,proof)|proof.len()==32&&proof.bytes().all(|byte|byte.is_ascii_hexdigit())).map_or(key,|(bare,_)|bare)
}

/// Presentation-only helper for tests inspecting emitted owned tree keys.
#[cfg(test)]
fn test_resource(key:&str)->Option<(String,String)> {
    let key=if key.starts_with("svc-"){key.split_once(':').map_or(key,|(_,key)|key)}else{key};
    let bare=key.rsplit_once('~').filter(|(_,proof)|proof.len()==32&&proof.bytes().all(|byte|byte.is_ascii_hexdigit())).map_or(key,|(bare,_)|bare);
    resource(bare)
}

/// The `namespace` storage value of the all-namespaces view.
pub const ALL_NAMESPACES: &str = "*";
/// Kinds without a namespace; their rows are bare names in every view.
const CLUSTER_SCOPED: &[&str] = &["nodes", "persistentvolumes", "namespaces"];
/// `ns/name` rows of a namespaced kind across all namespaces.
const ALL_ROWS: &str = "jsonpath={range .items[*]}{.metadata.namespace}{\"/\"}{.metadata.name}{\"\\n\"}{end}";

/// One part of a Kubernetes name: lowercase letters, digits and '-', starting and ending alphanumeric.
fn is_name_part(s: &str) -> bool {
    let alnum = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    s.starts_with(alnum) && s.ends_with(alnum) && s.chars().all(|c| alnum(c) || c == '-')
}

/// A DNS label (RFC 1123), the rule for namespace names.
fn is_dns_label(s: &str) -> bool {
    s.len() <= 63 && is_name_part(s)
}

/// A DNS subdomain (RFC 1123), the rule for most object names.
fn is_dns_subdomain(s: &str) -> bool {
    s.len() <= 253 && s.split('.').all(is_name_part)
}

const KUBECTL_INSTALL_HINT: &str = "Kube Explorer could not run kubectl: it is not installed, or not on the PATH the IDE was started with. \
Install it, restart the IDE if needed, then click Refresh.\n\
Install guide: https://kubernetes.io/docs/tasks/tools/\n\
macOS: brew install kubectl\n\
Windows: winget install -e --id Kubernetes.kubectl\n\
Linux: your distribution's package manager, or the binary download on that page.";

fn notify_install_hint() {
    host::notify(NotificationLevel::Info, "How to Install kubectl", KUBECTL_INSTALL_HINT);
}

/// Fixed: nothing from the project, the editor or the command argument goes in.
fn helm_repo_update_args() -> Vec<String> {
    vec!["repo".into(), "update".into()]
}

fn helm_repo_update() -> Result<(), Error> {
    let _task = host::begin_task("Updating Helm repositories", false);
    let out = match host::run_process(HELM, &helm_repo_update_args(), None) {
        Err(Error::Denied(_)) => {
            host::notify(
                NotificationLevel::Warning,
                "Helm repositories not updated",
                &ui_kit::denied::message("Updating Helm repositories", "process:helm", "Kube Explorer"),
            );
            return Ok(());
        }
        out => out?,
    };
    if out.status != Some(0) {
        return Err(failure(HELM, &out.stderr, out.status));
    }
    config::notify_success("Helm repositories updated", &output_tail(&out.stdout));
    Ok(())
}

/// The last 20 lines, at most 2000 characters (the end is kept).
fn output_tail(text: &str) -> String {
    if text.trim().is_empty() {
        return "Helm repositories updated.".into();
    }
    let lines: Vec<&str> = text.trim().lines().collect();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    let extra = tail.chars().count().saturating_sub(2000);
    tail.chars().skip(extra).collect()
}

/// A kubectl failure users can act on: a denial becomes the permission message; anything else a
/// classified reason naming the context, plus a next step.
pub(crate) fn tool_error(feature: &str, context: Option<&str>, error: Error) -> Error {
    match error {
        Error::Cancelled => Error::Cancelled,
        Error::Denied(capability) => Error::Invalid(ui_kit::denied::message(feature, &capability, "Kube Explorer")),
        other => Error::Failed(failure_reason(feature, context, &other.to_string())),
    }
}

pub(crate) fn failure_reason(feature: &str, context: Option<&str>, detail: &str) -> String {
    let on = context.filter(|context| !context.is_empty()).map_or_else(|| "the current context".to_owned(), |context| format!("context {context}"));
    let lower = detail.to_ascii_lowercase();
    if ["unauthorized", "expired", "must be logged in", "invalid_grant", "getting credentials", "exec plugin"].iter().any(|m| lower.contains(m)) {
        return format!("{feature} failed: the login for {on} has expired or was rejected. Sign in to the cluster again, then retry.");
    }
    if connection_failure(detail) {
        return format!("{feature} failed: {on} cannot be reached. Check the network or VPN, then retry.");
    }
    if lower.contains("forbidden") {
        return format!("{feature} failed: your account on {on} is not allowed to do this. Ask for access or switch context.");
    }
    if lower.contains("has been modified") || lower.contains("conflict") {
        return format!("{feature} failed: the object changed on {on} since it was opened. Reopen it, then retry.");
    }
    if lower.contains("notfound") || lower.contains("not found") {
        return format!("{feature} failed: not found on {on}. Refresh Kube Explorer, then retry.");
    }
    // Unclassified output is not echoed: kubeconfig and auth-plugin errors can carry credentials.
    format!("{feature} failed on {on}. Run kubectl with that context in a terminal to see why, or pick another context with Kube Explorer: Choose Kubeconfig.")
}

fn rich_details_unavailable() -> Error {
    Error::Invalid(format!("{} Open a project first if none is open.", ui_kit::denied::message("Resource details", "project.read", "Kube Explorer")))
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

/// Existing Reload Manifest transformation; private resource-editor replacement
/// deliberately retains resourceVersion and does not use this helper.
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
    let root = project.root()?;
    let root = root.trim_end_matches('/');
    let mut directory = file.rsplit_once('/').map(|(directory, _)| directory);
    while let Some(candidate) = directory.filter(|candidate| candidate.starts_with(root)) {
        if project.exists(&format!("{candidate}/Chart.yaml")).unwrap_or(false) {
            return Ok(candidate.to_owned());
        }
        directory = candidate.rsplit_once('/').map(|(parent, _)| parent);
    }
    Err("The open file is not in a Helm chart (no Chart.yaml in its folder or above).".into())
}

/// True when every document names a kind and none is a Secret or a List (which could hold one).
fn secret_free(manifest: &str) -> bool {
    manifest::documents(manifest).iter().all(|doc| doc.kind.as_ref().is_some_and(|kind| kind.text != "Secret" && !kind.text.ends_with("List")))
}

/// What `kubectl apply` would store, computed by the server and never persisted.
const DIFF_DRY_RUN: [&str; 7] = ["apply", "--dry-run=server", "-f", "-", "-o", "yaml", REQUEST_TIMEOUT];

const SERVER_FIELDS: [&str; 6] = ["managedFields", "resourceVersion", "uid", "creationTimestamp", "generation", "selfLink"];

/// kubectl `-o yaml` output (one object or a List) as [`normalized_objects`].
fn cluster_objects(text: &str) -> Option<String> {
    if text.len() > 16 * MAX_BYTES {
        return None;
    }

    normalized_objects(yaml_rust2::YamlLoader::load_from_str(text).ok()?)
}

/// One document per object (List items flattened) ordered by kind, namespace and name, keys sorted,
/// without status and server bookkeeping, so the cluster and the file line up field by field.
fn normalized_objects(docs: Vec<yaml_rust2::Yaml>) -> Option<String> {
    use yaml_rust2::Yaml;
    let mut objects = Vec::new();
    for doc in docs {
        let list = doc["kind"].as_str().is_some_and(|kind| kind.ends_with("List"));
        match doc {
            Yaml::Null => {}
            Yaml::Hash(mut fields) if list => objects.extend(fields.remove(&Yaml::String("items".into()))?.into_vec()?),
            Yaml::Hash(_) => objects.push(doc),
            _ => return None,
        }
    }
    let sort_key = |object: &Yaml| [&object["kind"], &object["metadata"]["namespace"], &object["metadata"]["name"]].map(|v| v.as_str().unwrap_or_default().to_owned());
    objects.sort_by_cached_key(sort_key);
    let mut out = String::new();
    for object in objects {
        let mut emitter = yaml_rust2::YamlEmitter::new(&mut out);
        emitter.multiline_strings(true);
        emitter.dump(&sorted_keys(without_server_fields(object)?, 0)?).ok()?;
        out.push('\n');
    }
    Some(out.strip_prefix("---\n").unwrap_or(&out).to_owned())
}

fn without_server_fields(object: yaml_rust2::Yaml) -> Option<yaml_rust2::Yaml> {
    use yaml_rust2::Yaml;
    let key = |name: &str| Yaml::String(name.into());
    let Yaml::Hash(mut fields) = object else { return None };
    fields.remove(&key("status"));
    if let Some(Yaml::Hash(metadata)) = fields.get_mut(&key("metadata")) {
        for field in SERVER_FIELDS {
            metadata.remove(&key(field));
        }
        let mut no_annotations = false;
        if let Some(Yaml::Hash(annotations)) = metadata.get_mut(&key("annotations")) {
            annotations.remove(&key("kubectl.kubernetes.io/last-applied-configuration"));
            no_annotations = annotations.is_empty();
        }
        if no_annotations {
            metadata.remove(&key("annotations"));
        }
    }
    Some(Yaml::Hash(fields))
}

fn sorted_keys(value: yaml_rust2::Yaml, depth: usize) -> Option<yaml_rust2::Yaml> {
    use yaml_rust2::Yaml;
    if depth > 64 {
        return None;
    }

    Some(match value {
        Yaml::Hash(fields) => {
            let mut entries: Vec<_> = fields.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Yaml::Hash(entries.into_iter().map(|(k, v)| Some((k, sorted_keys(v, depth + 1)?))).collect::<Option<_>>()?)
        }
        Yaml::Array(items) => Yaml::Array(items.into_iter().map(|v| sorted_keys(v, depth + 1)).collect::<Option<_>>()?),
        other => other,
    })
}

/// A terminal session key (at most 128 bytes) for an action, context and argv.
fn terminal_key(action: &str, context: &str, args: &[String]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (context, args).hash(&mut hasher);
    format!("{action}-{:016x}", hasher.finish())
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
    let number = t
        .strip_prefix('-')
        .unwrap_or(t)
        .split_once('.')
        .map_or(is_digits(t.strip_prefix('-').unwrap_or(t)), |(a, b)| is_digits(a) && is_digits(b));
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
    let (at, _) =
        dir.rmatch_indices("templates").find(|(i, _)| dir[..*i].ends_with('/') && (dir.len() == i + 9 || dir[i + 9..].starts_with('/')))?;
    Some(format!("{}/Chart.yaml", dir[..at].trim_end_matches('/')))
}

const MAX_BYTES: usize = 1 << 20;

/// The open project that holds `path`, with its root.
fn project_of(path: &str) -> Option<(String, openidea_plugin::Project)> {
    host::projects()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.root().ok()?, p)))
        .filter(|(root, _)| Path::new(path).starts_with(root))
        .max_by_key(|(root, _)| root.len())
}

fn manifest_file(path: &str) -> bool {
    (path.ends_with(".yaml") || path.ends_with(".yml") || path.ends_with(".tpl")) && !config::is_non_kubernetes(path)
}

fn check_enabled(code: &str) -> bool {
    let setting = checks::SETTINGS.iter().find(|(c, _)| *c == code).map(|(_, s)| *s);
    setting.and_then(host::setting_bool) != Some(false)
}

/// The manifest problems the diagnostics provider returns while the user types.
fn inspect_text(path: &str, text: &str, crd: &crd_schema::Cache, graph: &kustomize_graph::Cache) -> Vec<openidea_plugin::Diagnostic> {
    if !manifest_file(path) {
        return Vec::new();
    }
    let Some((_, project)) = project_of(path) else {
        return Vec::new();
    };
    if text.len() > MAX_BYTES {
        return Vec::new();
    }
    let in_chart = chart_file(path).is_some_and(|chart| project.read_file(&chart).is_ok());
    let directory = path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let patch_context = ["kustomization.yaml", "kustomization.yml", "Kustomization"]
        .iter()
        .any(|name| project.read_file(&format!("{directory}/{name}")).is_ok());
    let findings = checks::for_file(path, text, in_chart, patch_context);
    let mut diagnostics: Vec<_> = findings.iter().filter(|finding| check_enabled(finding.code)).map(|finding| finding.diagnostic(text)).collect();
    let doc = Document { path: path.into(), language: "yaml".into(), version: 0, text: text.into() };
    diagnostics.extend(crd.diagnostics(&doc));
    diagnostics.extend(kustomize_graph::diagnostics(&doc, graph));
    diagnostics
}

/// The problems of a chart template that need the chart's `Chart.yaml`, which a provider must not read.
fn chart_diagnostics(path: &str, text: &str) -> Vec<openidea_plugin::Diagnostic> {
    if !manifest_file(path) || text.len() > MAX_BYTES {
        return Vec::new();
    }
    let Some((root, project)) = project_of(path) else {
        return Vec::new();
    };
    let Some(chart) = chart_file(path).filter(|chart| project.read_file(chart).is_ok()) else {
        return Vec::new();
    };
    let chart_dir = chart.trim_end_matches("/Chart.yaml");
    let findings = helm::check(text, &helm::Chart::load(&project, &root, chart_dir, text));
    findings.iter().filter(|finding| check_enabled(finding.code)).map(|finding| finding.diagnostic(text)).collect()
}

/// Publishes only what the provider cannot compute; the provider reports everything else.
fn inspect(path: &str) {
    if let Some(project) = host::project_of(path)
        && let Ok(text) = project.read_file(path)
    {
        let _ = host::publish_diagnostics(path, &chart_diagnostics(path, &text));
    }
}
impl openidea_plugin::diagnostics::DiagnosticsProvider for KubeExplorer {
    fn diagnostics(&mut self,doc:&Document)->Vec<openidea_plugin::Diagnostic>{
        if matches!(doc.language.as_str(),"json"|"jsonc"){
            let mut found=json_assistance::diagnostics(doc);found.extend(self.crd_schema.diagnostics(doc));return found;
        }
        let mut found=inspect_text(&doc.path,&doc.text,&self.crd_schema,&self.kustomize_graph);
        found.extend(self.helm_assistance.diagnostics(doc));found
    }
}

/// Replaces each `{{name}}` of `template` once, so a value that itself contains a placeholder stays literal.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find("{{") {
        let Some(close) = rest[open..].find("}}") else {
            break;
        };
        let name = &rest[open + 2..open + close];
        out.push_str(&rest[..open]);
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(&rest[open..open + close + 2]),
        }
        rest = &rest[open + close + 2..];
    }
    out.push_str(rest);
    out
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Files a single change batch re-reads; the rest are evicted and warm again when opened or saved.
const MAX_WARMED_PER_CHANGE: usize = 32;

impl KubeExplorer {
    fn warm_path(&mut self, path: &str) {
        self.navigation.warm_file(path);
        self.label_refs.warm_file(path);
        self.kustomize_paths.warm(path);
        self.crd_schema.warm_file(path);
        self.helm_assistance.warm_file(path);
        self.kustomize_graph.warm_file(path);
    }

    fn evict_path(&mut self, path: &str) {
        self.navigation.evict(path);
        self.label_refs.evict(path);
        self.kustomize_paths.evict(path);
        self.crd_schema.evict(path);
        self.helm_assistance.evict(path);
        self.kustomize_graph.evict(path);
    }

    fn files_changed(&mut self, changed: &openidea_plugin::FilesChanged) {
        let all: Vec<&str> = changed.created.iter().chain(&changed.modified).chain(&changed.removed).map(String::as_str).collect();
        let stale_kustomizations = self.kustomize_graph.affected_by(&all);
        if changed.created.iter().chain(&changed.removed).any(|path| indexer::transformer_marker(path)) {
            self.navigation.forget_transformers();
        }
        for path in &all {
            self.evict_path(path);
        }
        for path in changed.created.iter().chain(&changed.modified).take(MAX_WARMED_PER_CHANGE) {
            self.warm_path(path);
        }
        for path in stale_kustomizations.iter().take(MAX_WARMED_PER_CHANGE) {
            self.kustomize_graph.warm_file(path);
        }
    }
}

impl Plugin for KubeExplorer {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        for kind in [EventKind::FileOpened, EventKind::FileSaved, EventKind::FilesChanged] {
            if let Err(error) = host::subscribe(kind) {
                host::log(LogLevel::Warn, &format!("Manifest inspections need the {} event: {error}", kind.name()));
            }
        }
        self.context = host::storage::get("context").ok().flatten();
        self.namespace = host::storage::get("namespace").ok().flatten();
        if self.context.is_some() {
            self.update_context_status();
        }
        Ok(())
    }

    fn deactivate(&mut self) {
        self.yaml_editors.clear();
        self.watch.clear();
        self.cluster_completion.clear();
        self.navigation.clear();
        self.crd_schema.clear(); self.helm_assistance.clear(); self.label_refs.clear(); self.kustomize_paths.clear(); self.kustomize_graph.clear();
        self.retire_forward("Plugin instance deactivated"); self.log_view = None;
        self.details = None; self.revealed = None; self.editing_secret = None; self.draft = None; self.pending_delete = None; self.rich_owner = None;
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.watch.closed_roots();
        for event in events {
            match event {
                Event::FilesChanged(changed) => self.files_changed(changed),
                Event::FileSaved(path) | Event::FileOpened(path) => {
                    self.warm_path(path);
                    inspect(path);
                }
                _ => {}
            }
        }
    }

    /// The IDE drops a command's result text: it is shown in a notification.
    fn run_command(&mut self,id:&str,ctx:&Invocation,arg:Option<&str>)->Result<Option<String>,Error>{
        if id==SHOW_TREE {host::reveal_view(TREE)?;return Ok(None);}
        if id=="source-occurrences" {return self.label_refs.show_occurrences(ctx);}
        if id=="helm-refresh-dependency-catalog" {
            let count=self.helm_assistance.refresh_dependency_catalog(ctx)?;
            host::notify(NotificationLevel::Info,"Helm dependency catalog",&format!("Cached {count} chart versions from the local Helm repository indexes. Refresh repositories separately to retrieve new versions."));
            return Ok(None);
        }
        // These explicit global/disconnected operations do not consume a project
        // process binding or require a usable source/workspace identity.
        if matches!(id,"helm-repo-update"|"helm-repo-cache"|"import-pasted-kubeconfig"|"review-imported-kubeconfigs"|"use-imported-kubeconfig"|"delete-imported-kubeconfig"|"how-to-install-kubectl"|"helm-parameters"|"edit-debug-images"|"telepresence-connect"|"telepresence-quit"|"open-resource-watch") {
            return self.with_global_process_scope(|this|this.run_command_in(id,ctx,arg));
        }
        self.with_process_scope(ctx,|this|this.run_command_in(id,ctx,arg))
    }
}
impl KubeExplorer {
    fn run_command_in(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        self.expire_forward_context(Some(ctx));
        self.expire_log_context(Some(ctx));
        self.expire_rich_context(Some(ctx));
        if id == "tree-switch" && matches!(arg, None | Some("ctx") | Some("ns")) {
            return self.pick_context_or_namespace(arg).map(|()| None);
        }
        if id.starts_with("tree-") { return self.tree_context_action(id, arg, ctx).map(|()|None); }
        match id {
            "apply-copied-yaml" => {
                let text = self.apply_copied_yaml()?;
                config::notify_success_with("Copied YAML applied", &text, &[NotificationAction::new("Show in Kube Explorer", SHOW_TREE)]);
                return Ok(None);
            }
            "reload-from-cluster" => return self.reload_from_cluster(ctx).map(|()| None),
            "helm-repo-update" => return helm_repo_update().map(|()| None),
            "helm-create" => return self.helm_create(ctx).map(|()| None),
            "helm-parameters" => return self.helm_parameters().map(|()| None),
            "customize-namespaces" => return self.customize_namespaces().map(|()| None),
            "edit-debug-images" => return self.edit_debug_images().map(|()| None),
            "helm-repo-cache" => return self.cache_helm_repositories().map(|()| None),
            "review-private-yaml" => return self.yaml_editors.review(ctx).map(|()|None),
            "apply-reviewed-private-yaml" => return self.yaml_editors.apply(ctx).map(|()|None),
            "retire-private-yaml" => return self.yaml_editors.retire(ctx).map(|()|None),
            "watch-resources" => return self.watch.prepare(ctx,&self.context,&self.namespace,false).map(|()|None),
            "open-resource-watch" => { crate::ui_group::reveal_view(watch::VIEW)?; return Ok(None); }
            "cache-cluster-resources" => {
                let count=self.cluster_completion.read_resources(ctx,self.context.as_deref(),self.namespace.as_deref())?;
                host::notify(NotificationLevel::Info,"Resource completion snapshot",&format!("Completion now offers {count} resource names from the cluster for the next 60 seconds."));
                return Ok(None);
            }
            "cache-cluster-namespaces" => {
                let count = self.cluster_completion.read(ctx, self.context.as_deref(), self.namespace.as_deref())?;
                host::notify(NotificationLevel::Info, "Namespace completion snapshot", &format!("Completion now offers {count} namespace names from the cluster for the next 60 seconds."));
                return Ok(None);
            }
            "import-pasted-kubeconfig" => return kubeconfig_import::import().map(|()| None),
            "review-imported-kubeconfigs" => return kubeconfig_import::review().map(|()| None),
            "use-imported-kubeconfig" => return self.use_imported_kubeconfig().map(|()| None),
            "delete-imported-kubeconfig" => return self.delete_imported_kubeconfig().map(|()| None),
            "choose-kubeconfig" => return self.choose_kubeconfig().map(|()| None),
            "telepresence-connect" => return self.telepresence("connect").map(|()| None),
            "telepresence-quit" => return self.telepresence("quit").map(|()| None),
            "how-to-install-kubectl" => {
                notify_install_hint();
                return Ok(None);
            }
            _ => {}
        }
        if let Some(action) = id.strip_prefix("resource-") {
            return self.resource_command(action, arg, ctx).map(|()| None);
        }
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
        match id {
            "apply" | "kustomize-apply" => config::notify_success_with(done, &text, &[NotificationAction::new("Show in Kube Explorer", SHOW_TREE)]),
            _ => config::notify_success(done, &text),
        }
        Ok(None)
    }
}

impl KubeExplorer {
    fn resource_command(&mut self, action: &str, key: Option<&str>, ctx: &Invocation) -> Result<(), Error> {
        let (kind,row)=self.checked_resource(key.ok_or_else(||Error::Invalid("Select a resource in the Kube Explorer window first.".into()))?)?.ok_or_else(||Error::Invalid("Select a resource in the Kube Explorer window first.".into()))?;
        let applies = match action {
            "logs" | "log-sources" | "log-view" => kind == "pods" || WORKLOADS.contains(&kind.as_str()),
            "shell" | "attach" | "debug" => kind == "pods",
            "show-secret" | "edit-secret" => kind == "secrets",
            "port-forward" => FORWARDABLE.iter().any(|(k, _)| *k == kind),
            "describe" | "yaml" | "delete" => true,
            _ => return Err(Error::NotFound(format!("command resource-{action}"))),
        };
        if !applies {
            return Err(format!("{action} does not apply to {kind}.").into());
        }
        if action == "delete" && !host::confirm("Delete Resource", &self.delete_question(&kind, &row)?)? {
            return Ok(());
        }
        if action == "log-sources" {
            return self.select_log_sources(&kind, &row);
        }
        if action == "debug" {
            let Some(image) = self.ask_debug_image()? else {
                return Ok(());
            };
            self.debug_image = Some(image);
        }
        self.selected = Some((kind, row));
        self.new_rich_callback();
        self.confirm_delete = action == "delete";
        self.button_in(action, Some(ctx))?;
        if self.details.is_some()
            || self.forward.is_some()
            || self.revealed.is_some()
            || self.editing_secret.is_some()
            || self.log_view.is_some()
        {
            crate::ui_group::reveal_view(PANEL)?;
            crate::refresh_panel();
        }
        crate::refresh_views();
        Ok(())
    }

    fn command(&mut self,id:&str,ctx:&Invocation)->Result<String,Error>{self.with_process_scope(ctx,|this|this.command_in(id,ctx))}
    fn command_in(&mut self, id: &str, ctx: &Invocation) -> Result<String, Error> {
        match id {
            "apply" => self.single_namespace_gate("Apply Manifest")?,
            "kustomize-apply" => self.single_namespace_gate("Apply Kustomization")?,
            "delete-manifest" => self.single_namespace_gate("Delete Manifest Resources")?,
            _ => {}
        }
        let editor = ctx.editor().ok_or("No manifest is open.")?;
        let manifest = editor.document()?.text;
        let path = editor.path();
        self.check_editor_owner(&path)?;
        match id {
            "delete-manifest" => {
                let out = self.kubectl(&["delete", "--dry-run=server", "-f", "-"], Some(&manifest))?;
                self.show(format!("Delete these resources from {}?", self.target()), out, "text")?;
                self.pending_delete = Some(manifest);
                Ok("Review the resources in Kube Explorer Details, then click Confirm Delete.".into())
            }
            "kustomize-render" => {
                let dir = parent(&path);
                let out = self.kubectl(&["kustomize", dir], None)?;
                self.show(format!("Kustomize: {dir}"), out, "yaml")?;
                Ok("The rendered YAML is shown in Kube Explorer Details.".into())
            }
            "kustomize-apply" => {
                self.confirm_apply(&format!("the kustomization in {}", parent(&path)))?;
                let out = self.kubectl(&["apply", "-k", parent(&path)], None)?;
                let target = self.target();
                self.reload();
                Ok(format!("{target}: {}", out.trim()))
            }
            "helm-template" | "helm-lint" | "helm-dependency-update" => {
                let chart = chart_dir(ctx, &path)?;
                let release = prompts::helm_release().unwrap_or_else(|| chart.rsplit('/').next().unwrap_or("release").to_owned());
                let release = release.as_str();
                if id=="helm-template" {prompts::refuse_option_like("release name",release)?;}
                let args: &[&str] = match id {
                    "helm-template" => &["template", release, &chart],
                    "helm-lint" => &["lint", &chart],
                    _ => &["dependency", "update", &chart],
                };
                let mut args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
                if id != "helm-dependency-update" {
                    args.extend(prompts::helm_flags());
                }
                let out = self.process(HELM, &args, None)?;
                // `helm lint` reports problems on stdout and fails; show them either way.
                if id == "helm-lint" {
                    self.show(format!("Helm lint: {release}"), format!("{}{}", out.stdout, out.stderr), "text")?;
                    return Ok(if out.status == Some(0) {
                        "The chart passed lint. The output is shown in Kube Explorer Details."
                    } else {
                        "The chart has lint errors. The output is shown in Kube Explorer Details."
                    }
                    .into());
                }
                if out.status != Some(0) {
                    return Err(failure(HELM, &out.stderr, out.status));
                }
                if id == "helm-template" {
                    self.show(format!("Helm template: {release}"), out.stdout, "yaml")?;
                    return Ok("The rendered YAML is shown in Kube Explorer Details.".into());
                }
                Ok(out.stdout.trim().to_owned())
            }
            "apply" => {
                let choices = [PickItem::new("apply", "Apply"), PickItem::new("diff", "Show Diff with Cluster First")];
                match host::pick(&self.apply_question(&path), &choices)?.as_deref() {
                    Some("apply") => {}
                    Some("diff") => {
                        let shown = self.diff_manifest(&manifest, &path)?;
                        let name = path.rsplit('/').next().unwrap_or(&path);
                        if !host::confirm(&format!("Apply {name}?"), &format!("{shown}\n{}", self.apply_question(&path)))? {
                            return Err(Error::Cancelled);
                        }
                    }
                    _ => return Err(Error::Cancelled),
                }
                let out = self.kubectl(&["apply", "-f", "-"], Some(&manifest))?;
                let target = self.target();
                self.reload();
                Ok(format!("{target}: {}", out.trim()))
            }
            "dry-run" => {
                let out = self.kubectl(&["apply", "--dry-run=server", "-f", "-"], Some(&manifest))?;
                Ok(out.trim().to_owned())
            }
            "diff" => self.diff_manifest(&manifest, &path),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

impl Providers for KubeExplorer {
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<openidea_plugin::Location> {
        let mut found = kustomize::definition(doc, at);
        found.extend(kustomize_graph::definition(doc, at));
        found.extend(helm_values::definition(doc, at));
        found.extend(self.navigation.definition(doc, at));
        found.extend(self.label_refs.definition(doc,at));
        found
    }
    fn line_markers(&mut self, doc: &Document) -> Vec<openidea_plugin::providers::LineMarker> {
        let mut found = kustomize::markers(doc);
        found.extend(self.navigation.markers(doc));
        found.extend(self.label_refs.markers(doc));
        found
    }
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if matches!(doc.language.as_str(),"json"|"jsonc"){return json_assistance::completion(doc,at,&self.crd_schema);}
        let mut items = self.cluster_completion.items(doc, at, &self.context, &self.namespace);
        items.extend(complete::items(doc, at));
        items.extend(kustomize::completion(doc, at,&self.kustomize_paths));
        items.extend(self.crd_schema.items(doc, at));
        items.extend(self.label_refs.completion(doc,at));
        items.extend(self.helm_assistance.completion(doc,at));
        items
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if matches!(doc.language.as_str(),"json"|"jsonc"){return vec![];}
        let mut actions = schema::code_actions(doc);
        actions.extend(self.helm_assistance.code_actions(doc));
        actions.extend(convert::code_actions(doc, selection));
        actions.extend(chart_actions::code_actions(doc, selection));
        actions
    }
}

impl Views for KubeExplorer {
    fn render(&mut self,view:&str,viewport:Option<Viewport>,ctx:&Invocation)->Result<View,Error>{
        (|| -> Result<View, Error> {
            self.with_process_scope(ctx,|this| {
                let mut rendered=this.render_in(view,viewport,ctx)?;
                rendered=this.protect_tree_view(rendered);
                if view==TREE {
                    rendered=this.service_tree(rendered);
                    let scope=this.process_scope.root().map(|root|format!("Project: {root}")).unwrap_or_else(||"Global CLI mode (no invoking project)".into());
                    rendered.status=Some(match rendered.status.take(){Some(status) if !status.is_empty()=>format!("{status} · {scope}"),_=>scope});
                }
                Ok(rendered)
            })
        })().or_else(|error| match error {
            Error::Denied(capability) if capability.starts_with("project.read") && view == TREE => Ok(View::new(Body::Tree(vec![
                TreeItem::new("kube:status", "Kubernetes — project access needed")
                    .icon(k8s!("cluster"))
                    .detail("Allow project.read for Kube Explorer in Settings › Plugins."),
            ]))),
            Error::Denied(capability) if capability.starts_with("project.read") => {
                // A concrete invoking project must never silently downgrade to
                // global CLI execution when its optional read grant is absent.
                // Rendering remains usable as an inert explanation; events and
                // commands retain their existing strict ownership checks.
                Ok(View::message(&format!("{} Kubernetes can also run in a window with no open project.", ui_kit::denied::message("Kubernetes for this project", "project.read", "Kube Explorer")))
                    .status("Project access unavailable"))
            }
            error => Err(error),
        }).map(ui_kit::actions::available)
    }
    fn handle(&mut self,view:&str,event:ViewEvent,ctx:&Invocation)->Result<(),Error>{self.with_process_scope(ctx,|this|this.handle_in(view,event,ctx))}
}
impl KubeExplorer {
    fn render_in(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        if _view == watch::VIEW { return self.watch.view(_ctx,&self.context,&self.namespace); }
        if self.rich_callback.is_none(){self.new_rich_callback();}
        self.expire_forward_context(Some(_ctx));
        self.expire_log_context(Some(_ctx));
        let cleared = self.expire_rich_context(Some(_ctx));
        if cleared && _view == PANEL && self.rich_owner.is_none() {
            return Ok(View::message("Cached details and Secret plaintext need an open project with project.read permission. The previous cache was cleared."));
        }
        if _view == TREE { self.render_resources() } else { self.render_tree_in(Some(_ctx)) }
    }

    fn handle_in(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        if _view == watch::VIEW { return self.watch.handle(event,_ctx,&self.context,&self.namespace); }
        self.expire_forward_context(Some(_ctx));
        self.expire_log_context(Some(_ctx));
        self.expire_rich_context(Some(_ctx));
        if matches!(&event, ViewEvent::Message(message) if serde_json::from_str::<serde_json::Value>(message).ok().is_some_and(|value|value["channel"] == "ui-kit")) {
            let config = self.context_menu_config(_view, _ctx)?;
            if ui_kit::web::handle_with_actions(self, _view, &event, _ctx, &config, Self::menu_actions)? { return Ok(()); }
        }

        if let ViewEvent::Button(key)=&event {
            if key.starts_with("service-action:") {
                if _view != TREE {return Err(Error::Stale);}
                return self.service_action(key,_ctx);
            }
        }
        let show_panel = _view == TREE && matches!(&event, ViewEvent::Activate(_) | ViewEvent::Button(_));
        let event=self.checked_tree_event(event)?;
        let event=self.checked_rich_event(event)?;
        self.handle_tree(event, _ctx)?;
        if show_panel {
            crate::refresh_panel();
        }
        if show_panel
            && (self.details.is_some()
                || self.show_forward_session
                || self.forward.is_some()
                || self.revealed.is_some()
                || self.editing_secret.is_some()
                || self.log_view.is_some()
                || self.draft.is_some())
        {
            crate::ui_group::reveal_view(PANEL)?;
        }
        Ok(())
    }
}

impl openidea_plugin::details::Details for KubeExplorer {
    fn detail(&mut self,view:&str,key:&str,ctx:&Invocation)->Result<Vec<openidea_plugin::details::Tab>,Error>{self.with_process_scope(ctx,|this|this.detail_in(view,key,ctx))}
}
impl KubeExplorer {
    fn detail_in(&mut self, view: &str, key: &str, _ctx: &Invocation) -> Result<Vec<openidea_plugin::details::Tab>, Error> {
        if view != TREE { return Ok(Vec::new()); }
        self.native_details(key)
    }
}

// API 1.3 registers the optional exports on the same component.
openidea_plugin::register_plugin!(KubeExplorer, providers, views, indexing, details, diagnostics, file_actions);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::ProcessOutput;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::views::FieldValue;

    fn ok(stdout: &str) -> Result<ProcessOutput, Error> {
        Ok(ProcessOutput { status: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    #[test]
    fn yaml_providers_do_not_panic_at_any_offset() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let host = MockHost::install().with_capabilities(&["project.read", "storage", "process:kubectl"]);
        host.set_project_root(Some("/p"));
        let seeds = [
            "{{ x }}{#}",
            "café",
            "<?php\nuse $😀",
            "é::class",
            "<!-- Paramètres --><a/>",
            "apiVersion: v1\nkind: Pod\nmetadata:\n  name: café\n  namespace: app\n  labels: {app: 😀}\nspec:\n  containers:\n  - name: é\n    env:\n    - name: X\n      valueFrom:\n        configMapKeyRef: {name: 😀cfg, key: é}\n",
            "key:  # c\n- 😀: é\n---\n--- !tag\nresources:\n- ../é\n",
            "apiVersion: v1\nkind: Service\nmetadata: {name: web, namespace: app}\nspec:\n  selector: {app: é😀}\n",
        ];
        let mut kube = KubeExplorer::default();
        for (n, seed) in seeds.iter().enumerate() {
            let path = format!("/p/{n}.yaml");
            host.add_file(&format!("{n}.yaml"), seed);
            kube.warm_path(&path);
            let kustomization = format!("/p/k{n}/kustomization.yaml");
            host.add_file(&format!("k{n}/kustomization.yaml"), seed);
            kube.warm_path(&kustomization);
            for path in [path, kustomization] {
                let doc = Document { path, language: "yaml".into(), version: 1, text: (*seed).into() };
                for offset in (0..=seed.len()).filter(|at| seed.is_char_boundary(*at)) {
                    let at = openidea_plugin::text::position(seed, offset);
                    kube.definition(&doc, at);
                    kube.completion(&doc, at, None);
                    kube.code_actions(&doc, Range::new(at, at));
                }
                kube.line_markers(&doc);
                kube.diagnostics(&doc);
            }
        }
    }

    #[test]
    fn chart_dir_walks_up_to_the_nearest_chart_without_searching() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        host.set_project_root(Some("/project"));
        host.add_file("charts/app/Chart.yaml", "name: app\n");
        host.add_file("charts/app/charts/sub/Chart.yaml", "name: sub\n");
        let ctx = host.invocation();
        assert_eq!(chart_dir(&ctx, "/project/charts/app/templates/a.yaml").unwrap(), "/project/charts/app");
        assert_eq!(chart_dir(&ctx, "/project/charts/app/charts/sub/templates/a.yaml").unwrap(), "/project/charts/app/charts/sub");
        assert!(chart_dir(&ctx, "/project/other/a.yaml").is_err());
    }

    #[test]
    fn reload_continues_past_a_forbidden_kind() {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        host.on_process(KUBECTL, |args, _| match args.iter().any(|a| a == "nodes") {
            true => Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Error from server (Forbidden): nodes is forbidden\n".into() }),
            false => ok("pod/web-1\n"),
        });
        let mut kube = KubeExplorer { context: Some("c".into()), ..Default::default() };
        for key in ["kind:nodes", "kind:pods"] {
            kube.expanded.insert(key.into());
        }
        kube.reload();
        assert_eq!(host.process_runs().len(), 2);
        assert!(kube.children["kind:nodes"].is_err());
        assert!(kube.children["kind:pods"].is_ok());
    }

    #[test]
    fn reload_stops_after_first_failure() {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "storage"]);
        host.on_process(KUBECTL, |_, _| {
            Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Unable to connect to the server\n".into() })
        });
        let mut kube = KubeExplorer { context: Some("c".into()), ..Default::default() };
        for key in ["kind:pods", "kind:services", "kind:nodes"] {
            kube.expanded.insert(key.into());
        }
        kube.reload();
        assert_eq!(host.process_runs().len(), 1);
        assert_eq!(kube.children.len(), 3);
        assert!(kube.children.values().all(Result::is_err));
    }

    #[test]
    fn literal_placeholder_text_not_substituted() {
        let _host = host();
        let details = Details {
            callback: String::new(),
            title: "cm/x".into(),
            text: "SENTINEL {{text}} {{callback}}".into(),
            lang: "yaml",
            editing: None,
            note: None,
        };
        let Body::Html(page) = KubeExplorer::default().render_details(&details).body else {
            panic!("expected html");
        };
        assert_eq!(page.matches("SENTINEL").count(), 2);
        assert!(page.contains("{{callback}}"));
    }

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "project.read", "editor.read", "storage"]);
        host.on_process(KUBECTL, |args, stdin| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("config") if args.contains(&"current-context".to_owned()) => ok("minikube\n"),
            Some("config") if args.iter().any(|a|a=="view") => ok(r#"{"contexts":[{"name":"minikube","context":{"cluster":"minikube","namespace":"default"}}],"clusters":[{"name":"minikube","cluster":{"server":"https://minikube.invalid"}}]}"#),
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
            Body::Tree(items) => items.into_iter().map(|i| test_tree_key(&i.key).to_owned()).collect(),
            other => panic!("not a tree: {other:?}"),
        }
    }

    #[test]
    fn expanding_a_kind_lists_its_resources_and_a_pod_opens_logs() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();
        let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());
        assert!(tree.contains(&"res:pods/web-1".to_owned()));

        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("logs")), &host.invocation()).unwrap();
        assert_eq!(host.terminals()[0].args, ["logs", "-f", "--tail=500", "web-1"]);
    }

    #[test]
    fn choosing_a_context_with_a_slash_persists_it_and_passes_it_to_kubectl() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("ctx",&host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Activate(kube.test_choice_key("ctx/arn:aws:eks:eu-west-1:1:cluster/prod",&host.invocation())), &host.invocation()).unwrap();
        kube.render(TREE,None,&host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();

        assert_eq!(host.storage()["context"], "arn:aws:eks:eu-west-1:1:cluster/prod");
        assert!(host.process_runs().last().unwrap().args.contains(&"--context=arn:aws:eks:eu-west-1:1:cluster/prod".to_owned()));
    }

    #[test]
    fn describe_shows_its_output_in_the_tree_view_until_back() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("describe")), &host.invocation()).unwrap();
        let shown = kube.render(PANEL, None, &host.invocation()).unwrap().body;
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("back")), &host.invocation()).unwrap();
        let back = kube.render(PANEL, None, &host.invocation()).unwrap().body;

        assert!(matches!(shown, Body::Html(page) if page.contains("Name: web-1")));
        assert!(matches!(back, Body::Tree(_)));
        assert!(host.tasks().iter().any(|t| t.title == "Describing pods/web-1" && t.ended));
    }

    fn page(kube: &mut KubeExplorer, host: &MockHost) -> String {
        match kube.render(PANEL, None, &host.invocation()).unwrap().body {
            Body::Html(page) => page,
            other => panic!("not a page: {other:?}"),
        }
    }

    #[test]
    fn highlights_yaml_and_diffs_like_the_page() {
        assert_eq!(yaml_line("  name: web # x"), r#"<span class="m">  </span><span class="k">name</span>: web # x"#);
        assert_eq!(
            yaml_line(r#"- image: "nginx""#),
            r#"<span class="m">- </span><span class="k">image</span>:<span class="s"> &quot;nginx&quot;</span>"#
        );
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
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();
        let yaml = page(&mut kube, &host);
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("describe")), &host.invocation()).unwrap();
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
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();

        host.answer_next_prompt(Some(&[]));
        kube.handle(TREE, ViewEvent::Message(kube.test_apply_message("kind: Pod # edited")), &host.invocation()).unwrap();

        assert!(host.process_runs().iter().any(|r| r.args.ends_with(&["replace".into(), "-f".into(), "-".into()])));
        assert!(page(&mut kube, &host).contains("name: web-1"));
    }

    #[test]
    fn tool_failures_name_the_context_and_a_reason_without_raw_output() {
        let reason = |detail: &str| failure_reason("Reading logs", Some("prod"), detail);
        assert!(reason("error: You must be logged in to the server (Unauthorized)").contains("login for context prod has expired"));
        assert!(reason("dial tcp 10.0.0.1:443: connect: connection refused").contains("context prod cannot be reached"));
        assert!(reason("Error from server (Forbidden): pods is forbidden").contains("not allowed"));
        assert!(reason("Error from server (NotFound): pods \"web\" not found").contains("not found on context prod"));
        let other = reason("token=abc123 parser failure");
        assert!(other.contains("context prod") && !other.contains("abc123"), "{other}");
        assert!(matches!(tool_error("Reading logs", None, Error::Denied("process:kubectl".into())), Error::Invalid(m) if m == ui_kit::denied::message("Reading logs", "process:kubectl", "Kube Explorer")));
    }

    #[test]
    fn declining_the_replace_confirm_runs_nothing() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();

        host.answer_next_prompt(None);
        let result = kube.handle(TREE, ViewEvent::Message(kube.test_apply_message("kind: Pod # edited")), &host.invocation());

        assert!(matches!(result, Err(Error::Cancelled)));
        assert!(!host.process_runs().iter().any(|r| r.args.iter().any(|a| a == "replace")));
        assert!(kube.details.is_some());
    }

    #[test]
    fn a_rejected_apply_keeps_the_edits_and_reports_why() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();
        let refreshes = host.refreshed_views().len();

        host.answer_next_prompt(Some(&[]));
        kube.handle(TREE, ViewEvent::Message(kube.test_apply_message("kind: Pod # conflict")), &host.invocation()).unwrap();

        let reply:serde_json::Value=serde_json::from_str(&host.view_messages().last().unwrap().1).unwrap();
        assert_eq!(reply["targetView"],PANEL);
        assert_eq!(reply["message"], "error:the object has been modified");
        assert_eq!(host.refreshed_views().len(), refreshes);
    }

    fn host_with_tee() -> (MockHost, std::rc::Rc<std::cell::RefCell<String>>) {
        let host = host().with_capabilities(&["process:kubectl", "process:tee", "editor.read", "storage", "events", "project.read"]);
        host.on_process(KUBECTL, |args, _| {
            if args.iter().any(|a| a=="config") {
                let context=args.iter().find_map(|a|a.strip_prefix("--context=")).unwrap_or("minikube");
                if args.iter().any(|a|a=="current-context") { return ok(context); }
                return ok(&format!(r#"{{"contexts":[{{"name":"{context}","context":{{"cluster":"local","namespace":"default"}}}}],"clusters":[{{"name":"local","cluster":{{"server":"https://local.invalid"}}}}]}}"#));
            }
            ok("apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-1\n  namespace: default\n  uid: uid-web\n  resourceVersion: \"7\"\n")
        });
        host.set_setting("yamlInEditor", "true");
        let written = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let sink = written.clone();
        host.on_process("tee", move |_, stdin| {
            *sink.borrow_mut() = stdin.unwrap_or_default().to_owned();
            ok("")
        });
        (host, written)
    }

    fn open_in_editor(kube: &mut KubeExplorer, host: &MockHost) -> String {
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();
        host.opened_locations()[0].0.clone()
    }

    #[test]
    fn secret_yaml_never_goes_to_a_temp_file() {
        let (host, written) = host_with_tee();
        host.set_setting("allowRevealSecrets", "true");
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:secrets/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();

        assert!(host.process_runs().iter().all(|r| r.program != "tee"), "tee must not run for a Secret");
        assert!(written.borrow().is_empty());
        assert!(host.opened_locations().is_empty());
        assert!(kube.details.as_ref().is_some_and(|d| d.text.contains("name: web-1")), "the YAML is shown in the panel instead");
    }

    #[test]
    fn yaml_stays_in_the_panel_unless_the_editor_option_is_on() {
        let (host, written) = host_with_tee();
        host.set_setting("yamlInEditor", "false");
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("yaml")), &host.invocation()).unwrap();

        assert!(host.process_runs().iter().all(|r| r.program != "tee"));
        assert!(written.borrow().is_empty());
        assert!(page(&mut kube, &host).contains("name: web-1"));
    }

    #[test]
    fn private_editor_uses_sdk_storage_preserves_identity_and_never_tee() {
        let (host, written) = host_with_tee();
        let path = open_in_editor(&mut KubeExplorer::default(), &host);
        assert!(path.starts_with(&format!("{}/resource-editor-", host::fs::plugin_dir().unwrap())));
        let bytes=host::fs::read_bytes(&path).unwrap(); let text=String::from_utf8(bytes).unwrap();
        assert!(text.contains("resourceVersion: \"7\"") && text.contains("uid: uid-web"));
        assert!(text.contains("# yaml-language-server: $schema="));
        assert!(written.borrow().is_empty());
        assert!(host.process_runs().iter().all(|r|r.program!="tee"));
        assert!(!host.storage().keys().any(|k|k.starts_with("edit:")));
    }

    #[test]
    fn saving_private_or_legacy_temp_files_never_applies() {
        let (host, _) = host_with_tee();
        let mut kube = KubeExplorer::default();
        let path = open_in_editor(&mut kube, &host);
        kube.handle_events(&[openidea_plugin::Event::FileSaved(path),openidea_plugin::Event::FileSaved("/tmp/legacy.yaml".into())], &host.invocation());
        assert!(host.process_runs().iter().all(|r|!r.args.iter().any(|a|a=="replace")));
    }

    #[test]
    fn saving_another_file_does_nothing() {
        let (host, _) = host_with_tee();
        KubeExplorer::default().handle_events(&[openidea_plugin::Event::FileSaved("/project/a.yaml".into())], &host.invocation());
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn a_manifest_offers_to_add_its_schema() {
        let doc =
            Document { path: "/project/pod.yaml".into(), language: "yaml".into(), version: 1, text: "apiVersion: v1\nkind: Pod\n".into() };

        let actions = KubeExplorer::default().code_actions(&doc, Default::default());

        assert_eq!(
            actions[0].edits[0].1[0].text.lines().next().unwrap(),
            format!("# yaml-language-server: $schema={}", schema::url("v1", "Pod").unwrap())
        );
    }

    #[test]
    fn managed_port_forward_starts_an_owned_process_after_one_confirm() {
        let host = host().with_capabilities(&["process:kubectl", "project.read", "editor.read", "storage"]);
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("port-forward")), &host.invocation()).unwrap();
        let form = kube.render(PANEL, None, &host.invocation()).unwrap().body;
        kube.handle(TREE, ViewEvent::Field(kube.test_control_key("ports"), "8080:80".into()), &host.invocation()).unwrap();
        host.answer_next_prompt(Some(&[]));
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("start-managed-forward")), &host.invocation()).unwrap();

        assert!(matches!(form, Body::Form(fields) if fields[0].value == FieldValue::Text("80:80 443:443".into())));
        assert!(host.terminals().is_empty());
        assert!(kube.forward_session.as_ref().unwrap().running());
    }

    #[test]
    fn workload_logs_follow_all_its_pods_and_pods_can_be_attached() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:deployments/web", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("logs")), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("attach")), &host.invocation()).unwrap();

        assert!(host.terminals()[0].args.starts_with(&["logs".into(), "-f".into(), "deployments/web".into(), "--all-pods=true".into()]));
        let attach = ["attach", "-it", "web-1"].map(str::to_owned);
        assert!(host.finish_terminal(&terminal_key("attach", "", &attach)).is_some());
    }

    #[test]
    fn deleting_a_manifests_resources_previews_them_first() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "kind: Deployment", &[]);
        let mut kube = KubeExplorer::default();

        kube.run_command("delete-manifest", &host.invocation(), None).unwrap();
        let deletes = || host.process_runs().into_iter().filter(|r| r.args.contains(&"delete".to_owned())).collect::<Vec<_>>();
        assert!(deletes()[0].args.contains(&"--dry-run=server".to_owned()));

        host.answer_next_prompt(Some(&[]));
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("confirm-delete-manifest")), &host.invocation()).unwrap();
        assert!(deletes()[1].args.ends_with(&["delete".into(), "-f".into(), "-".into()]));
    }

    #[test]
    fn declining_the_manifest_delete_confirm_deletes_nothing() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "kind: Deployment", &[]);
        let mut kube = KubeExplorer { namespace: Some("shop".into()), current_context: Some("minikube".into()), ..Default::default() };
        kube.run_command("delete-manifest", &host.invocation(), None).unwrap();
        assert!(kube.details.as_ref().unwrap().title.contains("minikube/shop"));

        host.answer_next_prompt(None);
        assert!(matches!(kube.button("confirm-delete-manifest"), Err(Error::Cancelled)));

        assert!(!host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned()) && !r.args.contains(&"--dry-run=server".to_owned())));
        assert!(kube.pending_delete.is_some());
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

    type Recorded = std::rc::Rc<std::cell::RefCell<Vec<(Vec<String>, Option<String>)>>>;

    fn helm_host(caps: &[&str], output: Result<ProcessOutput, Error>) -> (MockHost, Recorded) {
        let host = MockHost::install().with_capabilities(caps);
        let recorded = Recorded::default();
        let seen = recorded.clone();
        host.on_process(HELM, move |args, stdin| {
            seen.borrow_mut().push((args.to_vec(), stdin.map(str::to_owned)));
            output.clone()
        });
        (host, recorded)
    }

    fn repo_update(host: &MockHost, arg: Option<&str>) -> Result<Option<String>, Error> {
        KubeExplorer::default().run_command("helm-repo-update", &host.invocation(), arg)
    }

    #[test]
    fn repo_update_runs_exactly_helm_repo_update() {
        let (host, recorded) = helm_host(&["process:helm"], ok("Update Complete.\n"));

        repo_update(&host, None).unwrap();

        assert_eq!(helm_repo_update_args(), ["repo", "update"]);
        let runs = host.process_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!((runs[0].program.as_str(), runs[0].args.clone(), runs[0].cwd.clone()), (HELM, helm_repo_update_args(), None));
        assert_eq!(recorded.borrow()[0], (helm_repo_update_args(), None));
        assert_eq!(host.notifications()[0].body, "Update Complete.");
    }

    #[test]
    fn repo_update_ignores_the_command_argument() {
        for arg in ["--debug", "../../etc", "-rf"] {
            let (host, _) = helm_host(&["process:helm"], ok(""));

            repo_update(&host, Some(arg)).unwrap();

            assert_eq!(host.process_runs()[0].args, ["repo", "update"], "{arg}");
        }
    }

    #[test]
    fn repo_update_does_not_read_the_open_file() {
        let (host, _) = helm_host(&["process:helm"], ok(""));
        host.open_editor("-rf/../x.yaml", "yaml", "--set=evil", &[]);

        repo_update(&host, None).unwrap();

        assert_eq!(host.process_runs()[0].args, ["repo", "update"]);
        assert!(host.notifications().iter().all(|n| !n.body.contains("evil") && !n.body.contains("-rf")));
    }

    #[test]
    fn repo_update_without_helm_capability_notifies_instead_of_failing() {
        let (host, _) = helm_host(&["editor.read"], ok(""));

        assert_eq!(repo_update(&host, None), Ok(None));

        let note = &host.notifications()[0];
        assert_eq!(note.level, NotificationLevel::Warning);
        assert!(note.body.contains("helm") && note.body.contains("Settings"), "{}", note.body);
    }

    #[test]
    fn repo_update_failure_reports_stderr() {
        let failed = Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Error: no repositories found\n".into() });
        let (host, _) = helm_host(&["process:helm"], failed);

        assert_eq!(repo_update(&host, None), Err(Error::Failed("Error: no repositories found".into())));
    }

    #[test]
    fn repo_update_needs_no_open_editor() {
        let (host, _) = helm_host(&["process:helm"], ok("Update Complete.\n"));

        assert_eq!(repo_update(&host, None), Ok(None));

        assert_eq!(host.process_runs().len(), 1);
    }

    #[test]
    fn repo_update_reports_only_the_tail_of_its_output() {
        let lines: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let (host, _) = helm_host(&["process:helm"], ok(&lines));
        repo_update(&host, None).unwrap();
        let body = host.notifications()[0].body.clone();
        assert!(body.starts_with("line 11\n") && body.ends_with("line 30"), "{body}");

        let (host, _) = helm_host(&["process:helm"], ok(&"é".repeat(5000)));
        repo_update(&host, None).unwrap();
        assert_eq!(host.notifications()[0].body.chars().count(), 2000);
    }

    #[test]
    fn install_hint_spawns_no_process() {
        let host = host();

        KubeExplorer::default().run_command("how-to-install-kubectl", &host.invocation(), Some("--kubeconfig=/x")).unwrap();

        assert!(host.process_runs().is_empty());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn install_hint_text_is_static() {
        let host = host();
        host.set_setting("kubeconfig", "\"; rm -rf ~\"");
        let mut kube = KubeExplorer { context: Some("-evil".into()), ..KubeExplorer::default() };

        kube.run_command("how-to-install-kubectl", &host.invocation(), None).unwrap();

        assert_eq!(host.notifications()[0].body, KUBECTL_INSTALL_HINT);
        for expected in [
            "PATH",
            "https://kubernetes.io/docs/tasks/tools/",
            "brew install kubectl",
            "winget install -e --id Kubernetes.kubectl",
            "Linux",
        ] {
            assert!(KUBECTL_INSTALL_HINT.contains(expected), "{expected}");
        }
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn install_hint_needs_no_open_editor() {
        let host = MockHost::install().with_capabilities(&[]);

        assert_eq!(KubeExplorer::default().run_command("how-to-install-kubectl", &host.invocation(), None), Ok(None));

        assert_eq!(host.notifications().len(), 1);
    }

    fn toolbar(view: &View) -> Vec<String> {
        view.toolbar.iter().map(|b| b.id.clone()).collect()
    }

    #[test]
    fn tree_offers_the_install_hint_only_when_kubectl_fails() {
        let missing = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        missing.on_process(KUBECTL, |_, _| Err(Error::Failed("kubectl: not found".into())));
        let mut kube = KubeExplorer::default();
        assert!(toolbar(&kube.render_tree().unwrap()).contains(&"how-to-install-kubectl".to_owned()));
        let runs = missing.process_runs().len();
        kube.button("how-to-install-kubectl").unwrap();
        assert_eq!(missing.notifications()[0].body, KUBECTL_INSTALL_HINT);
        assert_eq!(missing.process_runs().len(), runs);
        drop(missing);

        let _working = host();
        assert!(!toolbar(&KubeExplorer::default().render_tree().unwrap()).contains(&"how-to-install-kubectl".to_owned()));
        drop(_working);

        let _denied = MockHost::install().with_capabilities(&["editor.read", "storage"]);
        assert!(!toolbar(&KubeExplorer::default().render_tree().unwrap()).contains(&"how-to-install-kubectl".to_owned()));
    }

    #[test]
    fn tree_hides_the_install_hint_when_kubectl_runs_but_has_no_context() {
        let host = MockHost::install().with_capabilities(&["process:kubectl", "editor.read", "storage"]);
        host.on_process(KUBECTL, |_, _| {
            Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "error: current-context is not set\n".into() })
        });

        let view = KubeExplorer::default().render_tree().unwrap();

        assert!(!toolbar(&view).contains(&"how-to-install-kubectl".to_owned()));
        assert!(toolbar(&view).contains(&"choose-kubeconfig".to_owned()));
        let Body::Tree(items) = &view.body else { panic!("tree") };
        assert!(items.iter().any(|item| item.label == "Context: (no current context)"));
    }

    #[test]
    fn repo_update_with_empty_output_still_says_it_finished() {
        let (host, _) = helm_host(&["process:helm"], ok(""));

        repo_update(&host, None).unwrap();

        assert_eq!(host.notifications()[0].body, "Helm repositories updated.");
    }

    #[test]
    fn resources_carry_the_icon_of_their_kind() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();
        let Body::Tree(items) = kube.render(PANEL, None, &host.invocation()).unwrap().body else {
            panic!("not a tree");
        };
        let icon = |key: &str| items.iter().find(|i| test_tree_key(&i.key)==key).and_then(|i| i.icon.clone());

        assert_eq!(icon("ctx").as_deref(), Some(k8s!("cluster")));
        assert_eq!(icon("kind:pods").as_deref(), Some(k8s!("pod")));
        assert_eq!(icon("res:pods/web-1").as_deref(), Some(k8s!("pod")));
    }

    #[test]
    fn an_expanded_kind_without_resources_has_a_placeholder_child() {
        let host = host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:jobs",&host.invocation())), &host.invocation()).unwrap();
        let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());

        assert!(tree.contains(&"kind:jobs!".to_owned()));
    }

    #[test]
    fn delete_runs_only_after_confirmation() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("delete")), &host.invocation()).unwrap();
        assert!(!host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("delete")), &host.invocation()).unwrap();
        assert!(host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));
    }

    #[test]
    fn the_context_menu_acts_on_the_right_clicked_resource() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.run_command("resource-logs", &host.invocation(), Some(&kube.test_resource_key("res:pods/web-1", &host.invocation()))).unwrap();
        kube.run_command("resource-describe", &host.invocation(), Some(&kube.test_resource_key("res:pods/web-1", &host.invocation()))).unwrap();
        assert_eq!(host.terminals().len(), 1);
        assert_eq!(host.terminals()[0].args.last().map(String::as_str), Some("web-1"));
        assert!(kube.details.is_some());
        assert_eq!(host.revealed_views(), [PANEL]);
    }

    #[test]
    fn the_context_menu_refuses_actions_that_do_not_fit_the_kind() {
        let host = host();
        let mut kube = KubeExplorer::default();
        assert!(kube.run_command("resource-shell", &host.invocation(), Some(&kube.test_resource_key("res:services/web", &host.invocation()))).is_err());
        assert!(kube.run_command("resource-logs", &host.invocation(), Some("kind:pods")).is_err());
        assert!(host.terminals().is_empty());
    }

    #[test]
    fn the_context_menu_delete_asks_first() {
        let host = host();
        let mut kube = KubeExplorer::default();
        kube.run_command("resource-delete", &host.invocation(), Some(&kube.test_resource_key("res:pods/web-1", &host.invocation()))).unwrap();
        assert!(!host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));

        host.answer_next_prompt(Some(&[]));
        kube.run_command("resource-delete", &host.invocation(), Some(&kube.test_resource_key("res:pods/web-1", &host.invocation()))).unwrap();
        assert!(host.process_runs().iter().any(|r| r.args.contains(&"delete".to_owned())));
    }

    #[test]
    fn every_context_menu_command_is_declared() {
        let actions = include_str!("ui_actions.json");
        for action in
            ["describe", "yaml", "logs", "log-view", "shell", "attach", "debug", "port-forward", "show-secret", "edit-secret", "delete"]
        {
            assert!(actions.contains(&format!(r#""command": "resource-{action}""#)), "{action}");
        }
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

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("crds",&host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("crd:cert-manager.io",&host.invocation())), &host.invocation()).unwrap();
        let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());

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

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("crds",&host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("crd:cert-manager.io",&host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:certificates.cert-manager.io",&host.invocation())), &host.invocation()).unwrap();
        let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());
        assert!(tree.contains(&"res:certificates.cert-manager.io/web-tls".to_owned()));

        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:certificates.cert-manager.io/web-tls", &host.invocation())), &host.invocation()).unwrap();
        assert_eq!(kube.selected, Some(("certificates.cert-manager.io".into(), "web-tls".into())));
    }

    #[test]
    fn built_in_groups_are_not_custom_resources() {
        let names = [
            "storageclasses.storage.k8s.io",
            "roles.rbac.authorization.k8s.io",
            "poddisruptionbudgets.policy",
            "horizontalpodautoscalers.autoscaling",
            "leases.coordination.k8s.io",
            "customresourcedefinitions.apiextensions.k8s.io",
            "pods.metrics.k8s.io",
            "storageversionmigrations.storagemigration.k8s.io",
            "gateways.gateway.networking.k8s.io",
            "certificates.cert-manager.io",
            "clusters.cluster.x-k8s.io",
            "volumesnapshots.snapshot.storage.k8s.io",
            "pods",
        ];

        let custom = custom_resource_names(names.iter().map(|n| (*n).to_owned()).collect());

        assert_eq!(
            custom,
            [
                "certificates.cert-manager.io",
                "clusters.cluster.x-k8s.io",
                "gateways.gateway.networking.k8s.io",
                "volumesnapshots.snapshot.storage.k8s.io",
            ]
        );
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
            kube.handle(TREE, ViewEvent::Expand(kube.test_node_key(key,&host.invocation())), &host.invocation()).unwrap();
        }
        assert_eq!(calls.get(), 1);
        let Body::Tree(mut items) = kube.render(PANEL, None, &host.invocation()).unwrap().body else {
            panic!("not a tree");
        };
        for item in &mut items {item.key=test_tree_key(&item.key).into();if let Some(parent)=&mut item.parent {*parent=test_tree_key(parent).into();}}
        items
    }

    #[test]
    fn groups_are_sorted_and_kinds_labelled_by_plural() {
        let items = crd_tree(ok("zeta.b.io\nalpha.a.io\nbeta.a.io\n"));
        let nodes: Vec<(&str, &str)> = items.iter().skip_while(|i| i.key != "crds").map(|i| (i.key.as_str(), i.label.as_str())).collect();

        assert_eq!(
            nodes,
            [
                ("crds", "Custom Resources"),
                ("crd:a.io", "a.io"),
                ("kind:alpha.a.io", "alpha"),
                ("kind:beta.a.io", "beta"),
                ("crd:b.io", "b.io"),
                ("kind:zeta.b.io", "zeta"),
            ]
        );
    }

    #[test]
    fn a_failing_api_resources_shows_an_error_node() {
        let items =
            crd_tree(Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "Error from server (Forbidden): nope\n".into() }));

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
        host.answer_next_prompt(None);

        assert_eq!(KubeExplorer::default().run_command("apply", &host.invocation(), None), Err(Error::Cancelled));
        assert!(!host.process_runs().iter().any(|r| r.args.contains(&"apply".to_owned())));

        host.answer_next_prompt(Some(&["apply"]));
        KubeExplorer::default().run_command("apply", &host.invocation(), None).unwrap();

        assert_eq!(host.notifications()[0].body, "minikube/(default): deployment.apps/web configured");
        assert_eq!(host.notifications_with_actions()[0].actions, [NotificationAction::new("Show in Kube Explorer", SHOW_TREE)]);
        assert_eq!(KubeExplorer::default().apply_question("/project/web.yaml"), "Apply /project/web.yaml to context minikube, namespace (default)?");
    }

    #[test]
    fn apply_can_show_the_diff_first_without_applying() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-1\n", &[]);

        host.answer_next_prompt(Some(&["diff"]));
        host.answer_next_prompt(None);
        assert_eq!(KubeExplorer::default().run_command("apply", &host.invocation(), None), Err(Error::Cancelled));

        assert!(!kubectl_calls(&host).iter().any(|args| args.contains(&"apply".to_owned()) && !args.contains(&"--dry-run=server".to_owned())));
        let (diff, name) = &host.opened_diffs()[0];
        assert_eq!(diff.left.as_deref(), Some("apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-1\n"));
        assert_eq!(diff.left_label.as_deref(), Some("Cluster (minikube/(default))"));
        assert_eq!(name.as_deref(), Some("web.yaml"));
    }

    #[test]
    fn apply_after_the_diff_asks_once_more_and_then_applies() {
        let host = host();
        host.open_editor("/project/web.yaml", "yaml", "apiVersion: v1\nkind: Pod\nmetadata:\n  name: web-1\n", &[]);

        host.answer_next_prompt(Some(&["diff"]));
        host.answer_next_prompt(Some(&[]));
        KubeExplorer::default().run_command("apply", &host.invocation(), None).unwrap();

        assert_eq!(host.opened_diffs().len(), 1);
        assert!(kubectl_calls(&host).iter().any(|args| args.ends_with(&["apply".into(), "-f".into(), "-".into()])));
    }

    #[test]
    fn diff_shows_a_new_resource_alone_and_keeps_secrets_masked() {
        let host = host();
        host.on_process(KUBECTL, |args, stdin| {
            if args.contains(&"diff".to_owned()) {
                return Ok(ProcessOutput { status: Some(1), stdout: "+ data: ***\n".into(), stderr: String::new() });
            }
            if args.contains(&"--dry-run=server".to_owned()) {
                return ok(stdin.unwrap_or_default());
            }
            ok(if args.contains(&"current-context".to_owned()) { "minikube\n" } else { "" })
        });
        host.open_editor("/project/new.yaml", "yaml", "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: new\n", &[]);
        KubeExplorer::default().run_command("diff", &host.invocation(), None).unwrap();
        let (diff, _) = &host.opened_diffs()[0];
        assert!(diff.left.is_none() && diff.right.as_deref().is_some_and(|r| r.contains("ConfigMap")));

        host.open_editor("/project/secret.yaml", "yaml", "apiVersion: v1\nkind: Secret\nmetadata:\n  name: s\n", &[]);
        let before = kubectl_calls(&host).len();
        KubeExplorer::default().run_command("diff", &host.invocation(), None).unwrap();
        assert_eq!(host.opened_diffs().len(), 1);
        let secret_calls = kubectl_calls(&host).split_off(before);
        assert_eq!(secret_calls.len(), 1, "{secret_calls:?}");
        assert!(secret_calls[0].contains(&"diff".to_owned()));
    }

    const LIVE_LIST: &str = "apiVersion: v1\nkind: List\nitems:\n- apiVersion: v1\n  kind: Service\n  metadata:\n    name: web\n    namespace: shop\n    uid: 1\n    resourceVersion: \"9\"\n    creationTimestamp: \"2026-01-01T00:00:00Z\"\n    annotations:\n      kubectl.kubernetes.io/last-applied-configuration: '{}'\n    managedFields:\n    - manager: kubectl\n  spec:\n    ports:\n    - port: 80\n  status:\n    loadBalancer: {}\n- apiVersion: v1\n  kind: ConfigMap\n  metadata:\n    name: web\n    namespace: shop\n    generation: 2\n    annotations:\n      team: a\n      kubectl.kubernetes.io/last-applied-configuration: '{}'\n  data:\n    b: \"2\"\n    a: \"1\"\n";

    #[test]
    fn cluster_objects_drop_server_fields_and_order_documents_by_kind_namespace_and_name() {
        let normalized = cluster_objects(LIVE_LIST).unwrap();

        assert_eq!(
            normalized,
            "apiVersion: v1\ndata:\n  a: \"1\"\n  b: \"2\"\nkind: ConfigMap\nmetadata:\n  annotations:\n    team: a\n  name: web\n  namespace: shop\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: web\n  namespace: shop\nspec:\n  ports:\n    - port: 80\n"
        );
        let local = "kind: Service\napiVersion: v1\nmetadata:\n  namespace: shop\n  name: web\nspec:\n  ports:\n  - port: 80\n---\napiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: web\n  namespace: shop\n  annotations:\n    team: a\ndata:\n  a: \"1\"\n  b: \"2\"\n";
        assert_eq!(convert::source_documents(local).and_then(normalized_objects).unwrap(), normalized);
        assert_eq!(cluster_objects("apiVersion: v1\nkind: List\nitems: []\n").as_deref(), Some(""));
        assert!(cluster_objects("not: [valid").is_none());
    }

    #[test]
    fn diff_compares_live_objects_with_the_server_dry_run_both_normalized() {
        let host = host();
        host.on_process(KUBECTL, |args, _| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("config") => ok("minikube\n"),
            Some("get") => ok(LIVE_LIST),
            Some("apply") => ok(&LIVE_LIST.replace("port: 80", "port: 8080").replace("uid: 1", "uid: 2")),
            _ => ok(""),
        });
        host.open_editor("/project/web.yaml", "yaml", "apiVersion: v1\nkind: Service\nmetadata:\n  name: web\n", &[]);
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };

        kube.run_command("diff", &host.invocation(), None).unwrap();

        let calls = kubectl_calls(&host);
        let dry_run = calls.iter().find(|args| args.contains(&"apply".to_owned())).unwrap();
        assert_eq!(dry_run, &["--namespace=shop", "apply", "--dry-run=server", "-f", "-", "-o", "yaml", "--request-timeout=4s"]);
        assert_eq!(calls.iter().filter(|args| args.contains(&"apply".to_owned())).count(), 1);
        let (diff, _) = &host.opened_diffs()[0];
        let (left, right) = (diff.left.as_deref().unwrap(), diff.right.as_deref().unwrap());
        assert_eq!(left, cluster_objects(LIVE_LIST).unwrap());
        assert_eq!(right, left.replace("port: 80", "port: 8080"));
        assert_eq!(diff.right_label.as_deref(), Some("After apply, server dry run (web.yaml)"));
    }

    #[test]
    fn a_refused_dry_run_compares_the_normalized_file_and_says_why() {
        let host = host();
        host.on_process(KUBECTL, |args, _| match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
            Some("config") => ok("minikube\n"),
            Some("apply") => Ok(ProcessOutput { status: Some(1), stdout: String::new(), stderr: "\nError: forbidden: cannot patch\n".into() }),
            _ => ok(""),
        });
        let shown = KubeExplorer::default().diff_manifest("metadata:\n  name: new\nkind: ConfigMap\napiVersion: v1\n", "/project/new.yaml").unwrap();

        assert_eq!(shown, "These resources are not in the cluster yet. The server dry run failed (Error: forbidden: cannot patch), so the file is shown as written.");
        let (diff, _) = &host.opened_diffs()[0];
        assert!(diff.left.is_none());
        assert_eq!(diff.right.as_deref(), Some("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: new\n"));
        assert_eq!(diff.right_label.as_deref(), Some("Local (new.yaml)"));
    }

    #[test]
    fn status_bar_names_the_context_and_namespace_and_hides_without_one() {
        let host = host();
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };
        kube.render(TREE, None, &host.invocation()).unwrap();
        let item = &host.status_items()[CONTEXT_STATUS];
        assert_eq!(item.text, "⎈ minikube / shop");
        assert_eq!(item.command.as_deref(), Some(SHOW_TREE));

        kube.current_context = Some(NO_CONTEXT.into());
        kube.update_context_status();
        assert!(!host.status_items().contains_key(CONTEXT_STATUS));
    }

    #[test]
    fn status_bar_menu_switches_and_runs_declared_commands() {
        let host = host();
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };
        kube.render(TREE, None, &host.invocation()).unwrap();
        let menu = host.status_menu(CONTEXT_STATUS);
        assert_eq!(menu, status_menu());
        let manifest = include_str!("../plugin.toml");
        for entry in &menu {
            if let StatusMenuEntry::Action { command, .. } = entry {
                assert!(manifest.contains(&format!("id = \"{command}\"")), "{command}");
            }
        }
        assert!(host.status_items()[CONTEXT_STATUS].tooltip.as_deref().unwrap().ends_with("Click to switch."));
    }

    #[test]
    fn switch_from_the_palette_or_status_bar_picks_a_context_or_namespace() {
        let host = host();
        let mut kube = KubeExplorer { current_context: Some("minikube".into()), namespace: Some("shop".into()), ..Default::default() };
        host.answer_next_prompt(Some(&["ctx/arn:aws:eks:eu-west-1:1:cluster/prod"]));
        kube.run_command("tree-switch", &host.invocation(), Some("ctx")).unwrap();
        assert_eq!((kube.context.as_deref(), kube.namespace.as_deref()), (Some("arn:aws:eks:eu-west-1:1:cluster/prod"), None));
        assert!(host.status_items()[CONTEXT_STATUS].text.starts_with("⎈ arn:aws:eks:eu-west-1:1:cluster/prod / "));

        host.answer_next_prompt(Some(&[&format!("ns/{ALL_NAMESPACES}")]));
        kube.run_command("tree-switch", &host.invocation(), Some("ns")).unwrap();
        assert_eq!(kube.namespace.as_deref(), Some(ALL_NAMESPACES));

        host.answer_next_prompt(None);
        kube.run_command("tree-switch", &host.invocation(), None).unwrap();
        assert_eq!(kube.namespace.as_deref(), Some(ALL_NAMESPACES));
    }

    #[test]
    fn apply_copied_yaml_names_the_resources_and_applies_after_the_choice() {
        const COPIED: &str = "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n---\napiVersion: v1\nkind: Service\nmetadata:\n  name: web\n";
        let applied = |host: &MockHost| kubectl_calls(host).iter().any(|call| call.contains(&"apply".to_owned()));
        let host = host().with_capabilities(&["process:kubectl", "project.read", "editor.read", "storage", "clipboard.read"]);
        let mut kube = KubeExplorer { current_context: Some("minikube".into()), namespace: Some("shop".into()), ..Default::default() };

        host.set_clipboard(Some(COPIED), None);
        host.answer_next_prompt(None);
        assert!(matches!(kube.run_command("apply-copied-yaml", &host.invocation(), None), Err(Error::Cancelled)));
        assert!(!applied(&host));

        host.answer_next_prompt(Some(&["apply"]));
        kube.run_command("apply-copied-yaml", &host.invocation(), None).unwrap();
        assert!(applied(&host));
        assert_eq!(host.notifications().last().unwrap().title, "Copied YAML applied");

        host.set_clipboard(Some("just some text"), None);
        let error = kube.run_command("apply-copied-yaml", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("doesn't hold a Kubernetes manifest"), "{error}");
        host.set_clipboard(None, None);
        let error = kube.run_command("apply-copied-yaml", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("holds no text"), "{error}");
    }

    #[test]
    fn apply_copied_yaml_without_the_permission_says_where_to_allow_it() {
        let host = host();
        host.set_clipboard(Some("apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: a\n"), None);
        let mut kube = KubeExplorer { current_context: Some("minikube".into()), ..Default::default() };
        let error = kube.run_command("apply-copied-yaml", &host.invocation(), None).unwrap_err().to_string();
        assert!(error.contains("clipboard.read") && error.contains("Kube Explorer"), "{error}");
        assert!(kubectl_calls(&host).iter().all(|call| !call.contains(&"apply".to_owned())));
    }

    #[test]
    fn repeating_open_shell_focuses_the_same_terminal() {
        let host = host();
        let mut kube = KubeExplorer { current_context: Some("minikube".into()), ..Default::default() };
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/web-1", &host.invocation())), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("shell")), &host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("shell")), &host.invocation()).unwrap();

        assert!(host.terminals().is_empty());
        let key = kube.shell_terminal_key(None, "web-1");
        assert!(host.finish_terminal(&key).is_some());
        assert!(host.finish_terminal(&key).is_none());
    }

    #[test]
    fn delete_question_names_kind_name_namespace_and_context() {
        let _host = host();
        let kube = KubeExplorer { context: Some("prod-eu".into()), namespace: Some("shop".into()), ..Default::default() };
        assert_eq!(kube.delete_question("pods", "web-1").unwrap(), "Delete pods/web-1 in namespace shop on context prod-eu?");
        assert_eq!(kube.delete_question("nodes", "node-a").unwrap(), "Delete nodes/node-a on context prod-eu?");
        let all = KubeExplorer { namespace: Some(ALL_NAMESPACES.into()), current_context: Some("minikube".into()), ..Default::default() };
        assert_eq!(all.delete_question("pods", "shop/web-1").unwrap(), "Delete pods/web-1 in namespace shop on context minikube?");
    }

    #[test]
    fn apply_confirms_target() {
        for command in ["apply", "kustomize-apply"] {
            let host = host();
            host.open_editor("/project/k/web.yaml", "yaml", "kind: Deployment", &[]);
            let mut kube = KubeExplorer { current_context: Some("minikube".into()), ..Default::default() };

            host.answer_next_prompt(None);
            assert!(matches!(kube.run_command(command, &host.invocation(), None), Err(Error::Cancelled)), "{command}");
            assert!(kubectl_calls(&host).iter().all(|call| !call.contains(&"apply".to_owned())), "{command}");

            host.answer_next_prompt(Some(&["apply"]));
            kube.run_command(command, &host.invocation(), None).unwrap();
            assert!(kubectl_calls(&host).iter().any(|call| call.contains(&"apply".to_owned())), "{command}");
        }
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
        assert_eq!(templates.len(), 16);
        let ids: BTreeSet<_> = templates.iter().map(|t| &t.0).collect();
        let names: BTreeSet<_> = templates.iter().map(|t| &t.1).collect();
        assert_eq!((ids.len(), names.len()), (16, 16));
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
        assert_eq!(args, ["--context=prod", "--namespace=shop", "debug", "-it", "web-1", "--image=busybox:stable", "--target=app"]);
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
        let mut kube = KubeExplorer { selected: Some(("pods".into(), "web-1".into())), ..Default::default() };
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
            "{{\"apiVersion\":\"v1\",\"data\":{{{}}},\"kind\":\"Secret\",\"metadata\":{{\"annotations\":{{\"a\":\"{{\\\"data\\\":{{\\\"x\\\":\\\"!\\\"}}}}\"}},\"name\":\"db\",\"namespace\":\"default\"}},\"type\":\"Opaque\"}}",
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
    fn native_multiline_field_clip_cannot_silently_truncate_revealed_binary_secret() {
        let bytes=vec![0xff;1<<20];
        let (_host,_)=secret_host(true,secret_json(&[("blob",&bytes)]),"");let mut kube=selected_secret();
        kube.button("show-secret").unwrap();
        let Body::Form(fields)=kube.render_tree().unwrap().body else{panic!("revealed fields")};
        // Exact pinned254 runtime converter clips Multiline text at1MiB bytes.
        let mut visible=String::new();
        for field in fields {
            let FieldValue::Multiline(text)=field.value else{panic!("multiline")};
            let mut end=text.len().min(1<<20);while !text.is_char_boundary(end){end-=1;}
            visible.push_str(&text[..end]);
        }
        let expected=secrets::display(&bytes);
        assert!(visible==expected,"native presentation lost binary display bytes: {} of {}",visible.len(),expected.len());
        let unicode=format!("{}😀tail","x".repeat((1<<20)-1));
        let view=kube.render_revealed("db",&[("unicode".into(),unicode.clone())]);
        assert!(view.status.unwrap().contains("without separators"));
        let Body::Form(fields)=view.body else{panic!("parts")};assert_eq!(fields.len(),2);
        let mut joined=String::new();
        for (part,field) in fields.into_iter().enumerate() {
            assert!(field.label.contains(&format!("part {} of 2",part+1)));assert!(field.key.len()<=128);
            let FieldValue::Multiline(text)=field.value else{panic!("multiline")};assert!(text.len()<=1<<20);joined.push_str(&text);
        }
        assert!(joined==unicode);

    }

    #[test]
    fn secret_values_are_base64_decoded() {
        let (_host, _) = secret_host(true, secret_json(&[("password", b"pa\"ss\nw\xc3\xa9"), ("user", b"admin")]), "");
        let mut kube = selected_secret();
        kube.button("show-secret").unwrap();
        let Body::Form(fields) = kube.render_tree().unwrap().body else { panic!("not a form") };
        let shown: Vec<_> = fields.iter().map(|f| (f.label.as_str(), f.value.clone())).collect();
        assert_eq!(shown, [("password", FieldValue::Multiline("pa\"ss\nwé".into())), ("user", FieldValue::Multiline("admin".into())),]);
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
        assert!(notes[0].body.contains("Allow Show Secret and YAML on a secret"));
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
        assert!(!error.contains(value) && !error.contains(&encoded) && error.contains("diagnostics withheld"));
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
        assert!(notes[0].body.contains("Allow Show Secret and YAML on a secret"));
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

    const CAPS: &[&str] = &["process:kubectl", "process:tee", "editor.read", "storage", "events", "project.read"];

    fn all_host() -> MockHost {
        let host = MockHost::install().with_capabilities(CAPS);
        host.set_setting("allowRevealSecrets", "true");
        host.set_setting("yamlInEditor", "true");
        host.on_process("tee", |_, _| ok(""));
        host.on_process(KUBECTL, |args, _| {
            let has = |a: &str| args.iter().any(|x| x == a);
            match args.iter().find(|a| !a.starts_with("--")).map(String::as_str) {
                Some("config") if has("view") => ok(r#"{"contexts":[{"name":"minikube","context":{"cluster":"minikube","namespace":"default"}}],"clusters":[{"name":"minikube","cluster":{"server":"https://minikube.invalid"}}]}"#),
                Some("config") => ok("minikube\n"),
                Some("get") if has("--all-namespaces") => ok("team-a/web\nteam-b/api\n"),
                Some("get") if has("nodes") => ok("node/node-1\n"),
                Some("get") if has("namespaces") => ok("namespace/team-a\nnamespace/team-b\n"),
                Some("get") if has("yaml") => ok("apiVersion: v1\nkind: Pod\nmetadata:\n  name: web\n  namespace: team-a\n"),
                Some("get") if has("json") => ok(r#"{"apiVersion":"v1","kind":"Secret","metadata":{"name":"web","namespace":"team-a","resourceVersion":"1"},"data":{}}"#),
                Some("get") if args.iter().any(|a| a.starts_with("jsonpath=")) => ok("80"),
                Some("describe") => ok("Name: web\n"),
                _ => ok(""),
            }
        });
        host
    }

    fn all_mode() -> KubeExplorer {
        KubeExplorer { namespace: Some(ALL_NAMESPACES.into()), current_context: Some("minikube".into()), ..Default::default() }
    }

    fn press(kube: &mut KubeExplorer, kind: &str, row: &str, button: &str, host: &MockHost) -> Result<(), Error> {
        kube.selected = Some((kind.into(), row.into()));
        kube.confirm_delete = false;
        kube.forward = None;
        kube.button_in(button, Some(&host.invocation()))?;
        match button {
            "delete" => kube.button_in("delete", Some(&host.invocation())),
            "port-forward" => kube.button_in("start-forward", Some(&host.invocation())),
            _ => Ok(()),
        }
    }

    /// Every argument list the actions passed to kubectl, in runs and terminals.
    fn kubectl_calls(host: &MockHost) -> Vec<Vec<String>> {
        let runs = host.process_runs().into_iter().filter(|r| r.program == KUBECTL).map(|r| r.args);
        runs.chain(host.terminals().into_iter().map(|t| t.args)).collect()
    }

    const ROW_ACTIONS: &[(&str, &str)] = &[
        ("pods", "describe"),
        ("pods", "yaml"),
        ("pods", "logs"),
        ("deployments", "logs"),
        ("pods", "log-view"),
        ("pods", "shell"),
        ("pods", "attach"),
        ("pods", "debug"),
        ("secrets", "show-secret"),
        ("secrets", "edit-secret"),
        ("services", "port-forward"),
        ("pods", "delete"),
    ];

    fn assert_refused(rows: &[&str]) {
        for row in rows {
            for (kind, button) in ROW_ACTIONS {
                let host = all_host();
                let mut kube = all_mode();
                assert!(press(&mut kube, kind, row, button, &host).is_err(), "{kind} {row:?} {button}");
                assert!(kubectl_calls(&host).is_empty(), "{kind} {row:?} {button}: {:?}", kubectl_calls(&host));
            }
        }
    }

    #[test]
    fn all_namespaces_node_is_the_first_namespace_child() {
        let host = all_host();
        let mut kube = KubeExplorer::default();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("ns",&host.invocation())), &host.invocation()).unwrap();
        let Body::Tree(items) = kube.render(PANEL, None, &host.invocation()).unwrap().body else { panic!("not a tree") };

        let at = items.iter().position(|i| test_tree_key(&i.key) == "ns").unwrap();
        assert_eq!(items[at + 1].key, kube.test_choice_key("ns/*",&host.invocation()));
        assert_eq!(items[at + 1].label, "All namespaces");
        assert_eq!(items[at + 1].parent.as_deref(), Some(kube.test_node_key("ns",&host.invocation()).as_str()));
        assert_eq!(items[at + 2].key, kube.test_choice_key("ns/team-a",&host.invocation()));
    }

    #[test]
    fn choosing_all_namespaces_persists_star_and_lists_with_all_namespaces_flag() {
        let host = all_host();
        let mut kube = KubeExplorer { namespace: Some("shop".into()), ..Default::default() };

        kube.handle(TREE, ViewEvent::Activate(kube.test_choice_key("ns/*",&host.invocation())), &host.invocation()).unwrap();
        kube.render(TREE,None,&host.invocation()).unwrap();
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();

        assert_eq!(host.storage()["namespace"], "*");
        let runs = host.process_runs();
        let list = runs.iter().find(|r| r.args.contains(&"pods".to_owned())).unwrap();
        assert!(list.args.windows(3).any(|w| w == ["get", "pods", "--all-namespaces"]), "{:?}", list.args);
        assert!(list.args.contains(&"jsonpath={range .items[*]}{.metadata.namespace}{\"/\"}{.metadata.name}{\"\\n\"}{end}".to_owned()));
        assert!(runs.iter().all(|r| !r.args.iter().any(|a| a.starts_with("--namespace"))), "{runs:?}");
        assert!(kube.target().ends_with("/(all namespaces)"), "{}", kube.target());
    }

    #[test]
    fn rows_show_their_namespace_and_name() {
        let host = all_host();
        let mut kube = all_mode();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();
        let Body::Tree(items) = kube.render(PANEL, None, &host.invocation()).unwrap().body else { panic!("not a tree") };

        let row = items.iter().find(|i| i.key.split('~').next() == Some("res:pods/team-a/web")).unwrap();
        assert_eq!(row.label, "team-a/web");
        assert!(items.iter().any(|i| i.key.split('~').next() == Some("res:pods/team-b/api")));
    }

    #[test]
    fn cluster_scoped_kinds_have_no_namespace_prefix() {
        let host = all_host();
        let mut kube = all_mode();

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:nodes",&host.invocation())), &host.invocation()).unwrap();
        let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:nodes/node-1", &host.invocation())), &host.invocation()).unwrap();
        kube.button("describe").unwrap();

        assert!(tree.contains(&"res:nodes/node-1".to_owned()), "{tree:?}");
        let calls = kubectl_calls(&host);
        assert!(!calls[0].contains(&"--all-namespaces".to_owned()));
        assert!(calls.iter().all(|args| !args.iter().any(|a| a.starts_with("--namespace"))), "{calls:?}");
        assert!(calls[1].ends_with(&["describe".into(), "nodes".into(), "node-1".into(), REQUEST_TIMEOUT.into()]));
    }

    #[test]
    fn describe_and_panel_yaml_in_all_mode_pass_the_rows_namespace() {
        let host = all_host();host.set_setting("yamlInEditor","false");
        let mut kube = all_mode();
        kube.handle(TREE, ViewEvent::Activate(kube.test_resource_key("res:pods/team-a/web", &host.invocation())), &host.invocation()).unwrap();
        press(&mut kube, "pods", "team-a/web", "yaml", &host).unwrap();
        let calls = kubectl_calls(&host);
        assert!(calls[0].ends_with(&["describe".into(), "pods".into(), "web".into(), REQUEST_TIMEOUT.into()]));
        assert!(calls[1].ends_with(&["get".into(), "pods".into(), "web".into(), "-o".into(), "yaml".into(), REQUEST_TIMEOUT.into()]));
        assert!(calls.iter().all(|args| args.contains(&"--namespace=team-a".to_owned())));
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn yaml_applied_in_the_panel_keeps_the_rows_namespace() {
        let host = all_host().with_capabilities(&["process:kubectl", "project.read", "editor.read", "storage"]);
        host.set_setting("yamlInEditor","false");
        let mut kube = all_mode();
        press(&mut kube, "pods", "team-a/web", "yaml", &host).unwrap();

        host.answer_next_prompt(Some(&[]));
        kube.handle(TREE, ViewEvent::Message(kube.test_apply_message("kind: Pod")), &host.invocation()).unwrap();
        kube.button("reload-yaml").unwrap();

        let calls = kubectl_calls(&host);
        assert!(calls.iter().any(|args| args.ends_with(&["replace".into(), "-f".into(), "-".into()])));
        assert!(calls.iter().all(|args| args.contains(&"--namespace=team-a".to_owned())), "{calls:?}");
    }

    #[test]
    fn delete_in_all_namespaces_targets_the_rows_namespace() {
        let host = all_host();
        let mut kube = all_mode();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/team-a/web", &host.invocation())), &host.invocation()).unwrap();

        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("delete")), &host.invocation()).unwrap();
        assert!(kubectl_calls(&host).iter().all(|args| !args.contains(&"delete".to_owned())));
        kube.handle(TREE, ViewEvent::Button(kube.test_control_key("delete")), &host.invocation()).unwrap();

        let calls = kubectl_calls(&host);
        let delete = calls.iter().find(|args| args.contains(&"delete".to_owned())).unwrap();
        assert!(delete.contains(&"--namespace=team-a".to_owned()), "{delete:?}");
        assert!(delete.ends_with(&["delete".into(), "pods".into(), "web".into(), "--wait=false".into()]), "{delete:?}");
        assert!(calls.iter().flatten().all(|a| !a.contains("team-a/web")));
    }

    #[test]
    fn no_resource_action_runs_without_a_namespace_in_all_mode() {
        assert_refused(&["web", "/web"]);
    }

    #[test]
    fn row_with_option_like_namespace_is_refused() {
        assert_refused(&["--all/x", "-n/x"]);
    }

    #[test]
    fn row_with_option_like_name_is_refused() {
        assert_refused(&["team-a/--force", "team-a/-x"]);
    }

    #[test]
    fn row_with_dotdot_or_extra_slash_is_refused() {
        assert_refused(&["../x", "a/b/c", "a//b", "team-a/..", "Team-A/web", "team-a/we b"]);
        for valid in ["team-a", "a", "0x"] {
            assert!(is_dns_label(valid), "{valid}");
        }
        for invalid in ["", "-a", "a-", "A", "a.b", "a_b", "..", &"a".repeat(64)] {
            assert!(!is_dns_label(invalid), "{invalid}");
        }
        for valid in ["web", "web-1.v2", &"a".repeat(253)] {
            assert!(is_dns_subdomain(valid), "{valid}");
        }
        for invalid in ["", ".", "..", "a..b", ".a", "a.", "a/b", "-a", "a.-b", &"a".repeat(254)] {
            assert!(!is_dns_subdomain(invalid), "{invalid}");
        }
    }

    #[test]
    fn namespace_reaches_kubectl_as_one_equals_argument() {
        let host = all_host();
        let mut kube = all_mode();

        for (kind, button) in ROW_ACTIONS {
            press(&mut kube, kind, "team-a/web", button, &host).unwrap();
        }

        let calls = kubectl_calls(&host);
        assert!(calls.len() >= ROW_ACTIONS.len(), "{calls:?}");
        // Local kubeconfig discovery has no resource namespace. Resource reads
        // and actions still use the namespace carried by the selected row.
        for args in calls.iter().filter(|args| !args.iter().any(|arg| arg == "config")) {
            assert_eq!(args.iter().filter(|a| a.starts_with("--namespace")).collect::<Vec<_>>(), ["--namespace=team-a"], "{args:?}");
            assert!(!args.iter().any(|a| a == "-n" || a == "team-a" || a.contains("team-a/")), "{args:?}");
        }
    }

    #[test]
    fn create_secret_is_refused_in_all_namespaces_mode() {
        let host = all_host();
        let mut kube = all_mode();

        let error = kube.button("create-secret").unwrap_err().to_string();
        assert!(error.contains("pick a namespace first"), "{error}");
        assert!(kube.draft.is_none());

        kube.draft = Some(Draft { name: "db".into(), key: "k".into(), value: "v".into(), entries: Vec::new() });
        assert!(kube.button("submit-secret").is_err());
        assert!(kubectl_calls(&host).is_empty());
        let view = kube.actions(View::new(Body::Message(String::new())));
        assert!(!view.toolbar.iter().find(|b| b.id == "create-secret").unwrap().enabled);
    }

    #[test]
    fn manifest_writes_are_refused_in_all_namespaces_mode() {
        for command in ["apply", "kustomize-apply", "delete-manifest"] {
            let host = host();
            host.open_editor("/project/k/web.yaml", "yaml", "kind: Deployment", &[]);
            let mut kube = all_mode();

            let error = kube.run_command(command, &host.invocation(), None).unwrap_err().to_string();

            assert!(error.contains("pick a namespace first"), "{command}: {error}");
            assert!(kubectl_calls(&host).is_empty(), "{command}");
        }
    }

    #[test]
    fn confirm_delete_manifest_is_refused_in_all_namespaces_mode() {
        let host = host();
        let mut kube = all_mode();
        kube.pending_delete = Some("kind: Deployment".into());

        let error = kube.button("confirm-delete-manifest").unwrap_err().to_string();

        assert!(error.contains("pick a namespace first"), "{error}");
        assert!(kubectl_calls(&host).is_empty());
    }

    #[test]
    fn manifest_reads_still_run_in_all_namespaces_mode() {
        for command in ["dry-run", "diff", "kustomize-render"] {
            let host = host();
            host.open_editor("/project/k/web.yaml", "yaml", "kind: Deployment", &[]);

            all_mode().run_command(command, &host.invocation(), None).unwrap();

            let calls = kubectl_calls(&host);
            assert_eq!(calls.len(), if command == "diff" { 2 } else { 1 }, "{command}");
            assert!(!calls.iter().flatten().any(|a| a.starts_with("--namespace")), "{command}: {calls:?}");
        }
    }

    #[test]
    fn manifest_writes_name_the_namespace_in_single_namespace_mode() {
        for (command, expected) in [
            ("apply", &["--namespace=shop", "apply", "-f", "-"][..]),
            ("kustomize-apply", &["--namespace=shop", "apply", "-k", "/project/k"]),
            ("delete-manifest", &["--namespace=shop", "delete", "--dry-run=server", "-f", "-"]),
        ] {
            let host = host();
            host.open_editor("/project/k/web.yaml", "yaml", "kind: Deployment", &[]);
            let mut kube = KubeExplorer { namespace: Some("shop".into()), current_context: Some("minikube".into()), ..Default::default() };
            if command != "delete-manifest" {
                host.answer_next_prompt(Some(&["apply"]));
            }

            host.answer_next_prompt(Some(&["ok"]));
            kube.run_command(command, &host.invocation(), None).unwrap();

            assert_eq!(kubectl_calls(&host)[0], expected, "{command}");
            if command == "delete-manifest" {
                host.answer_next_prompt(Some(&[]));
                kube.button("confirm-delete-manifest").unwrap();
                assert_eq!(kubectl_calls(&host).last().unwrap(), &["--namespace=shop", "delete", "-f", "-"]);
            }
        }
    }

    #[test]
    fn refresh_button_reloads_expanded_nodes() {
        for namespace in [None, Some(ALL_NAMESPACES)] {
            let host = MockHost::install().with_capabilities(CAPS);
            let listing = std::rc::Rc::new(std::cell::RefCell::new("pod/old\n"));
            let shown = listing.clone();
            host.on_process(KUBECTL, move |args, _| match args.contains(&"--all-namespaces".to_owned()) {
                true => ok(&shown.borrow().replace("pod/", "team-a/")),
                false => ok(&shown.borrow()),
            });
            let mut kube =
                KubeExplorer { namespace: namespace.map(str::to_owned), current_context: Some("minikube".into()), ..Default::default() };
            kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();

            *listing.borrow_mut() = "pod/new\n";
            kube.handle(TREE, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
            let tree = keys(kube.render(PANEL, None, &host.invocation()).unwrap());

            let new = if namespace.is_some() { "res:pods/team-a/new" } else { "res:pods/new" };
            assert!(tree.contains(&new.to_owned()), "{namespace:?}: {tree:?}");
            assert!(!tree.iter().any(|k| k.ends_with("/old")), "{namespace:?}: {tree:?}");
        }
    }

    #[test]
    fn existing_single_namespace_behaviour_is_unchanged() {
        let host = all_host();
        let mut kube = KubeExplorer { namespace: Some("shop".into()), current_context: Some("minikube".into()), ..Default::default() };

        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kind:pods",&host.invocation())), &host.invocation()).unwrap();
        press(&mut kube, "pods", "web-1", "delete", &host).unwrap();

        let calls = kubectl_calls(&host);
        assert_eq!(calls[0], ["--namespace=shop", "get", "pods", "-o", "name", REQUEST_TIMEOUT]);
        assert_eq!(calls[1], ["--namespace=shop", "delete", "pods", "web-1", "--wait=false"]);
        assert!(kube.target().ends_with("/shop"));
    }
}

#[cfg(test)]
mod services_tests {
    use super::*;
    use openidea_plugin::details::{Details as DetailsProvider, Pane};
    use openidea_plugin::mock::MockHost;

    #[test]
    fn services_keeps_the_tree_when_a_yaml_panel_is_open() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        let mut kube = KubeExplorer::default();
        kube.current_context = Some("test".into());
        kube.expire_rich_context(Some(&host.invocation()));
        kube.show_yaml("pods", "web", "kind: Pod".into(), None).unwrap();
        assert!(matches!(kube.render(TREE, None, &host.invocation()).unwrap().body, Body::Tree(_)));
        assert!(matches!(kube.render(PANEL, None, &host.invocation()).unwrap().body, Body::Html(_)));
    }

    #[test]
    fn service_log_preserves_context_and_all_namespace_row_scope() {
        let host = MockHost::install();
        let mut kube = KubeExplorer::default();
        kube.context = Some("staging".into());
        kube.namespace = Some(ALL_NAMESPACES.into());
        let key=kube.test_resource_key("res:pods/team-a/web", &host.invocation());
        let tabs = DetailsProvider::detail(&mut kube, TREE, &key, &host.invocation()).unwrap();
        let Pane::Log(log) = &tabs[1].body else { panic!("a managed log") };
        assert_eq!(log.program, KUBECTL);
        assert!(log.args.contains(&"--context=staging".into()));
        assert!(log.args.contains(&"--namespace=team-a".into()));
        assert!(log.args.contains(&"pods/web".into()));
        assert!(!log.args.iter().any(|a| a.contains(ALL_NAMESPACES)));
    }
}

#[cfg(test)]
mod services_status_regression {
    use super::*;
    use openidea_plugin::{
        details::{Details, Pane},
        mock::MockHost,
    };
    #[test]
    fn denied_cli_stays_visible_as_a_non_resource_services_row() {
        let host = MockHost::install().with_capabilities(&["project.read", "storage"]);
        let mut plugin = KubeExplorer::default();
        let view = plugin.render(TREE, None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Tree(ref rows) if rows.len()==1 && test_tree_key(&rows[0].key)=="kube:status"));
        let Body::Tree(rows)=view.body else{panic!("status row")};let key=rows[0].key.clone();
        plugin.handle(TREE, ViewEvent::Select(key.clone()), &host.invocation()).unwrap();
        let tabs = plugin.detail(TREE, &key, &host.invocation()).unwrap();
        assert!(matches!(&tabs[0].body, Pane::Message(text) if text.contains("not activated automatically")));
        assert!(plugin.selected.is_none());
        assert!(host.process_runs().is_empty());
        assert!(host.requests().is_empty());
    }
}

#[cfg(test)]
mod services_cluster_group_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn setup() -> (MockHost, KubeExplorer) {
        let host = MockHost::install().with_capabilities(&["project.read", "storage"]);
        let mut kube = KubeExplorer { context: Some("qa-synthetic".into()), namespace: Some("qa-demo".into()), ..Default::default() };
        kube.expanded.insert("kind:pods".into());
        kube.children.insert("kind:pods".into(), Ok(vec!["qa-web".into()]));
        (host, kube)
    }
    fn rows(view: View) -> Vec<TreeItem> { let Body::Tree(mut rows) = view.body else { panic!("tree expected") }; for row in &mut rows {row.key=test_tree_key(&row.key).into();if let Some(parent)=&mut row.parent {*parent=test_tree_key(parent).into();}} rows }

    #[test]
    fn one_selected_context_parent_keeps_resource_keys_and_owners() {
        let (host, mut kube) = setup();
        let items = rows(kube.render(TREE, None, &host.invocation()).unwrap());
        let roots: Vec<_> = items.iter().filter(|r| r.parent.is_none()).collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].key, "kube:cluster");
        assert!(roots[0].label.contains("qa-synthetic"));
        for key in ["ctx", "ns", "kind:pods", "kind:services", "crds"] {
            assert_eq!(items.iter().find(|r| r.key == key).unwrap().parent.as_deref(), Some("kube:cluster"));
        }
        assert_eq!(items.iter().find(|r| r.key.split('~').next() == Some("res:pods/qa-web")).unwrap().parent.as_deref(), Some("kind:pods"));
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn synthetic_parent_expansion_and_collapse_do_not_fetch_or_lose_cached_selection() {
        let (host, mut kube) = setup();
        kube.handle(TREE, ViewEvent::Select(kube.test_resource_key("res:pods/qa-web", &host.invocation())), &host.invocation()).unwrap();
        let selected = kube.selected.clone();
        kube.handle(TREE, ViewEvent::Collapse(kube.test_node_key("kube:cluster",&host.invocation())), &host.invocation()).unwrap();
        let collapsed = rows(kube.render(TREE, None, &host.invocation()).unwrap());
        assert_eq!(collapsed.len(), 1);
        assert_eq!(kube.selected, selected);
        kube.handle(TREE, ViewEvent::Expand(kube.test_node_key("kube:cluster",&host.invocation())), &host.invocation()).unwrap();
        let expanded = rows(kube.render(TREE, None, &host.invocation()).unwrap());
        assert!(expanded.iter().any(|r| r.key.split('~').next() == Some("res:pods/qa-web")));
        assert_eq!(kube.selected, selected);
        assert!(!kube.children.contains_key("kube:cluster"));
        assert!(!kube.expanded.contains("kube:cluster"));
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn context_namespace_presentation_changes_without_changing_parent_identity() {
        let (host, mut kube) = setup();
        let first = rows(kube.render(TREE, None, &host.invocation()).unwrap()).remove(0);
        kube.context = Some("other-context".into());
        kube.namespace = Some(ALL_NAMESPACES.into());
        let next = rows(kube.render(TREE, None, &host.invocation()).unwrap()).remove(0);
        assert_eq!(first.key, next.key);
        assert!(next.label.contains("other-context"));
        assert!(next.detail.as_deref().unwrap().contains("All namespaces"));
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn root_is_not_a_resource_and_rbac_errors_remain_under_their_kind() {
        let (host, mut kube) = setup();
        kube.children.insert("kind:pods".into(), Err("Forbidden: synthetic RBAC".into()));
        let items = rows(kube.render(TREE, None, &host.invocation()).unwrap());
        let error = items.iter().find(|r| r.key == "kind:pods!").unwrap();
        assert_eq!(error.parent.as_deref(), Some("kind:pods"));
        assert!(error.label.contains("Forbidden"));
        kube.handle(TREE, ViewEvent::Select(kube.test_node_key("kube:cluster",&host.invocation())), &host.invocation()).unwrap();
        assert!(kube.selected.is_none());
        assert!(resource("kube:cluster").is_none());
        assert!(host.process_runs().is_empty());
    }
}

#[cfg(test)]
mod rich_panel_owner_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn setup() -> (MockHost, KubeExplorer) {
        let host=MockHost::install().with_capabilities(&["process:kubectl","project.read","storage"]);
        host.set_setting("allowRevealSecrets","true");
        host.on_process(KUBECTL,|_,_|Ok(openidea_plugin::ProcessOutput {status:Some(0),stdout:r#"{"apiVersion":"v1","kind":"Secret","metadata":{"name":"demo","namespace":"shop","resourceVersion":"1"},"data":{"password":"c3ludGhldGljLXByaXZhdGU="}}"#.into(),stderr:String::new()}));
        let mut kube=KubeExplorer {context:Some("dev".into()),namespace:Some("shop".into()),current_context:Some("dev".into()),..Default::default()};
        kube.new_rich_callback();(host,kube)
    }
    #[test]
    fn reveal_is_owned_and_automatic_foreign_render_contains_no_plaintext() {
        let (host,mut kube)=setup();
        kube.handle(TREE,ViewEvent::Select(kube.test_resource_key("res:secrets/demo", &host.invocation())),&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("show-secret")),&host.invocation()).unwrap();
        assert_eq!(kube.rich_owner.as_deref(),Some("/project"));
        assert!(format!("{:?}",kube.render(PANEL,None,&host.invocation()).unwrap().body).contains("synthetic-private"));
        let before=host.view_messages().len();host.set_project_root(Some("/foreign"));
        let request=serde_json::json!({"channel":"ui-kit","frameId":"0123456789abcdef0123456789abcdef","sequence":1,"project":"/foreign","op":"render"});
        kube.handle(PANEL,ViewEvent::Message(request.to_string()),&host.invocation()).unwrap();
        assert!(kube.revealed.is_none());
        assert!(host.view_messages()[before..].iter().all(|(_,text)|!text.contains("synthetic-private")));
    }
    #[test]
    fn secret_draft_cannot_submit_from_a_different_project() {
        let (host,mut kube)=setup();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("create-secret")),&host.invocation()).unwrap();
        kube.handle(PANEL,ViewEvent::Field(kube.test_control_key("secret-value"),"synthetic-private".into()),&host.invocation()).unwrap();
        let before=host.process_runs().len();host.set_project_root(Some("/foreign"));
        assert!(matches!(kube.handle(PANEL,ViewEvent::Button("submit-secret".into()),&host.invocation()),Err(Error::Stale)));
        assert!(kube.draft.is_none());assert_eq!(host.process_runs().len(),before);
    }
    #[test]
    fn edit_cache_and_yaml_details_clear_before_stale_write_messages() {
        let (host,mut kube)=setup();
        kube.handle(TREE,ViewEvent::Select(kube.test_resource_key("res:secrets/demo", &host.invocation())),&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("edit-secret")),&host.invocation()).unwrap();
        assert!(kube.editing_secret.is_some());
        let before=host.process_runs().len();host.set_project_root(Some("/foreign"));
        assert!(kube.handle(PANEL,ViewEvent::Button("submit-secret-edit".into()),&host.invocation()).is_err());
        assert!(kube.editing_secret.is_none());assert_eq!(host.process_runs().len(),before);
        kube.expire_rich_context(Some(&host.invocation()));
        kube.show_yaml("pods","web","synthetic-private".into(),None).unwrap();
        host.set_project_root(Some("/another"));
        assert!(matches!(kube.handle(PANEL,ViewEvent::Message("apply\nkind: Pod".into()),&host.invocation()),Err(Error::Stale)));
        assert!(kube.details.is_none());assert_eq!(host.process_runs().len(),before);
    }
    #[test]
    fn absent_or_revoked_owner_refuses_plaintext_and_clears_existing_cache() {
        let (host,mut kube)=setup();host.set_project_root(None);
        kube.handle(TREE,ViewEvent::Select(kube.test_resource_key("res:secrets/demo", &host.invocation())),&host.invocation()).unwrap();
        assert!(matches!(kube.handle(TREE,ViewEvent::Button(kube.test_control_key("show-secret")),&host.invocation()),Err(Error::Invalid(m)) if m.starts_with("Resource details needs the project.read permission. Allow it in Settings › Plugins › Kube Explorer.")));
        assert!(host.process_runs().is_empty());
        assert!(matches!(kube.handle(TREE,ViewEvent::Activate(kube.test_resource_key("res:pods/web", &host.invocation())),&host.invocation()),Err(Error::Invalid(m)) if m.contains("project.read")));
        assert!(host.process_runs().is_empty());host.set_project_root(Some("/project"));
        kube.render(TREE,None,&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Select(kube.test_resource_key("res:secrets/demo",&host.invocation())),&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("show-secret")),&host.invocation()).unwrap();
        let host=host.with_capabilities(&["process:kubectl","storage"]);
        let before=host.process_runs().len();
        let rendered=kube.render(PANEL,None,&host.invocation()).unwrap();
        assert!(matches!(&rendered.body,Body::Message(message) if message.contains("project.read")));
        assert!(rendered.toolbar.is_empty());
        assert!(kube.revealed.is_none());
        assert_eq!(host.process_runs().len(),before);
    }
    #[test]
    fn kubeconfig_choice_change_drops_sensitive_details_without_discovery() {
        let (host,mut kube)=setup();
        kube.handle(TREE,ViewEvent::Select(kube.test_resource_key("res:secrets/demo", &host.invocation())),&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("show-secret")),&host.invocation()).unwrap();
        let before=host.process_runs().len();
        host.set_setting("kubeconfig","\"/other/config\"");
        let panel=kube.render(PANEL,None,&host.invocation()).unwrap();
        assert!(kube.revealed.is_none());assert!(!format!("{:?}",panel.body).contains("synthetic-private"));
        assert_eq!(host.process_runs().len(),before);
    }

    #[test]
    fn namespace_change_invalidates_sensitive_cache_before_panel_refresh_reply() {
        let (host,mut kube)=setup();
        kube.handle(TREE,ViewEvent::Button(kube.test_control_key("create-secret")),&host.invocation()).unwrap();
        kube.handle(PANEL,ViewEvent::Field(kube.test_control_key("secret-value"),"synthetic-private".into()),&host.invocation()).unwrap();
        kube.handle(TREE,ViewEvent::Activate(kube.test_choice_key("ns/other",&host.invocation())),&host.invocation()).unwrap();
        let panel=kube.render(PANEL,None,&host.invocation()).unwrap();
        assert!(kube.draft.is_none());assert!(!format!("{:?}",panel.body).contains("synthetic-private"));
    }
}

#[cfg(test)]
mod marker_icon_tests {
    #[test]
    fn marker_icons_are_declared_and_packaged() {
        let manifest = include_str!("../plugin.toml");
        for icon in [crate::KUSTOMIZE_MARKER, crate::LABEL_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")), "{icon}");
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file(), "{icon}");
        }
    }

    #[test]
    fn indexer_is_warm_only_and_declares_no_users_marker() {
        let manifest = include_str!("../plugin.toml");
        assert!(manifest.contains("[[contributes.documentIndexers]]"));
        assert!(!manifest.contains("kube-users:"));
        assert!(!manifest.contains("marker-users"));
    }
}
