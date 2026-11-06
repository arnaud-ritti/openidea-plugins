# Gap F Rows: Infra and Data Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gap rows the audit classed **F** for five plugins: `kube-explorer` (9 rows), `openapi` (5), `jpa-assist` (7), `devcontainer` (2) and `flyway` (1), building only the part of each row that the current API supports.

**Architecture:** Text-based checks over existing scanners (`publish_diagnostics` on `file-opened`/`file-saved`), static completion items, code actions on existing files, two fixed-argument commands, `fileTemplates`, and `runDetectors`. No new API is assumed. Tasks are independent and shippable alone, except where a task lists a dependency. Every F row was re-checked against the code and `api-facts.md`; what did not survive is in _Rulings_ and has no task.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK 0.1.37 (`openidea:plugin@1.1.0`), `MockHost` tests, `spring_text` (used by `openapi`).

**Spec:** [`docs/audit-gaps-2026-11-06.md`](../../audit-gaps-2026-11-06.md) (appendix, rows classed F), evidence in [`docs/audit-gaps-2026-11-06/B-infra-data.md`](../../audit-gaps-2026-11-06/B-infra-data.md), API limits in [`docs/audit-gaps-2026-11-06/api-facts.md`](../../audit-gaps-2026-11-06/api-facts.md). Style and shared task shape follow [`2026-11-06-feasible-parity.md`](2026-11-06-feasible-parity.md).

## Global Constraints

- Branch `gap-f-infra-data` (worktree `/Users/aritti/Projects/openidea-plugins-gap-f-infra-data`, from `main` `50aa031`). Each plugin stays its own Cargo project; run cargo inside `plugins/<name>` with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Per plugin, finish with `cargo test` (no new warnings) and `openidea-plugin build` (runs `check`).
- Never hand-edit `version` in `plugin.toml` (release-please). Conventional commits `feat(<plugin>): ...` / `fix(<plugin>): ...`; every message ends with the line `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not touch `.env`.
- Implementers do not edit `PARITY.md` or `LIMITATIONS.md`; they write a "Doc notes" paragraph in their report (what changed in the plugin's README "Not ported", limitation text and parity row).
- Licence: the originals of `kube-explorer`, `jpa-assist`, `openapi`, `devcontainer` and `flyway` are proprietary. Write from public documentation only (Kubernetes, Helm, OpenAPI, devcontainer spec, JPA, Flyway docs). Never copy vendor text: messages, thresholds, template bodies and trigger names are our own. Do not open decompiled code of the originals.
- **Quiet unless sure:** a name that is built at run time, comes from a library, or cannot be resolved stays invisible (no warning). Every check has a test `<check>_is_quiet_for_library_or_dynamic_name` (or a more specific quiet test named in its task).
- **UTF-16 and CRLF:** every scanner gains `ranges_use_utf16_columns_with_multibyte_text` and `crlf_files_report_correct_lines`.
- **Fixed file publishes an empty list:** every check group has `fixed_file_publishes_empty_list`.
- **One settings boolean per check group** (default on) in the plugin's `settings.schema.json`, read with `host::setting_bool(..) != Some(false)` (or the plugin's existing helper), and a test that switching it off silences the group.
- `events` is an optional capability: `activate` subscribes and logs a `Warn` on denial (`denied_events_log_a_warning_and_still_activate`). Every event-driven plugin has an `activation` entry (`on-event:file-opened`, `on-event:file-saved`).
- Providers have a 50 ms budget: scan in events or commands and cache; a provider returns immediately for files it does not own.
- `find_files` skips hidden and ignored paths on the real host, the mock does not. Devcontainer directories are hidden: use `read_file` on explicit paths, never a scan. A missing file is `Invalid` on the host and `NotFound` on the mock: treat both as absent (`missing_file_is_treated_as_absent`).
- Code actions edit existing files only (several files allowed); they cannot create files and cannot contain snippets (completion items can: `$0`, `${1:x}`).
- No popups, quick-picks or input dialogs. A command's returned text is dropped by the IDE: report with `host::notify`.
- Process programs are fixed in the manifest (`process:<program>`): check `plugin.toml` `[capabilities]` before adding a call; never build a program name from data.
- Not available, so never planned here: types (U4), rename and references (U1), file creation (U2), injection, streaming, decorations, caret control, library-bean knowledge.

## Review Focus

Failure modes the audit implies and no happy path exercises; each line has its test in the owning task.

- **Text from a process or the project reaching an argv:** a namespace, name or file path that starts with `-` or contains `..` or `/` must be one inert argument or refused, never an option. Tasks 2 and 3 (`*_option_like_*`, `*_path_escape_*`).
- **Destructive action in the wrong namespace:** in the all-namespaces view `delete` must name the row's own namespace and refuse a row without one. Task 3.
- **Plain files mistaken for the target format:** a `paths:` key in an unrelated YAML, a `devcontainer.json` in another tool's folder, a `Chart.yaml` with templated values, a `package.json` read as a spec. Tasks 1, 4, 6 (`*_ignores_unrelated_*`).
- **Mock vs host divergence:** hidden-directory files read through `read_file`, missing files read as absent. Tasks 4 and 6.
- **Stale diagnostics after fixing:** the candidate hint must disappear when the root key is added; the structure hint when the file moves. Tasks 4 and 6 (`fixed_file_publishes_empty_list`).

## Shared task shape

Unless a task says otherwise:

- [ ] **Step 1:** Write the failing tests named in the task, in the plugin's test module (`#[cfg(test)]` in `lib.rs`/`checks.rs`, as the plugin already does), against `MockHost`.
- [ ] **Step 2:** `cd plugins/<name> && cargo test <test_name>`; expect FAIL on the missing behaviour.
- [ ] **Step 3:** Implement in the named files; wire it in `lib.rs` (and `plugin.toml` / `settings.schema.json` when listed).
- [ ] **Step 4:** `cargo test` all green with no new warnings, then `openidea-plugin build`; expect no errors.
- [ ] **Step 5:** Commit `feat(<plugin>): <what>` with the Co-Authored-By trailer. Report "Doc notes" (no edits to `PARITY.md`/`LIMITATIONS.md`).

