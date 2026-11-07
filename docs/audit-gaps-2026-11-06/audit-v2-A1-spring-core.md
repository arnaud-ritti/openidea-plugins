# Audit v2: A1 Spring core and Spring OSGi (re-audit at HEAD ac0edf6, API 1.2)

Method: every row of `A1-spring-core.md` re-checked by reading the code (`plugins/spring-beans/src`, `plugins/spring-boot/src`, `plugins/spring-osgi/src`, `plugin.toml`), not READMEs or change-notes. Nothing run in the IDE; "Done" means the code path exists and is wired (a provider, event or command in `plugin.toml`/`lib.rs`), not that it works in the IDE. Class: F feasible, F* feasible with a risk, B blocked by a missing API (API 1.2 basis: `reclass-A1-spring-core.md`, `/tmp/oi/plugins/README.md`). Row numbers are the old file's. Row 37 (5 unnamed intentions) stays unverified and is left out of the counts.

Paths below: `sb` = `plugins/spring-beans/src`, `bt` = `plugins/spring-boot/src`, `os` = `plugins/spring-osgi/src`.

Wiring checked: `problems_of` (`sb/lib.rs:231`) runs `xml_checks::check_xml` + `xml_class_checks::check` for beans XML, `checks::check_java` for Java, and for Kotlin only `field-injection` plus the `kotlin_plugin` group (`lib.rs:243-256`). `diagnose` is called from `line_markers` and from the file-opened/saved handler (`lib.rs:461`, `:925-936`, which also routes `META-INF/spring.handlers|schemas` to `spring-meta`). Unresolved-reference checks (`depends_on`, `component_scan`, `unresolved_targets`) run only with `checkUnresolvedReferences` (default false, `checks.rs:138`, schema default false).

