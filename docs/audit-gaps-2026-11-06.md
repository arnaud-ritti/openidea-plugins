# Gap audit: IntelliJ originals against our plugins (2026-11-06)

What each IntelliJ plugin offers that ours does not, and whether the gap can be closed with the current OpenIDEA plugin API. It extends [audit-2026-11-06.md](audit-2026-11-06.md), which compared names only. This pass builds a feature inventory of each original and compares it row by row with the code on `main` at `c2c083d`.

Detail per pair (every gap row, with evidence from our code and the feasibility basis) is in `docs/audit-gaps-2026-11-06/`:
[Spring core and OSGi](audit-gaps-2026-11-06/A1-spring-core.md) · [rest of the Spring family](audit-gaps-2026-11-06/A2-spring-rest.md) · [Kubernetes, Flyway, Liquibase, OpenAPI, Dev Containers, JPA](audit-gaps-2026-11-06/B-infra-data.md) · [Symfony, Twig, Blade, Laravel](audit-gaps-2026-11-06/C-php.md) · [.ignore, Properties, Ideolog, Quarkus, GitLab, Issue Trackers, dotenv](audit-gaps-2026-11-06/D-tooling.md) · [API facts used for the feasibility column](audit-gaps-2026-11-06/api-facts.md).

## Method

1. The marketplace builds of the originals (JetBrains build 263.7307.x, Laravel Idea 13.4.1, Quarkus Tools 2.6.1, Symfony Plugin 2026.2.314, .ignore 4.5.7) were downloaded and unpacked. A script read, per plugin, the descriptors (`plugin.xml` and module descriptors), the message bundles, the vendor's inspection and intention description pages, and the file and live template names. It read no class files and no method bodies.
2. For the open-licence originals (.ignore, Properties, Ideolog, Quarkus Tools, GitLab, Issue Trackers, Spring OSGi, the Symfony Plugin, dotenv, the free Laravel plugin) the agents also read the public or Apache-2.0 sources.
3. Five research agents compared each pair with our code and recorded Done, Partial or Missing for every original feature, with evidence (a problem code, command id, template file, provider kind or README line).
4. Each Partial or Missing row got a class: **F** feasible with the current API; **F\*** feasible with a named risk (unverified in the IDE, false positives from invisible library beans, load cost); **B** blocked by a missing API (Track U items in `api-facts.md`).

## Totals

Counted as table rows: one row may group several near-identical original features (for example twelve Spring cache inspections), so rows are not features. The agents also counted per original feature in each file's own summary; those counts differ and are not comparable across files.

| Pair | F | F\* | B | Rows |
|---|---|---|---|---|
| Spring core -> `spring-beans` (+ `spring-boot`) | 17 | 21 | 18 | 56 |
| Spring OSGi -> `spring-osgi` | 2 | 3 | 5 | 10 |
| Spring Boot -> `spring-boot` | 4 | 9 | 10 | 23 |
| Spring Data -> `spring-data` | 4 | 8 | 5 | 17 |
| Spring MVC -> `spring-web` | 3 | 9 | 4 | 16 |
| Spring Security -> `spring-security` | 1 | 4 | 2 | 7 |
| Spring Cloud -> `spring-cloud` | 0 | 5 | 1 | 6 |
| Spring GraphQL -> `spring-graphql` | 1 | 2 | 0 | 3 |
| Spring Messaging -> `spring-messaging` | 0 | 3 | 0 | 3 |
| Spring Batch -> `spring-batch` | 0 | 2 | 1 | 3 |
| Spring WebSocket -> `spring-websocket` | 1 | 1 | 0 | 2 |
| Spring Debugger -> `spring-runtime` | 0 | 1 | 7 | 8 |
| Spring Initializr -> `spring-initializr` | 0 | 1 | 1 | 2 |
| Kubernetes -> `kube-explorer` | 9 | 25 | 22 | 56 |
| Flyway -> `flyway` | 1 | 3 | 6 | 10 |
| Liquibase -> `liquibase` | 0 | 6 | 10 | 16 |
| OpenAPI/Swagger -> `openapi` | 5 | 9 | 15 | 29 |
| Dev Containers -> `devcontainer` | 2 | 3 | 6 | 11 |
| JPA -> `jpa-assist` | 7 | 12 | 13 | 32 |
| Symfony Plugin -> `symfony` | 19 | 35 | 23 | 77 |
| Twig -> `twig` | 0 | 4 | 4 | 8 |
| Blade -> `blade` | 1 | 1 | 6 | 8 |
| Laravel Idea -> `laravel` (+ `blade`) | 8 | 14 | 15 | 37 |
| Free Laravel plugin -> `laravel` | 1 | 3 | 3 | 7 |
| .ignore -> `ignore` | 1 | 3 | 7 | 11 |
| Properties -> `properties` | 1 | 3 | 7 | 11 |
| Ideolog -> `ideolog` | 0 | 3 | 7 | 10 |
| Quarkus Tools -> `quarkus` | 0 | 8 | 10 | 18 |
| GitLab -> `gitlab` | 7 | 6 | 10 | 23 |
| Issue Trackers -> `tasks` | 0 | 3 | 9 | 12 |
| dotenv -> `dotenv` | 0 | 0 | 3 | 3 |
| **Total** | **95** | **210** | **230** | **535** |