## Rulings

Rows re-checked against the code and `api-facts.md`. Nothing below has a task.

| Plugin, row | Ruling | Why |
|---|---|---|
| kube-explorer, Refresh | **Already done** | `render_tree` adds a `refresh` button (`src/lib.rs:501`) handled by `reload()` (`:658`). The audit read only the YAML page. Task 3 adds a regression test because it changes `reload`. |
| kube-explorer, Chart.yaml unknown keys/values (rows 6, 7) | **Narrowed** | `Chart.yaml` only. `requirements.yaml` (Helm 2) is not read: its schema is not in the current Helm docs. |
| kube-explorer, Excessive whitespace trim (row 10) | **Narrowed to a stated rule** | The original's rule is unknown (audit saw the name only). We implement one rule from the Go `text/template` and Helm docs (trim that glues an output action to YAML text across a line break); control actions, comments and `indent`/`nindent` pipelines stay quiet. |
| jpa-assist, Immutable not-nullable property (row 2) | **Subsumed by row 3** | A `val` attribute is flagged whatever its nullability (JPA 3.1 §2.1: persistent fields must not be final); a separate non-null rule cannot be specified from public documentation. |
| jpa-assist, Persistent object class signature (row 10) | **Part only** | Built: nested (inner) entity, entity combined with `@Embeddable`/`@MappedSuperclass`, entity declared as interface/enum. Not built: `final` (Java already done; Kotlin `final` needs compiler-plugin detection, class F\*), and "entity and listener at once" (JPA allows an entity to hold its own callbacks, so a warning would be wrong). |
| jpa-assist, Entity listener warnings (row 12) | **Narrowed** | Only a listener whose source is found exactly once in the project by simple name and that has no superclass; unresolved or inherited listeners stay quiet (no type resolution, U4). |
| devcontainer, Completion (row 10) | **Built with a disclosed risk** | The language id of `devcontainer.json` is `jsonc`, owned by the built-in definition (`LIMITATIONS.md`, devcontainer section). The provider is declared for `["json", "jsonc"]`; whether the host calls it for `jsonc` is unverified in the IDE. Properties and enum values only (top level); feature-id completion is F\* and out of scope. |
| devcontainer, Folder structure (row 2) | **Path-only** | No `find_files` (hidden dirs), no `read_file` of siblings. |
| flyway, Run configuration producer and info providers (row 6) | **Producer only** | `runDetectors` have fixed args at the project root, so they cannot honour the `runner`/`arguments` settings or `mvn`, and only `info`, `validate`, `migrate` are offered (`clean` and `repair` are never one-click). The info provider is the existing Migrations view. |
| openapi, Reload specification (row 14) | **Built, weakly verifiable** | A button in the preview page (`web/`); cargo cannot run page JavaScript, so only the wiring is asserted by tests. Needs a manual IDE check. |

## File structure

| Plugin | Create | Modify |
|---|---|---|
| kube-explorer | `templates/csr.yaml` | `src/checks.rs`, `src/manifest.rs`, `src/lib.rs`, `plugin.toml`, `settings.schema.json` |
| openapi | `templates/swagger.yaml`, `templates/swagger.json` | `src/checks.rs`, `src/lib.rs`, `plugin.toml`, `web/index.html`, `web/main.js` |
| jpa-assist | none | `src/checks.rs`, `src/lib.rs`, `src/generate.rs`, `settings.schema.json` |
| devcontainer | none | `src/checks.rs`, `src/lib.rs`, `plugin.toml`, `settings.schema.json` |
| flyway | none | `plugin.toml`, `src/lib.rs` (manifest test) |

---

## kube-explorer

### Task 1: Manifest checks: deprecated table, Chart.yaml schema, excessive trim (rows 5, 6, 7, 10)

**Files:**
- Modify: `plugins/kube-explorer/src/checks.rs`, `src/manifest.rs`, `src/lib.rs` (the `inspect` setting lookup already uses `checks::SETTINGS`), `settings.schema.json`

**Interfaces:**
- Consumes: `Finding::new`, `documents`, `Doc`, `for_file(path, text, in_chart, patch_context)`.
- Produces: codes `CHART_UNKNOWN_KEY = "chart-unknown-key"`, `CHART_INVALID_VALUE = "chart-invalid-value"`, `HELM_EXCESSIVE_TRIM = "helm-excessive-trim"`; `pub fn chart_schema(text: &str) -> Vec<Finding>`; `pub fn helm_excessive_trim(text: &str) -> Vec<Finding>`; `Doc.top_entries: Vec<(String, Range<usize>, Option<Value>)>` (key, key byte range, value) filled by `scan` for indent-0 keys. `SETTINGS` gains `(CHART_UNKNOWN_KEY, "checkChartSchema")`, `(CHART_INVALID_VALUE, "checkChartSchema")`, `(HELM_EXCESSIVE_TRIM, "checkHelmExcessiveTrim")`; `settings.schema.json` gets both booleans (default `true`).

