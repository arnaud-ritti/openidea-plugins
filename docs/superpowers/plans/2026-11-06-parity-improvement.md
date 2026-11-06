# Parity Improvement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise each of the 36 plugins one parity step against its IntelliJ counterpart, using only what the current OpenIDEA plugin API allows, and queue the upstream API work that unlocks the rest.

**Architecture:** One task per plugin (or per shared crate), each independent and shippable alone. Every task follows the shared task shape below. Gaps that need a new host API are not worked here; they are collected in Track U with the plugins each one unlocks.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK v0.1.37, `MockHost` unit tests, `crates/spring-text` for the Spring plugins.

**Spec:** [`PARITY.md`](../../../PARITY.md) (ratings, gaps, Laravel Idea inventory) and [`LIMITATIONS.md`](../../../LIMITATIONS.md) (per-plugin "Not implemented" and "possible with the current API" rows).

## Global Constraints

- Each plugin stays its own Cargo project; no root workspace; run cargo inside `plugins/<name>`.
- Request only capabilities the code uses; degradable ones go in `optional` and handle `Error::Denied`.
- `*.key` and signing keys never enter the repo.
- Versions are managed by release-please (`# x-release-please-version` in `plugin.toml`): do not hand-edit `version`; use a conventional commit `feat(<plugin>): …` or `fix(<plugin>): …`, and write the `change-notes` text for the release.
- Clean-room plugins (every Spring plugin except `spring-osgi`, `jpa-assist`, `kube-explorer`, `devcontainer`, `flyway`, `liquibase`, `openapi`, `dependency-audit`, `twig`, `blade`, `laravel`'s Idea-only features) are written from public behaviour only. The Laravel Idea inventory in `PARITY.md` lists feature names only: no code, strings, icons or names from it.
- Source ports keep `LICENSE`/`NOTICE` and the recorded source revision.
- Plugin API (WIT, SDK, host) is upstream: no vendoring or patching here.
- Editor providers have a 50 ms budget, commands 5 s (60 s inside a task); `find_files` skips hidden and gitignored paths (the mock does not): probe known paths with `read_file`.
- Every task ends by editing three files: the plugin README "Not ported" list, the plugin's `LIMITATIONS.md` section, and its `PARITY.md` row.

## Review Focus

- **Dead scaffolding:** `plugins/spring-data/src/jpql.rs` (tokenizer and declaration finder) compiles but nothing calls it (cargo: `function `words` is never used`, also `fields `entities` and `complete` are never read`). The docs are right that JPQL is not done; Task 23 wires it in and removes the dead-code warnings.
- **Mock vs host divergence:** tests pass on `MockHost` where the real host fails (hidden/ignored `find_files`, `NotFound` vs `Invalid` for a missing file). Every new `find_files` or missing-file path gets a test that treats both error kinds as absent.
- **Budget overruns:** new indexing or scanning inside a provider call must stay under 50 ms on a large project; scanning belongs on `file-opened`/`file-saved`/command, cached in memory.
- **False positives from text matching:** new inspections warn only when every type involved is a project type; each gets a test for a name that exists in a library or is built at run time and must not warn.
- **Capability creep:** any task adding `process:` or `net:` records why in `plugin.toml` and the README, and stays `optional` when the feature degrades.

## Shared task shape (referenced by every task below)

Each task's steps, unless it says otherwise:

- [ ] **Step 1:** Write the failing test(s) named in the task, in the plugin's existing test location (`tests.rs`, or `#[cfg(test)]` in the named module), against `MockHost`.
- [ ] **Step 2:** Run `cd plugins/<name> && cargo test <test_name>`; expect FAIL on the missing behaviour.
- [ ] **Step 3:** Implement in the named files; one provider/command wiring change in `lib.rs` and, if needed, `plugin.toml`.
- [ ] **Step 4:** `cargo test` (all green), then `openidea-plugin build` (runs `check`; expect no errors).
- [ ] **Step 5:** Update README "Not ported", the `LIMITATIONS.md` section and the `PARITY.md` row.
- [ ] **Step 6:** Commit `feat(<plugin>): <what>` (or `fix`).

## Track 0: Verify the baseline

### Task 0: Re-audit stale and unreviewed rows

**Files:**
- Modify: `PARITY.md`, `LIMITATIONS.md`, `plugins/spring-data/README.md`
- Read: `plugins/spring-data/src/{lib,jpql}.rs`, `plugins/spring-webflow/src/lib.rs`, `plugins/{ignore,properties,twig,blade}/`

**Interfaces:** Produces the corrected "done / missing" status each later task starts from. Steps 1 and 2 are already done; steps 3 and 4 remain (they need the Marketplace pages and the IDE).

- [x] **Step 1 (done 2026-11-06):** `jpql.rs` is unused: `cargo build` in `plugins/spring-data` warns `Word`, `words`, `Decl`, `declarations` are never used and `entities`/`complete` are never read. JPQL is not done; docs are correct.
- [x] **Step 2 (done 2026-11-06):** only `spring-webflow` and `spring-beans` call the diagram code. `spring-integration`, `spring-batch` and `spring-modulith` have none; Task 29 covers those three.
- [ ] **Step 3:** List the original features of `.ignore`, `Properties` and `Twig` from their Marketplace pages; tick done / workaround / missing in a new section of `LIMITATIONS.md` for each (these have none; also none exists for Ideolog). The expected outcome is a per-plugin backlog used by Tasks 6, 7, 15 and 17.
- [ ] **Step 4:** In the IDE (`openidea-plugin link`), open a `.blade.php` file and confirm the status-bar language is `blade`. Record the result under the Laravel/Blade section of `LIMITATIONS.md` (it is currently "read, not run").
- [ ] **Step 5:** Commit `docs: correct stale parity rows after audit`.

## Track P: Per-plugin improvements

Ordered by parity gained per effort. "Target" is the rating after the task.

### Laravel and Blade (Idea inventory driven)

#### Task 1: `laravel` unknown-name inspections
Target: Medium-High → High vs free plugin; Low-Medium → Medium vs Laravel Idea.

**Files:** Modify `plugins/laravel/src/{index.rs,refs.rs,lib.rs,tests.rs}`; `plugins/laravel/settings.schema.json` (one boolean per check).

**Interfaces:** Consumes the existing component/view index in `index.rs`. Produces problems with codes `laravel.unknown-component`, `laravel.unknown-livewire`, `laravel.unknown-livewire-directive`, `laravel.unknown-inertia-page`, `laravel.unknown-factory`.

Tests (names and assertions):
- `unknown_blade_component_warns`: `<x-nope />` with no matching file or class yields one problem; `<x-card />` with `resources/views/components/card.blade.php` yields none.
- `package_component_namespace_is_skipped`: `<x-package::alert />` yields none (packages are invisible).
- `unknown_livewire_component_warns` and `known_livewire_component_is_quiet` for `<livewire:counter />` and `@livewire('counter')`.
- `unknown_inertia_page_warns`: `Inertia::render('Missing/Page')` against `resources/js/Pages/**` and `resources/js/pages/**`, extensions `vue|jsx|tsx|svelte`.
- `unknown_factory_for_model_warns`: `Post::factory()` with no `database/factories/PostFactory.php` yields a problem.

Approach: extend the existing index with `inertia_pages` and `factories` sets, reusing the pattern of the view check.

#### Task 2: `laravel` and `blade` intentions as code actions
Target: closes five Idea intentions.

**Files:** Modify `plugins/laravel/src/{php.rs,lib.rs,tests.rs}`, `plugins/blade/src/lib.rs`, `plugins/blade/plugin.toml` (add a `code-actions` provider for `blade`; needs `[capabilities]` unchanged).

Tests:
- `validation_string_to_array`: `'required|email|max:255'` becomes `['required', 'email', 'max:255']` in the same literal; a string with `regex:/a|b/` is left alone.
- `controller_string_to_class_array`: `'UserController@index'` becomes `[UserController::class, 'index']`, and the `use` import is not added when the class is in the same namespace (otherwise the action is not offered).
- `blade_braces_escape_toggle`: `{{ $x }}` ↔ `{!! $x !!}` for the selection under the caret, in `.blade.php` only.

#### Task 3: `laravel` New-menu file templates (written from scratch)
Target: replaces ~15 of Idea's ~50 templates without file-creation API.

**Files:** Create `plugins/laravel/templates/*.php.tpl` (controller, resource controller, job, event, listener, mailable, middleware, form request, policy, observer, enum, seeder, factory, migration, Pest feature test, Pest unit test); Modify `plugins/laravel/plugin.toml` with one `[[contributes.fileTemplates]]` each.

Test: `every_template_is_registered_and_parses` iterating `plugin.toml` and asserting each `path` exists and contains no `{{` placeholder the host does not define; then `openidea-plugin check`.

Limit to record: no package or name prompts (see `LIMITATIONS.md`, plugin file templates).

#### Task 4: `laravel` artisan run configurations
**Files:** Modify `plugins/laravel/plugin.toml` (`[[contributes.runDetectors]]` marker `artisan`: `serve`, `test`, `queue:work`, `schedule:work`); optional commands `db-seed`, `migrate-fresh` in `lib.rs`.

Test: `commands_declared_in_manifest_have_handlers` (manifest ids all dispatch in `run_command`).

#### Task 5: `blade` directive completion and unknown-directive check
Target: Low-Medium → Medium.

**Files:** Modify `plugins/blade/src/lib.rs`, `plugins/blade/plugin.toml` (add `completion` provider), `plugins/laravel/src/blade.rs` (custom directives).

Tests: `completes_core_and_livewire_directives_after_at`; `completes_custom_directive_from_project` (indexes `Blade::directive('name'` in `app/Providers/**` and `app/**`, text pattern); `escaped_at_is_not_completed` (`@@if`).

### Existing text plugins, current-API wins

#### Task 6: `properties` audit follow-up and key navigation
Target: Medium → Medium-High.

**Files:** Modify `plugins/properties/src/{usages.rs,props.rs,lib.rs}`.

Tests: `go_to_definition_from_message_source_getmessage` (`messageSource.getMessage("a.b", …)`, `ResourceBundle.getBundle(...).getString("a.b")`, `th:text="#{a.b}"`, `<spring:message code="a.b"/>`, `#{a.b}` in `.html`) opens the key in the main-locale file; `missing_key_in_code_warns` only when the bundle is unambiguous. Plus the backlog items found in Task 0 step 3.

#### Task 7: `ignore` backlog from audit
**Files:** Modify `plugins/ignore/src/*` per Task 0 findings; no speculative scope. If Task 0 finds no gap, close the task and mark the row High.

#### Task 8: `dotenv` Docker `env_file` and flow-style YAML
**Files:** Modify `plugins/dotenv/src/docker.rs`, `tests` in the same module.

Tests: `compose_env_file_list_and_scalar_are_declared`; `flow_style_environment_array_is_read` (`environment: [A=1, B]`); `anchor_merge_keys_are_skipped_not_crashing`.

#### Task 9: `dependency-audit` more lockfiles, ignore list
**Files:** Modify `plugins/dependency-audit/src/{manifest.rs,osv.rs,lib.rs}`, `settings.schema.json` (`ignoredAdvisories` multiline string).

Tests: `reads_libs_versions_toml_aliases`; `reads_yarn_lock_v1_and_berry`; `reads_pnpm_lock`; `reads_cargo_lock`; `ignored_advisory_is_not_reported`. Ecosystem names follow OSV (`crates.io`, `npm`, `Maven`). Add `PyPI` last (`requirements.txt`, `poetry.lock`) only if the first four fit one release.

#### Task 10: `openapi` JSON specs and `$ref` file completion
**Files:** Modify `plugins/openapi/src/{spec.rs,checks.rs,lib.rs}`, `web/` (post referenced files).

Tests: `json_ref_resolves_and_reports_unresolved`; `ref_file_completion_uses_cached_file_list` (list built on `file-opened`, never `find_files` in the provider call); `preview_receives_referenced_files`.

#### Task 11: `flyway` migrations view
**Files:** Modify `plugins/flyway/src/lib.rs`, `plugin.toml` (`[[contributes.views]]` Tree).

Test: `info_json_parses_into_tree_items_pending_applied_failed`; run via `Project::run_process` inside a task with `-outputType=json`. Done when the tree shows state per migration and a node opens its file.

#### Task 12: `liquibase` changesets view
**Files:** Modify `plugins/liquibase/src/{changelog.rs,lib.rs}`, `plugin.toml`.

Test: `tree_lists_changesets_across_includes_in_order` (XML, YAML, formatted SQL); double-click opens the changeset line. Text index only, no process needed.

#### Task 13: `tasks` cloud trackers on wildcard hosts
**Files:** Modify `plugins/tasks/src/trackers.rs`, `plugin.toml` (`net:*.atlassian.net`, `net:*.youtrack.cloud`, `net:*.myjetbrains.com` if not present), `settings.schema.json`.

Tests: `jira_cloud_url_is_built_per_site`, `youtrack_cloud_url_is_built_per_site`. Self-hosted stays out (fixed hosts); `LIMITATIONS.md` row unchanged.

#### Task 14: `gitlab` backlog
**Files:** Modify `plugins/gitlab/src/{review.rs,lib.rs}`. Sub-tasks from the `LIMITATIONS.md` list (lines 767-776): (a) *Apply suggestions* from a review comment as a `code-actions` quick fix (test `suggestion_block_becomes_edit`); (b) paging beyond the first page: 50 merge requests, 100 threads, 100 projects (test `second_page_is_fetched_when_first_is_full`); (c) fork checkout via a fork remote instead of `refs/merge-requests/<iid>/head` (test `fork_mr_uses_fork_remote`); (d) viewed-state against the last commit that touched the file (test `viewed_compares_file_last_commit`). Markdown rendering waits for a host API.

#### Task 15: `ideolog` backlog
Ideolog's `LIMITATIONS.md` section has no "possible" list, so this task starts with Task 0 step 3 (feature audit against the original) and expands from its result. Candidate from the section: pattern tables in a multiline string could become a settings view.

#### Task 16: `symfony` backlog
**Files:** Modify `plugins/symfony/src/{index.rs,refs.rs,lib.rs}`.
Sub-tasks from `LIMITATIONS.md` lines 200-206: Twig `paths:` namespaces from `config/packages/twig.yaml`; profiler list from `var/cache/dev/profiler/index.csv` as a view; duplicate XML service keys; YAML quoting checks. Tests: `twig_paths_namespace_resolves`, `profiler_index_rows_parse`, `duplicate_service_key_warns`, `unquoted_yaml_at_sign_is_reported`. (The static Twig function/filter completion is Task 17.)

#### Task 17: `twig` completion and structure checks
Target: Low → Low-Medium.

**Files:** Modify `plugins/twig/src/lib.rs`, `plugin.toml` (add `completion`, `line-markers`/diagnostics provider).

Tests: `completes_tags_filters_functions_tests_from_static_list`; `unclosed_block_tag_is_reported` (`{% block a %}` with no `endblock`); `extends_and_include_paths_complete_from_templates_dir`.

#### Task 18: `jpa-assist` backlog
**Files:** `plugins/jpa-assist/src/{model.rs,lib.rs}`, `plugin.toml`. Sub-tasks from `LIMITATIONS.md` lines 564-568: (a) gutter icons linking an entity and its repositories (`line-markers`; test `entity_marker_targets_repository`); (b) ⌘B from a part of a derived query to the entity field (`definition`; test `derived_query_part_goes_to_field`; may overlap `spring-data`, build it once in one plugin and note the other); (c) menu items limited by `editorLangId == java|kotlin|sql` (test `menu_entries_have_when_clauses`). Generator output to files stays blocked on Track U-2.

#### Task 19: `kube-explorer` custom resources and menus
**Files:** Modify `plugins/kube-explorer/src/lib.rs`.

Tests: `api_resources_output_becomes_crd_nodes` (`kubectl api-resources -o name`); `crd_kind_lists_instances`. After upstream PR `l0ng-ai/openidea#1746` lands, switch Delete/Describe/Logs to `tree-item.actions` (separate commit).

#### Task 20: `devcontainer` templates and read-configuration
**Files:** Create `plugins/devcontainer/templates/*.json.tpl` (Node, Python, Java, PHP, Rust, Compose); Modify `plugin.toml`, `src/lib.rs` (command `info` via `devcontainer read-configuration`, 5 s).

Tests: `read_configuration_json_is_summarised_in_notification`; `all_templates_registered`.

### Quarkus and Spring

#### Task 21: `quarkus` wider extension metadata
**Files:** Modify `plugins/quarkus/scripts/*` and `data/` (generator takes artifact ids), `src/lib.rs`.

Test: `bundled_catalog_covers_top_extensions` asserting a fixed list of at least 80 extension ids resolves keys. Go from 42 to the most-used set; keep the generator reproducible.

#### Task 22: `spring-boot` bundled Spring Cloud and starter metadata
**Files:** Modify `plugins/spring-boot/scripts/*`, `data/`, `src/meta.rs`.

Tests: `spring_cloud_key_completes`, `unknown_key_inside_known_cloud_group_is_reported`. Record the pinned versions in README.

#### Task 23: `spring-data` verified follow-up
**Files:** `plugins/spring-data/src/{jpql.rs,lib.rs}`. Wire the existing `jpql::words` and `jpql::declarations` into the `@Query` check and completion: entity names after `from`/`join`, alias path properties, unknown property warning only when the entity is a project class. Then Kotlin repositories through `spring_text::kotlin`. Tests: `query_unknown_entity_warns`, `query_alias_property_completes`, `query_property_of_library_entity_is_quiet`, `kotlin_repository_derives_entity`. Done when `cargo build` has no dead-code warnings from `jpql.rs`.

#### Task 24: `spring-web` WebFlux routes and test URL links
**Files:** Modify `plugins/spring-web/src/lib.rs`, `crates/spring-text/src/java.rs` if a scanner helper is needed.

Tests: `router_function_get_registers_endpoint` (`RouterFunctions.route().GET("/x", h)` and the Kotlin `coRouter { GET("/x") }`); `mockmvc_perform_get_links_to_handler` (`get("/users/{id}")` goes to the matching mapping).

#### Task 25: `spring-security` XML namespace and Kotlin DSL
**Files:** Modify `plugins/spring-security/src/lib.rs`, `crates/spring-text/src/xml.rs`.
Tests: `intercept_url_pattern_and_access_are_checked`, `kotlin_http_dsl_flags_deprecated_matchers`.

#### Task 26: `spring-websocket` JavaScript STOMP clients
**Files:** Modify `plugins/spring-websocket/src/lib.rs`, `plugin.toml` (add providers for `javascript`, `typescript`).
Tests: `js_subscribe_links_to_send_to_destination`, `js_send_links_to_message_mapping`.

#### Task 27: `spring-messaging` placeholders and XML containers
**Files:** Modify `plugins/spring-messaging/src/lib.rs`, `crates/spring-text/src/config.rs`.
Tests: `listener_topic_placeholder_resolves_from_application_properties`, `xml_listener_container_links_to_bean`.

#### Task 28: `spring-graphql` client documents and Kotlin
**Files:** Modify `plugins/spring-graphql/src/lib.rs`.
Tests: `client_document_field_links_to_schema`, `kotlin_query_mapping_links_to_schema`.

#### Task 29: Diagrams for `spring-webflow`, `spring-integration`, `spring-batch`, `spring-modulith`
`spring-webflow` already draws one; this task covers `spring-integration` (channels and endpoints), `spring-batch` (jobs, steps, flows) and `spring-modulith` (modules and dependencies). **Files:** Modify each plugin's `src/lib.rs` to call the `spring_text` diagram code (as `spring-webflow` does) with its own node/edge builder. Tests per plugin: `diagram_has_expected_nodes_and_edges` on a fixture. Modulith adds `copy-docs` (PlantUML text through `copy_to_clipboard`, needs the `clipboard` capability, optional).

#### Task 30: Kotlin sweep across Spring plugins
**Files:** `plugins/spring-{cloud,graphql,security,shell,ws,osgi,messaging,websocket}/src/lib.rs`, `crates/spring-text/src/kotlin.rs`.
Use the existing Kotlin scanner; one test per plugin (`kotlin_<annotation>_is_indexed`). Do not touch beans' type inference.

#### Task 31: `spring-beans` namespace and constructor-injection merge
**Files:** Modify `plugins/spring-beans/src/{sites.rs,model.rs,kotlin.rs}`.
Tests: `p_and_c_namespace_refs_are_followed`, `kotlin_bean_fun_without_return_type_uses_expression_type_when_obvious`, `use_constructor_injection_merges_into_existing_constructor`.

#### Task 32: `spring-initializr` CLI project generation
**Files:** Modify `plugins/spring-initializr/src/lib.rs`, `plugin.toml` (`optional = ["process:spring"]`).
Test: `spring_init_args_match_form_values` (build tool, language, Boot version, coordinates, deps). Runs `spring init` in `open_terminal`; the browser path stays the fallback.

#### Task 33: `spring-runtime` authenticated Actuator and more endpoints
**Files:** Modify `plugins/spring-runtime/src/{lib.rs,snapshot.rs}`, `plugin.toml` (`[[contributes.secrets]] actuator-token`, header `Authorization: Bearer {{secret:actuator-token}}`).
Tests: `bearer_header_uses_secret_substitution`, `loggers_and_caches_endpoints_parse`.

### Remaining (no current-API work found)

`spring-shell`, `spring-ws`, `spring-osgi`, `blade` (beyond Task 5), `quarkus` language-server features, `devcontainer` open-in-container: no further current-API items beyond Tasks 5, 21, 30 and 20; they move only with Track U.

## Track U: Upstream API work (`l0ng-ai/openidea`, one PR each)

Each is its own pull request; none is vendored here. Unlock list = plugins whose rating moves one step.

| # | API | Plugins unlocked |
|---|---|---|
| U1 | `references`, `rename`, `hover` provider kinds | all Spring, `dotenv`, `openapi`, `kube-explorer`, `symfony`, `laravel` |
| U2 | File creation in code-action edits (`create-file`) | `jpa-assist`, `laravel` generators, `quarkus`/`spring-initializr` wizards, `flyway`, `symfony`, all "create missing X" fixes |
| U3 | `find_files` include-ignored option and `files-changed` event | every indexed plugin; fixes `.devcontainer`, `target/`, `.env` discovery |
| U4 | Java/PHP type resolution (`lsp.request` or `resolve-type`) | all Spring, `jpa-assist`, `laravel` Eloquent/facades, `symfony` |
| U5 | `ui.show-quick-pick`, `ui.show-input`, `views.reveal` | `tasks`, `gitlab`, `devcontainer`, `liquibase`, `flyway`, `laravel` `make:*` |
| U6 | Provider `patterns` and `trigger-characters`; syntax for built-in language ids | `dotenv`, `flyway`, `blade`, `ideolog` |
| U7 | Diagnostics provider / `document-changed` event | every plugin with problems (re-check while typing) |

For each, the first step is to open an issue in `l0ng-ai/openidea` with the `LIMITATIONS.md` row as the use case and the unlock list as the justification; the PR follows the repo's `writing-openidea-plugins` reference for WIT changes. Start with U3 and U1: they unlock the most plugins.

## Self-Review

- **Spec coverage:** every plugin in `PARITY.md` appears in Tasks 1–33 or the "Remaining" list; each cross-cutting gap maps to U1–U7. Not-possible items (Eloquent magic, database, inlay hints, postfix templates, project wizards) are deliberately not tasked.
- **Placeholders:** Tasks 14, 16 and 18 now carry their sub-tasks from the `LIMITATIONS.md` lists. Tasks 7 and 15 still wait on Task 0 step 3 (no documented backlog for `ignore`, `properties`, `twig`, `ideolog`).
- **Consistency:** problem codes in Task 1 are the only new identifiers other tasks rely on (none do).
- **Proportion:** tasks give files, test names and assertions, not bodies.

## Order of execution

1. Task 0. 2. Tasks 1–5 (Laravel/Blade, freshest baseline). 3. Tasks 9, 8, 6, 13, 21, 22 (small, independent). 4. Tasks 10–12, 17, 19, 20. 5. Spring Tasks 23–33 in any order after Task 0. 6. Tasks 14, 16, 18 (lists known), then 7 and 15 after Task 0 step 3. Track U runs in parallel from day one.
