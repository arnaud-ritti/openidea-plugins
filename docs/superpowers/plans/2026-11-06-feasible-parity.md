# Feasible Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build every improvement the audit classed **F** (feasible) or **F\*** (feasible with a stated risk): about 27 verdict rows across 16 plugins, mostly inspections, caret intentions and New-menu templates.

**Architecture:** Every task adds text-based checks or contributions to an existing plugin: `publish_diagnostics` on `file-opened`/`file-saved`, `code-actions` providers (they receive the caret as `selection`), `completion`/`definition` providers, `fileTemplates`, commands over `process:` programs. No new API is assumed. Tasks are independent and shippable alone, except where a task lists a dependency.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK 0.1.37 (`openidea:plugin@1.1.0`), `MockHost` tests, `crates/spring-text` (`java::parse`, `Member`, `Annotation`, `JavaFile`) for the Spring plugins.

**Spec:** [`docs/audit-2026-11-06.md`](../../audit-2026-11-06.md) (verdict table, _Feasibility check_, _Limitations of the audit_), [`PARITY.md`](../../../PARITY.md) (_Audit update 2026-11-06_). Supersedes the overlapping tasks of [`2026-11-06-parity-improvement.md`](2026-11-06-parity-improvement.md); the Track U (upstream) section of that plan still applies to every **B** row, which is out of scope here.

## Baseline: what `parity-improvement` already did

An earlier session executed [`2026-11-06-parity-improvement.md`](2026-11-06-parity-improvement.md) on branch `parity-improvement` (worktree `/Users/aritti/Projects/openidea-plugins-parity`, head `e4ef2e5`, 108 commits ahead of `main`, merged from `parity-a1`, `parity-a2`, `parity-b`, `parity-d`). Checked 2026-11-06: `git merge-tree` against `main` is conflict-free on committed files, all 36 plugins pass `cargo test` on the branch (more tests than on `main`, for example `laravel` 73 against 42, `spring-data` 20 against 5, `spring-web` 13 against 5), and there are no dead-code warnings (`jpql.rs` is wired in).

Each task below carries a status tag against that branch. The tags come from grepping the branch for the check codes and features and from its test results, not from reading every diff. Verify the first test of a task still fails on the branch before implementing it (Step 2 of the shared shape does this).

| Status | Tasks |
|---|---|
| DONE | 5 (`spring-data` JPQL, `@Modifying`, pageable), 13 (custom Blade directives), 16 (Laravel unknown-name checks, 16 templates, artisan run detectors), 22 (Ideolog Laravel and Monolog formats) |
| PARTIAL | 12 (`blade`: `@` directive completion exists in `aritti.laravel`; the two inspections are open), 4 (`spring-security`: duplicate and unreachable patterns exist; no `anyRequest` ordering or debug check), 6 (`spring-web` WebFlux routes and test-URL links exist; no version/`@InitBinder` checks; other plugins none), 11 (`symfony`: duplicate XML service, unquoted YAML scalar, Twig paths, Twig completion, unclosed block, profiler view exist; no other inspections, no templates), 14 (`laravel`: string-to-array, controller-call and braces intentions exist; no Eloquent inspections, no rule-array-to-string, no scope-to-attribute), 20 (`devcontainer`: templates and `read-configuration` exist; no inspections), 21 (`flyway` and `liquibase` views, `openapi` JSON specs and the Liquibase duplicate-changeset check exist; only templates remain) |
| OPEN | 1, 2, 3, 7, 8, 9, 10, 15, 17, 18, 19 |

Also done on the branch and not in this plan: dependency-audit lockfiles and ignore list, dotenv `env_file`, Kotlin indexing across the Spring plugins, Spring diagrams (integration, batch, modulith), messaging placeholders, GraphQL client documents, WebSocket JS clients, `kube-explorer` custom resources, `gitlab` paging, suggestions and viewed state, `jpa-assist` entity gutter and menus, `properties` key navigation, `ignore` template and redundant globs, Ideolog Next Warning/Exception and more formats, `quarkus` metadata (2,321 to 2,986 lines).