Rules (own, from the Kubernetes deprecation guide and the Helm chart file reference):
- `DEPRECATED` gains rows `(apiVersion, kinds, replacement, removed-in)`: `autoscaling/v2beta1` HorizontalPodAutoscaler -> `autoscaling/v2` v1.25; `autoscaling/v2beta2` HorizontalPodAutoscaler -> `autoscaling/v2` v1.26; `discovery.k8s.io/v1beta1` EndpointSlice -> `discovery.k8s.io/v1` v1.25; `events.k8s.io/v1beta1` Event -> `events.k8s.io/v1` v1.25; `node.k8s.io/v1beta1` RuntimeClass -> `node.k8s.io/v1` v1.25; `storage.k8s.io/v1beta1` CSIStorageCapacity -> `storage.k8s.io/v1` v1.27; `storage.k8s.io/v1beta1` CSIDriver, CSINode, StorageClass, VolumeAttachment -> `storage.k8s.io/v1` v1.22; `certificates.k8s.io/v1beta1` CertificateSigningRequest -> `certificates.k8s.io/v1` v1.22; `coordination.k8s.io/v1beta1` Lease -> `coordination.k8s.io/v1` v1.22; `scheduling.k8s.io/v1beta1` PriorityClass -> `scheduling.k8s.io/v1` v1.22; `apiregistration.k8s.io/v1beta1` APIService -> `apiregistration.k8s.io/v1` v1.22; `authentication.k8s.io/v1beta1` TokenReview and `authorization.k8s.io/v1beta1` SubjectAccessReview, LocalSubjectAccessReview, SelfSubjectAccessReview -> `/v1` v1.22. The implementer verifies each row against `kubernetes.io/docs/reference/using-api/deprecation-guide/` before committing; a row not confirmed there is dropped. Rows match on apiVersion and kind together, so a custom resource of another group stays quiet.
- `chart_schema` (file name exactly `Chart.yaml`, no `{{` in the text): top-level keys outside `apiVersion, name, version, kubeVersion, description, type, keywords, home, sources, dependencies, maintainers, icon, appVersion, deprecated, annotations` warn `chart-unknown-key` on the key (for `apiVersion: v1` also allow `engine, tillerVersion`; `x-*` always allowed). Values warn `chart-invalid-value` on the value: `apiVersion` not `v1`/`v2`; `type` not `application`/`library`; `deprecated` not `true`/`false`; `version` not starting with an optional `v` and a digit. A templated or empty value is quiet.
- `helm_excessive_trim` (templates in a chart only, same gate as `helm_whitespace_trim`): an `{{-` or `-}}` marker is excessive when the whitespace it removes contains a line break, the neighbour it joins to is YAML text (not another action), and the action is an output action. Quiet for control and structure words (`if`, `else`, `end`, `range`, `with`, `define`, `template`, `block`; `include` is an output action unless piped to `indent`/`nindent`), comments, assignments (`:=`, `=`), and any pipeline containing `indent` or `nindent`.

- [ ] **Step 1: Write the failing tests** (in `checks.rs` tests, using the existing `codes`/`highlighted` helpers; the chart ones call `for_file("Chart.yaml", ..)`):
  - deprecated: `autoscaling_v2beta2_hpa_warns_with_replacement` (message contains `autoscaling/v2` and `v1.26`), `csi_driver_v1beta1_warns`, `deprecated_api_version_is_quiet_for_library_or_dynamic_name` extended with `apiVersion: autoscaling/v2beta2` + `kind: Widget`, and `coordination.k8s.io/v1beta1` + `kind: Foo`.
  - chart: `chart_unknown_top_level_key_warns` (`colour: red` highlighted as `colour`), `chart_v1_allows_engine_key`, `chart_x_prefixed_key_is_quiet`, `chart_type_must_be_application_or_library`, `chart_api_version_must_be_v1_or_v2`, `chart_deprecated_must_be_boolean`, `chart_version_without_digit_warns`, `chart_values_are_quiet_when_templated_or_empty`, `chart_schema_is_quiet_for_library_or_dynamic_name` (nested `maintainers:` keys and `annotations:` keys never warn), `chart_schema_ignores_unrelated_yaml` (a file not named `Chart.yaml`).
  - trim: `left_trim_joining_an_output_to_the_previous_line_warns`, `right_trim_joining_an_output_to_the_next_line_warns`, `trim_on_control_actions_is_quiet`, `nindent_pipeline_is_quiet`, `trim_between_two_actions_is_quiet`, `trim_on_comments_and_assignments_is_quiet`, `excessive_trim_is_only_read_by_the_helm_group`.
  - cross-cutting for all three: `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines` (a `Chart.yaml` and a template with `\r\n`), `fixed_file_publishes_empty_list` (publish a bad `Chart.yaml`, save the fixed one, expect `host.diagnostics()` empty), `each_group_can_be_switched_off` for `checkChartSchema` and `checkHelmExcessiveTrim`.
- [ ] **Steps 2 to 5:** shared shape, commit `feat(kube-explorer): deprecated API table, Chart.yaml schema and excessive trim checks`.

### Task 2: Commands without an editor: Helm repo update, kubectl install hint, CSR template (rows `Helm update repository`, `How to install kubectl`, `CertificateSigningRequest`)

**Strongest reviewer** for the two commands: they are the process-touching rows. No user text reaches an argv in either, and the tests must prove it.

**Files:**
- Modify: `plugins/kube-explorer/src/lib.rs`, `plugin.toml`
- Create: `plugins/kube-explorer/templates/csr.yaml`

