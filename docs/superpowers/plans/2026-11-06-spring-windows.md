# Spring: 15 tool windows to 5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Spring shows five tool windows (Endpoints, Structure, Beans, Runtime, Initializr) with distinct icons; `aritti.spring-web` and `aritti.spring-integration` absorb seven other Spring plugins; `spring-beans` folds its three windows into sections.

**Architecture:** Each absorbed module stays a library crate (`rlib`) with a cargo feature `embedded` that disables its own `register_plugin!`. The surviving plugin owns a struct holding the module structs and implements `Providers`, `Views` and `Plugin` by delegating to them: provider results are concatenated, modules that don't apply to the project are skipped, view calls go to the module owning the active section. A shared section switcher lives in the new `crates/ui-kit` (also used by the route lists, see `2026-11-07-route-lists.md`).

**Tech Stack:** Rust, `openidea-plugin` SDK v0.1.38 (API 1.2), `crates/spring-text`, `MockHost` tests, `openidea-plugin build`/`check`.

**Spec:** `docs/superpowers/specs/2026-11-06-tool-window-consolidation-design.md` (goals 3, 4; Spring parts and "My calls")

## Global Constraints

- Windows: `Spring Endpoints` (owner `aritti.spring-web`, absorbs `spring-websocket`, `spring-messaging`, `spring-graphql`, `spring-webflow`, `spring-cloud`), `Spring Structure` (owner `aritti.spring-integration`, absorbs `spring-batch`, `spring-modulith`), `Spring Beans` (`spring-beans` alone, three sections), `Spring Runtime` and `Spring Initializr` unchanged.
- The seven absorbed plugin ids are removed from `plugins/`, `release-please-config.json`, `.release-please-manifest.json` and the docs; their published releases are left alone and unmaintained. The survivors' README and change-notes tell users to uninstall the old ids (duplicate windows and completions otherwise).
- Merge commits use `feat!:` (survivors take a minor bump while pre-1.0). Never edit `version` in `plugin.toml`.
- Survivors keep their own settings and storage keys; only absorbed modules' command ids and storage keys get a `<module>.` prefix (`websocket.`, `messaging.`, `graphql.`, `webflow.`, `cloud.`, `batch.`, `modulith.`). Absorbed modules' old settings values are not migrated.
- The view id `diagram` (batch, integration, modulith) is renamed per module.
- The survivor asks for the union of its modules' capabilities, kept in `optional` wherever the module had them optional.
- Providers have a 50 ms budget and one instance serves calls one at a time: parse each document once, skip non-applicable modules, and gate on a measurement before the next merge.
- `pick`/`input`/`confirm` only in commands/events; HTML view bodies script-free.
- Tests: every module's existing tests stay green; new tests cover the dispatch layer and section switching. Commit per task.

## Review Focus

- A project with none of the absorbed technologies (a Laravel project) must cost near nothing: no module work, no windows with errors, no CPU-budget crash in the log.
- A module's provider returning `Err`/empty must not hide the other modules' results.
- Code actions and `resolve_code_action` go back to the module that produced them (action ids carry the module prefix).
- A module that refreshes or reveals its old view id (`host::refresh_view("diagram")`) must refresh the new window, not a missing view.
- The same `diagram`/command id/storage key in two modules must not collide after the merge (test: register all and assert uniqueness).
- Old and new plugin installed together: documented duplicates, not a crash.

---

### Task 1: Shared section switcher in `crates/ui-kit`

**Files:**
- Create: `crates/ui-kit/Cargo.toml`, `crates/ui-kit/src/lib.rs`, `crates/ui-kit/src/sections.rs` (a library crate depending on `openidea-plugin` v0.1.38, own `[workspace]`, like `spring-text`)
- Test: unit tests in `sections.rs`

**Interfaces:**
- Produces: `pub struct SectionDef { pub id: &'static str, pub label: &'static str }`; `pub struct Switcher { /* defs, active */ }` with `Switcher::new(storage_key: &'static str, defs: &'static [SectionDef]) -> Switcher` (loads the stored id, falls back to the first), `fn active(&self) -> &'static str`, `fn set(&mut self, id: &str) -> bool` (stores, false if unknown), `fn buttons(&self, view: View, badges: &[(&str, String)]) -> View` (prepends one `Button` per section, id `section:<id>`, active one marked, badge appended to the label), `fn handle(&mut self, event: &ViewEvent) -> bool` (true when it consumed a `section:` button).

- [ ] **Step 1: Write failing tests**: `new_falls_back_to_the_first_section_for_an_unknown_stored_id`, `set_stores_the_active_section`, `buttons_mark_the_active_section_and_show_badges`, `handle_consumes_only_section_buttons`.
- [ ] **Step 2: Run** `cargo test sections` in `crates/ui-kit`; expect FAIL.
- [ ] **Step 3: Implement** `sections.rs` as specified.
- [ ] **Step 4: Run** `cargo test` in `crates/ui-kit`; expect pass.
- [ ] **Step 5: Commit** `feat(ui-kit): add a section switcher for multi-section windows`.

