# Kube Explorer

Browse and operate a Kubernetes cluster from OpenIDEA through `kubectl`. Switch context and namespace, inspect and act on resources, apply or validate manifests, and render Kustomize and Helm output. It also checks, completes and links Kubernetes, Kustomize and Helm files as you edit them. MIT; see `LICENSE`.

Your kubeconfig is never modified: the chosen context and namespace are passed to `kubectl` as `--context` / `--namespace`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- `kubectl` on the `PATH` the IDE sees, and a kubeconfig, for everything that talks to a cluster.
- Optional: `helm` for the Helm commands, `telepresence` for Telepresence.
- Exec-based auth plugins your kubeconfig uses (`gke-gcloud-auth-plugin`, `aws`, …) must also be on the IDE's `PATH`.
- Editor checks, completion and navigation work without a cluster or `kubectl`.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Allow `process:kubectl` in Settings › Plugins › *Kube Explorer*. Also allow `project.read` and `events` for editor checks.
3. Open the Services tool window and expand **Kube Explorer**.
4. Double-click a context, then a namespace, to select them.
5. Expand a kind (for example Pods), select a resource and click **Describe**, **Follow Logs (Terminal)** or **Shell**.

To use a kubeconfig other than kubectl's default, set **Kubeconfig file** in the settings.

## Features

### Browse the cluster in Services

- Double-click a context or namespace to switch to it. Right-click › **Switch Context or Namespace** does the same.
- The status bar shows `⎈ context / namespace`: where every `kubectl` call goes. Click it for **Switch Context…**, **Switch Namespace…**, **Show Cluster Tree**, **Start Resource Watch…**, **Apply Copied YAML…** and **Choose Kubeconfig**.
- **Kube Explorer: Switch Context or Namespace** from Search Everywhere lists the contexts and namespaces to pick from.
- Expand a kind to list its resources. The tree is grouped under **Kubernetes — selected context**, with the selected namespace beside it.
- **Custom Resources** lists the cluster's CRD kinds by API group. Expand a kind to see its instances; they take **Describe**, **YAML** and **Delete**.
- **All namespaces**, at the top of the namespace list, shows every namespaced kind as `namespace/name` rows. Actions on a row run in that row's namespace.
- **Kube Explorer: Customize Namespaces** lets you choose which namespaces the list shows (none picked: all).
- Right-click a group to **Expand Group**, **Collapse Group** or **Refresh Group**.

### Act on a resource

Select a resource and use the toolbar, or right-click it.

- **Describe** (or double-click), **YAML**, **Delete** (asks first, naming the kind, name, namespace and context).
- Pods: **Shell**, **Attach** and **Debug**. Debug adds an ephemeral container (`kubectl debug -it`) targeting the pod's first container. It uses the **debugImage** setting (default `busybox:stable`) and remembers recent images.
- Running **Shell** or **Attach** again for the same pod focuses its open terminal.
- Pods and workloads (Deployments, StatefulSets, DaemonSets, Jobs): **Follow Logs (Terminal)**. For a workload you get the logs of all its pods. **Follow Logs from Selected Sources…** picks pods or containers.
- Pods, workloads and Services: **Port Forward**.
- Less common pod and log actions are in the **More Resource Actions** picker.

The **Kube Explorer Details** panel on the right shows tabs for the selected resource:

- **Resource**, **Describe**, **YAML** and **Events**. They load only when you open them.
- **Logs** streams live, with **Timestamps**. Pods add **All Containers** and **Previous Container**. Workloads combine all their pods and containers, with a source prefix on each line.
- Secrets have no YAML tab.

### Read and search a log snapshot

**Log Snapshot…** (toolbar, or right-click › **Logs**) opens the **Log Viewer**: one snapshot of `kubectl logs` for a pod or workload.

- **Filter…** by text, or by regular expression with **Regex** on. Matches are highlighted. **Match case** is available.
- **Since…** and **Until…** take a UTC date or time: `YYYY-MM-DD` or `YYYY-MM-DDTHH[:MM[:SS[.fraction]]]`, optionally ending in `Z`.
- **Wrap**, **Time** and **Source** show or hide line wrapping, timestamps and the `[pod/container]` prefix.
- **Scroll to end**, **Clear** and **Refresh**.
- **Copy** puts the shown lines on the clipboard (needs `clipboard`).
- **Download…** saves the shown lines to a new file in the project, by default `<project>/<kind>-<name>.log` (needs `project.write`). Log files may contain sensitive data.

The snapshot size is set by **logViewerLines** (default 1000 lines per pod). To follow a log live, use **Follow Logs (Terminal)**.

### Work with Secrets

Secret values stay hidden until you turn on **allowRevealSecrets** in the settings.

- **Show Secret…** decodes the values and shows them only in the panel. Values clear when you click **Back** or turn the setting off.
- **Create Secret…** (right-click the Secrets group) builds an Opaque Secret from a name and key/value pairs. Click **Add Key** for more. It fails if the name exists. Pick a single namespace first.
- **Edit Secret…** shows the decoded keys in a form. Edit values, add keys, or list keys to remove, then **Save**, which asks first and names the Secret, namespace and context. If someone changed the Secret since you opened it, nothing is saved: click **Back** and open it again.
- With the setting off, **YAML** on a Secret is refused; **Describe** still works.

The form fields aren't masked, so typed values are visible on screen.

### Edit resource YAML

- **YAML** (or **Edit YAML…**) opens the resource in the editable panel. Click **Apply** there to send your changes; it asks first and names the kind, name, namespace and context it replaces.
- With **yamlInEditor** on, supported built-in kinds open in a private editor copy with schema completion instead. Secrets and other kinds stay in the panel.

In a private editor copy, **Save** is local only. To send changes to the cluster:

1. Run **Kube Explorer: Review Private YAML Apply…** and check the target and summary.
2. Within 60 seconds, run **Kube Explorer: Apply Private YAML** in the same editor.
3. To apply again, reopen the resource from Services.

Run **Kube Explorer: End Private YAML Editing Session** when you're done with a copy.

### Forward ports

1. Choose **Port Forward…** on a pod, workload or Service.
2. Edit the mappings: `8080:80`, `443`, or `:80` for a free local port. Up to 16.
3. Click **Start in Terminal**, or **Start Managed Session**.

- **Start in Terminal** runs in a terminal. Stop it there. This is the best choice for long sessions.
- **Start Managed Session** shows one confirmation with the project and the exact `kubectl` command. Confirm and the forward starts.
- **Port Forward Status** shows the managed session's output. Use **Stop** or **Restart** there.
- A managed session ends when the plugin goes idle (about five minutes), the window closes, or the plugin reloads.

Both modes bind to `127.0.0.1` only.

### Watch resources

1. Right-click the cluster root › **Watch** › **Watch Resources…**, or run **Kube Explorer: Start Resource Watch…**.
2. Pick Pods, Services or Deployments in the selected namespace.
3. Check the command and confirm. The watch starts in the **Kubernetes Resource Watch** pane.

The pane lists resources as changes arrive. Use **Refresh Watch Status**, **Stop Watch** or **Restart…**. Reopen it with **Kube Explorer: Show Resource Watch**. The pane shows the names seen so far; the stream has no reliable end marker, so it never proves that a resource is absent.

### Run commands on the open file

