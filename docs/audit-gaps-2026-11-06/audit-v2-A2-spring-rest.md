# Audit v2: A2 Spring plugins (rest), re-checked at HEAD (ac0edf6) against plugin API 1.2

Method: every row of `A2-spring-rest.md` re-read against `plugins/<name>/src` and `plugin.toml` (problem codes, commands, views, providers found by grep; README and change-notes used only as claims to test). Paths below are relative to `plugins/<name>/src/`. Nothing is verified in the IDE (mock host only). Classes F / F* / B as in `api-facts.md`, re-judged with 1.2 (`pick`/`input`/`confirm`, `apply-edits` from a command, `viewItem.context`, `reveal_view`, document indexers, plugin icon); Track U1 (rename/find usages/hover), U2 (file creation), U4 (types), injection, inlays, folding, run configs, database and debugger APIs are unchanged, so no B row flipped. `spring-osgi` has no row in A2 (it is in A1).

Legend: Done / Partial / Missing. Class only for rows that are not Done; a Partial row is classed for its missing part.

## Spring Boot -> spring-boot (23 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Property value validity | Partial | `invalid-property-value` lib.rs:166, `values.rs` (Integer, Long, Short, Double, Boolean, Duration, DataSize) | F* | Enums, custom types and lists are never checked (README says so); "bad enum" of the original is not covered. Setting `springBoot.checkPropertyValues` |
| 2 | Library/starter keys | Partial | bundled `data/boot-properties.tsv` (Boot 3.5.16, Cloud 2025.0.3) + project metadata in `meta.rs` | B | No jar/classpath read |
| 3 | `@ConfigurationProperties` validity | Partial | `config-properties-prefix` checks.rs:205/220 (missing, empty, not kebab-case) | F* | Constructor-binding checks absent |
| 4 | Early `@EventListener` events | Done | `early-event-listener` checks.rs:247 | | Not in README/change-notes (code is ahead). Backlog says the visibility rule stays dropped; this is a different rule |
| 5 | Application setup | Done | `default-package-application` checks.rs:262, `redundant-enable-auto-configuration`/`redundant-component-scan` checks.rs:268-269, removal fix `removal_actions` lib.rs:95 | | Also not documented. Old audit said Missing |
| 6 | `Hooks.onOperatorDebug()` | Done | `reactor-debug-agent` build.rs:68, wired lib.rs:333 | | Hint only when `reactor-tools` is a dependency (README); not a check of the property default |
| 7 | additional-spring-configuration-metadata.json | Partial | `local_meta.rs`: `unknown-metadata-field`, `duplicate-metadata-name`, `invalid-metadata-type`, `missing-metadata-description` | F* | No deprecated/unresolved class/bad default checks (library types never judged) |
| 8 | Driver class availability | Done | `missing-jdbc-driver` lib.rs:172, `build.rs` | | Opt-in (`checkDriverDependency`, default false), Info; transitive drivers invisible |
| 9 | Define key in local metadata | Partial | `defining_actions` lib.rs:221 (edit of an existing file found by `find_files`) | B | Creating the file when absent is U2 |
| 10 | Open/create datasource | Missing | no command, no code | B | No database API |
| 11 | Add Starters | Done | `add-starters` in `spring-initializr` (lib.rs:273, `starters.rs:82` pom/gradle insertion via `apply_edits`) | | Lives in another plugin; `pick_many` over the start.spring.io catalogue; skips BOM/repository starters |
| 12 | Edit Default Path, Show/Hide Values | Missing | no `[[contributes.commands]]` in spring-boot/plugin.toml | B | Run-configuration level |
| 13 | Spring Boot Diagnostics action | Missing | none (`run_command` only returns NotFound, lib.rs:358) | F* | Command + view now possible; scope of the original unverified |
| 14 | Run config, update policy, run endpoints | Partial | `spring-runtime` Actuator view only | B | No run-configuration contribution |
| 15 | Completion beyond keys/values | Partial | `nav.rs:93`: logger packages, profiles, `${}` keys | F* | No class names or resources in values |
| 16 | Value references | Partial | `nav.rs:147`: `spring.config.import` targets, `Class`-valued keys; `definition` lib.rs:263 | F* | No bean/resource references |
| 17 | Config file set | Partial | `config.rs:21` (spring-text): `application*`/`bootstrap*` names only | F* | No `spring.config.additional-location`/imports as a set; U3 for ignored dirs |
| 18 | Hover docs, rename/usages for keys | Missing | none | B | U1 |
| 19 | Inlay providers (10) | Missing | none | B | No inlay API |
| 20 | Injection (6) | Missing | none | B | No injection API |
| 21 | Logical structure (10) | Missing | none | B | No outline API |
| 22 | Auto-configuration bean model | Missing | none | B | Jars unreadable, U4 |
| 23 | Icons for config files | Done | `[[contributes.fileIcons]]` plugin.toml:41 | | Old audit said Missing |

