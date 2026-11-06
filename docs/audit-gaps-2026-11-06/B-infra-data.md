# Group B: infrastructure plugins (gap audit)

Class key: F feasible, F* feasible with stated risk, B blocked (Track U item or other API fact). Counts are per original feature (grouped rows give their size). "LIM" = LIMITATIONS.md.

## Kubernetes (10485) -> kube-explorer

Original: 16 inspections, 8 distinct intentions (header says 16; the list repeats 8), 65 actions, 15 file templates, 16 live templates, 9 extension-point rows = 129. Done 40 / Partial 12 / Missing 77. Gaps (89): F 9, F* 54, B 26.

Evidence for what we have: `checks.rs` codes `deprecated-api-version`, `duplicate-env-name`, `missing-name`, `missing-selector`, `helm-whitespace-trim`, `chart-missing-key`; code actions in `lib.rs:1078` (schema modeline, List <-> documents); commands in `plugin.toml` (apply, dry-run, diff, delete-manifest, kustomize-*, helm-*); 15 `fileTemplates`; tree buttons in `lib.rs:544-563`; `kubeconfig`, `debugImage`, `allowRevealSecrets` settings.

### Inspections (16: Done 2, Partial 4, Missing 10; gaps F 4, F* 10)

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Unknown keys, Missing keys, Unknown values (3) | Validate a manifest against the Kubernetes schema of its kind | Partial: the "Add Kubernetes schema" code action writes a yaml-language-server modeline, so the language server validates; no own diagnostics, one-document built-in kinds only (`README`, LIM 667) | F* | Bundle the JSON schemas and validate in the plugin via `publish_diagnostics`; risk: wasm size, schema version drift, false positives on CRDs; the schema-association route has no API |
| 2 | Non-editable resources, Non-editable keys (2) | Flag read-only kinds and read-only fields (status, etc.) | Missing | F* | Needs the schema's read-only metadata bundled; false-positive risk on server-managed fields |
| 3 | Unknown resources (1) | Flag a `kind`/`apiVersion` pair that no schema knows | Missing | F* | Without cluster/CRD knowledge it flags every custom resource; false-positive risk |
| 4 | Deprecated keys, Deprecated values (2) | Flag deprecated fields and field values | Missing | F* | Needs a bundled deprecation table per version |
| 5 | Deprecated resources (1) | Flag deprecated resource types | Partial: `deprecated-api-version` covers apiVersions removed in v1.16/1.22/1.25 with the replacement | F | Extend the static table; text check |
| 6 | Chart.yaml unknown values (1) | Invalid values in Chart.yaml/requirements.yaml | Missing | F | Small fixed Chart.yaml schema; file read + `publish_diagnostics` |
| 7 | Chart.yaml unknown keys (1) | Unrecognized Chart.yaml keys | Missing | F | Same as 6 |
| 8 | Helm unknown values (1) | `.Values.x` references not declared in values.yaml | Missing | F* | Regex over template text vs. values.yaml keys; risk: scope changes (`range`, `with`), subcharts, `global`, `tpl` |
| 9 | Helm unknown templates (1) | `include "name"` with no `define` | Missing | F* | Regex over `templates/`; risk: dynamic names, library charts |
| 10 | Excessive whitespace trim (1) | Trims `{{- -}}` that swallow non-blank neighbours | Missing (ours checks a different problem: a trim marker without a space) | F | Text rule on `{{ }}` actions, rule is stated in the inventory |

Done: duplicated EnvVar (`duplicate-env-name`), Chart.yaml missing keys (`chart-missing-key`: apiVersion, name, version).