Reading the totals:
- Of 535 gap rows, 18% are feasible today (F), 39% are feasible with a risk (F\*), 43% are blocked by a missing API (B).
- Most B rows trace to the same upstream items: Java and PHP type resolution (U4) for the Spring and Eloquent type checks; file creation and input dialogs (U2, U5) for generators, wizards and `make:*` commands; references and rename (U1); language injection; and the lack of streaming, decorations and caret control.
- Most F\* rows are inspections that depend on beans, classes or keys the plugin cannot see (library and auto-configured beans, vendor bundles), so they need a quiet-unless-sure design, and templates, intentions and providers whose behaviour in the IDE is unverified.
- Where the gap is widest in volume: Spring core (56 rows, 64 original inspections against 10 of ours), Symfony (77 rows, mostly inspections), Kubernetes (56 rows, mostly actions and live templates), JPA (32 rows) and Laravel Idea (37 rows).

## Limits of this audit

- **Behaviour is inferred, not observed.** The proprietary originals were read through descriptors, bundles and description pages only. A description says what a check reports, not its exact rules, messages or false-positive behaviour. Every new check built from this list must be specified from public documentation.
- **Counts depend on grouping.** Rows group features, and the agents grouped differently (for example the Kubernetes live templates, the Symfony inspections by theme). Compare rows within a pair, not across pairs. The per-feature counts in each detail file's own summary are approximate.
- **Inventory gaps.** The Spring core inventory names only 5 of its 10 intentions; the Kubernetes, OpenAPI and Dev Containers headers count more intentions than they list; the Spring Initializr inventory has almost nothing user-facing, so its two rows come from public knowledge of the New Project wizard. Of the 104 Symfony inspections, 53 are in the public source and 51 were judged from names and description text.
- **Quarkus Java side.** Hover and definition support in the Quarkus plugin was checked only in `plugin.toml` and `lib.rs`.
- **Nothing was run in the IDE.** "Done" is evidenced by code; "F" and "F\*" are API-level judgements. The unverified IDE behaviours listed in `LIMITATIONS.md` (template placeholders, prefilled code-action edits, language ids, activation, completion on `@`) still apply to every F\* row that depends on them.
- **Licence.** The proprietary originals are used for a feature inventory only. No code, messages, templates or icons from them are in this repository, and everything built from this list must be written from documentation.

## Suggested order for the F rows

Largest volume at the lowest risk first: Spring XML diagnostics (the index already holds XML beans, the checks only run on Java), the Symfony inspections that are in the public source, the GitLab actions (7), the Kubernetes feasible rows (9), JPA (7), the OpenAPI rows (5) and the Laravel Idea rows (8). The full list of F rows follows. Each is a candidate task; none has been scheduled.

## Done

### Plan 3: infra and data

Branch `gap-f-infra-data` ([plan](superpowers/plans/2026-11-06-gap-f-infra-data.md)). Built with unit tests against the mock host; nothing was run in the IDE.

**kube-explorer.** Done: deprecated resources (13 more table entries, checked against the Kubernetes deprecation guide), Chart.yaml unknown keys and unknown values, excessive whitespace trim, Helm update repository, How to install kubectl, CertificateSigningRequest, All Namespaces. Narrowed: the trim check is text based and a `}}` inside a string literal can confuse it; in All Namespaces, Create Secret, Apply Manifest, Apply Kustomization and Delete Manifest Resources are refused, and cluster-scoped or non-DNS-named custom resources are listed but take no action; the install hint never shows once a context is stored. Ruled out: Refresh (the button already existed); `requirements.yaml` (Helm 2) is not covered.

