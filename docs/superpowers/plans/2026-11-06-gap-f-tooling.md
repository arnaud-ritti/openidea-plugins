# Gap F rows of the tooling plugins Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the F-class gap rows of `gitlab` (6 of 7 after re-check), `ignore` (1) and `properties` (1): five GitLab form/view features, GitLab note and pipeline actions, two `ignore` settings, one `properties` New-menu entry.

**Architecture:** GitLab: new `Form`, `List` and `Tree` views (the user opens them; no command can open a view) over new REST calls in `api.rs`, plus one `safe.rs` module that validates every value that reaches a URL path, a query string, a JSON id or a `git` argument. `ignore`: one boolean setting and a star toggle stored in `host::storage`. `properties`: a manifest `fileTemplates` entry. No new API is assumed.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK 0.1.37 (`views::{Form, Field, FieldValue::{Text,Multiline,Checkbox,Choice}, Button, Tree, List}`), `MockHost` tests (`on_fetch`, `on_process`, `set_secret`, `requests`, `process_runs`, `terminals`, `notifications`).

**Spec:** [`docs/audit-gaps-2026-11-06.md`](../../audit-gaps-2026-11-06.md) (appendix: `.ignore`, `Properties`, `GitLab`), [`docs/audit-gaps-2026-11-06/D-tooling.md`](../../audit-gaps-2026-11-06/D-tooling.md) (rows by plugin), [`api-facts.md`](../../audit-gaps-2026-11-06/api-facts.md), `PARITY.md`, `LIMITATIONS.md` (`aritti.gitlab`, _Project view selection_).

## Rulings (re-check of every F row against the code and the API)