**Interfaces:**
- Consumes: `HELM` const, `failure(program, stderr, status)`, `host::run_process`, `host::notify`, `Plugin::run_command`.
- Produces: commands `helm-repo-update` ("Kube Explorer: Update Helm Repositories") and `how-to-install-kubectl` ("Kube Explorer: How to Install kubectl"); `const KUBECTL_INSTALL_HINT: &str` (static text: kubectl missing or not on `PATH`, the page `https://kubernetes.io/docs/tasks/tools/`, one package-manager line each for macOS (`brew install kubectl`), Windows (`winget install -e --id Kubernetes.kubectl`) and Linux (your package manager, or the binary download on that page)); `fn helm_repo_update_args() -> Vec<String>` returning exactly `["repo", "update"]`; a tree button `how-to-install-kubectl` shown only when the context lookup failed with a non-`Denied` error; template `k8s-csr` ("Kubernetes CertificateSigningRequest", `templates/csr.yaml`, `apiVersion: certificates.k8s.io/v1`, `metadata.name: $NAME$`, `spec.request`, `spec.signerName`, `spec.usages`, `$END$`).
- `plugin.toml` capabilities are unchanged (`process:helm` is already optional, `process:kubectl` required); the commands are added to `[[contributes.commands]]` only. No capability is added for the hint (it runs nothing).

Both commands are handled in `run_command` before `command()`, which demands an open editor. The Helm result is reported with `host::notify` (last 20 lines of stdout, capped at 2000 characters); a non-zero status returns `failure(HELM, ..)`; `Error::Denied` becomes a notification asking to allow `helm` in Settings > Plugins.

- [ ] **Step 1: Write the failing tests:**
  - `repo_update_runs_exactly_helm_repo_update` (`MockHost::on_process("helm", ..)` records args, expect `["repo","update"]`, no cwd, no stdin).
  - `repo_update_ignores_the_command_argument` (`run_command("helm-repo-update", .., Some("--debug"))` and `Some("../../etc")`: argv unchanged).
  - `repo_update_does_not_read_the_open_file` (editor path `-rf/../x.yaml`; still `["repo","update"]`, no `read_file`).
  - `repo_update_without_helm_capability_notifies_instead_of_failing`; `repo_update_failure_reports_stderr`; `repo_update_needs_no_open_editor`.
  - `install_hint_spawns_no_process` (`host.processes()` empty), `install_hint_text_is_static` (set setting `kubeconfig` to `; rm -rf ~` and the context to `-evil`: the notification equals `KUBECTL_INSTALL_HINT`), `install_hint_needs_no_open_editor`.
  - `tree_offers_the_install_hint_only_when_kubectl_fails` (button present when `config current-context` errors with a generic error, absent on success and on `Denied`).
  - `csr_template_is_registered` is covered by the existing `every_template_is_registered_and_exists` once `assert_eq!(templates.len(), 16)` and the `(16, 16)` set sizes are updated; `templates_pass_the_manifest_checks` must pass for `csr.yaml` (it has `metadata.name`, a current apiVersion).
- [ ] **Steps 2 to 5:** shared shape, commit `feat(kube-explorer): update Helm repositories, kubectl install hint, CSR template`.

### Task 3: All namespaces (row `All Namespaces`)

**Strongest reviewer:** `delete`, `shell`, `debug`, `show-secret` and `port-forward` act on a selected row; in this mode the row carries its own namespace, and a wrong or missing one would act in the kubeconfig's default namespace.

**Files:**
- Modify: `plugins/kube-explorer/src/lib.rs`

**Interfaces:**
- Consumes: `args`, `load`, `reload`, `render_tree`, `handle_tree`, `button`, `resource(key)`, `KINDS`, `secrets::valid_key`-style validators.
- Produces: `const ALL_NAMESPACES: &str = "*"` stored as the `namespace` storage value; `fn is_dns_label(s: &str) -> bool` and `fn is_dns_subdomain(s: &str) -> bool` (Kubernetes name rules, lowercase alphanumerics, `-`, and `.` for subdomains, 1 to 253 chars, start and end alphanumeric); `fn split_scope(&self, name: &str) -> Result<(Option<String>, String), Error>`.

Design: the namespace list gets a first child with key `ns/*` labelled "All namespaces"; activating it stores `*`. In that mode `args()` emits no `--namespace`, `load` for a namespaced kind runs `get <kind> --all-namespaces -o jsonpath={range .items[*]}{.metadata.namespace}{"/"}{.metadata.name}{"\n"}{end}` and the children are `ns/name` (an empty `ns` for cluster-scoped kinds). Every per-resource call goes through `split_scope`: it validates both parts (`is_dns_label` for the namespace, `is_dns_subdomain` for the name), then adds the single argument `--namespace=<ns>` and the bare name. A row with an empty namespace for a kind not in a cluster-scoped list (`nodes`, `namespaces`, `pv`, and the `KINDS` entries that are cluster-scoped) is refused. `create-secret` is refused in this mode ("pick a namespace first"). `target()` shows `(all namespaces)`.