## Inspections (rows 1-31)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | AOP XML structure | Done | `aop-structure` `sb/xml_checks.rs:93-143` (pointcut id/expression, aspect ref, advisor, advice method/pointcut) | - | was Missing |
| 2 | PointcutExpression | Done | `unsupported-pointcut-designator` `sb/aop.rs:74`, Java `checks.rs` `pointcut_designators`, XML `xml_checks.rs:157`; quiet when weaving plugin in build (`weaving_plugin_in_build`) | - | was Missing |
| 3 | MissingAspectjAutoproxy | Missing | no code | F* | needs Spring version; obsolete; not planned |
| 4 | JdkProxiedBeanType | Missing | no code | B | U4 |
| 5 | XML syntactic (BeanAttributes, InjectionValueConsistency, Scopes, InjectionValueStyle + fix, required tags) | Done | `xml-bean-attributes` `:279`, `xml-injection-value` `:304`, `xml-scope` `:316` (only case variants / one-edit typos of `KNOWN_SCOPES`, incl. `bundle`), `xml-value-style` `:342` + quick fix `style_fix` wired in `lib.rs:728`; required attrs via `SCAN_RULES` | - | was Missing. Scope check is deliberately narrow (custom scopes) |
| 6 | XML names (DuplicatedBeanNames, BeanNameConvention) | Done | `name_conventions` `xml_checks.rs:~176`, `duplicate_names` `:~196` (same `<beans>` + XML-vs-code, profile aware); XML-vs-XML across files quiet by design | - | was Partial (Java only). `diagnose` no longer skips beans XML |
| 7 | AbstractBeanReferences | Done | `xml-abstract-reference` `xml_checks.rs:265`; `ref` kinds only, not `parent` | - | was Missing |
| 8 | XML refs/files (unresolved refs/classes, Placeholders, IncorrectResourceType, ComponentScanInconsistency, InactiveProfile) | Partial | refs/`depends-on` `unresolved-reference` (Info, opt-in, skipped if file imports) `:441`; placeholders `xml-placeholder-unresolved` (opt-in, one placeholder file) `:531`; `xml-import-missing` `:459`; `xml-component-scan-filters` `:424` (only `use-default-filters=false` without include; type/expression enums via `SCAN_RULES`); `xml-inactive-profile` `:499` (Hint, needs `change-active-profiles`) | F* | not done: unresolved `class=` attribute, import target not a Spring XML (IncorrectResourceType), filter type/expression consistency |
| 9 | XML class-based (FactoryMethod, PublicFactoryMethod, LookupMethod, BeanInstantiation, RequiredProperty, ConstructorArg) | Partial | `xml-factory-method` `sb/xml_class_checks.rs:88`, `xml-abstract-class` `:75` (interface or abstract), `xml-required-property` `:108`, `xml-constructor-args` `:121`, `bean-method-missing` for init/destroy `:97`; no check of factory-method visibility, none of `lookup-method` (the `"lookup-method"` string at `:71` only exempts abstract classes) | F* | 4 of 6 done; the two missing are small, same machinery |
| 10 | XML typed (5) | Missing | no code | B | U4 |
| 11 | UnparsedCustomBean | Missing | no code | B | needs a JVM/classpath |
| 12 | Autowiring no/several beans | Missing | no code; `unresolved_bean` (`checks.rs:466`) only serves name checks | F* | not planned (auto-config beans invisible) |
| 13 | Autowired constructors misuse | Done | `autowired-constructors` `sb/injection_checks.rs:86` (several `required=true`, `required=false` allowed) | - | was Missing |
| 14 | StaticMembersAutowiring | Done | `static-injection` `injection_checks.rs:57` (Autowired/Value/Inject/Resource) | - | was Missing |
| 15 | SpEL on static field | Done | `static-spel-injection` `injection_checks.rs:47` | - | was Missing |
| 16 | Self-invocation (Transactional/Cacheable) | Partial | `self-invocation` `checks.rs:446`, default off; overloads now handled by `overloaded_sibling_calls` `annotation_checks.rs:219`, still returns nothing for classes with `extends` (`:226`) | F* | inherited callees need U4; LIMITATIONS.md:398 still says overloads are missed (stale) |
| 17 | TransactionalComponent | Missing | no code | F* | not planned (auto-config) |
| 18 | ConfigProxyMethods | Done | `configuration-proxy-method` `checks.rs:219` (private/final, proxied) and `unproxied-bean-method-call` `annotation_checks.rs:265` (`proxyBeanMethods=false`, inter-bean call) | - | was Partial |
| 19 | ProfileExpression | Done | `profile-expression` `annotation_checks.rs:127` (empty, mixed `&`/`|`, parens), Error | - | was Missing |
| 20 | DependsOn, ComponentScan | Partial | `unresolved-depends-on` `checks.rs:485`, `component-scan-package-missing` `:512`, Java only, opt-in; XML `<component-scan>` base-package not resolved; Kotlin none | F* | library/auto-config beans invisible |
| 21 | LookupInjection, ScheduledMethods(scheduler), TestingOverridenBeanResolve, JavaBeanUnresolvedMethods, RequiredAnnotation | Partial | `unresolved-bean-name` for `@Lookup`/`@Scheduled(scheduler)`/`@MockitoSpyBean` `sb/target_checks.rs:47` (opt-in); `bean-method-missing` for `initMethod`/`destroyMethod` and `@TestBean(methodName)` `:105,:127` (default on); `required-not-setter` `:127`. Not covered: `@MockitoBean` names, `@Lookup` method shape, "@Required property not injected" in Java | F* | 4 of 5 groups; `@MockitoBean` is one more entry in the `match` at `:34` |
| 22 | ContextConfiguration, TestingSql, ImportResource, PropertySource | Partial | `resource-not-found` `target_checks.rs:58` (opt-in) for all five annotations, but `resource_exists` returns None (quiet) for relative paths except `ContextConfiguration`/`Sql`, `file:` and globs; no default-context-file check | F* | `classpath:`/absolute only; roots limited to `src/{main,test}/resources` |
| 23 | Cache: names, interface, Cacheable+CachePut | Done | `cache-names-missing` `annotation_checks.rs:199` (quiet with `@CacheConfig` or any supertype), `cache-annotation-on-interface` `:176` (Hint, skips repositories), `cacheable-and-cache-put` `:210` | - | was Missing |
| 24 | CacheableComponents | Partial | `cache-spel-syntax` `:316`, `cache-attribute-conflict` `:320`; nothing for `@CacheConfig` misuse or missing `CacheManager` (grep `CacheManager` empty) | F* | CacheManager needs auto-config knowledge |
| 25 | DirtiesContext, TestingTransactional | Partial | `dirties-context-mode` `bt/checks.rs:289`, `transaction-callback-signature` `:315` reports only non-void return (parameter/static rules unsourced) | F* | DirtiesContext Done, transactional half |
| 26 | AsyncMethod | Done | `async-return-type` `sb/annotation_checks.rs:150` | - | was Missing |
| 27 | EventListener | Partial | `event-listener-signature` `bt/checks.rs:173` (more than one param Error, plain-value return Hint); visibility rule deliberately absent | F* | was F; downgraded: Spring documents no visibility rule (backlog "Not planned") |
| 28 | spring.handlers / spring.schemas | Done | `spring-handler-class-missing`, `spring-schema-file-missing` `sb/target_checks.rs:154-157`, routed in `lib.rs:928`; class only checked when the project owns the package root | - | was Missing |
| 29 | SpEL syntax/refs | Missing | only `cache-spel-syntax` (bracket/quote balance, annotations of `@Cache*`); no SpEL in `@Value` | B | injection blocked; syntax-only would be F* |
| 30 | SpringCompilerPlugin (Kotlin) | Done | `kotlin-spring-plugin` `sb/kotlin_build.rs:38` + Gradle quick fix `fix_for` (wired `lib.rs:691-713`); Maven diagnostic without fix | - | was Missing |
| 31 | Kotlin: RedundantOpen, Java-style call, PathVariable DSL | Done | `kotlin-redundant-open` `kotlin_build.rs:71`, `kotlin-reified-extension` `sb/kotlin.rs:313`, `router_path_variables` `kotlin.rs:278` (all in `problems_of` `:248-254`, gated by `checkKotlinSpringPlugin`) | - | was Missing. Hints only |