**Branch hygiene found while checking:** the branch's `PARITY.md` still says of Laravel Idea that "no decompiled code was read or kept", and rates `properties`/`ignore` from its own audit; the root checkout has an untracked `PARITY.md`, modified `LIMITATIONS.md` and 36 modified `plugin.toml` files (release-please markers), so merging needs those committed or stashed first, and `PARITY.md`/`LIMITATIONS.md` will conflict textually.

## Global Constraints

- Each plugin stays its own Cargo project; no root workspace; run cargo inside `plugins/<name>` with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Request only capabilities the code uses; degradable ones go in `optional` and handle `Error::Denied`.
- Versions are managed by release-please: never hand-edit `version`; use conventional commits `feat(<plugin>): …` / `fix(<plugin>): …`. Commit message ends with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`.
- Clean-room plugins (all `spring-*` except `spring-osgi`, `jpa-assist`, `kube-explorer`, `devcontainer`, `flyway`, `liquibase`, `openapi`, `twig`, `blade`, and the Laravel Idea-only features) are written from public documentation and observed behaviour only. The audit saw the originals' names, not their rules or messages: **write your own messages and thresholds** from the Spring/Kubernetes/Laravel/Symfony docs. Nothing from the decompiled output (`/tmp/port`) may enter the repo.
- Source ports (`gitlab`, `quarkus`, `symfony`, `ideolog`, `laravel` free part) keep `LICENSE`/`NOTICE`; new Symfony/Quarkus checks that the original does not have in its public source are written from docs too.
- Providers: 50 ms budget, commands/events 5 s (60 s inside a task). Scan in events or commands, cache in memory, persist to `host::storage` (1 MiB) if rebuilding on `activate` is too slow (instances are reclaimed after 5 min idle). `find_files` skips hidden and ignored paths on the real host (the mock does not); a missing file is `Invalid` on the host, `NotFound` on the mock.
- Diagnostics: absolute path of a file in an open project; ranges use UTF-16 columns; set `code` on every problem; each check group gets a boolean in the plugin's `settings.schema.json` (default on, except where a task says default off).
- A plugin that declares a provider for `yaml`, `javascript`, `typescript` or `xml` loads for every such file: the provider returns immediately unless the path/content matches.

## Review Focus

Failure modes the audit implies and no single task's happy path exercises; each line has its test named in the owning task.

- **False positives from text matching:** a bean, class or key that exists in a library or is built at run time must not warn. Test per inspection: `<check>_is_quiet_for_library_or_dynamic_name`.
- **Mock vs host divergence:** a missing file or a hidden/ignored directory must read as "absent", not as an error. Test per task that reads files: `missing_file_is_treated_as_absent` using both error kinds.
- **Provider loaded for unrelated files:** the YAML/JS/XML providers and diagnostics must be silent for non-matching files. Tests: `yaml_provider_ignores_unrelated_files`, `js_provider_ignores_non_inertia_calls`.
- **Non-ASCII and CRLF input:** ranges must land on the right characters. Test: `ranges_use_utf16_columns_with_multibyte_text` and `crlf_files_report_correct_lines` in each plugin that gains a scanner.
- **Stale diagnostics:** fixing a problem and saving must clear it, and closing the cause (deleting the annotation) must not leave a ghost. Test per check group: `fixed_file_publishes_empty_list`.

## Shared task shape

Unless a task says otherwise, steps are:

- [ ] **Step 1:** Write the failing tests named in the task, in the plugin's test module (`tests.rs`, or `#[cfg(test)]` in the named file), against `MockHost`.
- [ ] **Step 2:** `cd plugins/<name> && cargo test <test_name>`; expect FAIL on the missing behaviour.
- [ ] **Step 3:** Implement in the named files; wire it in `lib.rs` (and `plugin.toml` / `settings.schema.json` when listed).
- [ ] **Step 4:** `cargo test` all green with no new warnings, then `openidea-plugin build` (runs `check`); expect no errors.
- [ ] **Step 5:** Update the plugin README "Not ported", its `LIMITATIONS.md` section and its `PARITY.md` row (class F/F\*/B and rating).
- [ ] **Step 6:** Commit `feat(<plugin>): <what>`.

---

## Phase 0: Land the baseline and de-risk the F\* rows

### Task 0 [PARTLY DONE]: Land the audit work on `main`

