# A1: Spring core and Spring OSGi

Sources read: inventories orig-20221.md and orig-16877.md; our `plugins/spring-beans` (README, plugin.toml, settings.schema.json, `checks.rs`, `lib.rs`, `sites.rs`), `plugins/spring-boot` (README, `checks.rs`), `plugins/spring-osgi`, LIMITATIONS.md, PARITY.md. OSGi: also a local decompiled copy (Apache-2.0). Spring core stays clean-room (inventory only).

Facts verified in code:
- `spring-beans` publishes diagnostics only for Java (`diagnose()` returns early for `xml`; Kotlin gets only `field-injection`). Codes: `field-injection`, `non-proxyable-method`, `scheduled-signature`, `configuration-proxy-method`, `bean-name-convention` (hint), `duplicate-bean-name`, `self-invocation` (off), `unresolved-depends-on` and `component-scan-package-missing` (off).
- `spring-boot` has `event-listener-signature`, `test-constructor-autowire`, `invalid-cron`, deprecated and unknown keys.
- No fileTemplates, commands, menus or live-template-like snippets in any Spring plugin (`grep fileTemplates plugins/spring-*/plugin.toml` is empty; only `jpa-assist` uses them).
- Providers are `definition`, `line-markers`, `completion`, `code-actions` (`plugin.toml`), plus two views: Spring Beans, Spring Bean Graph.

---

## Spring core (20221) -> `spring-beans` (+ `spring-boot`, `crates/spring-text`)

Original features: 64 inspections / 10 intentions (inventory names only 5) / 14 actions / 15 file templates (30 files incl. .html previews) / 10 live templates / ~58 other extension-point kinds (grouped into 12 rows below).
Inspections: 2 Done, 6 Partial, 56 Missing. Gaps counted per original feature:
- Inspections (62 gaps): F 21 / F* 33 / B 8.
- Intentions (5 named): F 3 / F* 1 / B 1 (5 unnamed: unverified).
- Actions (14): F 0 / F* 6 / B 8.
- File templates (15): F* 15.
- Live templates (10): F* 10.
- Extension-point rows (12): F* 3 / B 9.

Done (no row): field injection warning `field-injection` with constructor fix (SpringAutowiredFieldsWarning); `scheduled-signature` (ScheduledMethodInspection, parameters only).

### Inspections by group