### Task 2: Make modules embeddable

**Files:**
- Modify: `plugins/spring-batch`, `plugins/spring-modulith`, `plugins/spring-integration` (this task covers the Structure group; Task 5 repeats it for the Endpoints group): each `Cargo.toml` (`[features] embedded = []`), `src/lib.rs`
- Test: each module's existing tests

**Interfaces:**
- Produces: in each module a `pub fn applies(project: &Project) -> bool` (cheap check from a build-file dependency or a config marker the module already uses to decide it is relevant (for example `spring-batch` in `pom.xml` or `build.gradle`); return true when unsure), `pub const WINDOW_SECTION: &str` ("batch", "modulith", "integration"), and a `pub(crate) fn refresh(view: &str)` that calls `host::refresh_view(view)` normally and `host::refresh_view(OWNER_WINDOW)` under `cfg(feature = "embedded")`; every existing `host::refresh_view(...)`/`host::reveal_view(...)` call goes through it.
- Produces: `register_plugin!` wrapped in `#[cfg(not(feature = "embedded"))]`; module command ids, storage keys and view ids prefixed per the Global Constraints when built with `embedded` (a `fn id(name: &str) -> String`).

- [ ] **Step 1: Write failing test per module**: `refresh_goes_to_the_owner_window_when_embedded` (run with `--features embedded`; assert the mock's refreshed view list).
- [ ] **Step 2: Run** `cargo test --features embedded refresh_goes` in each module; expect FAIL.
- [ ] **Step 3: Implement** the feature, `applies`, `refresh` and prefixing; grep `refresh_view(`, `reveal_view(`, `storage::get(`, `storage::set(` to catch every call.
- [ ] **Step 4: Run** `cargo test` and `cargo test --features embedded` in each; expect pass.
- [ ] **Step 5: Commit** `feat(spring-batch,spring-modulith,spring-integration): make the modules embeddable`.

### Task 3: Spring Structure (integration owns batch and modulith) and the budget gate

**Files:**
- Create: `plugins/spring-integration/src/structure.rs`
- Modify: `plugins/spring-integration/Cargo.toml` (path dependencies on `spring-batch` and `spring-modulith` with `features = ["embedded"]`), `plugins/spring-integration/src/lib.rs`, `plugins/spring-integration/plugin.toml` (merged contributions), `release-please-config.json`, `.release-please-manifest.json`, docs
- Delete: `plugins/spring-batch`, `plugins/spring-modulith` (after their code lives behind the path dependency, move them to `plugins/spring-integration/modules/batch` and `.../modulith` and update the dependency paths, keeping git history with `git mv`)
- Test: `plugins/spring-integration/src/structure.rs`

**Interfaces:**
- Produces: `pub struct Structure { integration: Integration, batch: Batch, modulith: ModulithTools, sections: Switcher }` implementing `Providers` (concatenate each applicable module's result for `completion`, `definition`, `code_actions`, `line_markers`; `resolve_code_action` routes by the action id prefix), `Views` (one view `structure`; sections `channels`, `batch`, `modules`, `diagram` via `Switcher`), `Plugin` (`activate` calls each module's, `run_command` strips the module prefix and delegates).
- Consumes: `Switcher` (Task 1), `applies`/`refresh` (Task 2).

- [ ] **Step 1: Write failing tests**: `providers_skip_modules_that_do_not_apply`, `one_failing_module_does_not_hide_the_others`, `code_action_resolves_through_its_module`, `section_buttons_switch_the_structure_window`, `ids_are_unique_across_modules` (commands, views, storage keys), and `merged_providers_stay_under_budget_on_a_sample_project` (times the four providers over `ma-station-essence`-like fixture files with a loose threshold of 10 ms natively).
- [ ] **Step 2: Run** `cargo test structure` in `plugins/spring-integration`; expect FAIL.
- [ ] **Step 3: Implement** `structure.rs`, merge the three manifests' providers/commands/settings/templates/icons into `plugin.toml` with the prefixes, one window `structure` (location right), union capabilities; remove the two absorbed plugin directories from release-please config/manifest and docs.
- [ ] **Step 4: Run** `cargo test`, `openidea-plugin build`, `openidea-plugin check`; expect pass.
- [ ] **Step 5: Budget gate**: link the plugin in OpenIDEA 0.1.38, open `ma-station-essence` and a non-Spring project; read `~/Library/Logs/ai.l0ng.openidea/openidea.<date>.log` for `exceeded its CPU budget` and slow calls; expect none. Stop and revisit the split if a crash appears.
- [ ] **Step 6: Commit** `feat(spring-integration)!: absorb spring-batch and spring-modulith into one Structure window`.

### Task 4: Spring Endpoints (web owns websocket, messaging, graphql, webflow, cloud)

**Files:**
- Create: `plugins/spring-web/src/endpoints.rs`
- Modify: `plugins/spring-web/Cargo.toml`, `plugins/spring-web/src/lib.rs`, `plugins/spring-web/plugin.toml`, `plugins/spring-websocket`, `plugins/spring-messaging`, `plugins/spring-graphql`, `plugins/spring-webflow`, `plugins/spring-cloud` (embeddable as in Task 2, then moved under `plugins/spring-web/modules/`), `release-please-config.json`, `.release-please-manifest.json`, docs

**Interfaces:**
- Produces: `pub struct Endpoints { web: WebTools, websocket: WebSocketTools, messaging: …, graphql: …, webflow: …, cloud: …, sections: Switcher }` with the same `Providers`/`Views`/`Plugin` delegation as `Structure` (Task 3); one view `endpoints`, sections `http`, `websocket`, `messaging`, `graphql`, `webflow`, `feign`; the web module's endpoints tree is the `http` section and keeps its `viewItem.context` menu (`view` becomes `endpoints`, and `when` clauses keep working through the item key).
- Consumes: Tasks 1-3 patterns, and the route list of `2026-11-07-route-lists.md` Task 1 for the `http` section (flat list with controller filter, flat/tree toggle, row actions); the web module's own endpoints tree is replaced by it.

- [ ] **Step 1: Make the five modules embeddable** (repeat Task 2 per module: feature `embedded`, `applies`, `refresh`, prefixes) with the failing `refresh_goes_to_the_owner_window_when_embedded` test first; commit `feat(spring-*): make the endpoint modules embeddable`.
- [ ] **Step 2: Write failing tests** in `endpoints.rs` (the same five as Task 3, over six modules) and `endpoints_item_menu_still_targets_the_endpoints_view`.
- [ ] **Step 3: Implement** `endpoints.rs`, merge manifests (prefixes, one window `endpoints` on the right, union capabilities, `process:tee` and `net:` optional), `git mv` the five directories under `plugins/spring-web/modules/`, drop them from release-please/CI/docs.
- [ ] **Step 4: Run** `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 5: Budget gate** as in Task 3 on `ma-station-essence`: the Endpoints section still lists 18 endpoints (6 controllers); no crash in the log; a Laravel project (`baby-bob`) costs nothing.
- [ ] **Step 6: Commit** `feat(spring-web)!: absorb websocket, messaging, graphql, webflow and cloud into one Endpoints window`.

### Task 5: Spring Beans sections

**Files:**
- Modify: `plugins/spring-beans/src/lib.rs` (its three views and their refresh calls), `plugins/spring-beans/plugin.toml`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: one view `beans` (left) with sections `beans`, `graph`, `report` using `Switcher`; the old view ids are removed and every `refresh_view`/`reveal_view` of them goes to `beans` plus `set(section)`; the `spring-report` commands reveal the window and set section `report`.
- Consumes: `Switcher` (Task 1).

- [ ] **Step 1: Write failing tests**: `beans_window_has_three_sections`, `check_configuration_command_opens_the_report_section`, `graph_section_renders_the_bean_graph`.
- [ ] **Step 2: Run** `cargo test sections` in `plugins/spring-beans`; expect FAIL.
- [ ] **Step 3: Implement** the merge of the three views into one; update tests keyed by the old ids.
- [ ] **Step 4: Run** `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 5: Commit** `feat(spring-beans): fold the three windows into one with sections`.

### Task 6: Distinct icons, docs and real IDE check

**Files:**
- Create: `plugins/spring-web/icons/endpoints.svg`, `plugins/spring-integration/icons/structure.svg`, `plugins/spring-beans/icons/beans.svg`
- Modify: the three `plugin.toml` view icons, `plugins/spring-runtime/plugin.toml` and `plugins/spring-initializr/plugin.toml` (icons that differ from the three above), `ICONS.md`, the survivors' READMEs and change-notes, `LIMITATIONS.md`, `PARITY.md`, `docs/ide-verification-checklist.md`

- [ ] **Step 1: Draw** the three 16x16 SVGs: Spring leaf (colours kept) with a corner glyph: globe (endpoints), structure/branching (structure), bean (beans). Runtime and Initializr keep their own glyphs, distinct from the other three.
- [ ] **Step 2: Docs**: survivors' README/change-notes say to uninstall `aritti.spring-websocket`, `-messaging`, `-graphql`, `-webflow`, `-cloud`, `-batch`, `-modulith`; remove every reference to the removed ids and windows (grep each old plugin id and view id).
- [ ] **Step 3: Verify in OpenIDEA 0.1.38** on `ma-station-essence`: the stripe shows 5 Spring windows with distinct icons; Endpoints section lists 18 endpoints; Beans sections switch; no plugin warnings in the log.
- [ ] **Step 4: Commit** `docs(spring): describe the five Spring windows and the retired plugin ids`.