Also new, not in the original rows: `dirties-context-mode`, `transaction-callback-signature` (checks.rs:289, 315), file templates (6, plugin.toml:47-80), metadata JSON checks.

## Spring Data -> spring-data (17 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Parameter types | Done | `query-parameter-type` lib.rs:677, `types.rs` | | Info severity, setting `checkQueryTypes` |
| 2 | Return types | Done | `query-return-type` lib.rs:646 | | Same setting |
| 3 | Pageable missing for `Page` | Done | `page-without-pageable` checks.rs:94 | | |
| 4 | `@Modifying` on update/delete | Done | `query-needs-modifying` checks.rs:116 (plus reverse `modifying-without-query` lib.rs:630) | | |
| 5 | JDBC empty sequence | Done | `jdbc-empty-sequence-name` checks.rs:126 | | |
| 6 | `@MappedCollection` key column | Done | `mapped-collection-key-column` checks.rs:138 | | |
| 7 | JDBC entity in several aggregates | Done | `jdbc-shared-entity` lib.rs:729 | | Gated on `data.jdbc`/`data.relational` imports and a complete index |
| 8 | JDBC associated DB elements | Missing | none | B | No database API |
| 9 | Mongo `@Query` fields + sort intention | Partial | `mongo-unknown-field` lib.rs:706, `mongo.rs` (top-level keys only) | F* | Sort-properties intention absent |
| 10 | Show Inherited Members | Done | command `show-repository-members` lib.rs:1025 (`pick` over `members.rs` static list) | | Display only; menu `when editorLangId == java` excludes Kotlin though the command parses Kotlin |
| 11 | Run Query in Console | Missing | none | B | Database API |
| 12 | File templates (Java/Kotlin) | Done | plugin.toml:25-34 | | Old audit said Missing |
| 13 | Live template `repository` | Missing | none | F* | Snippet completion exists in spring-web (`mapping_snippets`), not here |
| 14 | Resolved query preview | Done | marker tooltips `describe` lib.rs:763, markers lib.rs:955+ | | Tooltip only, no target; derived markers share the line with others |
| 15 | Injection in `@Query` | Missing | none | B | |
| 16 | Repository generator | Missing | none (template only) | B | U2 |
| 17 | Folding/surround | Missing | none | B | |