| Row | Audit class | Ruling |
|---|---|---|
| gitlab 23 Open on GitLab menus | F | **Downgraded to B.** `run_command(id, ctx, arg)` gets only the window's project and the focused editor; `projectView.context` commands get no selection (`LIMITATIONS.md` lines 832-838, 1202; upstream `invocation.selected-paths()`, `l0ng-ai/openidea` #1744). A menu entry would open the link of the focused editor's file, not of the clicked node. No task. |
| gitlab 1 Create Merge Request | F | Kept, **narrowed**: Form view with title, description, target branch (Choice of the project's branches), assignee and reviewers as usernames, labels as text, draft/squash/delete-source checkboxes. No pickers for people or labels (that is row 14, F*), no milestone. The source branch is the checked-out branch, never a form value. The existing command keeps opening the browser page. |
| gitlab 4 Create Snippet | F | Kept, **narrowed**: title, description, visibility, personal or this project, the focused file or selection as the one file. Several files need a file chooser (U5): not built. Unverified whether `ctx.editor()` is set when a view button is clicked; if not, the form says "Focus a file first" and the command stays the primary path. |
| gitlab 5 Share Project | F | Kept, **narrowed**: form with name, namespace, description, remote name, visibility, optional `git init`, optional initial commit behind a confirm click that lists files. No file-selection tree (original's commit dialog). |
| gitlab 13 note edit/delete, award emoji | F | Kept. Works on the selected tree note (`Key::Note`); own notes only; fixed emoji list (no picker over all emoji). |
| gitlab 16 commit list and per-commit diffs | F | Kept, **narrowed**: a commit list view; picking one shows its diff read-only in the Changes view (diff comments need the MR diff refs, so they are hidden in commit mode). |
| gitlab 17 pipeline and job details | F | Kept, **narrowed**: pipeline and jobs tree, retry job, retry pipeline, cancel pipeline, open job in browser. No job log (`/jobs/:id/trace` is unbounded text; streaming is not available). |
| ignore 10 unignore toggle, starred templates | F | Kept: boolean setting plus a Star button on the selected template (storage list). The original's starred list is a setting; ours is storage, same behaviour. |
| properties 2 Add New Property File | F | Kept: `fileTemplates` entry with extension `properties`. Unverified in the IDE: that an empty-bodied template is accepted (fallback in Task 9). |

Nothing needs types, rename, file creation from code actions, dialogs, injection, streaming, decorations or caret control. The Form views rely on one documented limit: the user opens them from the sidebar (`LIMITATIONS.md` _No popup, quick pick or input API_).

## Global Constraints

- Each plugin stays its own Cargo project; run cargo inside `plugins/<name>` with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Quiet unless sure: a check or action that cannot be certain stays silent; every check or action gets a quiet test (`<name>_is_quiet_when_<condition>`).
- UTF-16 columns and CRLF: any range or text edit gets a multibyte (UTF-16) test and a CRLF test.
- A fixed file publishes an empty list (`fixed_file_publishes_empty_list`) for every diagnostics group.
- One settings boolean per group (default on) in `settings.schema.json`, scalar types only.
- `events` is an `optional` capability with a `Warn` log on `Error::Denied`; every event-driven plugin needs an `activation` entry.
- Providers have a 50 ms budget: scan in events or commands, cache in memory. `find_files` skips hidden and ignored paths on the real host (the mock does not); a missing file is `Invalid` on the host and `NotFound` in the mock, so treat both as absent.
- Code actions edit existing files only; new files come from `fileTemplates`. No popups or input dialogs. A command's returned text is dropped: report through notifications.
- Open licences: `gitlab`, `ignore`, `properties` keep `LICENSE`/`NOTICE`. Anything not in the public source (`idea/263.7307.200` for GitLab and Properties, `JetBrains/idea-gitignore` `860a6855` for `.ignore`) is written from docs (GitLab REST API docs, `git` man pages). If an implementer reads the public source for behaviour, add one line to the plugin's `NOTICE` naming file and revision.
- GitLab security (every `gitlab` task): the plugin never holds the token (`Authorization: Bearer {{secret:token}}` is filled by the IDE); never log it, never show it, never put it in a URL, notification, view, status or error. Every value from a form field, the repository or the server that goes into a URL path or query is validated by `safe.rs` and percent-encoded with `api::encode`; ids are parsed as `u64`; every `git` argument that is not a fixed literal passes a `safe.rs` check and follows `--` where the subcommand accepts it. `net:` stays `gitlab.com` only and `process:` stays `git` only: no manifest change may add a host or program.
- Per plugin finish with `cargo test` (no new warnings) and `openidea-plugin build` (`export PATH="$HOME/.cargo/bin:$PATH"`). Never hand-edit `version` in `plugin.toml`. Conventional commits `feat(<plugin>): ...` / `fix(<plugin>): ...` ending with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not touch `.env`.
- Implementers do not edit `PARITY.md` or `LIMITATIONS.md`; they write "Doc notes" in their report (row, new class, the new limit).

## Review Focus

Failure modes the audit implies and no task's happy path exercises; each has its test in the owning task.

- **Option injection through git:** a branch, remote or project name starting with `-` (or containing shell metacharacters, because `open_terminal` may join arguments into a shell line) must be refused, never passed. Task 1, 4.
- **URL and ID injection in REST paths:** usernames, branch names, emoji names, SHAs and discussion ids containing `/`, `?`, `&`, `#`, `%2F`, `..` must be refused or encoded, never alter the endpoint. Tasks 1, 2, 5, 6, 7.
- **Secret exposure:** the token value (`glpat-x` in tests) must not appear in logs, notifications, view bodies or request URLs/bodies. Task 1 (cross-cutting test over every new flow).
- **Accidental publication or commit of private data:** public snippet visibility and `git add -A` of secret-looking files need a second click or a refusal. Tasks 3, 4.
- **CRLF `.gitignore`:** the unignore code action and template insertion must not add `\n` to a CRLF file (today `append` does). Task 8.

## Shared task shape

Unless a task says otherwise: write the named failing tests in the plugin's test module (`plugins/gitlab/src/tests.rs`, `plugins/ignore/src/tests.rs`) against `MockHost`; run `cargo test <name>` and expect FAIL; implement; run `cargo test` (all green, no new warnings) then `openidea-plugin build`; commit.

---

### Task 1: GitLab input guards and secret hygiene (strongest reviewer)

**Files:**
- Create: `plugins/gitlab/src/safe.rs`
- Modify: `plugins/gitlab/src/lib.rs` (`mod safe;`), `plugins/gitlab/src/repo.rs` (clone and share argument lists), `plugins/gitlab/src/review.rs:113` (`is_hex_sha` moves to `safe::hex_sha`, callers updated), `plugins/gitlab/src/api.rs` (encode discussion ids in `reply`, `resolve`)
- Test: `plugins/gitlab/src/tests.rs`, `#[cfg(test)]` in `safe.rs`

**Interfaces:**
- Produces (all return `Result<&str, Error>` with a fixed message that never echoes more than 40 characters of the input; the value is returned unchanged when valid):
  - `pub fn ref_name(s: &str) -> Result<&str, Error>`: `[A-Za-z0-9._/@+-]{1,200}`, no leading `-` or `/`, no `..`, no trailing `/` or `.lock`.
  - `pub fn remote_name(s: &str) -> Result<&str, Error>`: `[A-Za-z0-9_.-]{1,64}`, no leading `-` or `.`.
  - `pub fn username(s: &str) -> Result<&str, Error>`: `[A-Za-z0-9_.-]{1,255}`, no leading `-`.
  - `pub fn hex_sha(s: &str) -> Result<&str, Error>`: 7 to 40 ASCII hex digits.
  - `pub fn discussion_id(s: &str) -> Result<&str, Error>`: 1 to 64 hex digits.
  - `pub fn emoji(s: &str) -> Result<&'static str, Error>`: member of `EMOJIS = ["thumbsup","thumbsdown","heart","tada","laughing","confused","rocket","eyes"]`.
  - `pub fn gitlab_url(s: &str) -> Result<&str, Error>`: starts with `https://gitlab.com/`, no whitespace or control characters.
  - `pub fn repo_file_path(s: &str) -> Result<&str, Error>`: relative, `/`-separated, no empty, `.` or `..` segment, no `\`, no NUL.
- Consumed by Tasks 2 to 7.

- [ ] **Step 1: Write the failing tests.** In `safe.rs`: `ref_name_refuses_options_traversal_and_metacharacters` (`"-x"`, `"--upload-pack=x"`, `"a/../b"`, `"a;rm -rf ~"`, `"a b"`, `"$(x)"`, `"x.lock"` refused; `"feature/a-1"` accepted), `remote_name_refuses_leading_dash_and_dot`, `username_refuses_url_metacharacters` (`"a&b"`, `"a#b"`, `"a/b"`, `"a%2Fb"`), `hex_sha_requires_7_to_40_hex`, `discussion_id_refuses_slashes_and_queries` (`"abc/notes"`, `"abc?x=1"`), `emoji_must_be_listed`, `gitlab_url_refuses_other_hosts_and_whitespace` (`"https://gitlab.com.evil.test/x"`, `"http://gitlab.com/x"`, `"https://gitlab.com/x y"`), `repo_file_path_refuses_traversal` (`"../x"`, `"a/../../x"`, `"/etc/passwd"`, `"a\\b"`). In `tests.rs`: `clone_puts_the_url_and_target_after_double_dash` (terminal args `["clone","--","https://gitlab.com/me/tool.git","<dir>/tool"]`), `clone_refuses_a_project_path_starting_with_a_dash`, `reply_and_resolve_encode_the_discussion_id`, `the_token_never_appears_in_requests_logs_notifications_or_views` (runs list, details, comment, clone and share against the mock; asserts `glpat-x` is in no request URL or body, no `host.logs()` line, no notification, no rendered `View` debug string, and every request has `Authorization` equal to the `{{secret:token}}` placeholder), `an_error_response_is_shown_without_markup_or_over_200_characters` (existing `error_message` bound).
- [ ] **Step 2:** `cd plugins/gitlab && cargo test safe` and the named `tests.rs` tests; expect FAIL.
- [ ] **Step 3:** Implement `safe.rs` with the signatures above (hand-rolled byte checks, no regex dependency). In `repo.rs`, build clone args as `["clone", "--", url, target]` after `ref_name`-style validation of `project.path` (as `remote_name`); update the expected strings of the existing `clones_next_to_the_current_project_in_a_terminal` test. Wrap `api.rs` discussion ids with `api::encode` after `safe::discussion_id`.
- [ ] **Step 4:** `cargo test` all green, no new warnings.
- [ ] **Step 5:** Commit `fix(gitlab): validate values that reach git arguments and REST paths`.

### Task 2: GitLab Create Merge Request form (strongest reviewer)

**Files:**
- Modify: `plugins/gitlab/src/api.rs`, `plugins/gitlab/src/lib.rs` (GitLab state, `render`/`handle` dispatch), `plugins/gitlab/plugin.toml` (view `new-merge-request`, title "Create Merge Request", location `right`; no new capability), `plugins/gitlab/settings.schema.json` (`forms`, default on)
- Create: `plugins/gitlab/src/forms.rs` (holds the three forms of Tasks 2 to 4)
- Test: `plugins/gitlab/src/tests.rs`

**Interfaces:**
- Consumes: Task 1 `safe::{ref_name, username}`; existing `gitlab_project`, `api::Project`, `api::encode`, `GitLab::me`.
- Produces in `api.rs`:
  - `pub struct NewMergeRequest { pub source_branch: String, pub target_branch: String, pub title: String, pub description: String, pub assignee_id: Option<u64>, pub reviewer_ids: Vec<u64>, pub labels: Vec<String>, pub squash: bool, pub remove_source_branch: bool }`
  - `impl Project { pub fn branches(&self) -> Result<Vec<String>, Error> }` (`GET /projects/:id/repository/branches?per_page=100`, paged)
  - `impl Project { pub fn create_merge_request(&self, new: &NewMergeRequest) -> Result<MergeRequest, Error> }` (`POST /projects/:id/merge_requests`, JSON body; `labels` joined with `,`)
  - `pub fn user_id(username: &str) -> Result<u64, Error>` (`GET /users?username=<encode(username)>`; empty result is an error naming the username)
- `forms.rs`: `pub struct MergeRequestForm { title, description, target, assignee, reviewers, labels, draft, squash, remove_source }` with `pub fn render(&self, branches: &[String], source: &str) -> View` and `pub fn field(&mut self, key: &str, value: &str)`; field keys `title`, `description`, `target-branch`, `assignee`, `reviewers`, `labels`, `draft`, `squash`, `remove-source-branch`; button `create`.

- [ ] **Step 1: Write the failing tests:** `form_lists_the_branches_and_defaults_the_target_to_the_project_default`; `create_posts_title_source_target_and_options` (body keys asserted; the source is the VCS branch, not a field); `draft_prefixes_the_title_with_draft`; `assignee_and_reviewers_are_resolved_by_username_lookup`; `an_unknown_username_fails_and_creates_nothing` (no POST in `writes`); `usernames_with_url_metacharacters_are_refused_before_any_request` (`"a&b#c/d"`, `"-x"`); `a_target_branch_outside_the_listed_branches_is_ignored` (a `Field` event with `"main\"}, \"x"` or `"--evil"` leaves the previous target); `labels_are_trimmed_and_empty_ones_dropped`; `a_detached_head_refuses_and_says_so`; `an_empty_title_is_refused`; `creating_reports_the_web_url_in_a_notification_and_refreshes_the_list`; `the_form_is_quiet_when_forms_setting_is_off` (renders a `Message`, no request); `without_a_token_the_form_asks_for_one`.
- [ ] **Step 2:** `cd plugins/gitlab && cargo test create_posts_title` etc.; expect FAIL.
- [ ] **Step 3:** Implement in `api.rs` and `forms.rs`; wire `new-merge-request` into `Views::render`/`handle`; read `forms` with `host::setting_bool("forms").unwrap_or(true)`. Reuse the task/notify idiom of `create_snippet`.
- [ ] **Step 4:** `cargo test` green; `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): create merge requests from a form view`.

### Task 3: GitLab Create Snippet form (strongest reviewer)

**Files:** Modify `plugins/gitlab/src/api.rs` (`create_snippet`), `plugins/gitlab/src/forms.rs`, `plugins/gitlab/src/lib.rs`, `plugins/gitlab/plugin.toml` (view `new-snippet`, location `right`); test in `tests.rs`.

**Interfaces:**
- Consumes: Task 1 `safe::repo_file_path`; Task 2 `forms.rs` module and `forms` setting.
- Produces: `pub struct SnippetDraft { pub title: String, pub description: String, pub visibility: String, pub file_path: String, pub content: String, pub project: Option<String> }` and `pub fn create_snippet(draft: &SnippetDraft) -> Result<String, Error>` replacing the three-argument version (`POST /snippets`, or `POST /projects/:id/snippets` with `api::encode(project)` when `project` is set; `visibility` in `private|internal|public`, validated). The command `create-snippet` builds a private `SnippetDraft` and keeps its behaviour. Fields: `title`, `description`, `visibility` (Choice, default `private`), `scope` (Choice `Personal` / `Project <group/name>`), `file-path`; buttons `create`, and `confirm-public` for the second click.

- [ ] **Step 1: Write the failing tests:** `form_creates_a_private_personal_snippet_from_the_selection` (body asserted, same shape as `creates_a_private_snippet_from_the_selection` plus `description`); `project_scope_posts_to_the_encoded_project_path` (`/projects/g%2Fp/snippets`); `public_visibility_needs_a_second_click` (first click: no POST and the button reads "Confirm public"; second: one POST); `changing_any_field_cancels_the_pending_public_confirm`; `a_visibility_outside_the_choices_is_ignored`; `file_path_traversal_is_refused` (`"../../x"`, `"/etc/x"`); `no_focused_file_says_focus_a_file_first`; `an_empty_selection_falls_back_to_the_whole_file_and_an_empty_file_is_refused`; `crlf_content_is_sent_unchanged` and `multibyte_selection_is_cut_on_utf16_columns` (selection `(0, 3, 0, 5)` over `"a😀b"`); `the_form_is_quiet_when_forms_setting_is_off`.
- [ ] **Step 2:** run the named tests; expect FAIL.
- [ ] **Step 3:** Implement; keep `create_snippet(ctx)` for the command and move its body building into a shared `fn snippet_content(ctx) -> Result<(String, String), Error>` (name, content).
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): create snippets from a form view`.

### Task 4: GitLab Share Project form (strongest reviewer)

**Files:** Modify `plugins/gitlab/src/repo.rs` (`share` replaced by `share_with(ctx, &ShareForm)`), `plugins/gitlab/src/api.rs`, `plugins/gitlab/src/forms.rs`, `plugins/gitlab/src/lib.rs`, `plugins/gitlab/plugin.toml` (view `share-project-form`, location `right`; `process:git` stays optional); test in `tests.rs`.

**Interfaces:**
- Consumes: Task 1 `safe::{remote_name, ref_name, gitlab_url}`; existing `branch::git_ok`, `repo::project_name`.
- Produces in `api.rs`: `pub struct Namespace { pub id: u64, pub full_path: String }`, `pub fn namespaces() -> Result<Vec<Namespace>, Error>` (`GET /namespaces?per_page=100`, paged), and `pub fn create_project(name: &str, namespace_id: Option<u64>, description: &str, visibility: &str) -> Result<ProjectInfo, Error>` (replaces `create_project(name)`). `ShareForm { name, namespace, description, remote, visibility, init, commit_all }`; fields `name`, `namespace` (Choice), `description`, `remote` (default `origin`, else `gitlab`), `visibility` (Choice, default `private`), `init`, `commit-all` (checkbox, default off); buttons `share`, `confirm-commit` (second click).
- Order of work (nothing remote happens before the local steps succeed): validate all fields; `git init` only when `vcs_status` is `None` and `init` is ticked; with `commit-all`, run `git status --porcelain --untracked-files=all`, refuse naming any path matching `.env*`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`, then show the file count and wait for the second click, then `git add -A` and `git commit -m "Initial commit"`; create the project; `git remote add -- <remote> <url>`; terminal `git push -u -- <remote> <branch>`.

- [ ] **Step 1: Write the failing tests:** `share_form_creates_in_the_chosen_namespace_with_description_and_visibility`; `remote_names_starting_with_a_dash_are_refused_before_any_request` (`"-u"`, `"--mirror"`, no POST, no git run); `remote_name_with_shell_metacharacters_is_refused` (`"a;b"`, `"a b"`); `a_branch_with_shell_metacharacters_is_refused_before_pushing` (`"x;rm -rf ~"`, no project created); `remote_add_and_push_put_names_after_double_dash` (git runs `["--version", "remote add -- origin https://gitlab.com/me/project.git"]`, terminal `["push","-u","--","origin","main"]`); `a_returned_clone_url_on_another_host_is_refused_and_not_added` (mock returns `https://evil.test/x.git`); `init_runs_only_when_there_is_no_repository_and_the_box_is_ticked`; `commit_all_lists_files_and_waits_for_the_second_click` (no `git add` after the first click); `commit_all_refuses_secret_looking_files` (`.env`, `deploy.key`, `id_rsa`); `nothing_is_created_when_git_init_or_commit_fails` (no POST to `/projects`); `an_existing_gitlab_remote_refuses`; `share_without_process_git_names_the_capability` (`Error::Denied`); `a_namespace_outside_the_choices_is_ignored`; `the_form_is_quiet_when_forms_setting_is_off`. Update `share_creates_the_project_adds_the_remote_and_pushes` to the new args.
- [ ] **Step 2:** run the named tests; expect FAIL.
- [ ] **Step 3:** Implement; the existing `share-project` command opens the form's defaults path unchanged (private, user namespace, no init, no commit) so its behaviour and test stay.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): share projects from a form with init and initial commit`.

### Task 5: GitLab note edit, delete and emoji reactions (strongest reviewer)

**Files:** Modify `plugins/gitlab/src/api.rs`, `plugins/gitlab/src/lib.rs` (`render_details`, `handle_details`, `act`, `CONFIRMED`), `plugins/gitlab/settings.schema.json` (`noteActions`, default on); test in `tests.rs`.

**Interfaces:**
- Consumes: Task 1 `safe::emoji`; existing `Key::Note(iid, discussion, note_id)`, `GitLab::me()`, `CONFIRMED`, `COMMENT_KEY`.
- Produces in `api.rs`: `Project::edit_note(&self, iid: u64, note_id: u64, body: &str) -> Result<(), Error>` (`PUT /merge_requests/:iid/notes/:id`), `Project::delete_note(&self, iid: u64, note_id: u64) -> Result<(), Error>` (`DELETE`), `Project::awards(&self, iid: u64, note_id: Option<u64>) -> Result<Vec<Award>, Error>` with `Award { id: u64, name: String, user: User }`, `Project::add_award(&self, iid: u64, note_id: Option<u64>, name: &str) -> Result<(), Error>` (`POST .../award_emoji?name=<encode>`), `Project::remove_award(&self, iid: u64, note_id: Option<u64>, award_id: u64) -> Result<(), Error>`. Details toolbar gains `edit-note` ("Save Edit"), `delete-note` (second click, add to `CONFIRMED`), `react`; form gains a Choice field `emoji` over `safe::EMOJIS`.

- [ ] **Step 1: Write the failing tests:** `edit_saves_the_comment_text_to_the_selected_own_note`; `edit_and_delete_are_not_offered_for_someone_elses_note_or_a_system_note`; `delete_needs_a_second_click`; `another_action_cancels_a_pending_delete` (mirrors `another_action_or_selection_cancels_a_pending_close`); `react_adds_the_chosen_emoji_to_the_selected_note`; `react_again_removes_my_own_award` (list shows my award, DELETE `/award_emoji/<id>`); `react_without_a_selected_note_awards_the_merge_request`; `an_emoji_outside_the_list_is_refused_and_no_request_is_sent` (`"thumbsup&name=x"`, `"../x"`); `note_ids_come_from_the_tree_key_not_from_form_text`; `a_failed_edit_keeps_the_comment_text_and_says_why` (as `a_failed_comment_keeps_its_text_and_says_why`); `note_actions_are_quiet_when_the_setting_is_off`; `an_empty_edit_is_refused`.
- [ ] **Step 2:** run the named tests; expect FAIL.
- [ ] **Step 3:** Implement; own-note test is `note.author.id == me.id && !note.system`.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): edit and delete notes and react with emoji`.

### Task 6: GitLab merge request commits (strongest reviewer)

**Files:** Modify `plugins/gitlab/src/api.rs`, `plugins/gitlab/src/lib.rs`, `plugins/gitlab/src/review.rs` (`render_changes` reads `self.commit`), `plugins/gitlab/plugin.toml` (view `merge-request-commits`, location `right`), `settings.schema.json` (`commitsView`, default on); test in `tests.rs`.

**Interfaces:**
- Consumes: Task 1 `safe::hex_sha`; `GitLab::selected()`, `self.diffs: Option<(u64, Vec<FileDiff>)>`.
- Produces: `pub struct Commit { pub id: String, pub short_id: String, pub title: String, pub author_name: String }`; `Project::mr_commits(&self, iid: u64) -> Result<Vec<Commit>, Error>` (`GET .../merge_requests/:iid/commits`, paged); `Project::commit_diff(&self, sha: &str) -> Result<Vec<FileDiff>, Error>` (`GET .../repository/commits/:sha/diff`, `sha` already `hex_sha`-checked). `GitLab.commit: Option<String>`; list view rows keyed by sha; `Activate(sha)` sets it and refreshes `CHANGES`; button `all-changes` clears it.

- [ ] **Step 1: Write the failing tests:** `lists_the_commits_of_the_selected_merge_request`; `activating_a_commit_shows_its_diff_in_the_changes_view_without_comment_threads`; `all_changes_returns_to_the_merge_request_diff`; `selecting_another_merge_request_clears_the_commit`; `a_non_hex_commit_key_sends_no_request` (`"abc/../x"`, `"HEAD"`, `"abcdef1?x"`); `an_empty_commit_list_shows_a_message_not_a_blank_list`; `the_view_is_quiet_without_a_selected_merge_request`; `the_view_is_quiet_when_the_setting_is_off`; `diff_text_with_html_is_escaped_in_commit_mode` (reuses `review::escape`); `crlf_and_multibyte_diff_lines_keep_their_numbers` (via `review::parse_diff`).
- [ ] **Step 2:** run the named tests; expect FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): browse the commits of a merge request`.

