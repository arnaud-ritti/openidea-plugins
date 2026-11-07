# Backlog: Spring rows still F or F* (2026-11-06)

## Status (2026-11-06, after the backlog implementation pass)

Derived from `change-notes`, README and code on branch `arnaud-ritti/implement-new-feature`. DONE = built per the change-notes/README; every item is tested against the mock host only and unverified in the IDE. 'not verified' = I could not confirm it from the change-notes, README or code.

- DONE: SP-FND (`crates/spring-text` changed), BOOT-VAL, BEANS-ANN-a, BEANS-ANN-b, BEANS-XML-REF, BEANS-XML-CLASS, BEANS-REFS, BEANS-KOTLIN, BEANS-CMD, OSGI, BOOT-META, BOOT-CFG-NAV, DATA-TYPES, DATA-MONGO, WEB-VIEW, WEB-URL, SEC-REFS, CLOUD-STREAM, GQL, MSG, BATCH, WS-XML.
- PARTIAL: TPL (file templates for spring-beans, spring-boot, spring-data, spring-web done; spring-beans snippet items done in audit v2 round 2 as live-template completions, plus three beans-XML file templates; spring-web snippets `getm`... done). BOOT-BUILD (Reactor debug agent hint and opt-in JDBC driver check done; the `@EventListener` visibility rule stays dropped, no documented source). CLOUD-MISC (Feign Clients view and `@BootstrapConfiguration` hint done; key hints skipped, `spring-boot` covers them).
- DONE per change-notes, detail not verified: DATA-NAV (derived query tooltips, Show Repository Inherited Members), WEB-KX (Copy as curl, templates).
- SKIPPED: Qute (P5 #12, tooling backlog) is not part of this file.

Basis: `A1-spring-core.md`, `A2-spring-rest.md`, `reclass-A1-spring-core.md` (API 1.2), `implementation-plan.md`, checked against `plugins/spring-*/src` and the `plugin.toml` files on this branch. Row ids: `A1#n`, `OSGi#n`, `Boot#n`, `Data#n`, `MVC#n`, `Sec#n`, `Cloud#n`, `GQL#n`, `Msg#n`, `Batch#n`, `WS#n`. Nothing is verified in the IDE.

Rules for every package: F* checks are quiet unless sure (project sources only; a name or class the plugin cannot see is never reported; opt-in setting when the false-positive risk stays high). Tests: MockHost unit test per problem code (positive, quiet case, library-bean case), direct `index()` tests for indexers, then `cargo test` and `openidea-plugin build`; update `LIMITATIONS.md`, the README, `change-notes`, and flip the row in `reclass-A1-spring-core.md`. `change-notes` in `plugin.toml` is shared by all packages of one plugin: merge those lines last.

## Done since audit (evidence in code)

- A1#1, #2, #5, #6, #7: `aop-structure`, `unsupported-pointcut-designator`, `xml-bean-attributes`, `xml-injection-value`, `xml-scope`, `xml-value-style`, `xml-abstract-reference`, XML `duplicate-bean-name` (`spring-beans/src/xml_checks.rs`, `aop.rs`).
- A1#13-15: `autowired-constructors`, `static-injection`, `static-spel-injection` (`injection_checks.rs`). #19 `profile-expression`. #23 `cache-names-missing`, `cache-annotation-on-interface`, `cacheable-and-cache-put`. #26 `async-return-type`. #25 (`spring-boot`) `dirties-context-mode`, `transaction-callback-signature`.
- A1#30 `kotlin-spring` (`kotlin_build.rs`); #32-34 XML intentions (`xml_actions.rs`); #38 four `generate-*` commands and #42 `change-active-profiles` (`commands.rs`, `plugin.toml`); #48, #49a markers from `indexer.rs` (change-notes). OSGi#1 `osgi-attribute-value`, OSGi#7 `bundle` scope, OSGi#10 `generate-osgi-bean`.
- Boot#3 `config-properties-prefix`, Boot#4 `early-event-listener`, Boot#5 `default-package-application`/`redundant-*`, Boot#11 `add-starters` and Initializr#2 pick lists (`spring-initializr/src/starters.rs`), Boot#23 file icon.
- Data#3 `page-without-pageable`, Data#4 `query-needs-modifying`, Data#5 `jdbc-empty-sequence-name`, Data#6 `mapped-collection-key-column`. MVC#2 `invalid-api-version`, MVC#4 `generate.rs`, MVC#7 Endpoints view, MVC#8 `copy-endpoint-curl`. Sec#1 `obsolete-path-pattern`. GQL#1 `batch-mapping-return-type`. WS#2 destinations view (`unrouted-destination`).

## Not planned (stay F/F* on paper)

A1#3 (obsolete), A1#12 and #17 (auto-configured beans invisible: only as an opt-in setting if asked), A1#27 (Spring documents no visibility rule; needs a source first, see BOOT-BUILD), Boot#13 and Runtime#8 (purpose unknown), A1#16 overload gap (folded into BEANS-ANN-a only for same-class overloads).

## Work packages

Size: S under half a day, M about a day. No L: the large rows are split.

| Pkg | Rows | What to build | Key risk | Files | Size | Deps |
|---|---|---|---|---|---|---|
| SP-FND | foundation for XML/ref packages | `crates/spring-text`: (a) table-driven XML check helper (element -> required attrs, enum values, attr refs) on `xml.rs`; (b) string-constant resolver for `static final String` in project files; (c) shared "call to sibling annotated method" scanner (moved from `spring-beans` self-invocation) | Refactor must not change `self-invocation` output: keep its tests as the contract | `crates/spring-text/src/{xml,java,lib}.rs`, `beans/src/checks.rs` | M | none |
| TPL | A1#39-41, Data#12-13, MVC#5-6 | `fileTemplates` for Component, Configuration, Service, Application, Boot Test, ConfigurationProperties (Java and Kotlin), Repository, Controller, RestController, ControllerAdvice; snippets as completion items (`getm`, `repository`, `service`...) | `$PACKAGE$`, `$NAME$`, `$END$` unverified; no tab stops, so snippets are plain text. Ship one template first as a probe, note the result in `LIMITATIONS.md`. Template text written from Spring docs, not the original | `spring-{beans,data,web}/plugin.toml`, new `templates/`, `sites.rs` completion | M | none |
| BOOT-VAL | Boot#1 (and the value half of Boot#15) | In `config_problems`: flag a value that cannot convert to the key's metadata type, only for `int/long/short/boolean/port/Duration/DataSize/Charset/Locale` and enums listed in the bundled data; quiet for `${}`, `#{}`, lists, maps, unknown and custom types | Converters, relaxed binding (`10s`, `1MB`, `TRUE`), keys whose metadata type is wrong for the starter. Severity Warning, code `invalid-property-value` | `spring-boot/src/{lib,meta}.rs`, `data/boot-properties.tsv` if enum values are missing | M | none |
| BEANS-ANN-a | A1#18 (false side), #24 (syntactic part), #16 (same-class overloads) | `proxyBeanMethods=false` with inter-bean `@Bean` calls; `@Cacheable` key/condition syntax and `@CacheConfig` consistency; self-invocation overload handling | Calls that look inter-bean but target a non-`@Bean` method; text cannot see the callee's overload resolution, so report only when every overload carries the annotation | `spring-beans/src/{annotation_checks,checks}.rs` | M | SP-FND (c) |
| BEANS-ANN-b | A1#21, #22 | Lookup/`@Scheduled(scheduler=)`/`@MockitoBean`/`@TestBean`/`initMethod`/`destroyMethod`/`@Required` targets; resource paths in `@ContextConfiguration`, `@ImportResource`, `@PropertySource`, `@Sql` | Library bean/method targets (report project-defined targets only); `find_files` skips ignored/generated paths and `classpath:` roots are guessed: use `read_file` on candidates, quiet on `classpath*:`, placeholders, wildcards | `spring-beans/src/annotation_checks.rs` (after ANN-a), `model.rs` | M | ANN-a, SP-FND (b) |
| BEANS-XML-REF | A1#8, #20 (XML `<component-scan>`, `depends-on`) | Unresolved `ref`/`parent`/`depends-on` warning (project beans); `${}` against `<context:property-placeholder>` files; `<import resource>` existence; component-scan `<filter>` consistency; inactive-profile dimming as Hint | Library and auto-config beans: warn only when the file imports no other XML and the bean is not found in a profile-tagged block; imports over `classpath:` are quiet | `spring-beans/src/xml_checks.rs`, `model.rs` | M | SP-FND (a) |
| BEANS-XML-CLASS | A1#9, #47 | New checks for XML `class=`: factory method exists/static/public, abstract instantiation, `@Required` setters, constructor-arg count; `name=` -> setter completion and definition | Parent classes in libraries: skip the bean when any supertype is not in project sources | new `spring-beans/src/xml_class_checks.rs`, `lib.rs` wiring | M | SP-FND (a); do not edit `xml_checks.rs` (XML-REF owns it) |
| BEANS-REFS | A1#28, #46 | Reference keys for `profile#`, `resource#`, `@ComponentScan` packages, `@Value` placeholders in `indexer.rs`; `spring.handlers`/`spring.schemas` class and file check (project classes only) | Index key churn breaks existing Go to Declaration; add keys, never rename. Indexer limited to what the host answers (no find usages) | `spring-beans/src/indexer.rs`, `sites.rs` | M | none |
| BEANS-KOTLIN | A1#31 (+ Kotlin side of #20) | Redundant `open` when `kotlin-spring` is on (via `kotlin_build.rs`), path variables in router DSL, `::class.java` extension hint | `open` rule needs plugin on and a Spring-annotated class; quiet otherwise | `spring-beans/src/{kotlin,kotlin_build}.rs` | S | none |
| BEANS-CMD | A1#43, #35 | Command "Check Spring Configuration" (run all diagnostics, show an Html/Message view) and "Dump configuration"; hard-coded string -> placeholder as a command (`pick` file, `apply_edits`) | `pick` is invalid in code actions, so command only; properties file chosen by `pick`, not guessed | `spring-beans/src/commands.rs`, `plugin.toml` | S | BEANS-XML-REF, XML-CLASS (content) |
| OSGI | OSGi#2, #6, #9 | `ref` unresolved as Info (not Error); `ref` completion ordered by exported interfaces (project hierarchy, fallback all); Compendium `osgix:` tag completion | Library beans (hence Info); low value for #9, do last | `spring-osgi/src/lib.rs` | S | none |
| BOOT-META | Boot#7, #9 | Diagnostics on `additional-spring-configuration-metadata.json` (unknown fields, bad `type`, missing description as Hint); intention "define key" appending to the existing file | Class resolution is project-only: do not flag unknown library classes. Needs `json` language id, unverified; file creation impossible (U2), so existing file only | new `spring-boot/src/local_meta.rs`, `lib.rs`, `plugin.toml` (json provider) | M | none |
| BOOT-CFG-NAV | Boot#15, #16, #17 | Definition from a class-valued key or `spring.config.import` to class/file; completion for logger package names, profiles (from `application-*.yml`), `${}` placeholders | Imports can be placeholders (skip); keys with class values are a fixed list from the metadata `sourceType`/`handle` | new `spring-boot/src/nav.rs`, `lib.rs` | M | BOOT-VAL (shares `lib.rs`, land after) |
| BOOT-BUILD | Boot#6, #8, A1#27 | `Hooks.onOperatorDebug()` hint when the agent dependency is present; datasource URL scheme vs driver dependency (Info); `@EventListener` visibility only if a documented source is found | Transitive dependencies make #8 noisy: Info, default off. #6 depends on build-file text. #27 stays dropped without a source | `spring-boot/src/checks.rs`, `settings.schema.json` | S | BOOT-VAL (same files) |
| DATA-TYPES | Data#1, #2 | Parameter/return type vs property type for derived queries, only when both are primitives/wrappers/`String`/`Instant`-like known types and clearly incompatible; `Page`/`Slice`/`Stream` return kinds per query verb | Conversions, projections, DTOs, `Optional`; quiet for generics and unknown simple names | `spring-data/src/checks.rs` | M | none |
| DATA-MONGO | Data#9, #7 | JSON `@Query` field check for Mongo (dotted/nested paths accepted when the root field exists); JDBC entity referenced from two aggregate roots | Nested documents, `$` operators; aggregate-root detection by simple name | `spring-data/src/{checks,lib}.rs` (after DATA-TYPES) | M | DATA-TYPES |
| DATA-NAV | Data#10, #14 | "Show inherited members" tree command for a repository (static list of `CrudRepository` and friends); derived query shown as marker tooltip | Custom parents invisible: list only known ones; tooltip text unverified | `spring-data/src/lib.rs`, `plugin.toml` | S | none |
| WEB-VIEW | MVC#1 | Unresolved view name as opt-in Hint (setting, default off) when no known template root exists for the engine | Other resolvers/forwards/`redirect:`; off by default | `spring-web/src/checks.rs`, `settings.schema.json` | S | none |
| WEB-URL | MVC#9, #11, #13 | Handler gutter marker with the URL; mapping-URL completion in test string literals (`mockMvc.perform(get("`); static resource definition from literals | Gutter icon allowlist and one target per marker (use `endpoints.rs` scan, not a new one); completion inside strings unverified | `spring-web/src/{endpoints,lib}.rs` | M | none |
| WEB-KX | MVC#10, #14, #3 | Kotlin controller path variable checks and endpoints; `<mvc:...>` attribute checks; `apiVersion()` without inserter in tests | No XSD available (hard-coded tables); #3 setup spread over config: hint only when no `ApiVersionInserter` text anywhere | `spring-web/src/{checks,endpoints}.rs`, new `xml_checks.rs` | M | SP-FND (a), WEB-URL (shares `endpoints.rs`) |
| SEC-REFS | Sec#2, #3, #4, #7 | Bean names in `@WithUserDetails`-style annotations vs project beans; more `<http>` XML checks (tables); self-invocation of `@PreAuthorize`/`@PostFilter`; matcher -> endpoint navigation | Library beans (project-only, Info); aspect mode unknown (default off for #4); #7 duplicates the web mapping scan: keep a small copy in `spring-text` | `spring-security/src/lib.rs`, `spring-text` | M | SP-FND (a, c) |
| CLOUD-STREAM | Cloud#1, #2, #3 | Legacy `@StreamListener`/`@SendTo` channel bound to a `@Input`/`@Output` interface; handler signature rules; `@BootstrapConfiguration` found by scan | Legacy API, shrinking audience; channel interfaces from libraries invisible | `spring-cloud/src/{checks,lib}.rs` | M | none |
| CLOUD-MISC | Cloud#4, #6 | Feign client list as a tree view; Cloud key value hints from bundled data | Hints already come from `spring-boot` metadata, check overlap before building | `spring-cloud/src/lib.rs`, `plugin.toml` | S | none |
| GQL | GQL#2, #3 | `unmapped-field` for non-root fields only when the type has no `@SchemaMapping`/DataFetcher anywhere in the project (Info); endpoint path view from `spring.graphql.path` | Library DataFetchers/Querydsl auto-registration; no HTTP client | `spring-graphql/src/{checks,lib}.rs` | S | none |
| MSG | Msg#1, #2, #3 | Hard-coded tables for Rabbit/JMS XML elements; `ref`/`method` -> listener bean (own scan); `static final` destination constants | XSD absent; SpEL destinations stay quiet | `spring-messaging/src/lib.rs` | M | SP-FND (a, b) |
| BATCH | Batch#1, #2 | XML required attrs/enums tables; late-binding `jobParameters` key completion | Tables incomplete: report only elements fully listed | `spring-batch/src/lib.rs` | M | SP-FND (a) |
| WS-XML | WS#1 | Register provider for `xml`; checks on `<websocket:...>` bean refs and required attributes | Hard-coded tables; unverified `xml` provider activation | `spring-websocket/src/lib.rs`, `plugin.toml` | S | SP-FND (a) |

## Order, three waves

**Wave 1: foundations and independent wins (4 M + 4 S).** SP-FND, TPL, BOOT-VAL, BEANS-ANN-a, OSGI, GQL, WEB-VIEW, CLOUD-MISC. SP-FND unblocks eight later packages; TPL answers the placeholder question for 30+ rows; BOOT-VAL is the most common user-visible error class.

**Wave 2: reuse the foundation (10 M + 2 S).** BEANS-XML-REF, BEANS-XML-CLASS, BEANS-ANN-b, BOOT-CFG-NAV, DATA-TYPES, DATA-NAV, WEB-URL, SEC-REFS, CLOUD-STREAM, MSG, BATCH, WS-XML.

**Wave 3: low value or high risk (4 M + 3 S).** BEANS-REFS, BOOT-META, DATA-MONGO, WEB-KX, BEANS-KOTLIN, BEANS-CMD, BOOT-BUILD. BEANS-CMD must come after XML-REF and XML-CLASS.

Parallel without sharing a source file (only append-only `plugin.toml` changes, merge change-notes last):
- Wave 1: all eight (TPL touches only manifests and `sites.rs`; ANN-a touches `annotation_checks.rs`/`checks.rs`; SP-FND moves the scanner out of `checks.rs`, so land SP-FND before ANN-a, or let ANN-a start on the non-scanner rows).
- Wave 2: one stream per plugin: `spring-beans` (XML-REF, XML-CLASS in a new file, then ANN-b), `spring-boot` (CFG-NAV), `spring-data` (TYPES, NAV share `lib.rs`: serial), `spring-web` (URL), and the rest each in their own plugin dir.
- Wave 3: `spring-beans` KOTLIN/REFS/CMD touch different files (`kotlin.rs`, `indexer.rs`, `commands.rs`); BOOT-META and BOOT-BUILD touch different files; DATA-MONGO and WEB-KX stay behind their wave 2 predecessors in the same plugin.

## Summary (20 lines)

- Packages: 27 (18 M, 9 S, no L), covering 63 rows still F or F*; 5 rows consciously dropped.
- Done since audit: 40 rows with evidence above (A1 20, OSGi 3, Boot 6, Data 4, MVC 4, Sec 1, GQL 1, WS 1).
- Wave 1: 8 packages, 4 M + 4 S, about 5.6 days.
- Wave 2: 12 packages, 10 M + 2 S, about 10.8 days; mostly one stream per plugin.
- Wave 3: 7 packages, 4 M + 3 S, about 5.2 days; do only after the IDE has confirmed earlier waves.
- Total: 18 M + 9 S, about 21.6 days (M = 1 day, S = 0.4).
- Top 5 by value per effort:
  1. BOOT-VAL: common config mistakes, one file, no new infrastructure.
  2. TPL: visible in the New menu, 30+ rows, and it settles the placeholder question for later work.
  3. SP-FND: small, but unblocks eight packages and the shared self-invocation scanner.
  4. BEANS-ANN-a: proxy and cache mistakes bite in practice and the signals are plain annotations.
  5. WEB-URL: handler gutter and URL completion are the visible half of the MVC gap.
- Shared foundations: `crates/spring-text` XML table helper, constant resolver, sibling-call scanner; indexer keys only in BEANS-REFS (additive).
- Biggest false-positive risks: BEANS-XML-REF, BEANS-ANN-b, SEC-REFS, BOOT-BUILD (library or transitive beans); design all four Info or opt-in.
- Biggest unverified IDE behaviour: template placeholders (TPL), `json`/`xml` provider activation (BOOT-META, WS-XML), string-literal completion (WEB-URL).