| # | Group / original features | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | Spring AOP, XML structure: AopErrors, AopWarnings (2) | `<aop:config>` advice without pointcut/pointcut-ref, aspect without ref, pointcut without id | Missing: no XML diagnostics | F | Pure XML structure; `spring_text::xml` + `publish_diagnostics` on file-opened/saved |
| 2 | Spring AOP: PointcutExpression (1) | Designators Spring does not support (`@this`, `adviceexecution`, `cflow`...) in `@Pointcut`, advice annotations and XML expressions | Missing; only designator completion exists (`sites.rs` Pointcut site) | F | Lexical check of the designator list on text we already locate |
| 3 | Spring AOP: MissingAspectjAutoproxy (1) | `@Aspect` on Spring < 2.5 with no `<aop:aspectj-autoproxy/>` | Missing | F* | Needs Spring version from build file; obsolete, near-zero value |
| 4 | Spring AOP: JdkProxiedBeanType (1) | JDK-proxy bean typed by class instead of interface | Missing | B | Type resolution (U4) |
| 5 | XML syntactic: BeanAttributes, InjectionValueConsistency, Scopes (known scope values), InjectionValueStyle (`<value>/<ref>` -> attributes, with fix), plus required-tag part of SpringXmlModel (4 + part) | class vs factory-bean conflicts, ref/value/sub-element rules, scope names, style hint | Missing | F | XML attribute/element rules; fix edits an existing file. Custom scopes (`session`, `request`, `bundle`) must be whitelisted |
| 6 | XML names: DuplicatedBeanNames, BeanNameConvention (2) | duplicates (per profile) and naming convention in `<bean>` | Partial: `duplicate-bean-name` and `bean-name-convention` exist but run on Java only (`diagnose` skips xml); XML beans are in the index | F | Index already holds XML beans (`model.rs`); extend `check_*` to XML sites |
| 7 | XML refs: AbstractBeanReferences (1) | `ref`/`parent` to a bean with `abstract="true"` | Missing | F | `abstract` attribute is in the same file/index |
| 8 | XML refs and files: SpringXmlModel unresolved bean refs/classes, Placeholders (`${}` against `<ctx:property-placeholder>` files), IncorrectResourceType (`<import resource>` not a Spring XML), ComponentScanInconsistency (`<filter>` type/expression), InactiveProfile (5) | unresolved refs, placeholders, resource kinds, filter consistency, dimmed inactive profile | Missing (navigation of `ref` exists, no warning) | F* | Library beans/auto-config are invisible (LIMITATIONS "Missing API"); `find_files` skips ignored paths; active profile needs a plugin setting (no run-config data) |
| 9 | XML class-based (project classes only): FactoryMethod, PublicFactoryMethod, LookupMethod, BeanInstantiation (interface/abstract class), RequiredProperty (`@Required` setters), ConstructorArg (6) | existence, static/public/abstract/visibility of referenced methods; instantiation of abstract types; missing required props; arg count vs constructors | Missing | F* | `spring_text::java` parses project classes; quiet when hierarchy reaches a library (same approach `spring-osgi` uses); FP risk on library parents |
| 10 | XML typed: InjectionValueType, XmlAutowiring, RequiredBeanType, XmlAutowireExplicitly, UtilSchema (5) | assignability of injected values/beans, autowire candidates, util:* types | Missing | B | Java type resolution (U4) |
| 11 | XML custom: UnparsedCustomBean (1) | custom namespace handlers not run | Missing | B | Running namespace handlers needs classpath and a JVM (no process/class access) |
| 12 | Autowiring in code: JavaInjectionPointsAutowiring part A (no bean / several beans of the type, missing qualifier) (1) | unsatisfied or ambiguous injection | Missing, deliberately not done (LIMITATIONS: mostly false positives) | F* | Library/auto-configured beans unknown; high FP risk |
| 13 | Autowiring in code: JavaInjectionPointsAutowiring part B (misuse of `@Autowired` on constructors: several required, mixed with others) (part of 1) | constructor annotation misuse | Missing | F | Annotation structure only |
| 14 | StaticMembersAutowiring (1) | `@Autowired`/`@Value`/`@Inject` on static fields/methods | Missing | F | Modifier + annotation (`checks.rs` already reads both) |
| 15 | SpringElStaticFieldInjection (1) | `@Value("#{..}")` on a static field | Missing | F | Same as above |
| 16 | Proxies: TransactionalMethodCalls, CacheableMethodCalls (2) | self-invocation bypasses the proxy | Partial: `self-invocation`, default off, skips `extends` classes, misses overloads | F* | Text-based; needs type resolution to be reliable (U4) for inherited/overloaded callees |
| 17 | Proxies: TransactionalComponent (1) | no `PlatformTransactionManager` bean for a `@Transactional` component | Missing | F* | Boot auto-configures it: needs auto-config knowledge, high FP risk |
| 18 | Config: ConfigProxyMethods (1) | `proxyBeanMethods=false` misuse and non-overridable `@Bean` methods | Partial: `configuration-proxy-method` covers private/final in proxied classes; the `false` side (inter-bean calls in lite mode) missing | F* | Text-based call detection on same-class methods |
| 19 | Config: ProfileExpression (1) | empty profile, mixed `&`/`\|` without parentheses | Missing | F | Parses the string literal only |
| 20 | Config: DependsOn, ComponentScan (2) | unresolved bean name / package | Partial: `unresolved-depends-on`, `component-scan-package-missing`, off by default, project beans only, Java only; XML `<component-scan>` and Kotlin not checked | F* | Library and auto-config beans invisible (README) |
| 21 | Config: LookupInjection, ScheduledMethods (`scheduler=` bean), TestingOverridenBeanResolve (`@MockitoBean`/`@TestBean` name and factory method), JavaBeanUnresolvedMethods (`initMethod`/`destroyMethod`), RequiredAnnotation (Java `@Required` not injected) (5) | bean name or method must exist | Missing | F* | Project beans/methods F; library-provided targets FP |
| 22 | Config files: ContextConfiguration, TestingSql, ImportResource, PropertySource/TestPropertySource (4) | unresolved resource paths (`classpath:`, relative, default context file) | Missing | F* | `find_files` skips ignored/generated paths (LIMITATIONS); `classpath:` roots guessed from `src/*/resources` |
| 23 | Cache: CacheNames, CacheAnnotationsOnInterface, CacheableAndCachePut (3) | missing cache name; annotation on interface; both annotations on one method | Missing (`non-proxyable-method` covers a different cache rule) | F | Annotation arguments and declaration kind from `spring_text::java` |
| 24 | Cache: CacheableComponents (1) | wrong `@Cache*` config: key/condition, `@CacheConfig`, no `CacheManager` | Missing | F* | Syntactic part F; "no CacheManager bean" needs auto-config knowledge |
| 25 | Testing: DirtiesContext (mode misuse), TestingTransactional (`@BeforeTransaction`/`@AfterTransaction` signature) (2) | wrong attribute for class/method level; lifecycle signature | Missing | F | Annotation and method signature only |
| 26 | Async/Events: AsyncMethod (return type void/Future) (1) | invalid return type | Missing | F | Return type text; type names resolved by simple name |
| 27 | Async/Events: EventListener (1) | `@EventListener` signature rules | Partial: `spring-boot` `event-listener-signature` (parameter count, plain-return hint); original also flags non-public methods | F | Add a visibility rule in `spring-boot/checks.rs` |
| 28 | Spring Core: SpringHandlersSchemasHighlighting (1) | unresolved class/file in `spring.handlers`/`spring.schemas` | Missing | F* | Diagnostics on any file path via events; class targets library -> FP |
| 29 | Spring Core: SpringEl syntax and references (1) | syntax errors, unresolved properties/methods in SpEL | Missing; only `#param` completion | B | SpEL injection blocked (LIMITATIONS); a hand-written syntax-only check would be F*, resolution is U4 |
| 30 | Kotlin: SpringCompilerPlugin (1) | add kotlin-spring plugin to build | Missing | F | Edit of existing `pom.xml`/`build.gradle*` via code action |
| 31 | Kotlin: RedundantOpenModifier, JavaStyleCallReplaceableByKotlinExtension, PathVariableKotlinRouterDsl (3) | redundant `open`; `::class.java` call with extension; undeclared path variables in router DSL | Missing; Kotlin gets no checks | F* | Text patterns; `open` check must know the plugin is on (build file) and that the callee is a Spring API |

