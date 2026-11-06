# Gap F Rows of the Rest of Spring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the rows the gap audit classed **F** for `spring-boot`, `spring-data`, `spring-web`, `spring-security`, `spring-graphql` and `spring-websocket`: 8 Java/Kotlin text checks, 2 caret code actions, 1 file-icon contribution and 2 tool-window views.

**Architecture:** Checks are pure functions over `spring_text::java::JavaFile` returning `Vec<Finding>`, published through the `publish_diagnostics` call each plugin already makes from its `line_markers` provider (spring-security and spring-graphql keep their own problem lists). Views copy the `spring-messaging` shape (`Views` trait, `Indexes`, `Expansion`, refresh button). Code actions and icons use existing contribution points. No new API is assumed and no task touches `events`.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK 0.1.37 (`MockHost` tests), `crates/spring-text` (`java::parse`, `Member`, `Annotation`, `Finding`, `Indexes`, `Expansion`).

**Spec:** [`docs/audit-gaps-2026-11-06.md`](../../audit-gaps-2026-11-06.md) (appendix, F lists for the six plugins), [`docs/audit-gaps-2026-11-06/A2-spring-rest.md`](../../audit-gaps-2026-11-06/A2-spring-rest.md) (evidence), [`docs/audit-gaps-2026-11-06/api-facts.md`](../../audit-gaps-2026-11-06/api-facts.md) (what the API gives). Style and shared task shape follow [`2026-11-06-feasible-parity.md`](2026-11-06-feasible-parity.md).

## Global Constraints

- Clean-room: messages, rules and templates are written from public Spring documentation; never copy vendor text; do not open decompiled code of the proprietary originals (nothing under `/tmp/port`).
- Quiet unless sure: library or auto-configured beans and dynamic names (constants, placeholders, concatenation, non-literal arguments) are invisible to a text scan and must not warn. Every check has a test named `<check>_is_quiet_for_library_or_dynamic_name`. An annotation imported from a non-Spring package is quiet (`JavaFile::from_spring`, Task 1).
- Ranges are UTF-16 columns; every scanner gets `ranges_use_utf16_columns_with_multibyte_text` and `crlf_files_report_correct_lines`. A fixed file publishes an empty list (`fixed_file_publishes_empty_list`).
- One settings boolean per check group, default on, in the plugin's `settings.schema.json` (key `<plugin camelCase>.check<Group>`); read with `host::setting_bool(key).unwrap_or(true)`; a test switches each off.
- `events` is an optional capability; if a task ever subscribes it logs `LogLevel::Warn` on denial. Every event-driven plugin needs an `activation` entry. No task here is event-driven; views add `on-view:<id>` to `activation`.
- Providers have a 50 ms budget (`PROVIDER_BUDGET`): scan in commands or view renders (`COMMAND_BUDGET`), cache in `Indexes`. `find_files` skips hidden and ignored paths on the real host, the mock does not; a missing file is `Invalid` on the host, `NotFound` in the mock: treat both as absent.
- Code actions edit existing files only. No popups or input dialogs. A command's returned text is dropped: report through notifications.
- Security-sensitive tasks (a process with repo-controlled text, secrets, paths from user input) need mandatory option-injection and path-escape tests and are flagged "strongest reviewer". None of the tasks below qualifies: no process, secret or user-typed path is involved (view keys are built by the plugin; Task 7 and Task 10 still test that a malformed key is ignored).
- Per plugin, finish with `cargo test` (no new warnings) and `openidea-plugin build`; `export PATH="$HOME/.cargo/bin:$PATH"` first. Never hand-edit `version` in `plugin.toml` (release-please). Conventional commits `feat(<plugin>): ...` ending with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not touch `.env`.
- Implementers do not edit `PARITY.md` or `LIMITATIONS.md`; they write "Doc notes" in their report (rows now covered, remaining limits). Each task may update its plugin `README.md` and the `description`/`change-notes` strings only if the feature is user-visible.

## Review Focus

Failure modes the audit implies and no happy path exercises; each line's test sits in the owning task.

