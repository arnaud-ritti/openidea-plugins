# Backlog: infra and data plugins (open F and F* rows after the 1.2 reclass)

## Status (2026-11-06, after the backlog implementation pass)

Derived from `change-notes`, README and code on branch `arnaud-ritti/implement-new-feature`. DONE = built per the change-notes/README; every item is tested against the mock host only and unverified in the IDE. 'not verified' = I could not confirm it from the change-notes, README or code.

- DONE: KE-1, KE-2, KE-3, KE-4, KE-5, KE-6, KE-8 (kube-explorer), FW-1, LQ-1, LQ-2, LQ-3, OA-1, OA-3, OA-4, OA-5, DC-1, JPA-1, JPA-2, JPA-3 (`@NamedEntityGraph` names: not verified), JPA-4, JPA-5. KE-1 covers core, apps, batch, networking and rbac kinds only.
- PARTIAL: KE-7 (Chart.yaml quick fixes, Select Log Sources and debug image prompt done; the kubeconfig reload on change was dropped because of the 50 ms provider budget). OA-2 (extract inline schema, self-contained copy and reference copy done for YAML; the JSON edits were skipped). DC-2 (Dev Containers view with Open Shell and Stop done, audit v2 round 2 added stopped containers and Start; gutter markers that run commands skipped, a marker can only open a location).

Sources: [B-infra-data.md](B-infra-data.md) (row ids: K-I, K-N, K-A, K-LT, K-X = kube-explorer; FW, LQ, OA, DC, JPA by row number), [reclass-B-infra-data.md](reclass-B-infra-data.md), [implementation-plan.md](implementation-plan.md), [api-facts.md](api-facts.md). Checked against the plugin sources and READMEs in this worktree (HEAD `f288eda`). Nothing here was run in the IDE.

Conventions for every package: MockHost tests for success and each dismissal; `cargo test` and `openidea-plugin build` in the plugin directory; `change-notes` in `plugin.toml`; update the plugin README limits and flip the row in `reclass-B-infra-data.md`. F* checks follow quiet-unless-sure: report only when the text proves the problem, stay silent on anything dynamic (anchors, `tpl`, library bases, unknown kinds).

## Done since audit (evidence)

