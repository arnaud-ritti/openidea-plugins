# Kube Explorer

Browse a Kubernetes cluster from OpenIDEA through `kubectl`.

- **Kube Explorer** tool window (right): double-click a context or namespace
  to switch to it, and expand a kind to list its resources. The toolbar acts on
  the selected resource: Describe (or double-click), YAML, Logs (pods; for
  Deployments, StatefulSets, DaemonSets and Jobs, the logs of all their pods),
  Shell, Attach, Debug (a pod: `kubectl debug -it` adds an ephemeral container on
  the image of the `debugImage` setting, `busybox:stable` by default, targeting the
  pod's first container), Port Forward (edit the `local:remote` ports, then Start) and
  Delete (click twice to confirm). Output replaces the tree until you click Back.
  **Custom Resources** lists the cluster's CRD kinds (`kubectl api-resources`,
  without core and built-in groups) by API group; expanding a kind lists its
  instances, which take Describe, YAML and Delete like any resource.
  **All namespaces**, the first entry of the Namespace list, shows every
  namespaced kind as `namespace/name` rows; an action on a row runs in that
  row's namespace. In that mode Create Secret, Apply Manifest, Apply
  Kustomization and Delete Manifest Resources are refused (pick a namespace
  first), and custom resources that are cluster-scoped or have a name that is not
  a DNS name are listed but take no action. If kubectl cannot be started and no
  context is stored, the tree offers a How to Install kubectl button.
- **Secrets**: Show Secret (a selected secret) decodes its values in the plugin and
  shows them only in the panel. It is off until you turn on `allowRevealSecrets` in
  the settings; while it is off, YAML on a secret is refused too (that YAML holds the
  base64 data), and Describe, which prints no values, still works. With it on, YAML
  on a secret is allowed and is not stripped: it opens in the panel, read-only, and is
  never written to a temp file (`tee` would create it readable by every local user).
  Show Secret itself never sends values to notifications, logs,
  storage, the clipboard, temp files or command arguments; a value that is not UTF-8
  is shown as base64. The SDK has no view-close event, so revealed values clear on Back
  only (and when the setting is turned off). Create Secret builds an Opaque Secret from
  a name and key/value pairs (Add Key for more than one) and runs `kubectl create -f -`
  with the manifest on stdin, so values never appear in arguments and no
  last-applied annotation is written; it fails on an existing name rather than
  overwriting. Keys are limited to `[-._a-zA-Z0-9]`. The form has no masked field in
  this plugin API, so typed values are visible on screen; it is cleared when you
  submit or click Back.
- **Edit Secret** (a selected secret, or the context menu; needs `allowRevealSecrets`):
  a form of the decoded keys. Edit values, add a key, or list keys to remove, then
  Save. The Secret is read as JSON and sent back with `kubectl replace -f -` on stdin
  with only `data` changed, so labels, type and `resourceVersion` stay and a change made
  in the cluster since you opened the form is refused (nothing is saved; open it again).
  Binary values are kept unchanged and not shown. Values are visible on screen, as in
  Create Secret.
- **Log Viewer** (a pod or workload, toolbar or context menu): one `kubectl logs --tail=N
  --timestamps` snapshot (`logViewerLines`, 1000 by default; at most the newest 2 MiB and
  5000 lines shown) in the panel. Filter… (text, or a regular expression with Regex on,
  matches highlighted), Match case, Since… (a UTC date or time, compared with the
  timestamps), Until… (the same for the end of the range), Wrap, Time and Source (show
  or hide the timestamp and the `[pod/container]` prefix of each line), Scroll to end,
  Clear (empties the view; Refresh reads the log again), Refresh, Copy (the shown lines
  on the clipboard, optional `clipboard`) and Download… (writes the shown lines, as
  displayed, to an absolute path you confirm, by default `<project>/<kind>-<name>.log`;
  optional `process:tee`, overwrites the file). Scroll to end relies on the page's
  script, which the IDE may block. Following a log stays in the terminal (Logs, Select
  Log Sources).