### Task 7: GitLab pipeline and job details (strongest reviewer)

**Files:** Modify `plugins/gitlab/src/api.rs` (`Pipeline` gains `id: u64`, `web_url: String`), `plugins/gitlab/src/lib.rs`, `plugins/gitlab/plugin.toml` (view `merge-request-pipeline`, location `right`), `settings.schema.json` (`pipelineView`, default on); test in `tests.rs`.

**Interfaces:**
- Consumes: Task 1 `safe::gitlab_url`; `pipeline_label`, `CONFIRMED`.
- Produces: `pub struct Job { pub id: u64, pub name: String, pub stage: String, pub status: String, pub web_url: String, pub allow_failure: bool }`; `Project::mr_pipelines(&self, iid: u64) -> Result<Vec<Pipeline>, Error>`; `Project::pipeline_jobs(&self, pipeline_id: u64) -> Result<Vec<Job>, Error>`; `Project::retry_job(&self, job_id: u64)`, `retry_pipeline(&self, id: u64)`, `cancel_pipeline(&self, id: u64)` (all `POST`, `Result<(), Error>`). Tree keys `p:<id>`, `s:<pipeline>:<stage>`, `j:<id>`; ids parsed as `u64` only; buttons `retry` (selected job or pipeline), `cancel` (second click, add to `CONFIRMED`), `browser`, `refresh`.