### Intentions

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 32 | UsePNamespace (1) | `<property>` -> `p:` attribute | Missing | F | Same-file code action (also adds `xmlns:p`) |
| 33 | IntroduceBean (1) | inner bean -> top-level with id | Missing | F | Same-file edit; id invented |
| 34 | UpdateSchema (1) | DTD doctype -> schema header | Missing | F | Same-file edit |
| 35 | HardCodedStringToPlaceholder (1) | move property value to a properties file | Missing | F* | Edits an existing properties file; no chooser (U5), picks first/convention file |
| 36 | ParseCustomBean (1) | run namespace handlers for a tag | Missing | B | See row 11 |
| 37 | 5 further intentions counted by the inventory but not named | unknown | unverified | - | - |

### Actions, templates, live templates

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 38 | Generate Setter Dependency, Constructor Dependency, Autowire Bean, Event Listener (4) | Generate menu entries that ask which bean or event | Missing (we only have "Use constructor injection" on field injection) | B | Chooser dialogs (U5); caret control for insertion |
| 39 | Create Spring Component (generic + Java + Kotlin), Spring Config (4) | New menu entries, choose stereotype | Missing | F* | `fileTemplates` per stereotype; placeholders unverified; no choice dialog, so one template per stereotype |
| 40 | Templates (15 templates, 30 files): Application, Boot Test, Component, Configuration Properties, Configuration, Service (Java and Kotlin) + 3 beans XML | file templates in New menu | Missing | F* | `fileTemplates` contribution (`jpa-assist` uses it); `$PACKAGE$` unverified |
| 41 | Live templates (10: component, configuration, service, configProperties, bean; Java and Kotlin) | abbreviations | Missing | F* | No live-template contribution; only completion items with plain insert text (no tab stops) |
| 42 | ChangeActiveProfiles (1) | pick active profiles | Missing | B | Quick-pick (U5); a string setting would be F* but feeds only our own checks |
| 43 | SpringConfigurationCheckAction, DumpSpringConfiguration (2) | run the model checks on demand; dump the configuration | Missing (Spring Beans view lists beans, no check/dump) | F* | Command + diagnostics/Html view; depends on rows 5-9 existing |
| 44 | Show Implicit Beans, Show XML Infrastructure Beans (2) | view filters | Missing | B | Implicit beans come from classpath/auto-config (no classpath API) |
| 45 | ShowBeansDiagramAction (1) | UML bean diagram | Partial: Spring Bean Graph view (hand-drawn SVG; node click may not navigate, PARITY) | B | No diagram API |

