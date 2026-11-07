# Re-audit v2, group B (infra and data), against HEAD ac0edf6 and plugin API 1.2

Method: every row of [B-infra-data.md](B-infra-data.md) re-checked in `plugins/<name>/` by grep and reading code; READMEs and change-notes were only compared, not trusted. Status: Done / Partial / Missing (code exists and is wired, mock-tested only; nothing here ran in the IDE). Class for what is not Done: F feasible, F* feasible with risk, B blocked, under API 1.2 (`pick`/`pick_many`/`input`/`confirm`, `set_context`, `selected_paths`, `viewItem.context`, `reveal_view`, `documentIndexers`).

Two API facts that move classes and are not in [reclass-B-infra-data.md](reclass-B-infra-data.md):
- `editor.write` (`TextEditor::apply_edits`) lets a command edit the open file with no 50 ms budget. jpa-assist already uses it (`lib.rs:154`). `api-facts.md` ("no document edit from commands") is out of date.
- A plugin can write files through an optional `process:<tool>` (kube-explorer writes with `tee` at `lib.rs:431`, then `open_location`; `helm create` makes a chart). So U2 is a capability and approval cost, not a hard block, for rows that only need file output inside the project. I class those F*.
Units: counts follow the weights of the old tables (a row "(3)" counts 3). Devcontainer: the old header says 17 but its tables sum to 16; I count 16.