**openapi.** Done: possible specification candidate (an Info hint, `missing-openapi-root`, files over 1 MiB skipped), Swagger 2.0 templates (YAML and JSON), the Endpoints panel for JSON (the file is read in full before the 1 MiB cap applies), Reload specification (a Reload button in the preview, unverified because cargo cannot run page scripts). Narrowed: the live templates are three plain completion items (`openapi-path`, `openapi-operation`, `swagger2-root`), not abbreviation-based live templates; `swagger2-root` is also offered in OpenAPI 3 files, and the tab stops are unverified.

**devcontainer.** Done: folder structure, as a path-only hint, and completion, narrowed to top-level properties and enum or boolean values. Files over 256 KB publish nothing. The `jsonc` language id and whether completion `Document.path` equals the file-opened path are unverified.

**flyway.** Done: run configuration producer and info providers, narrowed to three detectors (`info`, `validate`, `migrate`) on a root `flyway.toml` or `flyway.conf`. They ignore the `runner` and `arguments` settings, a config in `conf/` is not detected, and `migrate` writes to the database in one click. Whether the host matches `files` at the project root only is unverified.

**jpa-assist.** Done: the three Kotlin rows (val attributes, immutable collections, data class without `equals`, `hashCode` and `toString`), persistent object class signature, entity listener warnings, and seven Add @X callback actions. Narrowed: the Kotlin checks cover the first class in a file and judge collection types by simple name (a user-defined `List` is flagged, `@Version val` still warns); listeners are found by simple file name; `parse_class` accepts classes only, so entities on an interface or enum drop out of the structure view and the generators. Ruled out: immutable not-nullable property (Kotlin), not implemented.

## Appendix: gap rows classed F (feasible with the current API)

Row text is shortened; the detail files hold the evidence and the exact basis.

### Spring core (20221) -> `spring-beans` (+ `spring-boot`, `crates/spring-text`)

- Spring AOP, XML structure: AopErrors, AopWarnings (2) (Pure XML structure; `spring_text::xml` + `publish_diagnostics` on file-opened/saved)
- Spring AOP: PointcutExpression (1) (Lexical check of the designator list on text we already locate)
- XML syntactic: BeanAttributes, InjectionValueConsistency, Scopes (known scope values), InjectionValueStyle (`<value>/<ref>` -> attributes, with fix),  (XML attribute/element rules; fix edits an existing file. Custom scopes (`session`, `request`, `bundle`) must be whitelisted)
- XML names: DuplicatedBeanNames, BeanNameConvention (2) (Index already holds XML beans (`model.rs`); extend `check_*` to XML sites)
- XML refs: AbstractBeanReferences (1) (`abstract` attribute is in the same file/index)
- Autowiring in code: JavaInjectionPointsAutowiring part B (misuse of `@Autowired` on constructors: several required, mixed with others) (part of 1) (Annotation structure only)
- StaticMembersAutowiring (1) (Modifier + annotation (`checks.rs` already reads both))
- SpringElStaticFieldInjection (1) (Same as above)
- Config: ProfileExpression (1) (Parses the string literal only)
- Cache: CacheNames, CacheAnnotationsOnInterface, CacheableAndCachePut (3) (Annotation arguments and declaration kind from `spring_text::java`)
- Testing: DirtiesContext (mode misuse), TestingTransactional (`@BeforeTransaction`/`@AfterTransaction` signature) (2) (Annotation and method signature only)
- Async/Events: AsyncMethod (return type void/Future) (1) (Return type text; type names resolved by simple name)
- Async/Events: EventListener (1) (Add a visibility rule in `spring-boot/checks.rs`)
- Kotlin: SpringCompilerPlugin (1) (Edit of existing `pom.xml`/`build.gradle*` via code action)
- UsePNamespace (1) (Same-file code action (also adds `xmlns:p`))
- IntroduceBean (1) (Same-file edit; id invented)
- UpdateSchema (1) (Same-file edit)

### Spring OSGi (16877) -> `spring-osgi`

- ElementsInconsistency: enum/attribute value validation (cardinality, auto-export, class-loader options, ordering basis) (Enumerations are in the decompiled model (`CollectionCardinality`, `AutoExportModes`, `OrderingBasis`); XML text only)
- `bundle` bean scope (SpringBundleCustomBeanScope) (Add value to the scope whitelist)

### Spring Boot -> spring-boot

- `@ConfigurationProperties` validity (Annotation text on project sources; the binding part is F*)
- Early application events with `@EventListener` (Event class simple names in the listener parameter)
- Invalid application setup (Package declaration and annotation text; code action edits existing file)
- Icons for config files (`fileIcons` contribution)

### Spring Data -> spring-data

- Pageable missing for `Page` return (Signature text)
- `@Modifying` on update/delete queries (Query string starts with update/delete)
- JDBC empty sequence name (Annotation text)
- JDBC `@MappedCollection` key column (Annotation and field type text)

