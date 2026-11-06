# Gap F: Spring core and OSGi Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build every row of the A1 gap audit classed **F** (Spring core -> `spring-beans`/`spring-boot`, Spring OSGi -> `spring-osgi`): XML diagnostics, a few Java annotation checks and three XML intentions.

**Architecture:** `spring-beans` gains XML diagnostics (it publishes none today; `diagnose()` returns early for `xml`) over `spring_text::xml::elements`, published from `file-opened`/`file-saved`. Java checks join `checks.rs` through new modules. Every check is text-based and quiet unless sure. `spring-osgi` gets attribute-value validation in its existing `check()`.

**Tech Stack:** Rust (`wasm32-wasip2`), `openidea-plugin` SDK 0.1.37, `MockHost` tests, `crates/spring-text` (`xml::elements`, `java::parse`, `kotlin::parse`, `Finding`).

**Spec:** [`docs/audit-gaps-2026-11-06/A1-spring-core.md`](../../audit-gaps-2026-11-06/A1-spring-core.md) (rows named below), [`api-facts.md`](../../audit-gaps-2026-11-06/api-facts.md), [`PARITY.md`](../../../PARITY.md), [`LIMITATIONS.md`](../../../LIMITATIONS.md).

## Global Constraints

- Branch `gap-f-spring-core`, worktree `/Users/aritti/Projects/openidea-plugins-gap-f-spring-core`. Run cargo inside `plugins/<name>` with `export PATH="$HOME/.cargo/bin:$PATH"`; no root workspace.
- Clean-room: messages, rules and templates are written from public Spring documentation (reference guide, XSDs, javadoc). The audit saw descriptor names only; never copy vendor text; do not open decompiled code of the proprietary Spring plugin. `spring-osgi` is an Apache-2.0 source port: keep `LICENSE`/`NOTICE`; its upstream source revision is `07887c0596764e16977fd4570c2234bfc81dde49` (intellij-obsolete-plugins).
- Quiet unless sure: library and auto-configured beans, dynamic names (`${...}`, `#{...}`, non-literal annotation arguments, concatenation) and foreign annotations (same import test as `spring()` in `checks.rs`) never warn. Each check has a `<check>_is_quiet_for_library_or_dynamic_name` test.
- Every new scanner has `ranges_use_utf16_columns_with_multibyte_text` and `crlf_files_report_correct_lines` tests; every group has `fixed_file_publishes_empty_list`.
- One settings boolean per check group in `settings.schema.json` (default on, key `springBeans.<name>`), read in `Groups::from_settings`.
- `events` stays an optional capability, with a Warn log on denial (existing `subscribe_to_file_events`). Activation already lists `on-event:file-opened`/`file-saved`; do not touch `version` in `plugin.toml`.
- Providers have a 50 ms budget; scan in events. `find_files` skips hidden and ignored paths on the real host (not in the mock); a missing file is `Invalid` on the host, `NotFound` in the mock (treat both as absent). Code actions edit existing files only; no popups or dialogs.
- Per plugin finish with `cargo test` (no new warnings) and `openidea-plugin build`. Conventional commits `feat(spring-beans): ...`, ending with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. Do not touch `.env`.
- Implementers do not edit `PARITY.md` or `LIMITATIONS.md`; their report has a **Doc notes** section (what changed, what stays unverified in the IDE).
- No task here runs a process, handles secrets or builds a path from user input except Task 11 (reads a build file next to the source file; path comes from `build_module`, not user input), so no task needs the strongest reviewer by the security rule. Task 11 gets a path-escape test anyway.

## Rulings (rows re-checked against code and api-facts)

