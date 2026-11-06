# Group C audit: PHP frameworks (Symfony, Twig, Blade, Laravel)

Conventions: evidence codes are `Diagnostic.code` values in our `src/`; "README" = the plugin README. Classes: F, F*, B as in `api-facts.md`. "Paid" = the check is not in the public MIT repository (`src/main/resources/META-INF/pay.xml` is empty, the class is absent from `plugin.xml`), so it was judged from its name and the inventory text only. Of the 104 Symfony inspections, 53 are in the public source and 51 are not.

---

## Symfony Plugin (7219) -> `aritti.symfony`

Original features: 104 inspections / 16 intentions / 30 actions (2 are menu groups) / 16 file templates / 31 grouped extension-point rows. Ours: 9 text-based problem checks plus 7 more codes, 2 code actions, 9 templates, 5 line-marker kinds.

Totals (rows counted per feature, not per group):

| Kind | Original | Done | Partial | Missing | n/a | F | F* | B |
|---|---|---|---|---|---|---|---|---|
| Inspections | 104 | 25 | 1 | 78 | 0 | 27 | 47 | 5 |
| Intentions | 16 | 1 | 0 | 15 | 0 | 4 | 7 | 4 |
| Actions | 30 | 14 | 0 | 14 | 2 | 5 | 5 | 4 |
| File templates | 16 | 7 | 0 | 9 | 0 | 9 | 0 | 0 |
| Extension-point rows | 31 | 1 | 7 | 23 | 0 | 3 | 13 | 14 |
| Total | 197 | 48 | 8 | 139 | 2 | 48 | 72 | 27 |

### Inspections by theme (104)

Done means a same-purpose check with a code in our `checks.rs` / `lib.rs`. Where the original attaches a "create ..." quick fix we lack it (see the quick-fix row at the end of this section).

| Theme | Original count | Done | Partial | Missing | Classes |
|---|---|---|---|---|---|
| Routes | 11 | 9 | 0 | 2 | Done: PhpRouteMissing (`missing-route`), DuplicateLocalRoute (`duplicate-route`), ControllerMethodInspectionYaml/Xml (`controller-method-missing`), RouteSettingDeprecatedInspectionYaml/Xml and RouteControllerDeprecatedInspectionYaml/Xml (`deprecated-route-setting`: `Bundle:Controller:action`, `pattern`, `_method`/`_scheme`, `type: annotation`), TwigRouteMissingInspection (`twig-missing-route`). Missing: TwigRouteCompareInspection, TwigPathReplacement |
| Services / DI | 22 | 7 | 0 | 15 | Done: MissingServiceXml/Yaml (`missing-service`, once the compiled container exists), XmlDuplicateServiceKey (`duplicate-service`, XML only), XmlDuplicateParameterKey (`duplicate-parameter-key`), ServiceDeprecatedClassesYaml/Xml (`deprecated-service-class`), ServiceNamedArgumentExists (`named-argument-missing`). Missing: see rows 3 to 8 |
| Controller / constructor practice (PHP) | 14 | 0 | 0 | 14 | ConstructorNoRequestStack, ConstructorNoSession, ConstructorNoToken, ConstructorNoUser, AutowireWrongClass, AutowireControllerAsService, ProjectDirParameter, ConstructorTwigExtensionHeavyConstructor, ConstructorCommandHeavyConstructor, ConstructorDeprecatedInjection, RequestStackInAction, UseControllerShortcuts, ControllerActionMissingExit, FirstClassCallable. All paid |
| Twig | 13 | 4 | 0 | 9 | Done: TwigTemplateMissing and PhpTemplateMissing (`missing-template`, `render*`, `#[Template]`), TwigAssetMissing (`twig-missing-asset`), PhpAssetMissing (`missing-asset`). Missing: rows 11 to 14 |
| Forms / validator | 4 | 1 | 0 | 3 | Done: FormTypeAsClassConstant (`form-type-string`). Missing: FormTypeBuilderInstance, FormViewTemplate, ConstraintDeprecatedArray |
| Translations | 4 | 3 | 0 | 1 | Done: TwigTranslationKey, PhpTranslationKey (`missing-translation`, with the add-key quick fix), PhpTranslationDomain (`missing-translation-domain`). Missing: TwigTranslationDomain |
| Doctrine | 21 | 0 | 0 | 21 | All paid. Rows 17 to 19 |
| Annotations / attributes | 2 | 1 | 0 | 1 | Done: TemplateMissingAnnotationPhpAttributeLocal (`#[Template]` handled by `missing-template`). Missing: RouteAttributeNamespaceDeprecated |
| Commands | 5 | 0 | 0 | 5 | CommandStaticDeprecation, RegisterCommandsDeprecation, ConsoleInputMode (paid), CommandInvokableReturnType, CommandInvokableReturnValue (public) |
| Deprecation (Symfony 8.1 API) | 6 | 0 | 0 | 6 | HttpFoundationPropertyWrite, HttpKernelEventArgument, FilesystemMirrorCopyOnWindows, ConstraintViolationListFindByCodes, TaggedCollectionDefaultMethod, FromCallableOption. All paid |
| YAML / XML syntax | 2 | 0 | 1 | 1 | YamlQuotedEscaped (Partial: ours `unquoted-yaml-scalar` covers unquoted `@` and `%` starts), YamlUnquotedColon |
| Total | 104 | 25 | 1 | 78 | |