`parity-improvement` is merged (`efb7abe`, 2026-11-06 13:20). What remains uncommitted in the root checkout: 36 `plugin.toml` release-please markers (including `laravel` and `blade`), `release-please-config.json`, `.release-please-manifest.json`, 21 new `plugins/*/LICENSE` files, `docs/audit-2026-11-06.md`, this plan, and the re-applied `PARITY.md` section _Audit update 2026-11-06_ (the first version was lost when the untracked file was replaced by the merge).

- [ ] **Step 1:** `git diff --stat` shows only markers and the `PARITY.md` edit as tracked changes; `git status --short` lists the files above plus `.env`, `.openidea/`, `.github/`, `.claude/skills/publishing-openidea-plugins/`. Do not stage `.env`; ask before staging the others.
- [ ] **Step 2:** Commit `chore: release-please markers, config and manifest, LICENSE files for MIT plugins`, then `docs: audit parity against the originals, add feasibility plan`.
- [ ] **Step 3:** Decide with the owner whether `docs/audit-2026-11-06.md` (which names the decompile) is published; `PARITY.md` carries only the short factual note.
- [ ] **Step 4:** `for d in plugins/*/; do (cd "$d" && openidea-plugin build) || echo "FAILED: $d"; done` on the merged `main`; expect no `FAILED` line.

### Task 1 [OPEN]: Probe the three untested IDE behaviours

Gates Task 12 (Blade completion), 15 (Inertia/Ziggy), 8 (GitLab CI navigation) and 18 (YAML intentions).

**Files:** a throwaway plugin in `/tmp/probe` (outside the repo); results go to `LIMITATIONS.md` under _All plugins_.

**Interfaces:** Also settles the ledger's unverified items (host language ids `jsp`/`vue`/`javascriptreact`/`typescriptreact`, template placeholders `$NAME$`/`$PACKAGE$`/`$END$`, prefilled code-action edits without `resolve-code-action`, `.blade.php` to `blade`); until it runs, template and intention tasks are F\*. Produces three recorded answers: (a) does a `completion` provider on a plugin-owned language open on `@` with no trigger characters; (b) is `definition` on `yaml` called for a file the plugin does not own, and what is `doc.path`; (c) the activation cost (Settings › Plugins call latency) of a plugin declaring `languages = ["yaml"]`, `["javascript"]`.

- [ ] **Step 1:** `openidea-plugin new /tmp/probe`, one provider for `yaml` returning a fixed location and one completion for a language file `.probe` that logs `trigger`.
- [ ] **Step 2:** `openidea-plugin link` it, open a `.gitlab-ci.yml`, a `.probe` file, a JS file; record behaviours in the IDE.
- [ ] **Step 3:** Write the three answers into `LIMITATIONS.md` (_All plugins_). If (a) fails, mark Task 12's completion F\*→B and keep its inspections; if (b) fails, Tasks 8 and 18 keep diagnostics only.
- [ ] **Step 4:** Commit `docs: record provider behaviour probes`.

---

## Phase 1: Spring text-matchable inspections (largest gap: 1 check against ~64)

Each task adds `src/checks.rs` to the plugin (pure functions over `spring_text::java::JavaFile`/text returning `Vec<Finding>` with `start`, `end`, `code`, `message`), publishes them where the plugin already publishes (or on `file-opened`/`file-saved`), and a boolean setting. Pattern to follow: `publish_field_injections` in `plugins/spring-beans/src/lib.rs`.

### Task 2 [OPEN]: `spring-beans` core inspections

**Files:** Create `plugins/spring-beans/src/checks.rs`; modify `src/lib.rs`, `src/tests.rs`, `plugin.toml` (activation events `on-event:file-opened`, `file-saved` if absent), `settings.schema.json`.

**Interfaces:** Produces `pub fn check_java(file: &java::JavaFile, text: &str, index: &BeanIndex) -> Vec<Finding>` where `Finding { start: usize, end: usize, code: &'static str, message: String }`. Codes: `non-proxyable-method`, `scheduled-signature`, `duplicate-bean-name`, `configuration-proxy-method`, `bean-name-convention`, `self-invocation`.