- **Constants and placeholders in annotation values** (`@ConfigurationProperties(PREFIX)`, `version = VERSION`, `@Query(Q)`) must not warn. Tests: the `..._is_quiet_for_library_or_dynamic_name` of Tasks 2, 4, 5, 9.
- **Custom types shadowing Spring names** (own `Page`, own `@Query`, own `ApiVersionParser`) must not warn. Tests: `foreign_import_is_quiet` in Tasks 2, 4, 5, 9.
- **Repository methods implemented by a custom fragment** (`Page<User> search(Criteria c)`) must not warn about a missing `Pageable`. Test: `page_without_pageable_is_quiet_for_non_query_methods` (Task 4).
- **Views over a project that is still being scanned or has no matches** must show "Scanning..." or an empty message, never an error, and a refresh during a scan must not discard it. Tests: `view_is_a_message_while_scanning`, `view_without_endpoints_is_a_message` (Tasks 7, 10).
- **Generated code in files with CRLF or tabs** must keep the file's line ending and indentation, and must not collide with an existing method name. Tests: `generated_method_keeps_crlf_and_tabs`, `generated_method_name_avoids_collisions` (Task 6).

## Rulings

Every F row was re-checked against the code, `api-facts.md` and the API. Rows are groups: only the F part is built.

| Row | Ruling |
|---|---|
| Boot 3 `@ConfigurationProperties` validity | Build the prefix part only (empty or invalid prefix). The constructor-binding part is F\* and is not planned. |
| Boot 4 early application events | Build. The four event types are in the Spring Boot documentation; text of the listener parameter suffices. |
| Boot 5 invalid application setup | Build. Default package gets no quick fix (a fix would be a package move: rename and file move, not possible). Only argument-less `@EnableAutoConfiguration` and `@ComponentScan` are redundant, and only those get the removal fix. |
| Boot 23 icons | Build. The icon follows the active icon theme; the SVG is our own drawing, not the Spring logo. |
| Data 3 Pageable, Data 4 `@Modifying` | Build, limited to repository interface methods that are derived or carry `@Query`/`@NativeQuery`. |
| Data 5 JDBC empty sequence name | Conditional: Task 4 step 1 confirms the annotation (`@Sequence`) and its attribute names in the public Spring Data Relational reference. If they are not documented, drop the check and record the ruling in the ledger. |
| Data 6 `@MappedCollection` key column | Build as a **Hint**: the default key column is derived from the naming strategy, so the omission is a style risk, not an error. |
| Web 2 version SemVer | Build; quiet when the project mentions an `ApiVersionParser`. A leading `v` is accepted because the parser's behaviour on it is not documented. |
| Web 4 Generate Request Mapping / Exception Handler | Build as two code actions (menus are flat, no Generate menu). Java controllers only; Kotlin is not covered. The caret is not moved afterwards (no caret control); whether the host offers actions on a caret in blank class space is unverified. |
| Web 7 Endpoints tool window | Build over the existing mapping scan: literal annotated mappings and functional routes only. No other endpoint providers (Feign and the like stay in `spring-cloud`). |
| Security 1 obsolete pattern format | Build only `**` before the last path segment in `requestMatchers`, `mvcMatchers`, `securityMatcher` (and Kotlin `authorize`); no quick fix (the rewrite changes what matches). |
| GraphQL 1 `@BatchMapping` return type | Build with a deny-list of clearly invalid return types, not an allow-list: custom wrappers stay quiet. |
| WebSocket 2 messaging URL provider | Build as the plugin's own "WebSocket Destinations" view; a plugin cannot add items to another plugin's Endpoints view. |

No row was downgraded to B. Nothing here needs types, rename or references, file creation, dialogs, injection, streaming, decorations, caret control or library-bean knowledge, apart from the narrowed parts above.

## Shared task shape

Unless a task says otherwise:

- [ ] **Step 1:** Write the failing tests named in the task, in the file's `#[cfg(test)]` module (or the plugin `lib.rs` tests), against `MockHost`.
- [ ] **Step 2:** `cd plugins/<name> && cargo test <test_name>`; expect FAIL on the missing behaviour.
- [ ] **Step 3:** Implement in the named files; wire it into the plugin's provider, `plugin.toml` and `settings.schema.json` when listed.
- [ ] **Step 4:** `cargo test` all green with no new warnings; `openidea-plugin build` reports no error.
- [ ] **Step 5:** Commit `feat(<plugin>): <what>` with the trailer; report Doc notes.

Diagnostics are published as `Finding::diagnostic(text)`; codes are listed per task.

---

### Task 1: `JavaFile::from_spring`

**Files:** Modify `crates/spring-text/src/java.rs` (impl `JavaFile`, test in the same file).

**Interfaces:** Produces `pub fn from_spring(&self, simple: &str) -> bool`: false when the file imports `.<simple>` from a package outside `org.springframework.`, true otherwise (no import, wildcard import, or Spring import). Same rule as the private `from_spring` copies in `spring-boot/src/checks.rs` and `spring-web/src/checks.rs`, which stay as they are.

Tests: `from_spring_accepts_spring_wildcard_and_missing_imports`, `from_spring_rejects_foreign_import`.

- [ ] Steps: shared shape, run `cargo test` in `crates/spring-text` instead of a plugin; commit `feat(spring-text): add JavaFile::from_spring`. Run `cargo test` in the five Spring plugins that depend on it before moving on.

### Task 2: `spring-boot` Java checks (3 groups)