- **Row 27 (EventListener non-public method): downgraded.** Spring's reference guide states no visibility rule for `@EventListener`; proxied beans only break for `private` methods under interface proxies, which needs the proxy mode (unknown). Not sure, so quiet: no task.
- **Row 5, Scopes: narrowed.** A custom scope can be registered anywhere (`CustomScopeConfigurer`, `registerScope`, a library), so "unknown scope value" is not sure. The check warns only when the value is a case variant or one edit away from a known scope (`singleton`, `prototype`, `request`, `session`, `application`, `websocket`, plus `bundle`, which is also the OSGi row 7 whitelist entry).
- **Row 7, AbstractBeanReferences: narrowed.** `parent="..."` pointing to an abstract bean is the intended use. Only `ref`-style references (`ref`, `<ref bean>`, `<idref bean>`, `depends-on`, `factory-bean`, `p:x-ref`, `c:x-ref`) are checked.
- **Row 6, XML names: narrowed.** Spring XML lets a later file override an earlier definition, so XML-vs-XML clashes across files are quiet; same-file clashes (a parse error in Spring) and XML-vs-Java clashes are reported.
- **Row 30 (SpringCompilerPlugin): Maven fix dropped.** The Gradle fix inserts into an existing `plugins {}` block; for Maven the diagnostic stays but no edit is offered.
- **Row 2 (PointcutExpression): quiet under full AspectJ.** Designators such as `call` are valid with the AspectJ compiler or load-time weaving; the check stays quiet when the module's build file names an AspectJ weaving plugin.

## Review Focus

- A bean `<bean id="a" class="x.Y"/>` in a file that is not a Spring beans file (pom.xml, Web Flow, Maven settings): no diagnostics at all, empty list published.
- `${...}` placeholder in `scope`, `ref`, `class`, `profile`, `expression`: quiet.
- Nested `<beans profile="...">` with the same id in two profiles: not a duplicate.
- Pointcut text such as `execution(* get(..))` or `execution(* *.call(..))`: the method named like a designator must not warn.
- An `@Autowired` constructor next to an `@Autowired(required = false)` one, Lombok (`@RequiredArgsConstructor`) classes, records: no false positives.

## Task 1: XML diagnostics plumbing and AOP structure (row 1)

