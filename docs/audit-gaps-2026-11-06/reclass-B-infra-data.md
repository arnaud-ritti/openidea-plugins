# Re-classification of the B and F* rows of B-infra-data.md against plugin API 1.2

Source: [B-infra-data.md](B-infra-data.md) (classed against 1.1). 1.2 facts used, from `/tmp/oi/plugins/README.md` and `/tmp/oi/wit/indexing.wit`:
`host::pick/pick_many/input/confirm` (command or event of a window only; one-line input), `host::set_context`, `ctx.selected_paths()`, `viewItem.context` menus, `host::reveal_view`, `workspaceContains:`, and `document-indexer` (definitions / references / aliases / gutter markers keyed by plugin-chosen strings, project-wide, Go to Declaration only, 4096 defs+refs per doc, `project.read`, trusted projects).
Not changed by 1.2: U1 (references/rename/hover/find usages), U2 (file creation; a dialog does not write a file), U3, U4, database APIs, injection, folding, decorations, streaming, floating toolbars, schema association (LIM 667/1102).
Code already using 1.2: kube-explorer `viewItem.context` + `confirm` + `reveal_view`; flyway `confirm`; liquibase `input`/`confirm`; devcontainer `pick`/`confirm`; jpa-assist `pick`/`pick_many`/`input`/`set_context`. kube-explorer has no `pick`/`input` yet. No plugin here declares a `documentIndexers` (only spring-beans and symfony do). Liquibase and openapi already have `definition` providers (Liquibase also `line_markers`), so the indexer adds little there for plain `include`/`$ref`.

Ids: K-I = Kubernetes inspections, K-N = intentions, K-A1..A9 = action themes, K-LT = live templates, K-X = other extension points; FW Flyway; LQ Liquibase; OA OpenAPI; DC Dev Containers; JPA. "Unchanged: U5-gone" means the dialog blocker is lifted but another blocker stays.

## Rows that flipped

| Row | Old | New | 1.2 feature | Sketch | Size |
|---|---|---|---|---|---|
| K-A8.2 Attach Custom image | B | F | `host::input` (README: one-line text box, command context) | `resource-debug`: `input("Debug image", stored)` instead of the `debugImage` setting; keep the setting as default | S |
| K-A6.2 Create Chart | F* | F | `input` for the chart name | New command: `input` name, `run_process(helm create <name>)` in the project cwd (`process:helm` is already declared) | S |
| K-A6.3 Configure Helm Parameters | F* | F | `input`/`pick` plus `storage` | Command asks release name, values file and `--set` once, stores them, `helm-template`/lint read them | S |
| K-A2.3 Customize Namespaces | F* | F | `pick_many` | Command runs `kubectl get ns`, `pick_many` the visible ones, store in `storage`, tree filters on it | S |
| K-A4.2 Select Log Sources | F* | F | `pick_many` | `resource-logs` on a workload: `pick_many` over its pods/containers, then terminal `kubectl logs -f` per pick | S |
| K-A8.3 Edit recents | F* | F | `pick_many` | Recent debug images kept in `storage`; a command removes entries chosen with `pick_many` | S |
| K-X3 Find usages, rename, references (~50) | B | F* | `document-indexer` definitions/references, Go to Declaration only | Key `kube:<Kind>/<name>` defined at `metadata.name`; referenced from `configMapKeyRef.name`, `secretName`, `serviceName`, `scaleTargetRef`, selectors. Find usages, rename, hover stay U1; false positives on name clashes across namespaces | M |
| FW8 Line markers entity <-> migration | B | F* | indexer `markers` + `definitions` | Key `table:<name>`: `CREATE TABLE` in `V*__*.sql` defines it, `@Table(name)`/snake_cased `@Entity` class marks it (text rules, no U4); reverse key for the SQL-side marker. Only the first target is followed | M |
| FW11 SQL resolve extension | B | F* | indexer `references` | `${placeholder}` in `.sql` references key `flyway:ph:<name>`, defined in `flyway.conf`/`flyway.toml` placeholder entries (the plugin already scans these for completion). Go to Declaration only | S |
| LQ7 References (include, tables, columns, rollback, contexts) | B | F* | indexer | Keys `lb:table:<t>` (`createTable`), `lb:cs:<id>::<author>`, `lb:file:<path>`; refs from `tableName`, `rollback` and `include file`. The existing `definition` provider stays for `include`. Completion and rename stay U1; 4096-per-document cap on big changelogs | M |
| OA6 Compare specs of two files | B | F* | `pick` (second-file chooser; blocker was U5) | `find_files` for specs, `pick`, semantic diff in own code rendered in an Html view. `find_files` skips ignored dirs (U3); the diff engine is the real cost | L |
| DC7 Create and clone sources | B | F* | `input` for the URL (blocker was U5) | `input` URL, `git clone` (`process:git`) then `devcontainer up --workspace-folder`. Risk: clone target is outside the project, the process cwd and approval are unverified | M |

