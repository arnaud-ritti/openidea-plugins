# Backlog: tooling plugins (group D), rows still F or F*

Source: [D-tooling.md](D-tooling.md) after [reclass-D-tooling.md](reclass-D-tooling.md) (API 1.2), checked against the code at `f288eda` on 2026-11-06. Row ids are `plugin #row` as in D-tooling.md. Nothing here is verified in the IDE; the mock host proves logic only. B rows are out of scope (blockers listed at the end).

Result of the check: 21 rows are F or F* and open (gitlab 4, ignore 2, properties 3, ideolog 3, quarkus 8, tasks 1, dotenv 0, dependency-audit has no audit rows). 15 rows were closed since the audit.

## Done since audit

| Row | Evidence |
|---|---|
| ignore #3 Ignore/Unignore groups | `ignore/src/lib.rs:109-141` `pick_target` over `ROOT_IGNORE_FILES`; `add-selected` command in the `projectView.context` menu |
| ignore #10 settings, starred templates | commit `d69a2e6` (unignore setting, starred templates) |
| properties #2 Add New Property File | commit `a7ba491`; `fileTemplates` id `properties` in `properties/plugin.toml:70` |
| gitlab #1 Create Merge Request form | view `new-merge-request` + command `create-merge-request` (`gitlab/plugin.toml`), README "Create Merge Request" |
| gitlab #3 Mark Viewed | README: "Viewed collapses a file", remembered per commit |
| gitlab #4 Create Snippet | command `create-snippet`, view `new-snippet`, `projectView.context` entry |
| gitlab #5 Share Project | view `share-project-form`, command `share-project` |
| gitlab #13 note edit/delete, emoji | README lines 106-108 (Delete, award emoji from a fixed list of eight) |
| gitlab #14 MR attribute editing | command `edit-merge-request`; change-notes "Unreleased: Edit labels, assignees, reviewers, milestone and title" |
| gitlab #15 list filters | same change-notes: Filter button picks author, reviewer, label, text |
| gitlab #16 / #17 commits, pipelines | views `merge-request-commits`, `merge-request-pipeline`; commit `74200d8` |
| tasks #4 / #5 / #8 | change-notes "Unreleased": Search Issues, Save/Load/Remove Context, Add/Remove Repository (`tasks/src/lib.rs:427,650-745`, tests at `:1089`) |

Stale text to fix when touching these: `gitlab/README.md` lines 91-92, 203-212 still say "no pickers", "five quick filters", "no commit list".

## Work packages

### P1 gitlab: Open on GitLab in Project view
- Rows: gitlab #23 (F). Build: `projectView.context` entry for `open-in-browser` using `ctx.selected_paths()`, one URL per path (or first path). Risk: low; log menus stay unavailable. Files: `gitlab/plugin.toml`, `gitlab/src/lib.rs`, `tests.rs`. Size S. Deps: none. Test: MockHost with selected paths, assert opened URL for a file and a folder.

### P2 gitlab: CI editing aids
- Rows: gitlab #9 (F*), #11 (F*), #12 (F*, local part only).
- Build: (a) `completion` provider on `yaml` for `.gitlab-ci.yml`: top-level and job keywords from a bundled list, predefined `CI_*` variables after `$`/`${`. (b) `!reference` accepted without findings. (c) `include: local:` read through `read_file`, merge its jobs and stages into `ci.rs` so the quiet-when-`include` rule narrows to remote/project/template includes only.
- Risk: keyword list drifts from GitLab versions (record the doc revision); `$` trigger is U6, so the popup opens on letters only; whether the IDE's own YAML handles `!reference` is unverified; remote includes need `net:` hosts, excluded.
- Files: `gitlab/src/ci.rs`, `gitlab/src/lib.rs`, new `gitlab/src/ci_data.rs`, `plugin.toml` (provider on `yaml`), `README.md`. Size M. Deps: none. Test: unit tests on `ci.rs` with fixture YAML, include read through MockHost files, completion labels at a cursor.