### Gap table (inspections)

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | Inspection, TwigRouteCompareInspection (1) | Route name in a comparison with the current route (`app.request.attributes.get('_route') == 'x'`) does not exist | Missing: no such code (only `twig-missing-route` for `path()`/`url()`) | F | Text pattern in `.twig`, route names already indexed |
| 2 | Inspection, TwigPathReplacement (1) | A hard-coded `href` path that matches a known route; suggests `path()` | Missing | F* | HTML-language provider; matching URL paths against route patterns with placeholders; false positives |
| 3 | Inspection, Yaml/Xml/PhpServiceInstance (3) | Service id passed where the constructor parameter type is another class | Missing | F* | Needs class hierarchy; project classes only via `index.rs`; vendor types unresolvable, so silent there |
| 4 | Inspection, YamlParameter (1) | `%param%` that no parameter definition has | Missing (only quoting of `%`) | F* | Parameters would come from the compiled container XML; many bundle/env/dynamic parameters, false-positive risk |
| 5 | Inspection, YamlClass (1) | `class:` of a service that does not exist as a PHP class | Missing | F* | Project PSR-4 index only; vendor FQCNs not findable (`find_files` skips `vendor/`), so must stay silent for them |
| 6 | Inspection, ServiceDeprecatedClassesInspectionPhp (1) | Deprecated class/service used from PHP | Missing (Yaml/Xml variants exist) | F* | Same text check as `deprecated-service-class`, but PHP usage sites are many and untyped |
| 7 | Inspection, TaggedExtendsInterfaceClassYaml/Xml (2) | Tagged service whose class lacks the interface or parent the tag requires | Missing | F* | Static tag-to-interface map plus class hierarchy; project classes only |
| 8 | Inspection, Xml/YamlServiceArgument (2), EventMethodCall (1), ContainerConstantXml/Yaml (2) | Missing or mismatching service arguments vs constructor; listener `method:` absent in class; `!php/const` or `type="constant"` not defined | Missing (`named-argument-missing` covers only `$name:` keys) | F* | Needs constructor signatures and autowiring knowledge, inheritance, and vendor constants; narrow to project classes |
| 9 | Inspection, ContainerSettingDeprecatedYaml/Xml (2) | Old service keys (`factory_class`, `factory_method`, `factory_service`) | Missing (only route settings are covered) | F | Pure YAML/XML key match, same way as `deprecated-route-setting` |
| 10 | Inspection group "controller / constructor practice" (13 of 14, all but AutowireWrongClass) | Style and runtime-safety advice for PHP classes: do not inject RequestStack, Session, token, user in constructors; avoid heavy constructors in Twig extensions and commands; deprecated injection patterns; Request type-hint instead of RequestStack; controller shortcuts; missing `throw` before `createNotFoundException`; `kernel.project_dir`; first-class callable syntax | Missing, no code | F* | Parameter and call matching by name after resolving `use` imports; deciding "this is a controller/service" is textual and can misfire; PHP-version gate for first-class callables |
| 11 | Inspection, AutowireWrongClass (1) | Autowired parameter whose type is a different class than the service intended | Missing | B | Needs type resolution of the parameter and the bound service (U4) |
| 12 | Inspection, TwigForConditionDeprecated, TwigComponentMixedSyntax, TwigComponentSelfMacroImport, TwigAssetsTagMissing (4) | `for ... if` form, mixing `<twig:X>` with `{% component %}` blocks, `{% from _self import %}` inside a component, missing asset in `{% javascripts %}`/`{% stylesheets %}` | Missing | F | Syntax-level scanning of `.twig` text (ours already has `twig::unclosed_tags`); asset check reuses `twig-missing-asset` logic |
| 13 | Inspection, TwigWithExtendsWithRootHtml (1), TwigEnumFunction (1) | Content outside blocks in a template that extends another; `enum()` / `enum_cases()` class missing or not an enum | Missing | F* | First needs block nesting and comment/verbatim awareness; second needs class lookup and "is enum" for project classes only |
| 14 | Inspection, TwigVariablePath, TwigVariableDeprecated, TwigExtensionDeprecated (3) | Unknown member of a typed Twig variable; deprecated PHP method or deprecated Twig tag/filter/function | Missing | B | Typed variables and their PHP members (U4); extension classes live in `vendor/` |
| 15 | Inspection, FormTypeBuilderInstance, FormViewTemplate, ConstraintDeprecatedArray (3) | Class used as form type is not a form type; form passed to a template without `createView()`; array first arg of a Constraint constructor | Missing | F* | Class hierarchy (project only); local-variable origin by text; use-resolved class names |
| 16 | Inspection, TwigTranslationDomain (1) | Domain in a Twig translation call has no translation file | Missing in Twig (PHP side done as `missing-translation-domain`) | F | Same index and same caveats as the PHP check |
| 17 | Inspection group Doctrine attribute and mapping checks (11): EmptyTable, TableAttributeDeprecatedProperties, LifecycleMethodVisibility, LifecycleMissingCallbacksAttribute, OneToManyMappedByRequired, OneToOneOwingSide, JoinColumnNullable Php/Xml/Yaml, DiscriminatorMapDuplicateClass Php/Xml | Flag useless, deprecated or invalid ORM mapping details in `#[ORM\...]`, XML and YAML mapping | Missing | F | Attribute or XML/YAML text of one file; paid originals, written from Doctrine docs |
| 18 | Inspection group Doctrine QueryBuilder and Collections deprecations (9): QueryBuilderAdder, ClearParameter, SetParametersArray, QueryBuilderAlias, QueryBuilderArray, JoinWithToOn, CollectionsCriteriaOrdering, QueryBuilderSortDirection, CurrentDateTimeDefault | Flag call patterns removed in ORM 3 / DBAL 2.11 / Collections | Missing | F* | Call matching by method name only (receiver type unknown), so any class with `orderBy` etc. would match; DQL string parsing for WITH/ON |
| 19 | Inspection, DoctrineMappingArrayAccessDeprecation (1) | `$mapping['x']` on mapping value objects | Missing | B | Needs the receiver type (U4) |
| 20 | Inspection, RouteAttributeNamespaceDeprecated (1) | Old `Routing\Annotation\Route` import instead of `Routing\Attribute\Route` | Missing | F | Import line match |
| 21 | Inspection, CommandInvokableReturnType, ConsoleInputMode (2) | `__invoke()` of an `#[AsCommand]` class lacks `: int`; several base modes OR-ed in one InputArgument/InputOption | Missing | F | Text patterns in one file |
| 22 | Inspection, CommandStaticDeprecation, RegisterCommandsDeprecation, CommandInvokableReturnValue (3) | `$defaultName` static property; `Bundle::registerCommands()`; invokable command returning a non-int | Missing | F* | Class-role detection by `extends`/attribute text; return-value typing only for literals |
| 23 | Inspection, FilesystemMirrorCopyOnWindows, TaggedCollectionDefaultMethod, FromCallableOption (3) | Option key `copy_on_windows`; `defaultIndexMethod`/`defaultPriorityMethod` arguments; YAML options combined with `from_callable` | Missing | F | Key and named-argument text matching |
| 24 | Inspection, HttpFoundationPropertyWrite, HttpKernelEventArgument, ConstraintViolationListFindByCodes (3) | `$request->attributes = ...` writes; grouped attributes in `setController`; list implementation without `findByCodes` | Missing | F* | Receiver type unknown for the first two; third needs "implements" by use-resolved name |
| 25 | Inspection, YamlQuotedEscaped (1, Partial) | Deprecated YAML quoting forms: unquoted start with `@`, `%`, backtick, `|`, `>`, and an unescaped backslash in double quotes | Partial: `unquoted-yaml-scalar` covers `@` and `%` only, `yaml_quotes.rs` | F | Extend the existing scanner |
| 26 | Inspection, YamlUnquotedColon (1) | Colon inside an unquoted mapping value | Missing | F | Same scanner |
| 27 | Quick fixes attached to Done checks (not counted above) | Create template, did-you-mean for route/template/domain/key typos, create missing method | Missing: only "Add translation key" and "Quote" exist (`lib.rs:608-650`) | create template: B (U2); typo suggestions: F | A quick fix can edit existing files only; replacing a misspelt name with a close indexed name is an in-file edit |