Tests (names; each asserts the code and the highlighted text):
- `transactional_on_private_method_warns`, `async_on_final_method_warns`, `cacheable_on_static_method_warns`; `transactional_on_public_method_is_quiet`.
- `scheduled_with_parameters_warns`; `scheduled_without_parameters_is_quiet`.
- `duplicate_bean_names_across_files_warn_on_both`; `same_name_in_different_profiles_is_quiet`; `component_in_library_name_is_quiet`.
- `configuration_with_final_bean_method_warns`; `proxy_bean_methods_false_is_quiet`.
- `capitalised_component_name_gets_a_hint`.
- F\*: `self_call_to_transactional_method_warns_only_for_unique_name` (an overloaded or inherited name is quiet); `call_through_injected_self_is_quiet`.
- Review Focus tests: `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`.

- [ ] Steps: shared shape. `self-invocation` ships default **off** (setting `springBeans.checkSelfInvocation`).

### Task 3 [OPEN]: `spring-beans` unresolved references (F\*, default off)

**Files:** Modify `plugins/spring-beans/src/{checks.rs,lib.rs,tests.rs}`, `settings.schema.json`.

**Interfaces:** Consumes `BeanIndex` from Task 2. Adds codes `unresolved-depends-on`, `component-scan-package-missing`.

Tests: `depends_on_unknown_name_warns_when_enabled`; `depends_on_is_quiet_when_project_uses_auto_configuration` (the project has `@SpringBootApplication` and the name is not project-shaped); `depends_on_known_xml_bean_is_quiet`; `component_scan_base_package_without_matching_sources_warns`; `component_scan_with_class_reference_is_quiet`.

- [ ] Steps: shared shape. Document in README that library and auto-configured beans are invisible, so the check is off by default.

### Task 4 [PARTIAL]: `spring-security` matcher checks

**Files:** Modify `plugins/spring-security/src/lib.rs` (add `checks` module), tests in the same file.

Codes `matchers-after-any-request`, `matcher-order`, `debug-enabled`.

Tests: `request_matchers_after_any_request_warn`; `narrow_matcher_after_broader_matcher_warns` (`requestMatchers("/**")` before `requestMatchers("/admin/**")`); `ordered_matchers_are_quiet`; `enable_web_security_debug_true_warns`; `lambda_dsl_and_kotlin_dsl_are_both_read`.

- [ ] Steps: shared shape.

### Task 5 [DONE on parity-improvement]: `spring-data` checks and JPQL wiring

**Files:** Modify `plugins/spring-data/src/{lib.rs,jpql.rs}`, tests.

Codes `modifying-missing`, `pageable-missing`, `return-type`, `unknown-entity`, `unknown-property`.

Tests: `update_query_without_modifying_warns`; `page_return_without_pageable_parameter_warns`; `unknown_entity_in_jpql_warns_only_for_project_entities`; `alias_property_of_library_entity_is_quiet`; `kotlin_repository_derives_entity`. Done when `cargo build` prints no dead-code warnings from `jpql.rs` (wire `jpql::words`/`declarations`, delete what stays unused).

- [ ] Steps: shared shape.

### Task 6 [PARTIAL]: `spring-web`, `spring-boot`, `spring-cloud`, `spring-graphql`, `spring-batch`, `spring-websocket` small checks

One commit per plugin. Each adds a `checks` module with one or two codes, tests `<code>_warns` and `<code>_is_quiet_for_library_or_dynamic_name`:

- `spring-web`: `api-version-mismatch` (a `version` attribute on a mapping with no configured version strategy), `init-binder-signature`.
- `spring-boot`: `test-constructor-autowire`, `event-listener-signature`.
- `spring-cloud`: `stream-channel-unbound`, `bootstrap-config-missing-dependency`.
- `spring-graphql`: `unmatched-mapping` (a `@QueryMapping`/`@SchemaMapping` field not in any `*.graphqls`; skip when no schema file exists).
- `spring-batch`, `spring-websocket`: one configuration check each, taken from the framework docs.

- [ ] Steps: shared shape per plugin.

### Task 7 [OPEN]: `jpa-assist` checks

**Files:** Modify `plugins/jpa-assist/src/{model.rs,lib.rs}`, tests.

Codes `entity-missing-id`, `attribute-type`, `entity-listener-signature`.

Tests: `entity_without_id_warns`; `entity_with_embedded_id_is_quiet`; `mapped_superclass_id_is_inherited` (superclass in project only; library superclass is quiet); `listener_method_with_wrong_parameters_warns`.