### Spring MVC -> spring-web

- Endpoint version SemVer format (Literal text of the attribute)
- Generate Request Mapping / Exception Handler (2 actions) (Caret code action in an existing controller)
- Endpoints tool window (22 endpoint providers) (Tree/Table view over the existing mapping scan)

### Spring Security -> spring-security

- Obsolete request pattern format for Spring 7 (Pattern literal text)

### Spring GraphQL -> spring-graphql

- `@BatchMapping` return type (Return type text)

### Spring WebSocket -> spring-websocket

- Messaging URL provider (destinations in the Endpoints list) (Tree view over the destination index)

### Kubernetes (10485) -> kube-explorer

- Deprecated resources (1) (Extend the static table; text check)
- Chart.yaml unknown values (1) (Small fixed Chart.yaml schema; file read + `publish_diagnostics`)
- Chart.yaml unknown keys (1) (Same as 6)
- Excessive whitespace trim (1) (Text rule on `{{ }}` actions, rule is stated in the inventory)
- Helm update repository (1) (New command running `helm repo update` (`process:helm`))
- All Namespaces (1) (`-A` on list calls plus an "All" node)
- Refresh (1) (Add a toolbar button calling `reload`)
- How to install kubectl (1) (Notification with the instruction text)
- CertificateSigningRequest (Add `templates/csr.yaml`)

### Flyway (23740) -> flyway

- Run configuration producer and info providers (3 rows) (`runDetectors` on `flyway.toml`/`flyway.conf` with fixed args; liquibase already declares them)

### OpenAPI/Swagger (14394) -> openapi

- Possible specification candidate YAML/JSON (2 inspections) (Text heuristic (`paths`, `components`, `$ref`) in `publish_diagnostics`)
- Reload specification (action) (Html view button)
- Swagger 2.0 templates, YAML and JSON (2 templates) (Two more `fileTemplates`)
- Live templates `opnp`, `swag`, `endp` in YAML and JSON (6) (Extra completion items in the existing `completion` provider, gated to spec files)
- Endpoints panel (Reuse `json.rs` scanner for JSON)

### Dev Containers (21962) -> devcontainer

- Dev Container folder structure (inspection) (Path-only check on the opened file; the host skips dot-directories in `find_files`, so no scan (read_file for the parent))
- Completion (4 contributors) (`completion` provider on `json`, gated by file name inside the plugin)

### JPA (20202) -> jpa-assist

- Immutable collection-like property (Kotlin) (Kotlin attribute parsing exists in `model.rs`; text rule)
- Immutable not-nullable property (Kotlin) (Same)
- JPA entity with val attributes (Kotlin) (Same)
- Data class entity without equals/hashCode/toString (Skip when the three overrides exist)
- Persistent object class signature (Text rules in `checks.rs`)
- Entity listener warnings (Text rule)
- Entity listener methods (action) (Code action on the class line)

### Symfony Plugin (7219) -> `aritti.symfony`

- Inspection, TwigRouteCompareInspection (1) (Text pattern in `.twig`, route names already indexed)
- Inspection, ContainerSettingDeprecatedYaml/Xml (2) (Pure YAML/XML key match, same way as `deprecated-route-setting`)
- Inspection, TwigForConditionDeprecated, TwigComponentMixedSyntax, TwigComponentSelfMacroImport, TwigAssetsTagMissing (4) (Syntax-level scanning of `.twig` text (ours already has `twig::unclosed_tags`); asset check reuses `twig-missing-asset` logic)
- Inspection, TwigTranslationDomain (1) (Same index and same caveats as the PHP check)
- Inspection group Doctrine attribute and mapping checks (11): EmptyTable, TableAttributeDeprecatedProperties, LifecycleMethodVisibility, LifecycleMissi (Attribute or XML/YAML text of one file; paid originals, written from Doctrine docs)
- Inspection, RouteAttributeNamespaceDeprecated (1) (Import line match)
- Inspection, CommandInvokableReturnType, ConsoleInputMode (2) (Text patterns in one file)
- Inspection, FilesystemMirrorCopyOnWindows, TaggedCollectionDefaultMethod, FromCallableOption (3) (Key and named-argument text matching)
- Inspection, YamlQuotedEscaped (1, Partial) (Extend the existing scanner)
- Inspection, YamlUnquotedColon (1) (Same scanner)
- AddRouteAttributeIntention (Code action at the caret, in-file edit plus import)
- RouteActionParameterIntention, CommandInvokeParameterIntention (2) (The choice popup becomes one code action per parameter type)
- FormStringToClassConstantIntention (Static alias table, adds a `use` line)
- SymfonyNewYamlService, SymfonyNewXmlService, SymfonyNewPhpService (3) (Static fileTemplates (`container.yml/xml/php`))
- Symfony2.FormTypeMigrationGenerator (Whole-file edit by code action; same table as intention 4)
- SymfonyTwigBlockOverwrite (One code action per parent block, edits the open file)
- Tag names in YAML/XML/`tags:` (TaggedParameter) (Static list plus tags found in the index)
- Settings (7 configurables: container files, routing files, Twig namespaces, ...) (Scalar types only)
- Status bar widget, file icons, icon provider, editor notification (Status items, fileIcons globs exist)