### Intentions (16)

Done 1, Missing 15. Ours has two code actions only: add translation key, quote YAML scalar.

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | TwigTranslationKeyIntention | Create a missing translation key in the right file | Done: "Add translation key ... to ..." (`lib.rs:643`) | | |
| 2 | AddRouteAttributeIntention | Add `#[Route]` with generated path and name to a public controller method | Missing | F | Code action at the caret, in-file edit plus import |
| 3 | RouteActionParameterIntention, CommandInvokeParameterIntention (2) | Add a supported parameter (Request, etc.) to an action or `__invoke` | Missing | F | The choice popup becomes one code action per parameter type |
| 4 | FormStringToClassConstantIntention | `'text'` becomes `TextType::class` | Missing (the diagnostic `form-type-string` exists without a fix) | F | Static alias table, adds a `use` line |
| 5 | PhpServiceIntention, PhpServiceArgumentIntention (2) | Generate or re-sync a service definition from a PHP class constructor | Missing | F* | Edits an existing `services.yaml`; constructor parsing is textual; the original asks YAML or XML in a dialog (default to the file present) |
| 6 | XmlServiceTagIntention, YamlServiceTagIntention (2) | Add the tags a class's interfaces or attributes imply | Missing | F* | Static interface-to-tag map; class parents outside the project unresolvable |
| 7 | DoctrineRepositoryClassConstantIntention | `'App:Product'` becomes `Product::class` | Missing | F* | Alias to namespace mapping from entity files |
| 8 | CommandToInvokableIntention | Rewrite a Command with `execute()` into an invokable command | Missing | F* | Large textual rewrite of one class, must handle `configure()` |
| 9 | TwigExtensionToAttributeIntention | Move `getFilters()`/`getFunctions()`/`getTests()` to attributes | Missing | F* | Textual rewrite of one class |
| 10 | XmlServiceArgumentIntention, XmlServiceSuggestIntention (2) | Suggest service ids for an `<argument>` from the constructor parameter type | Missing | B | Matching a type to services needs type hierarchy (U4) |
| 11 | PhpPropertyArgumentIntention | Inject a missing `$this->svc` property through the constructor | Missing | B | Property type comes from the service (U4) |
| 12 | PhpBundleCompilerPassIntention | New compiler pass file and registration in `build()` | Missing | B | Creates a file (U2) |

### Actions (30)

Done 14, Missing 14, n/a 2 (`Symfony.Menu`, `Symfony.TwigLanugage` are menu groups). Ours: New menu templates (Command, Controller, Form Type, Twig Extension, Compiler Pass, Kernel Test, Web Test) cover the 7 `SymfonyNew*` actions and the 7 `*Shortcut` variants, with a fixed `App\...` namespace (README, `plugin.toml` fileTemplates).