**Files:** Modify `plugins/spring-boot/src/checks.rs` (tests inline), `src/lib.rs` (`checked`, `code_actions`), `settings.schema.json`, `plugin.toml` (add `"code-actions"` to the `java` provider's `kinds`).

**Interfaces:**
- Consumes `Finding`, `JavaFile::from_spring` (Task 1), existing `Groups::from_settings`.
- Produces `Groups` gains `pub configuration_properties: bool`, `pub early_events: bool`, `pub application_setup: bool` (settings `springBoot.checkConfigurationProperties`, `springBoot.checkEarlyEvents`, `springBoot.checkApplicationSetup`).
- Produces `pub fn removal_fixes(file: &JavaFile, text: &str) -> Vec<(usize, usize, String)>`: for each redundant annotation finding, the byte range to delete (annotation plus its trailing blank/newline) and the title `Remove redundant @<Name>`. `code_actions` for `.java` documents turns the entry covering the selection into a `CodeAction` (kind `quickfix`, one edit in the document).
- Codes: `config-properties-prefix` (invalid prefix: Warning; no prefix: Hint), `early-event-listener`, `default-package-application`, `redundant-enable-auto-configuration`, `redundant-component-scan`. `check_java` signature is unchanged; the three groups run inside it.

Rules (own wording, from the Spring Boot reference): a prefix is valid when every dot-separated segment matches `[a-z0-9]+(-[a-z0-9]+)*`; `@ConfigurationProperties` on a class or `@Bean` method (value or `prefix` literal); early events are `ApplicationStartingEvent`, `ApplicationEnvironmentPreparedEvent`, `ApplicationContextInitializedEvent`, `ApplicationPreparedEvent`, read from the single listener parameter type or the `classes` argument of `@EventListener`; the message says to register such a listener with `SpringApplication.addListeners` or `spring.factories`. Application setup applies to a type with `@SpringBootApplication`: empty `package` is the default package; argument-less `@EnableAutoConfiguration`/`@ComponentScan` on the same type are redundant.

Tests: `uppercase_prefix_warns`, `underscore_prefix_warns`, `missing_prefix_hints`, `kebab_case_prefix_is_quiet`, `config_properties_prefix_is_quiet_for_library_or_dynamic_name` (constant, `${...}`, foreign import); `early_event_listener_warns_for_parameter_and_classes_argument`, `ready_event_listener_is_quiet`, `early_event_listener_is_quiet_for_library_or_dynamic_name` (foreign `ApplicationStartingEvent` import); `default_package_application_warns`, `packaged_application_is_quiet`, `redundant_annotations_warn_and_offer_removal`, `component_scan_with_arguments_is_quiet`, `application_setup_is_quiet_for_library_or_dynamic_name` (foreign `SpringBootApplication`); `removal_fix_deletes_the_annotation_line`; `groups_can_be_switched_off`; Review Focus: `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.

- [ ] Steps: shared shape; commit `feat(spring-boot): check configuration properties prefixes, early events and application setup`.

### Task 3: `spring-boot` file icons

**Files:** Create `plugins/spring-boot/icons/config.svg`, `config_dark.svg` (own drawing: a gear on a document); modify `plugin.toml`.

**Interfaces:** Produces one `[[contributes.fileIcons]]` with `id = "spring-boot-config"`, `patterns = ["application*.properties", "application*.yml", "application*.yaml", "bootstrap*.properties", "bootstrap*.yml", "bootstrap*.yaml"]`, `icon = "icons/config.svg"`, `iconDark = "icons/config_dark.svg"`.

Verification replaces tests (a manifest has none): `openidea-plugin check` accepts the manifest and both files exist; `cargo test` stays green.

- [ ] **Step 1:** Add the SVGs and the contribution. **Step 2:** `openidea-plugin build` passes. **Step 3:** Commit `feat(spring-boot): icon for application configuration files`. Doc note: the icon applies only under an icon theme that merges plugin icons (unverified).

### Task 4: `spring-data` checks (3 groups)

**Files:** Create `plugins/spring-data/src/checks.rs` (tests inline), `settings.schema.json`; modify `src/lib.rs` (declare module; in `line_markers`, publish `problems(...)` plus these findings), `plugin.toml` (add `[contributes.settings] schema = "settings.schema.json"`, `title = "Spring Data"`).

**Interfaces:**
- Consumes `repositories(file) -> Vec<(usize, String)>` (type index, domain) and `derived_method(m: &Member) -> bool` from `lib.rs` (make them `pub(crate)`), `JavaFile::from_spring`.
- Produces `pub struct Groups { pub pageable: bool, pub modifying: bool, pub jdbc_mapping: bool }` with `Groups::from_settings()` (`springData.checkPageable`, `springData.checkModifying`, `springData.checkJdbcMapping`) and `pub fn check(file: &JavaFile, repository_types: &[usize], groups: Groups) -> Vec<Finding>`. Entity checks (JDBC) run on every type, not only repositories, so `line_markers` must publish findings even when `repositories` is empty (today it publishes an empty list and returns early: restructure so the empty list is published only when both are empty).
- Codes: `page-without-pageable`, `query-needs-modifying`, `jdbc-empty-sequence-name`, `mapped-collection-key-column`.

Rules: `page-without-pageable`: in a repository interface, a derived or `@Query`/`@NativeQuery` method returning `Page` or `Slice` (not `default`/`static`) with no parameter whose type name ends in `Pageable` or `PageRequest`. `query-needs-modifying`: `@Query`/`@NativeQuery` with exactly one string literal (no `+`, not JSON starting `{`/`[`) whose first word, case-insensitive, is `update` or `delete`, and no `@Modifying` (and no `@Procedure`). `jdbc-empty-sequence-name`: see Rulings; flag a blank string literal in the sequence name attribute. `mapped-collection-key-column`: a field typed `List` or `Map` annotated `@MappedCollection` without a `keyColumn` argument; severity Hint; `Set`/other types quiet.

Tests: `page_without_pageable_warns`, `slice_without_pageable_warns`, `page_with_pageable_is_quiet`, `page_without_pageable_is_quiet_for_non_query_methods`, `page_check_is_quiet_for_library_or_dynamic_name` (own `Page` import, default method); `update_query_without_modifying_warns`, `delete_native_query_without_modifying_warns`, `select_query_is_quiet`, `modifying_present_is_quiet`, `query_modifying_is_quiet_for_library_or_dynamic_name` (constant, concatenation, JSON, foreign `Query`); `blank_sequence_name_warns`, `named_sequence_is_quiet`, `sequence_name_is_quiet_for_library_or_dynamic_name`; `list_mapped_collection_without_key_column_hints`, `map_with_key_column_is_quiet`, `set_mapped_collection_is_quiet`, `mapped_collection_is_quiet_for_library_or_dynamic_name`; `groups_can_be_switched_off`; Review Focus: `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`, `kotlin_repository_is_checked`.

- [ ] Steps: shared shape (step 1 begins by confirming the `@Sequence` ruling); commit `feat(spring-data): check Page returns, query modifications and JDBC mappings`.

### Task 5: `spring-web` API version format

**Files:** Modify `plugins/spring-web/src/checks.rs` (tests inline), `src/lib.rs` (pass the new argument), `settings.schema.json`.

**Interfaces:**
- Consumes `Versioning`, `project_versioning`, `Groups`, private `from_spring` of `checks.rs`.
- Produces `Versioning` gains `pub custom_parser: bool` (text mentions `ApiVersionParser`); `pub fn project_standard_parser(complete: bool, files: &[Versioning]) -> Option<bool>` (`Some(true)` only when every file was read and none has `custom_parser`); `check_java(file: &JavaFile, versioning_configured: Option<bool>, standard_parser: Option<bool>, groups: Groups) -> Vec<Finding>`; `Groups` gains `pub version_format: bool` (`springWeb.checkVersionFormat`). Code `invalid-api-version` on the literal of the `version` argument of a mapping annotation. Valid: `v?\d+(\.\d+){0,2}\+?` (the trailing `+` is a baseline version). Update every existing `check_java` call and test helper.

Tests: `malformed_version_warns` (`"1.x"`, `"latest"`, `"1..2"`), `semantic_versions_are_quiet` (`"1"`, `"1.1"`, `"1.2.3"`, `"1.2+"`, `"v2"`), `version_format_is_quiet_for_library_or_dynamic_name` (constant, array, custom parser file, foreign mapping import, `None` parser), `version_format_group_can_be_switched_off`, `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.

- [ ] Steps: shared shape; commit `feat(spring-web): check the format of mapping versions`.

### Task 6: `spring-web` generate handler code actions

**Files:** Create `plugins/spring-web/src/generate.rs` (tests inline); modify `src/lib.rs` (`code_actions` appends these to the quick fixes; no new provider kind).

**Interfaces:**
- Produces `pub fn actions(doc_path: &str, text: &str, selection: Range) -> Vec<CodeAction>`. For a Java file, when the selection start lies inside the body of a type annotated `@Controller`/`@RestController` (request mapping) or those plus `@ControllerAdvice`/`@RestControllerAdvice` (exception handler) but outside every member (`Member.start..end`), it returns `Add @RequestMapping method` and/or `Add @ExceptionHandler method` (kind `refactor`, not preferred). Each carries one edit inserting the method before the closing brace of the type (`TypeDecl.end - 1`) plus an import edit after the last import (after `package` if none) for each annotation not yet imported or covered by a wildcard `org.springframework.web.bind.annotation.*`.
- Generated text, indented like the first member (tab if the file indents with tabs), using the file's line ending (`\r\n` if the file contains it):
  `@RequestMapping("/path")` / `public String handle() { return ""; }` and `@ExceptionHandler(Exception.class)` / `public String handleException(Exception exception) { return ""; }`, bodies on their own lines. The method name gets a numeric suffix (`handle2`) when the type already has a member of that name.

Tests: `controller_body_offers_request_mapping_and_exception_handler`, `controller_advice_offers_only_exception_handler`, `selection_inside_a_method_offers_nothing`, `non_controller_class_offers_nothing`, `generated_method_is_inserted_before_the_closing_brace`, `imports_are_added_only_when_missing`, `generated_method_keeps_crlf_and_tabs`, `generated_method_name_avoids_collisions`, `kotlin_file_offers_nothing`, `generate_is_quiet_for_library_or_dynamic_name` (foreign `Controller` import), `ranges_use_utf16_columns_with_multibyte_text` (selection after a multibyte comment).

- [ ] Steps: shared shape; commit `feat(spring-web): add handler and exception handler code actions`.

### Task 7: `spring-web` Endpoints tool window

**Files:** Modify `plugins/spring-web/src/lib.rs` (impl `Views`, `register_plugin!(WebTools, providers, views)`, add `Expansion` field), `plugin.toml` (activation `on-view:endpoints`, `[[contributes.views]]` `id = "endpoints"`, `title = "Endpoints"`, `icon = "send"`, `location = "left"`).

**Interfaces:**
- Consumes `WebTools.endpoints: Indexes<FileInfo>`, `FileInfo.endpoints: Vec<Endpoint { verb, path, range, handler }>`, `Expansion`, `project_of`.
- Produces `impl Views for WebTools`: `render` groups endpoints by file (item key `f:<path>`, detail = count), one child per endpoint (key `e:<line>:<column>:<path>`, label `<VERB or ANY> <path>`, detail = file name and line), sorted by file then path then verb; a Refresh button (keeps a scan in progress, as `spring-messaging`); status `<n> endpoints`; `handle` expands nodes and, on `Activate` of an `e:` key, calls `host::open_location(path, line, column)`.

Tests: `view_lists_annotated_and_functional_endpoints`, `view_is_a_message_while_scanning`, `view_without_endpoints_is_a_message`, `activating_an_endpoint_opens_its_location`, `malformed_key_is_ignored`, `refresh_keeps_a_scan_in_progress`, `view_ignores_dynamic_and_non_literal_mappings` (quiet test), `paths_with_colons_survive_the_key`.

- [ ] Steps: shared shape; commit `feat(spring-web): add an Endpoints tool window`.

### Task 8: `spring-security` obsolete path patterns

**Files:** Modify `plugins/spring-security/src/lib.rs` (new `path_pattern_problems`, call in `problems` inside the `if security` block next to `matcher_problems`; tests inline), `settings.schema.json`.

**Interfaces:**
- Consumes `kotlin_code_only`, `closing`, `argument_ranges`, `leading_patterns`, `Problem`.
- Produces `fn path_pattern_problems(text: &str, code: &str, kotlin: bool) -> Vec<Problem>` scanning calls to `requestMatchers`, `mvcMatchers`, `securityMatcher` (and `authorize` when `kotlin`); for each leading literal pattern, a segment `**` that is not the last segment yields a Warning, code `obsolete-path-pattern`, on the `**` bytes, no fix. Setting `springSecurity.checkPathPatterns`. `antMatchers` and `regexMatchers` are skipped (the first is flagged as removed already).

Tests: `double_star_before_the_last_segment_warns` (`"/a/**/b"`, `"/**/*.css"`), `trailing_double_star_is_quiet`, `kotlin_authorize_pattern_warns`, `path_patterns_are_quiet_for_library_or_dynamic_name` (explicit `antMatcher("/a/**/b")` argument, constant, concatenation, `HttpMethod` first argument still read), `ant_matchers_are_not_reported_twice`, `path_pattern_group_can_be_switched_off`, `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.

- [ ] Steps: shared shape; commit `feat(spring-security): flag path patterns that PathPatternParser rejects`.

### Task 9: `spring-graphql` `@BatchMapping` return type

**Files:** Create `plugins/spring-graphql/src/checks.rs` (tests inline); modify `src/lib.rs` (add the findings to the `problems` vector published at the end of the Java-side `links` function), `settings.schema.json`.

**Interfaces:**
- Consumes `JavaFile::from_spring`, `Finding`.
- Produces `pub fn batch_mapping_return(file: &JavaFile) -> Vec<Finding>`, code `batch-mapping-return-type`, on the return type text of a `@BatchMapping` method inside a `@Controller`/`@RestController`. Deny-list (own wording from the Spring for GraphQL reference, which allows `Map`, `List`, `Flux` and `Mono<Map>`): `void`/`Unit`, primitives and their boxes, `String`, `Object`, `Set`, `Collection`, `Iterable`, `Optional`, arrays, and `Mono` whose first type argument is not `Map`. Everything else is quiet. Setting `springGraphql.checkBatchMappingReturn`.

Tests: `void_batch_mapping_warns`, `string_batch_mapping_warns`, `mono_of_list_batch_mapping_warns`, `valid_batch_mapping_returns_are_quiet` (`Map<..>`, `List<..>`, `Flux<..>`, `Mono<Map<..>>`), `batch_mapping_return_is_quiet_for_library_or_dynamic_name` (custom wrapper type, foreign `BatchMapping` import), `kotlin_unit_batch_mapping_warns`, `batch_mapping_group_can_be_switched_off`, `fixed_file_publishes_empty_list`, `ranges_use_utf16_columns_with_multibyte_text`, `crlf_files_report_correct_lines`.

- [ ] Steps: shared shape; commit `feat(spring-graphql): check @BatchMapping return types`.

### Task 10: `spring-websocket` destinations view

**Files:** Modify `plugins/spring-websocket/src/lib.rs` (impl `Views`, `register_plugin!(WebSocketTools, providers, views)`, `Expansion` field, refactor `snapshot`/`serves_stomp` to share code with a project-level entry point), `plugin.toml` (activation `on-view:destinations`, `[[contributes.views]]` `id = "destinations"`, `title = "WebSocket Destinations"`, `icon = "send"`, `location = "left"`).

**Interfaces:**
- Consumes `Snapshot { config, uses }`, `Config::keys(&Use) -> Vec<String>`, `Kind`, `Snapshot::related`.
- Produces `fn project_snapshot(&mut self, project: &Project, budget: Duration) -> Snapshot` (script files count only when `serves_stomp` accepts the project root; extract `serves_stomp` to take `&Project` and let the document version call it) and `impl Views for WebSocketTools`: `render` shows a tree, one node per destination key (`d:<key>`, detail = number of senders and receivers), children per use (`u:<line>:<column>:<path>`, label `<file>:<line>`, detail = kind and `via`); a destination with only senders or only receivers gets the `warning` icon; Refresh button, `Expansion`, status `<n> destinations`; `handle` opens a use with `host::open_location`.

Tests: `view_lists_handlers_and_clients_under_one_destination`, `view_is_a_message_while_scanning`, `view_without_destinations_is_a_message`, `destination_with_only_senders_gets_a_warning_icon`, `activating_a_use_opens_its_location`, `malformed_key_is_ignored`, `view_skips_scripts_in_projects_without_stomp` (quiet test), `refresh_keeps_a_scan_in_progress`, `ranges_use_utf16_columns_with_multibyte_text`, plus the existing provider tests unchanged.

- [ ] Steps: shared shape; commit `feat(spring-websocket): add a destinations tool window`.

---

## Self-review

- **Spec coverage:** Boot 3/4/5/23 -> Tasks 2, 3; Data 3/4/5/6 -> Task 4; Web 2/4/7 -> Tasks 5, 6, 7; Security 1 -> Task 8; GraphQL 1 -> Task 9; WebSocket 2 -> Task 10. Every F row has a task or a ruling.
- **Step scan:** each task names files, signatures, codes, settings keys and tests; bodies are left to the implementer.
- **Type consistency:** `Groups` is extended per plugin (Boot, Data, Web); `check_java` changes only in Task 5 (all callers named); `JavaFile::from_spring` (Task 1) is used by Tasks 2, 4, 9.
- **Review Focus:** each line maps to named tests above.
- **Proportion:** about 250 lines for 12 rows across 6 plugins; no code blocks beyond manifest keys.
- **Order:** Task 1 first; the rest are independent (Tasks 6 and 7 both edit `spring-web/src/lib.rs`: run them in order).

## Ledger (fill during execution)

Rulings changed or added while executing:

| Row / task | Ruling and reason |
|---|---|
| | |

Per-task status (`todo`, `done <commit>`, `narrowed: ...`, `dropped: ...`):

| Task | Status | Doc notes |
|---|---|---|
| 1 `JavaFile::from_spring` | todo | |
| 2 spring-boot Java checks | todo | |
| 3 spring-boot file icons | todo | |
| 4 spring-data checks | todo | |
| 5 spring-web version format | todo | |
| 6 spring-web generate actions | todo | |
| 7 spring-web Endpoints view | todo | |
| 8 spring-security path patterns | todo | |
| 9 spring-graphql `@BatchMapping` | todo | |
| 10 spring-websocket destinations view | todo | |