**Files:**
- Create: `plugins/spring-beans/src/xml_checks.rs`
- Modify: `plugins/spring-beans/src/lib.rs` (`mod xml_checks;`, `diagnose`, `handle_events`), `plugins/spring-beans/src/checks.rs` (`Groups`), `plugins/spring-beans/settings.schema.json`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn check_xml(path: &str, text: &str, world: &World, groups: Groups) -> Vec<Finding>` in `xml_checks.rs`, the one entry point later XML tasks extend; `Groups` gains `pub aop: bool` and `pub xml: bool` (both default true, settings `checkAop`, `checkXml`); a private `fn is_beans_file(els: &[xml::Element]) -> bool` (root local name `beans`) and `fn in_namespace(els: &[xml::Element], i: usize, ns: &str) -> bool`, constants `BEANS_NS`, `AOP_NS`.
- Consumes: `spring_text::xml::{elements, namespace, root}`, `Finding::new(start, end, code, message).with_severity(...)`.

Rules (code `aop-structure`, severity Error, range = element name): inside `<aop:config>` (AOP namespace): `<aop:pointcut>` without `id` or `expression`; `<aop:aspect>` without `ref`; `<aop:advisor>` without `advice-ref`; an advice element (`before`, `after`, `after-returning`, `after-throwing`, `around`) without `method`, or with neither `pointcut` nor `pointcut-ref`, or with both. Elements outside the AOP namespace are never checked.

- [ ] **Step 1: Write failing tests** in `tests.rs`: `aop_pointcut_without_expression_is_reported`, `aop_aspect_without_ref_is_reported`, `aop_advice_without_pointcut_is_reported`, `aop_advice_with_both_pointcut_and_pointcut_ref_is_reported`, `aop_advisor_without_advice_ref_is_reported`, `aop_structure_is_quiet_for_library_or_dynamic_name` (`${...}` values, a lookalike element in another namespace, a non-beans file), `xml_ranges_use_utf16_columns_with_multibyte_text`, `xml_crlf_files_report_correct_lines`, `fixed_xml_file_publishes_empty_list`, `switched_off_aop_group_is_quiet`; plus an events test: a `FileSaved` for a `.xml` path publishes diagnostics, a `pom.xml` publishes none.
- [ ] **Step 2:** `cd plugins/spring-beans && cargo test aop_` expect FAIL.
- [ ] **Step 3: Implement.** `diagnose` runs `xml_checks::check_xml` for `language == "xml"` (index lookup as for Java) and publishes an empty list when the file is not a beans file; `handle_events` also accepts `.xml` (and `.kt`, which Task 11 needs; Kotlin keeps its existing `field-injection` diagnostics path) and passes the language from the extension. Add the two settings and schema entries.
- [ ] **Step 4:** `cargo test` all of spring-beans passes.
- [ ] **Step 5: Commit** `feat(spring-beans): report AOP configuration mistakes in XML`.

## Task 2: Unsupported pointcut designators (row 2)

**Files:**
- Create: `plugins/spring-beans/src/aop.rs`
- Modify: `plugins/spring-beans/src/checks.rs` (call from `check_java`, extract `build_module` use), `plugins/spring-beans/src/xml_checks.rs`, `plugins/spring-beans/src/lib.rs` (`mod aop;`), `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn unsupported_designators(expression: &str) -> Vec<(usize, usize, &'static str)>` (byte range inside `expression`, designator name) in `aop.rs`; `pub const SPRING_UNSUPPORTED: [&str; 14] = ["call", "get", "set", "preinitialization", "staticinitialization", "initialization", "handler", "adviceexecution", "withincode", "cflow", "cflowbelow", "if", "@this", "@withincode"]`; code `unsupported-pointcut-designator` (Error); `fn weaving_plugin_in_build(path: &str) -> bool` in `checks.rs` (build file of `build_module(path)` contains `aspectj-maven-plugin`, `aspectj-gradle-plugin`, `freefair.aspectj`, `aspectjtools`).
- Consumes: `Groups.aop` from Task 1; `Annotation::strings(&["value","pointcut"])` (empty when dynamic); XML `expression` attribute of `aop:pointcut` and `pointcut` attribute of the five advice elements.

Algorithm: scan at nesting level 0 and inside bare grouping parentheses; an identifier (optionally `@`-prefixed) followed by `(` is a designator, its balanced parentheses are skipped without inspection (so `execution(* get(..))` never matches); report names in `SPRING_UNSUPPORTED`. Java sites: `@Pointcut`, `@Before`, `@After`, `@AfterReturning`, `@AfterThrowing`, `@Around` (args `value`, `pointcut`) in classes annotated `@Aspect`; skip a dynamic argument.

- [ ] **Step 1: Failing tests:** in `aop.rs` `#[cfg(test)]`: `finds_call_cflow_and_at_this`, `ignores_method_named_like_a_designator` (`execution(* get(..))`, `execution(* *.call(..))`), `descends_into_grouping_and_negation` (`!(call(* *(..)) || within(a..*))`), `ignores_named_pointcut_references`; in `tests.rs`: `unsupported_designator_in_java_advice_is_reported` (range is the designator in the string, UTF-16 test with a multibyte literal before it), `unsupported_designator_in_xml_expression_is_reported`, `unsupported_designator_is_quiet_for_library_or_dynamic_name` (concatenated or `${}` expression, foreign `@Before`, AspectJ weaving plugin in the module's `pom.xml`).
- [ ] **Step 2:** `cargo test designator` expect FAIL.
- [ ] **Step 3: Implement** `unsupported_designators` and wire Java (`check_java`) and XML (`check_xml`).
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): flag AspectJ pointcut designators Spring AOP does not support`.

## Task 3: XML bean attributes, value consistency and scopes (row 5)

**Files:**
- Modify: `plugins/spring-beans/src/xml_checks.rs`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Consumes: `check_xml`, `in_namespace`, `BEANS_NS` (Task 1), `Groups.xml`.
- Produces: `pub const KNOWN_SCOPES: [&str; 7] = ["singleton", "prototype", "request", "session", "application", "websocket", "bundle"]`; codes `xml-bean-attributes`, `xml-injection-value`, `xml-scope` (all in group `xml`).

Rules (all skip values containing `${` or `#{`, and elements outside the beans namespace):
- `xml-bean-attributes` (Error): `<bean>` with `class` and `factory-bean` together (range of `factory-bean`); `factory-bean` without `factory-method`; a `<bean>` with no `class`, no `parent`, no `factory-bean` and not `abstract="true"`.
- `xml-injection-value` (Error): `<property>` or `<constructor-arg>` with `ref` and `value` attributes together; with a value/ref attribute and also a child element other than `description`/`meta`; with neither (property only; constructor-arg may rely on `p:`/`c:` and index, so quiet). Range: element name.
- `xml-scope` (Warning): `scope` attribute on `<bean>` or `<beans>` default... (bean only) whose value equals a `KNOWN_SCOPES` entry ignoring case but not exactly, or is within one edit of one; any other value (`thread`, `custom`) is quiet.

- [ ] **Step 1: Failing tests:** `bean_with_class_and_factory_bean_is_reported`, `factory_bean_without_factory_method_is_reported`, `bean_without_class_parent_or_factory_bean_is_reported`, `abstract_bean_without_class_is_quiet`, `property_with_ref_and_value_is_reported`, `property_with_attribute_and_child_element_is_reported`, `property_with_nothing_is_reported`, `constructor_arg_without_value_is_quiet`, `misspelled_scope_is_reported`, `bundle_scope_is_accepted`, `custom_scope_is_quiet`, `xml_attribute_checks_are_quiet_for_library_or_dynamic_name` (placeholders, non-beans namespace elements, a `<bean>` inside another namespace's file), `switched_off_xml_group_is_quiet`.
- [ ] **Step 2:** `cargo test xml` expect FAIL.
- [ ] **Step 3: Implement** in `check_xml` with one function per code.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): check XML bean attributes, injected values and scopes`.

## Task 4: InjectionValueStyle hint with fix (row 5, style part)

**Files:**
- Modify: `plugins/spring-beans/src/xml_checks.rs`, `plugins/spring-beans/src/lib.rs` (`code_actions`), `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn attribute_style_edit(text: &str, property: &xml::Element, els: &[xml::Element]) -> Option<TextEdit>` in `xml_checks.rs`; code `xml-value-style` (Hint, group `xml`); code action title `Use the value attribute` / `Use the ref attribute`.
- Consumes: Task 3 helpers.

Rule: a `<property>` or `<constructor-arg>` whose only child is `<value>` (no attributes, text only, no `<`, no newline, no `"`) or `<ref bean="x"/>` (no other attributes), and that has no `value`/`ref` attribute, gets a Hint on the child. Fix: replace from the `>` of the parent start tag through its end with ` value="t"/>` (or ` ref="x"/>`); the parent must not be self-closing. Text with `&` entities is copied verbatim.

- [ ] **Step 1: Failing tests:** `value_element_is_hinted_and_rewritten_to_attribute`, `ref_element_is_hinted_and_rewritten_to_attribute`, `value_style_is_quiet_for_library_or_dynamic_name` (multi-line text, CDATA, `type` attribute, quote inside text, second child, `<ref local=...>`), `rewrite_keeps_multibyte_text`, `code_action_applies_the_edit_to_the_open_file`.
- [ ] **Step 2:** `cargo test value_style` expect FAIL.
- [ ] **Step 3: Implement** the hint in `check_xml` and the quick fix in `code_actions` for `xml` documents (selection inside the hint range).
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): hint and fix <value> and <ref> children that can be attributes`.

## Task 5: XML intentions: p-namespace, introduce bean, update schema (rows 32, 33, 34)

**Files:**
- Create: `plugins/spring-beans/src/xml_actions.rs`
- Modify: `plugins/spring-beans/src/lib.rs` (`code_actions`, `mod xml_actions;`), `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces in `xml_actions.rs`: `pub fn use_p_namespace(text: &str, caret: usize) -> Option<Vec<TextEdit>>`, `pub fn introduce_bean(text: &str, caret: usize) -> Option<Vec<TextEdit>>`, `pub fn update_schema(text: &str) -> Option<Vec<TextEdit>>`; titles `Use p-namespace`, `Introduce bean`, `Update XML schema`.
- Consumes: `xml::elements`, `xml::namespace`, `TextEdit` from the SDK.

Behaviour:
- `use_p_namespace`: caret in a `<property name="x" value="v"/>` (or `ref="b"` becoming `p:x-ref="b"`) with attributes only; replaces the element by an attribute on the parent `<bean>` start tag, deleting the property element, and adds `xmlns:p="http://www.springframework.org/schema/p"` to the root `<beans>` when no prefix is bound to that URI (reuse the bound prefix otherwise). Quiet when the property name contains `-`/`.`, when `p:x` already exists on the bean, or when the bean is not in the beans namespace.
- `introduce_bean`: caret on an inner `<bean>` (child of `<property>`, `<constructor-arg>`, `<entry>` or collection) with `class`; moves it to the top level after the enclosing top-level element with `id="<decapitalized simple class>"` (suffix `2`, `3` until unused among all ids and names in the file) and replaces the inner element by `ref="id"` on `<property>`/`<constructor-arg>` parents, or `<ref bean="id"/>` elsewhere. Quiet when the inner bean has an `id` already (offer nothing) or contains `<`-less self text that cannot be parsed.
- `update_schema`: file starting with a `<!DOCTYPE beans ...>` Spring DTD declaration is rewritten to the `xmlns`/`xsi:schemaLocation` form of the beans schema (`http://www.springframework.org/schema/beans`, `https://www.springframework.org/schema/beans/spring-beans.xsd`), keeping the original elements; offered only for a `<beans>` root without `xmlns`.

- [ ] **Step 1: Failing tests:** `p_namespace_moves_value_and_adds_declaration`, `p_namespace_reuses_bound_prefix`, `p_namespace_for_ref_uses_ref_suffix`, `p_namespace_is_quiet_for_library_or_dynamic_name` (dotted name, existing attribute, non-beans element), `introduce_bean_moves_inner_bean_and_references_it`, `introduce_bean_picks_an_unused_id`, `introduce_bean_is_quiet_when_bean_has_id`, `update_schema_replaces_doctype`, `update_schema_is_quiet_for_schema_files`, `intentions_keep_multibyte_text_and_crlf`; each applied edit set re-parses with `xml::elements` and yields the expected structure.
- [ ] **Step 2:** `cargo test p_namespace introduce_bean update_schema` (one invocation per filter) expect FAIL.
- [ ] **Step 3: Implement** the three functions and offer them from `code_actions` for `xml` documents inside beans files.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): XML intentions to use p-namespace, introduce a bean and update the schema`.

## Task 6: XML bean names and abstract references (rows 6, 7)

**Files:**
- Modify: `plugins/spring-beans/src/xml_checks.rs`, `plugins/spring-beans/src/model.rs` (`Bean.is_abstract: bool`), `plugins/spring-beans/src/checks.rs` (make `profiles_overlap`, `same_scope`, `capitalised`, `names_of` `pub(crate)`), `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `Bean.is_abstract` (true for `<bean abstract="true">`, false elsewhere; every `Bean { .. }` literal sets it); codes `duplicate-bean-name` and `bean-name-convention` now also from XML, `xml-abstract-reference` (Error, group `xml`).
- Consumes: `World::beans()`, `World::named()`, groups `duplicate_names` and `naming`, `is_xml_ref` from `model.rs`.