| # | Original feature | What it does | Ours | Class | Basis |
|---|---|---|---|---|---|
| 1 | SymfonyNewYamlService, SymfonyNewXmlService, SymfonyNewPhpService (3) | New service definition file | Missing | F | Static fileTemplates (`container.yml/xml/php`) |
| 2 | Symfony2.FormTypeMigrationGenerator | Class-constant migration of all form type strings in a file | Missing | F | Whole-file edit by code action; same table as intention 4 |
| 3 | SymfonyTwigBlockOverwrite | Insert an override of a parent block | Missing (block completion exists) | F | One code action per parent block, edits the open file |
| 4 | Symfony2.ServiceGenerator, ServiceArgumentGenerator (2) | Generate service or arguments from the class | Missing | F* | As intention 5; the Generate menu is not an API surface, expose as code action |
| 5 | Symfony.TwigTranslationGenerator | Create a translation key from a Twig selection | Missing (quick fix handles missing keys only) | F* | Selection range given to code actions; edits an existing translation file |
| 6 | Symfony.TwigFormFieldGenerator, Symfony.FormBuilderFieldGeneratorAction (2) | Generate form fields from an entity | Missing | F* | Entity properties parsed from text; Doctrine types mapped by a static table |
| 7 | Symfony2.PhpCompilerPassGenerateGenerator | New compiler pass | Missing | B | File creation (U2) |
| 8 | SymfonyTwigExtendsGenerator | Choose a template to extend | Missing | B | Choice popup (U5) |
| 9 | Symfony.AddApplicationDatabaseConnections, Navigation.FromTableToDoctrineEntity (2) | Database tool integration | Missing | B | No database API |

### File templates (16)

Done 7: command (attributes style, `templates/Command.php`), compiler_pass, controller_attributes, form_type, kernel_test_case, twig_extension, web_test_case. Missing 9, all F (static `fileTemplates`; placeholder expansion is unverified):

| # | Original | Ours | Class |
|---|---|---|---|
| 1 | command.php, command_configure.php, command_property.php, command_invokable.php (4 command variants) | Missing, one Command template only | F |
| 2 | controller_annotations.php | Missing, attributes variant only | F |
| 3 | twig_extension_function_attribute.php | Missing | F |
| 4 | container.php, container.xml, container.yml (3) | Missing | F |

Ours beyond the original: Event Subscriber and Voter templates, a Symfony Profiler view (latest 100 requests), cache warmup/clear commands, Encore/AssetMapper entry completion, Twig `twig.yaml` namespaces, duplicate-route, unclosed Twig tags.

### Other extension points (31 grouped rows)

Done 1, Partial 7, Missing 23.

