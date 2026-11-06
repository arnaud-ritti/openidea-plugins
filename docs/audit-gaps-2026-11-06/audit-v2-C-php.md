# Audit v2, group C (PHP): symfony, twig, blade, laravel

Re-audit of every row of [C-php.md](C-php.md) against HEAD (`ac0edf6`), by grep of `plugins/{symfony,twig,blade,laravel}/src` and `plugin.toml`, not the READMEs. Paths are relative to `plugins/<name>/src/`. Status: Done / Partial / Missing. Class for what is not Done, under API 1.2 (pick, input, confirm and pick_many from commands only; set_context; selected_paths; viewItem menus; reveal_view; documentIndexers): F feasible, F* feasible with a risk, B blocked. Under 1.2, U5 (dialogs) is resolved for commands but not for code actions; U2 (file creation) is still open, so generators go through `php artisan` in a terminal; U1 (rename, usages, hover) and U4 (types) are still open. Document indexers give go-to-declaration and gutter markers, not usages. Nothing here is verified in the IDE (mock host only). `sym` = symfony.

## Symfony inspections (C-php rows 1 to 27)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | TwigRouteCompare | Done | `twig-route-compare` twig_checks.rs:171 | - | |
| 2 | TwigPathReplacement | Partial | `twig-path-replacement` twig_checks.rs:224, fix `Replace with` lib.rs:940 | F* | exact `href`/`action` equal to a placeholder-free route path only |
| 3 | Service instance (Yaml/Xml/Php) | Partial | `service-argument-type` wiring.rs:394, YAML and XML | F* | PHP variant missing; project hierarchy only |
| 4 | YamlParameter | Done | `unknown-parameter` config_checks.rs:104 | - | silent without compiled container |
| 5 | YamlClass | Done | `service-class-missing` checks.rs:527 | - | project PSR-4 namespaces only |
| 6 | Deprecated class from PHP | Done | `deprecated-class-usage` checks.rs:689 | - | `new X` / `X::class` of project classes |
| 7 | TaggedExtendsInterface | Done | `service-tag-interface` wiring.rs:410 | - | 7 core tags |
| 8 | ServiceArgument, EventMethodCall, ContainerConstant | Partial | `service-argument-count` wiring.rs:377, `listener-method-missing` :420, `yaml-php-const-missing` :473 | F | constant check is YAML only; XML `type="constant"` missing |
| 9 | ContainerSettingDeprecated | Done | `deprecated-factory-setting` config_checks.rs:12 | - | |
| 10 | Controller / constructor practice (13) | Partial | only `constructor-container-injection` php_checks.rs:453 maps loosely; `controller-container-get` :420, `controller-superglobal` :431, `controller-debug-call` :438, `controller-base-class` :414 are other checks | F* | none of RequestStack, Session, token, user, heavy constructor, `project_dir`, first-class callable, missing `throw` exists; SY2 is thin on this row |
| 11 | AutowireWrongClass | Missing | no code | F* | B before; the project class model (wiring.rs:290 `model_of`) now exists, project types only |
| 12 | TwigForCondition, ComponentMixedSyntax, SelfMacroImport, AssetsTag | Done | `twig-for-condition` twig_checks.rs:448, `twig-component-block-syntax` :541, `twig-component-self-import` :550, `twig-assets-tag-missing` :256 | - | "mixed syntax" is approximated as a block inside `<twig:X>` |
| 13 | WithExtendsRootHtml, TwigEnumFunction | Done | `twig-content-outside-block` :330/:339, `twig-enum-class` :290/:297 | - | |
| 14 | TwigVariablePath/Deprecated/ExtensionDeprecated | Missing | no code | B | U4; vendor extensions |
| 15 | FormTypeBuilderInstance, FormViewTemplate, ConstraintDeprecatedArray | Missing | no `createView`/Constraint-array code; `form-removed-option` php_checks.rs:492 is a different check | F* | |
| 16 | TwigTranslationDomain | Done | `twig-missing-translation-domain` twig_checks.rs:190 | - | |
| 17 | Doctrine mapping checks (11) | Partial | 8 of 11: `orm-empty-table` doctrine.rs:180, `orm-table-deprecated-property` :191, `orm-lifecycle-callbacks-missing` :279, `orm-one-to-many-mapped-by` :234, `orm-inverse-side-join-column` :247, `orm-join-column-nullable` PHP :261, XML :300, YAML :326 | F | missing: LifecycleMethodVisibility, DiscriminatorMapDuplicate Php/Xml |
| 18 | Doctrine QueryBuilder / Collections (9) | Partial | 1 of 9: `collections-criteria-ordering` doctrine.rs:136; extras `orm-query-iterate` :97, `orm-entity-manager-merge` :113, `orm-flush-entity` :114, `orm-column-deprecated-type` :82 are not in the original list | F* | no setParameters, clearParameter, join WITH, sort direction, `CURRENT_DATE`; SY5 is only partial |
| 19 | DoctrineMappingArrayAccess | Missing | no code | B | U4 |
| 20 | RouteAttributeNamespaceDeprecated | Missing | no code | F | import line match; Symfony 6.4 UPGRADE is a public basis, the "no public basis" ruling in backlog-php is wrong |
| 21 | CommandInvokableReturnType, ConsoleInputMode | Done | `command-invokable-return-type` php_checks.rs:185, `console-input-mode` :249 | - | |
| 22 | CommandStatic, RegisterCommands, InvokableReturnValue | Partial | `command-default-name` php_checks.rs:355 | F* | `registerCommands`, invokable return value missing |
| 23 | FilesystemMirror, TaggedCollection, FromCallable | Done | php_checks.rs:512, :551, :573 | - | `from_callable` PHP arrays only, YAML unchecked |
| 24 | HttpFoundationPropertyWrite, HttpKernelEventArgument, FindByCodes | Missing | `http-kernel-event-argument` php_checks.rs:327 is the 4.3 `GetResponseEvent` rename, not the 8.1 grouped-attributes check | F* | the three originals have no code |
| 25 | YamlQuotedEscaped | Done | `unquoted-yaml-scalar` config_checks.rs:142 (also backtick, pipe, `>`), `yaml-unescaped-backslash` :147 | - | |
| 26 | YamlUnquotedColon | Done | `yaml-unquoted-colon` config_checks.rs:152 | - | |
| 27 | Quick fixes on Done checks | Partial | `Change to "x"` typo fix lib.rs:1169; add key lib.rs:961; quote lib.rs:897 | B | "create template" still B (U2); typo fix is Done |