Right-click in a YAML editor, or use Search Everywhere:

- **Kube Explorer: Apply Manifest to Cluster** asks first and names the target context and namespace. Choose **Apply**, or **Show Diff with Cluster First** to review the changes, then confirm or cancel the apply. When it succeeds, the notification offers **Show in Kube Explorer**.
- **Kube Explorer: Diff Manifest with Cluster** opens the IDE diff viewer: the cluster's objects on the left, what the cluster would store after applying the open file on the right (a server dry run, which changes nothing). Both sides drop `status`, `managedFields`, `resourceVersion`, `uid`, `creationTimestamp`, `generation`, `selfLink` and the last-applied annotation, sort their keys and list one document per object by kind, namespace and name, so only real changes remain. Resources not yet in the cluster show only the right side. If the server refuses the dry run (validation, an admission webhook, or no patch permission), the right side is the open file in the same layout and the notification says why. Manifests with a Secret or a List show a masked diff in Kube Explorer Details instead, unless **allowRevealSecrets** is on.
- **Kube Explorer: Validate Manifest (Server Dry Run)** checks the manifest against the cluster without changing it.
- **Kube Explorer: Delete Manifest Resources from Cluster…** lists what would be deleted (server dry run) under a title naming the context and namespace. **Confirm Delete** asks once more with the context, namespace and number of resources.
- **Kube Explorer: Reload Manifest from Cluster** replaces each document with the cluster's current version. Leading comments stay; one undo reverts it. Documents the cluster doesn't have are left alone. Needs `editor.write`.
- **Kube Explorer: Render Kustomization** and **Apply Kustomization to Cluster** use the file's folder. Apply asks first, naming the context and namespace.
- **Kube Explorer: Preview Helm Template**, **Lint Helm Chart** and **Update Helm Dependencies** use the chart around the file. **Configure Helm Parameters** sets the release name, values file and `--set` overrides for template and lint.
- **Kube Explorer: Create Helm Chart** scaffolds a new chart.

**Kube Explorer: Apply Copied YAML…** applies a manifest you copied (from a browser, a chat or another file) without saving it first. It names the copied resources and the target context and namespace, and offers the same **Apply** or **Show Diff with Cluster First** choice. Needs `clipboard.read`; the clipboard is read only when you run the command.

In **All namespaces** mode, Apply Manifest, Apply Copied YAML, Apply Kustomization, Delete Manifest Resources and Create Secret are refused. Pick a namespace first.

### Manage Helm repositories

- **Kube Explorer: Update Helm Repositories** runs `helm repo update`.
- **Kube Explorer: Read Helm Repositories for Chart.yaml Quick Fixes** stores `helm repo list` for the `Chart.yaml` quick fixes. Run it once before using them.
- **Kube Explorer: Read Local Helm Dependency Catalog** reads your installed repository indexes for dependency version completion. It doesn't update repositories or use the network.

### Spot problems in manifests

Checks run when you open or save a YAML file (needs `events` and `project.read`). A file counts as a manifest when a document has both `apiVersion:` and `kind:`. Files with Helm `{{ }}` actions get only the Helm checks. Each check has a setting.

- **Deprecated API versions** (`deprecated-api-version`): versions removed in Kubernetes 1.16 to 1.27, with the replacement.
- **Duplicate env names** (`duplicate-env-name`), **missing name** (`missing-name`), **missing selector** (`missing-selector`, apps/v1 workloads).
- **Unknown keys and kinds** (`unknown-key`, `unknown-kind`) for core, apps, batch, networking and rbac kinds.
- **Read-only fields** (`read-only-field`, info): `status`, `metadata.uid`, `resourceVersion` and similar.
- **Deprecated keys** (`deprecated-key`): `spec.serviceAccount`, `spec.rollbackTo`, `spec.loadBalancerIP`, old annotations and more.
- **Missing required keys** (`missing-key`, info): `spec.containers` of a Pod, a CronJob's `schedule`, `roleRef` and more.
- **Invalid values** (`invalid-value`, info): a `restartPolicy`, Service `type`, strategy and so on outside the allowed set, or a non-number / non-boolean where one is needed.
- **Chart.yaml**: missing `apiVersion`, `name` or `version` (`chart-missing-key`), unknown keys (`chart-unknown-key`) and bad values (`chart-invalid-value`).
- **Helm templates**: trim markers without a space (`helm-whitespace-trim`), trim markers that join lines (`helm-excessive-trim`), `.Values` paths missing from `values.yaml` (`helm-unknown-value`, info) and `include` names with no `define` (`helm-unknown-include`).
- **Local CRDs**: resources of a CRD defined in the project are checked against its schema.
- **JSON** manifests get key checks and duplicate-key warnings.

To silence checks for YAML files that aren't Kubernetes (Compose files, CI files), list them in **nonKubernetesGlobs**.

### Fix and refactor with ⌥⏎

- **Add Kubernetes schema** to a one-document file of a built-in kind.
- **Convert List to separate documents**, and **Convert documents to a single List**. Comments and line endings are kept.
- In a v2 `Chart.yaml`: **Add chart dependency**, and on a `repository:` line **Use repository alias** or **Use repository URL**.
- **Use local Helm values schema in YAML language server** adds a schema directive for an installed YAML language server.
- **Use local CRD schema in YAML language server…** creates a schema file next to the manifest and adds the directive. It shows a preview; Undo reverts it.

### Complete and create manifests

- **New** file templates: Pod, Deployment, StatefulSet, DaemonSet, Job, CronJob, Service, Ingress, ConfigMap, Secret, Namespace, Role, RoleBinding, ClusterRole, ClusterRoleBinding and CertificateSigningRequest (all named **Kubernetes …**).
- In an empty YAML document, type `kpod`, `kdeploy`, `ksts`, `kds`, `kjob`, `kcronjob`, `ksvc`, `king`, `kcm`, `ksecret`, `kns`, `krole`, `krb`, `kcr` or `kcrb` to expand the matching template.
- In a Helm template, inside `{{ }}`: `.Values.` completes keys from `values.yaml`; `include "` completes defined names; `incl` expands an include.
- In `kustomization.yaml`, `kustomization.yml` and `Kustomization`: top-level keys.
- Resources of a local CRD: property, enum and boolean values.
- `values.yaml` keys from a local `values.schema.json`, and chart dependency fields.
- Cluster names: run **Kube Explorer: Read Cluster Namespaces for Completion** or **Kube Explorer: Load Resource Names from Cluster for Completion…**. For 60 seconds, `metadata.namespace` and reference fields (Secret, ConfigMap, Service, PersistentVolumeClaim, ServiceAccount names) complete from the cluster.

### Navigate between manifests

- **Go to Declaration** and gutter markers link Pods and workloads to the ConfigMaps, Secrets, Services, claims and service accounts they use, and back.
- Ingress backends and TLS Secrets, `metadata.namespace` (to the Namespace) and HPA scale targets are linked too.
- Label selectors link to the resources they match, including affinity and topology spread terms. When a selector line matches several resources, its gutter icon opens a chooser (up to 200 targets). Rename on a label key updates matching keys.
- Helm `include` / `template` names jump to their `define`. `.Values.a.b` jumps to the key in `values.yaml`.
- Kustomize patches, replacements, generators, `helmCharts` `valuesFile` and resource/component paths jump to their targets. Paths complete too.
- **Kube Explorer: Find Local Label or Data-Key Occurrences** (editor right-click) lists where a label or ConfigMap/Secret key is used.