| # | Original feature (kind, group) | What it does | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | GotoCompletion registrars: routes, templates, services, translations, assets, Encore, roles | Completion and navigation of core names | Done (README table) | | |
| 2 | Service parameters (`%x%`) completion and goto | Parameters from container config | Missing | F* | Compiled container XML or YAML `parameters:`; bundle parameters need the compiled file |
| 3 | Event names and listeners (PhpEventDispatcher, listener tags, MessageSubscriber) | Event class and name completion, listener navigation | Missing | F* | Textual scan of `getSubscribedEvents`, `#[AsEventListener]`, tags |
| 4 | Tag names in YAML/XML/`tags:` (TaggedParameter) | Completion of container tag names | Missing | F | Static list plus tags found in the index |
| 5 | Form types, options, field names (about 5 registrars, 3 in PHP attributes) | Completion of type classes and options | Missing | B | Options come from the type hierarchy including vendor (U4) |
| 6 | Doctrine: entity and repository names, mapping fields in XML/YAML, QueryBuilder and DQL field completion, types (6 registrars, 4 type providers, DQL language) | Doctrine awareness | Missing | B | Types (U4) and language injection for DQL |
| 7 | Console command names (PhpCommand, ConsoleHelper) | Command-name completion and navigation | Missing | F* | `#[AsCommand]` and `$defaultName` scan |
| 8 | Validator constraint messages, translation placeholders | Messages link to translation keys | Missing | F* | Constraint class option names static; message key lookup uses the translation index |
| 9 | Security voters, `#[IsGranted]`/Security expressions | Voter attributes and expression completion | Partial: roles only (`Roles` row of README) | F* | Voter `supports()` constants parsed textually |
| 10 | Twig filters, functions, tests of project and vendor extensions (FilterGoto, TwigExtensionSymbol, GlobalExtensionVariableCollector) | Completion and navigation to extension definitions | Partial: static lists from Twig docs (README) | F* | Project extensions via `new TwigFilter('x'` scan; vendor extensions not findable (U3) |
| 11 | Twig macros (MacroScope, TwigMacroSymbol) | Completion and navigation of imported macros | Missing (LIMITATIONS notes it) | F* | Textual `{% macro %}` and `{% import %}` scan in one or two files |
| 12 | Twig variable collectors (11): globals from `twig.yaml`, `app`, `for`/`set`/`include with`, `{# @var #}`, macro scope, controller render | Variables offered in templates | Partial: keys of the array passed to `render()` only (`twig_variables`) | F* | All textual sources; no types |
| 13 | Typed Twig variable members, variable type inlays, annotators | `{{ post.| }}` members, type hints | Missing | B | Types (U4), inlay hints |
| 14 | UX Twig components `<twig:X>`, Stimulus controllers, Vite (registrars, symbol contributor, variable collectors) | Completion and navigation | Missing | F* | Component classes and `assets/controllers` scanned textually |
| 15 | Line markers (16 providers: routes in PHP/YAML/XML, services, DI, Doctrine, forms, serializer, Vite, tests) | Gutter navigation | Partial: 5 kinds (controller to template, template to controllers, block override, class to service, YAML service to class) | F* | `line-markers` provider with one target; routes would link name to definition |
| 16 | Go-to-related (PHP and Twig, 5 collectors) | Controller, template, model, form, route | Partial via gutter | F* | Same provider; no related-items popup |
| 17 | Goto symbol and file contributors (10 + 1: services, routes, commands, blocks, macros, entities, tables, UX components) | Search Everywhere entries | Missing | B | No symbol or file contributor provider (U1) |
| 18 | Find usages, usage types, referencesSearch (routes, form fields, Twig methods, templates), usage targets (4+4+2) | Usages of routes and templates | Missing | B | U1 |
| 19 | Move-file handler for templates, rename | Updates references | Missing | B | U1 |
| 20 | Documentation providers and link handlers | Hover and quick docs | Missing | B | U1 |
| 21 | Type providers (container `get()`, event dispatcher, repository, find, signatures: 7) | Return types for PHP inference | Missing | B | U4 |
| 22 | Parameter name hints for service arguments, inlay provider | Inlay hints | Missing | B | No inlay API |
| 23 | Folding (PHP route/translation strings, Twig) | Folding builders | Partial: Twig folds in `aritti.twig`; PHP folding missing | B | No folding control for PHP |
| 24 | Implicit usage provider | Suppress "unused" for services and controllers | Missing | B | No such hook |
| 25 | Multi-host injection (Twig and DQL inside PHP strings) | Language injection | Missing | B | Injection not available |
| 26 | Run configuration for commands, test line marker, Run Anything | Run `bin/console` commands | Missing | F* | `runDetectors` allow one program and fixed args; arbitrary command pick is U5 |
| 27 | Database integration (connections from config, entity-table navigation, `tables` extension) | Database tool | Missing | B | No database API |
| 28 | MCP tools, endpoints view, terminal command specs | AI and tooling hooks | Missing | B | None exposed |
| 29 | PHP attribute and annotation completion (PhpAnnotation* providers: `#[Route(` args, `@Template`, ORM attributes, docblock goto, attribute auto-popup) | Argument completion inside attributes | Missing | F* | Trigger on `(` needs U6 for auto-popup; manual invoke works |
| 30 | Settings (7 configurables: container files, routing files, Twig namespaces, ...) | Project settings | Partial: scalar settings only, lists as comma-separated strings (`settings.schema.json`) | F | Scalar types only |
| 31 | Status bar widget, file icons, icon provider, editor notification | UI niceties | Missing | F | Status items, fileIcons globs exist |

---

## Twig (7303, proprietary) -> `aritti.twig`

Original features: 13 live templates / 0 inspections / 12 grouped extension-point rows. Done 5, Partial 2, Missing 18 (13 templates + 5 rows). Gaps by class: F 0 / F* 16 / B 4.

Done: `.twig` detection and hand-written lexer highlighting (README), file icon (`plugin.toml` fileIcons), `{# #}` comment toggling (`twig.toml`), structure view (blocks, macros), folding of tag pairs and comments.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | Live templates (13): blk, mcr, esc, emb, ext, for, from, if, imp, inc, sbx, spl, verb | Expand a short abbreviation into a tag pair with tab stops | Missing; no live-template contribution exists | F* | Snippet completion items (`$1`, `$0` supported per SDK doc) with the abbreviation as label; a Twig-only provider means a hand-combined `register_plugin!` in this crate (LIMITATIONS "Twig needed its own language plugin"); not verified in the IDE |
| 2 | completion.contributor / languageSupport (tags, filters, functions, tests) | Completion of Twig names and template paths | Partial: lives in `aritti.symfony`, static lists, only in Symfony projects (`twig_names`, README) | F* | Move to or duplicate in `aritti.twig` (same hand-combine cost); non-Symfony projects get nothing today |
| 3 | lang.parserDefinition errors, highlightErrorFilter | Syntax errors in Twig source (unclosed tags, bad expressions) | Partial: `unclosed-tag` only (in `aritti.symfony`) | F* | Diagnostics from a lexer-level scan on open/save; expression errors need a real parser |
| 4 | lang.braceMatcher, typedHandler, backspaceHandlerDelegate, editorSmartKeysConfigurable | Pair matching, auto-close of `{%`, `{{`, quotes | Missing (LIMITATIONS "Twig editing behaviour") | B | Language format has no auto-pair/bracket rules (U6) |
| 5 | lang.formatter, codeStyleSettingsProvider, langCodeStyleSettingsProvider | Built-in formatter and style page | Missing | F* | `languages/twig.toml` has a formatter field for an external tool; none bundled, depends on a user-installed formatter |
| 6 | HTML as template data language, multiHostInjector, fileViewProviderFactory | HTML intelligence and validation inside Twig | Missing (the lexer only highlights HTML) | B | Injection not available |
| 7 | findUsagesProvider, renameHandler, elementManipulator (5), targetElementEvaluator, referenceContributor, elementDescriptionProvider | Find usages and rename of blocks, macros, variables, templates | Missing | B | U1 (navigation to templates exists in `aritti.symfony` only) |
| 8 | debug.templateLanguage (2), indexPatternBuilder, breadcrumbsInfoProvider, spellchecker dictionary, colorSettingsPage, liveTemplateContext | Debugger template mapping, TODO scan, breadcrumbs, colour page | Missing | B | No such APIs |