## Kubernetes -> kube-explorer (129 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| K-I1a | Unknown keys (1) | Partial | `kinds.rs` `UNKNOWN_KEY` over 29 kinds; quiet for CRDs, Helm files, patches | F* | more kinds, CRD silence is by design |
| K-I1b | Missing keys (1) | Missing | only our `missing-name`, `missing-selector` (`checks.rs:211,221`); no required-key table | F* | needs required fields per kind |
| K-I1c | Unknown values (1) | Missing | no enum or type check in `kinds.rs` | F* | |
| K-I2 | Non-editable resources, keys (2) | Partial | `READ_ONLY_FIELD` (metadata, status fields) only; no read-only kind check | F* | |
| K-I3 | Unknown resources (1) | Partial | `UNKNOWN_KIND` only inside `COMPLETE_GROUPS` (`kinds.rs:143`) | F* | |
| K-I4 | Deprecated keys, values (2) | Partial | `DEPRECATED_SPEC`/`DEPRECATED_ANNOTATIONS` (`kinds.rs:156,162`), small table; no value check | F* | |
| K-I5 | Deprecated resources (1) | Done | `DEPRECATED` table, 24 rows to v1.27 (`checks.rs:~120`) | | old: Partial |
| K-I6-10 | Chart.yaml values, keys; Helm unknown values, templates; excessive trim (5) | Done | `chart_schema` (`checks.rs:278`); `helm.rs:224` `helm-unknown-value`, `helm-unknown-include` (quiet on `tpl`, `range`, subcharts); `helm_excessive_trim` (`checks.rs:333`) | | old: Missing |
| K-I-d | Duplicate env, Chart missing keys (2) | Done | `duplicate-env-name`, `chart-missing-key` | | |
| K-N1 | Helm repo URL <-> alias (2) | Done | `chart_actions.rs:169`; aliases from `helm-repo-cache` command (`:36`), never from the provider | | old: Missing |
| K-N2 | Helm update repository (1) | Done | command `helm-repo-update` (`lib.rs:957`) | | old: Missing |
| K-N3 | Apply CRD (1) | Missing | nothing | B | no schema-association API (LIM 667/1102) |
| K-N4 | Mark non-Kubernetes file (1) | Done | setting `nonKubernetesGlobs`, `config.rs:72`, used in `lib.rs:1137` | | settings glob, not a marker comment |
| K-N-d | Helm dep update, List <-> documents (3) | Done | `helm-dependency-update`; `convert.rs:15` | | |
| K-A1.1 | Connect, current, switcher, default dir (4) | Done | `lib.rs:730` Activate on `ctx/`; `--context` | | |
| K-A1.2 | Custom kubeconfigs (1) | Done | `config.rs:21,38` comma list + `choose-kubeconfig` (`host::pick`) | | old: Partial. Chosen file, no merge |
| K-A1.3-6 | Open kubeconfig, paste kubeconfig, floating Add, open settings (4) | Missing | | B | `open_location` limited to the project; file write outside it; no editor toolbar or open-settings API |
| K-A2 | Namespace picker, switcher, All, Customize (4) | Done | `lib.rs:149` `all_namespaces` (`-A`), `prompts.rs:136` `pick_many` | | old: 2 Missing |
| K-A3.1 | View YAML, Describe, Delete, Console, Shell, PF, Create Secret (7) | Done | buttons + `resource-*` commands, `lib.rs:1240` | | |
| K-A3.2 | Edit Secret (1) | Done | `secrets.rs:298,345` form, `kubectl replace`, resourceVersion kept | | old: Missing. Needs `allowRevealSecrets` |
| K-A3.3/4 | Connect to Database (1), Stop/Restart port forward (2) | Missing | `start-forward` opens a terminal (`lib.rs:825`) | B | database APIs; no process handle |
| K-A4.1 | Open Cluster Logs tab (1) | Partial | per-resource terminal and snapshot only | B | live cluster stream |
| K-A4.2 | Select Log Sources (1) | Done | `prompts.rs:155` `pick_many` pods/containers | | old: Partial |
| K-A4.3 | Download Log, deployment (2) | Partial | `logs.rs` `log-copy` copies shown lines; no file | F* | `tee` route into the project |
| K-A4.4 | Filters (7) | Partial | pattern `log-filter`, `log-regex`, `log-case`, clear (empty input) Done = 4; `log-since` start only; no filtered download, no log fields (`logs.rs:124-131`) | F* | 4 Done, 2 Partial, 1 Missing |
| K-A4.5 | Scroll end, Soft wrap, Clear (3) | Partial | `log-wrap` only (`logs.rs:129`) | F | 1 Done, 2 Missing; buttons on the same Html view |
| K-A5.1 | Apply, Delete, Compare (3) | Done | `apply`, `delete-manifest`, `diff` | | |
| K-A5.2/4 | Floating toolbar, Always show (4) | Partial | functions via commands/menu | B | no editor decorations |
| K-A5.3 | Reload from cluster (2) | Missing | README admits not built | F | a command with `kubectl get -o yaml` and `apply_edits` has no 50 ms limit; README rationale is stale |
| K-A6 | Helm dep/lint/template, Create Chart, Parameters, Generate Dependency (6) | Done | `prompts.rs:60,75` (`helm create`, `--set`/values), `chart_actions.rs:109` | | old: 3 Missing |
| K-A7.1 | Telepresence connect (1) | Done | `config.rs:105`, commands connect/quit | | old: Missing |
| K-A7.2 | Tunnel to run config (1) | Missing | | B | run configs not editable |
| K-A8 | Busybox, Custom, Edit recents (3) | Done | `prompts.rs:96` `input` with recents, `:114` `pick_many` | | old: 1 Done |
| K-A9.1/2 | Create resource, Refresh, Notifications (3) | Done | File > New; `lib.rs:612,781`; `notifySuccess` (`config.rs:32`) | | old: Refresh Partial |
| K-A9.3/4/6 | External schema, Find usages, Install kubectl (3) | Missing | | B | LIM 667, U1, U2 plus binary download |
| K-A9.7 | How to install kubectl (1) | Done | `how-to-install-kubectl` | | old: Missing |
| K-FT | File templates (15) | Done | 15 `fileTemplates`, `templates/csr.yaml` | | old: 14 |
| K-LT | Live templates (16) | Done | `complete.rs:11-26` 15 `k*` items, `:154` `incl` | | old: Missing. Only in an empty document |
| K-X1 | Helm `.Values`/`include` completion (1) | Done | `complete.rs:89,119` | | old: Missing |
| K-X2, X5-9 | Cluster-aware completion, inlay, Go-template, banner, icons, structure (6) | Missing | | B | LIM 653; no inlay, injection, decorations, structure API |
| K-X3 | Find usages, rename, references, docs (1) | Partial | `indexer.rs:77`: ConfigMap, Secret, Service, claim, service account to `metadata.name`; Go to Declaration only | B | U1 for the rest |
| K-X4 | Line markers (1) | Done | indexer `markers` (`indexer.rs:~148`), `kube-users:` keys | | old: Missing. No `line-markers` provider; README says "open YAML files" but the index is project-wide |