## Symfony intentions (12), actions (9), templates (2)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| I1 | TwigTranslationKeyIntention | Done | lib.rs:961 | - | |
| I2 | AddRouteAttribute | Done | `Add #[Route] attribute` intentions.rs:309 | - | |
| I3 | RouteActionParameter, CommandInvokeParameter | Done | `Add parameter: ...` intentions.rs:394 | - | one action per type, no chooser |
| I4 | FormStringToClassConstant | Done | `Use TextType::class` intentions.rs:419, all-in-file :448 | - | |
| I5 | PhpService, PhpServiceArgument | Done | `Add service definition` lib.rs:1125, `Resync arguments` :1096 | - | YAML for resync; appends to an existing file |
| I6 | Xml/YamlServiceTag | Partial | `Add tags ...` lib.rs:1100 | F* | YAML only; XML missing |
| I7 | DoctrineRepositoryClassConstant | Done | `Use X::class` rewrites.rs:77 | - | |
| I8 | CommandToInvokable | Done | rewrites.rs:357 | - | narrow `configure()` only |
| I9 | TwigExtensionToAttribute | Done | rewrites.rs:452 | - | |
| I10, I11 | XmlServiceArgument/Suggest; PhpPropertyArgument | Missing | no code | F* | B before; project class model (wiring.rs:290); code actions cannot `pick` |
| I12 | PhpBundleCompilerPass | Missing | no code | B | U2 |
| A1 | SymfonyNew Yaml/Xml/Php Service | Done | fileTemplates services-yaml/xml/php plugin.toml | - | |
| A2 | FormTypeMigrationGenerator | Done | intentions.rs:448 | - | |
| A3 | TwigBlockOverwrite | Done | `Override block` intentions.rs:492 | - | |
| A4 | ServiceGenerator, ServiceArgumentGenerator | Done | same as I5 | - | |
| A5 | TwigTranslationGenerator | Done | `Extract translation` lib.rs:1046 | - | plain-text selection only |
| A6 | TwigFormField, FormBuilderField generators | Partial | `Add form fields for X` generators.rs:198 | F* | PHP form type only; no Twig side |
| A7, A9 | PhpCompilerPassGenerate; database actions | Missing | New-menu template only (CompilerPass.php); no DB code | B | U2, no DB API |
| A8 | TwigExtendsGenerator | Done | command `twig-extends`, `host::pick` lib.rs:731 | - | needs `editor.write` |
| T1 | File templates, 4 command variants, annotations controller, twig function attribute | Done | plugin.toml: command, command-invokable, -with-input, -with-attributes, controller-annotations, twig-extension-attributes | - | all F rows of the old list built; placeholder expansion unverified |
| T2 | container.php/xml/yml | Done | templates ServicesPhp.php, ServicesXml.xml, ServicesYaml.yaml | - | |