Ours beyond the original: Symfony-aware route, asset, template and translation checks in Twig files (from `aritti.symfony`).

---

## Blade (7569, proprietary) -> `aritti.blade`

Original features: 2 inspections / 12 grouped extension-point rows. Done 6, Partial 1, Missing 6, n/a 1. Gaps by class: F 1 / F* 1 / B 6.

Done: BladeControlDirectives = `unpaired-directive`; BladeUnpairedParenthesesInStringLiteral = `unpaired-parenthesis-in-string` (README, `checks.rs`); highlighting; structure view (`@section`, `@push`, ...); folding of about 35 directive pairs; `{{-- --}}` comments.

| # | Original feature | What it does | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | completion.contributor, weigher, htmlInText completion | Directive completion with arguments | Partial: names only, in `aritti.laravel` (README: "names only, no snippets") | F | Snippet completion items are in the SDK |
| 2 | typedHandler (2), enterHandlerDelegate, braceMatcher, completion.confidence | Auto-close `{{`, `@if` to `@endif` on Enter, pair matching, popup after `@` | Missing | B | U6 (trigger characters, no pair rules in the language format) |
| 3 | lang.formatter (2), codeStyleSettingsProvider | Formatter and style page | Missing | F* | External formatter hook only; no bundled tool |
| 4 | multiHostInjector, embeddedCssProvider, htmlScriptInjectionBlocker, templateLanguageHighlightingExtension, xml.xmlExtension, fileNSInfo | PHP, HTML, CSS and JS intelligence inside Blade | Missing (lexer approximates highlight; LIMITATIONS) | B | Injection not available |
| 5 | psi.referenceContributor (2), referencesSearch, elementManipulator (2), referenceScopeExtension | References and usages of views, sections | Missing here; navigation to view names is in `aritti.laravel` | B | U1 |
| 6 | lang.documentationProvider | Quick docs for directives | Missing | B | U1 |
| 7 | typeProvider3 | Variable types (`$loop`, `$slot`, `@inject`) | Missing | B | U4 |
| 8 | todoIndexer, indexPatternBuilder, breadcrumbsInfoProvider, php.debug.templateLanguage, php.debug.template.configurable | TODO scanning, breadcrumbs, debugger mapping | Missing | B | No such APIs |
| 9 | inspectionSuppressor (3), intentionActionFilter, InspectionExtension, annotator | Suppress PHP inspections in Blade, extra annotations | n/a | | We do not run PHP inspections |

Ours beyond the original: gutter links between views and layouts, section/stack markers, `@` completion for project directives (in `aritti.laravel`).

---

## Laravel Idea (13441, paid) -> `aritti.laravel` (+ `aritti.blade`)

Original features: 14 inspections / 15 intentions / 36 actions / 50 file templates / 20 grouped extension-point rows. Ours: 15 diagnostic codes, 4 code-action rewrites (+2 reverse), 16 templates, 10 artisan commands, 4 run detectors.

| Kind | Original | Done | Partial | Missing | n/a | F | F* | B |
|---|---|---|---|---|---|---|---|---|
| Inspections | 14 | 10 | 1 | 3 | 0 | 2 | 2 | 0 |
| Intentions | 15 | 4 | 0 | 11 | 0 | 6 | 4 | 1 |
| Actions | 36 | 3 | 7 | 26 | 0 | 0 | 7 | 26 |
| File templates | 50 | 15 | 0 | 32 | 3 | 28 | 4 | 0 |
| Extension-point rows | 20 | 2 | 5 | 13 | 0 | 2 | 4 | 12 |
| Total | 135 | 34 | 13 | 85 | 3 | 38 | 21 | 39 |

Done inspections: BladeUnknownComponent (`laravel.unknown-component`), BladeUnknownLivewireComponent (`laravel.unknown-livewire`), BladeUnknownLivewireDirective (`laravel.unknown-livewire-directive`), LaravelUnknownRouteName (`missing-route`, opt-in setting), LaravelUnknownView (`missing-view`), LaravelUnknownEloquentFactory (`laravel.unknown-factory`), LaravelUnknownInertiaPage (`laravel.unknown-inertia-page`), GuardedAttributeAssignment (`guarded-attribute-assignment`), UnscheduledPrunableModel (`unscheduled-prunable`), UnknownConfigKey (`missing-config`). Ours also reports `missing-translation`.

