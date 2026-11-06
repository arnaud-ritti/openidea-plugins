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
- Quick fixes (⌥⏎): on a missing translation key, add it to one of the domain's
  YAML files; on an unquoted `@` / `%` YAML value, quote it.
- Gutter: controller `render()` → template; template → controllers rendering
  it; `{% block %}` → the parent block it overrides; PHP class → its service
  definition; YAML service → its class file.

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
- the remaining inspections (service arguments, deprecated Twig, …) and intentions (they need resolved PHP types);
- refactorings, find usages, MCP tools, the profiler's per-request panels;
- a "create template" quick fix: plugin code actions can only edit existing
  files.

Matching is textual: a call is recognised by its method name, not its class,
and templates are looked up in `templates/` and the `twig: paths:` directories of
`config/packages/twig.yaml` (names starting with `@` are never reported missing).