Rules: for each explicit `id` or `name` token on a `<bean>`: capitalised -> `bean-name-convention` Hint (as for Java); duplicate -> `duplicate-bean-name` when the same name is declared in the same `<beans>` element of this file (range: second occurrence), or by a non-XML bean in the project under the Java rules (`same_scope`, `profiles_overlap`, not conditional); names containing `#` (generated) and XML-vs-XML clashes across files are quiet. `xml-abstract-reference`: a `ref`-style attribute (not `parent`) naming a bean for which every definition found by `world.named(name)` is abstract.

- [ ] **Step 1: Failing tests:** `xml_bean_name_with_capital_is_hinted`, `duplicate_xml_ids_in_one_file_are_reported`, `xml_bean_clashing_with_java_bean_is_reported`, `same_id_in_different_profile_blocks_is_quiet`, `same_id_in_two_xml_files_is_quiet`, `generated_names_are_quiet`, `ref_to_abstract_bean_is_reported`, `parent_to_abstract_bean_is_quiet`, `ref_to_bean_with_a_concrete_definition_is_quiet`, `xml_names_are_quiet_for_library_or_dynamic_name`.
- [ ] **Step 2:** `cargo test xml_bean` expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test` passes (Java duplicate and naming tests unchanged).
- [ ] **Step 5: Commit** `feat(spring-beans): check bean names and abstract references in XML`.

## Task 7: Autowiring misuse in code (rows 13, 14, 15)

**Files:**
- Create: `plugins/spring-beans/src/injection_checks.rs`
- Modify: `plugins/spring-beans/src/checks.rs` (call from `check_java`, `Groups.injection`), `plugins/spring-beans/src/lib.rs`, `plugins/spring-beans/settings.schema.json`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn check(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>)` in `injection_checks.rs`; `Groups.injection` (default on, `checkInjectionAnnotations`); codes `static-injection` (Warning), `static-spel-injection` (Warning), `autowired-constructors` (Error). Reuses `spring()` from `checks.rs` (make it `pub(crate)`).

