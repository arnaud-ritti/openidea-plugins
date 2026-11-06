# Kube Explorer

Browse a Kubernetes cluster from OpenIDEA through `kubectl`.

- **Kube Explorer** tool window (right): double-click a context or namespace
  to switch to it, and expand a kind to list its resources. The toolbar acts on
  the selected resource: Describe (or double-click), YAML, Logs (pods; for
  Deployments, StatefulSets, DaemonSets and Jobs, the logs of all their pods),
  Shell, Attach, Port Forward (edit the `local:remote` ports, then Start) and
  Delete (click twice to confirm). Output replaces the tree until you click Back.
  **Custom Resources** lists the cluster's CRD kinds (`kubectl api-resources`,
  without core and built-in groups) by API group; expanding a kind lists its
  instances, which take Describe, YAML and Delete like any resource.
- **YAML** opens in a regular editor on `/tmp/kube-explorer-<kind>-<name>-<random>.yaml`,
  with a `yaml-language-server` schema modeline (completion, validation and
  docs from the Kubernetes JSON schemas). Every save runs `kubectl replace -f`
  in the context and namespace the file was opened from. The file leaves out
  `metadata.resourceVersion`, so a save overwrites any change made in the
  cluster since. `tee` writes the file (`process:tee`, optional) because the
  plugin API can't write files. Without `tee`, `events` or `project.read`, or on
  Windows, you edit the YAML in the panel instead.
- **⌥⏎ on a manifest**: *Add Kubernetes schema* (one-document files of a
  built-in kind).
- **Manifest problems** on open and save (optional `events` and `project.read`;
  a file is a manifest when a document has both `apiVersion:` and `kind:`; files
  with Helm `{{ }}` actions only get the Helm checks): `deprecated-api-version`
  (versions removed in v1.16, v1.22 or v1.25, with the replacement),
  `duplicate-env-name`, `missing-name`, `missing-selector` (apps/v1 workloads),
  `chart-missing-key` (`Chart.yaml` without `apiVersion`, `name` or `version`)
  and `helm-whitespace-trim` (`{{-foo` and `foo-}}` in a chart's `templates/`;
  a trim marker needs a space beside the dash). Each has a switch in the settings.
- **Commands** on the open file: Apply, Validate (server dry run), Diff,
  Delete Manifest Resources (lists them with a server dry run, then waits for
  Confirm Delete), Render / Apply Kustomization (the file's directory), Preview
  Helm Template, Lint Helm Chart and Update Helm Dependencies (the chart
  around the file; optional `process:helm`).

The chosen context and namespace are passed as `--context` / `--namespace`,
so your kubeconfig is never modified. To use another kubeconfig file, set it in
Settings › Plugins › Kube Explorer; otherwise kubectl uses its default.

Not ported: cluster-backed checks (unknown resources or fields against the
live API), CRD-aware validation, and Helm values checks.

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