## Intentions (rows 32-37)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 32 | UsePNamespace | Done | `xml_actions::use_p_namespace` via `xml_code_actions` `lib.rs:731` | - | was Missing; skips dotted/dashed names |
| 33 | IntroduceBean | Done | `introduce_bean` `lib.rs:734` | - | was Missing; needs `class` |
| 34 | UpdateSchema | Done | `update_schema` `lib.rs:737` | - | was Missing |
| 35 | HardCodedStringToPlaceholder | Partial | command `externalize-annotation-string` (`commands.rs:317`, editor.context menu): `pick` of a properties file, `input` key, replaces the string; the `key=value` line goes to the clipboard, the properties file is not edited | F* | edit of another file stays impossible from a command (only focused editor); a code-action form does not exist |
| 36 | ParseCustomBean | Missing | no code | B | JVM |
| 37 | 5 unnamed intentions | unverified | - | - | - |

## Actions, templates, live templates (rows 38-45)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 38 | Generate Setter/Constructor/Autowire/Event Listener | Done | commands + `editor.context` menus in `plugin.toml`, handlers `lib.rs:943-946`, `commands.rs:generate` (`pick`, `input`, `apply_edits`); Java only, `when = "editorLangId == java"` | - | was B. Insertion at fixed spots (after fields/members), not caret |
| 39 | Create Spring Component / Config | Done | `fileTemplates` `spring-component-java|kotlin`, `spring-configuration-java|kotlin` in `plugin.toml`, files in `plugins/spring-beans/templates` | - | was F*. One entry per stereotype, no chooser; `$PACKAGE$` unverified in the IDE |
| 40 | 15 templates (30 files) | Partial | 12 templates (Application, Boot Test, Component, Configuration, ConfigurationProperties, Service, each Java+Kotlin); the 3 beans-XML templates are absent (`ls templates`) | F* | missing: beans XML (3). README says "Java and Kotlin" only, consistent |
| 41 | Live templates (10) | Missing | no snippets in `spring-beans`; but `sb/sites.rs:203` already offers pointcut designators as snippet completion items, so snippets with tab stops work for completion | F | old basis ("no tab stops") was wrong; completion-based abbreviations, no Tab expansion |
| 42 | ChangeActiveProfiles | Done | `change-active-profiles` `commands.rs:33` (`pick_many`, storage); read by `duplicate_names` (`checks.rs:275`) and `inactive_profiles` (`xml_checks.rs:488`) | - | was B. Feeds our checks only |
| 43 | SpringConfigurationCheck / Dump | Done | `check-spring-configuration`, `dump-spring-configuration` (`commands.rs:~250,:282`), view `spring-report` in `plugin.toml` | - | was F* |
| 44 | Show Implicit / XML Infrastructure Beans | Missing | no code | B | classpath |
| 45 | Beans diagram | Partial | `bean-graph` view (`lib.rs:41`); no diagram API | B | unchanged |