### Use several kubeconfigs

- Put several files in **Kubeconfig file**, separated by commas.
- **Kube Explorer: Choose Kubeconfig** (or right-click the root › **Kubeconfig** › **Choose Kubeconfig…**) picks the one in use. It resets the context and namespace.

### Import a kubeconfig privately

**Kube Explorer: Import Pasted Kubeconfig…** (Tools menu) stores a pasted kubeconfig context inside the plugin.

1. Paste the YAML or JSON.
2. Enter a label and the context to keep.
3. Check the redacted review and confirm.

**Kube Explorer: Show Imported Kubeconfigs** shows what's stored, without tokens or keys.

**Kube Explorer: Use Imported Kubeconfig** selects one profile: every `kubectl` call then gets `--kubeconfig` for it, and the Services tree names it. Choose it again to stop using it. **Kube Explorer: Delete Imported Kubeconfig** removes a profile and, if it was in use, the selection.

Imported profiles are never merged into your files or listed in Choose Kubeconfig. Import, show, use and delete need no project, no `kubectl` and no network.

### Telepresence

**Kube Explorer: Telepresence Connect** and **Telepresence Quit** run `telepresence connect` / `telepresence quit` in a terminal.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Kube Explorer**; bind keys in Settings › Keymap. *Tools* means the Tools menu, *Editor* the editor right-click menu (YAML files).

### Open file and project

| Command | What it does | Shortcut |
|---|---|---|
| **Kube Explorer: Apply Manifest to Cluster** (*Editor*) | Applies the open manifest, after asking (or shows the diff first). | None |
| **Kube Explorer: Apply Copied YAML…** | Applies the manifest on the clipboard, after naming its resources and the target (or shows the diff first). | None |
| **Kube Explorer: Validate Manifest (Server Dry Run)** | Validates it with a server dry run. | None |
| **Kube Explorer: Diff Manifest with Cluster** (*Editor*) | Opens the diff with the cluster's objects. | None |
| **Kube Explorer: Delete Manifest Resources from Cluster…** | Lists the resources, then deletes after **Confirm Delete** and a confirmation. | None |
| **Kube Explorer: Reload Manifest from Cluster** (*Editor*) | Replaces the documents with the cluster's versions. | None |
| **Kube Explorer: Render Kustomization** | Renders the file's folder with Kustomize. | None |
| **Kube Explorer: Apply Kustomization to Cluster** | Applies the file's folder with Kustomize, after asking. | None |
| **Kube Explorer: Preview Helm Template** | Runs `helm template` on the chart around the file. | None |
| **Kube Explorer: Lint Helm Chart** | Runs `helm lint`. | None |
| **Kube Explorer: Update Helm Dependencies** | Runs `helm dependency update`. | None |
| **Kube Explorer: Configure Helm Parameters** | Sets release name, values file and `--set` for template and lint. | None |
| **Kube Explorer: Create Helm Chart** | Runs `helm create` for a new chart. | None |
| **Kube Explorer: Update Helm Repositories** | Runs `helm repo update`. | None |
| **Kube Explorer: Read Helm Repositories for Chart.yaml Quick Fixes** | Stores `helm repo list` for the Chart.yaml quick fixes. | None |
| **Kube Explorer: Read Local Helm Dependency Catalog** (*Tools*) | Reads local repository indexes for dependency completion. | None |
| **Kube Explorer: Find Local Label or Data-Key Occurrences** (*Editor*) | Lists local uses of the label or key under the caret. | None |
| **Kube Explorer: Read Cluster Namespaces for Completion** | Caches namespace names for completion (60 seconds). | None |
| **Kube Explorer: Load Resource Names from Cluster for Completion…** | Caches names of one kind in the selected namespace (60 seconds). | None |
| **Kube Explorer: Review Private YAML Apply…** | Reviews a private YAML copy before applying. | None |
| **Kube Explorer: Apply Private YAML** | Applies the reviewed copy (within 60 seconds). | None |
| **Kube Explorer: End Private YAML Editing Session** | Ends apply rights for a private copy. | None |

### Cluster, kubeconfig and tools