Kubernetes totals: Done 88, Partial 15, Missing 26 (old 40 / 12 / 77). Gaps 41: F 3, F* 14, B 24 (old 9 / 54 / 26).

## Flyway -> flyway (24 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| FW1 | Run Migrate (1) | Done | command `migrate` | | |
| FW2 | Empty migration, with dialog (2) | Partial | template `migration-sql` + `copy-next-version` (`lib.rs:107`, clipboard) | F* | `input` description, then write `V<n>__x.sql` with optional `process:tee`; otherwise U2 |
| FW3 | Java/Kotlin migration, Java/SQL callback (3) | Done | `plugin.toml` `migration-java/-kotlin`, `callback-java/-sql`; templates carry no `package` line | | old: Missing. `$NAME$` unverified (README) |
| FW4/5/7/10 | Generators (5), curate diff items (7), console filter (1), Project view decorator (1) | Missing | | B | database/diagram APIs, no terminal output, no decorations |
| FW6 | Run config producer and info (3 rows, count 1) | Done | `runDetectors` info, validate, migrate; Migrations view | | old: Partial |
| FW8 | Line markers entity <-> migration (1) | Done | `indexer.rs` `fw:table:`, `fw:entity:`, markers both ways (`:132-150`) | | old: Missing. Text rules, `@Table`/snake_case |
| FW9 | File icons (1) | Done | `fileIcons` `V*__*.sql`, `U*__*.sql`, `R__*.sql` | | old: Missing. Pattern match unverified |
| FW11 | SQL resolve of placeholders (1) | Done | `indexer.rs:81-93`, `fw:ph:` to conf/toml entries | | old: Missing |

Flyway totals: Done 8, Partial 2, Missing 14 (old 1 / 3 / 20). Gaps 16: F 0, F* 2, B 14.

## Liquibase -> liquibase (35 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| LQ1 | Duplicate changeset id, Unresolved property (2) | Done | `checks.rs:49,85` | | |
| LQ2 | Run Update, Empty changelog (2 + 2 templates) | Done | `runDetectors` update; `templates/changelog.*` | | |
| LQ3-6 | Generators (5), curate (7), set context/label (2), JPA-generated templates (1) | Missing | | B | database APIs; entity model U4 |
| LQ7 | References (1) | Partial | `indexer.rs`: `lb:cs:`, `lb:file:`, `lb:table:`, rollback `changeSetId`; Go to Declaration only | B | columns, contexts, find usages, rename are U1 |
| LQ8 | Include path completion (1) | Done | `lib.rs:453` `find_files`, `complete.rs:153` | | old: Missing. Skips hidden/ignored dirs |
| LQ9 | XML element/attribute completion (1) | Done | `complete.rs:228`, off when `schemaLocation` is set (`:230`) | | old: Missing |
| LQ10 | JSON changelogs (1) | Done | `changelog.rs:81`, `json.rs`; no completion for JSON | | old: Missing |
| LQ11 | Line markers, run markers (1) | Partial | `lib.rs:521` `navigate` marker on each include; no run action | B | a marker can only open a location |
| LQ12/13/14/15/17 | Folding, docs, injector, console, decorator (5) | Missing | | B | |
| LQ16 | File icons (1) | Done | `fileIcons` `db.changelog-*` | | old: Missing |
| LQ18 | Author provider (1) | Done | `lib.rs:439`, `complete.rs:184`, setting or `git config user.name` | | old: Missing |
| LQ-d | Logical structure, Go to include (2) | Done | view `changesets`; `lib.rs:498` | | |

Liquibase totals: Done 13, Partial 2, Missing 20 (old 8 / 1 / 26). Gaps 22: F 0, F* 0, B 22.