## Other extension points (rows 46-57)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 46 | Annotation-string refs/completion | Partial | `Site` list `sb/sites.rs:28-40` (`Qualifier`, `Named`, `DependsOn`, `Lookup`, `Resource`, SpEL `@bean`, pointcuts) + indexer references `sb/indexer.rs:205-240` for `@Profile`/`@ActiveProfiles`, `@ComponentScan` packages, `@Value` placeholders, resource annotations (Go to Declaration project-wide) | F* | no scope or init-method refs; no completion for profiles/resources/packages (indexer is navigation only) |
| 47 | XML model | Partial | `<property name>` navigation + completion of setters `lib.rs:521,:577`, `xml_class_checks.rs:setters`; no scope/depends-on value completion, no constructor-arg name | F* | was "no name->setter" |
| 48 | Extra line markers | Partial | indexer markers for `publishEvent`/listeners (`indexer.rs:87-120`), class <-> XML bean (`:74`), injection (`:123`); `@ComponentScan` and `@Bean` override markers skipped (LIMITATIONS.md:239); Kotlin publishers/listeners none | F | was F*; remaining items are effort only |
| 49 | Multi-target chooser / advised-method markers | Partial | chooser: aliases `indexer.rs:59-71` (type hierarchy) feed multi-target definitions; advised methods missing | B | 49a done, 49b stays B (per-document index) |
| 50 | SpEL language | Partial | `#param` completion and `@bean` navigation (`sites.rs:31`) | B | injection |
| 51 | Structure view | Missing | no code | B | structure symbols exist only for plugin-owned languages |
| 52 | Rename / usages / gotoRelated | Missing | no rename; Go to Declaration covered by rows 46/48 | B | U1 |
| 53 | implicitUsageProvider | Missing | no code | B | |
| 54 | Folding / typedHandler | Missing | no code | B | |
| 55 | Unresolved-ref quick fixes | Missing | no code | B | U2 |
| 56 | Spring facet / contexts | Missing | one global context (`World::new`) | B | no project model |
| 57 | Diagram provider | Partial | same as 45 | B | |

## Spring OSGi (rows 1-10, old numbering)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| O1 | Enum/attribute validation | Done | `osgi-attribute-value` `os/lib.rs:~388-440` (cardinality, auto-export, class-loader, member-type, basis, ranking, timeout) | - | was Partial |
| O2 | Unresolved refs as errors | Partial | `unresolved-osgi-ref` `os/lib.rs:371` (Info, only `service|listener|registration-listener` `ref`; `comparator-ref`/`depends-on` not) | F* | Info because library beans invisible |
| O3 | comparator-ref must be Comparator | Missing | no code | B | |
| O4 | ServiceCommon | Partial | `osgi-service-common` `os/lib.rs:449`, only when hierarchy is in sources | B | |
| O5 | Listener first-parameter vs service type | Missing | `osgi-listener` `:468` checks method existence, visibility, void, `(Service, Map)`/`(ServiceReference)` shape, interface; not the first parameter's type vs the service class | B | |
| O6 | `ref` completion filtered by interfaces | Partial | sorts implementors first (`sort_text`, `os/lib.rs:~555`), never hides | F* | |
| O7 | `bundle` scope | Done | `bundle` in `KNOWN_SCOPES` `sb/xml_checks.rs:19` | - | was n/a; osgi README still lists it as not ported (stale) |
| O8 | ReferenceEffectiveTypeProvider | Missing | no code | B | |
| O9 | Compendium DOM model | Partial | only `<osgix:` tag-name completion (`compendium_tags`); no attribute checks | F* | host trigger inside tag names unverified |
| O10 | Generate OSGi bean | Done | `generate-osgi-bean` command + menu (`os/plugin.toml`, `os/lib.rs:644`, `pick`+`input`+`apply_edits`) | - | was B. README "Not ported" list is stale |

## Regressions, wrong old evidence, unbacked claims