### P3 quarkus: Java-side config checks
- Rows: quarkus #2 unassigned properties (F*), #3 `@ConfigProperty`/`@ConfigMapping` completion (F*), #6 annotation-level diagnostics (F*), #8 quick fixes (F*).
- Build: reuse `java.rs` declaration reader. #3 completion provider for `java` (text-only context detection of `name = "` and `prefix = "`). #2 diagnostics on Java `file-saved` for keys no `application*` file sets (quiet when the key has `defaultValue`, when env or profile files exist elsewhere, or for `Optional`). #6 start with REST client without `@RegisterRestClient`, scheduler cron syntax. #8 quick fixes: add key to `application.properties` (existing file only), insert `defaultValue`.
- Risk: false positives without type info and with values from env, system properties or other profiles; 50 ms budget on large Java files; import insertion by text; add-key fix needs the properties file to exist.
- Files: `quarkus/src/java.rs`, `checks.rs`, `lib.rs`, `plugin.toml` (java adds `completion`, `code-actions`, `events`). Size L. Deps: none. Test: Java snippets in `lib.rs` tests (pattern at `lib.rs:259-283`); one quiet-case test per false-positive source.

### P4 quarkus: metadata-driven properties checks
- Rows: quarkus #1 required properties (F*), #16 named-map keys `quarkus.datasource."name".*` (F*).
- Build: extend `scripts/gen-metadata.py` and `data/quarkus-properties.tsv` with required/default info; completion and known-key matching for quoted map segments. Required-key check opt-in setting, Info severity.
- Risk: values supplied from env or other sources, so default off; ambiguous map key patterns; TSV size growth (wasm size). Files: `quarkus/scripts/gen-metadata.py`, `data/*.tsv`, `src/checks.rs`, `src/lib.rs`, `settings.schema.json`. Size M. Deps: none (touches checks.rs, so run before or after P3, not with it). Test: completion/diagnostic tests against the TSV.

### P5 quarkus: key to Java definition and Qute language
- Rows: quarkus #4 (F*), #12 (F*).
- Build #4: document indexer over `.properties`, `.yaml` and `.java` anchoring `prefix.key` (Go to Declaration both ways, markers); Java detection stays text-based. Build #12: `languages` contribution for `*.qute.html/txt/json/yaml` with syntax; section tag highlighting and completion of `{#if}`, `{#each}`.
- Risk: indexer on Java unverified; overlap with the IDE's own navigation; multi-dot extensions (`.qute.html`) and host-language loss are unverified. Do #12 only after a one-file spike in the IDE. Files: new `quarkus/src/indexer.rs`, `quarkus/languages/qute.toml`, `plugin.toml`. Size L (#4) + S (#12). Deps: P3 (shares `java.rs` reader). Test: direct `index()` tests (as in the symfony/liquibase indexers); language file checked with `openidea-plugin check`.

### P6 properties: live unused keys and goto related
- Rows: properties #1 (F*), #11 (F*).
- Build: #1 publish diagnostics for unused keys on `file-saved`/`scheduled`, reusing `usages.rs::mark_used`, capped by file count and size, opt-in setting. #11 document indexer: one definition per key per locale file so Go to Declaration lists every locale.
- Risk: whole-project scan cost, dynamic keys give false positives (keep Hint severity with the unnecessary tag); multi-target chooser UX unverified. Files: `properties/src/usages.rs`, `lib.rs`, new `indexer.rs`, `plugin.toml`, `settings.schema.json`. Size M. Deps: none. Test: MockHost project with several files, unused and dynamic cases; indexer test with three locales.

### P7 properties: bundle editor writes sibling locales
- Rows: properties #7 (F*). Build: let the `bundle` custom editor edit other locales (write sibling files) and show incomplete keys. Risk: unverified that an editor may write a sibling file; edits to existing files only. Files: `properties/src/views.rs`, `lib.rs`. Size M. Deps: none (P6 touches `lib.rs`, so serialise). Test: MockHost edits per locale; spike in the IDE first.