## Spring MVC -> spring-web (16 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Unresolved view | Partial | `unresolved-view` lib.rs:461 (Hint, setting default off) | F* | Resolver-unaware, so opt-in by design |
| 2 | Version SemVer format | Done | `invalid-api-version` checks.rs:119 | | |
| 3 | apiVersion inserter in tests | Done | `api-version-without-inserter` lib.rs:481 | | Hint |
| 4 | Generate Request Mapping / Exception Handler | Done | `generate.rs` (REQUEST_MAPPING, EXCEPTION_HANDLER code actions, lib.rs:718) | | Java only (`generate.rs` returns for non-.java) |
| 5 | File templates | Done | 6 templates plugin.toml:25-58 | | |
| 6 | Live templates | Partial | `mapping_snippets` lib.rs:569: getm, postm, putm, delm, patm, reqm (snippet items) | F* | No controller/router DSL snippets |
| 7 | Endpoints tool window | Done | view `endpoints` plugin.toml:65, `endpoints_view` lib.rs:724 | | README still says "not ported" for related items; the view is not in README's Not-ported list |
| 8 | Open in HTTP client, URL inlays | Partial | `copy-endpoint-curl` lib.rs:837 (`viewItem.context` + `input` + clipboard) | B | Sending a request needs a fixed `net:` host; clipboard is optional capability |
| 9 | URL completion in tests | Done | `request_completions` lib.rs:506 | | Test URL literals only |
| 10 | Kotlin controllers | Partial | path variable checks (README:17); functional routes in `endpoints.rs` | F* | api-version, init-binder, generate actions Java only |
| 11 | Handler gutter marker | Done | `url_markers` lib.rs:487 | | Tooltip with URL, `target: None`, icon None; skipped on lines already holding a marker |
| 12 | Template model variables | Missing | none | B | U4/U1 |
| 13 | Static resource references | Done | `static_files` lib.rs:529 (definition on literals) | | README "Not ported" still lists it: README stale |
| 14 | MVC XML namespace | Partial | `xml_checks.rs:9`: 6 element rules (view-controller, redirect-view-controller, status-controller, resources, mapping, exclude-mapping) | F* | Original models 28 DOM classes |
| 15 | Injection | Missing | none | B | |
| 16 | Docs, rename handler | Missing | none | B | U1 |

## Spring Security -> spring-security (7 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Obsolete request pattern format | Done | `obsolete-path-pattern` lib.rs:555 | | |
| 2 | Unresolved beans in annotations | Done | `unresolved-bean-reference` refs.rs:99 (`@WithUserDetails`, `@bean` in SpEL, XML refs) | | Info severity: library beans invisible |
| 3 | Security XML model | Partial | refs.rs:155-203 (`missing-attribute`, `invalid-attribute-value`, `unresolved-reference` tables) + lib.rs `intercept-url`, `duplicate-pattern`, `use-expressions-false` | F* | Hand-written tables |
| 4 | Self-invocation | Done | `security-self-invocation` lib.rs:733 | | Opt-in (`checkSelfInvocation`, default false); Java only (`!kotlin`, lib.rs:687) |
| 5 | Matcher inlay hints | Missing | none | B | |
| 6 | SpEL language support | Partial | completion lib.rs:844 | B | Injection |
| 7 | Matcher to endpoint navigation | Done | `definition` lib.rs:803 with `endpoints::covers` | | Own mapping scan duplicated from spring-web |

## Spring Cloud -> spring-cloud (6 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Unresolved legacy channel | Done | `stream-listener-channel` checks.rs:225 | | PARITY.md/README claim "not covered" may be stale |
| 2 | Stream handler signature errors | Partial | `stream-listener-return` :211, `send-to-without-return` :217 | F* | Only two constraints of the original |
| 3 | Bootstrap config scanned | Done | `bootstrap-configuration-scanned` checks.rs:200 | | Approximation: `@BootstrapConfiguration` together with `@Configuration`/`@Component`; no package scan resolution |
| 4 | Feign as endpoints | Done | view `feign-clients`, `feign_view` lib.rs:701 | | |
| 5 | Cloud auto-config model | Missing | none | B | |
| 6 | Cloud key hints | Partial | none in this plugin (`spring-boot` bundles the Cloud keys) | F* | Backlog chose to skip; effectively covered elsewhere |