- Regressions: none found. Old Done rows still hold: `field-injection` (`sb/lib.rs:214`, Java and Kotlin), `scheduled-signature` (`checks.rs:198`).
- Wrong or stale old evidence: row 41 ("no tab stops") is wrong, snippet completion items exist and `sites.rs:203` uses them. Row 6 "diagnose skips xml" no longer true. Row 16 "misses overloads" no longer true (`annotation_checks.rs:219`); row 42/38 "B" became Done via `pick`/`pick_many`/`input`.
- Docs the code does not match (all understate): `plugins/spring-osgi/README.md` "Not ported" lists the Generate provider and the `bundle` scope (both built); `LIMITATIONS.md:398` says self-invocation misses overloaded callees (handled) and `:399` that Kotlin got no checks "from this pass" (Kotlin has 3 hint checks plus kotlin-spring).
- Claims checked and backed: spring-beans change-notes items (Check/Dump commands, Move Annotation String, 12 file templates, unresolved name/resource checks, component-scan filters, spring.handlers/schemas, Kotlin hints, setter completion, hierarchy-following markers, document-index keys). `LIMITATIONS.md:415` itself marks hierarchy aliases as not verified in the real host.
- Reclass prediction not delivered: `reclass-A1` row 48 said `@ComponentScan` and `@Bean` override markers would flip to F and be added; they were skipped (LIMITATIONS.md:239).

## Totals (rows, excluding row 37)

| | Done | Partial | Missing | rows |
|---|---|---|---|---|
| Spring core -> spring-beans/boot (rows 1-57) | 22 | 18 | 16 | 56 |
| Spring OSGi (10) | 3 | 4 | 3 | 10 |
| **Total** | **25** | **22** | **19** | **66** |

Class of the 41 rows not Done: F 2 (41, 48) / F* 19 / B 20. Spring core: F 2 / F* 16 / B 16 (34 rows). OSGi: F* 3 (O2, O6, O9) / B 4 (O3, O4, O5, O8).
Old audit: 2 inspections Done, 6 Partial, 56 Missing (plus 0 rows Done); old class totals F 26 / F* 71 / B 31 (per feature), 56+10 rows. Today's open rows are fewer (41 vs 66).

## Rows that changed since the old audit

Missing -> Done: 1, 2, 5, 7, 13, 14, 15, 19, 23, 26, 28, 30, 31, 32, 33, 34, 38, 39, 43(F* -> Done), 42(B -> Done), O10.
Partial -> Done: 6, 18, O1; Missing -> Done also O7 (n/a before).
Missing -> Partial: 8, 9, 21, 22, 24, 25, 35, 40 (templates), 47 (name->setter), 48, 49.
Class changes: 27 F -> F* (no documented rule), 41 F* -> F (snippets), 48 F* -> F, 38/42 B -> Done.
Unchanged: 3, 4, 10, 11, 12, 16, 17, 20, 29, 36, 44, 45, 46, 50-57, O2-O6, O8, O9 (O2/O6 gained Info/sort behaviour already counted Partial).

## Ten best next rows

1. 41 Live templates: snippet completion items with tab stops (component, service, configuration, configProperties, bean; Java and Kotlin) in `spring-beans`; F, S.
2. 40 Beans-XML file templates (3): add `templates/*.xml` + `fileTemplates` entries; F*, S.
3. 9 `lookup-method` and factory-method visibility in `xml_class_checks.rs`; F*, S.
4. 21 `@MockitoBean` (`name`/`value`) in `target_checks.rs:34`; F*, S.
5. 24 `@CacheConfig` misuse (syntactic half of CacheableComponents); F*, S.
6. 48 `@ComponentScan` and `@Bean` override markers in `indexer.rs`, Kotlin publishers/listeners; F, M.
7. 22 Relative `@ImportResource`/`@PropertySource` paths and `ContextConfiguration` default context file in `resource_exists`; F*, M.
8. 8 Import target that is not a Spring XML, filter type/expression consistency, unresolved `class=`; F*, M.
9. 47 / 46 Completion for XML `scope`, `depends-on` values and constructor-arg names, and for profiles/resources in annotations (indexer is navigation only); F*, M.
10. O2/O6/O9 and docs: `unresolved-osgi-ref` for `comparator-ref`/`depends-on`, fix the stale osgi README and `LIMITATIONS.md:398-399`; F*, S.