### Gap table

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | Inspection, LivewireComponentUnknownMethod | `wire:click` etc. naming a method the Livewire class lacks | Missing | F* | Component class from the Livewire index, methods by text; traits and parents give false positives |
| 2 | Inspection, LaravelRoutesInspection | Unknown, deprecated or abstract controller or method in route files; missing `resource` methods | Missing (`controllers.rs` indexes methods but nothing reports) | F* | Project controllers only; invokable, namespaced and vendor controllers must stay silent |
| 3 | Inspection, LaravelFunctionsInspection | `env()` called outside config files | Missing | F | Text call match by path (`config/` excluded) |
| 4 | Inspection, LaravelSimplifyOrderBy | `orderBy(date column, dir)` replaced by `latest`/`oldest` | Partial: `simplify-order-by` for `created_at` only (`eloquent_checks.rs:52`) | F | Extend column rule |
| 5 | Quick fixes "create missing component, view, factory, Livewire component" on Done inspections | Scaffolds the missing target | Missing | B | File creation and dialogs (U2, U5) |
| 6 | Intention, ConvertCssClassesToClassDirectiveCall, WrapCssClassesWithClassDirectiveCall (2) | `class="a b"` becomes `@class([...])` | Missing | F | Attribute text edit in Blade, caret range given |
| 7 | Intention, MakeAllArrayFromValidationString (1) | Convert every validation string in an array | Missing (single conversion done: `actions.rs:38`) | F | Same rewrite applied in a loop |
| 8 | Intention, ConvertAllControllerCalls (1) | Convert all `'C@a'` in the file | Missing (single conversion done: `actions.rs:82`); also `'Controller'` to `Controller::class` not covered | F | In-file edits |
| 9 | Intention, ConvertRuleCallFromStringToObject (1) | `'exists:users,id'` to `Rule::exists(...)` | Missing | F | Rule table, import insertion |
| 10 | Intention, ConvertRootFacadeCall (1) | `Route::` alias to the full facade name | Missing | F | Static alias table, import insertion |
| 11 | Intention, ConvertScopeCallToAttribute, ConvertAllScopeCalls (2) | `scopeActive()` to `#[Scope] active()` | Missing | F* | Class-level textual rewrite; callers not renamed (they use the short name), safe by design |
| 12 | Intention, MoveDependencyInjectionToConstructor (1) | Move a method argument to a promoted constructor property | Missing | F* | Edits two places in one class; constructor creation by text |
| 13 | Intention, ConvertFacadeToDependency (1) | Replace a facade call with an injected dependency | Missing | F* | Core facade map static; custom facades need types (U4) |
| 14 | Intention, PromoteRequestToFormRequest (1) | Create a Form Request and move validation into it | Missing | B | File creation (U2) |
| 15 | Action, GenerateTranslationFromBlade/FromPhp (2) | Extract a selected string into a lang key | Missing | F* | Selection range given; edits existing lang file |
| 16 | Action, CreateEloquentFieldsForValidation, EloquentAddAllFields (2) | Add rules or entries for all model fields | Missing | F* | Columns exist in our migration index; model of a request class by naming convention |
| 17 | Action, RemoveModelPhpDocs, FindUnusedViews (2) | Bulk strip phpDocs; list views never referenced | Missing | F* | Multi-file edits are allowed in code actions but starting them from a command is unverified; unused views as `unnecessary` diagnostics, dynamic view names give false positives |
| 18 | Action, GenerateNewClass (1) | Empty class anywhere | Missing | F* | New-menu template with a fixed namespace |
| 19 | Action, Generate ResourceController, Policy, Observer, Listener, DatabaseFactory, DatabaseClassFactory (6, Partial) | Model-aware generation (fields, relations, methods) | Partial: static `ResourceController`, `Policy`, `Observer`, `Listener`, `Factory` templates, no model data | B | Model-aware files need file creation from a command (U2) and a model picker (U5) |
| 20 | Action, Generate JsonResource, JsonResourceCollection, NovaResource, FilamentResource, FractalTransformer, LivewireForm (6) | As above, other targets | Missing | B | U2, U5 |
| 21 | Action, ExtractView, ExtractBladeComponent, CreateDtoFromRequest, OverrideVendorFileInProject, NewPivotTable (5) | Create a file from selection or vendor file | Missing | B | U2 |
| 22 | Action, NewEloquentModel, LaravelActionChooser (2) | Dialog-driven multi-file generation, popup | Missing | B | U2, U5 |
| 23 | Action, RunArtisanCommand (1) | Run any artisan command by name | Partial: 10 fixed commands (`plugin.toml` commands) | B (arbitrary name); F (more fixed commands) | Input dialog (U5); fixed list can grow |
| 24 | Action, GenerateHelperCode, HelperCodeParameters (2) | Generate stubs the original uses for its type inference | Missing | B | Only useful with type inference (U4) |
| 25 | Action, NewModelFromTable, NewModelFromTable.FromDBGrip, FromTableToEloquentModel, AddApplicationDatabaseConnections (4) | Database tool integration | Missing | B | No database API |
| 26 | File templates (32 real): Action, Channel, Class, Console Command, Class Factory, Cast, Scope, Exception, Http Exception, Feature Test, Unit Test, Mail, Rule, Validation Rule, Service Provider, Trait, View Composer, JSON Resource and Collection, Livewire, Volt, Nova, Dusk Component/Page/Test, React, Svelte, Vue pages | Plain New-menu files | Missing; Done are Controller, Resource Controller, Job, Event, Listener, Mailable, Middleware, Form Request, Policy, Enum, Seeder, Factory, Migration, Pest Feature, Pest Unit | 28 F | Static `fileTemplates` (extension and placeholders unverified) |
| 27 | File templates, Blade Class Component, Blade View Component, View, Markdown Mail (4) | Templates with a `blade.php` extension or a companion view | Missing | F* | Compound extension and second file; companion file is U2 |
| 28 | File templates, 2 includes and Generated Code (3) | Internal fragments of the original | n/a | | |
| 29 | Other: Eloquent attribute, relation and scope completion and type inference (typeProvider4 x10, 10 completion contributors) | Model-aware completion | Missing | B | U4 |
| 30 | Other: facades, container, helper code types | `app()` and facade resolution | Missing | B | U4 |
| 31 | Other: query-builder column completion | Columns from migrations | Partial (README "Columns": static call, chain from model, `DB::table`) | F* | Limits are the chain shapes without types |
| 32 | Other: Livewire and Blade component props, slots, attributes | Completion of `wire:` and `<x-comp :prop>` | Partial: component names only | F* | Textual `@props` and constructor scan; slot names |
| 33 | Other: line markers (11) | model/migration/factory/policy/observer links, route to controller, event to listener | Partial: view, controller to route, sections, stacks | F* | `line-markers`, one target each |
| 34 | Other: custom usage searchers (10), rename listeners, goto symbol (4), documentation | Usages, rename, symbol search, hover | Missing | B | U1 |
| 35 | Other: code vision (2), declarative inlays, folding builder, postfix templates, surround | Editor decorations | Missing | B | No such APIs |
| 36 | Other: multi-host injection (2), implicit usage (2), inspection suppressors (6) | SQL and PHP injection, unused suppression | Missing | B | Injection and suppression not available |
| 37 | Other: database meta source, connection provider, schema dump | Database tool | Missing | B | No database API |
| 38 | Other: MCP tools, LLM chat, microservice endpoints, Qodana, command specs | AI and tooling | Missing | B | None exposed |
| 39 | Other: settings (17 project, 6 application configurables) | Rich settings | Partial: scalar switches, extra view dirs, main locale (`settings.schema.json`) | F | Scalar types only |