- **Completion in YAML**: in an empty document `kpod`, `kdeploy`, `ksts`, `kds`, `kjob`,
  `kcronjob`, `ksvc`, `king`, `kcm`, `ksecret`, `kns`, `krole`, `krb`, `kcr`, `kcrb`
  expand to the manifests of the New-file templates. In a chart's template, inside
  `{{ }}`: `.Values.` completes the keys of the chart's `values.yaml`, `include "` the
  names `define`d in the file and in `templates/_helpers.tpl`, and `incl` an include.
- **Chart.yaml quick fixes** (⌥⏎ in a v2 `Chart.yaml`): *Add chart dependency* writes a
  stanza with `NAME`, `VERSION` and `REPOSITORY` to replace. On a `repository:` line,
  *Use repository alias* and *Use repository URL* swap between a URL and `@alias`; they
  need the list that **Read Helm Repositories for Chart.yaml Quick Fixes** (`helm repo
  list`) stored, because a quick fix cannot run helm or ask you anything.
- **Several kubeconfigs**: the `kubeconfig` setting takes files separated by commas;
  **Choose Kubeconfig** picks the one in use (one file, not a merge of them) and resets
  the context and namespace. `nonKubernetesGlobs` (e.g. `*.compose.yaml, ci/*.yml`)
  lists YAML files that get no problems, `notifySuccess` turns off the notifications of
  successful commands (errors always show). **Telepresence Connect / Quit** run
  `telepresence connect` and `telepresence quit` in a terminal (optional
  `process:telepresence`).
- **Gutter markers** (indexer): a reference to a ConfigMap, Secret, Service, claim or
  service account has a marker that goes to its definition, and a definition has one
  that goes to the manifests using it. The indexer reads every YAML file of the project
  (up to 4096 anchors per file), and names are matched by kind and name, not by
  namespace, so equal names in two namespaces can pick the wrong target. The markers
  need the optional `project.read`: without it, or in an untrusted project, there are none.
- **New file templates** (File › New): Pod, Deployment, StatefulSet, DaemonSet, Job,
  CronJob, Service, Ingress, ConfigMap, Secret (`stringData` placeholder), Namespace,
  Role, RoleBinding, ClusterRole, ClusterRoleBinding and CertificateSigningRequest, on current stable
  apiVersions.
- **YAML** (not for secrets, which open in the panel) opens in a regular editor on `/tmp/kube-explorer-<kind>-<name>-<random>.yaml`,
  with a `yaml-language-server` schema modeline (completion, validation and
  docs from the Kubernetes JSON schemas). Every save runs `kubectl replace -f`
  in the context and namespace the file was opened from. The file leaves out
  `metadata.resourceVersion`, so a save overwrites any change made in the
  cluster since. `tee` writes the file (`process:tee`, optional) because the
  plugin API can't write files. Without `tee`, `events` or `project.read`, or on
  Windows, you edit the YAML in the panel instead.
- **⌥⏎ on a manifest**: *Add Kubernetes schema* (one-document files of a
  built-in kind); *Convert List to separate documents* (caret inside a
  `kind: List` with `items:`) and *Convert documents to a single List* (two or
  more manifest documents). Both edit only the open file and copy the item lines
  as written, so comments stay; neither is offered when the file has anchors,
  aliases or Helm actions.