## OpenAPI -> openapi (46 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| OA1 | Unresolved reference (2) | Done | `checks.rs:118` | | |
| OA2 | Spec candidate (2) | Done | `checks.rs:41` `missing-openapi-root` (Info), `lib.rs:153` | | old: Missing |
| OA3 | Extract model (1) | Partial | `lib.rs:622` via `refs::extract_schema`; code action returns early for JSON (`lib.rs:620`) | F* | JSON edits |
| OA4 | Generate HTTP requests (2) | Partial | `copy-http-requests` to clipboard | F* | `tee` route into an `.http` file |
| OA5 | Create specification (1) | Done | templates | | |
| OA6 | Compare two files (1) | Done | `lib.rs:211`, `diff.rs`, `pick` | | old: Missing |
| OA7 | Compare with last revision (1) | Done | `lib.rs:240` `git show HEAD:` (optional `process:git`) | | old: Missing |
| OA8/11/13/15/17 | Dump from module, composed schema (2), dev tools, auto-scroll, export draft (6) | Missing | | B | U4, no virtual documents, no webview debug or scroll events |
| OA9 | Export self-contained (2) | Partial | `lib.rs:268`, `refs.rs:355`; YAML only, clipboard only | F* | JSON; file output via `tee` |
| OA10 | Copy JSON schema meta (1) | Partial | `copy-reference` copies a `$ref` pointer; original format unknown | F* | may not be the same output |
| OA12 | Switch preview (1) | Done | `web/main.js:21,31`, `index.html:19` Redoc 2.5.4 | | old: Missing |
| OA14 | Reload (1) | Done | `index.html:18` button | | old: Missing |
| OA16 | Configure sources (1) | Partial | `specGlobs` (`lib.rs:405`) feeds completion and definition only; preview patterns static (`plugin.toml:62`); problems and endpoints still need a root key (README) | B | custom-editor patterns are fixed in the manifest |
| OA18 | Swagger 2.0 templates (2) | Done | `swagger-yaml`, `swagger-json` | | old: Missing |
| OA19 | Live templates `opnp`, `swag`, `endp` (6) | Partial | `lib.rs:429` 3 snippet items (`openapi-path`, `openapi-operation`, `swagger2-root`), names differ | F | rename and add a root item |
| OA20 | Keyword/value completion (1) | Partial | `keys.rs:285` YAML; JSON gets skeletons only (`lib.rs:472`) | F* | |
| OA21/23/25-28/30 | JSON schema validation, documentation, code-first, structure, try it out, remote URLs, MCP (7) | Missing | | B | LIM 1102, U1, U4, no structure/network/MCP API |
| OA22 | Rename, usages, references (1) | Partial | `$ref` Go to Definition only (`lib.rs:568`) | B | U1 |
| OA24 | Line markers on operations (1) | Partial | `lib.rs:603` navigate to the first local schema; no run | B | run needs an action |
| OA29 | File icons (1) | Done | `fileIcons` `openapi` | | old: Missing |
| OA31 | Endpoints panel (1) | Done | `lib.rs:656` scans yaml/yml/json | | old: Partial |
| OA-p | Swagger UI preview (1) | Done | `customEditors` preview, preview-any | | |

OpenAPI totals: Done 17, Partial 16, Missing 13 (old 7 / 4 / 35). Gaps 29: F 6, F* 7, B 16.

## Dev Containers -> devcontainer (16 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| DC1/3/6/8/12/13 | IDE settings check, add changed settings (2), Connect, backends, docs and references, start from gutter (7) | Missing | | B | JetBrains settings model, no remote backend, U1, marker only opens a location |
| DC2 | Folder structure (1) | Done | `checks.rs:57`, wired `lib.rs:133` | | old: Missing |
| DC4/5 | Create Dev Container, New configuration (3) | Done | `up`, `rebuild` (`confirm`, `lib.rs:73`); 7 `fileTemplates` | | |
| DC7 | Create and clone (1) | Done | `lib.rs:245` `input`, `git clone`, `devcontainer up` | | old: Missing. Needs optional `process:git` |
| DC9 | Recent containers, Dashboard (2) | Partial | `dashboard.rs:29` lists running containers of this folder, Open Shell and Stop | F | `docker ps -a` for stopped ones |
| DC10 | Completion (1) | Done | `lib.rs:308` properties and values, `jsonc` unverified | | old: Missing |
| DC11 | Feature id completion (1) | Done | `features.rs`, cache from `refresh-features` | | old: Missing. Empty until the command ran and the host is granted |