- [ ] **Step 1: Write the failing tests:** `shows_pipelines_with_jobs_grouped_by_stage_and_status_icons`; `retry_posts_to_the_selected_job`; `retry_on_a_pipeline_posts_to_pipeline_retry`; `cancel_needs_a_second_click`; `tree_keys_with_non_numeric_ids_send_no_request` (`"j:1/../2"`, `"p:1?x"`, `"j:-1"`); `open_in_browser_refuses_a_job_url_on_another_host` (`https://evil.test/j/1`, `javascript:alert(1)`); `retry_is_disabled_for_a_running_job` (`Button::enabled(false)`); `a_merge_request_without_pipelines_shows_a_message`; `a_failed_retry_reports_the_gitlab_error_without_the_token`; `the_view_is_quiet_when_the_setting_is_off`.
- [ ] **Step 2:** run the named tests; expect FAIL.
- [ ] **Step 3:** Implement; reload the tree after an action as `act` does for the details view.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(gitlab): pipeline and job details with retry and cancel`.

### Task 8: `ignore` unignore toggle and starred templates

**Files:** Modify `plugins/ignore/src/lib.rs` (`gitignore_actions`, `append`, `template_tree`, `Views::render`/`handle`), `plugins/ignore/settings.schema.json`; test in `plugins/ignore/src/tests.rs`.

**Interfaces:**
- Produces: setting `unignoreActions` (boolean, default true: when off, `gitignore_actions` returns only the plain "Add to .gitignore" action); `const STARRED_PREFIX: &str = "starred:"` (storage key `starred:<template key>` = `"1"`); `fn starred() -> HashSet<String>`; `template_tree(user: &[String], expanded: &HashSet<String>, starred: &HashSet<String>) -> Vec<TreeItem>` (starred templates also listed under a new group `g:starred` "Starred" when non-empty, and shown with the `star` icon); button `star` ("Star"/"Unstar" by state) enabled when a `t:`/`u:` item is selected; `fn append(content: &str, addition: &str, blank_line: bool) -> TextEdit` now writes the file's own line ending (`\r\n` when `content` contains `\r\n`).

- [ ] **Step 1: Write the failing tests:** `unignore_action_is_absent_when_the_setting_is_off`; `unignore_action_is_offered_by_default` (existing behaviour); `star_button_toggles_and_persists_a_template` (`host.storage()` holds `starred:t:Rust`; second click removes it); `starred_templates_are_listed_under_starred_and_marked`; `the_star_button_is_disabled_without_a_template_selected` (group item `g:` selected); `starring_a_deleted_user_template_is_dropped_on_render` (no ghost row); `unignore_edit_uses_crlf_in_a_crlf_gitignore` (content `"/target/\r\n"` yields an edit text `"/src/a.rs\r\n"` with no bare `\n`); `template_insertion_uses_crlf_in_a_crlf_file`; `appending_to_a_file_without_final_newline_adds_one_crlf`; `unignore_ranges_use_utf16_columns_with_multibyte_text` (content `"/ü😀"` without newline: edit range starts at `Position { line: 0, character: 4 }`); `a_missing_gitignore_offers_no_action` (both `Invalid` and `NotFound` kinds). Existing `offers_add_to_gitignore_for_project_files` stays green.
- [ ] **Step 2:** `cd plugins/ignore && cargo test unignore` etc.; expect FAIL.
- [ ] **Step 3:** Implement; detect CRLF by `content.contains("\r\n")`; read the setting with the existing `setting("unignoreActions", true)`.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(ignore): unignore setting, starred templates, keep CRLF when appending`.