- **Manifest problems** on open and save (optional `events` and `project.read`;
  a file is a manifest when a document has both `apiVersion:` and `kind:`; files
  with Helm `{{ }}` actions only get the Helm checks): `deprecated-api-version`
  (versions removed in v1.16 through v1.27, with the replacement),
  `duplicate-env-name`, `missing-name`, `missing-selector` (apps/v1 workloads),
  `chart-missing-key` (`Chart.yaml` without `apiVersion`, `name` or `version`),
  `chart-unknown-key` and `chart-invalid-value` (a key outside the Chart.yaml
  schema; a bad `apiVersion`, `type`, `deprecated` or `version`; one switch,
  `checkChartSchema`), `helm-whitespace-trim` (`{{-foo` and `foo-}}` in a chart's
  `templates/`; a trim marker needs a space beside the dash) and
  `helm-excessive-trim` (`{{-` or `-}}` on an output action that joins a line
  to the one before or after it; it works on the text, so a `}}` inside a string
  literal can confuse it). `unknown-key` (a top-level key a built-in kind does not have,
  and, as an Info because a newer Kubernetes may have added it, a `spec` key outside the
  bundled table), `unknown-kind` (a kind its core, apps, batch, networking.k8s.io or rbac
  group does not have; custom resources, other groups, `*List`, anchors and Helm files
  are never judged), `read-only-field` (`status`, `metadata.uid`, `resourceVersion`, ...;
  Info; also `ComponentStatus`, `Binding` and `Eviction`, which are not applied) and
  `deprecated-key` (`spec.serviceAccount`, `spec.rollbackTo`, `spec.loadBalancerIP`, the
  `ingress.class`, `storage-class`, `storage-provisioner` and pod seccomp annotations,
  `metadata.clusterName`, `Endpoints`, `ComponentStatus`, the `Recycle` reclaim policy).
  `missing-key` (an Info: `spec.containers` of a Pod, `spec.template` of a workload that
  has a selector, `spec.serviceName`, a CronJob's `schedule` or `jobTemplate`,
  `spec.podSelector`, a claim's `accessModes`, `roleRef`, ...; a document without its
  companion keys is taken for a patch and left alone; switch `checkRequiredKeys`) and
  `invalid-value` (an Info: a value outside the known set of `restartPolicy`,
  `dnsPolicy`, Service `type`, the traffic and affinity policies, update strategies,
  `concurrencyPolicy`, `completionMode`, `volumeMode`, and a non-number or non-boolean in
  the numeric and boolean `spec` fields; templated values are skipped; switch
  `checkFieldValues`). The kind tables cover the core, apps, batch, networking and rbac
  kinds plus a few of autoscaling, policy, storage, scheduling, coordination, discovery,
  node, certificates, admissionregistration and apiextensions; the other kinds of those
  groups are only checked for existing. In a chart template `helm-unknown-value` (an Info: a `.Values`
  path whose first missing segment is not in `values.yaml`; quiet for `global`, `tpl`,
  `with`/`range`/`define` bodies, aliases in `values.yaml`, charts with `dependencies`
  or `charts/`, and a missing last key next to `default`, `if`, `required`) and
  `helm-unknown-include` (`include "x"` or `template "x"` with no `define "x"` in the
  chart's `templates/`). Each has a switch in the settings.
- **Commands** on the open file: Apply, Validate (server dry run), Diff,
  Delete Manifest Resources (lists them with a server dry run, then waits for
  Confirm Delete), Reload Manifest from Cluster (replaces each document of the open
  file with `kubectl get -o yaml` of the same kind, name and namespace, keeping leading
  comments; one undo step; needs the optional `editor.write`; a document the cluster
  does not have is left), Render / Apply Kustomization (the file's directory), Preview
  Helm Template, Lint Helm Chart and Update Helm Dependencies (the chart
  around the file; optional `process:helm`). Update Helm Repositories runs
  `helm repo update` (also optional `process:helm`; if denied, a notification
  says so). How to Install kubectl shows the install instructions in a
  notification; it cannot install anything.

The chosen context and namespace are passed as `--context` / `--namespace`,
so your kubeconfig is never modified. To use another kubeconfig file (or several), set it in
Settings › Plugins › Kube Explorer; otherwise kubectl uses its default.

Not ported: cluster-backed checks (unknown resources or fields against the
live API), CRD-aware validation and `requirements.yaml` (Helm 2). The key tables cover
the kinds listed above only.

Requires `process:kubectl`. Exec-based auth plugins (`gke-gcloud-auth-plugin`,
`aws`, …) must be on the PATH the IDE was started with.

Icons: the Kubernetes logo and Icons Set by The Kubernetes Authors
(Apache-2.0, see `NOTICE`). Tree items can only name icons the IDE knows, so
each is declared as a `fileIcons` entry on a pattern no file has, and the tree
uses its `ext:fileIcon/aritti.kube-explorer/<id>` name.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
