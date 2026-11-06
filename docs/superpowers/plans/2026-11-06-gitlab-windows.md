# GitLab: 10 tool windows to 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The GitLab plugin shows three tool windows (`GitLab Projects`, `Merge Requests`, `Merge Request`) with distinct icons; the Merge Request window holds five sections and the three form windows become prompt sequences.

**Architecture:** `Merge Request` (`DETAILS`, right) becomes the host of five sections (Overview, Changes, Commits, Files, Pipelines). A `Section` enum and a `GitLab.section` field select which existing renderer/handler runs; section buttons are toolbar buttons that set the section and refresh the view. The create-MR, create-snippet and share-project commands ask with `input`/`pick`/`confirm` and call the existing API code instead of opening form views.

**Tech Stack:** Rust, `openidea-plugin` SDK v0.1.38 (API 1.2), `MockHost` tests, `openidea-plugin build`/`check`.

**Spec:** `docs/superpowers/specs/2026-11-06-tool-window-consolidation-design.md` (goals 1, 2, 4; GitLab parts)

## Global Constraints

- Plugin lives in `plugins/gitlab` (own Cargo project, no root workspace). Never edit `version` in `plugin.toml` (release-please owns it; CI `versions` job fails otherwise). Update `change-notes` ("Unreleased:" prefix).
- Windows after the change: `gitlab-projects` (left), `merge-requests` (left), `merge-request` (right). `merge-request-changes`, `merge-request-files`, `merge-request-commits`, `merge-request-pipeline`, `new-merge-request`, `new-snippet`, `share-project-form` are removed from `plugin.toml`.
- Changes moves to the right, inside `merge-request`.
- A view body is replaced on each render; `pick`/`input`/`confirm` work only inside a command or event of a window and return `Error::Invalid` elsewhere; `input` is one line.
- HTML view bodies must work script-free (scripts are blocked in the desktop app).
- Dismissed prompt anywhere: no side effect. API failure after all prompts: show the error with `host::notify` and offer "Open on GitLab" with `confirm`; answers are not kept.
- Create Merge Request description is a one-line `input`; longer text goes through "Open on GitLab".
- Icons: SVG, at most 16 KiB, family logo (GitLab fox, colours kept) plus a small function glyph.
- Commits: conventional (`feat(gitlab):`), one per task; no `!` here (GitLab keeps its id; versions stay release-please's).

## Review Focus

- No merge request selected: every section shows the "Select a merge request" message and no section button errors.
- Token missing: the `NO_TOKEN` view with "Create Access Token" shows whichever section is active.
- Active section survives a plugin reload (stored under `gitlab.section`); an unknown stored value falls back to Overview.
- Badge counts come from data already cached; opening the window must not trigger four extra API fetches.
- The Changes section still receives `ViewEvent::Message` (post_message from its HTML) after the move.
- A prompt dismissed in the middle of create-MR/snippet/share leaves no project, branch, snippet or MR behind.
- `forms` setting is gone from `settings.schema.json` and docs; a stale stored value does not break loading.

---

### Task 1: Section infrastructure with Overview and Changes

**Files:**
- Create: `plugins/gitlab/src/sections.rs`
- Modify: `plugins/gitlab/src/lib.rs` (constants at 30-39, `render` at ~1989, `handle` at ~2017, `render_details`, `handle_details`, `GitLab` struct at 393), `plugins/gitlab/plugin.toml`, `plugins/gitlab/src/tests.rs`

**Interfaces:**
- Produces: `pub enum Section { Overview, Changes, Commits, Files, Pipelines }` with `fn id(self) -> &'static str` ("overview", "changes", …), `fn label(self) -> &'static str`, `fn parse(id: &str) -> Option<Section>`, `Section::ALL: [Section; 5]`.
- Produces: `pub struct Badges { pub changes: Option<usize>, pub commits: Option<usize>, pub files: Option<usize>, pub pipeline: Option<String> }` and `pub fn with_sections(view: View, active: Section, badges: &Badges) -> View` that prepends one `Button` per section (id `section:<id>`, label `Changes (12)` when a badge exists, active one visibly marked in its label).
- Produces: `GitLab::section: Section`, `GitLab::set_section(&mut self, s: Section)` (stores under `gitlab.section`, refreshes `DETAILS`), `GitLab::refresh_section(&self, s: Section)` (refreshes `DETAILS` when `s` is the active section).
- Consumes: existing `render_details`, `render_changes`, `handle_details`, `handle_changes`.

- [ ] **Step 1: Write failing tests** in `tests.rs`: `section_buttons_list_the_five_sections_with_the_active_one_marked`, `clicking_a_section_button_switches_the_view_and_stores_it`, `unknown_stored_section_falls_back_to_overview`, `no_selected_merge_request_shows_the_select_message_in_every_section`, `changes_section_still_routes_view_messages`. Assert on `View` buttons by id (`section:changes`) and on body kind.
- [ ] **Step 2: Run** `cargo test sections` in `plugins/gitlab`; expect FAIL (missing module).
- [ ] **Step 3: Implement** `sections.rs` and the `GitLab.section` field (loaded in the constructor/`activate` from storage). `Views::render` for `DETAILS` runs the active section's renderer and wraps it with `with_sections`; `Views::handle` for `DETAILS` handles `Button("section:<id>")` itself and routes every other event to the active section's handler. Overview = today's `render_details`/`handle_details`; Changes = today's `render_changes`/`handle_changes` (messages included). Remove `CHANGES` from the manifest and constants; replace `host::refresh_view(CHANGES)` calls with `self.refresh_section(Section::Changes)`.
- [ ] **Step 4: Update** existing tests that used the `merge-request-changes` view id to call the `DETAILS` view with the Changes section active.
- [ ] **Step 5: Run** `cargo test`, `openidea-plugin build`, `openidea-plugin check`; expect all pass.
- [ ] **Step 6: Commit** `feat(gitlab): add sections to the Merge Request window and move Changes into it`.

### Task 2: Commits, Files and Pipelines sections

**Files:**
- Modify: `plugins/gitlab/src/lib.rs` (`render_commits` ~1618, `handle_commits` ~1654, `render_pipelines` ~1703, `handle_pipelines` ~1807, `reveal_view`/`refresh_view` call sites), `plugins/gitlab/src/review.rs` (`render_files` 462, `handle_files` 496, refreshes at 530, 636), `plugins/gitlab/src/branch.rs` (424-426), `plugins/gitlab/plugin.toml`, `plugins/gitlab/src/tests.rs`

**Interfaces:**
- Consumes: `Section`, `Badges`, `with_sections`, `GitLab::refresh_section`, `GitLab::set_section` from Task 1.
- Produces: `GitLab::badges(&self) -> Badges` reading only cached state (files from the loaded diff, commits list, last pipeline status).

- [ ] **Step 1: Write failing tests**: `commits_section_renders_the_commit_list`, `files_section_toggles_viewed_and_refreshes_the_window`, `pipelines_section_retries_a_job`, `badges_use_cached_data_and_make_no_api_call` (assert the mock's request count is unchanged after `render`).
- [ ] **Step 2: Run** `cargo test commits_section files_section pipelines_section badges`; expect FAIL.
- [ ] **Step 3: Route** `Section::Commits`, `Files`, `Pipelines` to the existing renderers/handlers; replace every `refresh_view(COMMITS|FILES|PIPELINES)` with `refresh_section`; the status-item / branch flow in `branch.rs` that calls `reveal_view(DETAILS)` also calls `set_section(Section::Overview)`. Remove the three views and constants from `plugin.toml`.
- [ ] **Step 4: Update** existing tests keyed by the old view ids (grep `merge-request-files`, `merge-request-commits`, `merge-request-pipeline` in `tests.rs`).
- [ ] **Step 5: Run** full `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 6: Commit** `feat(gitlab): fold commits, files and pipelines into the Merge Request window`.

### Task 3: Create Merge Request as prompts

**Files:**
- Create: `plugins/gitlab/src/prompts.rs`
- Modify: `plugins/gitlab/src/lib.rs` (`create-merge-request` command ~1348, `render_new_merge_request`/`handle_new_merge_request` ~1481-1513, `NEW_MR` constant), `plugins/gitlab/plugin.toml`, `plugins/gitlab/src/tests.rs`

**Interfaces:**
- Produces: `pub struct NewMergeRequest { pub source: String, pub target: String, pub title: String, pub description: String, pub draft: bool }` and `pub fn ask_new_merge_request(defaults: &MrDefaults) -> Result<Option<NewMergeRequest>, Error>` (`Ok(None)` when any prompt is dismissed). `MrDefaults { source: String, target: String, title: String }` come from the current branch, the project's default branch and the last commit subject.
- Consumes: the existing function that performs the create request (today `create_merge_request(ctx)` reading form state): refactor it to take a `NewMergeRequest`.

- [ ] **Step 1: Write failing tests** with `answer_next_prompt`: `create_merge_request_asks_branches_title_description_and_draft` (answers in order, assert the POST body), `dismissing_any_prompt_creates_nothing` (loop over each prompt position, assert zero POSTs), `api_failure_notifies_and_offers_to_open_gitlab`.
- [ ] **Step 2: Run** `cargo test create_merge_request`; expect FAIL.
- [ ] **Step 3: Implement** `ask_new_merge_request` (`pick` source branch from local branches defaulting to current, `pick` target from project branches defaulting to default branch, `input` title prefilled, `input` description, `confirm` "Create as draft?") and rewire the command; on `Err` from the API call, `host::notify` then `confirm("Open on GitLab?")` then `host::open_url` of the new-MR page. Delete the `NEW_MR` view, its state and its `forms_enabled()` branch.
- [ ] **Step 4: Run** `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 5: Commit** `feat(gitlab): create merge requests with prompts instead of a form window`.

### Task 4: Create Snippet and Share Project as prompts

**Files:**
- Modify: `plugins/gitlab/src/prompts.rs`, `plugins/gitlab/src/lib.rs` (`handle_new_snippet` ~1522-1547, `handle_share` ~1565-1602, `NEW_SNIPPET`, `SHARE`), `plugins/gitlab/src/forms.rs` (remove the form-only state), `plugins/gitlab/plugin.toml`, `plugins/gitlab/settings.schema.json`, `plugins/gitlab/src/tests.rs`

**Interfaces:**
- Produces: `pub fn ask_new_snippet(default_title: &str, paths: &[String]) -> Result<Option<SnippetDraft>, Error>` (title `input`, visibility `pick` private/internal/public, `confirm` when public, extra project paths `input` one per prompt until empty) and `pub fn ask_share(default_name: &str) -> Result<Option<ShareRequest>, Error>` (name `input`, visibility `pick`, `confirm` when public, `confirm` for committing pending changes if the existing flow requires it).
- Consumes: today's snippet and share request code, refactored to take the structs; `SnippetDraft` already exists (`files` list).

- [ ] **Step 1: Write failing tests**: `snippet_prompts_collect_title_visibility_and_extra_files`, `public_snippet_needs_the_confirmation`, `share_prompts_collect_name_and_visibility`, `dismissing_any_snippet_or_share_prompt_creates_nothing`.
- [ ] **Step 2: Run** `cargo test snippet share`; expect FAIL.
- [ ] **Step 3: Implement** the two prompt functions and rewire `create-snippet` and `share-project`; delete the `NEW_SNIPPET`/`SHARE` views and the `forms` setting (`forms_enabled()` and its schema entry); a stored `forms` value is ignored.
- [ ] **Step 4: Run** `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 5: Commit** `feat(gitlab): create snippets and share projects with prompts`.

### Task 5: Distinct window icons

**Files:**
- Create: `plugins/gitlab/icons/projects.svg`, `plugins/gitlab/icons/merge-requests.svg`, `plugins/gitlab/icons/merge-request.svg`
- Modify: `plugins/gitlab/plugin.toml` (the three `icon =` lines), `ICONS.md`

- [ ] **Step 1: Draw** three 16x16 SVGs: GitLab fox (colours kept, from `icons/gitlab.svg`) at about 12 px with a bottom-right glyph: list (projects), two arrows (merge requests), info `i` (merge request). Each file under 16 KiB.
- [ ] **Step 2: Point** the three views at them; `openidea-plugin check` must report `ok`.
- [ ] **Step 3: Commit** `feat(gitlab): give the three windows distinct icons`.

### Task 6: Docs and real IDE check

**Files:**
- Modify: `plugins/gitlab/README.md`, `plugins/gitlab/plugin.toml` (`change-notes`), `LIMITATIONS.md` (GitLab section), `PARITY.md` (gitlab row)

- [ ] **Step 1: Update** README and change-notes (3 windows, sections, prompts, `forms` removed) and the LIMITATIONS/PARITY lines that mention form views and the bottom Changes window.
- [ ] **Step 2: Grep** `merge-request-changes`, `merge-request-files`, `merge-request-commits`, `merge-request-pipeline`, `new-merge-request`, `new-snippet`, `share-project-form` across the repo; expect no hits outside git history.
- [ ] **Step 3: Verify in OpenIDEA 0.1.38** (link the plugin as done earlier, grant it, reload): stripe shows 3 GitLab icons, all distinct; selecting a merge request fills the right window; each section button works; the three commands run their prompts and a dismissed prompt creates nothing. A GitLab token is needed: ask the user to enter it under Settings › Plugins › GitLab, or run the checks that need no network.
- [ ] **Step 4: Commit** `docs(gitlab): describe the consolidated windows`.