- [ ] Steps: shared shape.

---

## Phase 2: Source-port gaps

### Task 8 [OPEN]: `gitlab` CI file support

**Files:** Create `plugins/gitlab/src/ci.rs`; modify `src/lib.rs`, `plugin.toml` (provider `kinds = ["definition"]`, `languages = ["yaml"]`; activation `on-file:**/.gitlab-ci.yml`, `on-file:**/*.gitlab-ci.yml`), `src/tests.rs`.

**Interfaces:** Produces `pub fn parse(text: &str) -> CiFile` with `jobs: Vec<Job { name, name_range, needs: Vec<Ref>, extends: Vec<Ref>, stage: Option<Ref>, script: Vec<Line> }>`, `stages: Vec<Ref>`, and `pub fn check(file: &CiFile) -> Vec<Finding>`; `pub fn is_ci_path(path: &str) -> bool` (the six patterns of the existing `gitlab-ci` file icon). Codes `undefined-job`, `undefined-stage`, `duplicate-job-usage`, `single-quoted-shell-variable`.

Tests: `needs_unknown_job_warns`; `extends_hidden_job_is_defined`; `stage_not_in_stages_warns`; `default_stages_are_known_when_stages_absent`; `duplicate_needs_entry_warns`; `single_quoted_dollar_variable_in_script_warns`; `double_quoted_variable_is_quiet`; `include_present_makes_unknown_jobs_quiet` (jobs from `include:` files are invisible); `yaml_provider_ignores_unrelated_files`; `definition_on_extends_opens_job`.

- [ ] Steps: shared shape. The reader is a block scanner (top-level keys, `needs:`/`extends:` lists, `script:` blocks), not a YAML parser; unknown shapes yield no findings.

### Task 9 [OPEN]: `gitlab` worktree and update branch (F\*)

**Files:** Modify `plugins/gitlab/src/{review.rs,branch.rs,lib.rs}`, `plugin.toml`, tests.

Commands `Open Merge Request in Worktree` (`git worktree add <path> origin/<branch>` via `process:git`, then notify the path; no editor open) and `Update Merge Request Branch` (`PUT /projects/:id/merge_requests/:iid/rebase`).

Tests: `worktree_args_use_the_merge_request_branch`; `update_branch_request_targets_rebase_endpoint`; `update_branch_conflict_response_is_reported`.

- [ ] Steps: shared shape.

### Task 10 [OPEN]: `quarkus` property checks

**Files:** Modify `plugins/quarkus/src/{lib.rs,java.rs}`, create `src/checks.rs`, tests in `lib.rs`.

**Interfaces:** `pub fn check_properties(text: &str, meta: &Catalog) -> Vec<Finding>`. Codes `duplicate-key`, `missing-separator`, `unassigned-value`, `invalid-value`, `undefined-expression`, `missing-required` (F\*, project-wide, default off).

Tests: `duplicate_key_in_same_profile_warns`; `same_key_in_different_profiles_is_quiet`; `line_without_equals_warns`; `empty_value_warns_unless_optional_in_metadata`; `enum_value_not_in_metadata_warns`; `duration_and_memory_size_values_are_accepted`; `unknown_extension_value_is_quiet`; `unresolved_expression_warns_unless_env_or_system_property`; `yaml_files_are_checked_the_same_way`.

- [ ] Steps: shared shape.

### Task 11 [PARTIAL]: `symfony` text inspections and New-menu templates

**Files:** Create `plugins/symfony/src/checks.rs`, `plugins/symfony/templates/*.php`; modify `src/{lib.rs,index.rs}`, `plugin.toml`, `settings.schema.json`.

**Interfaces:** Consumes the existing index. Codes (one boolean per group): `controller-method-missing`, `deprecated-route-setting`, `deprecated-service-class`, `duplicate-service-key` (XML), `duplicate-parameter-key` (XML), `unquoted-at-sign` (YAML), `named-argument-missing`, `form-type-string`, `missing-asset`, `missing-translation-domain`, `twig-missing-route`, `twig-missing-asset`.