## Rows that stay F* (no 1.2 feature changes the class)

| Rows | Reason |
|---|---|
| K-I1-4, K-I8, K-I9 (schema, deprecated, Helm values/templates checks) | Bundled-data or regex risk; nothing in 1.2 helps. Unchanged: risk only |
| K-N1 Helm repo URL<->alias, K-N4 mark as non-Kubernetes | Code-action budget / marker comment. Code-action resolve cannot call `pick`/`input` (README), so no dialog there. Unchanged |
| K-A1.2 custom kubeconfigs, K-A3.2 Edit Secret, K-A4.3-5 log viewer, K-A5.3 reload, K-A6.4 generate dependency, K-A7.1 Telepresence, K-A9.2 notifications | `pick`/`input` make the UX nicer but not the blocker: values would show on screen, edits to the open file cannot come from a command, no streaming. Unchanged |
| K-LT 1-16 (live templates), K-X1 Helm completion, K-X4 manifest line markers | No live-template contribution; completion loads for all YAML. K-X4 could use indexer markers instead of the line-marker provider (cross-file, no 50 ms budget): same class, better route |
| FW2-3, FW9 (templates, icons) | File creation stays U2; `fileTemplates` unchanged. `workspaceContains:` only gates menus |
| LQ8-11, LQ16, LQ18, OA3, OA7, OA9, OA10, OA12, OA16, OA20, OA24, OA29, DC9, DC11, DC13 | Provider, text-rule or unverified-icon risks; nothing in 1.2 touches them. DC9: `pick` could drive a container list, class unchanged |
| JPA 4-6, 8, 9, 11, 13-16, 21, 30 | Text-rule inspections. The indexer graph is host-side; the plugin cannot query it, so "entity-ness by simple name" gets no help. Unchanged: risk only |

## Rows that stay B

| Rows | Blocker |
|---|---|
| K-N3, K-A9.3, LQ/OA/K schema association (OA21) | Schema association (LIM 667/1102). Unchanged |
| K-A1.3 open kubeconfig, K-A1.4 paste kubeconfig, K-A9.6 install kubectl, OA4 generate HTTP requests, OA17, FW/LQ generators (FW4, LQ3, LQ6) | U2 file creation (and `open_location` limits, U4 entity model, DB). `input` supplies the text but nothing writes it. Unchanged: U2 |
| K-A1.5, K-A1.6, K-A5.2, K-A5.4, K-X7, DC (floating toolbar, settings, banners) | No editor decorations / open-settings API. Unchanged |
| K-A3.3, K-A3.4, K-A4.1, K-A7.2, FW5, FW7, LQ4, LQ5, LQ15, DC6, DC8 | DB APIs, streaming/process handles, run-config edits, remote backend. Unchanged |
| K-X2 cluster-aware completion | Providers cannot wait for kubectl (LIM 653). Unchanged |
| K-X5, K-X6, K-X8, K-X9, FW10, LQ12, LQ14, LQ17, OA13, OA15, OA26, OA27, OA28, OA30 | Inlay hints, injection, folding, content icons, decorations, structure view, webview network, MCP. Unchanged |
| K-A9.4, LQ13, OA22, OA23, DC12, JPA31 | U1. The indexer gives Go to Declaration only (no find usages, rename, hover); OA22 `$ref` navigation is already done by the openapi `definition` provider. Unchanged: U1 |
| DC1, DC3 | JetBrains IDE-settings model. Unchanged |
| OA8, OA11, OA25, JPA17-20, 23-26, 29, 32-34 | U4, DB, diagram, facet, virtual documents. Unchanged |

## Summary

Rows re-judged: about 130 table lines classed B or F* in the source file (some group several features); the tables above list them by id or group.
- Flipped: 12. B to F: 1 (K-A8.2). B to F*: 6 (K-X3, FW8, FW11, LQ7, OA6, DC7). F* to F: 5 (K-A6.2, K-A6.3, K-A2.3, K-A4.2, K-A8.3).
- Net class change: B minus 7, F* plus 1 (minus 5 to F, plus 6 from B), F plus 6.
- Everything else is unchanged: most remaining B rows are U1, U2, U4, DB or decoration blockers that 1.2 does not touch. The indexer only gives Go to Declaration and gutter markers, which is why only five reference-type rows moved, all to F*.

Five best-value rows:
1. K-A6.2 Create Chart (S): `input` plus `helm create`.
2. K-A8.2 Attach Custom image (S): `input` replaces a setting, pairs with K-A8.3.
3. K-A2.3 Customize Namespaces (S): `pick_many` over `kubectl get ns`, big usability win on large clusters.
4. K-A4.2 Select Log Sources (S): `pick_many` containers or pods for workload logs.
5. LQ7 Liquibase references via an indexer (M): first plugin here to use `documentIndexers`; the FW8/FW11 and K-X3 indexers can reuse the same key helper afterwards.