| Rows | Evidence |
|---|---|
| K-I5 deprecated resources, K-I6/I7 Chart.yaml values and keys, K-I10 excessive trim | `checks.rs` codes `deprecated-api-version`, `chart-invalid-value`, `chart-unknown-key`, `helm-excessive-trim` |
| K-N2 Helm repo update, K-A9.7 how to install kubectl, K-A2.2 All namespaces, K-FT1 CSR template | commands `helm-repo-update`, `how-to-install-kubectl`; `templates/` has CSR; README "All namespaces" |
| K-A6.2, K-A6.3, K-A2.3, K-A4.2, K-A8.2, K-A8.3 | commands `helm-create`, `helm-parameters`, `customize-namespaces`, `resource-log-sources`, `edit-debug-images`; `prompts.rs` |
| K-X3 references | `kube-explorer/src/indexer.rs` (`kube:<Kind>/<name>`), `documentIndexers` in `plugin.toml`. Go to Declaration only |
| Refresh (Misc row 2) | ruled out in the audit (button existed); no work |
| FW6 run detectors, FW8 entity/migration markers, FW11 placeholders | `[[contributes.runDetectors]]` x3, `flyway/src/indexer.rs` (`fw:table:`, `fw:entity:`, `fw:ph:`) |
| LQ7 references | `liquibase/src/indexer.rs`, `documentIndexers` in `plugin.toml` |
| OA2 spec candidate, OA6 compare two files, OA14 reload, OA18 Swagger 2.0 templates, OA31 JSON endpoints | `missing-openapi-root` in `checks.rs`; `diff.rs` + command `compare-specs`; `web/main.js` reload button; `swagger-yaml`/`swagger-json` templates; `json.rs` scanner used by the endpoints view |
| DC2 folder structure, DC7 clone, DC10 completion | `folder-structure` in `checks.rs`; command `clone` (`host::input`, `git clone`); `completion` provider on `json`/`jsonc` |
| JPA 1, 3, 7, 10, 12, 22 (2 covered by 3) | `jpa-immutable-collection`, `jpa-val-attribute` (flags every Kotlin `val`, so row 2's case is inside it), `jpa-data-class` quiet when overrides exist, `jpa-entity-nested/not-a-class/multiple-roles`, `jpa-listener-without-callbacks`, actions "Add @X callback" |

## Work packages

Size S under a day, M 1-3 days, L a week. "Files" are under `plugins/<name>/`.

### kube-explorer (8 packages; `lib.rs` is shared, so keep one lane per wave)

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| KE-1 | K-I1, I2, I3, I4 | Bundled, trimmed table (kind -> allowed top-level and `spec` keys, read-only fields, deprecated keys) for the core, apps, batch, networking, rbac kinds, in a new `kinds.rs`. Diagnostics through `checks::for_file`. Quiet for CRDs, unknown groups, files with Helm actions, anchors and merge keys; unknown keys only inside kinds with a complete table. I3 only for built-in groups with an unknown kind | Wasm size, version drift, false positives on fields from newer or older Kubernetes; the modeline route already gives the language server's validation | `kinds.rs` (new), `checks.rs`, `settings.schema.json` | L | none | Per-kind accept and reject fixtures, CRD and Helm silence, setting switch |
| KE-2 | K-I8, I9 | Parse `values.yaml` keys of the chart around the file; flag `.Values.a.b` whose first missing segment is absent. Flag `include "x"` with no `define "x"` in `templates/` (including `_helpers.tpl`). Quiet when the file uses `tpl`, `range`/`with` rebinding (`.` is not `.Values`), `$.Values` dynamic keys, `global`, or the chart has `dependencies:` | Subcharts, `--set`-only values, dynamic names; judged by text only | `helm.rs` (new), `checks.rs`, `settings.schema.json` | M | none | Fixtures for each quiet condition; chart found via `find_files` is skipped for dot dirs: use `read_file` |
| KE-3 | K-LT 1-16, K-X1 | One `completion` provider on `yaml`: 15 `k*` abbreviations and `incl` as snippet items, plus `.Values.` keys and `include` names inside `{{ }}` (reuses KE-2 parsing if it landed). Offer templates only on an empty-line prefix match and not inside an existing manifest body | Loads for every YAML file; 50 ms budget; `$1`/`$0` snippet syntax unverified (liquibase and openapi already use it) | `templates/` snippets, `complete.rs` (new), `plugin.toml` | M | soft on KE-2 | Prefix matching, no items in non-manifest YAML, budget on a large file |
| KE-4 | K-A1.2, K-A9.2, K-N4, K-A7.1 | `kubeconfig` setting accepts a path list and a `pick` chooser command; notification toggles gate `notify` calls; `nonKubernetesGlobs` setting suppresses checks; Telepresence connect/quit commands via optional `process:telepresence` in a terminal | Merge semantics of several `--kubeconfig` files (use one chosen file, not `KUBECONFIG` merge); settings are scalar only (comma list) | `lib.rs`, `plugin.toml`, `settings.schema.json` | S | none | Setting parse, glob match, denied process |
| KE-5 | K-A4.3, A4.4, A4.5 | Html view over one `kubectl logs --tail=N` snapshot with form filters (text, regex, case, date from `--timestamps`), soft wrap, download as clipboard copy. Live follow stays in the terminal command | Output size cap, no file write (U2), regex engine in wasm; form events inside a view | `logs.rs` (new), `lib.rs`, `plugin.toml` | L | none | Filter functions on fixture logs; size cap; view render |
| KE-6 | K-A3.2 | Edit Secret: Form of keys and values prefilled from the decoded secret (needs `allowRevealSecrets`), `kubectl apply` through stdin like Create Secret | Values visible on screen (no masked field); lost update, so send `resourceVersion` | `secrets.rs`, `lib.rs`, `plugin.toml` | M | none | Decode/encode round trip, setting off, conflict |
| KE-7 | K-N1, K-A6.4, K-A5.3 | Code actions: Chart.yaml dependency stanza (placeholders for repo and version, no dialog possible in a code action); repo URL <-> alias using a `helm repo list` result cached in `storage` by a command, never run in the provider. Reload from cluster: only if a spike shows kubectl fits the budget, else drop | `pick`/`input` return Invalid in code actions; stale cache; 50 ms | `convert.rs` or `chart_actions.rs` (new), `lib.rs` | M | none | Edit-shape tests like `convert.rs`; no-cache case returns no action |
| KE-8 | K-X4 | Indexer `markers` from a resource to its users and back (reuses the `kube:` keys of `indexer.rs`); no line-marker provider | Name clashes across namespaces produce wrong targets; multi-target chooser is host side | `indexer.rs` | S | none | Direct `index()` tests, as the existing ones |

### flyway

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| FW-1 | FW2, FW3, FW9 | `fileTemplates` for an empty versioned SQL migration, Java and Kotlin migration, Java callback and SQL callback; `fileIcons` for `V*__*.sql` (and `U`, `R`). Keep `Copy Next Migration Version` for the number | `$PACKAGE$` placeholder unverified (LIM 1263); icon glob on real names unverified; the New menu cannot compute the next version | `plugin.toml`, `templates/` (new), icons | S | none | A test that each template path exists and registers (as `openapi/checks.rs` does) |

### liquibase (a lane: LQ-1 then LQ-2; LQ-3 touches only `line_markers`)

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| LQ-1 | LQ8, LQ9, LQ18 | `file:` completion in YAML and XML from `find_files` (relative to the changelog); XML element and attribute completion from `catalog.rs`; `author` value completion from `git config user.name` (optional `process:git`) or a setting. XML provider only inside `<changeSet>` and quiet next to the lemminx schema | 50 ms and large XML; `find_files` skips dot dirs and ignored paths | `complete.rs`, `plugin.toml` (add `xml`), `lib.rs` | M | none | Completion fixtures; process denied |
| LQ-2 | LQ10 | JSON changelog support: add `json` language, parse `databaseChangeLog` JSON for checks, view and indexer. Today `.json` is only a guard in the scan | Needs a JSON key/array scanner (openapi `json.rs` is the model to copy, not share: no cross-plugin crates) | `changelog.rs`, `checks.rs`, `indexer.rs`, `plugin.toml` | M | after LQ-1 (same files) | Parse fixtures for each check and the indexer |
| LQ-3 | LQ11, LQ16 | Real gutter markers (`line_markers` now only publishes problems): include navigates to the file; `fileIcons` for `db.changelog-*`. Run icon only if the unverified icon works, else navigate only | One target per marker; run icon behaviour unverified | `lib.rs` (`line_markers`), `plugin.toml`, `icons/` | S | none | Marker positions for `include` in each format |

### openapi (a lane: OA-4, OA-3, then OA-1/OA-2; OA-5 independent)

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| OA-1 | OA19, OA20 | Skeleton items `opnp`, `swag`, `endp` in the completion provider, gated to spec files; keyword and enum completion from a static key tree chosen by indentation and path (YAML block style only; quiet in flow style and JSON) | Wrong context in odd indentation; JSON string quoting | `lib.rs` (completion), `keys.rs` (new) | M | soft on OA-4 | Context-by-indent fixtures |
| OA-2 | OA3, OA9, OA10 | Extract inline schema to `components/schemas` code action (YAML, and JSON with comma care); Export self-contained spec to clipboard (resolve local and relative `$ref`, refuse cycles); Copy schema meta at the caret | Indentation and JSON syntax in edits; cycles; format of OA10 unknown (use JSON pointer plus `$ref` form) | `refs.rs` (new), `lib.rs`, `plugin.toml` | M | after OA-1 | Edit-shape tests, cycle refusal |
| OA-3 | OA7 | Compare With Last Revision: `git show HEAD:<path>` via optional `process:git`, feed the existing `diff.rs` and `compare` view | Path relative to repo root; file new or untracked | `lib.rs`, `diff.rs`, `plugin.toml` | S | uses done OA6 | Diff of two strings; git denied; untracked |
| OA-4 | OA16, OA24, OA29 | `specGlobs` setting extends spec detection; `fileIcons` for `openapi.*`/`swagger.*`; operation line markers (navigate only unless the run icon is verified) | Detection change reaches every check; run icon unverified | `spec.rs`, `checks.rs`, `plugin.toml`, `settings.schema.json` | S | none | Detection with and without the setting |
| OA-5 | OA12 | Redoc option next to Swagger UI in the preview (switch button) | Bundle size; Redoc MIT notice; frame has no network | `web/`, `NOTICE.md` | M | none | Static page test like `preview_page_has_a_reload_button` |

### devcontainer (one lane; both touch `lib.rs`)

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| DC-1 | DC11 | Feature id completion from the public index (`containers.dev` JSON) with a `storage` cache refreshed by a command, never fetched in the provider; optional `net:` host | `net:` host fixed in the manifest; stale cache | `lib.rs`, `plugin.toml` | S | none | Completion from a seeded cache, empty cache |
| DC-2 | DC9, DC13 | Dashboard: Table view over `docker ps --filter label=devcontainer.local_folder=<root>` (stop and open shell per row); `line-markers` on `"image"`/`"dockerComposeFile"` that start the container | Label name unverified; run icon unverified | `lib.rs`, `plugin.toml` | M | after DC-1 | Parse fixture of `docker ps --format json` |

### jpa-assist (JPA-3 before JPA-4; others independent but share `checks.rs`)

| Pkg | Rows | Build | Key risk | Files | Size | Deps | Tests |
|---|---|---|---|---|---|---|---|
| JPA-1 | 4, 5, 6 | Read `build.gradle(.kts)`/`pom.xml` for `kotlin-jpa` or `plugin.jpa`; warn when Kotlin entities exist without it; with it, flag a redundant no-arg constructor and `open`. Fix edits the existing build file | Plugin detection by text (version catalogs, convention plugins): report rows 5-6 only when the plugin is named literally | `build.rs` (new), `checks.rs` | M | none | Gradle, Kotlin DSL, Maven fixtures; unknown build stays quiet |
| JPA-2 | 8, 9, 11 | Attribute signature rules by spec (two relationship annotations, both `@Id` kinds, annotation on getter and field mixed), relationship on `Map`/primitive/`String`, listener class needs a public no-arg constructor | Access-type inference; entity-ness of a target by simple name | `checks.rs`, `model.rs` | M | none | One fixture per rule plus clean entity |
| JPA-3 | 13, 14, 15 | Name checks: `@AttributeOverride`, `@MapsId`, `@MapKey`, `@NamedAttributeNode`, `@NamedEntityGraph` names against the entity model; unknown named query and `:param` against method parameters (string literals only) | Inherited attributes, embeddables, duplicate simple names across packages; silent when the target class is not in the project | `queries.rs`, `model.rs`, `checks.rs` | M | after JPA-2 (model changes) | Fixtures incl. embedded and inherited |
| JPA-4 | 16, 30 | `persistence.xml` / `orm.xml` checks: `<class>` names a class in the project; `<provider>`, value patterns; `xml` provider added | Classes found by simple-name scan; second language gets loaded | `xml.rs` (new), `plugin.toml` | M | after JPA-3 (class lookup) | XML fixtures |
| JPA-5 | 21 | Add inverse attribute: code action on a relationship editing both entity files (`mappedBy`, field, imports) | Two-file edit unverified in the IDE; generic type text and imports | `generate.rs`, `lib.rs` | M | after JPA-2 | Edit-shape tests, missing target file |

## Order and waves

Value per effort first, no new shared foundation needed (the indexer key helper and the `pick` plumbing already exist); JPA-3 then JPA-4 is the one real chain, the rest are file-level conflicts inside a plugin.

- Wave 1 (10 packages: 7 S, 3 M): FW-1, LQ-3, LQ-1, OA-4, OA-3, DC-1, KE-4, KE-8, KE-2, JPA-2. Small, text-only or manifest-only, and the highest hit rate (templates, include completion, Helm values, JPA spec rules).
- Wave 2 (9 packages, all M): KE-3, KE-6, KE-7, LQ-2, OA-1, OA-2, DC-2, JPA-1, JPA-3. Need wave 1 files (OA detection, KE-2 parsing, JPA model) or carry unverified editing behaviour.
- Wave 3 (5 packages: 2 L, 3 M): KE-1, KE-5, OA-5, JPA-4, JPA-5. Largest or lowest certainty: bundled schema data, log viewer, Redoc bundle, a second XML language, two-file edits.

Parallel without touching the same files: one agent per plugin is always safe (separate directories). Inside a plugin, serialize as a lane: kube-explorer KE-4/KE-8/KE-2 (wave 1) can run together because they touch `lib.rs`/`settings`, `indexer.rs` and new `helm.rs` plus one `checks.rs` line each; expect a trivial merge on `checks.rs::for_file` and `settings.schema.json`. Never run together: KE-1 and KE-2 (both `checks.rs`), LQ-1 and LQ-2, OA-1 and OA-2 and OA-4 (`lib.rs`/`spec.rs`), DC-1 and DC-2, JPA-2 and JPA-3 and JPA-5 (`checks.rs`/`model.rs`).

## Summary

- 24 packages cover about 60 open rows: kube-explorer 8, jpa-assist 5, openapi 5, liquibase 3, devcontainer 2, flyway 1.
- Sizes: 7 S, 15 M, 2 L.
- Wave 1: 10 packages (7 S, 3 M). Wave 2: 9 (9 M). Wave 3: 5 (2 L, 3 M).
- Top 5 by value per effort: KE-2 Helm values and template checks, JPA-2 JPA spec signature checks, LQ-1 include path, XML and author completion, FW-1 Flyway templates and icons, OA-1 OpenAPI live templates and keyword completion.
- Already done (not in the backlog): 8 kube-explorer rows from the F pass, the 6 kube-explorer rows flipped by 1.2 plus K-X3, FW6/FW8/FW11, LQ7, OA2/OA6/OA14/OA18/OA31, DC2/DC7/DC10, JPA 1/3/7/10/12/22.
- Biggest risk class: unverified IDE behaviour (snippet syntax, run icons, `$PACKAGE$`, two-file edits), then false positives in KE-1, KE-2, JPA-2/3.
- Untouched: all B rows (U1, U2, U4, DB, injection, decorations), which this backlog does not cover.