Tests (per code `<code>_warns` and `<code>_is_quiet_for_library_or_dynamic_name`) plus `twig_checks_run_without_the_twig_language_provider`. Templates (`fileTemplates`, fixed `namespace App\…` per type, `$NAME$` for the class): Command, Controller, Form type, Twig extension, Compiler pass, Kernel test, Web test, Event subscriber, Voter.

Test `every_template_is_registered_and_exists` iterating `plugin.toml`.

- [ ] Steps: shared shape; split the commit in three (route/controller group, service/XML/YAML group, Twig group, templates).

---

## Phase 3: Laravel and Blade

### Task 12 [PARTIAL]: `blade` inspections (directive completion already lives in `aritti.laravel`)

**Files:** Modify `plugins/blade/src/lib.rs` (move checks to `src/checks.rs`), `plugin.toml` (`completion` provider for `blade`, activation `on-event:file-opened`), tests.

**Interfaces:** `pub fn check(text: &str) -> Vec<Finding>` reusing the pair matcher used for folding. Codes `unpaired-directive`, `unpaired-parenthesis-in-string`. `fn complete(doc: &Document, at: Position, trigger: Option<String>) -> Vec<CompletionItem>`.

Tests: `endif_without_if_warns`; `if_without_endif_warns`; `elseif_inside_if_is_quiet`; `verbatim_content_is_not_checked`; `unbalanced_parenthesis_in_string_argument_warns`; `completes_core_and_livewire_directives_after_at`; `escaped_at_is_not_completed`; `email_address_is_not_completed`.

- [ ] Steps: shared shape. If Task 1 answer (a) is "no", ship inspections only and document completion as B.

### Task 13 [DONE on parity-improvement]: Custom Blade directives (F\*)

**Files:** Create `plugins/laravel/src/directives.rs`; modify `plugins/blade/src/lib.rs`, `plugins/laravel/{lib.rs,index.rs,plugin.toml}`.

**Interfaces:** `pub fn scan(text: &str) -> Vec<String>` (names in `Blade::directive('name'`); the Laravel index stores them under `host::storage` key `blade-directives`; `aritti.blade` cannot read another plugin's storage, so the **blade** plugin runs the same scan itself over `app/**/*.php` (cached, rebuilt on `file-saved`).

Tests: `finds_directive_names_in_providers`; `ignores_commented_directives`; `directive_completes_after_at`; `scan_is_skipped_when_vendor_only`; `missing_file_is_treated_as_absent`.

- [ ] Steps: shared shape.

### Task 14 [PARTIAL]: `laravel` Eloquent inspections and intentions

**Files:** Create `plugins/laravel/src/eloquent_checks.rs`, `src/intentions.rs`; modify `src/{lib.rs,php.rs,tests.rs}`, `plugin.toml` (provider `code-actions` for `php`, `blade`).