### Task 9: `properties` Add New Property File

**Files:**
- Create: `plugins/properties/templates/properties.properties`
- Modify: `plugins/properties/plugin.toml` (second `[[contributes.fileTemplates]]`: `id = "properties"`, `name = "Properties File"`, `extension = "properties"`, `path = "templates/properties.properties"`)
- Test: `#[cfg(test)]` in `plugins/properties/src/lib.rs` (existing `mod tests` at line 570)

**Interfaces:** No code. The template body is one newline (the original creates an empty file).

- [ ] **Step 1: Write the failing test** `new_menu_declares_a_properties_file_template`: reads `include_str!("../plugin.toml")`, finds the section with `id = "properties"`, asserts `extension` is `properties` and that the `path` file exists under `env!("CARGO_MANIFEST_DIR")`; plus `xml_properties_template_is_unchanged` (id `xml-properties` still present).
- [ ] **Step 2:** `cd plugins/properties && cargo test new_menu_declares`; expect FAIL.
- [ ] **Step 3:** Add the file and the manifest entry. If `openidea-plugin check` rejects a whitespace-only template, use a single line `# ` comment instead and mention it in the report.
- [ ] **Step 4:** `cargo test`, `openidea-plugin build`.
- [ ] **Step 5:** Commit `feat(properties): Properties File entry in the New menu`.

