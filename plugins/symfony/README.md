# Symfony

Partial port of the [Symfony Plugin](https://github.com/Haehnchen/idea-php-symfony2-plugin)
for IntelliJ (`fr.adrienbrault.idea.symfony2plugin` 2026.2.314, revision
`b6c1034f`, MIT, by Daniel Espendiller and Adrien Brault). The port uses only
the free code of the public repository.

## Completion and go to definition (⌘B)

| Names        | Indexed from                                                                                                       | Referenced in                                                                                                                                                  |
| ------------ | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Routes       | `#[Route]` / `@Route` in `src/**/*Controller.php`, `config/routes*` YAML/XML, compiled `var/cache/dev/url_generating_routes.php` | PHP `generateUrl`, `redirectToRoute`, `->generate`; Twig `path()`, `url()`                                                                                     |
| Templates    | `templates/**/*.twig` (`templates/bundles/XBundle/` as `@X/`; `twig: paths:` of `config/packages/twig.yaml` as `@namespace/`)                                                       | PHP `render*`, `stream`, `htmlTemplate`, `textTemplate`, `#[Template]`; Twig `extends`, `include`, `embed`, `import`, `from`, `use`, `include()`, `source()` |
| Blocks       | the parent templates (`extends` chain)                                                                              | Twig `{% block name %}`, `block('name')`                                                                                                                       |
| Variables    | keys of the array passed to `render()` by controllers                                                              | Twig expressions (completion only)                                                                                                                             |
| Services     | `services:` in `config/**` YAML, `<service>` in XML, compiled `var/cache/dev/App_KernelDevDebugContainer.xml`      | PHP `$container->get/has`, `service()`, `ref()`; YAML `'@id'`, `'@?id'`; XML `type="service" id=`, `alias=`, `parent=`                                          |
| Classes      | `composer.json` PSR-4 autoload directories                                                                         | YAML service keys, `class:`, `parent:`; XML `class=`                                                                                                           |
| Translations | `translations/**` YAML (nested keys joined with `.`) and XLIFF, by domain                                          | PHP `->trans`, `t()`, `new TranslatableMessage`; Twig `'key'\|trans`; domain from the call, `{% trans_default_domain %}`, else `messages`                       |
| Assets       | files under `public/`                                                                                              | Twig `asset()`; PHP `->getUrl()`                                                                                                                               |
| Entries      | `webpack.config.js` `addEntry`/`addStyleEntry`, `importmap.php`                                                    | Twig `encore_entry_link_tags()`, `encore_entry_script_tags()`, `importmap()`                                                                                    |
| Roles        | `ROLE_*` in security config and controllers, plus built-in attributes (`IS_AUTHENTICATED_*`, `PUBLIC_ACCESS`, …)  | PHP `isGranted`, `denyAccessUnlessGranted`, `#[IsGranted]`; Twig `is_granted()`                                                                                |
| Tags         | Symfony's built-in service tags plus the tags used in `config/**` YAML and XML service files                      | YAML `tags:` of a service (`- { name: x }`, `- name: x`, `['x']`, `- x`); XML `<tag name="">` (completion only)                                              |

## Twig names and structure

In `.twig` files of a Symfony project: completion of tag names (`if`, `for`,
`block`, `extends`, … and their `end…` forms) after `{%`, filters after `|`,
functions in expressions and tests after `is` / `is not`, from static lists
taken from Twig's public documentation (not read from the project's Twig
extensions). Template paths after `extends`, `include`, `embed`, `import`,
`from` and `use` come from the cached `templates/` index.

## Problems, quick fixes and gutter

- Problems: missing template, missing translation key (in a domain that has
  keys), missing route (once the compiled routes exist), missing service (once
  the compiled container exists), route defined twice in a file, service id defined twice in an XML file, unquoted YAML values starting with `@` or `%` in `config/` and `translations/` (Symfony's parser rejects them), Twig block tags (`block`, `if`, `for`, `macro`,
  `embed`, `apply`, `autoescape`, `with`, `verbatim`, `cache`, `sandbox`) never closed. Files are
  checked when opened or saved. _Settings › Plugins › Symfony_ turns the
  checks off.
