# Audit v2, group D (tooling) against HEAD (ac0edf6), plugin API 1.2

Scope: every gap row of [D-tooling.md](D-tooling.md) (the 88 rows that were Partial or Missing on 2026-11-06) re-checked in `plugins/<name>/src`, `plugin.toml` and `settings.schema.json` by grep and reading code. READMEs and change-notes were used only to find claims to test. The ~110 features D-tooling.md called Done were not re-audited one by one; spot checks are in "Claims the code does not back". Nothing is verified in the IDE (mock host only).

Classes under API 1.2: F feasible, F* feasible with a stated risk, B blocked. API 1.2 gives pick/pick_many/input/confirm, `set_context`, `selected_paths`, `viewItem.context`, `reveal_view`, document indexers, plugin icon. It does not give hover, rename, find usages, inlay hints, caret/selection control, a close-editor call, file creation, or user-driven folding; folds are whole-line ranges (`syntax::Fold` has `start_line`, `end_line`, `kind`).

Status: Done = behaviour exists in code. Partial = part of it. Missing = nothing in code.

## .ignore -> ignore

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | inspection Cover entry | Partial | `check.rs:139 covered_entries`, code `cover-entry` (`check.rs:160`) compares patterns only | B | file-matching part needs find_files over ignored paths (U3) |
| 2 | Close Ignored Editors | Missing | no command in `plugin.toml` (only `add-selected`) | B | no close-editor call in 1.2 |
| 3 | Ignore / Unignore file groups | Partial | `add-selected` in `projectView.context` (`plugin.toml`), `lib.rs:112 pick_target` (pick over `ROOT_IGNORE_FILES`), `lib.rs:139 add_selected` | F* | works only on the focused ignore file; otherwise it opens the file and returns "run the command again" via the returned string (`lib.rs:155`), which api-facts says is dropped. No unignore variant in the menu (only the quick fix `lib.rs:86`), no `editor.context` entry |
| 4 | New menu per ignore type + generator dialog | Partial | one `fileTemplates` entry `gitignore` -> `<name>.ignore` (`plugin.toml`) | F* | one `fileTemplates` entry per type is possible; a bare dot-name file is not creatable (backlog P9) |
| 5 | syntax for the 33 ignore types | Partial | `languages/dotignore.toml`, `highlights` `lib.rs:506`; built-in `ignore` language gets none | B | U6 |
| 6 | line marker (directory entries) | Missing | no `line-markers` provider in `plugin.toml` | B | icon allowlist has no folder icon |
| 7 | path references + element manipulator | Partial | definition `lib.rs:245 entry_target`, completion `lib.rs:257 path_completions` (new) | B | `find_files` hides dot and ignored paths, so the entries an ignore file usually names are never offered (`lib.rs:255` says so) |
| 8 | VCS file status provider | Partial | status item `Ignored` `lib.rs:208 show_ignored_status` | B | no Project-view or tab decoration |
| 9 | hide ignored files in Project view | Missing | no code | B | no Project-view filter |
| 10 | settings: unignore toggle, starred templates | Done | `unignoreActions` in schema + `lib.rs:98`; `STARRED_PREFIX` storage `lib.rs:22,323`, star icon `lib.rs:338` | - | closed since audit |
| 11 | custom ignore languages table | Missing | no code, no setting | B | a plugin cannot register languages at runtime |

## Properties -> properties

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | inspection Unused property | Done | command `find-unused`; live hints via setting `unusedProperties.live` (default false), `lib.rs:294 refresh_live_unused`, `scan_unused` `lib.rs:248` | - | still text search; keys built at run time look unused; live mode is opt-in |
| 2 | Add New Property File | Done | `fileTemplates` `properties` + `templates/properties.properties` (`plugin.toml`, commit a7ba491) | - | |
| 3 | Create Resource Bundle | Missing | no command | B | U2 file creation |
| 4 | Combine / Dissociate Resource Bundle | Missing | no code | B | IDE-internal grouping |
| 5 | find usages, rename, safe delete, move, copy | Partial | `definition` `lib.rs:526` (parent-locale chain, siblings, Java/Kotlin/html/jsp keys) | B | no usages or rename API (U1) |
| 6 | documentation provider | Missing | no hover provider | B | U1 |
| 7 | resource bundle editor | Partial | `customEditors` `bundle`; `views.rs:339 handle_bundle` edits the focused file only, `open_locale` `views.rs:322` opens another locale, `bundle_incomplete_only` filter | F* | other-locale cells cannot be written from the editor (README says so); code actions (quick fix "Remove property from all locales") do edit siblings |
| 8 | folding builder | Missing | no fold code | B | `properties` is a built-in language; folds are line-based |
| 9 | enter handler, join lines, mover, typed handlers | Missing | no code | B | no caret control |
| 10 | diff/merge resolver, structural search | Missing | no code | B | |
| 11 | goto related (same key in all locales) | Done | `lib.rs:562 bundle_siblings` appended to the definition result | - | multi-target UX unverified |