### Blade (7569, proprietary) -> `aritti.blade`

- completion.contributor, weigher, htmlInText completion (Snippet completion items are in the SDK)

### Laravel Idea (13441, paid) -> `aritti.laravel` (+ `aritti.blade`)

- Inspection, LaravelFunctionsInspection (Text call match by path (`config/` excluded))
- Inspection, LaravelSimplifyOrderBy (Extend column rule)
- Intention, ConvertCssClassesToClassDirectiveCall, WrapCssClassesWithClassDirectiveCall (2) (Attribute text edit in Blade, caret range given)
- Intention, MakeAllArrayFromValidationString (1) (Same rewrite applied in a loop)
- Intention, ConvertAllControllerCalls (1) (In-file edits)
- Intention, ConvertRuleCallFromStringToObject (1) (Rule table, import insertion)
- Intention, ConvertRootFacadeCall (1) (Static alias table, import insertion)
- Other: settings (17 project, 6 application configurables) (Scalar types only)

### Laravel (free plugin `de.espend.idea.laravel` 0.15.4, MIT) -> `aritti.laravel`

- Template path configuration, `ide-blade.json` namespaces (`ViewCollector`, `JsonTemplatePaths`) (Read the JSON with `read_file` (explicit path); `find_files` would skip it if hidden)

### .ignore (7495) -> ignore

- settings: unignore actions toggle, starred templates (scalar settings + storage)

### Properties (11594) -> properties

- action Add New Property File (fileTemplates (extension `properties`))

### GitLab (22857) -> gitlab

- action Create Merge Request (Form view + REST `POST merge_requests`)
- action Create Snippet (Form view + REST snippets)
- action Share Project (Form view + `process:git`)
- note edit/delete, award emoji (REST PUT/DELETE/award_emoji with buttons)
- commit list and per-commit diffs in MR (REST commits/diffs + view)
- pipeline and job details in MR (REST pipelines/jobs + buttons)
- Open on GitLab menus (add `projectView.context` entry (log menus not available))

## Done

### Plan 2: rest of Spring

Branch `gap-f-spring-rest`, [plan](superpowers/plans/2026-11-06-gap-f-spring-rest.md). Built with unit tests against the mock host; nothing was run in the IDE.

**Spring Boot -> spring-boot.** `@ConfigurationProperties` (prefix part only), early application events and invalid application setup are done. The icon row is done as a file icon for `application*` and `bootstrap*` config files; it shows only under an icon theme that merges plugin icons, which is unverified.

**Spring Data -> spring-data.** All four rows are done. `Page` without `Pageable` is flagged; `Slice` without one is valid and not flagged. `@Modifying` covers update and delete `@Query`. The JDBC sequence row checks only an empty `sequence` attribute. `@MappedCollection` key column is a Hint.

**Spring MVC -> spring-web.** The version format row covers a lone literal in `version` of Java mapping annotations; Kotlin and functional routes are not covered. The generate row is two Java-only refactor actions (caret not moved, placeholder path). The Endpoints row is an Endpoints view for literal annotated mappings and functional routes, not the IDE's own window.

**Spring Security -> spring-security.** Done: a `**` before the last path segment is flagged in `requestMatchers`, `mvcMatchers`, `securityMatcher` and Kotlin `authorize`. No quick fix; possible false positive with `ant_path_matcher`.

**Spring GraphQL -> spring-graphql.** Done as a deny-list of `@BatchMapping` return types in Java and Kotlin. `Collection` and `Set` are valid, raw `Mono` and custom wrappers are quiet, and a Kotlin `= { ... }` lambda body still warns.

**Spring WebSocket -> spring-websocket.** Done as the plugin's own "WebSocket Destinations" view; a plugin cannot add items to the Endpoints view of `spring-web`.

Known limits and unverified items are in the "Added by gap F, plan 2" section of [LIMITATIONS.md](../LIMITATIONS.md).