Rules (from the reference guide: `@Autowired` is not supported on static fields and methods; only one constructor may carry `@Autowired(required = true)`):
- `@Autowired`, `@Value`, `@Inject`, `@Resource` on a `static` field or method (range: member name). For `@Value` whose literal contains `#{` on a static field the code is `static-spel-injection` instead (not both). A `static final` constant with `@Value` is also reported.
- Constructors: more than one `@Autowired` constructor where at least one is required (no `required = false`) is reported on each required one beyond the first and on any other annotated constructor; all of them `required = false` is quiet. A non-literal `required` argument is quiet.

- [ ] **Step 1: Failing tests:** `autowired_static_field_is_reported`, `value_spel_on_static_field_reports_the_spel_code_only`, `inject_on_static_method_is_reported`, `two_required_autowired_constructors_are_reported`, `required_next_to_optional_constructor_is_reported`, `optional_autowired_constructors_are_quiet`, `injection_checks_are_quiet_for_library_or_dynamic_name` (foreign `@Autowired`, `required = FLAG`, lombok-style class, record, non-static members), `switched_off_injection_group_is_quiet`, UTF-16 range test.
- [ ] **Step 2:** `cargo test static_ constructors` expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): report @Autowired on static members and conflicting @Autowired constructors`.

## Task 8: Profile expressions and @Async return types (rows 19, 26)

**Files:**
- Create: `plugins/spring-beans/src/annotation_checks.rs`
- Modify: `plugins/spring-beans/src/checks.rs`, `plugins/spring-beans/settings.schema.json`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn profile_expression_problem(value: &str) -> Option<&'static str>` and `pub fn check_profiles(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>)`, `pub fn check_async(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>)` in `annotation_checks.rs`; `Groups.profiles` (`checkProfiles`) and `Groups.async_methods` (`checkAsync`), both default on; codes `profile-expression` (Error) and `async-return-type` (Warning).