## Ideolog -> ideolog

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Highlight column / value / Remove highlightings | Partial | `patterns.rs:123 add_highlight` (regex, line/field/match, 6 colours) via the Patterns view | F* | not from the caret or selection; a command reading the editor selection could add the escaped text as a pattern |
| 2 | Hide / whitelist lines | Missing | `folding` `lib.rs:118` only folds stack traces | B | user state cannot drive folds; folds are line ranges, not hidden |
| 3 | time-gap highlighting | Done | setting `timeGapSeconds`, `log.rs:410 seconds` | - | ISO-like, Logcat, syslog, time-only stamps only (`log.rs:413`) |
| 4 | Show Ideolog settings | Partial | command `patterns` -> `host::reveal_view` (`lib.rs:80`); status item click | B | opens our view, not the IDE settings dialog |
| 5 | extend selection by field | Missing | no code | B | no selection control |
| 6 | error stripe heat map | Missing | no code | B | |
| 7 | console filters, command block highlighter | Missing | definition provider covers log files only | B | no console API |
| 8 | large file editor | Missing | highlights first 512 KiB only | B | |
| 9 | editor notification: detected format | Done | `lib.rs:56 show_format` sets status item `Log: <format>` on editor-focused/file-opened | - | no per-file banner or switch UI |
| 10 | settings UI (format table, patterns) | Partial | Patterns view `patterns.rs:83 render`, add via `input`/`pick`, remove via `pick` | F | no edit in place, no filters; `customPatterns` text setting still exists |

## Quarkus Tools -> quarkus

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Required properties | Done | `checks.rs:104 missing_required`, code `missing-required`, setting `quarkus.checkRequired` (opt-in), `@!required` in `data/quarkus-properties.tsv:2800` | - | Info severity, opt-in |
| 2 | Unassigned properties | Done | `java_checks.rs:57 unassigned`, code `unassigned-property`, setting `quarkus.checkUnassigned` | - | skips `quarkus.*`, `${}`, defaults, Optional |
| 3 | Java completion `@ConfigProperty`/`@ConfigMapping` | Done | `plugin.toml` java `completion`; `java.rs:177 completion_slot`, `java.rs:207 key_items` | - | |
| 4 | Java definition key <-> field | Done | `[[contributes.documentIndexers]]`, `indexer.rs` (definitions, references, markers both ways) | - | no `definition` provider; relies on the 1.2 indexer. Named-map member keys not linked (README) |
| 5 | hover | Partial | docs only in completion items | B | U1 |
| 6 | annotation diagnostics (REST client, scheduler, health, metrics, OpenAPI) | Partial | `java_checks.rs:75 rest_clients`, `:103 scheduled` (codes `rest-client-not-registered`, `invalid-cron`, `invalid-every`, `scheduled-without-trigger`, `scheduled-both-triggers`) | F* | no health, metrics or OpenAPI checks (grep: none) |
| 7 | diagnostics needing types | Missing | no code | B | U4 |
| 8 | Java quick fixes | Partial | `lib.rs:284 code_actions`: `Fix::DefaultValue`, `Fix::AddKey` only | F* | no `@RegisterRestClient`, `@Inject`, HealthCheck fixes |
| 9 | code lens | Missing | no code | B | |
| 10 | JAX-RS workspace symbols | Missing | no code | B | |
| 11 | inlay hints | Missing | no code | B | |
| 12 | Qute language | Missing | no `languages` entry in `plugin.toml`; README says it would replace built-in HTML | B | reclassed F* -> B: the host files are `*.qute.html`, `.html` is built in |
| 13 | Qute inspections, data model | Missing | no code | B | U4 |
| 14 | generate Qute template | Missing | no command | B | U2 |
| 15 | metadata beyond bundled | Partial | bundled 3.40 TSV only | B | jars unreadable |
| 16 | named-map keys | Done | `lib.rs:173 named_items`, `checks.rs:77` using `@!named` (`tsv:2799`), tests `lib.rs:667,683` | - | only roots the metadata lists as named |
| 17 | Dev Mode run config | Partial | 3 `runDetectors` (`plugin.toml`) | B | marker file names only, no wrapper, no attach |
| 18 | project wizard | Missing | only `open-code-quarkus` | B | U2 |