### Other extension points

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 46 | References/completion in annotation strings (psi.referenceContributor 20, referenceInjector 6, completion.contributor 10, resourceTypeProvider 4) | resolve and complete: `@Qualifier`, `@DependsOn`, profiles, scopes, `@PropertySource`/`@ImportResource`/`@ContextConfiguration` paths, `@ComponentScan` packages, `@Value` placeholders, `@Bean` init methods | Partial: `@Qualifier`/`@Named`/`@Resource`/`@DependsOn`/`getBean`/XML refs/SpEL `@bean`/pointcuts done (README, `sites.rs`); the rest missing | F* | `definition`/`completion` providers; project files only; `@Value` keys are IDE built-in per README |
| 47 | XML model (dom.implementation 110, dom.converter 46): property/constructor-arg name -> setter/parameter, scope, depends-on, attribute value completion | typed XML editing | Partial: `ref`/`parent`/`depends-on`/`factory-bean`, `class` navigation + completion; no `name`->setter | F* | Project classes by `spring_text::java`; library parents unknown |
| 48 | Extra line markers (24): event publish/listen, class -> XML definition, `@ComponentScan`, `@Bean` overrides | gutter navigation | Partial: injection -> bean, bean -> injectors (`line_markers`) | F* | `line-markers` provider, one target per marker |
| 49 | Multi-target marker chooser, advised-method markers (aop.provider, pointcutDescriptor, 2+) | chooser popup; which methods an aspect advises | Missing (first target only) | B | One-target `LineMarker`; signature matching needs U4 |
| 50 | SpEL language (spring.el.contexts 10, multiHostInjector 8) | highlighting/completion/refs in `#{}` | Partial: `#param` completion only | B | Injection blocked |
| 51 | Structure view (logicalStructureElementsProvider 36, xmlStructureView 4, fileStructureGroupRuleProvider) | beans in structure | Missing | B | No structure API |
| 52 | Rename/Find usages/element descriptions/useScopeEnlarger/gotoRelated (renameHandler 5, findUsagesHandlerFactory 2, gotoRelatedProvider 4, semContributor 6) | refactoring and usage search for beans | Missing | B | U1 |
| 53 | implicitUsageProvider (10) | beans are not "unused" | Missing | B | No unused-symbol API (host-side) |
| 54 | Folding of placeholders (4), typedHandler (4) | UI editing aids | Missing | B | Folding/caret control blocked |
| 55 | Unresolved-reference quick fixes (codeInsight.unresolvedReferenceQuickFixProvider 4) | create bean/class | Missing | B | File creation (U2) |
| 56 | Spring facet and contexts: facetType, framework.detector, projectConfigurable, settings provider, config sets, active profiles, testingImplicitContextsProvider 12, customModule/LocalComponentsDiscoverer, componentScanExtender | which beans form which context; test contexts; modules | Missing (one global project-wide context) | B | Project model/run-configuration API absent (LIMITATIONS "Missing API") |
| 57 | Bean diagram provider (diagram.Provider 4, gutterDiagramActionProvider 4) | UML diagrams | Partial (row 45) | B | No diagram API |

Not user-facing, skipped: plugin-to-plugin hooks (`spring.customNamespaces`, `spring.customBeanScope`, `spring.valueConverter`, `spring.effective.types.provider`, `spring.factoryMethodTypeHandler`, `spring.autodetected.models`), file-based indexes, properties suppressors, compiler validators.

Ours beyond the original: `non-proxyable-method` (private/final/static on proxied annotations, security/retry annotations too); `test-constructor-autowire`, `invalid-cron`, Boot metadata key completion and deprecation fixes (spring-boot); Spring Beans tool window; AspectJ named-pointcut navigation.