Rules: `@Profile` string literals (class and method level): empty or blank value, a `!` followed by nothing, `&` and `|` mixed at one nesting level without parentheses, unbalanced parentheses. A plain list `{"a","b"}` is valid. `@Async` method return types: warn when the return type is a primitive or one of `String`, `Integer`, `Long`, `Double`, `Boolean`, `Object`, `List`, `Set`, `Map`, `Optional`; quiet for `void`, `Future`, `CompletableFuture`, `ListenableFuture`, `CompletionStage` and any other name (could be a Future subtype). Also quiet for Kotlin and foreign `@Async`.

- [ ] **Step 1: Failing tests:** `profile_expression_problem_cases` (table: `""`, `"a & b | c"`, `"!"`, `"(a & b"` report; `"a | b"`, `"!dev"`, `"(a & b) | c"` quiet), `empty_profile_is_reported`, `mixed_operators_are_reported`, `profile_checks_are_quiet_for_library_or_dynamic_name` (constant argument, foreign `@Profile`), `async_returning_string_is_reported`, `async_returning_future_types_is_quiet`, `async_check_is_quiet_for_library_or_dynamic_name` (project type, foreign annotation), `switched_off_profile_and_async_groups_are_quiet`.
- [ ] **Step 2:** `cargo test profile_ async_` expect FAIL.
- [ ] **Step 3: Implement** and call both from `check_java`.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): check @Profile expressions and @Async return types`.

## Task 9: Cache annotation checks (row 23)

**Files:**
- Modify: `plugins/spring-beans/src/annotation_checks.rs`, `plugins/spring-beans/src/checks.rs`, `plugins/spring-beans/settings.schema.json`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn check_cache(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>)`; `Groups.cache` (`checkCache`, default on); codes `cache-names-missing` (Warning), `cache-annotation-on-interface` (Warning), `cacheable-and-cache-put` (Warning).
- Consumes: Task 8's module and `spring()`.

Rules (reference guide, cache abstraction): `@Cacheable`/`@CachePut`/`@CacheEvict` with none of `value`/`cacheNames` and no `@CacheConfig(cacheNames = ...)` on the class and no `@Caching` wrapper: reported on the annotation (quiet when any such argument is non-literal, and when the class has a `@CacheConfig` without literal `cacheNames`); any cache annotation on a method or type of an `interface` (the guide recommends annotating concrete classes; interface annotations only work with interface proxies); one method carrying both `@Cacheable` and `@CachePut` (range: the second annotation).