- [ ] **Step 1: Write the failing tests:**
  - `all_namespaces_node_is_the_first_namespace_child` and `choosing_all_namespaces_persists_star_and_lists_with_all_namespaces_flag` (`get pods --all-namespaces` appears in the recorded args; no `--namespace=` in list calls).
  - `rows_show_their_namespace_and_name`, `cluster_scoped_kinds_have_no_namespace_prefix`.
  - `describe_and_yaml_in_all_mode_pass_the_rows_namespace`; `delete_in_all_namespaces_targets_the_rows_namespace` (confirm flow; recorded args contain `--namespace=team-a` and the bare name, never `team-a/web`).
  - `no_resource_action_runs_without_a_namespace_in_all_mode` (a namespaced kind with an empty namespace is refused for `delete`, `shell`, `debug`, `show-secret`, `port-forward`, `logs`).
  - Option injection and path escape: `row_with_option_like_namespace_is_refused` (`--all/x`, `-n/x`), `row_with_option_like_name_is_refused` (`team-a/--force`), `row_with_dotdot_or_extra_slash_is_refused` (`../x`, `a/b/c`, `a//b`), `namespace_reaches_kubectl_as_one_equals_argument` (value never a separate arg).
  - `create_secret_is_refused_in_all_namespaces_mode`.
  - Regression: `refresh_button_reloads_expanded_nodes` (clears the cache and reloads expanded keys in both modes), `existing_single_namespace_behaviour_is_unchanged` (the current tests still pass without edits).
- [ ] **Steps 2 to 5:** shared shape, commit `feat(kube-explorer): browse all namespaces`.

---

## openapi

### Task 4: Spec-candidate hint and JSON endpoints (rows `Possible specification candidate`, `Endpoints panel`)

**Files:**
- Modify: `plugins/openapi/src/checks.rs`, `src/lib.rs`, `plugin.toml` (settings contribution and schema, see below)
- Create: `plugins/openapi/settings.schema.json` (`openapi.checkSpecCandidates`, boolean, default `true`)

**Interfaces:**
- Consumes: `spec::scan`, `spec::operations(text, &keys)`, `spec::parse`, `problems(path)`, `endpoint_items(project)`.
- Produces: `pub fn spec_candidate(text: &str) -> Vec<Diagnostic>` in `checks.rs`, code `missing-openapi-root`, severity `Information`, one diagnostic on the first `paths` key; empty unless the text has no root `openapi`/`swagger` key, has a top-level `paths` object whose children start with `/`, and at least one child holds an HTTP method key (`spec::operations` non-empty). `problems()` calls it when `spec::parse` returns `None` and the setting is on. A cheap gate (`text.contains("paths")`) runs before any scan. `plugin.toml` gets `[contributes.settings] title = "OpenAPI" schema = "settings.schema.json"`.
- `endpoint_items` also lists `**/*.json`, skipping files over 1 MiB before parsing (`spec::parse` already gates on `openapi`/`swagger`).