## Spring GraphQL -> spring-graphql (3 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | `@BatchMapping` return type | Done | `batch-mapping-return-type` checks.rs:54 | | |
| 2 | Unmatched schema mapping | Partial | `unmapped-root-field` lib.rs:687; `unmapped-field` lib.rs:703 (Info, only fields with arguments on non-root types) | F* | Fields without arguments on non-root types stay unreported (property fetcher) |
| 3 | Endpoints provider | Done | view `endpoints`, `endpoints_view` lib.rs:828 (paths from `spring.graphql.*`) | | |

## Spring Messaging -> spring-messaging (3 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | XML namespaces model | Partial | `listener_xml.rs` RABBIT (7 rules) and JMS (2 rules) | F* | Other namespaces and elements absent |
| 2 | Bean link from XML `ref`/`method` | Done | `target_at` listener_xml.rs, `unresolved-listener-method` :175, `unresolved-reference` :164 (Info) | | README in this plugin may still say "no bean index" |
| 3 | Constants and SpEL as destinations | Partial | `constant_key` lib.rs:143, `parse_constants` lib.rs:393 | F* | SpEL not parsed |

## Spring Batch -> spring-batch (3 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Remaining XML model checks | Partial | `xml_rules.rs` RULES (10 elements, booleans only), wired lib.rs:535 | F* | No property types, no more enums |
| 2 | Job/step scope, late binding | Partial | `jobParameters[` keys completion/index lib.rs:190 | F* | No scope checks, SpEL itself not parsed |
| 3 | Typed bean refs | Missing | none (`unknown-bean` is name only) | B | U4 |

## Spring WebSocket -> spring-websocket (2 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | XML WebSocket check | Done | `xml_checks.rs` RULES, `check` wired lib.rs:520, xml provider + `on-language:xml` activation in plugin.toml | | Old audit: provider not registered for xml; now is |
| 2 | Messaging URL provider list | Done | view `destinations`, `destinations_view` lib.rs:422 | | |

## Spring Debugger -> spring-runtime (8 rows, unchanged)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Debug-session integration | Missing | no debugger capability | B | |
| 2 | Inlay hints of values | Partial | `config_markers` lib.rs:597 / `java_markers` lib.rs:517 tooltips | B | |
| 3 | Code vision | Partial | gutter markers only | B | |
| 4 | Project view decorators | Missing | none | B | |
| 5 | Run configuration extender | Missing | none | B | |
| 6 | Clone/persist datasource | Missing | none | B | |
| 7 | Runtime application provider | Partial | one URL setting, `net:localhost` (+ optional 127.0.0.1) | B | `net:` host fixed |
| 8 | Completion (2) | Missing | none | F* | Original unverified |

## Spring Initializr -> spring-initializr (2 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | New Project wizard | Partial | `generate_with_cli` lib.rs:200 (`spring init` terminal), `open_url` | B | U2, no open-project API |
| 2 | Dependency/version pick lists | Done | `pick_dependencies` lib.rs:249, `pick_boot_version` :264, `starters.rs:34/46` | | Only for start.spring.io (manifest host fixed); other servers warn |

## Totals (90 gap rows)

| Pair | Rows | Done | Partial | Missing | F | F* | B |
|---|---|---|---|---|---|---|---|
| Boot | 23 | 6 | 9 | 8 | 0 | 7 | 10 |
| Data | 17 | 10 | 1 | 6 | 0 | 2 | 5 |
| Web | 16 | 8 | 5 | 3 | 0 | 4 | 4 |
| Security | 7 | 4 | 2 | 1 | 0 | 1 | 2 |
| Cloud | 6 | 3 | 2 | 1 | 0 | 2 | 1 |
| GraphQL | 3 | 2 | 1 | 0 | 0 | 1 | 0 |
| Messaging | 3 | 1 | 2 | 0 | 0 | 2 | 0 |
| Batch | 3 | 0 | 2 | 1 | 0 | 2 | 1 |
| WebSocket | 2 | 2 | 0 | 0 | 0 | 0 | 0 |
| Runtime | 8 | 0 | 3 | 5 | 0 | 1 | 7 |
| Initializr | 2 | 1 | 1 | 0 | 0 | 0 | 1 |
| **Total** | **90** | **37** | **28** | **25** | **0** | **22** | **31** |

