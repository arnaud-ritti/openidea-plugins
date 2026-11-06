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
    directory, with the same exceptions as in PHP. Both run on `.twig` files
    from their extension, so they work without the `aritti.twig` plugin.
- Quick fixes (⌥⏎): on a missing translation key, add it to one of the domain's
  YAML files; on an unquoted `@` / `%` YAML value, quote it.
- Gutter: controller `render()` → template; template → controllers rendering
  it; `{% block %}` → the parent block it overrides; PHP class → its service
  definition; YAML service → its class file.

## New file templates

New menu entries for a PHP class with a fixed `App\…` namespace: Command,
Controller, Form Type, Twig Extension, Compiler Pass, Kernel Test, Web Test,
Event Subscriber and Voter.

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
public directory and extra compiled container and route files.

## Not ported

These features rely on PhpStorm's PHP, Twig and YAML PSI and type inference,
which the OpenIDEA plugin API has no equivalent for:

- container `get()` return types, Twig variable types (`{{ post.| }}`),
  form types and options, Doctrine (QueryBuilder, repositories, DQL),
  serializer, event dispatcher, constraint validators;
- the remaining inspections (deprecated Twig, …) and intentions (they need resolved PHP types);
- refactorings, find usages, MCP tools, the profiler's per-request panels;
- a "create template" quick fix: plugin code actions can only edit existing
  files.

Matching is textual: a call is recognised by its method name, not its class,
and templates are looked up in `templates/` and the `twig: paths:` directories of
`config/packages/twig.yaml` (names starting with `@` are never reported missing).
