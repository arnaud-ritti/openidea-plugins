# Reclassification of group D (tooling) against API 1.2

Scope: the 79 B and F* rows of [D-tooling.md](D-tooling.md) (F* 26, B 53). Judged against the 1.2 README (`/tmp/oi/plugins/README.md`), `/tmp/oi/wit` and the current code in `plugins/{ignore,properties,ideolog,quarkus,gitlab,tasks,dotenv}`. Row ids are `pair #row` as in D-tooling.md.

1.2 facts used (README host API table and sections): `host::pick/pick_many/input/confirm` return the choice and "only from a command or event of a window (else `invalid`)"; `ctx.selected_paths()` for Project view and editor tab menus; `viewItem.context` menus need the plugin's own view; `reveal_view` brings one of the plugin's own `[[contributes.views]]` to front; `set_context` only feeds `when`; `workspaceContains:` is an exact path and only gates commands and menus; document indexers publish definitions, references, aliases and gutter markers (go to declaration plus markers, no usages list, no rename), and `find_files` still "honours `.gitignore`". Nothing in 1.2 creates files, opens projects, reads the clipboard or adds decorations.

## Rows that changed class, or whose state the code changed

| Row | Old | New | What changes it | Sketch | Size |
|---|---|---|---|---|---|
| ignore #3 Ignore / Unignore file groups | F* | F | `selected_paths()` + `projectView.context` menu. Already built in `ignore` 0.2.0 (`lib.rs:120`, "Add Selected Paths to the Open Ignore File"). | Remaining: `pick` the target ignore file instead of "the open one"; list candidates with `find_files("**/.gitignore")` (risk: dot files may be skipped, fall back to root `.gitignore`). | S |
| gitlab #3 Mark Viewed / Not Viewed | F* | F | Not 1.2: already built (README "Viewed collapses a file", remembered per commit). | none, done | - |
| gitlab #14 MR attribute editing | F* | F | `pick`/`pick_many`/`input` from a view button click (view events are window events; unverified that Form button clicks count, gitlab already prompts from a command). The old risk was "picking from lists with Form fields". | Buttons Edit labels / assignees / reviewers / milestone / title / target branch: GET project labels, members, milestones, `pick_many`, `PUT merge_requests/:iid`; `input` for title. | M |
| gitlab #15 list filters | F* | F | `input` for text, `pick` for author / label / reviewer from REST lists (quick filters already exist). | "Filter..." button: `pick` the dimension, `pick` or `input` the value, add to the query string, refresh. | S |
| tasks #4 Go to Task, issue search | B | F | U5 quick-pick/input is now `pick` + `input`. Switch Task already uses `pick` (`tasks/src/lib.rs:86`). | Command "Tasks: Search Issues": `input` query, tracker search call, `pick` results, create or activate the local task. | S |
| tasks #5 Configure Servers (several per type) | F* | F | `input` + `pick` + `storage` replace "numbered scalar settings". Hosts stay fixed, so this means several repositories on one host, not several servers. | Stored list of repos per tracker; commands Add (`input`), Remove (`pick`), Select (`pick`). | M |
| tasks #8 Context save / load / clear | F* | F | `input` names a context, `pick` chooses one. Files only (no breakpoints or bookmarks), as before. | Save: `input` name, store open file list; Load: `pick`, `open_location` each; Clear: `pick` + `confirm`. | S |

## Rows reviewed because 1.2 looks relevant, unchanged