Devcontainer totals: Done 7, Partial 2, Missing 7 (old 3 / 0 / 13 on the same 16). Gaps 9: F 2, F* 0, B 7.

## JPA -> jpa-assist (39 units)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| JPA1-3 | Immutable collection, immutable not-null, val attributes (3) | Done | `checks.rs:215-250` (`jpa-val-attribute` flags every Kotlin `val`, so row 2 is inside it) | | old: Missing |
| JPA4-6 | Kotlin plugin missing, redundant constructor, redundant open (3) | Done | `checks.rs:476` `inspect_kotlin_build`, fix at `lib.rs:1052` | | old: Missing |
| JPA7 | Data class without overrides (1) | Done | `checks.rs:129` `own_equality` | | old: Partial |
| JPA8 | Attribute signature (1) | Partial | `inspect_attribute_signatures` (`checks.rs:368`): two relations, two id kinds, mixed access; no missing getter/setter | F* | access-type inference |
| JPA9 | Attribute type (1) | Partial | `attribute-type`, `jpa-relationship-type` (`checks.rs:396-412`): basic or collection mismatch only | F* | no target-is-entity check |
| JPA10 | Class signature (1) | Done | `checks.rs:806`: final, nested, not a class, several roles | | old: Partial |
| JPA11/12 | Listener problems, warnings (2) | Done | `jpa-listener-constructor`, `entity-listener-signature`, `jpa-listener-without-callbacks` | | old: Missing |
| JPA13 | Unresolved references in queries (1) | Partial | `checks.rs:615` AttributeOverride, MapsId, MapKey; Criteria API not | B | U4 |
| JPA14 | Unresolved queries, parameters (1) | Done | `checks.rs:641,766`; quiet with `orm.xml` | | old: Missing. README line "`@Query` not parsed" is gone |
| JPA15 | Entity graph names (1) | Partial | only `@NamedAttributeNode` (`checks.rs:602`); `@NamedEntityGraph`/`@EntityGraph` names unchecked | F | text rule. Backlog marks JPA-3 DONE |
| JPA16 | persistence.xml problems (1) | Done | `xml.rs:~160-250` unit name, provider, class, duplicate, not in project | | old: Missing |
| JPA17-20, 23-26, 29, 31-34 | Facet, orm.xml and annotation DB refs, JPQL, ER diagram, console (4), data sources, database view, rename, datasource, MCP (17 units) | Missing | | B | facet, database, diagram, injection, U1 |
| JPA21 | Add inverse attribute (1) | Done | `lib.rs:917` `inverse_actions` | | old: Missing |
| JPA22 | Entity listener methods (1) | Done | `lib.rs:1089` "Add @X callback" | | old: Missing |
| JPA27/28/pk | Structure view, equals/hashCode, entity without id (3) | Done | view `structure`, code actions, `entity-missing-id` | | |
| JPA30 | persistence.xml editing support (1) | Partial | `lib.rs:953` `<class>` completion; no property names | F* | |

JPA totals: Done 17, Partial 5, Missing 17 (old 3 / 4 / 32). Gaps 22: F 1, F* 3, B 18. Extras not in the original: `new-entity`, `new-attribute` wizards (`lib.rs:862`, `wizard.rs`).

## Totals

| Pair | Units | Done | Partial | Missing | F | F* | B | Gaps |
|---|---|---|---|---|---|---|---|---|
| kube-explorer | 129 | 88 | 15 | 26 | 3 | 14 | 24 | 41 |
| flyway | 24 | 8 | 2 | 14 | 0 | 2 | 14 | 16 |
| liquibase | 35 | 13 | 2 | 20 | 0 | 0 | 22 | 22 |
| openapi | 46 | 17 | 16 | 13 | 6 | 7 | 16 | 29 |
| devcontainer | 16 | 7 | 2 | 7 | 2 | 0 | 7 | 9 |
| jpa-assist | 39 | 17 | 5 | 17 | 1 | 3 | 18 | 22 |
| **Total** | 289 | 150 | 42 | 97 | 12 | 26 | 101 | 139 |