---

## Self-review

- **Spec coverage:** gitlab rows 1, 4, 5, 13, 16, 17 map to Tasks 2, 3, 4, 5, 6, 7; row 23 is ruled B; ignore row 10 to Task 8; properties row 2 to Task 9. Task 1 carries the cross-cutting security tests the brief makes mandatory (option injection: Tasks 1 and 4; path escape: Tasks 1 and 3; URL/ID injection: Tasks 1, 2, 5, 6, 7; token: Task 1).
- **Settings groups:** `forms` (Tasks 2 to 4), `noteActions`, `commitsView`, `pipelineView`, `unignoreActions`. The Properties template has no setting: a manifest entry cannot be switched off.
- **UTF-16 and CRLF:** applicable only where text is cut or edited (Tasks 3, 6, 8). No diagnostics are added, so `fixed_file_publishes_empty_list` has no owner in this plan.
- **Type consistency:** `NewMergeRequest`, `SnippetDraft`, `Namespace`, `Award`, `Commit`, `Job` and the `safe::*` names are defined once and used under the same names later; `create_snippet` and `create_project` change signature in Tasks 3 and 4, and their callers are listed there.
- **Proportion:** signatures, test names and ordering decisions only; bodies are left to the implementer.

## Ledger (fill during execution)

| Item | Status | Commit | Notes |
|---|---|---|---|
| Ruling: gitlab 23 downgraded to B | decided | | |
| Task 1 guards and secret hygiene | | | |
| Task 2 Create MR form | | | |
| Task 3 Create Snippet form | | | |
| Task 4 Share Project form | | | |
| Task 5 note edit/delete, emoji | | | |
| Task 6 MR commits | | | |
| Task 7 pipeline and jobs | | | |
| Task 8 ignore settings, CRLF | | | |
| Task 9 properties template | | | |
| Unverified in the IDE | | | `ctx.editor()` inside a view click; `open_terminal` argument joining; empty file template; form field events on `Choice` |
| Doc notes for `PARITY.md` / `LIMITATIONS.md` | | | |
