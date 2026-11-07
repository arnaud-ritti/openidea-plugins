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
- **New file templates** (File › New): Pod, Deployment, StatefulSet, DaemonSet, Job,
  CronJob, Service, Ingress, ConfigMap, Secret (`stringData` placeholder), Namespace,
  Role, RoleBinding, ClusterRole, ClusterRoleBinding and CertificateSigningRequest, on current stable
  apiVersions.
- **YAML** opens in the panel, which applies your edits. With `yamlInEditor` on (off by default; never for secrets) it opens in a regular editor on `/tmp/kube-explorer-<kind>-<name>-<random>.yaml`,
  with a `yaml-language-server` schema modeline (completion, validation and
  docs from the Kubernetes JSON schemas). Every save runs `kubectl replace -f`
  in the context and namespace the file was opened from. The file leaves out
  `metadata.resourceVersion`, so a save overwrites any change made in the
  cluster since. `tee` writes the file (`process:tee`, optional) because the
  plugin API can't write files and `tee` creates it readable by every local user of the
  machine (hence the option). Without `tee`, `events` or `project.read`, or on
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
  literal can confuse it). Each has a switch in the settings.
- **Commands** on the open file: Apply, Validate (server dry run), Diff,
  Delete Manifest Resources (lists them with a server dry run, then waits for
  Confirm Delete), Render / Apply Kustomization (the file's directory), Preview
  Helm Template, Lint Helm Chart and Update Helm Dependencies (the chart
  around the file; optional `process:helm`). Update Helm Repositories runs
  `helm repo update` (also optional `process:helm`; if denied, a notification
  says so). How to Install kubectl shows the install instructions in a
  notification; it cannot install anything.

The chosen context and namespace are passed as `--context` / `--namespace`,
so your kubeconfig is never modified. To use another kubeconfig file, set it in
Settings › Plugins › Kube Explorer; otherwise kubectl uses its default.

Not ported: cluster-backed checks (unknown resources or fields against the
live API), CRD-aware validation, Helm values checks and `requirements.yaml` (Helm 2).

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