### Intentions (8: Done 3, Missing 5; gaps F 1, F* 3, B 1)

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Helm repo URL <-> alias (2) | Rewrite a chart dependency repository between URL and local alias | Missing | F* | Needs `helm repo list` (`process:helm`, optional) inside a 50 ms code-action budget; risk: timing |
| 2 | Helm update repository (1) | Refresh the local chart index | Missing | F | New command running `helm repo update` (`process:helm`) |
| 3 | Apply CRD (1) | Register a CRD file as an external schema | Missing | B | No schema-association API for the built-in YAML server (LIM 667/1102) |
| 4 | Mark as non-Kubernetes file (1) | Exclude a file from Kubernetes handling | Missing | F* | Marker comment inserted into the file (edits the user's file) or a settings list; ours detects manifests by `apiVersion`+`kind`, so it is rarely needed |

Done: Helm dependency update (command `helm-dependency-update`), List -> multi-document and multi-document -> List (code actions in `convert.rs`).

### Actions (65 by theme: Done 21, Partial 8, Missing 36; gaps F 3, F* 23, B 18)

**1. Cluster connection and kubeconfig (9: Done 4, Partial 1, Missing 4; F* 1, B 4)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Connect Cluster, Set as Current, Context switcher, Add from default dir (4) | Done: contexts from `kubectl config get-contexts`, double-click switches, passed as `--context`; the kubeconfig is never modified (by design) | | |
| 2 | Attach from custom kubeconfigs (1) | Partial: one `kubeconfig` setting | F* | Allow a list of files; risk: merge semantics of `--kubeconfig` |
| 3 | Open kubeconfig in editor (1) | Missing | B | `open_location` refuses paths outside the project (LIM 664) |
| 4 | Paste kubeconfig content (1) | Missing | B | Input dialog (U5) and a file write (U2) |
| 5 | Floating toolbar Add Clusters (1) | Missing | B | No editor floating toolbar in the API |
| 6 | Open Kubernetes settings (1) | Missing | B | No open-settings API listed; unverified |

**2. Namespaces (4: Done 2, Missing 2; F 1, F* 1)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Namespace picker, Namespace switcher (2) | Done: double-click a namespace (`lib.rs:278`, `current_context`) | | |
| 2 | All Namespaces (1) | Missing | F | `-A` on list calls plus an "All" node |
| 3 | Customize Namespaces (1) | Missing | F* | Scalar setting with a comma list; no dialog (U5) |

**3. Resource actions (11: Done 7, Missing 4; F* 1, B 3)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | View YAML, Describe, Delete, Open Console (attach), Run Shell, Forward Ports, Create Secret (7) | Done: buttons YAML, Describe, Delete (confirm twice), Attach, Shell, Port Forward, Create Secret (setting-gated) | | |
| 2 | Edit Secret (1) | Missing (Show Secret and YAML on secrets exist behind `allowRevealSecrets`) | F* | Form plus `kubectl apply`; values visible on screen (no masked field) |
| 3 | Connect to Database (1) | Missing | B | Database APIs |
| 4 | Stop / Restart Port Forwarding (2) | Missing | B | Port forward runs in a terminal; no process handle (LIM 670; streaming processes) |

**4. Logs (14: Partial 2, Missing 12; F* 13, B 1)**

Ours: Logs button opens `kubectl logs -f --tail=500` for a pod, or `--all-pods --all-containers` for a workload, in a terminal (`lib.rs:726-733`). No in-plugin log viewer.

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Open Cluster Logs tab (1) | Partial: per-resource terminal only | B | Cluster-wide live log view needs streaming processes |
| 2 | Select Log Sources (1) | Partial: pod vs. workload target | F* | Source choice inside a snapshot view |
| 3 | Download Log, Download Log (deployment) (2) | Missing | F* | `kubectl logs` in `run_process` shown in an Html view; no file write (U2); output size |
| 4 | Filtered download, Clear filters, Date/time filter, Log fields, Pattern matching, Regex, Match case (7) | Missing | F* | Snapshot viewer with form filters over one `run_process` result; live follow is B (streaming) |
| 5 | Scroll to end, Soft wrap, Clear displayed (3) | Missing (the terminal does these natively) | F* | Buttons of that same Html viewer |

**5. Open-file actions and floating toolbar (9: Done 3, Partial 3, Missing 3; F* 1, B 5)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Apply, Delete, Compare with Cluster (3) | Done: commands `apply`, `delete-manifest`, `diff` (editor context menu for apply) | | |
| 2 | Floating toolbar Apply, Delete, Compare (3) | Partial: same functions through commands and menu; no floating toolbar | B | No editor floating toolbar/decorations |
| 3 | Reload Content From Cluster, plus its floating twin (2) | Missing | F* | Code action that replaces the file text with `kubectl get -o yaml`; risk: kubectl round trip inside the 50 ms budget, no document edit from commands |
| 4 | Always Show Floating Toolbar (1) | Missing | B | As 2 |

**6. Helm (6: Done 3, Missing 3; F* 3)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Dependency update, Lint, Template (3) | Done: commands `helm-dependency-update`, `helm-lint`, `helm-template` (chart found around the open file) | | |
| 2 | Create Chart (1) | Missing | F* | `helm create` through `run_process` (`process:helm`); chart name needs input (U5), so a setting |
| 3 | Configure Helm Parameters (1) | Missing | F* | Settings for release name, values files, `--set`; scalar types only |
| 4 | Generate Dependency (1) | Missing | F* | Code action inserting a `dependencies:` stanza in Chart.yaml; repo/version need input (U5) |

**7. Telepresence (2: Missing 2; F* 1, B 1)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Connect Telepresence (1) | Missing | F* | Optional `process:telepresence` in the manifest and a terminal command |
| 2 | Add tunnel to run configuration (1) | Missing | B | Editing run configurations is not in the API |

**8. Ephemeral containers (3: Done 1, Partial 1, Missing 1; F* 1, B 1)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Attach Busybox (1) | Done: Debug button, `kubectl debug -it`, default `busybox:stable` | | |
| 2 | Attach Custom (1) | Partial: image from the `debugImage` setting, first container targeted | B | Per-use image prompt needs U5 |
| 3 | Edit recents (1) | Missing | F* | Recent images kept in `storage`; edit via settings |

**9. Misc (7: Done 1, Partial 1, Missing 5; F 2, F* 1, B 3)**

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Create Kubernetes Resource (1) | Done: File > New templates | | |
| 2 | Refresh (1) | Partial: Refresh only on the YAML page (`lib.rs:428`); tree reload on context change; no refresh button unverified | F | Add a toolbar button calling `reload` |
| 3 | Configure External Schema (1) | Missing | B | No schema-association API (LIM 667) |
| 4 | Find Usages (1) | Missing | B | U1 |
| 5 | Customize Notifications (1) | Missing | F* | Settings toggles gating `notify` calls |
| 6 | Install kubectl (1) | Missing | B | Needs a binary download and write; `net:` hosts fixed, no file write (U2) |
| 7 | How to install kubectl (1) | Missing | F | Notification with the instruction text |

### File templates (15: Done 14, Missing 1; F 1)

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | CertificateSigningRequest | Missing: 14 of the 15 are in `plugin.toml` (all but this one) | F | Add `templates/csr.yaml` |

### Live templates (16: Missing 16; F* 16)

| # | Original | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | 15 `k*` abbreviations for resources plus `incl` | Expand an abbreviation into a manifest or Helm include | Missing; the plugin has no completion provider | F* | Completion provider on `yaml` with the abbreviation as label; loads for every YAML file and needs path filtering inside; no live-template contribution exists |

### Other extension points (9 rows, all Missing; F* 2, B 7)

| # | Original feature | Ours | Class | Basis |
|---|---|---|---|---|
| 1 | Helm `.Values`/`include` completion (completion contributors) | Missing | F* | Provider on `yaml`; template syntax in YAML context; 50 ms |
| 2 | Cluster-aware completion (ConfigMap/Secret names, images, labels) | Missing | B | Providers cannot wait for kubectl (LIM 653) |
| 3 | Find usages, rename, references, element descriptions, docs (about 50 registrations) | Missing | B | U1 |
| 4 | Line markers (navigate between manifests) | Missing | F* | Definition/line-marker providers on `yaml`; project scan; false positives on name matches |
| 5 | Inlay hints | Missing | B | No inlay hints |
| 6 | Go-template language, injection, folding, highlighting | Missing | B | Injection and folding control unavailable; syntax for built-in languages is U6 |
| 7 | Editor banner / floating toolbar | Missing | B | No editor decorations |
| 8 | Icons for manifests (iconProvider) | Missing (our icons are tree-only) | B | Content-based file icons not available |
| 9 | YAML structure view | Missing | B | No structure API |

Ours beyond the original: Kustomize render/apply, Show/Create Secret, custom-resource tree by API group, `missing-name`/`missing-selector` checks, StatefulSet template, server dry-run validation.

## Flyway (23740) -> flyway

Original: 18 actions, 6 extension-point rows (no inspections or intentions; the 5 distinct templates, listed as 10 files, back the create actions below and are not counted separately) = 24. Done 1 / Partial 3 / Missing 20. Gaps (23): F 1, F* 6, B 16.

Evidence: `plugin.toml` commands `check`, `copy-next-version`, `info`, `validate`, `migrate`, `repair`, `clean`, view `migrations`; `checks.rs` naming/duplicate checks; completion for SQL placeholders, `flyway.conf`, `flyway.toml`; no `fileTemplates`, `fileIcons` or `runDetectors` in the manifest.

| # | Original feature (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Run Flyway Migrate (action) | Run migrate from the IDE | Done: command `migrate` in a terminal | | |
| 2 | Empty migration, Empty migration dialog (2 actions) | Create a new versioned SQL file with the next version | Partial: "Copy Next Migration Version" puts `V3__` on the clipboard | F* | New-menu `fileTemplates` entry (name typed by the user) plus the existing command; no versioned-name dialog (U5) and the file lands in the folder chosen in the New menu |
| 3 | Java migration, Java callback, SQL callback (3 actions; Java/Kotlin/SQL templates) | Create a migration or callback class/script | Missing | F* | `fileTemplates` for java/kt/sql; `$PACKAGE$` unverified (LIM 1263) |
| 4 | Init migration, Data model snapshot, Migration from diff, Generate DDL by entity, Generate init DDL schema (5 actions) | Generate migration SQL from the database or the entity model | Missing (jpa-assist copies entity DDL to the clipboard) | B | Database/diagram APIs; no entity model in this plugin (U4); file creation U2 |
| 5 | Add, Remove, Move to another migration, Remove and ignore, Restore from ignored (7 actions) | Curate which schema diff goes into which migration | Missing | B | Needs the diff UI over a database model; no database API |
| 6 | Run configuration producer and info providers (3 rows) | Flyway run configurations, parse `info` output | Partial: commands plus the Migrations view (`info` JSON) | F | `runDetectors` on `flyway.toml`/`flyway.conf` with fixed args; liquibase already declares them |
| 7 | Console filter (6 registrations) | Clickable migration file names in Flyway output | Missing | B | Terminal output is not readable by plugins (no streaming) |
| 8 | Line markers (entity <-> migration) | Gutter icons | Missing | B | Needs entity resolution (U4) |
| 9 | File icons | Icon on migration files | Missing | F* | `fileIcons` pattern `V*__*.sql`; unverified for pattern matching on real names |
| 10 | Project view decorator | Marks pending/applied in the tree | Missing | B | No Project-view decorations |
| 11 | SQL resolve extension | Resolve Flyway placeholders in SQL | Missing | B | U1 |

Ours beyond the original: naming scheme, duplicate version and repeatable-description checks, Migrations view with states, clean with confirmation, completion of placeholders and config keys.

## Liquibase (23741) -> liquibase

Original: 2 inspections, 16 actions, 3 template rows (internal XML/YAML, JPA-generated changelogs), 14 extension-point rows = 35. Done 8 / Partial 1 / Missing 26. Gaps (27): F 0, F* 6, B 21.

Evidence: `checks.rs` (`duplicate-changeset`, `unresolved-property`), `plugin.toml` runDetectors status/validate/update-sql/update, templates xml/yaml/sql, view `changesets`, completion for YAML and formatted SQL, definition on `include`.

| # | Original feature (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Duplicate changeset id, Unresolved property (2 inspections) | | Done (`duplicate-changeset`, `unresolved-property`) | | |
| 2 | Run Update, Empty changelog (2 actions) | | Done (run detector `update`; templates) | | |
| 3 | Init changelog, Snapshot, Changelog from diff, Init schema, Diff changes (5 actions) | Generate changelogs from a live database | Missing | B | Database APIs; file creation U2 |
| 4 | Add changelog, Add changeset, Remove from changelog, Move, Remove and ignore, Restore from ignored (7 actions) | Curate generated diff items | Missing | B | Diff UI over a database model |
| 5 | Set context, Set label (2 actions) | Tag diff items | Missing | B | Same as 4 |
| 6 | JPA-generated changelog templates (JSON/SQL/XML/YAML) | Changelog from entities | Missing | B | Entity model (U4) and file creation (U2) |
| 7 | References (about 76 registrations: include, columns, tables, rollback, contexts) | Navigate/complete references | Partial: Go to Definition on `include` only | B | U1 (the file-path completion part is row 8) |
| 8 | Include file path completion | Complete `file:` values | Missing | F* | Provider on `yaml`/`xml` with `find_files`; hidden/ignored skipped by the host |
| 9 | XML changelog completion (DOM description) | Element/attribute completion | Missing (relies on lemminx and the XSD, LIM/README) | F* | Provider on `xml` using the change-type catalog already in `catalog.rs`; risk: 50 ms and large XML |
| 10 | JSON changelogs | Support for JSON format | Missing (README: not supported) | F* | Add `json` language to the provider, as openapi does for JSON |
| 11 | Line markers and run line markers (12) | Gutter run/navigate on changesets | Missing | F* | `line-markers` provider; one target per marker; run icon behaviour unverified |
| 12 | Folding builder (4) | Fold changesets | Missing | B | No folding control |
| 13 | Documentation provider | Hover docs | Missing | B | U1 |
| 14 | Language injector | SQL inside `<sql>` | Missing | B | No injection |
| 15 | Console filter | Links in output | Missing | B | No terminal output access |
| 16 | File icons | Changelog icon | Missing | F* | `fileIcons` patterns by file name (`db.changelog-*`); no content detection |
| 17 | Project view decorator | Tree decoration | Missing | B | No Project-view decorations |
| 18 | Changeset author provider | Default author for new changesets | Missing | F* | Completion of `author` from `git config user.name` (`process:git`) or a setting |

Done on the extension-point side: logical structure -> Changesets view; Go to Definition on `include`.

Ours beyond the original: `incomplete-changeset`, `unknown-change-type`, `unresolved-include`, `missing-header` checks, formatted-SQL completion, `${property}` scan across config files.

## OpenAPI/Swagger (14394) -> openapi

Original: 4 inspections, 1 intention (header says 2, one listed), 17 actions, 5 templates, 6 live templates, 13 extension-point rows = 46. Done 7 / Partial 4 / Missing 35. Gaps (39): F 12, F* 10, B 17.

Evidence: `checks.rs` (`unresolved-ref`, `duplicate-operation-id`, `path-parameter`), providers `completion, definition, code-actions` on `yaml`/`json`, Swagger UI in `web/`, commands `endpoints`, `copy-http-requests`, `preview`, `preview-any`, templates `openapi-yaml`, `openapi-json`, `asyncapi-yaml`.

| # | Original feature (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Unresolved reference YAML/JSON (2 inspections) | | Done (`unresolved-ref`, both formats) | | |
| 2 | Possible specification candidate YAML/JSON (2 inspections) | Flags files that look like spec fragments but lack the root key | Missing (we gate on a top-level `openapi`/`swagger` key) | F | Text heuristic (`paths`, `components`, `$ref`) in `publish_diagnostics` |
| 3 | Extract model declaration (intention) | Move an inline schema to `components` | Missing | F* | Code action editing the open file; risk: indentation/JSON syntax, `$ref` rewrite |
| 4 | Generate HTTP requests (2 actions) | Write requests into an `.http` file | Partial: "Copy Requests for the HTTP Client" copies to the clipboard | B | File creation (U2) |
| 5 | Create OpenAPI specification (action) | | Done (New menu templates) | | |
| 6 | Compare specifications of two files (action) | Semantic diff | Missing | B | Needs a second-file chooser (U5) |
| 7 | Compare with last revision (action) | Diff against VCS | Missing | F* | `git show` via `process:git` (not declared today), rendered in an Html view; semantic diff is our own code |
| 8 | Dump OpenAPI for module (action) | Spec from code | Missing | B | Framework analysis (U4) |
| 9 | Export self-contained YAML/JSON (2 actions) | Inline all `$ref`s | Missing | F* | Resolve refs in the plugin and copy to clipboard; file output U2; cycles |
| 10 | Copy JSON schema meta (action) | Copy schema coordinates at caret | Missing | F* | Command with caret like jpa-assist `copy-entity`; format unknown |
| 11 | Open composed schema, Open schema for injected file (2 actions) | Virtual composed document | Missing | B | No virtual documents; U2 |
| 12 | Switch preview type (action) | Swagger UI <-> Redoc | Missing | F* | Bundle Redoc (MIT) in `web/`; size, frame has no network |
| 13 | Open dev tools (action) | Webview devtools | Missing | B | No webview debug API |
| 14 | Reload specification (action) | Force preview reload | Missing (preview follows edits) | F | Html view button |
| 15 | Auto-scroll sync (action) | Scroll preview with editor | Missing | B | No caret/scroll events |
| 16 | Configure specification sources (action) | Declare which files are specs | Missing (files are detected by root key; `openapi.*` names auto-preview) | F* | Scalar settings with a glob list |
| 17 | Export draft from endpoints panel (action) | | Missing | B | Code-first endpoints (U4) and file creation (U2) |
| 18 | Swagger 2.0 templates, YAML and JSON (2 templates) | | Missing (ours: OpenAPI 3.1 and AsyncAPI) | F | Two more `fileTemplates` |
| 19 | Live templates `opnp`, `swag`, `endp` in YAML and JSON (6) | Skeleton expansion | Missing | F | Extra completion items in the existing `completion` provider, gated to spec files |
| 20 | Keyword/value completion (completion contributors) | Complete keys, enums | Partial: `$ref` completion; YAML gets the schema modeline | F* | Static key tree by indentation; risk: false context in flow style |
| 21 | JSON schema validation of JSON specs | | Missing (modeline is YAML only) | B | No schema association for the built-in JSON server (LIM 1102) |
| 22 | Rename, find usages, references (about 10 registrations) | Components | Missing | B | U1 |
| 23 | Documentation provider | Hover | Missing | B | U1 |
| 24 | Line markers / run line markers on operations (12) | Run request gutter | Missing | F* | `line-markers` with the run icon; behaviour unverified |
| 25 | Code-first endpoints providers (microservices) | Endpoints from Spring etc. | Missing | B | U4 |
| 26 | Structure views YAML/JSON | | Missing | B | No structure API |
| 27 | Try it out in preview | | Missing (frame has no network) | B | Webview network not granted |
| 28 | Remote specification URLs | | Missing | B | `net:` hosts fixed in the manifest |
| 29 | File icons | | Missing | F* | `fileIcons` patterns for `openapi.*` |
| 30 | MCP toolset | | Missing | B | No MCP API |
| 31 | Endpoints panel | Lists endpoints | Partial: YAML files only | F | Reuse `json.rs` scanner for JSON |

Done on the extension-point side: Swagger UI preview with relative `$ref` files.

Ours beyond the original: `duplicate-operation-id` and `path-parameter` checks, AsyncAPI template, `$ref` file-path completion, schema modeline for four spec versions.

## Dev Containers (21962) -> devcontainer

Original: 2 inspections, 1 intention (header says 2, one listed), 9 actions, 5 extension-point rows = 17. Done 3 / Partial 0 / Missing 14. Gaps (14): F 2, F* 4, B 8.

Evidence: `plugin.toml` commands `up`, `rebuild`, `shell`, `stop`, `info`, 7 `fileTemplates`; `checks.rs` `unknown-property`, `features-malformed`.

| # | Original feature (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Validate IDE settings (inspection) | Checks JetBrains settings under `customizations` | Missing | B | Specific to JetBrains settings; no IDE settings model in OpenIDEA |
| 2 | Dev Container folder structure (inspection) | Nested/missing `.devcontainer` parent folders | Missing | F | Path-only check on the opened file; the host skips dot-directories in `find_files`, so no scan (read_file for the parent) |
| 3 | Add changed settings (intention + action, 2) | Copy modified IDE settings into the file | Missing | B | No settings read API |
| 4 | Create Dev Container (action) | | Done (`up`, Rebuild too) | | |
| 5 | New Dev Container configuration, New config file (2 actions) | | Done (New menu: starter plus Node, Python, Java, PHP, Rust, Compose) | | |
| 6 | Connect (action) | Open the IDE inside the container | Missing | B | Remote backend is not available (README) |
| 7 | Create and clone sources (action) | Clone repo into a container | Missing | B | URL input (U5) |
| 8 | Manage backends (action) | | Missing | B | Gateway concept absent |
| 9 | Show recent containers, Dashboard (2 actions) | List and manage containers | Missing | F* | Table view over `docker ps` filtered by the folder label (`process:docker`, used today only for stop); labels unverified |
| 10 | Completion (4 contributors) | Properties, values | Missing | F | `completion` provider on `json`, gated by file name inside the plugin |
| 11 | Feature id completion from the registry | Suggest features | Missing | F* | Fixed `net:` host, cached result because of the 50 ms budget |
| 12 | Documentation provider, references (4) | Hover, links | Missing | B | U1 |
| 13 | Line markers / run line markers (4) | Start container from the gutter | Missing | F* | `line-markers` with run icon; unverified |

Ours beyond the original: `devcontainer up/rebuild/exec/read-configuration/stop` commands, `unknown-property` and `features-malformed` checks.

## JPA (20202) -> jpa-assist

Original: 22 inspections, 1 intention, 8 actions, 8 extension-point rows = 39. Done 3 / Partial 4 / Missing 32. Gaps (36): F 7, F* 12, B 17.

Evidence: `checks.rs` codes `entity-missing-id`, `attribute-type`, `entity-listener-signature`, `jpa-final-entity`, `jpa-data-class`, `jpa-lombok-data`, `jpa-lombok-equals`, `jpa-eager-fetch`, `jpa-eager-to-one`, `jpa-repository-id`, `jpa-derived-query`; commands `copy-ddl`, `copy-ddl-all`, `copy-dto`, `copy-entity`; `structure` view; code actions equals/hashCode/toString; line markers entity -> repository; 4 templates.

### Inspections

| # | Original (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Immutable collection-like property (Kotlin) | `List`/`Set`/`Map` on `@OneToMany`/`@ManyToMany` | Missing | F | Kotlin attribute parsing exists in `model.rs`; text rule |
| 2 | Immutable not-nullable property (Kotlin) | `val` with `@NotNull` | Missing | F | Same |
| 3 | JPA entity with val attributes (Kotlin) | `val` as a persistent attribute | Missing | F | Same |
| 4 | Kotlin JPA compiler plugin missing | Suggest the `kotlin-jpa` plugin | Missing | F* | Read `build.gradle(.kts)`/`pom.xml` via `read_file`; plugin-detection false positives; quick fix edits an existing build file |
| 5 | Redundant default constructor (Kotlin) | Zero-arg constructor when the plugin is applied | Missing | F* | Needs row 4's build detection |
| 6 | Redundant `open` modifier (Kotlin) | `open` entity classes with the plugin | Missing | F* | Same |
| 7 | Data class entity without equals/hashCode/toString | | Partial: `jpa-data-class` warns on every data class entity | F | Skip when the three overrides exist |
| 8 | Persistent attribute signature | Multiple/illegal attribute annotations, missing getter/setter, wrong place | Missing | F* | Text rules per JPA spec; getter/setter pairing and access-type inference risk |
| 9 | Persistent attribute type | Relationship on a wrong type (for example `@OneToOne` on a `Map`) | Partial: `attribute-type` covers `@Temporal`, `@Enumerated`, `@Version` | F* | Needs entity-ness of the target type: project lookup by simple name (no U4), same-name collisions |
| 10 | Persistent object class signature | Non-top-level, `final`, annotated twice, entity and listener at once | Partial: `jpa-final-entity` only | F | Text rules in `checks.rs` |
| 11 | Entity listener problems | No public no-arg constructor, bad callback signatures | Partial: `entity-listener-signature` covers callbacks only (constructor unverified) | F* | Listener class resolved by simple name |
| 12 | Entity listener warnings | Listener class without callbacks | Missing | F | Text rule |
| 13 | Unresolved references in queries | `AttributeOverride`, `MapsId`, `MapKey` names, Criteria API | Missing | F* | Annotation string vs. model for annotations; Criteria API is B (U4) |
| 14 | Unresolved queries and parameters | Unknown named query, unknown parameter | Missing (README: `@Query` not parsed) | F* | Named-query names across the project; parameter names vs. method params; string-literal parsing |
| 15 | Unresolved entity graph names | `@NamedAttributeNode`, `@NamedEntityGraph` | Missing | F* | Strings vs. attribute model |
| 16 | persistence.xml problems | Class references, value patterns | Missing | F* | XML scan plus class lookup by name; no U4 types |
| 17 | persistence.xml not in facet | | Missing | B | No facet concept |
| 18 | orm.xml database references | | Missing | B | No database APIs |
| 19 | Unresolved database references in annotations / in XML (2) | | Missing | B | Datasource model needed |
| 20 | Query language checks (JPQL) | Type mismatches, constant conditions | Missing | B | Injection and type resolution (U4) |

Done: persistent entity misses primary key (`entity-missing-id`, own or inherited).

### Intention and actions

| # | Original (kind) | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 21 | Add inverse attribute (intention) | Make a relationship bidirectional | Missing | F* | Code action editing two existing files; imports and generic types by text |
| 22 | Entity listener methods (action) | Generate callback methods | Missing (ours generates equals/hashCode/toString) | F | Code action on the class line |
| 23 | ER diagram (action) | | Missing | B | No diagram API |
| 24 | JPA console: open, execute, close, terminate (4 actions) | JPQL console | Missing | B | Database/console APIs and streaming |
| 25 | Assign data sources (action) | | Missing | B | Database APIs |
| 26 | Open in database view (action) | | Missing | B | Database APIs |

### Extension points

| # | Original | Ours | Class | Basis |
|---|---|---|---|---|
| 27 | Logical structure / presentation | Done: JPA Structure view | | |
| 28 | Equals/hashCode generator | Done: code actions | | |
| 29 | JPQL language support (injection, completion, references) | Missing | B | Injection |
| 30 | persistence.xml / orm.xml editing support (DOM converters, completion) | Missing | F* | Provider on `xml`; see row 16 |
| 31 | Rename and find usages of attributes | Missing | B | U1 |
| 32 | Datasource and table info detection | Missing | B | Database APIs |
| 33 | MCP toolset and bundled skills | Missing | B | No MCP API |
| 34 | Persistence facet / unit settings | Missing | B | No facet concept |

Ours beyond the original: Lombok `@Data`/`@EqualsAndHashCode`, `EAGER` fetch checks with fixes, Spring Data derived-query checks and completion, repository id type check, Copy DDL/DTO/Entity-from-CREATE-TABLE.

## dependency-audit

No counterpart in this group. Ours beyond the original: OSV.dev vulnerability marks in `pom.xml`, Gradle, `package.json`, `composer.json` and lockfiles, with a quick fix to upgrade.

## Group summary

| Pair | Original features | Done | Partial | Missing | F | F* | B | Gaps |
|---|---|---|---|---|---|---|---|---|
| kubernetes -> kube-explorer | 129 | 40 | 12 | 77 | 9 | 54 | 26 | 89 |
| flyway -> flyway | 24 | 1 | 3 | 20 | 1 | 6 | 16 | 23 |
| liquibase -> liquibase | 35 | 8 | 1 | 26 | 0 | 6 | 21 | 27 |
| openapi -> openapi | 46 | 7 | 4 | 35 | 12 | 10 | 17 | 39 |
| devcontainer -> devcontainer | 17 | 3 | 0 | 14 | 2 | 4 | 8 | 14 |
| jpa -> jpa-assist | 39 | 3 | 4 | 32 | 7 | 12 | 17 | 36 |
| **Total** | 290 | 62 | 24 | 204 | 31 | 92 | 105 | 228 |

Notes: the Kubernetes live templates (16 F*) are one mechanism and dominate its F* count; excluding them F* is 38. Flyway template files are counted once under the create actions. Counts follow the original's own listing; extension-point rows are my grouping.