- Text inspections (`src/checks.rs`, written from the Symfony documentation;
  each is silent when the class, service or file it needs is not in the
  index, and when a name is dynamic or comes from a library). Three groups,
  each with a setting:
  - _Route controllers_ (`controllerChecks`): `controller-method-missing`, a
    route in `config/routes*` (YAML/XML) naming `Class::method` where the
    project class has no such public method, an invokable class without
    `__invoke`, or a class-level `#[Route]` with neither `__invoke` nor routed
    methods; `deprecated-route-setting`, only these: the
    `Bundle:Controller:action` notation (removed in 4.0), `pattern` (removed in
    3.0), `type: annotation` on imports (deprecated in 6.4; only reported when composer.json requires `symfony/framework-bundle` 6.4 or newer), and the
    `_method` / `_scheme` requirements (removed in 3.0). Classes that extend a
    class other than `AbstractController`, use traits or define `__call` are
    not reported.
  - _Services_ (`serviceChecks`, YAML/XML under `config/`):
    `deprecated-service-class`, a `class:` naming a project class with
    `@deprecated` or `#[Deprecated]`; `duplicate-parameter-key`, one
    parameter twice in a file; `named-argument-missing`, a `$name` argument the
    class constructor has no parameter for (not for variadic constructors,
    services with `parent` or a factory, `calls`, or classes without a
    constructor).
  - _PHP calls_ (`phpCallChecks`): `form-type-string`, `$builder->add('f',
    'text')` with a core type alias removed in Symfony 3; `missing-asset`,
    `->getUrl('x')` / `asset('x')` of a file absent from the public directory
    (not for URLs, absolute paths, dynamic names or a second package
    argument); `missing-translation-domain`, `->trans(…, domain)` / `t()` with a
    domain that has no file in `translations/` (only lower-case project-style
    domain names are reported: `messages`, `validators`, `security`, dotted or
    capitalised names such as `EasyAdminBundle` never are). Assets under
    `build/`, `bundles/`, `assets/` and in directories with no indexed file
    are not reported; `->getUrl()` is only checked in files that mention
    `Packages` or `Symfony\Component\Asset`. Class files are read at most once
    per run (256 KB and 500 files caps); beyond that the checks stay silent.
    A parent named `Controller` / `AbstractController` counts only when it
    resolves to the framework's. Services with `constructor`, `parent` or a
    factory are not checked for named arguments.
  - _Twig_ (`twigChecks`): `twig-missing-route`, `path('x')` / `url('x')` of a
    route absent from the compiled routes (silent without them, for
    `_`-prefixed routes and dynamic names; replaces `missing-route` in Twig
    files); `twig-missing-asset`, `asset('x')` of a file absent from the public
    directory, with the same exceptions as in PHP; `twig-route-compare`, a
    literal compared with `app.current_route` or
    `app.request.attributes.get('_route')` (`==`, `!=`, `is same as`, `in [...]`)
    that is not a compiled route; `twig-missing-translation-domain`, the domain
    of `'k'|trans(args, 'domain')` (or `domain:`) without translation files.
    They run on `.twig` files from their extension, so they work without the
    `aritti.twig` plugin. Comments and `{% verbatim %}` bodies are skipped.
  - _Twig syntax_ (`twigSyntaxChecks`): `twig-for-condition` (warning), the `if`
    condition of `{% for %}` (deprecated in Twig 2.10, removed in 3.0; use
    `|filter`); `twig-component-block-syntax` (error), a `{% block %}` inside a
    `<twig:Name>` element; `twig-component-self-import` (error), `_self` as
    the source of `{% from %}` / `{% import %}` inside a component. An
    unbalanced `<twig:...>` file is left alone.
  - _YAML syntax_ (`yamlSyntaxChecks`, files under `config/` and
    `translations/`): `unquoted-yaml-scalar` (also a value starting with a
    backtick, or with `|` or `>` that is no block scalar header),
    `yaml-unescaped-backslash` (hint), a backslash in a double-quoted string
    that starts no escape, and `yaml-unquoted-colon` (hint), a `: ` inside an
    unquoted value. Also in the _service_ group (`serviceChecks`):
    `deprecated-factory-setting`, `factory_class` / `factory_method` /
    `factory_service` (YAML) or `factory-class` / `factory-method` /
    `factory-service` (XML) on a service, removed in Symfony 3.0; use `factory`.
  - _Commands_ (`commandChecks`, PHP): `command-invokable-return-type` (hint),
    the `__invoke()` of a class with `#[AsCommand]` (the attribute must resolve
    to the Symfony class) has no `int` or `never` return type;
    `console-input-mode` (warning), two base modes ORed in the mode of
    `InputArgument` (`REQUIRED`, `OPTIONAL`) or `InputOption` (`VALUE_NONE`,
    `VALUE_REQUIRED`, `VALUE_OPTIONAL`), through `new` or `addArgument()` /
    `addOption()`; deprecated in Symfony 8.1. Modifiers, numbers and variables
    are ignored.
  - _Deprecations_ (`deprecationChecks`, PHP), only when `composer.json` names
    `symfony/framework-bundle` 8.1 or newer: `filesystem-mirror-copy-on-windows`,
    the `copy_on_windows` option of `->mirror()` in a file that mentions
    `Symfony\Component\Filesystem` (use `follow_symlinks`);
    `tagged-collection-default-method`, `defaultIndexMethod:` or
    `defaultPriorityMethod:` with a string value on `TaggedIterator`,
    `TaggedLocator`, `AutowireIterator` or `AutowireLocator` (use
    `#[AsTaggedItem]`); `from-callable-option`, `alias`, `parent`,
    `synthetic`, `file`, `arguments`, `properties`, `configurator` or `calls`
    next to `from_callable` in one PHP-array service (YAML is not checked).
  - _Doctrine_ (`doctrineChecks`, PHP attributes, `*.orm.xml`, `*.orm.yml|yaml`;
    written from the Doctrine ORM documentation, not from the Symfony Plugin's
    source). PHP files count only when they import `Doctrine\ORM\Mapping` (or
    a member of it, aliases followed): `orm-empty-table` (hint), `#[Table]`
    with no arguments or `name: ''`; `orm-table-deprecated-property`
    (warning), `indexes:` or `uniqueConstraints:` in `#[Table(...)]`
    (deprecated since ORM 3.2, use `#[Index]` / `#[UniqueConstraint]`);
    `orm-lifecycle-callbacks-missing` (warning), a lifecycle attribute
    (`PrePersist`, `PostPersist`, `PreUpdate`, `PostUpdate`, `PreRemove`,
    `PostRemove`, `PostLoad`, `PreFlush`) in an `#[Entity]` class
    without `#[HasLifecycleCallbacks]` (a `#[MappedSuperclass]` is skipped: a
    subclass may carry it);
    `orm-one-to-many-mapped-by` (warning), `#[OneToMany]` without `mappedBy`
    and without a `#[JoinTable]` on the property; `orm-inverse-side-join-column`
    (hint), `#[JoinColumn]` next to `#[OneToOne(mappedBy: ...)]`;
    `orm-join-column-nullable` (hint), an explicit `nullable: true` (the
    default) on a join column in PHP, XML (`<join-column>`) or YAML
    (`joinColumn` / `joinColumns`).