Ours beyond the original: Ziggy `route()` completion in JS/TS/Vue, Vite entry and ability completion, tool window for routes/views/config/translations/middleware/env, Observer template, validation array-to-string intention (reverse direction), `missing-translation`.

---

## Laravel (free plugin `de.espend.idea.laravel` 0.15.4, MIT) -> `aritti.laravel`

Original features (from `plugin.xml`, source read): 17 / Done 11 / Partial 1 / Missing 5. Gaps: F 1 / F* 3 / B 2.

Done: controller completion and goto for strings and `[C::class, 'm']`, `Route::resource` references, route names from `->name()`/`'as'`, view completion and goto (including `includeWhen`/`includeFirst`, `each`), config key index and completion, translation keys (plus `@lang`), Blade extends/include/PHP-usage markers, section/yield/stack markers, Blade template-name completion, `@trans`, template-usage index, `Route::view`.

| # | Original feature (kind, name or group) | What it does (own words) | Ours (Partial/Missing + evidence) | Class | Basis |
|---|---|---|---|---|---|
| 1 | Config `providers` class-array completion (`ProviderGotoCompletion`) | Complete and open provider classes in `config/app.php` | Missing (README "Not ported") | F* | Project PSR-4 classes only; vendor providers not findable (U3) |
| 2 | Service container `DicCompletionRegistrar` (`app('x')`, `App::make`) | Complete container names and open their classes | Missing | F* | Static alias list of core bindings plus `bind('x'` scan; user bindings in providers are dynamic |
| 3 | Container type provider (`DicTypeProvider`) and `@inject` type provider (`BladeInjectTypeProvider`) | Return types for `app('x')` and injected variables | Missing | B | U4 |
| 4 | `@inject('x', Class)` class completion and goto | Class reference inside the directive | Missing | F* | Project classes by PSR-4 index |
| 5 | Extract partial view refactoring (`ExtractPartialViewAction`, dialog) | Move selected Blade into a new partial | Missing | B | File creation and dialog (U2, U5) |
| 6 | Template path configuration, `ide-blade.json` namespaces (`ViewCollector`, `JsonTemplatePaths`) | Extra view roots and namespaces | Partial: extra view directories setting; `ide-blade.json` not read | F | Read the JSON with `read_file` (explicit path); `find_files` would skip it if hidden |
| 7 | Auto-popup of completion in string arguments (`PhpParameterStringCompletionConfidence`) | Popup on typing a quote | Missing | B | U6 (trigger characters) |

Ours beyond the original: see Laravel Idea section (middleware, env, validation rules, abilities, columns, Livewire/Blade components, Vite, Inertia, Eloquent checks, artisan commands).

---

## Group summary

| Pair | Original features | Done | Partial | Missing | n/a | Gap F | Gap F* | Gap B |
|---|---|---|---|---|---|---|---|---|
| Symfony Plugin -> `symfony` | 197 (104 insp, 16 int, 30 act, 16 tpl, 31 other) | 48 | 8 | 139 | 2 | 48 | 72 | 27 |
| Twig -> `twig` | 25 (13 live tpl, 12 rows) | 5 | 2 | 18 | 0 | 0 | 16 | 4 |
| Blade -> `blade` | 14 (2 insp, 12 rows) | 6 | 1 | 6 | 1 | 1 | 1 | 6 |
| Laravel Idea -> `laravel` / `blade` | 135 (14 insp, 15 int, 36 act, 50 tpl, 20 other) | 34 | 13 | 85 | 3 | 38 | 21 | 39 |
| Free Laravel -> `laravel` | 17 | 11 | 1 | 5 | 0 | 1 | 3 | 2 |
| Total | 388 | 104 | 25 | 253 | 6 | 88 | 113 | 78 |

Notes: Twig gap rows count each of the 13 live templates; Blade `n/a` is the suppressor row. Gap totals differ from Missing plus Partial where one row spans several items (for example the Symfony other-extension rows count once each). Twig and Blade inventories list no inspections beyond the two Blade ones, so most of their gaps are editor behaviours that depend on U1, U4, U6 or injection.