| Row | Old | New | Why not |
|---|---|---|---|
| ignore #4 New ignore file group + dialog | F* | F* | `pick_many` covers the dialog, but creating a bare dot-named file is still only the unverified fileTemplates path (U2). |
| ignore #7 path completion | F* | F* | Still U3: `find_files` honours `.gitignore` and hides dot dirs. |
| ignore #11 custom ignore languages table | B | B | Languages are manifest-contributed; `input` cannot register a filename at run time. |
| properties #3 Create Resource Bundle | B | B | `input`/`pick` give the dialog, but a new locale file is still U2 (file creation). |
| properties #5 usages, rename, safe delete | B | B | Indexers give go to declaration and markers only; no usages window, no rename (U1). |
| properties #11 goto related (all locales) | F* | F* | A document indexer (definitions per key, references per locale) is now a clean route and would answer without the 50 ms budget; multi-target UX still unverified. Same class. |
| ideolog #4 Show Ideolog settings | B | B | `reveal_view` reveals the plugin's own view, not the IDE settings dialog. It only becomes F* once #10 (a settings Table/Form view) exists. |
| ideolog #10 settings UI (tables) | F* | F* | Form/Table view still needed; `input`/`pick` ease row edits but the risk is the same. |
| quarkus #4 key <-> Java definition | F* | F* | A document indexer over `.properties`, `.yaml` and `.java` is now the right route (anchors `prefix.key` in both). Needs the host syntax symbols for Java; `@ConfigProperty` detection stays text-based. Larger win than before, same class. |
| quarkus #17 Dev Mode run config | B | B | `workspaceContains:` is exact path only; detectors still match marker names and can't see Quarkus in a pom. |
| quarkus #18 project wizard | B | B | `input`/`pick` give the form, but no file creation and no open-project / pick-directory. |
| gitlab #6 Clone dialog | B | B | The clone view and `git clone` exist; the VCS clone dialog extension point does not. |
| gitlab #18 Markdown completion, paste, upload | B | B | No clipboard read, no file chooser (`pick` lists plugin-given items only). |
| gitlab #19 Open in Worktree | B | B | No open-project API; `confirm`/`pick` don't change that. |
| gitlab #22 MR as editor tabs | B | B | `reveal_view` fronts a tool window; no virtual file system or tab provider. |
| tasks #1, #2 self-hosted / other trackers | B | B | `net:` hosts fixed in the manifest. |
| tasks #7 Analyze Stacktrace | B | B | `input` could take a pasted trace, but there is no analyzer to hand it to. |

## Remaining B and F* rows with no 1.2 feature (unchanged: U-code from api-facts.md)

- ignore: #1 B U3; #2 B (no close-editor call); #5 B U6; #6 B (gutter icon allowlist); #8, #9 B (Project view decorations).
- properties: #1 F* (diagnostics from scan, cost); #4 B (bundle grouping); #6 B U1; #7 F* (writing sibling files from an editor); #8 B folding; #9 B caret; #10 B (no diff / structural search API).
- ideolog: #1, #6 B editor decorations; #2 B folding; #3 F* (timestamp parsing budget); #5 B selection control; #7 B streaming / console; #8 B (no large-file editor); #9 F* (status item only).
- quarkus: #1, #2, #3, #6, #8, #12, #16 F* (risks as before); #5 B U1; #7, #13 B U4; #9 B code vision; #10 B (no symbol provider); #11 B inlay hints; #14 B U2; #15 B (jars unreadable).
- gitlab: #2 B (no diff viewer); #7 B (`net:` / secrets fixed); #8, #10 B injection; #9, #11, #12 F* (completion `$` trigger U6; schema size; remote includes need `net:`); #20 B annotate gutter; #21 B (secrets not available to `process:git`).
- tasks: #3 F* (vendors possibly discontinued); #6 B changelists; #9 B (no commit dialog); #10 B U1; #11 B (no editor for settings field); #12 B (host controls TLS).
- dotenv: #1 B folding; #2 B U1; #3 B U6.

## Summary

| | F* -> F | B -> F | B -> F* | F* -> B | unchanged |
|---|---|---|---|---|---|
| Rows | 6 (ignore #3, gitlab #3, #14, #15, tasks #5, #8) | 1 (tasks #4) | 0 | 0 | 72 |

Of the 7 flips, 2 are not 1.2 effects: gitlab #3 was already built, and ignore #3 is built in 0.2.0 apart from the target `pick`. New F total in group D: 9 -> 16 (ignore #3 and gitlab #3 are done, 5 open). F*: 26 -> 20. B: 53 -> 52. The 1.2 API mostly pays off in tasks (dialogs) and gitlab (form pickers); the dominant blockers (U1 references and rename, U2 file creation, U3 ignored files, U4 type resolution, decorations, folding, injection) are untouched.

Best 5 to implement:
1. tasks #4 issue search (`input` + `pick`, S): turns a B into a working feature with code that already uses `pick`.
2. gitlab #14 MR attribute editing (M): the biggest functional gap left in GitLab (only "Add Me as Reviewer" today).
3. gitlab #15 typed list filters (S): reuses the same REST list fetches as #14.
4. tasks #8 named contexts (S): `input` + `pick` + storage + `open_location`, no new capability.
5. tasks #5 several repositories per tracker (M); alternative if a larger payoff is wanted: quarkus #4 / properties #11 via a document indexer (L, still F*, needs verification of the indexer on Java).