- Quick fixes (⌥⏎): on a missing translation key, add it to one of the domain's
  YAML files; on an unquoted `@` / `%` YAML value, quote it; on an unescaped backslash in a
  double-quoted YAML string, double it. Quick fixes follow the check settings.
- Gutter: controller `render()` → template; template → controllers rendering
  it; `{% block %}` → the parent block it overrides; PHP class → its service
  definition; YAML service → its class file.

### Code actions

Refactorings in the code actions menu (⌥⏎), from `src/intentions.rs`. Each
edits the open file only, and is not offered when a needed import would clash
with another class of the same short name.

- _Add #[Route] attribute_: caret in a public, non-static, non-magic method
  without `#[Route]` of a class named `*Controller` or extending
  `AbstractController` / `Controller`, when `composer.json` requires
  `symfony/framework-bundle` 6.2 or newer. Path and name come from the class
  and method names (`/blog/show-post`, `app_blog_show_post`); the caret is not
  moved to the path.
- _Add parameter: Request $request_ / _UserInterface $user_: in a public route
  action (`#[Route]`, `@Route`, or `__invoke` of a class with a `#[Route]`).
  In the `__invoke` of an `#[AsCommand]` class: `InputInterface`,
  `OutputInterface`, `Cursor`, `SymfonyStyle`, `Application`. One action per
  type (no chooser); a type already in the signature, by short or full name, is
  not offered.