| Command | What it does | Shortcut |
|---|---|---|
| **Kube Explorer: Show Cluster Tree** | Reveals the cluster tree (also in the status bar item's menu). | None |
| **Kube Explorer: Switch Context or Namespace** | Picks a context or namespace (from a row: switches to it). | None |
| **Kube Explorer: Choose Kubeconfig** | Picks which configured kubeconfig file to use. | None |
| **Kube Explorer: Customize Namespaces** | Chooses which namespaces the tree lists. | None |
| **Kube Explorer: Edit Recent Debug Images** | Edits the recent images offered by **Debug Pod…**. | None |
| **Kube Explorer: Start Resource Watch…** | Starts a watch of Pods, Services or Deployments, after a confirmation. | None |
| **Kube Explorer: Show Resource Watch** | Opens the watch pane. | None |
| **Kube Explorer: Import Pasted Kubeconfig…** (*Tools*) | Stores a pasted kubeconfig context privately. | None |
| **Kube Explorer: Show Imported Kubeconfigs** (*Tools*) | Shows imported profiles without secrets. | None |
| **Kube Explorer: Use Imported Kubeconfig** (*Tools*) | Uses an imported profile for every `kubectl` call, or stops using it. | None |
| **Kube Explorer: Delete Imported Kubeconfig** (*Tools*) | Deletes an imported profile. | None |
| **Kube Explorer: How to Install kubectl** | Shows install instructions. | None |
| **Kube Explorer: Telepresence Connect** | Runs `telepresence connect` in a terminal. | None |
| **Kube Explorer: Telepresence Quit** | Runs `telepresence quit` in a terminal. | None |

### Services right-click menu

These act on the selected row in Services › Kube Explorer.

| Menu item | Where | What it does | Shortcut |
|---|---|---|---|
| **Describe Resource** | Resources | `kubectl describe`. | None |
| **Edit YAML…** | Resources | Opens the YAML for editing. | None |
| **Delete Resource…** | Resources | Deletes after a confirmation naming the namespace and context. | None |
| **Follow Logs (Terminal)** | Pods, workloads › **Logs** | Streams logs in a terminal. | None |
| **Log Snapshot…** | Pods, workloads › **Logs** | Opens the Log Viewer. | None |
| **Follow Logs from Selected Sources…** | Pods, workloads › **Logs** | Picks pods or containers to log. | None |
| **Open Shell** | Pods | Opens a shell in the pod, or focuses the open one. | None |
| **Attach to Pod** | Pods › **Container** | Attaches to the container, or focuses the open terminal. | None |
| **Debug Pod…** | Pods › **Container** | Starts an ephemeral debug container. | None |
| **Port Forward…** | Pods, workloads, Services | Opens the port-forward form. | None |
| **Show Secret…** | Secrets | Shows decoded values. | None |
| **Edit Secret…** | Secrets | Edits decoded values. | None |
| **Create Secret…** | Secrets group | Creates an Opaque Secret. | None |
| **Switch Context or Namespace** | Contexts, namespaces | Selects it. | None |
| **Expand Group** / **Collapse Group** | Groups, root | Expands or collapses the row. | None |
| **Refresh Group** | Groups, root | Reloads the row. | None |
| **Watch Resources…** | Root › **Watch** | Starts a resource watch, after a confirmation. | None |
| **Open Resource Watch** | Root › **Watch** | Opens the watch pane. | None |
| **Choose Kubeconfig…** | Root › **Kubeconfig** | Picks the kubeconfig file. | None |
| **Import Kubeconfig Privately…** | Root › **Kubeconfig** | Same as **Kube Explorer: Import Pasted Kubeconfig…**. | None |
| **Review Imported Kubeconfigs…** | Root › **Kubeconfig** | Same as **Kube Explorer: Show Imported Kubeconfigs**. | None |

## Settings

Settings › Plugins › Kube Explorer.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Kubeconfig file, or several separated by commas (Choose Kubeconfig picks one; empty: KUBECONFIG or ~/.kube/config) | `kubeconfig` | empty | Which kubeconfig files to use. |
| Image of the ephemeral debug container (Debug button on a pod) | `debugImage` | `busybox:stable` | Image for **Debug Pod…**. |
| Allow Show Secret and YAML on a secret | `allowRevealSecrets` | off | Show, Edit and YAML on Secrets. |
| Open built-in YAML in a private editor copy | `yamlInEditor` | off | Panel or private editor for YAML. |
| Lines per pod the Log Viewer reads (10 to 20000) | `logViewerLines` | 1000 | Log snapshot size. |
| Show a notification when a command succeeds (errors are always shown) | `notifySuccess` | on | Success notifications. |
| YAML files that are not Kubernetes (globs, comma-separated) | `nonKubernetesGlobs` | empty | Files with no manifest checks. |
| Report unknown top-level or spec keys | `checkKindSchema` | on | Unknown keys in built-in kinds (core, apps, batch, networking, rbac) and kinds their API group lacks. |
| Hint at missing required keys | `checkRequiredKeys` | on | Keys the API requires, such as `spec.containers`, `spec.template` or `roleRef`. |
| Hint at invalid field values | `checkFieldValues` | on | Values that are not one the field accepts, such as `restartPolicy` or Service `type`, or not an integer or boolean where one is required. |
| Report removed or deprecated apiVersions (extensions/v1beta1, batch/v1beta1, ...) | `checkDeprecatedApiVersions` | on | Flags `apiVersion` values the API no longer serves or has deprecated. |
| Report an env variable defined twice in one container | `checkDuplicateEnvNames` | on | Flags an environment variable declared twice in one container. |
| Report a built-in resource without metadata.name or generateName | `checkMissingName` | on | Flags a built-in resource with neither `metadata.name` nor `generateName`. |
| Report a Deployment, StatefulSet, DaemonSet or ReplicaSet without spec.selector | `checkMissingSelector` | on | Flags workloads without `spec.selector`. |
| Report Helm whitespace trim markers written without a space ({{-foo, foo-}}) | `checkHelmWhitespaceTrim` | on | Flags `{{-foo` and `foo-}}` trim markers written without a space. |
| Report unknown Chart.yaml keys and invalid apiVersion, type, deprecated or version values | `checkChartSchema` | on | Flags unknown `Chart.yaml` keys and invalid `apiVersion`, `type`, `deprecated` or `version` values. |
| Report a Helm trim marker on an output that joins it to a neighbouring YAML line | `checkHelmExcessiveTrim` | on | Flags a trim marker that joins an output to a neighbouring YAML line. |
| Hint at fields the API server owns (status, metadata.uid, resourceVersion, ...) in a manifest | `checkReadOnlyFields` | on | Hints at fields the API server owns, such as `status`, `metadata.uid` and `resourceVersion`. |
| Report deprecated or removed keys (spec.serviceAccount, spec.rollbackTo, the ingress.class annotation, ...) | `checkDeprecatedKeys` | on | Flags deprecated or removed keys, such as `spec.serviceAccount`. |
| Hint at a .Values path in a chart template that values.yaml does not define | `checkHelmValues` | on | Hints at a `.Values` path that `values.yaml` does not define. |
| Report an include or template of a name that no define in the chart's templates provides | `checkHelmIncludes` | on | Flags an `include` or `template` of a name that no `define` in the chart provides. |
| Report a Chart.yaml without apiVersion, name or version | `checkChartKeys` | on | Flags a `Chart.yaml` without `apiVersion`, `name` or `version`. |

## Permissions

The plugin always uses its own storage (`storage`). Optional permissions are off until you allow them in Settings › Plugins › *Kube Explorer*.

| Permission | Used for | Without it |
|---|---|---|
| `process:kubectl` | Browsing and every cluster operation. | The tree shows a **Kubernetes — permission required** row. Private kubeconfig import still works. |
| `project.read` | Manifest checks, navigation, gutters, the Log Viewer and managed sessions, which belong to the open project. | No checks or links. Cluster browsing still works in Global mode. |
| `events` | Running checks when you open or save a file. | No manifest checks. |
| `editor.read` | Commands that read the open editor, such as the private YAML review. | Those commands can't run. |
| `editor.write` | **Kube Explorer: Reload Manifest from Cluster**. | That command is refused. |
| `project.write` | **Download…** in the Log Viewer. | Logs can't be saved to a file. |
| `process:helm` | Helm template, lint, dependency and repository commands. | Those commands fail with a notification. |
| `process:telepresence` | Telepresence Connect and Quit. | Those commands fail. |
| `clipboard` | **Copy** in the Log Viewer. | Copy is refused. |
| `clipboard.read` | **Kube Explorer: Apply Copied YAML…** reads the copied manifest. | That command explains where to allow it. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| The tree shows **Kubernetes — permission required** | Allow `process:kubectl` in Settings › Plugins › *Kube Explorer*. |
| The tree shows **Context: (no current context)** | Your kubeconfig sets no current context. Click **Choose Kubeconfig…** to pick another file, or set one with `kubectl config use-context`. |
| The tree shows **How to Install kubectl** | `kubectl` can't be started and no context is stored. Install it so it's on the IDE's `PATH`, then refresh. |
| Authentication fails for GKE, EKS or similar | Put the auth plugin (`gke-gcloud-auth-plugin`, `aws`, …) on the IDE's `PATH`. |
| *To show them, turn on "Allow Show Secret and YAML on a secret" in Settings › Plugins › Kube Explorer.* | Turn on **Allow Show Secret and YAML on a secret** in the settings. Off: secret data is never read; on: secret YAML is shown unstripped. |
| *Refusing … in All namespaces…* or Apply/Create Secret refused | Switch from **All namespaces** to a single namespace. |
| *… failed: the login for context … has expired or was rejected* | Sign in again with your cloud CLI or auth plugin, then retry. |
| *… failed: context … cannot be reached* | Check the network or VPN to the cluster. |
| *… failed: your account on context … is not allowed to do this* | Ask for access, or switch context. |
| *… failed on context …. Run kubectl with that context in a terminal…* | Run the same `kubectl` command with `--context` in a terminal to see the full error; Kube Explorer doesn't show raw kubectl output because it can contain credentials. |
| *This resource is no longer listed. Refresh the group.* | Right-click the group and choose **Refresh Group**. |
| *Select one namespace before watching.* | Pick a single namespace, then start the watch. |
| *The secret changed in the cluster since it was opened…* | Click **Back** and open **Edit Secret…** again. Nothing was saved. |
| *Kube Explorer is not allowed to run helm…* | Allow `process:helm`. |
| *The open file is not in a Helm chart…* | Open a file inside a folder with a `Chart.yaml` (or below one). |
| Chart.yaml alias/URL quick fixes are missing | Run **Kube Explorer: Read Helm Repositories for Chart.yaml Quick Fixes** once. |
| *Helm dependency search failed; run Update Helm Repositories…* | Run **Kube Explorer: Update Helm Repositories**, then read the catalog again. |
| *Reloading a manifest needs the editor.write permission. Allow it in Settings › Plugins › Kube Explorer.* | Allow `editor.write`. |
| *Allow the optional clipboard capability to copy the shown lines.* | Allow `clipboard`. |
| *Project access is unavailable…* or *Resource details needs the project.read permission…* | Allow `project.read` and open a project. Kubernetes also works in a window with no open project. |
| *At most 16 private editor leases…* | Run **Kube Explorer: End Private YAML Editing Session** on copies you no longer need. |
| No problems shown in a manifest | Allow `events` and `project.read`. Check the file isn't matched by **nonKubernetesGlobs**. |
| Problems shown in a non-Kubernetes YAML file | Add it to **nonKubernetesGlobs**. |
| Managed port forward stopped | It ends when the plugin goes idle. Use **Start in Terminal** for long sessions. |

## Limitations

- No checks against the live cluster API (unknown resources or fields), and no `requirements.yaml` (Helm 2).
- Built-in key tables cover the kinds listed under [Technical details](#technical-details) only.
- No automatic Rename or Find Usages for resources; no full resource-reference completion, deployment-owner closure, or CRD namespaces in cluster hints.
- Port forwarding doesn't accept named remote ports.
- Managed watches and forwards don't survive the plugin going idle, and don't reconnect.
- Snapshots and spawned processes don't guarantee readiness, live-stream completeness or runtime identity.
- Imported kubeconfig profiles can't be exported. No X.509/key-pair validation and no encrypted OS credential storage.
- CRD checks don't reproduce server admission, CEL validation, runtime defaults or remote references.
- Helm chart semantics beyond the explicit template/lint commands aren't checked.
- **Scroll to end** in the Log Viewer relies on a page script the IDE may block.
- No gutter action to start anything: markers only open locations.

## Technical details

<details>
<summary>Services tree and actions</summary>

The tree is one stable `kube:cluster` parent labelled **Kubernetes — selected context**. Root expand/collapse only changes presentation: it discovers nothing, changes no selection and issues no kubectl call. **Custom Resources** uses `kubectl api-resources`, without core and built-in groups. In **All namespaces** mode, custom resources that are cluster-scoped or whose name isn't a DNS name are listed but take no action. Editing and terminal workflows open the existing Kube Explorer Details panel or terminal; native read-only details stay beside the Services tree. Without the `process:kubectl` grant, a non-resource status row explains the missing grant and the private import workflow; selecting it shows read-only help and activates no cluster. Context menus follow the row kind; the toolbar shows applicable native icon buttons with tooltips. Actions validate the current scoped row key and refuse stale keys, mismatched kinds, changed targets/scope and foreign-view callbacks.

Describe/YAML/Events run only when opened, with bounded request timeouts. Events target the exact built-in kind/name and row namespace; unknown custom kinds don't guess an event kind. Secrets omit the native YAML tab; the explicit Show/Edit/YAML actions keep their reveal gate. Workload logs use a bounded concurrent stream count. The host cancels the active stream when its pane is hidden or its target changes. Watch preparation needs an invoking project and a single namespace.

</details>

<details>
<summary>Secrets</summary>

**Show Secret** decodes values in the plugin and never sends them to notifications, logs, storage, the clipboard, temp files or command arguments. A non-UTF-8 value is shown as base64. With `allowRevealSecrets` on, YAML on a Secret is not stripped: it opens in the panel, read-only, and is never written to a temp file. Revealed values clear on Back and when the setting is turned off (the SDK has no view-close event), which also clears cached Secret YAML and retires its page Apply token.

**Create Secret** runs `kubectl create -f -` with the manifest on stdin, so values never appear in arguments and no last-applied annotation is written. Keys are limited to `[-._a-zA-Z0-9]`. The form has no masked field in this plugin API; it's cleared on submit or Back.

**Edit Secret** reads the Secret as JSON and sends it back with `kubectl replace -f -` on stdin with only `data` changed, so labels, type and `resourceVersion` stay and a change made in the cluster since the form was opened is refused. Binary values are kept unchanged and not shown.

Secret JSON reads are bounded to 2 MiB with strict JSON/object/depth validation before any plaintext or form is published. Data keys must be unique supported Secret keys (up to 4,096); repeated data members refuse. Show/Edit responses must identify a v1 Secret with the selected name and a concrete valid namespace matching the selected namespace when explicit; Edit needs a nonempty `resourceVersion` before building replacement authority. Decoding requires canonical padded standard base64 (CR/LF wrapping allowed): truncated groups, misplaced or excess padding, nonzero trailing bits and other whitespace refuse instead of silently producing changed editable bytes. Raw failed-command diagnostics, including process-level errors, are withheld; safe Conflict/AlreadyExists messages and numeric status remain, and failed forms stay available for retry. Secret YAML failed read/Apply diagnostics are also withheld.

Revealed values larger than the native multiline field's 1 MiB limit are shown as numbered Unicode-safe parts with explicit join-without-separators instructions (a large binary value's base64 can exceed the limit even when decoded bytes fit); no partial field is presented as the whole value.

</details>

<details>
<summary>Log Viewer</summary>

Runs one `kubectl logs --tail=N --timestamps` snapshot; at most the newest 2 MiB and 5,000 lines are shown. **Since…/Until…** accept bounded ASCII prefixes with valid calendar fields: date, hour, minute, second or up to 9 fractional digits, optional trailing `Z`. A date or minute prefix includes that whole precision unit. Timezone offsets and leap seconds are outside this client-side filter. Invalid, Unicode or reversed input is rejected before replacing the last accepted filter, keeping the snapshot and prior display. Timestamp comparison pads an absent fraction with zero; malformed timestamp-like payloads stay ordinary text and aren't stripped by Hide Time.

**Download…** writes the shown lines, as displayed, through `project.write` to a new file inside the owning project. The confirmation shows the exact path, line/byte count and SHA-256 of the displayed snapshot; it's a digest review, not a truncated full-content preview. Outside or nested-project paths, existing files, dirty/open buffers and missing parents are refused. Cancel writes nothing. No `tee` or new cluster request runs for export. New files have normal filesystem permissions.

Each snapshot needs an invoking project with `project.read` and records its project, captured context/resolved namespace, a non-credential local configuration fingerprint and the selected source. A different or unavailable project, changed selection, namespace, context or kubeconfig choice drops it before rendering, Copy, Download or Refresh (deactivation too). Refresh verifies the current local mapping still matches the snapshot and reads with the captured explicit context/namespace; an unconfirmable or changed mapping clears the snapshot, and a failed read keeps output only after matching owner/scope verification, marked as a failed refresh. Rendering performs no log or configuration discovery. Log and export controls are correlated with the exact loaded snapshot and retire on reopen or successful refresh.

</details>

<details>
<summary>Private YAML copies</summary>

With `yamlInEditor`, supported modern built-ins open in a private SDK-storage editor copy with the schema modeline. The review shows the exact target, editor revision, byte count and digest. **Apply Private YAML** is a separate event that rechecks project, local kubeconfig/context/namespace and unchanged current text/version, consumes the approval and sends the reviewed text on stdin with `kubectl replace`. UID and `resourceVersion` are kept and can't be edited, so server conflict checks still apply. A successful apply consumes the lease. Review cancellation, expiry, changed source, missing/foreign project, changed local scope and replay don't replace a resource; a failed apply consumes the approval but keeps the editable copy.

Copies support stable built-in core/apps/batch/networking/rbac identities only (no Secrets, CRDs or guessed legacy schemas). They're bounded to 64 KiB, one alias/tag/duplicate-free document, depth 16 and 4,096 parser nodes; at most 16 live leases and 64 persistent backing files. **End Private YAML Editing Session** removes apply authority and may blank only a generated copy whose current text and backing bytes still match the original digest; edited copies stay. There's no private file-delete API or conditional write/CAS: files remain after retirement, external filesystem races aren't excluded, storage isn't encrypted or automatically erased, and backups/editor histories can keep contents. Closing an editor isn't an observed lease-close event; use End Private YAML Editing Session or reload/deactivate to release authority.

</details>

<details>
<summary>Quick fixes</summary>

List/document conversions edit only the open file and copy item lines as written, so comments and CRLF stay. Neither is offered when the file has anchors, aliases or Helm actions, when parsed values are ambiguous duplicates, when splitting a List with nonempty wrapper metadata, or when whitespace/chomping changes would alter ConfigMap scalar bytes.

*Add chart dependency* writes a stanza with `NAME`, `VERSION` and `REPOSITORY` placeholders. The alias/URL fixes use the list stored by **Read Helm Repositories for Chart.yaml Quick Fixes** (`helm repo list`), because a quick fix can't run helm or ask anything. Aliases whose cached name maps to conflicting URLs offer no conversion; identical rows deduplicate. Chart source edits accept one bounded YAML mapping document without duplicate keys, aliases, anchors, explicit tags or complex keys (8,192 nodes, depth 32), keep trailing comments and CRLF outside the scalar, and refuse unsupported quoted-key or flow-sequence insertion shapes.

*Use local Helm values schema…* keeps existing directives intact. *Use local CRD schema…* binds resource version/text and source schema, refuses existing or open schema destinations, keeps CRLF and percent-encodes filenames; nullable and int-or-string OpenAPI forms become JSON Schema alternatives. Neither fetches schemas or proves remote schema availability; an installed YAML language server supplies its own semantics. Modeline refactors are never offered for JSON documents.

</details>

<details>
<summary>Commands on the open file</summary>

Helm Template revalidates its release argument when consumed, including the fallback chart directory basename and persisted values, so a value can't become a Helm flag; Lint and Dependency Update don't consume a release argument. If `process:helm` is denied, Update Helm Repositories says so in a notification. **Read Local Helm Dependency Catalog** uses `helm repo list` and bounded `helm search repo --versions`. Repository signatures and current chart dependency identity bind a ten-minute cache; version completion is withdrawn after dependency identity changes or expiry. **How to Install kubectl** shows instructions in a notification; it can't install anything.

</details>

<details>
<summary>Manifest checks</summary>

Without `events` and `project.read`, checks are silently off. `helm-whitespace-trim` applies to `{{-foo` and `foo-}}` in a chart's `templates/`: a trim marker needs a space beside the dash. `helm-excessive-trim` flags `{{-` or `-}}` on an output action that joins a line to the one before or after it; it works on text, so a `}}` inside a string literal can confuse it.

`unknown-key` covers top-level keys a built-in kind doesn't have, and (as an Info, because a newer Kubernetes may have added it) a `spec` key outside the bundled table. `unknown-kind` covers a kind its core, apps, batch, networking.k8s.io or rbac group doesn't have; custom resources, other groups, `*List`, anchors and Helm files are never judged. `read-only-field` also covers `ComponentStatus`, `Binding` and `Eviction`, which aren't applied. `deprecated-key` covers `spec.serviceAccount`, `spec.rollbackTo`, `spec.loadBalancerIP`, the `ingress.class`, `storage-class`, `storage-provisioner` and pod seccomp annotations, `metadata.clusterName`, `Endpoints`, `ComponentStatus` and the `Recycle` reclaim policy.

`missing-key` covers `spec.containers` of a Pod, `spec.template` of a workload that has a selector, `spec.serviceName`, a CronJob's `schedule` or `jobTemplate`, `spec.podSelector`, a claim's `accessModes`, `roleRef` and more; a document without its companion keys is taken for a patch and left alone. `invalid-value` covers `restartPolicy`, `dnsPolicy`, Service `type`, the traffic and affinity policies, update strategies, `concurrencyPolicy`, `completionMode`, `volumeMode`, and non-number or non-boolean values in numeric and boolean `spec` fields; templated values are skipped.

`helm-unknown-value` flags a `.Values` path whose first missing segment isn't in `values.yaml`. It's quiet for `global`, `tpl`, `with`/`range`/`define` bodies, aliases in `values.yaml`, charts with `dependencies` or `charts/`, and a missing last key next to `default`, `if` or `required`. `helm-unknown-include` flags `include "x"` or `template "x"` with no `define "x"` in the chart's `templates/`.

The kind tables cover the core, apps, batch, networking and rbac kinds, plus a few of autoscaling, policy, storage, scheduling, coordination, discovery, node, certificates, admissionregistration and apiextensions; other kinds of those groups are only checked for existence.

</details>

<details>
<summary>Completion and navigation</summary>

Templates use current stable apiVersions; the Secret template has a `stringData` placeholder. Helm `.Values.` completion has `with`/`range` scope awareness for `.Values` and `$.Values`; `include "` offers names `define`d in the file and in `templates/_helpers.tpl`. Unresolved includes are reported as Info.

Kustomization root key completion offers documented SIG CLI fields, omits existing keys and replaces the whole partial key, sorted deterministically. Nested mappings, values, comments, flow-style documents and multiple YAML documents stay quiet. This doesn't add full nested schema validation.

Source navigation and gutters are read-only. Ingress `spec.tls[].secretName` links to a core Secret in its source namespace; valid namespaced-resource `metadata.namespace` links to a core Namespace. Arbitrary `data.namespace`, custom `secretName` fields, field-name matches in ConfigMap/custom-resource data and cluster-resource namespace fields don't get links. Marker tooltips label read-only source candidates and the unresolved deployment/cluster identity.

Kustomization patch/replacement selectors (kind, name, namespace, group, version, plain labelSelector/annotationSelector) link to resources listed in the same kustomization; replacement `fieldPath`(s) link to literal keys; selectors matching none of the fully local parsable resources show an Info. Local resources/bases/components directories report a missing kustomization or a bounded-depth cycle. configMapGenerator/secretGenerator files, `envs`, `env` and replacement path references navigate to exact local files. Regex, remote, dynamic, quoted-key, flow, merge, generated, indexed and unmodelled forms stay quiet. Block-list resource/component/patch paths offer relative completion, Go to Declaration and gutters: a unique Kustomization file is required; paths outside the project, remote URLs, ambiguous directory files and nested-project targets are refused; source literals and target existence are rechecked at invocation; provider text is limited to 256 KiB; gutters check at most 16 targets per request.

The label graph models exact node affinity (`matchExpressions`), pod affinity/anti-affinity and topology spread labelSelector terms; namespaces, `namespaceSelector`, `matchLabelKeys`, `matchFields` and `Gt`/`Lt` terms stay unmodelled. Label-key Rename edits single-line quoted YAML keys with `''` or basic double-quote escapes when the scanned text round-trips to the parsed value; multiline, block, anchored, tagged and `\u`-escaped scalars are refused.

**Find Local Label or Data-Key Occurrences** covers explicitly namespaced Pod/workload labels, supported complete selectors and named ConfigMap/Secret key references (data-key completion included), revalidating the source version and destination after selection. No cluster process or network request is needed; the cache keeps structural facts and source fingerprints, not Secret values. Dynamic/generated identity, absent namespaces, duplicate resource declarations and unsupported scalar spellings withhold authority.

**Cluster-backed hints.** **Read Cluster Namespaces for Completion** queries namespace names for the selected context and local redacted cluster identity (`process:kubectl` and an owning `project.read` invocation; only `namespace/<DNS-label>` output accepted; 64 KiB / 512 unique names). For 60 seconds, exact current `metadata.namespace` values in known built-in namespaced kinds get cached suggestions with UTF-16 replacement ranges and captured context/age labels (documents limited to 64 KiB, output to 200 suggestions; cold, expired, foreign, nested-project, changed-choice, malformed, flow-metadata, Helm-action, alias/tag/tab and unknown/cluster-scoped contexts stay quiet). **Load Resource Names from Cluster for Completion…** captures names for one chosen Secret, ConfigMap, Service, PersistentVolumeClaim or ServiceAccount kind in the selected single namespace/project (512 names / 64 KiB per kind, 60 seconds); completion uses them only at existing schema-known reference paths with an explicit matching source `metadata.namespace` and innermost project owner (up to 200 candidates). Both commands may connect and authenticate through kubectl, request names only, keep no object bodies or Secret values, and never run from completion or render; providers never run kubectl. A failed same-owner refresh retires that snapshot; failed foreign reads keep the prior owner's cache. These are dated metadata hints, not live authorisation, namespace existence validation or proof of a deployment target.

**Navigation cache.** Warmed by open, save and file-change events (a file change updates only the files it names): at most 64 source graphs / 8 MiB of accounted graph storage, project-owned YAML inputs up to 128 KiB, and only source names, relationship keys, positions and isolation facts (no Secret payload or full source text). Cache eviction drops the oldest file first and a file change evicts only the named paths; no persistence, cluster calls, kubectl, Helm execution or runtime discovery is involved. Editor requests reparse the current provider text, including unsaved edits, without project-wide scans, and enforce the optional `project.read` grant and ownership. Cached target candidates are re-read and reparsed before returning their current range, with a per-call 16-target-file cap plus a shared resolver's 10 ms cooperative read budget and 256-target cap. A target edit/deletion, denied permission, unknown project, foreign root, oversized source or unknown schema yields no affected link; namespace/transform checks run only while warming. The plugin registers no document indexer, so none of these associations enables automatic Rename/Find Usages; they're bounded positive source hints, not complete usage counts or runtime resolution.

</details>

<details>
<summary>CRD, JSON and values schema assistance</summary>

Indexed or opened local `apiextensions.k8s.io/v1` CRDs contribute structural-schema property and enum/boolean completion for matching group/version/kind resources. Only a unique same-project definition whose current source still matches its indexed snapshot is used; changed, duplicate or foreign-project definitions withdraw assistance. Diagnostics validate types, required fields, enums, explicit additional-property rejection, unknown fields the API server would prune (respecting `x-kubernetes-preserve-unknown-fields`, `additionalProperties` and embedded resources), numeric bounds, string length and supported patterns, collection sizes/uniqueness, read-only and deprecated fields, `x-kubernetes-int-or-string`, null on a non-nullable field (Info: dropped, or replaced by its default; `nullable: true` keeps null), and `metadata.namespace` on cluster-scoped CRDs. Ambiguous versions and composition keywords stay quiet. Cache limits are 64 CRD source files of 128 KiB each; providers don't run kubectl or fetch schemas.

JSON resources get physical-range built-in key diagnostics and root/spec/metadata completion through the same conservative kind tables, plus duplicate-key warnings; built-in Pod/Service enum choices and local CRD property/enum assistance are available. JSONC comments and trailing commas are masked without changing byte offsets; completion stays quiet inside comments, malformed JSON/JSONC stays quiet rather than getting guessed structural diagnostics, and UTF-16 locations keep multibyte source spelling.

A local chart's `values.schema.json` contributes guarded `values.yaml` completion and live diagnostics. Provider requests don't run Helm; full chart rendering still uses the explicit template/lint commands.

</details>

<details>
<summary>Port forwarding</summary>

Port mappings are numeric with valid ranges and unique explicit local ports; named remote ports are outside this form, and CLI flags can't be entered as mappings. Both modes show the target and argv before launch. Scope includes the selected kubeconfig/context, resolved namespace and a fingerprint of the selected local cluster/server/user/TLS/proxy metadata; raw credentials aren't kept and no `--raw` config read is made. Rechecking before launch/restart rejects changed selections or context-to-cluster mappings. This isn't an immutable kubeconfig snapshot, so a final filesystem/credential race at external CLI launch stays outside the plugin's atomic control.

**Start in Terminal** keeps the long-lived terminal workflow; managed controls don't control terminal or external sessions. **Start Managed Session** shows one confirmation with the project, the exact kubectl command and the loopback restriction. The managed draft belongs to a concrete invoking project root (`project.read` required) and starts with that project as working directory. After you confirm, the forward starts immediately, once the selection, settings and local kubeconfig fingerprint are checked again. Restart needs a stopped session and its own confirmation; replacing a running forward is confirmed and stops only that owned process. Spawn isn't a claim that a listener is ready; exits and polling/start failures are shown, and prior output is kept on a failed replacement within the 16 KiB presentation bound.

Managed forwarding is limited to the plugin instance's lifetime: idle reclamation (normally about five minutes), window close, disable/reload and failure end the child and forget its in-memory status. There's no keepalive loop or automatic reconnect. Observed project, resource, context, namespace or kubeconfig changes retire the owned child and need a new draft; status and controls refuse a different project's session, and only the owned handle is stopped. There's one lease per plugin instance, not a per-window registry (same-project windows share it). Rendering polls cached output without invoking kubectl; **Port Forward Status** and explicit refresh revalidate the captured local cluster mapping. Each refresh drains at most four bounded chunks and the displayed tail stays within 16 KiB on UTF-8 boundaries. There's no reader EOF guarantee, so this is available-output draining, not guaranteed final stream capture, and Stop/retirement may discard remaining output.

</details>

<details>
<summary>Managed resource watch</summary>

**Watch Resources…** shows one confirmation with the project and the exact kubectl command; after you confirm, the watch starts its owned `host.start` child (`get <kind> --watch --output-watch-events -o json` with pinned context/namespace and a ten-minute request timeout; needs `project.read` and `process:kubectl`). Scope inspection and watch spawn use the owning project working directory. The pane shows positive observed names only; it doesn't replace the Services tree, validate resource absence, publish Rename edges or prove a source deployment target.

**Refresh Watch Status** revalidates local scope. Every refresh and render drains all available output, resyncs at the next event after a malformed line, and skips and counts events that are too large; rendering doesn't run kubectl or discover files. Fragmented/concatenated JSON is accepted behind strict bounded structure, kind/API, namespace/name/UID and opaque `resourceVersion` checks. ADDED/MODIFIED update observations; DELETED removes only a matching UID; BOOKMARK is accepted without an assumed interval; ERROR/410, unknown shapes, caps and polling failures release the handle and keep the last accepted metadata as history. Raw object bodies and stderr aren't displayed. Bounds are 64 KiB per event, 128 KiB pending JSON, 16 levels / 4,096 YAML-guard nodes and 128 observed resources; the view shows at most 32 current and 16 prior names with counts, age and explicit incompleteness, and overflow stops instead of claiming a complete truncated result. **Stop Watch** kills/releases only its stored child. **Restart…** starts a new LIST→WATCH after a confirmation, with prior names kept as separately labelled history (never merged into possibly incomplete new observations); a failed new start keeps the previous owned session. Context/namespace/kubeconfig-choice changes retire live state; foreign renders give a safe message without stopping another project's watch; deactivation and lost project access dispose owned state; there's no durable per-window registry.

</details>

<details>
<summary>Pasted kubeconfig import</summary>

The import accepts YAML or JSON in a manual multiline prompt, asks for a new label and exact context, then shows a redacted review of profile/context/cluster/namespace/declared user alias/server and authentication mode. A missing namespace becomes an explicit `default` in the saved selected context. Only that context, its cluster and its optional user are normalised and stored. The review command revalidates stored identities and shows the same metadata without tokens, keys or certificate data; labels/aliases overlapping credential material are refused, and errors never include pasted parser excerpts or credential values. Import and review need only plugin storage: no kubectl/process, network, clipboard, installation, credential execution or cluster validation. Your kubeconfig/settings, active context, namespace, Services selection and forwarding scope are never touched.

Supported input is one Kubernetes `apiVersion: v1`, `kind: Config` document with unique named cluster/user/context arrays and valid references. Static bearer tokens, paired inline client-certificate/key data, optional inline CA data and anonymous users are supported, with bounded syntax (not X.509/key-pair) checks. API servers must be HTTPS ASCII DNS names, full decimal IPv4 authorities or bracketed IPv6 literals (compressed, expanded and IPv4-mapped spellings kept exactly), with an optional decimal port 1..65535 and a single terminal slash; no URI credentials, base path, query or fragment. IPv6 zone identifiers/IPvFuture, HTTP, insecure TLS, proxy/TLS-name overrides, impersonation, external credential files, tokenFile, exec, auth-provider and unknown extension fields are rejected, even on unused users/clusters. YAML aliases, anchors, tags, duplicate/complex keys and multiple documents are rejected before the document model is built. Bounds are 128 KiB input/canonical config, depth 16, 4,096 parser events, 64 entries per collection, 32 published profiles, 192 KiB per private record and 4 KiB per redacted metadata record.

Credential-bearing normalised UTF-8 JSON is written only under `host::fs::plugin_dir()` through `write_bytes`; that SDK directory and its files are owner-only (0700/private ACL), persist across reload/restart and are removed on uninstall. Generic KV storage holds only redacted metadata and an opaque private-file identity/path. Files use a SHA-256 identity of the complete canonical private record: existing different contents are refused, an identical unpublished record is reused, and both stored bytes and redacted metadata are revalidated before review. There's no exclusive-create/CAS or private-file deletion: concurrent external edits, non-transactional count/label prechecks and cross-file payload/metadata publication aren't atomic, and a metadata failure after the private write can leave an owner-only orphan (capped at 64; retrying the identical import can publish it; no automatic orphan removal or credential rotation overwrite). Corrupt identities fail closed and existing profile records are kept. These are plugin-owned profiles, not a native account manager or encrypted OS credential vault.

</details>

<details>
<summary>Settings, permissions and project scoping</summary>

**Choose Kubeconfig** picks one file, not a merge of them. `storage` is required. Denied capabilities degrade: commands stay gated by their own grants, and the tree labels Project or Global CLI mode. Explicit global compatibility remains for browsing with no invoking project, standalone Helm repository operations and unbound forwarding inspection; disconnected kubeconfig import/review, static help/preferences and external global utility commands don't need a project. The external-file browsing workflow may use exec-based auth plugins installed on the IDE `PATH`; the pasted-import subset rejects executable authentication.

Project-capable browser, resource, Secret and source-command process calls use one explicit invocation project as their working directory (relative kubeconfig settings resolve from that one base), local to the call and never reused as a last-project fallback. A captured project that closes or loses access can't silently become a global process, and a known invoking project whose identity lookup is denied fails before process or editor actions. Source manifest, Kustomize, Helm chart and Reload operations reject an editor owned by another or an innermost nested project before running a tool or applying a reload edit. Discovered resource types are admitted only as literal DNS plural/group names, never CLI options, and lookup validates again before execution. API-resource discovery refuses malformed or oversized batches (256 KiB / 4,096 types) as an error rather than a false empty inventory. Single-namespace callbacks reject slash/percent paths, comma target lists, whitespace/control characters, dot segments and leading options (literal RBAC names containing colons stay supported), and bare resource parsing keeps the original admitted name including embedded or repeated `~`.

Cached rich details, revealed Secret plaintext, Secret edit/draft data, pending manifest review, log snapshots and forwarding/watch drafts are bound to a concrete invoking project (`project.read`), the current context/namespace and kubeconfig choice; foreign, missing or revoked identity and scope/choice changes clear them before rendering or stale write actions. Resource rows and mutable controls carry opaque owner/settings-epoch and instance proofs: selection, context-menu commands, native detail requests and old forms retire after project/context/namespace/kubeconfig changes (even with the same literal kind/name), stale callbacks refuse, failed submissions keep current tokens for retry, and long names use short issued IDs or explicit non-actionable rows within the host's 128-byte field-key and 1,024-byte tree-ID bounds. Native invocation identity is a per-call snapshot, so a project or credential change after an event begins can still race process start (API 1.3 has no atomic expected-project/client check at spawn), and same-path kubeconfig/credential changes aren't continuously watched. These proofs correlate UI state; they don't grant Kubernetes permissions, identify resource UIDs or prove runtime deployment ownership.

State changes refresh both the native Services tree and the packaged resource panel: the panel receives only the model-free `{"channel":"ui-kit-refresh"}` signal, and its hub requests a new model with its own frame, sequence and project identity, coalescing while an action or form field is pending; stable renders emit no further invalidation. Successful panel navigation replaces the prior visible mode (Secret reveal/edit/create, YAML/describe, log snapshot or forwarding review); known unsaved Secret form values refuse replacement until Save or explicit Back, and panel navigation keeps an independently owned forwarding child, watch or private editor. Icons are the Kubernetes logo and Icons Set; tree items can only name icons the IDE knows, so each is declared as a `fileIcons` entry on a pattern no file has and the tree uses its `ext:fileIcon/aritti.kube-explorer/<id>` name.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