## GitLab -> gitlab

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Create Merge Request | Done | `forms.rs:19 MergeRequestForm` (title, description, target, assignee, reviewers, labels, draft, squash, remove source), `api.rs:569 create_merge_request`, view `new-merge-request`, `lib.rs:1323 reveal_view(NEW_MR)` | - | users typed as usernames, no picker |
| 2 | Review mode, inline discussions | Partial | `editorReview` markers, Changes page, `review.rs` | B | no diff viewer or gutter click |
| 3 | Mark Viewed / Not Viewed | Partial | `review.rs:216,485` checkbox in an Html page, storage `viewed_key` `review.rs:74` | F* | needs page scripts (README: blocked); a Tree/List view with a toggle button avoids them |
| 4 | Create Snippet | Partial | `forms.rs:140-144` form, `api.rs:479 create_snippet`; `lib.rs:1253` input fallback | F* | one file per snippet (README) |
| 5 | Share Project | Done | `forms.rs:236` form, `repo.rs:189 git init`, `:222 commit`, `:228 api::create_project`, `:237` push in a terminal | - | push is a terminal, not a task |
| 6 | Clone dialog | Partial | GitLab Projects view clones on double-click (`repo.rs:62`) | B | no clone-dialog extension |
| 7 | Accounts, OAuth, self-managed | Partial | `net:gitlab.com` only (`plugin.toml`), one `token` secret | B | fixed hosts |
| 8 | shell injection in `script:` | Missing | no code | B | |
| 9 | CI variables | Partial | `ci_data.rs` `CI_VARIABLES`, `ci.rs:574 completions` after `$` / `${` | F* | completion only; no resolve/hover; list can drift |
| 10 | `rules:if` language | Missing | no code | B | |
| 11 | CI schema, `!reference` | Partial | `ci_data.rs` `GLOBAL_KEYWORDS`/`JOB_KEYWORDS`, `ci.rs:574` | F* | keyword completion only; no value completion or validation; `!reference` unhandled (grep: none) |
| 12 | `include:` | Partial | `ci.rs:418 resolve_includes`, called `lib.rs:1911` (local files, depth 3, jobs and stages) | F* | remote/project/template/component includes stay unread (`has_include`) -> B |
| 13 | edit/delete note, award emoji | Done | `api.rs:746 edit_note`, `:750 delete_note`, `:766 add_award`, `:772 remove_award`; `lib.rs:989,995,1005`, setting-gated `note_actions_enabled` | - | |
| 14 | MR attribute editing | Done | `attributes.rs` (pick for title, description, labels, assignees, reviewers, milestone, target via `pick`/`pick_many`/`input`), `api.rs:718 update_merge_request` | - | |
| 15 | list filters | Done | `lib.rs:125-155` pick of dimension + `input` text; quick filters | - | |
| 16 | commit list, per-commit diffs | Done | view `merge-request-commits`, `api.rs:606 mr_commits`, `:631 commit_diff`, `review.rs:439` | - | read-only |
| 17 | pipeline and job details | Done | view `merge-request-pipeline`, `api.rs:611-627` (pipelines, jobs, retry, cancel), `lib.rs:1861` | - | |
| 18 | Markdown completion, upload | Missing | no code | B | |
| 19 | Open in Worktree | Partial | `lib.rs:1383 open_worktree` (git worktree add + notification) | B | no open-project API |
| 20 | annotate gutter | Missing | no code | B | |
| 21 | git HTTP auth provider | Missing | no code | B | |
| 22 | MR as editor tabs | Missing | tool windows only | B | |
| 23 | Open on GitLab menus | Done | `projectView.context` entries `open-in-browser`, `create-snippet` (`plugin.toml`), `lib.rs:1182 selected_paths` | - | log menus unavailable |