- _Use TextType::class_: on the second argument of `->add('field', 'text')`
  when it is a pre-3.0 form type alias; _Use class constants for all form types
  in this file_ when the file has two or more.
- _Override block "name"_: with the caret on `{% extends %}`, one action per
  parent block the file does not define; appends a block calling `parent()`
  at the end of the file.

## New file templates

New menu entries for a PHP class with a fixed `App\…` namespace: Command,
Controller, Form Type, Twig Extension, Compiler Pass, Kernel Test, Web Test,
Event Subscriber and Voter.

Three more create the default `config/services` file of the Symfony recipe
(`_defaults` with autowire and autoconfigure, the `App\` resource): Service
Config (YAML), (XML) and (PHP).

## File icons and status item

`services.yaml`, `services.yml`, `routes.yaml`, `routes.yml`, `bundles.php` and
`symfony.lock` get a small "S" icon (drawn for this plugin, not the Symfony
logo). The status bar shows `Symfony 7.1` (the `symfony/framework-bundle`
constraint; just `Symfony` without one) with the route, service and template
counts as tooltip; clicking it reindexes. It disappears when the project is
not a Symfony project.

## Tool window and commands

The _Symfony_ tool window lists routes, templates, services and translations
by domain; double-click opens the definition. _Symfony: Warm Up Cache_ and
_Symfony: Clear Cache_ run `php bin/console cache:warmup` or `cache:clear`
(optional `process:php` capability, trusted projects only), then reindex,
because the compiled files hold the routes and services of bundles.
_Symfony: Reindex Project_ rebuilds the index. Saving a file under `config/`,
`templates/` or `translations/`, a PHP file under `src/`, `composer.json`,
`webpack.config.js` or `importmap.php` also rebuilds it.

The _Symfony Profiler_ tool window (command _Symfony: Show Profiler_) lists the
latest 100 requests, newest first, from `var/cache/dev/profiler/index.csv`
(time in UTC, method, status, URL, IP, token); sub-requests are left out, rows
cannot be opened.

A project counts as Symfony when it has `symfony.lock` or requires
`symfony/framework-bundle` or `symfony/symfony` in `composer.json`. Twig files
need the `aritti.twig` plugin (the IDE has no Twig language). Settings: the
public directory, extra compiled container and route files, and extra Twig
namespaces (`twigNamespaces`, comma-separated `name=directory` pairs, an empty
name is the main namespace; added to `twig: paths:`).

## Not ported

These features rely on PhpStorm's PHP, Twig and YAML PSI and type inference,
which the OpenIDEA plugin API has no equivalent for:

- container `get()` return types, Twig variable types (`{{ post.| }}`),
  form types and options, Doctrine (QueryBuilder, repositories, DQL),
  serializer, event dispatcher, constraint validators;
- the remaining inspections and intentions (they need resolved PHP types);
- refactorings, find usages, MCP tools, the profiler's per-request panels;
- a "create template" quick fix: plugin code actions can only edit existing
  files.

Matching is textual: a call is recognised by its method name, not its class,
and templates are looked up in `templates/` and the `twig: paths:` directories of
`config/packages/twig.yaml` (names starting with `@` are never reported missing).