- [ ] **Step 1: Failing tests:** `cacheable_without_cache_name_is_reported`, `cache_config_on_class_supplies_the_name`, `cache_annotation_on_interface_method_is_reported`, `cacheable_and_cache_put_together_are_reported`, `cache_checks_are_quiet_for_library_or_dynamic_name` (constant name, foreign annotation, `@Caching`), `switched_off_cache_group_is_quiet`, UTF-16 range test.
- [ ] **Step 2:** `cargo test cache` expect FAIL.
- [ ] **Step 3: Implement.** The interface rule walks `file.types` of kind `interface` (the loop in `check_java` only visits classes).
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): check cache annotation names, placement and combinations`.

## Task 10: Spring test annotation checks (row 25)

**Files:**
- Modify: `plugins/spring-boot/src/checks.rs`, `plugins/spring-boot/settings.schema.json`, `plugins/spring-boot/src/lib.rs` (`Groups`), tests in `checks.rs`

**Interfaces:**
- Produces: `Groups.test_lifecycle` (`springBoot.checkTestLifecycle`, default on); codes `dirties-context-mode` (Warning) and `transaction-callback-signature` (Warning); `fn test_lifecycle(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>)`.
- Consumes: `from_spring(file, name)` and `Finding`.

Rules (Spring TestContext docs): `@DirtiesContext` on a class with `methodMode` (only valid on methods), on a method with `classMode` or `hierarchyMode` (class-level only); `@BeforeTransaction`/`@AfterTransaction` on a method that is not `void`, takes parameters, or is `static` (the docs require `void`; interface default methods are quiet).

- [ ] **Step 1: Failing tests:** `dirties_context_method_mode_on_class_is_reported`, `dirties_context_class_mode_on_method_is_reported`, `before_transaction_with_parameters_is_reported`, `after_transaction_returning_a_value_is_reported`, `test_lifecycle_is_quiet_for_library_or_dynamic_name` (valid usage, foreign annotations, interface default method), `switched_off_group_is_quiet`.
- [ ] **Step 2:** `cd plugins/spring-boot && cargo test dirties transaction_callback` expect FAIL.
- [ ] **Step 3: Implement** and add to `check_java`.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-boot): check @DirtiesContext modes and transaction callback signatures`.

## Task 11: Kotlin compiler plugin (row 30)

**Files:**
- Create: `plugins/spring-beans/src/kotlin_build.rs`
- Modify: `plugins/spring-beans/src/lib.rs` (`diagnose` for `kotlin`, `code_actions`), `plugins/spring-beans/settings.schema.json`, `plugins/spring-beans/src/tests.rs`

**Interfaces:**
- Produces: `pub fn missing_spring_plugin(path: &str, file: &JavaFile) -> Vec<Finding>` (code `kotlin-spring-plugin`, Warning, range: class name); `pub fn gradle_fix(build_path: &str, build_text: &str) -> Option<TextEdit>`; `Groups.kotlin_plugin` (`checkKotlinSpringPlugin`, default on).
- Consumes: `build_module(path)` (make `pub(crate)`), `spring_text::kotlin::parse`.

Rules (Kotlin docs: the `kotlin-spring` compiler plugin opens classes carrying Spring annotations): a Kotlin `class` without `open`, `abstract`, `sealed`, `data`, `enum`, `inner`, `annotation` modifier, annotated at class level with `@Configuration` (unless `proxyBeanMethods = false`), or at class or member level with `@Transactional`, `@Async`, `@Cacheable`, `@CacheEvict`, `@CachePut`, `@Retryable`, is reported when the nearest build file exists and its text names none of `kotlin-spring`, `plugin.spring`, `allopen`, `"spring"` inside a `compilerPlugins` block. No build file found: quiet. Fix (Gradle only): in `build.gradle.kts`, insert `kotlin("plugin.spring") version "<V>"` as a new line at the end of the existing `plugins { ... }` block, where `<V>` is the version of the `kotlin("jvm")` line; in `build.gradle`, `id 'org.jetbrains.kotlin.plugin.spring' version '<V>'` from the `org.jetbrains.kotlin.jvm` line. No block, no `jvm` plugin line, or Maven: no edit offered. The edit targets the existing build file found via `build_module`.