## Issue Trackers -> tasks

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | self-hosted servers | Missing | `net:` list fixed (`plugin.toml`) | B | |
| 2 | Bugzilla, Trac, Redmine, Mantis, Generic | Missing | `trackers.rs:32 TRACKERS` has 6 | B | |
| 3 | FogBugz, Lighthouse, Pivotal | Missing | not in `TRACKERS`; backlog P10 skipped | F* | services discontinued; low value |
| 4 | Go to Task, issue search | Done | commands `search-issues` (`lib.rs:428 input`), `Go to Issue` pick `lib.rs:454`, `switch` pick `lib.rs:86` | - | |
| 5 | several servers/repositories per type | Done | commands `add-repository`/`remove-repository`, `lib.rs:716-770`, `TaskList.repositories` (`model.rs:113`) | - | repository trackers only, same host |
| 6 | Create Changelist | Missing | branch per task | B | |
| 7 | Analyze Stacktrace | Missing | no code | B | |
| 8 | contexts save/load/clear | Done | commands `save-context`/`load-context`/`remove-context`, `lib.rs:650-700`, test `lib.rs:1138` | - | files only |
| 9 | check-in handler, commit placeholder | Partial | `copy-commit-message`, `fillCommitTemplate` | B | no commit dialog |
| 10 | issue navigation, documentation | Missing | no code | B | |
| 11 | Jira JQL language | Missing | no code | B | |
| 12 | certificate actions | Missing | no code | B | |

## dotenv -> dotenv

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | folding builder, hideValues | Missing | `SyntaxProvider` implements `highlights` only (`lib.rs:226`); wit wrapper `folding` `lib.rs:354` returns the empty default | B | dotenv is a plugin language so folds exist, but they are whole-line ranges and cannot hide a value inside a line |
| 2 | find usages + rename | Partial | `lib.rs:156 rename_usages` quick fix after editing | B | U1 |
| 3 | typed handler for `$` | Partial | completion provider, `_trigger` ignored (`lib.rs:246`) | B | U6 |

## dependency-audit (no rows in D-tooling.md; new rows against IntelliJ's Package Checker)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| DA1 | Maven `pom.xml` check | Done | `manifest.rs:105 kind`, `osv.rs`, literal and `${property}` versions | - | |
| DA2 | Gradle string coordinates | Done | `manifest.rs` Gradle kinds | - | |
| DA3 | npm / composer with lockfile | Done | `manifest.rs:123 lockfile` | - | |
| DA4 | catalog, yarn, pnpm, Cargo, PyPI | Done | `manifest.rs:60-65` globs, parsers `:635 yarn_lock`, `:700` Cargo/poetry, tests `:938-1010` | - | plugin.toml change-notes list only 7 files (see below) |
| DA5 | Upgrade quick fix | Partial | `lib.rs:342 code_actions` only for `Pom, Gradle, PackageJson, ComposerJson` | F | none for `libs.versions.toml` or `requirements.txt`, which are single-version edits |
| DA6 | Scan Project + view | Done | command `scan`, view `vulnerabilities`, `lib.rs:108 find_files(glob, 200)` | - | 200 files per glob |
| DA7 | ignored advisories setting | Done | `ignoredAdvisories` in schema | - | |
| DA8 | BOM / parent-managed and variable versions | Missing | README "Not checked" | B | needs build-tool resolution |

## Totals (88 old gap rows)

| plugin | Done | Partial | Missing | not Done: F | F* | B |
|---|---|---|---|---|---|---|
| ignore (11) | 1 | 6 | 4 | 0 | 2 | 8 |
| properties (11) | 3 | 2 | 6 | 0 | 1 | 7 |
| ideolog (10) | 2 | 3 | 5 | 1 | 1 | 6 |
| quarkus (18) | 5 | 5 | 8 | 0 | 2 | 11 |
| gitlab (23) | 8 | 9 | 6 | 0 | 5 | 10 |
| tasks (12) | 3 | 1 | 8 | 0 | 1 | 8 |
| dotenv (3) | 0 | 2 | 1 | 0 | 0 | 3 |
| total (88) | 22 | 28 | 38 | 1 | 12 | 53 |