## Symfony other extension points (31)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Goto/completion registrars | Done | README table, refs.rs | - | |
| 2 | `%param%` | Done | `param#` indexer.rs | - | |
| 3 | Event names, listeners | Partial | `event#`, `event-dispatch#` indexer.rs | F* | links listener to dispatch; no event-name completion |
| 4 | Tag names | Done | lib.rs completion tests (`completes_core_and_project_tags_in_yaml_service_tags`) | - | |
| 5, 6, 13 | Form types and options; Doctrine awareness (DQL, repositories); typed Twig members | Missing | no code (mapping checks are row 17) | B | U3/U4, DQL injection |
| 7 | Console command names | Done | `command#`, commands `go-to-command`, `run-console-command` plugin.toml | - | |
| 8 | Validator messages to translation keys | Missing | `validator-of#` indexer.rs:426 links constraint to validator, not messages | F* | |
| 9 | Voters / IsGranted | Partial | `voter#` indexer.rs:393; roles | F* | no expression completion |
| 10 | Twig filters/functions/tests | Partial | `twig-filter#` etc. indexer.rs (project extensions), static lists twig.rs:20 | F* | vendor extensions still static; README claims "not read from project extensions" is stale |
| 11 | Twig macros | Partial | `macro#` indexer.rs, local macro args twig.rs | F* | imported macro completion not shown |
| 12 | Twig variable collectors | Done | twig.rs:129 `local_variables`, :199 `includes`, README | - | untyped |
| 14 | UX components, Stimulus, Vite | Partial | `twig-component#`, `stimulus#` indexer.rs | F* | Vite missing |
| 15, 16 | Line markers (16); go-to-related | Partial | markers `template#`, `override#`, `route-use#`, `class-service#` indexer.rs:700-840, multi-target chooser | F* | 5 kinds; no Doctrine, form, serializer, test markers; no related popup |
| 17 | Goto symbol / file | Partial | commands `go-to-route/-service/-command/-template` using `host::pick` lib.rs:705 | B | Search Everywhere contributor still absent (U1) |
| 18-22 | Find usages; move/rename; docs/hover; type providers; parameter inlays | Missing | no code (`route-use#` marker is navigation only) | B | U1, U4, no inlay API |
| 23 | Folding | Partial | Twig folds in aritti.twig; PHP none | B | no PHP folding control |
| 24, 25, 27, 28 | Implicit usage; injection; database; MCP/endpoints | Missing | no code; `viewItem.context` menus now exist but are unused | B | |
| 26 | Run configuration / commands | Partial | `run-console-command` (pick + input, lib.rs:677) runs in terminal | F* | no run config, no test marker |
| 29 | Attribute/annotation completion | Partial | attr_args.rs (`#[Route(`, `#[Template(`, `#[ORM\`...) | F* | no `@Template` docblock, no auto-popup (U6) |
| 30 | Settings | Done | settings.schema.json: extraServiceFiles, extraRoutingFiles, twigNamespaces, compiledContainers | - | lists as comma strings |
| 31 | Status bar, icons | Done | `update_status_item` lib.rs:560, fileIcons plugin.toml:210 | - | no editor notification; plugin `icon` key not set (icon.svg unused) |

## Twig (8 rows; old C-php Twig table)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Live templates (13) | Done | all 13 in complete.rs:35-47, snippet items | - | |
| 2 | Name completion | Done | twig complete.rs; yields to aritti.symfony in Symfony projects | - | static lists |
| 3 | Syntax errors | Partial | `unclosed-tag`, `unexpected-end-tag`, `unterminated-delimiter` twig/checks.rs:5-7, published on open/save lib.rs:26 | F* | no expression errors; moved out of symfony |
| 4, 6, 7, 8 | Brace match/auto-close; HTML data language and injection; usages and rename; debugger, breadcrumbs, colour page | Missing | no code | B | U6, injection, U1 |
| 5 | Formatter | Missing | none; change-notes say OpenIDEA only drives Prettier/Ruff | B | was F*, TW3 skipped |

## Blade (9 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Directive completion | Done | laravel directives.rs (names and snippets, ~45), blade README | - | lives in aritti.laravel, so absent without it |
| 2, 4-8 | Auto-close/Enter; injection; references and usages of views; quick docs; variable types; TODO, breadcrumbs, debugger | Missing | no code (view navigation lives in aritti.laravel) | B | U6, U1, U4, injection |
| 3 | Formatter | Missing | none | B | was F*, BL3 skipped |
| 9 | Suppressors | n/a | | | |

## Laravel Idea (39 gap rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | LivewireComponentUnknownMethod | Done | `laravel.unknown-livewire-method` lib.rs:355 | - | setting `reportUnknownLivewireMethods` |
| 2 | LaravelRoutesInspection | Partial | `laravel.unknown-route-controller` lib.rs:395, `unknown-route-method` :404 | F* | no abstract/deprecated, no missing `resource` methods |
| 3 | env() outside config | Done | `env-outside-config` function_checks.rs:57 | - | |
| 4 | SimplifyOrderBy | Done | `simplify-order-by` eloquent_checks.rs:77, any `_at`/`_date` column | - | |
| 5 | Create missing X quick fixes | Partial | command `create-missing` generate.rs:316 (`confirm` then artisan) | F* | command not quick fix; view, component, controller only |
| 6 | CSS classes to @class | Done | actions.rs:236 area, README | - | |
| 7 | Make-all validation arrays | Done | actions.rs:148 | - | |
| 8 | Convert all controller calls | Done | actions.rs:175, :193 | - | |
| 9 | Rule string to object | Done | actions.rs:215-236 (`Rule::exists/unique/in/notIn`) | - | |
| 10 | Root facade import | Done | `Import {fqcn}` actions.rs:288 | - | |
| 11 | Scope call to attribute | Partial | `Use #[Scope]` class_actions.rs:201 | F* | per method; no convert-all |
| 12 | Move DI to constructor | Done | class_actions.rs:251 | - | |
| 13 | Facade to dependency | Done | class_actions.rs:309 | - | core facades |
| 14, 21 | PromoteRequestToFormRequest; Extract view/component, Dto, override vendor, pivot | Missing | no code | B | U2; artisan cannot move code |
| 15 | Extract string to lang key | Done | lang_actions.rs:110 (Blade and PHP) | - | |
| 16 | Eloquent fields for validation | Done | lang_actions.rs:217, :242 | - | multi-line or empty array only |
| 17 | RemoveModelPhpDocs, FindUnusedViews | Partial | lib.rs:966, command `unused-views` lib.rs:669 | F* | per file, not bulk; backlog said "not verified", it exists |
| 18 | GenerateNewClass | Done | generator `class` generate.rs:71 | - | via artisan |
| 19 | Generate Resource/Policy/Observer/Listener/Factory | Partial | `generate-for-model` generate.rs:253 (`--model`) | F* | no field/relation-aware bodies |
| 20 | Generate JsonResource/Nova/Filament/Fractal/LivewireForm | Partial | make:resource, livewire generators generate.rs | F* | Nova, Filament, Fractal, LivewireForm missing |
| 22 | NewEloquentModel, ActionChooser | Done | `new-model` lib.rs:560, `make` pick generate.rs:213 | - | terminal output |
| 23 | RunArtisanCommand | Done | `artisan-run` pick + input generate.rs:154-159 | - | |
| 24, 25 | Helper code; database actions | Missing | no code | B | U4, no DB API |
| 26 | File templates (32) | Partial | 27 templates in plugin.toml (16 old + Action, Cast, Scope, Notification, Rule, Trait, JsonResource, ServiceProvider, ViewComposer, Exception, Livewire) | F | still absent: Channel, Console Command, Class Factory, Http Exception, PHPUnit tests, Resource Collection, Volt, Nova, Dusk, React, Svelte, Vue |
| 27 | Blade templates (4) | Missing | no code | F* | companion file is U2 |
| 28 | Internal fragments | n/a | | | |
| 29, 30 | Model completion and inference; facade/container types | Missing | no code | B | U4 |
| 31 | Query-builder columns | Partial | README 153: relations, scopes, `Model::query()` chains; php.rs | F* | variable of unknown type loses the model |
| 32 | Component props, slots | Done | components.rs (`@props`, constructor, `<x-slot:`, `livewire_props` :165) | - | |
| 33 | Line markers (11) | Partial | indexer.rs:114 (model, migration, factory, policy, observer, seeder, event, listener; counterpart must exist) + provider markers | F* | no route-to-controller marker from indexer |
| 34-38 | Usages, rename, goto symbol, docs; code vision, inlays, postfix; injection, suppressors; database; MCP/LLM | Missing | no code | B | U1 and missing APIs |
| 39 | Settings | Partial | settings.schema.json (controllersNamespace, viewPaths, locale, ~8 report toggles) | F | scalar only |

## Free Laravel plugin (7 rows)

| # | feature | status | evidence | class | note |
|---|---|---|---|---|---|
| 1 | Config `providers` completion | Done | lib.rs:767-804 (`config/app.php`, `bootstrap/providers.php`) | - | project `app/Providers` only; backlog said "not verified" |
| 2 | Container `app('x')` | Done | refs.rs:564, index.rs:370/708 | - | core aliases plus literal `bind(` |
| 3, 5, 7 | Container and `@inject` types; Extract partial view; auto-popup in strings | Missing | no code | B | U4, U2, U6 |
| 4 | `@inject` class | Done | refs.rs:565 | - | |
| 6 | `ide-blade.json` | Done | index.rs:201-205 | - | two fixed paths only |

## Totals

Row ids are the C-php ids (merged ids such as 18-22 count per id). Symfony 27 + 12 + 9 + 2 + 31 = 81, Twig 8, Blade 9, Laravel Idea 39, Free Laravel 7 = 144 rows (old file: 388 original features, so row and feature counts differ).

| Section | Rows | Done | Partial | Missing | n/a | F | F* | B |
|---|---|---|---|---|---|---|---|---|
| Symfony inspections | 27 | 13 | 8 | 6 | 0 | 3 | 8 | 3 |
| Symfony intentions, actions, templates | 23 | 16 | 2 | 5 | 0 | 0 | 4 | 3 |
| Symfony other extension points | 31 | 7 | 11 | 13 | 0 | 0 | 10 | 14 |
| Twig | 8 | 2 | 1 | 5 | 0 | 0 | 1 | 5 |
| Blade | 9 | 1 | 0 | 7 | 1 | 0 | 0 | 7 |
| Laravel Idea | 39 | 16 | 10 | 12 | 1 | 2 | 9 | 11 |
| Free Laravel | 7 | 4 | 0 | 3 | 0 | 0 | 0 | 3 |
| Total | 144 | 59 | 32 | 51 | 2 | 5 | 32 | 46 |

The old file counted features (F 88 / F* 113 / B 78); this file counts rows, so only the shift is comparable. B shrank because dialogs (U5) exist for commands and artisan covers file creation for `make:*`. What remains is U1, U4, U6, U2 outside artisan, injection and formatters.

## Rows that changed vs the old audit

- Missing to Done: Symfony inspections 1, 4, 5, 6, 7, 9, 12, 13, 16, 21, 23, 25, 26; intentions 2 to 5, 7 to 9; actions 1 to 5, 8; both template rows; other rows 2, 4, 7, 12, 30. Twig 1 to 2. Laravel Idea 1, 3, 6 to 10, 12, 13, 15, 16, 18, 22, 23, 32. Free Laravel 1, 2, 4, 6.
- Missing to Partial: Symfony 2, 3, 8, 10, 17, 18, 22, 27; I6; A6; other 3, 9, 10, 11, 14, 17, 23, 26, 29. Twig 3 (moved to aritti.twig). Laravel Idea 2, 5, 11, 17, 19, 20, 26, 33.
- Class changes: Twig 5 and Blade 3 (formatter) F* to B; Symfony 11, I10, I11 B to F* (project class model); Laravel Idea 5, 19 to 23 B to F or done (artisan plus `pick`/`input`/`confirm`); Symfony other 17 B to Partial.
- No row the old audit called Done has regressed. The old "Done" Symfony inspection rows were checked by code: all still exist.

## Claims the code does not back

- sym README "Twig names ... not read from the project's Twig extensions" is stale: `twig-filter#/function#/test#` exist (indexer.rs). The sym change-notes say Twig variables "from `app`" and similar: fine. `twig.rs:1` header is also stale.
- backlog-php Status is stale in three places: LA6 "counterpart check skipped" (it exists: indexer.rs `exists`), LA5 "RemoveModelPhpDocs not verified" (lib.rs:966), LA10 "providers not verified" (lib.rs:767).
- backlog SY2 DONE-list names in README "practice hints" but only 5 of the ~20 originals in its scope exist; 12 of the 13 named constructor/controller checks, `RegisterCommands`, `InvokableReturnValue`, `HttpFoundationPropertyWrite`, `FindByCodes` have no code.
- backlog SY5 ("Doctrine QueryBuilder deprecations") is not done beyond Criteria ordering; the extras (`orm-query-iterate`, `-merge`, `-flush-entity`) were not in the original list.
- sym `http-kernel-event-argument` shares a name with the original but is a different (4.3 rename) check.
- Plugin `icon` key (API 1.2) is set only in blade; symfony, twig and laravel ship `icon.svg` but do not declare it. `viewItem.context`, `set_context`, `selected_paths`, `reveal_view` are used by none of the four plugins.

## Best next 10 rows

1. Symfony 10 (practice: RequestStack, Session, token, user, `project_dir`, heavy constructors, missing `throw`, first-class callable): F*, largest block of untouched inspections, public docs.
2. Symfony 18 (QueryBuilder deprecations, 8 left): F*, gate by `Doctrine\ORM\QueryBuilder` import.
3. Symfony 20 RouteAttributeNamespaceDeprecated: F, one import-line check.
4. Symfony 24 and 22 (`registerCommands`, invokable return value, request attribute write, `findByCodes`): F*.
5. Symfony 15 (FormViewTemplate, FormTypeBuilderInstance, Constraint array): F*.
6. Symfony 17 remainder (LifecycleMethodVisibility, DiscriminatorMapDuplicate Php/Xml): F.
7. Laravel Idea 26 (remaining ~15 plain templates: Channel, Console Command, Http Exception, Class Factory, PHPUnit tests, Resource Collection): F, static files.
8. Laravel Idea 2 and 11 (missing `resource` methods; convert-all scope calls): F*.
9. Symfony I10, I11, I6 (suggest service ids from constructor types, property injection, XML tags) on top of `wiring.rs` `model_of`: F*.
10. Housekeeping: declare `icon` in symfony, twig and laravel; fix the three stale statements above; add `viewItem.context` menus to the Symfony and Laravel tool windows (jump, reindex) and `reveal_view` for `show-profiler`.