- [ ] **Step 1: Write the failing tests:**
  - `yaml_with_paths_but_no_root_key_gets_a_hint`, `json_with_paths_but_no_root_key_gets_a_hint`, `hint_disappears_when_the_root_key_is_added` (`fixed_file_publishes_empty_list`).
  - Quiet: `spec_candidate_is_quiet_for_library_or_dynamic_name` (a `paths:` list of strings, a `paths` object whose keys do not start with `/`, `paths` with `/x` but no method key, `package.json` with a `paths` entry), `candidate_hint_ignores_unrelated_yaml` (docker-compose, GitHub workflow), `candidate_check_can_be_switched_off`.
  - `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.
  - Endpoints: `lists_endpoints_of_json_specs` (a JSON spec next to a YAML spec; the activated key `line:col:path` opens the JSON file), `endpoints_skip_non_spec_json_and_huge_files` (`package.json`, a 2 MiB JSON), `endpoints_view_counts_yaml_and_json_files`.
  - Host divergence: `missing_file_is_treated_as_absent` (both error kinds).
- [ ] **Steps 2 to 5:** shared shape, commit `feat(openapi): hint at spec fragments without a root key, list JSON endpoints`.

### Task 5: Swagger 2.0 templates, skeleton completion, preview reload (rows `Swagger 2.0 templates`, `Live templates`, `Reload specification`)

**Files:**
- Create: `plugins/openapi/templates/swagger.yaml`, `templates/swagger.json`
- Modify: `plugins/openapi/plugin.toml` (two `fileTemplates`: `swagger-yaml` "Swagger 2.0 (YAML)", `swagger-json` "Swagger 2.0 (JSON)"), `src/lib.rs` (completion), `web/index.html`, `web/main.js`

**Interfaces:**
- Consumes: `Providers::completion`, `spec::maybe_spec`, `spec::is_json`, `ref_at`.
- Produces: `fn skeleton_items(doc: &Document) -> Vec<CompletionItem>`; triggers (our own names, not the vendor's): `openapi-path` (a path item with a `get` operation and a 200 response), `openapi-operation` (an operation skeleton: summary, operationId, parameters, responses), `swagger2-root` (a Swagger 2.0 root with info, paths). Each has a YAML and a JSON body chosen by the document (`spec::is_json(&doc.text)` or the `.json` extension), `kind = CompletionKind::Snippet` (or the SDK's snippet kind), `insert_text` using `${1:..}` and `$0`. Offered only when the file is a spec by name (`openapi.*`, `swagger.*`, `*.openapi.*`, `*.swagger.*`) or `spec::maybe_spec(&doc.text)`, and never when the caret is inside a `$ref` (the existing `ref_at` branch wins). The preview page gets a "Reload" button (`id="reload"`) whose handler clears `known` and calls `window.openidea.document.get().then(doc => render(doc.text))`.
- Template bodies are written from the OpenAPI 2.0 and 3.x specifications; `$NAME$` for the title and `$END$` as in `openapi.yaml`.

- [ ] **Step 1: Write the failing tests:**
  - `swagger_templates_are_registered_and_exist` (reads `plugin.toml` with `include_str!`, asserts five ids, files exist, only `$NAME$`/`$END$` placeholders), `swagger_templates_are_detected_as_specs` (`spec::parse` on each file with `$NAME$` replaced returns version `2.0`, and `checks::check` reports nothing).
  - `skeleton_items_are_offered_in_a_spec_yaml_and_json` (YAML body for `openapi.yaml`, JSON body for `openapi.json`; the JSON body parses with `json::keys` and contains the expected keys), `skeletons_are_offered_in_an_empty_openapi_named_file`, `skeletons_are_quiet_in_unrelated_yaml_and_json` (`docker-compose.yml`, `package.json`), `skeletons_do_not_hide_ref_completion` (inside a `$ref` only references are returned), `skeletons_ignore_the_trigger_character`.
  - `preview_page_has_a_reload_button` (`include_str!("../web/index.html")` contains `id="reload"`; `main.js` binds it and resets `known`). Manual IDE check required: record it in the report.
- [ ] **Steps 2 to 5:** shared shape, commit `feat(openapi): Swagger 2.0 templates, skeleton completion and a preview reload button`.

---

## devcontainer

### Task 6: Folder-structure hint and completion (rows `Dev Container folder structure`, `Completion`)

**Files:**
- Modify: `plugins/devcontainer/src/checks.rs`, `src/lib.rs`, `plugin.toml`, `settings.schema.json`

**Interfaces:**
- Consumes: `is_devcontainer_file(path)`, `inspect(path)`, `enabled_findings`, the JSONC scanner (`members`, `skip_trivia`), `Finding`.
- Produces: code `FOLDER_STRUCTURE = "folder-structure"`; `pub fn folder_structure(path: &str) -> Option<Finding>` (path-only); `pub fn context_at(text: &str, offset: usize) -> Option<Context>` with `pub enum Context { Key, Value { key: String } }` for the top-level object only; `Providers::completion` impl. Settings `checkFolderStructure` (default on) and `completeProperties` (default on).
- `plugin.toml`: `[[contributes.providers]] kinds = ["completion"] languages = ["json", "jsonc"]`. Capabilities unchanged; `Providers` joins `register_plugin!(Main, providers)`.

Rules:
- Structure (from the devcontainer specification: `.devcontainer/devcontainer.json`, `.devcontainer.json`, or `.devcontainer/<folder>/devcontainer.json`, one level): a file named exactly `devcontainer.json` is flagged (Information, on the first token, message our own) when neither its parent directory nor its grandparent directory is `.devcontainer`. Path segments are normalised (`\` to `/`). A file named `.devcontainer.json` is never flagged. `is_devcontainer_file` is widened to every file named `devcontainer.json`/`.devcontainer.json`, but the property and features checks still run only in the valid locations, so a misplaced file gets the structure hint and nothing else.
- Completion (top-level only, `doc.path` must satisfy the widened `is_devcontainer_file`): in key position, the properties of `KNOWN` not already present, inserted as `"name": $0`; in value position of `shutdownAction` (`none`, `stopContainer`, `stopCompose`), `userEnvProbe` (`none`, `loginShell`, `loginInteractiveShell`, `interactiveShell`), `waitFor` (`initializeCommand`, `onCreateCommand`, `updateContentCommand`, `postCreateCommand`, `postStartCommand`, `postAttachCommand`), `updateRemoteUserUID`, `overrideCommand`, `init`, `privileged` (`true`, `false`). Nothing inside nested objects or arrays, inside comments, or when the text cannot be scanned. The scanner must tolerate unfinished text (an unterminated string at the caret).

- [ ] **Step 1: Write the failing tests:**
  - Structure: `devcontainer_json_outside_a_devcontainer_folder_gets_a_hint`, `config_two_levels_below_devcontainer_gets_a_hint`, `config_one_folder_below_devcontainer_is_quiet`, `root_config_files_are_quiet`, `windows_separators_are_normalised`, `misplaced_config_gets_only_the_structure_hint` (replaces the property expectation of `non_devcontainer_json_is_ignored`: `other/devcontainer.json` with a typo yields exactly `[folder-structure]`; `package.json` stays ignored), `folder_structure_is_quiet_for_library_or_dynamic_name` (`devcontainer.json.bak`, `my-devcontainer.json`), `moving_the_file_clears_the_hint` (`fixed_file_publishes_empty_list`), `folder_structure_can_be_switched_off`.
  - Completion: `suggests_missing_top_level_properties` (already-present keys excluded), `suggests_enum_values_for_shutdown_action`, `suggests_booleans_for_privileged`, `completion_is_quiet_inside_nested_objects_and_arrays` (`features`, `customizations`, `forwardPorts`), `completion_is_quiet_inside_comments_and_for_other_files` (`package.json`, `tsconfig.json`, `.vscode/settings.json`), `completion_survives_an_unterminated_string_at_the_caret`, `completion_reads_jsonc_comments_and_trailing_commas`, `completion_ranges_use_utf16_columns_with_multibyte_text` (replace range), `completion_can_be_switched_off`, `completion_is_fast_on_a_256_kib_file` (returns empty over `MAX_BYTES` without scanning).
  - Unchanged: the existing `crlf_files_report_correct_lines` and `ranges_use_utf16_columns_with_multibyte_text` extended to the structure finding (it reports at line 0 for a file with a BOM too).
- [ ] **Steps 2 to 5:** shared shape. Doc notes must flag that the `jsonc` language id is unverified. Commit `feat(devcontainer): folder structure hint and property completion`.

---

## flyway

### Task 7: Run configurations for `flyway.toml` and `flyway.conf` (row 6, producer part)

**Strongest reviewer:** the manifest fixes a program and its arguments, and `migrate` writes to a database.

**Files:**
- Modify: `plugins/flyway/plugin.toml`, `plugins/flyway/src/lib.rs` (a manifest test)

**Interfaces:**
- Consumes: the `[[contributes.runDetectors]]` form already used by `plugins/liquibase/plugin.toml` (`id`, `name`, `files`, `command`, `args`).
- Produces: three detectors, each `files = ["flyway.toml", "flyway.conf"]`, `command = "flyway"`: `info` (`args = ["info"]`, name "Flyway: info"), `validate` (`["validate"]`, "Flyway: validate"), `migrate` (`["migrate"]`, "Flyway: migrate (writes to the database)"). No `clean`, no `repair`, no `mvn`. No capability change (liquibase declares detectors with no process capability); `openidea-plugin check` must accept the manifest.

- [ ] **Step 1: Write the failing tests** (reading `include_str!("../plugin.toml")`, as `liquibase` and `laravel` do):
  - `run_detectors_cover_info_validate_and_migrate_only` (exact ids and args).
  - `run_detectors_never_include_clean_or_repair` (no arg equals `clean`, `repair`, `undo`).
  - `run_detectors_match_both_config_file_names_at_the_root` (`files` equals `["flyway.toml","flyway.conf"]`, no path separator, no glob).
  - `run_detectors_use_the_flyway_program_only` (`command == "flyway"`, args contain no `-url`, `-password`, `-user`, `-environment` or `${`): secrets stay in the config file.
  - `migrate_detector_is_labelled_as_writing`.
- [ ] **Steps 2 to 5:** shared shape. Doc notes: detectors ignore the `runner` and `arguments` settings; a config in `conf/` is not at the project root and is not detected. Commit `feat(flyway): run configurations for flyway.toml and flyway.conf`.

---

## jpa-assist

### Task 8: Kotlin entity checks (rows 1, 3, 7; row 2 is a ruling)

**Files:**
- Modify: `plugins/jpa-assist/src/checks.rs`, `src/lib.rs`, `settings.schema.json`

**Interfaces:**
- Consumes: `class_members(masked, kotlin)`, `without_annotations`, `annotations`, `words`, `has_word`, `inspect_entity`, `Finding::new`, `setting(name)`.
- Produces: codes `jpa-immutable-collection`, `jpa-val-attribute`; `pub fn inspect_kotlin_entity(text: &str) -> Vec<Finding>` (empty unless `@Entity` in a Kotlin file); the existing `jpa-data-class` finding is skipped when the class body declares `equals`, `hashCode` and `toString` all three (`override fun`). Setting `jpaAssist.checkKotlinEntities` (default on) gates `jpa-immutable-collection`, `jpa-val-attribute` and `jpa-data-class` (filter by code in `findings`).

Rules (from the JPA 3.1 specification §2.1 and the Kotlin collections docs; own messages):
- `jpa-val-attribute`: a persistent attribute declared with `val` in an `@Entity` class (constructor property or body). Quiet when it carries `@Id`, `@EmbeddedId`, `@Transient`, when the class is `@Immutable` (Hibernate), and for `@Embeddable`/`@MappedSuperclass` classes.
- `jpa-immutable-collection`: an attribute with `@OneToMany`, `@ManyToMany` or `@ElementCollection` whose declared type's simple name is `List`, `Set`, `Map`, `Collection` or `Iterable` (not `Mutable*`, not `ArrayList`, `HashSet`, `LinkedHashSet`).

- [ ] **Step 1: Write the failing tests** (use the file's `problems(host, path, text)` helper):
  - `val_attribute_in_an_entity_warns_on_the_name` (highlight is `val`), `val_in_constructor_and_in_body_both_warn`, `val_id_is_quiet`, `val_transient_is_quiet`, `immutable_hibernate_entity_is_quiet`, `val_in_embeddable_is_quiet`, `var_attribute_is_quiet`, `java_entity_is_ignored`, `val_attribute_is_quiet_for_library_or_dynamic_name` (a `val` computed property with a getter body or `by lazy` is not an attribute).
  - `immutable_list_on_one_to_many_warns`, `mutable_list_on_one_to_many_is_quiet`, `arraylist_on_many_to_many_is_quiet`, `immutable_list_without_a_relation_annotation_is_quiet`, `element_collection_set_warns`.
  - `data_class_entity_warns_without_the_three_overrides`, `data_class_entity_with_equals_hashcode_and_tostring_is_quiet`, `data_class_with_only_equals_still_warns`.
  - `kotlin_checks_can_be_switched_off`, `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.
- [ ] **Steps 2 to 5:** shared shape, commit `feat(jpa-assist): Kotlin entity checks for val attributes, immutable collections and data classes`.

### Task 9: Class signature, listener warnings and listener-method action (rows 10, 12, 22)

**Files:**
- Modify: `plugins/jpa-assist/src/checks.rs`, `src/lib.rs`, `src/generate.rs`, `settings.schema.json`

**Interfaces:**
- Consumes: `inspect_entity`, `parse_class`, `Class.body_end`, `Class.pos`, `import_edit`, `JpaAssist::load`, `JpaAssist::class`, `action(doc, title, kind, preferred, edits)`, `CALLBACKS`.
- Produces: codes `jpa-entity-nested`, `jpa-entity-multiple-roles`, `jpa-entity-not-a-class`, `jpa-listener-without-callbacks`; `pub fn inspect_class_signature(text: &str, kotlin: bool) -> Vec<Finding>`; `pub fn listener_references(text: &str) -> Vec<(String, usize, usize)>` (simple class name and byte range of each `X.class` inside `@EntityListeners(...)`); `pub fn has_callbacks(text: &str) -> bool`; `pub fn callback_method(class: &Class, annotation: &str) -> Option<Edit>` in `generate.rs` (inserts before `class.body_end`, plus the `jakarta.persistence.<Annotation>` import through `import_edit`; `javax` when the file imports `javax.persistence`). Settings `jpaAssist.checkClassSignature` and `jpaAssist.checkListenerUsage` (default on).

Rules (JPA 3.1 §2.1 and §3.5):
- `jpa-entity-nested`: an `@Entity` class declared inside another class body that is non-static (Java: no `static`; Kotlin: marked `inner`). Static nested and Kotlin nested classes stay quiet.
- `jpa-entity-multiple-roles`: `@Entity` together with `@Embeddable` or `@MappedSuperclass` on the same declaration; reported on the second annotation.
- `jpa-entity-not-a-class`: `@Entity` on an `interface`, `enum` or `annotation class`; the declaration keyword is the first of `class|interface|enum` after the annotations (the current `words(.., "class")` can hit a later class).
- `jpa-listener-without-callbacks`: a name inside `@EntityListeners(...)` whose source is found by simple name exactly once in the project, has no `extends`/`: Parent` superclass, and has none of the seven callback annotations. Reported on the `X` of `X.class`. Unresolved, ambiguous (two files) and subclassed listeners are quiet.
- Action: on the entity class line, one `source` code action per callback not already declared (`Add @PrePersist callback`, `PostPersist`, `PreUpdate`, `PostUpdate`, `PreRemove`, `PostRemove`, `PostLoad`), inserting `@X void x() {}` (Kotlin: `@X fun x() {}`) at the end of the body; existing file only, no snippet.

- [ ] **Step 1: Write the failing tests:**
  - `inner_entity_class_warns`, `static_nested_entity_is_quiet`, `kotlin_nested_entity_is_quiet_and_inner_warns`, `entity_with_embeddable_warns_on_the_second_annotation`, `entity_with_mapped_superclass_warns`, `entity_on_interface_warns`, `entity_on_enum_warns`, `entity_signature_is_quiet_for_library_or_dynamic_name` (a plain entity, an entity whose file also declares a later unrelated `class`, a non-JPA class named `Entity`).
  - `listener_without_callbacks_warns_on_the_class_reference` (listener file added with `host.add_file`), `listener_with_a_callback_is_quiet`, `unresolved_listener_is_quiet`, `listener_found_in_two_files_is_quiet`, `listener_with_a_superclass_is_quiet`, `listener_in_kotlin_entity_is_read`, `fixing_the_listener_clears_the_warning_on_the_entity_after_reopen` (`fixed_file_publishes_empty_list`).
  - `class_line_offers_one_action_per_missing_callback`, `declared_callbacks_are_not_offered_again`, `callback_action_adds_the_import_once`, `callback_action_edits_only_the_open_file` (a single path in `edits`), `callback_action_uses_javax_when_the_file_does_too`, `callback_action_is_quiet_off_the_class_line_and_for_non_entities`, `callback_action_for_kotlin_uses_fun`.
  - Cross-cutting: `class_signature_group_can_be_switched_off`, `listener_usage_group_can_be_switched_off`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`, `missing_file_is_treated_as_absent`.
- [ ] **Steps 2 to 5:** shared shape, commit `feat(jpa-assist): class signature and listener checks, callback method actions`.

---

## Self-review

- **Coverage:** kube-explorer 9 rows: deprecated (1), Chart keys and values (1), trim (1), Helm update, All Namespaces, kubectl hint, CSR are tasks 1 to 3; Refresh is a ruling. openapi 5: candidate and endpoints (Task 4), Swagger templates, live templates, reload (Task 5). jpa-assist 7: rows 1, 3, 7 (Task 8), 10, 12, 22 (Task 9), row 2 ruled. devcontainer 2 (Task 6). flyway 1 (Task 7).
- **Step scan:** every task names files, signatures, codes, rules and test names; no body is written out.
- **Type consistency:** `Finding`/`Diagnostic` types are each plugin's own; codes are constants listed per task; `Doc.top_entries` is produced in Task 1 and used only there.
- **Review Focus:** every line maps to tests in Tasks 1 to 9 (option injection: 2, 3; wrong namespace: 3; plain files: 1, 4, 6; mock vs host: 4, 6, 9; stale diagnostics: all).
- **Proportion:** nine tasks, tests as names and assertions, no implementation bodies.

## Ledger (fill during execution)

### Rulings

| Row | Ruling | Confirmed at execution |
|---|---|---|
| kube-explorer Refresh | already done | |
| kube-explorer Chart.yaml (requirements.yaml) | narrowed | |
| kube-explorer excessive trim | narrowed rule | |
| jpa-assist row 2 | subsumed by row 3 | |
| jpa-assist row 10 | part only | |
| jpa-assist row 12 | narrowed | |
| devcontainer completion | `jsonc` id unverified | |
| devcontainer folder structure | path-only | |
| flyway row 6 | producer only | |
| openapi reload | wiring-only test, manual check | |

### Per-task status

| Task | Plugin | Status | Commit | Doc notes written |
|---|---|---|---|---|
| 1 | kube-explorer manifest checks | | | |
| 2 | kube-explorer commands, CSR | | | |
| 3 | kube-explorer all namespaces | | | |
| 4 | openapi candidate, JSON endpoints | | | |
| 5 | openapi templates, skeletons, reload | | | |
| 6 | devcontainer structure, completion | | | |
| 7 | flyway run detectors | | | |
| 8 | jpa-assist Kotlin checks | | | |
| 9 | jpa-assist class signature, listeners, action | | | |
