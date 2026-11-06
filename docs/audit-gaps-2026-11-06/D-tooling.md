# Group D audit: tooling plugins

Evidence is from `plugins/<name>` (`plugin.toml`, `README.md`, `src/*.rs`), `LIMITATIONS.md` and the open-licence sources under `the local scratch copy` and `the local scratch copy*/dec`. Counts of original features are by inventory line plus the notable extension points; "rows" in the gap tables group near-identical features.

## .ignore (7495) -> ignore

Original 21 features (5 inspections, 6 actions/groups, 10 extension points) / Done 10 / Partial 7 / Missing 4. Gap rows 11: F 1, F* 3, B 7.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | inspection Cover entry | warns when an entry is already matched by another entry | Partial: `cover-entry` in `check.rs:160` compares patterns only; README: only single-path entries and globs in a literal dir are checked | B | U3 (find_files skips ignored paths, so the file-matching part can't run) |
| 2 | action Close Ignored Editors | closes every open tab whose file is ignored | Missing (change-notes list it as not ported) | B | no close-editor call in the API facts |
| 3 | actions Ignore file / Unignore file groups | project-view and editor menu to add the selected file(s) to a chosen ignore file | Partial: only the "Add to .gitignore" / unignore code actions inside a file (`lib.rs:76-85`); no `projectView.context` menu in `plugin.toml` | F* | menus `projectView.context` + command; risk: unverified that a command receives the selected paths; no quick-pick for choosing the target file (U5), so root `.gitignore` only |
| 4 | action New ignore file group + generator dialog | New menu entry per ignore type (.gitignore, .dockerignore, .npmignore...), dialog to pick several templates | Partial: one file template `gitignore` creating `<name>.ignore` (`plugin.toml` fileTemplates); dot-named files can't be created from New | F* | fileTemplates (one per type); risk: whether a template can yield a bare dot-name is unverified; multi-select dialog is U5 |
| 5 | syntax highlighting for the 33 ignore file types | colours comments, negation, globs | Partial: only `dotignore` language highlights; built-in `ignore` language can't take plugin highlighting (README) | B | U6 (syntax for built-in languages) |
| 6 | line marker provider (directory entries) | folder icon on entries naming a directory | Missing | B | line-marker icon allowlist is navigate/implemented/implementing/run (no folder icon) |
| 7 | reference contributor + element manipulator (path references) | entries resolve to files; Ctrl-click, path completion | Partial: definition opens a file when the entry names exactly one (README); no path completion (README "not ported") | F* | completion provider + find_files; risk: find_files hides ignored and hidden paths, which are what ignore files name (U3) |
| 8 | VCS file status provider | shows ignored files in the ignored colour in Project view and tabs | Partial: status bar item "Ignored" for the focused file | B | Project-view decorations / editor-tab decorations not available |
| 9 | hide ignored files in Project view (setting) | filters ignored files out of the tree | Missing (change-notes) | B | Project-view decorations |
| 10 | settings: unignore actions toggle, starred templates | extra toggle and favourite templates | Partial: schema has `missingGitignore`, `ignoredEditing`, `insertAtCursor`; user templates carry a star icon (`lib.rs:208`) but starring isn't a setting | F | scalar settings + storage |
| 11 | settings: custom ignore languages table | user-defined ignore file types (name, filename) | Missing | B | settings are scalar types only |

Ours beyond the original: the Ignore Templates tool window, github/gitignore templates embedded at build time, `dotignore` highlighting for unrecognised ignore files.

## Properties (11594) -> properties

Original 23 features (6 inspections, 2 intentions, 4 actions, 1 template, 10 extension-point groups) / Done 11 / Partial 4 / Missing 8. Gap rows 11: F 1, F* 3, B 7.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | inspection Unused property | flags keys nothing outside the file references | Partial: `Properties: Find Unused Properties` command (`plugin.toml`), searches other files by text on demand and fades keys; not a live inspection | F* | diagnostics from `file-saved`/`scheduled`; risk: whole-project text scan cost and false positives for keys built dynamically |
| 2 | action Add New Property File | New menu entry for a `.properties` file | Missing: only `xml-properties` template in `plugin.toml` | F | fileTemplates (extension `properties`) |
| 3 | action Create Resource Bundle | dialog that creates one file per locale | Missing | B | U2 (file creation), U5 (dialog) |
| 4 | actions Combine Properties Files / Dissociate Resource Bundle | merges files into, or splits them out of, a virtual bundle | Missing | B | bundles are an IDE-internal grouping; no API |
| 5 | find usages, rename, safe delete, move, copy, automatic renamers (about 10 extension points) | refactor a key across bundle and code | Partial: Go to Declaration to parent locale / declarations of the key (README); no usages or rename | B | U1 |
| 6 | documentation provider | quick doc for a key with its translations | Missing | B | U1 (hover) |
| 7 | resource bundle editor (full) | edit every locale, add/remove locales, show incomplete keys | Partial: `bundle` custom editor; other locales read-only (README) | F* | customEditors; risk: writing sibling files from an editor is unverified |
| 8 | folding builder | folds long/multi-line property text | Missing | B | folding control not available |
| 9 | enter handler, join lines, statement mover, typed handlers | line-continuation aware editing | Missing | B | caret control not available |
| 10 | diff/merge resolver, structural search profile | semantic merge of properties, search templates | Missing | B | no diff or structural search API |
| 11 | goto related provider | jumps to the same key in all locales | Partial: only the parent-locale chain via definition | F* | definition provider returning several locations; risk: unverified multi-target UX |

Ours beyond the original: key completion and missing-key checks in Java/Kotlin/Thymeleaf/JSP code, slf4j `simplelogger.properties` completion, `Properties: Reformat File`, invalid `\u` escape check (`InvalidUnicodeEscape`).

## Ideolog (9746) -> ideolog

Original 27 features (13 intentions, 4 actions, 10 extension-point groups) / Done 8 / Partial 1 / Missing 18. Gap rows 10: F 0, F* 3, B 7.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | intentions Highlight column / Highlight value / Remove highlightings (3) | user-driven colouring of a field or value | Missing (README "not ported"); only fixed level colouring and custom patterns | B | editor decorations on demand |
| 2 | intentions Hide this / Hide above / Hide below / Hide containing / Hide not containing / Whitelist this / Reset hidden (7) | hides log lines by position or text | Missing | B | folding control not available (a filtered Html view would be a different feature) |
| 3 | intention Toggle time highlighting, time-gap highlighting | marks events far apart in time | Partial: time column colour only | F* | highlighting is computed in the plugin's own syntax component; risk: timestamp parsing for every format inside the 50 ms budget |
| 4 | intention Show Ideolog settings | opens the settings dialog | Missing | B | U5 (opening settings/views from a command) |
| 5 | extend word selection by log field | selection grows to field/column | Missing | B | selection control not available |
| 6 | error stripe heat map | density of errors on the scrollbar | Missing | B | editor decorations |
| 7 | console filter providers, command block highlighter | stack links and colours in Run console and terminal | Missing; links only in log files (`definition` provider, `plugin.toml`) | B | no console/terminal output API (streaming processes) |
| 8 | large file editor + size checker | opens huge logs in a lightweight editor | Missing; ours highlights only the first 512 KiB | B | no editor-provider API for large files |
| 9 | editor notification (detected format, change format) | banner naming the format used | Missing | F* | status item (`events`) with the detected format; risk: no per-file banner or switch UI |
| 10 | settings UI (format table, highlighting patterns, filters) | table editors with dialogs | Partial: one `customPatterns` text setting (`settings.schema.json`) | F* | Table/Form views; risk: scalar-only settings, so a separate view is needed |

Ours beyond the original: Go to Next Warning / Next Exception commands, more built-in formats (Laravel/Monolog, Serilog, nginx, syslog, Loguru...), stack-frame links for Python/.NET/PHP/Ruby/Dart/Swift/Node/Go/Rust.

## Quarkus Tools (13234) -> quarkus

Original 35 features (13 inspections, 6 actions, 16 extension-point groups) / Done 9 / Partial 2 / Missing 24. Gap rows 18: F 0, F* 8, B 10.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | inspection Required properties | reports required keys absent from the file | Missing; README: bundled metadata doesn't say which are required | F* | regenerate metadata with defaults/required info; risk: false positives for values supplied by env or other sources |
| 2 | inspection Unassigned properties | `@ConfigProperty` in Java whose key no properties file sets | Missing; `java.rs` already reads the declarations | F* | diagnostics from a Java `file-saved`; risk: values from env, system properties, profiles, defaults |
| 3 | Java completion for `@ConfigProperty(name=)` / `@ConfigMapping(prefix=)` | offers known keys inside the annotation | Missing (`plugin.toml`: java only has `line-markers`) | F* | completion provider for `java`; risk: text-only context detection, 50 ms budget |
| 4 | Java definition (key <-> field) | jump between properties key and Java declaration | Missing (README relies on the IDE) | F* | definition provider on `properties`/`yaml`/`java`; risk: only project sources, unverified overlap with the IDE |
| 5 | Java and properties hover | shows key docs and defaults on hover | Partial: docs only in completion items | B | U1 (hover) |
| 6 | Java diagnostics, annotation-level (REST client missing `@RegisterRestClient`/`@Inject`, scheduler cron syntax, health annotations, metrics, OpenAPI) | validates annotations in Java | Missing | F* | text parser in `java.rs` plus `publish_diagnostics`; risk: false positives without type info |
| 7 | Java diagnostics needing types (HealthCheck implementor, fault-tolerance fallback signatures, inheritance) | semantic annotation checks | Missing | B | U4 |
| 8 | Java quick fixes (add annotation, implement HealthCheck, insert default value, add key to properties) | one-click repairs | Missing | F* | code-actions editing existing files; risk: import insertion by text |
| 9 | Java code lens (JAX-RS URLs, REST client, scheduler) | lens above endpoints | Missing | B | code vision not available |
| 10 | JAX-RS workspace symbols | endpoints in Go to Symbol | Missing | B | no symbol provider API |
| 11 | Java inlay hints (config values inline) | inline resolved values | Missing | B | inlay hints not available |
| 12 | Qute templates: language, highlighting, section completion | file type for `.qute.*` templates | Missing | F* | `languages` contribution with syntax; risk: multi-dot extensions and html/txt/json host languages |
| 13 | Qute inspections (undefined object, namespace, section tag, global) and data-model completion/definition | type-aware template checks | Missing | B | U4 (Java type resolution) plus injection |
| 14 | Qute: generate template file command | creates a template for a method | Missing | B | U2 |
| 15 | extension metadata beyond the bundled ~170 and dependency jars | keys of any extension on the classpath | Partial: bundled Quarkus 3.40.1 metadata (README) | B | jars can't be read (LIMITATIONS "No metadata from libraries") |
| 16 | named-map keys (`quarkus.datasource."name".*`) completion | per-name groups | Missing (README) | F* | completion logic over metadata; risk: ambiguous map key patterns |
| 17 | Dev Mode run configuration, debugger attach, run dashboard, console links | runs `quarkus:dev` only in Quarkus modules | Partial: 3 run detectors by marker file name (`plugin.toml`); show in every Maven/Gradle project, no wrappers, no attach | B | detectors match names only, `process:` programs fixed, no debugger API |
| 18 | project wizard, facets, framework detection | creates and configures Quarkus projects | Missing; command opens code.quarkus.io | B | U2 (file creation) and U5 (dialogs) |

Ours beyond the original: separate settings per check group (`quarkus.checkSyntax`, `checkValues`, `checkExpressions`).

## GitLab (22857) -> gitlab

Original 39 features (4 inspections, 1 intention, 22 actions, 12 extension-point groups) / Done 16 / Partial 11 / Missing 12. Gap rows 23: F 7, F* 6, B 10.

Actions by theme: merge request (Show.List, List.Refresh, Refresh, Show, CheckoutBranch, Open.Link, Copy.Link, Timeline.ShowEvents, Show.In.Toolwindow, Review.Submit) 10 Done; Open in Worktree, Create, Branch.Update, Review.Mode.Toggle, Changes.MarkViewed/MarkNotViewed 6 Partial; clone, accounts/settings, share, snippet 4 Partial; CI shell-injection action and clear-variables action 2 Missing. The four CI inspections are Done (`ciJobReferences`, `ciStages`, `ciDuplicateJobUsage`, `ciSingleQuotedVariables` in README).

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | action Create Merge Request | in-IDE form: title, description, branches, assignees, reviewers, labels, squash/delete options | Partial: `create-merge-request` opens gitlab.com's new-MR page | F | Form view + REST `POST merge_requests` |
| 2 | action Review Mode toggle, Update to Enable Review Mode | opens MR changes in the IDE diff viewer with inline discussions and gutter comments | Partial: unresolved threads as gutter markers/Info problems (`editorReview`); Changes view in an Html page; `update-merge-request-branch` is a server-side rebase | B | no diff viewer, gutter click or editor decorations |
| 3 | actions Mark Viewed / Not Viewed | per-file viewed state | Partial: works in the Changes page only and needs scripts, which are blocked (README) | F* | Tree view of files with a toggle button; risk: loses the diff layout |
| 4 | action Create Snippet | dialog: title, description, visibility, project or personal, several files | Partial: private snippet from selection or file, no fields | F | Form view + REST snippets |
| 5 | action Share Project | dialog: name, namespace, description, remote, visibility; init, commit, push | Partial: private project, no `git init`/commit (README) | F | Form view + `process:git` |
| 6 | action Clone Repository (clone dialog extension) | clone tab in the VCS clone dialog with search | Partial: GitLab Projects view, `git clone` in a terminal | B | no clone-dialog extension point |
| 7 | action Manage Accounts, OAuth, self-managed servers | several accounts, OAuth login, any GitLab host | Partial: one PAT, gitlab.com only (README Limits) | B | `net:` hosts and secrets fixed in the manifest; no OAuth callback |
| 8 | action/intention Inject Shell Script, shell injection settings | shell highlighting inside `script:` blocks | Missing | B | language injection |
| 9 | CI env var references, completion, clear-downloaded-data action | knows predefined `CI_*` variables, completes and resolves `$VAR` | Missing | F* | completion provider on `yaml` with a bundled variable list; risk: list drifts from GitLab versions, `$` trigger is U6 |
| 10 | CI `rules:if` expression language | highlighting and completion of expressions | Missing | B | injection / partial-file syntax |
| 11 | CI schema support, `!reference` tag recognition | keyword completion and validation of `.gitlab-ci.yml` | Missing (README "not ported") | F* | bundled keyword completion; risk: full validation is large; `!reference` handling by the IDE's YAML is unverified |
| 12 | CI `include:` | follows includes for jobs and stages | Partial: checks go quiet when `include:` is present (README) | F* | read_file of local includes; risk: remote/project/template includes need `net:` (B) |
| 13 | note edit/delete, award emoji | edit or delete own notes, reactions | Missing | F | REST PUT/DELETE/award_emoji with buttons |
| 14 | MR attribute editing (labels, assignees, reviewers, milestone, title, description, target branch) | edits MR metadata | Partial: only `set_reviewers` for "Add Me as Reviewer" (`api.rs:496`) | F* | REST calls; risk: picking from lists with Form fields is unverified |
| 15 | list filters | filter by author, label, reviewer, text | Partial: five quick filters (README) | F* | query string from a Form view; risk: UI for typed filters |
| 16 | commit list and per-commit diffs in MR | choose a commit to review | Missing (README Limits) | F | REST commits/diffs + view |
| 17 | pipeline and job details in MR | jobs list, status, retry | Partial: pipeline status in the status line (`lib.rs:156`) | F | REST pipelines/jobs + buttons |
| 18 | Markdown completion (@mention, #issue, emoji), paste-image upload, Upload File action | rich note input | Missing (README: plain text) | B | clipboard read, no completion in form fields, U5 file chooser |
| 19 | Open in Worktree | opens the worktree as a project | Partial: creates the worktree, only a notification (README) | B | no open-project API |
| 20 | annotate gutter provider | blame line to MR | Missing | B | annotation gutter not available |
| 21 | git HTTP auth data provider | supplies the token to git over HTTPS | Missing | B | secrets only substituted in `net` headers; not available to `process:git` |
| 22 | MR/diff as editor tabs (virtual file system, tab title provider) | MR content opens in editors | Missing; tool windows instead | B | U5 (open a view from a command), no virtual file system |
| 23 | Open on GitLab menus | open file/commit in browser from Project view and VCS log | Partial: editor and editor tab menus only (`plugin.toml`) | F | add `projectView.context` entry (log menus not available) |

Ours beyond the original: Apply suggestion quick fix, Open Merge Request in Worktree runs without a dialog, status-bar "Create Merge Request" prompt.

## Issue Trackers (11545) -> tasks

Original 34 features (14 actions, 14 tracker types, 6 other extension points) / Done 5 / Partial 13 / Missing 16. Gap rows 12: F 0, F* 3, B 9.

Tracker types. Original (14): GitHub, GitLab, Forgejo/Gitea, YouTrack, Jira, Trello, Bugzilla, Trac, Redmine, FogBugz, Mantis, Lighthouse, Pivotal Tracker, Generic (configurable REST). Ours (6, `trackers.rs` TRACKERS): GitHub (api.github.com), GitLab (gitlab.com), Codeberg (Forgejo, codeberg.org only), YouTrack Cloud (`*.youtrack.cloud`), Jira Cloud (`*.atlassian.net`), Trello. Not in ours: the other 8 and every self-hosted server.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | self-hosted servers of GitHub, GitLab, Forgejo, YouTrack, Jira (Server/DC) | any server URL per tracker | Missing: hosts are fixed (`plugin.toml` `net:` list) | B | `net:` hosts fixed in the manifest |
| 2 | tracker types Bugzilla, Trac, Redmine, Mantis, Generic | user-hosted trackers and a configurable REST/regex tracker | Missing | B | `net:` hosts fixed in the manifest |
| 3 | tracker types FogBugz, Lighthouse, Pivotal Tracker | vendor-hosted trackers | Missing | F* | fixed or wildcard vendor hosts; risk: services believed discontinued, unverified |
| 4 | action Go to Task, issue search | popup with server-side search | Partial: Tasks tool window list, settings queries (`youtrackQuery`, `jiraQuery`) | B | U5 (quick-pick/input) |
| 5 | action Configure Servers (several servers per type) | several repositories of one type | Partial: one repository per tracker (`githubRepository`, `gitlabProject`, ...) | F* | numbered scalar settings; risk: clumsy |
| 6 | action Create Changelist | changelist per task | Missing: ours uses a git branch per task | B | no changelist API |
| 7 | action Analyze Stacktrace | opens a stack trace from the task description in the analyzer | Missing | B | clipboard read / no analyzer API |
| 8 | actions Context save / load / clear | named contexts with open files, breakpoints, bookmarks | Partial: automatic last 10 files per task (README) | F* | storage + `open_location`; risk: files only, no editor state |
| 9 | check-in handler, commit placeholder provider, changelist decorator | task id and time into the commit dialog, close issue on commit | Partial: Copy Commit Message, `commit.template` | B | no commit dialog or pre-commit hook |
| 10 | issue navigation, issue documentation | clickable issue ids in commit messages and editors, hover | Missing | B | U1 |
| 11 | Jira JQL language | highlighting and completion for the query field | Missing | B | no editor for a settings field |
| 12 | actions Show Certificate, deadlock with certificate dialog | TLS certificate acceptance | Missing | B | host controls TLS |

Ours beyond the original: work time posted to YouTrack and Jira; Trello support with key and token in one header; auto refresh every 20 minutes.

## dotenv (public source the public source) -> dotenv

Original 20 features (11 inspections, 9 extension-point groups) / Done 16 / Partial 3 / Missing 1. Gap rows 3: F 0, F* 0, B 3. All 11 inspections are Done (table in README, `lint.rs`); completion, go to declaration, `${...}` references, highlighting, comments and settings are Done.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | folding builder + setting `hideValuesInTheFile` | folds every value so secrets stay hidden | Missing; schema has `completion` and `showValues` only | B | folding control not available |
| 2 | find usages + rename of keys (searchers, symbol rename) | usages window and rename refactoring | Partial: definition from key to usages, and a "Rename usages of OLD to NEW" quick fix after editing (README) | B | U1 |
| 3 | typed handler for `$` / `${` | opens completion as soon as `$` is typed | Partial: popup opens on the letters after `$` (README) | B | U6 (trigger characters) |

Ours beyond the original (current upstream): Docker and compose declarations (`env_file`), PHP, PHPUnit, Go, Ruby, Rust, C# usages, Kotlin/Java/Python/JS, and the rename quick fix.

## Group summary

| Pair | Original features | Done | Partial | Missing | Gap rows | F | F* | B |
|---|---|---|---|---|---|---|---|---|
| .ignore -> ignore | 21 | 10 | 7 | 4 | 11 | 1 | 3 | 7 |
| Properties -> properties | 23 | 11 | 4 | 8 | 11 | 1 | 3 | 7 |
| Ideolog -> ideolog | 27 | 8 | 1 | 18 | 10 | 0 | 3 | 7 |
| Quarkus Tools -> quarkus | 35 | 9 | 2 | 24 | 18 | 0 | 8 | 10 |
| GitLab -> gitlab | 39 | 16 | 11 | 12 | 23 | 7 | 6 | 10 |
| Issue Trackers -> tasks | 34 | 5 | 13 | 16 | 12 | 0 | 3 | 9 |
| dotenv -> dotenv | 20 | 16 | 3 | 1 | 3 | 0 | 0 | 3 |
| Total | 199 | 75 | 41 | 83 | 88 | 9 | 26 | 53 |