- [ ] **Step 1: Failing tests:** `final_transactional_kotlin_class_without_plugin_is_reported`, `configuration_class_without_plugin_is_reported`, `plugin_in_build_file_silences_the_check`, `open_or_abstract_class_is_quiet`, `kotlin_spring_plugin_is_quiet_for_library_or_dynamic_name` (no build file, foreign annotation, `proxyBeanMethods = false`, `allopen` in build), `gradle_kts_fix_inserts_plugin_line`, `groovy_gradle_fix_inserts_plugin_line`, `maven_build_offers_no_fix`, `fix_never_edits_outside_the_project` (a path whose parent walk would leave the project root yields no fix), `switched_off_group_is_quiet`.
- [ ] **Step 2:** `cargo test kotlin_spring gradle_ fix_never` expect FAIL.
- [ ] **Step 3: Implement.** `diagnose` handles `language == "kotlin"` (existing `field-injection` plus this check); the code action for the finding returns an edit on the build file.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5: Commit** `feat(spring-beans): suggest the kotlin-spring plugin for classes Spring must proxy`.

## Task 12: Spring OSGi attribute values (OSGi rows 1, 7)

**Files:**
- Modify: `plugins/spring-osgi/src/lib.rs` (`check`, tests in the same file)

**Interfaces:**
- Produces: `fn check_values(doc: &OsgiXml, i: usize, out: &mut Vec<Diagnostic>)`; code `osgi-attribute-value` (Error) via the existing `problem(text, at, message, code)`.
- Consumes: `OsgiXml::osgi(&Element)`, `name_range`.

Rules (Spring Dynamic Modules reference, `spring-osgi` XSD; values containing `${` are quiet; message names the allowed values):
- `cardinality`: on `reference` `0..1`/`1..1`; on `list`/`set` `0..N`/`1..N`.
- `auto-export` on `service`: `disabled`, `interfaces`, `class-hierarchy`, `all-classes`.
- `context-class-loader`: on `service` `unmanaged`/`service-provider`; on `reference`/`list`/`set` `client`/`service-provider`/`unmanaged`.
- `ranking` on `service`, `timeout` on `reference`/`list`/`set`: integer (`timeout` non-negative).
- `member-type` on `list`/`set`: `service-object`/`service-reference`; `basis` on `natural` inside a comparator: `service`/`service-reference`.
- `greedy-proxying`: `true`/`false`.
Row 7 (`bundle` scope) is covered by Task 3's `KNOWN_SCOPES`; OSGi has no scope check of its own.

- [ ] **Step 1: Failing tests:** `wrong_cardinality_is_reported`, `cardinality_for_list_and_reference_differ`, `wrong_auto_export_is_reported`, `wrong_context_class_loader_is_reported`, `non_integer_ranking_is_reported`, `wrong_member_type_is_reported`, `wrong_ordering_basis_is_reported`, `attribute_values_are_quiet_for_library_or_dynamic_name` (valid values, `${placeholder}`, elements of another namespace, default-namespace file `DEFAULT_NS`), UTF-16 range test, `ignores_other_xml` still passes.
- [ ] **Step 2:** `cd plugins/spring-osgi && cargo test attribute` expect FAIL.
- [ ] **Step 3: Implement** and call from `check`.
- [ ] **Step 4:** `cargo test` passes; `openidea-plugin build`.
- [ ] **Step 5: Commit** `feat(spring-osgi): validate enumerated attribute values of osgi elements`.

## Ledger

Fill during execution: per task status, commit, reviewer rounds, rulings made, Doc notes for `PARITY.md`/`LIMITATIONS.md` (spring-beans XML diagnostics, scope narrowing, AspectJ weaving quietness, Kotlin Gradle fix unverified, code actions on XML unverified in the IDE), and the audit rows ticked in `docs/audit-gaps-2026-11-06.md`.

| Task | Rows | Status | Commit | Notes |
|---|---|---|---|---|
| 1 | A1 1 | | | |
| 2 | A1 2 | | | |
| 3 | A1 5 | | | |
| 4 | A1 5 (style) | | | |
| 5 | A1 32-34 | | | |
| 6 | A1 6, 7 | | | |
| 7 | A1 13-15 | | | |
| 8 | A1 19, 26 | | | |
| 9 | A1 23 | | | |
| 10 | A1 25 | | | |
| 11 | A1 30 | | | |
| 12 | OSGi 1, 7 | | | |
| - | A1 27 | ruled out | | no public visibility rule |