---

## Spring OSGi (16877) -> `spring-osgi`

Original features: 3 inspections / 0 intentions / 0 actions / 0 templates / 4 extension-point kinds (dom.implementation 6, effective types, bundle scope, custom namespace). Done 0 / Partial 3 inspections / Missing 4 EP rows. Gaps (10 rows): F 2 / F* 3 / B 5.

Our checks (evidence `lib.rs`): `osgi-elements-inconsistency`, `osgi-service-common`, `osgi-listener`; definition and completion for `ref`, `comparator-ref`, `depends-on`, `interface`, listener methods.

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | ElementsInconsistency: enum/attribute value validation (cardinality, auto-export, class-loader options, ordering basis) | invalid attribute values in `osgi:*` elements | Partial: only the three exclusivity rules (`interface` vs `<interfaces>`, `ref` vs nested bean, `comparator-ref` vs `<comparator>`) | F | Enumerations are in the decompiled model (`CollectionCardinality`, `AutoExportModes`, `OrderingBasis`); XML text only |
| 2 | ElementsInconsistency: unresolved bean references (`ref`, `depends-on`, `comparator-ref`) reported as errors | red reference | Partial: navigation and completion only, no problem | F* | Library/auto-config beans invisible; spring-osgi uses project beans only |
| 3 | ElementsInconsistency: `comparator-ref` bean must be a `java.util.Comparator` | type of referenced bean | Missing (README "Not ported") | B | U4 for non-project hierarchies; F* when the whole hierarchy is in sources |
| 4 | ServiceCommon: registration-related type checks against effective types | service bean vs exported interfaces beyond direct implements | Partial: `osgi-service-common` checks the bean implements each interface when the hierarchy is in sources | B | U4 (library supertypes) |
| 5 | Listener: first parameter of registration/unregistration method vs the service bean class; beans of an `<osgi:reference>` as service implementors | signature vs service type | Missing (README "Not ported") | B | U4 |
| 6 | Completion of `ref` restricted to beans implementing the exported interfaces | filtered completion | Missing (all beans offered) | F* | Project-source hierarchy only; fall back to all beans when it reaches a library |
| 7 | `bundle` bean scope (SpringBundleCustomBeanScope) | `scope="bundle"` accepted | n/a today (no scope check in `spring-beans`); becomes a whitelist entry when row 5 of the Spring core table lands | F | Add value to the scope whitelist |
| 8 | ReferenceEffectiveTypeProvider | `<osgi:reference>` supplies bean types to the Spring model | Missing | B | U4 plus `spring-beans` has no type model for XML beans |
| 9 | Compendium DOM model (`osgix:` placeholder, managed-service) | tag/attribute completion and checks | Missing | F* | XML text; low value; unverified whether host XML support already handles it |
| 10 | OsgiSpringBeanGenerateProvider (not in the inventory; in the decompiled source) | Generate menu: add OSGi service/reference bean | Missing | B | Generate chooser (U5) |

Ours beyond the original: none identified.

---

## Group summary

Counts are per original feature (rows 31-57 above group extension points; EP rows counted once).

| Pair | Original features | Done | Partial | Missing | F | F* | B |
|---|---|---|---|---|---|---|---|
| Spring core -> spring-beans (+spring-boot) | inspections 64, intentions 10 (5 named), actions 14, templates 15, live templates 10, EP rows 12 | 2 | 10 (6 inspections, 1 action, 3 EP rows) | rest | 24 (inspections 21, intentions 3) | 68 (inspections 33, intentions 1, actions 6, templates 15, live templates 10, EP rows 3) | 26 (inspections 8, intentions 1, actions 8, EP rows 9) |
| Spring OSGi -> spring-osgi | inspections 3, EP/other rows 7 | 0 | 3 inspections (rows 1-5 split them into checks) | 4 EP rows | 2 | 3 | 5 |

Totals: F 26 / F* 71 / B 31 (Spring core F 24 / F* 68 / B 26; OSGi rows F 2 / F* 3 / B 5).

Notes on the OSGi counts: the Partial/Missing split is approximate because the three OSGi inspections are decomposed into several checks; rows are checks, not the 3 inventory entries.