Old audit: 0 Done / 18 Partial / 72 Missing (F 14, F* 45, B 31). Now: 37 Done. No F row remains: all 14 old F rows are Done. F* 45 -> 22 left; B 31 -> 31 left (zero B rows flipped; Done rows came from F/F*). Old "Done" rows (22 outside the gap count) were re-verified by code and all still hold.

## Rows that changed vs the old audit

- Now Done (37 rows, all were Partial or Missing): Boot 4, 5, 6, 8, 11, 23; Data 1-7, 10, 12, 14; Web 2-5, 7, 9, 11, 13; Security 1, 2, 4, 7; Cloud 1, 3, 4; GraphQL 1, 3; Messaging 2; WebSocket 1, 2; Initializr 2.
- Now Partial (were Missing): Boot 3, 7, 9, 16; Web 1, 6; Batch 1, 2; Messaging 3.
- Boot 11 (Add Starters) is done by `spring-initializr`, not `spring-boot`. Web 8 stays Partial B: only copy-as-curl.
- Old Done rows (test-constructor, cron, matcher-order, debug-enabled, derived names, handler gutters, Feign gutters, tool windows): re-verified, all still hold. None of the old Done was found missing.

## Doc claims the code does not match (all understate the code except one)

- `spring-web/README.md:24` lists static resource references and (via PARITY) endpoint features as not ported; code has `static_files` (lib.rs:529). Stale, harmless.
- `spring-boot/README.md` and change-notes omit `early-event-listener`, `config-properties-prefix`, `default-package-application`, `redundant-*`, `dirties-context-mode`, `transaction-callback-signature` (all in checks.rs, Groups setting `application_setup`). Backlog Status "the @EventListener visibility rule stays dropped" is consistent.
- `spring-boot/README.md` says Go to Definition from `${}`/key values is IDE-provided; plugin only covers config.import and Class-valued keys.
- No claim found whose code is missing. Unverified-in-IDE risks: JSON metadata files given to plugins (README admits), `viewItemKind == e` menu `when`, snippet placeholders (`${1:/}`), New-menu template placeholders.
- `spring-data` menu `when = "editorLangId == java"` (plugin.toml) hides the command for Kotlin repositories; `spring-security` self-invocation and `spring-web` generate actions are Java only although manifests list Kotlin.

## 10 best next rows (value over cost, none needs a new API)

1. Boot 13 Spring Boot Diagnostics (F*): command that lists unknown/deprecated/invalid keys of the project into a view with `reveal_view`.
2. Boot 15 completion of class names and resource paths in values (F*): reuse `packages` and `find_files`; class-valued keys already known from metadata types.
3. Data 13 `repository` live template (F*): copy the `mapping_snippets` approach, small.
4. Boot 17 config file set (F*): honour `spring.config.import`, `spring.config.name`/`additional-location` as extra config files in `is_config_file`.
5. Boot 7 / 3 deepen metadata and binding checks (F*): deprecated block shape and constructor-binding rules, text only.
6. Data 9 sort-properties intention for Mongo `@Query` (F*): caret code action, JSON key reorder.
7. Web 6 / Web 10 live templates for controller and router DSL; port `api-version`, `init-binder` and generate actions to Kotlin (F*).
8. Cloud 2 / Batch 2 / Messaging 3 extra constraints (F*): stream handler signature rules, `#{jobParameters}` scope checks, SpEL literal destinations; each a few hundred lines of text parsing.
9. Security 3 / Messaging 1 / Batch 1 / Web 14 XML tables (F*): add the missing elements of each namespace; mechanical, share `spring-text::xml_check`.
10. Web 1 turn on `unresolved-view` by default once a view-resolver signal exists (F*), and fix the Kotlin gating of `show-repository-members`.