Old class counts were F 9, F* 26, B 53. Now F 1, F* 12, B 53 (13 rows still open are F/F*; 22 closed). Class moves: quarkus #12 F* -> B, gitlab #4 F -> F*, ideolog #1 B -> F*. dependency-audit: 6 Done, 1 Partial, 1 Missing (8 rows, not in the 88).

## Rows that changed vs the old audit

Closed (22): ignore #10; properties #1, #2, #11; ideolog #3, #9; quarkus #1, #2, #3, #4, #16; gitlab #1, #5, #13, #14, #15, #16, #17, #23; tasks #4, #5, #8.
Moved up but still open: ignore #3 (menu + pick, fragile), #7 (path completion); ideolog #1, #4, #10 (Patterns view); gitlab #3, #9, #11, #12 (CI keywords, CI_* vars, local includes), #4; properties #7 (incomplete filter, open locale); quarkus #6, #8 (Java checks, two fixes), #15 unchanged.
Class changes: quarkus #12 F* -> B; gitlab #12 F* with a B tail (remote includes); gitlab #4 F -> F*; ideolog #1 B -> F*, #10 F* -> F; dotenv #1 B stays B (folds are line ranges); tasks #3 F* but low value.
Unchanged Missing: 38 rows, 35 of them B.

## Claims the code does not back, and stale claims

- gitlab README "Limits" (`README.md:223`): "included files are not followed". Code follows local includes (`ci.rs:418`, `lib.rs:1911`). Stale, understates.
- gitlab README intro says "clones and shares projects" and the row 6 note: clone is a double-click in a view, share ends in a terminal push, not an in-IDE push.
- dependency-audit `plugin.toml` change-notes list 7 manifests; code and README handle 12 (catalog, yarn, pnpm, Cargo, requirements.txt, poetry). Release notes understate.
- ignore `add_selected` (`lib.rs:155`) returns guidance text that api-facts says the host drops; if still dropped, the "open the file, then run again" path shows the user nothing. Needs an IDE check or a `host::notify`.
- quarkus README "Not ported ... go to definition": the Java-side language server part is meant, but Go to Declaration config <-> Java is implemented by the indexer (`indexer.rs`); wording is confusing. Backlog "P5 key to declaration done" is correct.
- backlog-tooling.md lists P4 as DONE for `quarkus.checkRequired`; it is opt-in and Info severity, so the audit's "Required properties" is only conditionally on.
- properties `unusedProperties.live` is off by default; the "live inspection" only exists after the user turns it on.
- Spot checks of old Done rows (dotenv 11 inspections `lint.rs`, ignore inspections `check.rs`, gitlab 4 CI inspections `ci.rs`) found them present; not a full re-audit.

## The 10 best next rows

1. gitlab #3 Viewed state as a Tree/List view toggle (state already in `review.rs:74`; removes the script dependency). F*.
2. gitlab #11 CI value completion and `!reference` for `.gitlab-ci.yml` (keywords exist in `ci_data.rs`). F*.
3. quarkus #8 + #6 add `@RegisterRestClient` and similar quick fixes, health/metrics checks next to `java_checks.rs:75`. F*.
4. ideolog #1 "Highlight selection" command reusing `patterns.rs:123 add_highlight` with the editor selection. F*.
5. ideolog #10 edit and reorder patterns in the Patterns view (uses `input`). F.
6. dependency-audit DA5 upgrade fix for `libs.versions.toml` and `requirements.txt`. F.
7. ignore #3 notify instead of a dropped return string, add the unignore command and an `editor.context` entry. F*.
8. properties #7 write other locales from the bundle editor if the host allows `apply_edits` on a non-focused file, else keep. F*.
9. gitlab #4 multi-file snippet from selected paths (`selected_paths`) with a Form. F*.
10. ignore #4 one `fileTemplates` entry per ignore type, after testing whether a dot-name template survives the New menu. F*.