Old: 290 units, Done 62, Partial 24, Missing 204; gaps 228 = F 31, F* 92, B 105.

## Old Done rows that are not Done, and claims the code does not back

- No old Done row regressed; all 62 re-verified in code.
- Backlog says JPA-3 is DONE ("@NamedEntityGraph names: not verified"). Confirmed partial: only `@NamedAttributeNode` attribute names are checked (JPA15).
- Backlog says OA-2 `specGlobs` covers "declare spec sources". Partial: it feeds completion and definition only; the preview patterns and the problems/endpoints gates ignore it (README says so; the change-note wording "the specGlobs setting" invites more).
- kube-explorer README says markers match "across all open YAML files". The indexer is project-wide (`documentIndexers`, 4096 anchors per file), so the sentence is wrong, and the markers disappear if the optional `project.read` is denied (`plugin.toml` lists it as optional) or the project is untrusted.
- kube-explorer README: "Reload from Cluster ... would not fit a provider's 50 ms". The premise is outdated (K-A5.3): a command with `editor.write` has no budget.
- Feature id completion (DC11) and Helm repo quick fixes (K-N1) are empty until the user runs `refresh-features` / `helm-repo-cache`; the READMEs say so, the change-notes do not.
- Unverified in the IDE (own READMEs admit it for most): fileIcons globs (flyway, liquibase, openapi), `jsonc` language id, `$NAME$`, snippet syntax, indexer markers.

## Rows that changed vs the old audit

About 75 old rows or sub-rows changed status: kube-explorer 40 units Missing/Partial to Done (K-I5-10, K-N1/2/4, K-A1.2, A2.2/2.3, A3.2, A4.2, A4.3-5 partly, A6.2-4, A7.1, A8.2/8.3, A9.2/9.5/9.7, FT1, all 16 LT, X1, X4) and K-I1-4, K-X3 to Partial; flyway FW3, FW6, FW8, FW9, FW11; liquibase LQ8-10, LQ16, LQ18, LQ11 (Partial); openapi OA2, OA6, OA7, OA12, OA14, OA18, OA29, OA31, OA3/9/10/16/19/24 to Partial; devcontainer DC2, DC7, DC10, DC11, DC9 (Partial); jpa 1-7, 10-12, 14, 16, 21, 22, 15 (Partial), 8/9/13/30 stay Partial or Missing as noted. Class changes without a status change: K-A5.3 F* to F (`editor.write`), FW2 and OA4 and K-A4.3 B to F* (`tee` route), OA19 F to F (rename only).

## The 10 best next rows

1. K-A5.3 Reload from cluster (F, S): a command with `kubectl get -o yaml` and `apply_edits`; also fixes the stale README.
2. K-A4.5 and K-A4.4 leftovers (F, S): Clear, Scroll to end, log fields, end-of-range time on the existing Html log view.
3. JPA15 entity graph names (F, S): check `@NamedEntityGraph(name)` and `@EntityGraph("x")` references; closes an over-stated DONE.
4. OA19 live templates (F, S): add `opnp`, `endp`, `swag` labels and an OpenAPI 3 root skeleton to the existing completion.
5. DC9 recent containers (F, S): `docker ps -a` with the folder label, Start action.
6. FW2 and OA4 file output via `tee` (F*, M): versioned empty migration from `input`, `.http` file from the request list; one shared capability decision.
7. OA3/OA9 JSON (F*, M): extract-schema and bundle for JSON specs with `apply_edits` or clipboard.
8. OA20 JSON key completion (F*, M): reuse the `keys.rs` tree over `json.rs` positions.
9. JPA8/JPA9 attribute checks (F*, M): getter/setter pairing and target-is-entity by project lookup.
10. K-I1b/c/K-I3 required keys and enums for the 29 kinds (F*, L): biggest remaining Kubernetes inspection gap; keep quiet for CRDs.