**Interfaces:** `code_actions(doc, selection) -> Vec<CodeAction>` returning edits for the open file only. Inspection codes `guarded-attribute-assignment`, `unscheduled-prunable`, `simplify-order-by`. Intentions (kind `refactor`): `validation-string-to-array`, `rule-array-to-string`, `controller-call-to-class-array`, `blade-braces-escape`, `blade-braces-unescape`, `scope-call-to-attribute` (dropped: `PARITY.md` rates it B, it needs the model's scope definition).

Tests: `guarded_attribute_assigned_through_fill_warns`; `prunable_model_without_schedule_warns`; `order_by_created_at_desc_becomes_latest`; `validation_string_becomes_array`; `regex_rule_with_pipe_is_left_alone`; `controller_string_becomes_class_array_and_adds_import`; `no_import_added_when_class_in_same_namespace`; `blade_braces_toggle_only_in_blade_files`; `intentions_are_offered_only_at_the_caret`.

- [ ] Steps: shared shape; commit inspections and intentions separately.

### Task 15 [OPEN]: `laravel` JS/TS completion (F\*)

**Files:** Modify `plugins/laravel/src/{lib.rs,refs.rs}`, `plugin.toml` (`completion` + `definition` for `javascript`, `typescript`, `vue`; activation `on-language:vue` stays off).

Tests: `inertia_render_completes_page_names_from_resources_js_pages`; `inertia_page_extensions_vue_jsx_tsx_svelte`; `ziggy_route_completes_route_names`; `js_provider_ignores_non_inertia_calls`; `completion_does_not_scan_project` (works from the cache with an empty project).

- [ ] Steps: shared shape. Gate: Task 1(c) activation cost acceptable; otherwise declare `on-file:resources/js/**` only.

### Task 16 [DONE on parity-improvement]: `laravel` unknown-name inspections and New-menu templates

**Files:** `plugins/laravel/src/{index.rs,refs.rs,lib.rs,tests.rs}`, `plugins/laravel/templates/*.php`, `plugin.toml`. Supersedes Tasks 1, 3 and 4 of the earlier plan.

Codes `unknown-blade-component`, `unknown-livewire-component`, `unknown-inertia-page`, `unknown-factory`. Templates (fixed `App\…` namespaces): controller, resource controller, job, event, listener, mailable, middleware, form request, policy, observer, enum, seeder, factory, migration, Pest feature/unit test. Run detectors: `artisan serve`, `test`, `queue:work`, `schedule:work`.

Tests: `unknown_blade_component_warns`; `package_component_namespace_is_skipped`; `unknown_livewire_component_warns`; `known_livewire_component_is_quiet`; `unknown_inertia_page_warns`; `unknown_factory_for_model_warns`; `every_template_is_registered_and_exists`; `run_detectors_use_marker_artisan`.

- [ ] Steps: shared shape.

### Task 17 [OPEN (optional)]: `laravel` Pest navigation and FormRequest promotion (F\*, optional)

**Files:** `plugins/laravel/src/{php.rs,lib.rs,tests.rs}`.

Tests: `pest_it_description_links_to_matching_subject_only_when_unique`; `promote_request_to_form_request_offered_only_when_class_exists`. Skip the task if both tests need guesses (document why).

- [ ] Steps: shared shape.

---

## Phase 4: Kubernetes, dev containers, config

### Task 18 [OPEN]: `kube-explorer` manifest inspections and intentions

**Files:** Create `plugins/kube-explorer/src/{manifest.rs,checks.rs}`; modify `src/lib.rs`, `plugin.toml` (provider `code-actions` for `yaml`; activation `on-file:**/*.yaml` is too broad, use `on-event:file-opened` and test the first lines), tests.

**Interfaces:** `pub fn documents(text: &str) -> Vec<Doc { api_version, kind, name, range, start_line }>`; `pub fn check(docs: &[Doc], text: &str) -> Vec<Finding>`. Codes `deprecated-api-version` (fixed table: `extensions/v1beta1`, `apps/v1beta1`, `apps/v1beta2`, `batch/v1beta1`, `networking.k8s.io/v1beta1`, `policy/v1beta1`), `duplicate-env-name`, `missing-name`, `missing-selector`, `helm-whitespace-trim`, `chart-missing-key`. Intentions `list-to-documents`, `documents-to-list`.

Tests: `non_kubernetes_yaml_is_ignored`; `deprecated_ingress_api_version_warns`; `duplicate_env_name_in_container_warns`; `deployment_without_selector_warns`; `multi_document_file_reports_per_document`; `list_becomes_separate_documents`; `documents_become_list`; `chart_yaml_missing_version_warns`; `yaml_provider_ignores_unrelated_files`.

- [ ] Steps: shared shape.

### Task 19 [OPEN]: `kube-explorer` templates, debug container, secrets (F\*)

**Files:** `plugins/kube-explorer/templates/*.yaml`, `src/lib.rs`, `plugin.toml`, tests.

Templates written from the Kubernetes docs: Pod, Deployment, StatefulSet, DaemonSet, Job, CronJob, Service, Ingress, ConfigMap, Secret, Namespace, Role, RoleBinding, ClusterRole, ClusterRoleBinding. Commands: `Debug Pod (ephemeral container)` via `open_terminal("kubectl", ["debug", …])`; `Show Secret` (decode in the plugin, values shown only in a Form view, setting `kubeExplorer.allowRevealSecrets` default off) and `Create Secret` (builds a manifest, pipes it to `kubectl apply -f -` through `stdin`, never in arguments).

Tests: `every_template_is_registered_and_exists`; `debug_args_include_context_namespace_and_target`; `secret_values_are_base64_decoded`; `secret_reveal_is_denied_when_setting_off`; `create_secret_passes_values_on_stdin_not_args`.

- [ ] Steps: shared shape.

### Task 20 [PARTIAL]: `devcontainer` inspections and templates

**Files:** `plugins/devcontainer/src/lib.rs`, `plugin.toml`, `templates/*.json`. Codes `unknown-property`, `features-malformed`; command `Read Configuration` via `devcontainer read-configuration` (5 s). Tests: `unknown_top_level_property_warns`; `read_configuration_json_is_summarised_in_notification`; `all_templates_registered`. Superseded plan Task 20 is replaced.

- [ ] Steps: shared shape. Remember `find_files` skips `.devcontainer`: probe `.devcontainer/devcontainer.json` with `read_file`.

### Task 21 [PARTIAL]: `flyway`, `liquibase`, `openapi` leftovers

- [ ] **Step 1:** `plugins/liquibase`: the duplicate-changeset check exists in `src/checks.rs`; confirm `unresolved-property` and add it if absent with the test `duplicate_id_and_author_warns` and `unresolved_property_placeholder_warns`. Add changelog `fileTemplates` (XML, YAML).
- [ ] **Step 2:** `plugins/openapi`: JSON specs and `$ref` resolution (superseded plan Task 10): tests `json_ref_resolves_and_reports_unresolved`, `ref_file_completion_uses_cached_file_list`. Add OpenAPI/AsyncAPI templates.
- [ ] **Step 3:** `plugins/flyway`: migrations view (superseded plan Task 11): test `info_json_parses_into_tree_items_pending_applied_failed`.
- [ ] **Step 4:** Shared shape steps 4-6 for each plugin; one commit per plugin.

### Task 22 [DONE on parity-improvement]: `ideolog` Laravel log format

**Files:** `plugins/ideolog/settings.schema.json` (default of the custom-formats string), `src/log.rs` tests.

Tests: `laravel_log_line_is_parsed_with_the_default_pattern` (`[2026-11-06 10:00:00] local.ERROR: message`); `stack_trace_lines_stay_in_the_event`. Pattern written from Laravel's Monolog format, not from the wiki file.

- [ ] Steps: shared shape.

### Task 23: Small existing-plugin items still marked F in the audit

- [ ] `tasks`: nothing feasible (all **B**). `properties`: nothing (all **B**). `dotenv` Docker `env_file` and flow-style YAML (earlier plan Task 8) is extra, not parity: optional.
- [ ] `dependency-audit` is outside this audit: leave to the earlier plan (Task 9).

---

## Order of execution

1. Task 0, then Task 1 (blocks 8, 12, 15, 18 only for their provider parts; their diagnostics can start earlier).
2. Phase 1 in order 2, 4, 5, 6, 7, 3.
3. Task 8 (gitlab CI), 10 (quarkus), 11 (symfony).
4. Phase 3: 16, 14, 12, 13, 15, 17.
5. Phase 4: 18, 19, 20, 21, 22.

## Self-Review

- **Spec coverage:** every **F**/**F\*** row of the audit's verdict table maps to a task: spring-beans (2, 3), spring-security (4), spring-data (5), spring-web (6), gitlab (8, 9), quarkus (10), symfony inspections and templates (11), blade (12, 13), laravel (13-17), kube-explorer (18, 19), ideolog (22), templates for symfony/laravel/kube/devcontainer (11, 16, 19, 20). The jpa-assist, spring-boot/cloud/graphql/batch/websocket rows are in Tasks 6 and 7. **B** rows are deliberately absent; `tasks` and `properties` have no F rows.
- **Placeholders:** Tasks 6 and 7 carry check names but not the rules of each check; those come from the framework docs at implementation time (the audit saw only names). That is a deliberate decision, stated in Global Constraints, not an omission.
- **Consistency:** `Finding { start, end, code, message }` is defined in Task 2 and reused unchanged in Tasks 4-8, 10-12, 18. `BeanIndex` is defined in Task 2 and consumed in Task 3. The `is_ci_path` patterns equal the existing `gitlab-ci` file icon patterns.
- **Proportion:** tasks give files, signatures, codes and test names; no bodies. Open risk: Task 1 may downgrade Tasks 12, 15, 8 and 18's provider parts to diagnostics-only.