### P8 ideolog: settings view, format banner, time gaps
- Rows: ideolog #10 (F*), #9 (F*), #3 (F*).
- Build: #10 Table/Form view listing custom patterns and highlighting rules with add/remove via `input`/`pick`, stored in `storage` (also makes #4 "Show settings" a `reveal_view`, still B for the IDE dialog). #9 status item naming the detected format. #3 time-gap colouring: parse timestamps for the built-in formats.
- Risk: scalar-only settings so a separate view; no per-file banner or switch UI; timestamp parsing for every format inside the 50 ms budget (cap to the first 512 KiB, as today). Files: `ideolog/src/lib.rs`, `log.rs`, `plugin.toml`, `settings.schema.json`. Size M (#10), S (#9), M (#3). Deps: none. Test: parser tests per format, pattern round trip, status text.

### P9 ignore: New menu per ignore type and path completion
- Rows: ignore #4 (F*), #7 (F*).
- Build #4: one `fileTemplates` entry per type (.dockerignore, .npmignore...) with `pick_many` for templates left out. Build #7: completion of entries from `find_files`.
- Risk: a bare dot-name from the New menu is unverified (U2 flavour), possible fallback is `<name>.ignore`; `find_files` hides dot paths and gitignored files, which are what ignore files name (U3), so completion is useful only for visible paths. Files: `ignore/plugin.toml`, `file-templates/*`, `src/lib.rs`. Size S (#4), M (#7). Deps: none. Test: template files exist and pass `openidea-plugin check`; completion with MockHost files (mock does not hide, so cover the real-host gap in notes).

### P10 tasks: FogBugz, Lighthouse, Pivotal Tracker
- Rows: tasks #3 (F*). Build: three tracker entries in `trackers.rs` with their vendor hosts. Risk: services believed discontinued and unverified, hosts fixed in the manifest (`net:`). Recommendation: skip unless a user asks; lowest value. Files: `tasks/src/trackers.rs`, `plugin.toml`. Size S each. Deps: none. Test: JSON fixtures per tracker.

## Recommended order

Value per effort, shared foundations first. Only plugin-local files are involved, so packages in different plugins never collide.

- **Wave 1 (S and small M, no new foundation):** P1 (S), P8 (#9, then #10; M), P6 (#1 only, M), P9 #4 (S). Top value: everyday-visible features, no unverified IDE behaviour except P9 #4.
- **Wave 2 (extend existing readers and data):** P2 (M), P4 (M), P3 (L). P3 and P4 both touch `quarkus/src/checks.rs`: run P4 first, then P3.
- **Wave 3 (needs IDE spikes, unverified UX):** P5 (L, indexer on Java and Qute), P6 #11 (indexer), P7 (M), P9 #7 (M), P10 (optional).

Parallel safe (distinct files): P1 with P2 only if two agents avoid `gitlab/src/lib.rs` (P1 touches the menu and `open-in-browser`, P2 the provider; take them in sequence or merge carefully). Fully independent: {P1 or P2 (gitlab)} || {P3/P4/P5 chain (quarkus)} || {P6, P7 chain (properties)} || P8 (ideolog) || P9 (ignore) || P10 (tasks). Within one plugin, serialise.

## Blocked, not in this backlog (reminder)

B rows remain on: U1 references/rename/hover (properties #5 #6, quarkus #5, dotenv #2, tasks #10), U2 file creation (properties #3, quarkus #14 #18), U3 ignored files (ignore #1), U4 type resolution (quarkus #7 #13), U6 trigger characters (dotenv #3, ignore #5), decorations, folding and caret control (ignore #8 #9, ideolog #1 #2 #5 #6, properties #8 #9, dotenv #1), injection (gitlab #8 #10), fixed `net:` hosts and secrets (tasks #1 #2, gitlab #7 #21).
