# Limitations of the OpenIDEA plugin API

What ported plugins can't do because OpenIDEA (plugin API 1.1, `l0ng-ai/openidea`
v0.1.37) has no API for it, and what such an API could look like. One section
per port; append new findings to the matching section or add one. Upstream
proposals go to `l0ng-ai/openidea` as their own pull requests.

## Symfony (`aritti.symfony`) and Twig (`aritti.twig`)

Port of `fr.adrienbrault.idea.symfony2plugin` 2026.2.314. Checked against the
SDK, WIT and host source of v0.1.37 on 2026-11-05.

### No access to the semantic model of built-in languages

The plugin API gives a provider the document text only. Plugins can't query
PHP's syntax tree, symbol index or language server (intelephense/phpactor),
and they can't add to it.

- **Blocks:**
  - container `get('id')` return types;
  - PHP type providers;
  - Twig variable members (`{{ post.| }}` needs the properties and methods of the PHP class);
  - form types and their options (`AsFormType`, `FormField`);
  - Doctrine (entity fields, repositories, QueryBuilder, DQL);
  - serializer groups;
  - event listener methods;
  - constraint validators;
  - `!php/enum` and `!php/const` in YAML;
  - every inspection that needs a resolved PHP type: service arguments, service instances, `EventMethodCall`, `ContainerConstant`, `FormTypeAsClassConstant`, command return types;
  - 16 intentions.
- **Why:** `providers.wit` passes `document {path, language, version, text}` and nothing else. The host's own index (`crates/ide-index`) and LSP client (`crates/ide-lsp`) aren't exposed through WIT. A plugin's completion items are only added to the language server's list; a plugin can't change the types the server infers.
- **Workaround in the port:** string literals are matched with text patterns. A call is recognised by its method name (`->generate(`), not by the class it's called on, so a `->generate('…')` on an unrelated object also gets route names.
- **Would need:** a host function that forwards LSP requests (`definition`, `hover`, `workspace/symbol`, `typeDefinition`) to the language's server. Or a symbol query on the IDE index (`classes`, `members_of(fqcn)`, `return_type(fqcn, method)`).

### No references, rename or hover providers

The provider kinds are `completion`, `inline-completion`, `code-actions`, `definition` and `line-markers`.

- **Blocks:**
  - Find Usages of routes, templates, services, translation keys and `AsFormType`;
  - renaming a route, service id, translation key or block everywhere it's used;
  - the original's documentation targets (a service's class and arguments, a route's path and controller on hover).
- **Why:** `providers.wit` has no `references`, `rename` or `hover` export. A completion item's `documentation` only shows in the completion list.
- **Would need:** new provider kinds `references` → `list<location>`, `rename` → workspace edits, and `hover` → markdown.

### Code actions can't create, rename or delete files

- **Blocks:**
  - the "Create template" quick fix on a missing template (`PhpTemplateMissingInspection`, `TwigTemplateMissingInspection`, `TemplateMissingAnnotationPhpAttributeLocalInspection`);
  - creating a translation file for a domain that has none;
  - generating services, controllers or form types into new files.
- **Why:** plugin `code-action.edits` are `(path, list<text-edit>)`, which the host turns into `FileEdit::Edit` only (`crates/ide-workspace/src/documents/workspace_edit.rs`). The host's `WorkspaceEdit` supports resource operations, but the plugin API doesn't expose them, and there's no other host function that writes files.
- **Workaround in the port:** the missing-key quick fix appends the key to an existing YAML file of the domain.
- **Would need:** a `create-file` / `rename-file` / `delete-file` case in the code-action edits, mapped to the host's existing resource operations.
- **Update 2026-11-06:** this holds for quick fixes only. A command can create a file through the optional `process:tee` capability (`host::run_process("tee", [path], Some(content))`, user-approved). Where this file says "a plugin can't create files", read "a code action can't".

### Problems aren't re-checked while typing

- **Blocks:** live inspections. Problems are only refreshed when a file is opened or saved, so they're stale while you edit.
- **Why:** the only events are `file-opened`, `file-saved`, `editor-focused`, `vcs-changed`, `action-performed` and `scheduled`. There's no `document-changed` event, and no provider that runs on the edit path for diagnostics. `publish_diagnostics` can only be called from commands and event handlers.
- **Would need:** a `diagnostics` provider kind called per document version under the editor budget, or a debounced `document-changed` event.

### No file-change events, short budgets, little storage

- **Blocks:**
  - an incremental index;
  - an index that stays current after `cache:warmup`, `git checkout`, `composer install` or files created outside the IDE;
  - indexing large projects completely.
- **Why:**
  - Events cover only files the IDE opens or saves. `vcs-changed` gives just the repository root.
  - An instance idle for 5 minutes is reclaimed, and `activate` has 5 s.
  - `host::storage` is limited to 1 MiB, too small to persist the index. A reclaimed instance rebuilds it from scratch.
  - Providers get 50 ms, so the index can't be built lazily.
- **Workaround in the port:**
  - The whole index is rebuilt on activation and on saves under `config/`, `templates/`, `translations/` and `src/`.
  - Reading stops after 3 s, and the index is marked partial.
  - All problem checks are skipped while the index is partial.
- **Would need:**
  - a `files-changed` event with created, changed and deleted paths;
  - a per-plugin cache directory (or more storage) for a persisted index;
  - a background task budget for indexing.

### `find_files` skips ignored files and there are no library roots

- **Blocks:**
  - bundle templates (`@Twig/…`, `@EasyAdmin/…`, any `@Namespace/` from `vendor/`);
  - Twig `paths:` namespaces that point into `vendor/` (directories of the project listed in `config/packages/twig.yaml` resolve);
  - vendor translations (`validators`, `security`);
  - vendor service and route XML;
  - Twig functions and filters of vendor extensions;
  - asset files in a gitignored `public/build/`.
- **Why:** the host walks with `ignore::WalkBuilder` (`hidden(true)`, `.gitignore` honoured; `crates/ide-host/src/plugins/api.rs`), so `vendor/` and `var/` are never listed. `read_file` still works on a known path. The SDK's `MockHost` doesn't skip these, so tests pass while the IDE finds nothing.
- **Workaround in the port:**
  - The compiled `var/cache/dev` container and URL generator are read by known path; they include what bundles define.
  - Names starting with `@` are never reported as missing templates.
  - Missing routes and services are reported only once the compiled files exist.
- **Would need:** a `find_files` flag to include ignored files under given roots, or library roots that a language (Composer `vendor/`) can declare.

### Gutter markers have one target and a fixed icon set

- **Blocks:**
  - a template rendered by several controllers (the original shows a chooser);
  - a block overridden by several child templates;
  - framework-specific icons (Symfony, Twig, Doctrine).
- **Why:** `line-marker` has `target: option<location>` and an icon from an allowlist (`navigate`, `implemented`, `implementing`, `overridden`, `overriding`, `run`). An unknown icon falls back to `navigate`.
- **Workaround in the port:** the marker opens the first controller and lists all of them in its tooltip.
- **Would need:** `targets: list<location>` (the IDE shows a chooser), and an icon taken from an SVG in the package.

### No popups, choosers or input dialogs

- **Blocks:**
  - choosing the translation file or domain when adding a key;
  - the original's "create service / controller / form" dialogs;
  - choosing among several definitions or targets (except what the IDE does for multi-location `definition` results).
- **Why:** UI is limited to views, notifications and status items. Nothing returns a user's choice to a command.
- **Workaround in the port:** one quick fix per YAML file of the domain.
- **API 1.2 (`host::pick`, `host::input`, `host::confirm`):** not usable for the translation file. `resolve_code_action` runs with an empty invocation (no window, so the calls return `Invalid`), and a command can edit only the active editor's document, not a translation file. Service/controller/form dialogs have no file-creation API either.
- **Would need:** `pick` allowed in `resolve_code_action`, or a command able to edit another existing file.

### No language injection or template data language

- **Blocks:**
  - HTML, CSS and JS intelligence inside `.twig` files (the original makes Twig a templating language whose template data language is HTML);
  - DQL inside PHP strings;
  - the expression language in `@=…` YAML values;
  - regex in route requirements.
- **Why:** a plugin language is all or nothing. The `syntax-provider` gives flat tokens for the whole document, and there's no way to say that ranges of it are another language and should be handed to that language's grammar or server. `ui-mode`/`framework-ui-modes` choose one mode per file.
- **Workaround in `aritti.twig`:** the plugin's lexer highlights the HTML tags and attributes around the Twig code itself. There's no HTML completion or validation inside `.twig` files.
- **Would need:** injected ranges (`language`, `range`) returned with the syntax result, or a `template-data-language = "html"` key in `languages/<id>.toml`.

### Twig needed its own language plugin

- **Blocks:** shipping Twig and Symfony support as one plugin.
- **Why:**
  - OpenIDEA has no `twig` language or grammar (`languages/` upstream).
  - `syntax-provider` is only called for languages the plugin owns.
  - `register_plugin!` has no arm for `syntax` together with `providers` or `views`.
- **Workaround:** `aritti.twig` owns the language (lexer, folds, outline). `aritti.symfony` adds the providers for the `twig` language id. A single plugin would need the exports expanded by hand, as in `plugins/dotenv` and `plugins/ignore`.
- **Would need:** an SDK arm `register_plugin!(T, providers, views, syntax)`, or a Twig grammar built into the IDE.

### Twig editing behaviour of the language format

- **Blocks:**
  - closing `{% … %}` / `{{ … }}` as you type;
  - Enter-indentation between `{% block %}` and `{% endblock %}`;
  - matching a `{% if %}` with its `{% endif %}`;
  - surround-with and live templates for Twig tags.
- **Why:** the documented `languages/<id>.toml` format has comments, indentation, file detection, servers and debugger only; I found no auto-pair, brackets or on-enter rules for plugin languages. Folding comes from `syntax-provider`, but there's no matching-pair or typed-character handler.
- **Would need:** `auto-pairs`, `brackets` and `on-enter` rules in the language format, or editor providers for typed characters.

### Settings are scalars only

- **Blocks:** list settings such as the original's container files, routing files and Twig namespace paths.
- **Why:** settings schemas support `boolean`, `string`, `number` and `integer` only.
- **Workaround in the port:** comma-separated string settings (`compiledContainers`, `compiledRoutes`).
- **Would need:** an `array` of strings type, shown as an editable list.

### No contribution points for other IDE subsystems

- **Blocks:**
  - Symfony tools for the IDE's built-in MCP server (`symfony-mcp.xml`);
  - routes in an endpoints view or HTTP client (`symfony_endpoints.xml`);
  - Doctrine mappings in the database tool (`symfony_database.xml`);
  - `bin/console` command completion in the terminal (`symfony_terminal.xml`);
  - Markdown integration (`markdown.xml`);
  - extensions from PHP Toolbox and PHP Annotations, which the original depends on.
- **Why:**
  - The plugin API has no MCP or AI tool registration, no endpoints, database or terminal completion extension points, and no plugin-to-plugin API.
  - `executeAction("plugin.<id>.<command>", arg)` is one-way and returns nothing to the caller.
  - `open_terminal` starts a program but can't contribute completions.
- **Would need:** an MCP tool contribution (`[[contributes.mcpTools]]` → a command with a JSON result), and an endpoints provider.

### Code actions don't get the diagnostic, and the range is undocumented

- **Blocks:** nothing outright. Quick fixes have to re-derive the problem from the text.
- **Why:** `code-actions(doc, selection)` receives no diagnostics. The docs don't say whether `selection` is the caret or the diagnostic's range when ⌥⏎ is pressed on a squiggle.
- **Workaround in the port:** the reference is looked up at `selection.start`. Check this in the IDE.
- **Would need:** the diagnostics under the range (`code`, `range`) passed to `code-actions`, and the selection semantics documented.

### Completion doesn't open on a quote

- **Blocks:** the original's auto-popup of routes, templates and services when `'` or `"` is typed in `render('`, `path('` or `'@`. Twig `{% block ` doesn't pop up either.
- **Why:** the editor hard-codes its trigger characters (`src/editor/lsp/completionItems.ts`, `triggerCharacter`: `.`, `::`, `->`, `@`). A provider can't declare its own; it only receives the `trigger` the editor picked.
- **Workaround:** none. The YAML `'@…` case pops up through `@`; everything else waits for the editor's regular popup or explicit completion (Ctrl+Space).
- **Would need:** `trigger-characters = ["'", "\"", "/", " "]` on `[[contributes.providers]]`.

### An empty plugin answer in a file with no language server switches the feature off

- **Blocks:** dependable completion, ⌘B and quick fixes in `.twig` files. The same applies to PHP, YAML and XML files when their language server isn't installed.
- **Why:** completion, `definition` and code actions return the language server's result when the plugin's answer is empty (`crates/ide-host/src/ops/lsp.rs`: `if items.is_empty() { return result; }`, the same for `found` and `plugin_actions`). With no server for `twig`, that result is an "unavailable" error, and the editor backs that feature off for 15 s (`src/editor/lsp/availability.ts`, `DEFAULT_BACKOFF_MS`). The Symfony provider answers nothing whenever the caret isn't on a route, template, translation, block or variable, which happens on most keystrokes. So template and route completion in Twig can go quiet for 15 s.
- **Workaround:** none in the plugin. A provider can't return a "no result, but available" answer.
- **Would need:** the host treating a plugin provider's answer, even an empty one, as available for the document.

### Events can't be filtered, and providers get no project

- **Blocks:** waking the plugin only for files that feed the index, and finding a document's project without extra host calls.
- **Why:**
  - `host::subscribe(kind)` takes only the event kind (`crates/ide-host/src/plugins/api.rs`, `subscribe(kind, on)`), so every file opened or saved in any project wakes the plugin. It then filters by path itself (`affects_index`).
  - Editor providers receive a `document`, not an `Invocation`. To read a parent template (blocks, overriding markers) or resolve a quick fix, the plugin calls `host::projects()` and matches roots on every call, inside the 50 ms budget.
- **Would need:** `subscribe(kind, glob)` or `on-event:file-saved:<glob>`, and the document's project passed to editor providers.

### Added on 2026-11-06 with the current API

- **Twig `paths:` namespaces:** `@ns/…` templates resolve and complete from `config/packages/twig.yaml` (map form, quoted or unquoted keys, `%kernel.project_dir%`, an empty namespace is the main one). Limits: only that one file is read (not `when@` overrides or other `packages/` files); names starting with `@` are still never reported as missing.
- **Symfony Profiler view:** `symfony-profiler` lists the latest 100 requests from `var/cache/dev/profiler/index.csv` (sub-requests hidden, times in UTC). Limits: rows can't be opened (no panels); the command `show-profiler` refreshes the view but a command can't open it; the view reads the file when rendered.
- **Twig completion and checks:** tags (with their `end` forms), filters, functions and tests are completed from static lists taken from Twig's documentation, and an unclosed block tag is reported (`unclosed-tag`; verbatim and comment bodies, inline `{% block t 'x' %}` and the tags `block if for macro embed apply autoescape with verbatim cache sandbox` are handled). These come from `aritti.symfony` for the `twig` language in a Symfony project. Outside one, `aritti.twig` now has its own provider (same lists, thirteen snippets, `unclosed-tag`, end tags without an opener, `unterminated-delimiter`, setting `diagnostics`) and leaves name completion and the unclosed-tag check to `aritti.symfony` when `symfony.lock` or `symfony/framework-bundle` is present, so nothing is listed or reported twice (the registration was hand-combined, see "Twig needed its own language plugin"). Limits: the lists are static, so filters, functions and tests of the project's own Twig extensions and of bundles are not offered; completion and the check only run in projects `aritti.symfony` recognises as Symfony (`symfony/framework-bundle` in `composer.json`); a stray `{% endx %}` with no opener is not reported; the function list is offered at every expression position; the `{% set x %}…{% endset %}` capture form is not checked. Template-path completion (`extends`, `include`, `embed`, `import`, `from`, `use`) was already there.
- **Duplicate XML service ids:** a second `<service id>` with the same id in one `config` XML file is a warning (`duplicate-service`). Ids defined in different files are not reported, because Symfony lets later files such as `services_dev.xml` override an id.
- **Unquoted YAML values:** a plain scalar starting with `@` or `%` in YAML under `/config/` and `/translations/` (block values, `- ` items and flow collections) is reported (`unquoted-yaml-scalar`) with a quick fix that wraps it in single quotes.

### Not ported, but possible with the current API

These aren't API limits; they were left out for size:
- Twig macro completion and navigation (`{% import 'x.twig' as m %}`, then `m.name(`), and navigation to filters and functions that the project's own extension classes declare;
- Twig `paths:` read from other config files, including `when@` overrides.

### Added by the feasible-parity pass (2026-11-06)

All checks are text-based, quiet on unresolvable input, switchable in settings (`controllerChecks`, `serviceChecks`, `phpCallChecks`, `twigChecks`) and gated by the master `diagnostics` switch; they run on open and save only.

- **New codes:** `controller-method-missing`, `deprecated-route-setting` (`Bundle:Controller:action`, `pattern`, `type: annotation` from Symfony 6.4 on, `_method`/`_scheme` requirements), `deprecated-service-class`, `duplicate-parameter-key`, `named-argument-missing`, `form-type-string`, `missing-asset`, `missing-translation-domain`, `twig-missing-route`, `twig-missing-asset`; nine New-menu templates (command, controller, form type, Twig extension, compiler pass, kernel test, web test, event subscriber, voter).
- **Limits:** classes come from the PSR-4 index of project sources; a class with a parent or trait outside the project is unknown and nothing is reported. Each check reads class files (cached per run, 256 KB per class, 500 reads). `missing-asset` is quiet for `build`, `bundles`, `assets`, `_assets` and for directories with no indexed file. `missing-translation-domain` reports only lowercase project-style domains when the project has no file for them; vendor domains written in lowercase (`sonata_admin`) are still reported. `type: annotation` needs a `symfony/framework-bundle` constraint of 6.4 or newer in `composer.json`. With `twigChecks` off, Twig routes are no longer reported at all (the older `missing-route` skips Twig). Twig checks do not skip `{# #}` comments or `{% verbatim %}` spans.
- **Not verified in the IDE:** the `$NAME$` and `$END$` placeholders in the template files (copied from the `jpa-assist` convention; the namespaces are fixed, so no `$PACKAGE$`), and that a template with no name prompt creates a usable file.
- **Would need for type-aware checks:** the symbol queries described above (U4).

### Added by the gap F pass (2026-11-06)

Branch `gap-f-php`, tested against the mock host only. The public Symfony Plugin source was unreachable, so rules follow the plan text, the Symfony upgrade notes and the Doctrine documentation, and the messages are our own.

- **New codes:** Twig (`twigSyntaxChecks`, `twigChecks`) `twig-route-compare`, `twig-missing-translation-domain`, `twig-for-condition`, `twig-component-block-syntax`, `twig-component-self-import`; YAML/XML `deprecated-factory-setting` (`serviceChecks`), `yaml-unescaped-backslash` (with quick fix) and `yaml-unquoted-colon` (`yamlSyntaxChecks`, which now gates all YAML scalar codes); PHP `command-invokable-return-type`, `console-input-mode` (`commandChecks`), `filesystem-mirror-copy-on-windows`, `tagged-collection-default-method`, `from-callable-option` (`deprecationChecks`, only when `symfony/framework-bundle` is 8.1 or newer); Doctrine `orm-empty-table`, `orm-table-deprecated-property` (ORM 3.2), `orm-lifecycle-callbacks-missing`, `orm-one-to-many-mapped-by`, `orm-inverse-side-join-column`, `orm-join-column-nullable` (`doctrineChecks`, from the Doctrine 3.7 docs).
- **Also added:** six code actions (route attribute, parameter type, form type constant for one or all, block override), tag-name completion, the `twigNamespaces` setting, three service-config templates, file icons (own glyph) and a `symfony` status item. `events` is now optional for `aritti.symfony`, so the install prompt changes; without it problems are not refreshed on open and save.
- **Rulings:** `route-attribute-namespace-deprecated` dropped (Symfony UPGRADE 6.4/7.0 do not deprecate it); `from-callable-option` flags the option set of UPGRADE-8.1.md (`alias`, `parent`, `synthetic`, `file`, `arguments`, `properties`, `configurator`, `calls`), PHP-array services only; `tagged-collection-default-method` reads string literals only; Doctrine `lifecycle-method-visibility` and `discriminator-map-duplicate` dropped (the docs state no such rule); `orm-inverse-side-join-column` is a hint, quiet for `JoinTable` and a positional `OneToMany` argument; any positional string literal counts as `mappedBy`; the lifecycle-callbacks check covers `#[ORM\Entity]` only (a recollection of Doctrine, not verified).
- **Known limits:** the core tag list comes from memory of Symfony's tag page (two entries removed after checking symfony-docs 7.4); a Twig `{% component %}` inside `<twig:Name>` silences the block rule; a flow YAML value whose trailing comment holds `, "..\.."` can give a false `yaml-unescaped-backslash`; flow `tags: [..]` is read from its first line only; XML `<tag name>` is recognised anywhere; the status item of a multi-project window shows an arbitrary indexed project; the replace range of an empty YAML tag value is unverified.
- **Not verified in the IDE:** several edits of one code action applied against the original coordinates; `$END$` in the new service templates; the `fileIcons` name patterns; the status item `reindex` command; the `Invalid` read-error kind (the mock returns `NotFound`, tests use closures).

### Added by the A1 reclass pass (row 38, 42, 48, 49a)

- **Commands** (`editor.context`, Java only; `editor.read` required, `editor.write` and `storage` optional): Generate Setter Dependency, Constructor Dependency (extends a single existing constructor through the same merge as "Use constructor injection"), Autowired Dependency and Event Listener. The bean comes from `pick` over the project index, the event type from `input`; the member lands in the class around the caret (innermost class, else the first) after its fields or last member, with imports from the project's own qualified type names. Nothing is edited when the prompt is dismissed, the field or method already exists, or the class has several constructors. The caret does not move to the new code (no cursor API) and library types are not offered.
- **Change Active Spring Profiles** (`tools` menu): `pick_many` over `@Profile` and XML `profile=` names plus `application-*.yml/yaml/properties` file names, stored in plugin storage. It narrows only the duplicate-bean-name checks (a bean whose profiles are not active is no clash); the dialog cannot preselect the current choice, so the title names it. Not wired to run configurations, `set_context` or navigation.
- **Indexer keys:** type keys became `type:Simple#bean` with `prefix#` aliases per declared supertype, so inherited types resolve across files. New keys: event publishers (`publishEvent(new X(...))`) mark `event:X#listener` (Java `@EventListener`, `@TransactionalEventListener` with value/classes or first parameter, `ApplicationListener<X>`), listeners mark `publish:X#call` back; a Java class marks the XML beans of its simple name (`xml:X#bean`) and each XML bean marks the class (`class:X#decl`). Several targets open the host's chooser. Not verified in the real host.
- **Skipped:** publishers whose argument is not `new X(...)` (a variable's type needs flow analysis); Kotlin publishers and listeners (the Kotlin scanner has no call or annotation-argument model here); events matched by simple name only; `@ComponentScan` and `@Bean` override markers (effort, no new API limit); the bean-to-injection-points marker stays on the provider path.

### Added with the document indexer (API 1.2)

`[[contributes.documentIndexers]]` (`php`, `yaml`, `yml`, `xml`, `twig`, `js`, `ts`) publishes `service#id` and `route#name` keys from the existing scanners (YAML and XML services, `#[Route]` attributes, `config/routes*` YAML, XML routes) and the references the plugin already recognises (`@service` and `service('id')`, `path()`, `generate()`, `generateUrl()`, `redirectToRoute()`). Go to Declaration then works from the host's project graph, outside the 50 ms provider budget.
- **Not done:** services declared by PHP attributes (`#[AsAlias]`, `#[AsTaggedItem]`) and PHP config files (`services.php`): the plugin has no scanner for them, and writing one was not part of a cheap reuse. Vendor, `node_modules` and `var/cache` files are skipped.
- **More keys (js/ts added to the extensions):** `command#name` (`#[AsCommand]`, `$defaultName`; used by `bin/console x` in any indexed file and `->find('x')`), `param#name` (YAML `parameters:` and XML `<parameter key>`; used by `%x%` in YAML/XML and `getParameter()`/`hasParameter()`), `twig-filter#`, `twig-function#`, `twig-test#` (`new TwigFilter('x')`, `#[AsTwigFilter]` and the function/test forms; used by `|x`, `x(` and `is x` inside `{{ }}`/`{% %}`), `twig-component#` (`#[AsTwigComponent]`, `templates/components/**`; used by `<twig:X>` and `component('X')`) and `stimulus#` (`assets/controllers/*_controller.js`; used by `data-controller` and `stimulus_controller()`). Go to Declaration only. Not covered: commands registered by the `console.command` tag or `setName()`, composer scripts (`json` is not indexed), `setParameter()`, parameters from bundles or `.env`, vendor Twig extensions, `{% component %}` tags, UX components from `config/packages/twig_component.yaml` namespaces, Stimulus controllers in `controllers.json`. A name with no definition (a built-in filter, `path`) simply does not jump. Twig scanning is textual, so a quoted `foo(` inside `{{ }}` counts as a function use.
- **Gutter markers moved onto the index (the `line-markers` provider is gone):** a controller `render()` goes to its template, a template to all controllers rendering it, a block to the parent's block and to every child overriding it, a PHP class to its service definitions, a service `class` to the class, a route to all `path()`/`generate()` uses (chooser when several). Changes: the icon is the host's default (no "overriding" icon, the allowlist cannot be chosen per index marker); a block's parent is the one named by `{% extends %}` directly (no walk up to a grandparent that defines the block); templates are named by their path under `templates/` (and `bundles/`), so `twig.yaml` paths and `twigNamespaces` are not applied; a marker appears once the host has indexed the project, not before.
- **Commands (window, not provider):** `run-console-command` lists `php bin/console list --raw` (needs `process:php`) plus the indexed `#[AsCommand]` names, asks for arguments, and opens a terminal tab; `go-to-route|service|console-command|template` pick from the plugin's index and `open_location` (no type-ahead filter is documented, so big lists are unverified); `twig-extends` (editor context, `editor.read`/`editor.write`) picks a template and inserts `{% extends '...' %}` at the top of the focused editor. Not done: a run configuration (the terminal tab is the replacement), Search Everywhere contributors (no API).

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Twig (`aritti.twig`):** completion of tags, filters, functions and tests, thirteen snippets (`blk`, `mcr`, `esc`, `emb`, `ext`, `for`, `from`, `if`, `imp`, `inc`, `sbx`, `spl`, `verb`) in HTML text, and the inspections `unclosed-tag`, end tag without opener and `unterminated-delimiter` (`{{`, `{%`, `{#`). A stray `}}` is not reported (common in HTML text). In a Symfony project `aritti.symfony` owns name completion and unclosed-tag. Unverified: that the provider is called in `.twig` files with no language server (see the empty-answer limit above) and that snippets expand as plain text.
- **No Twig formatter (TW3, skipped):** OpenIDEA drives only Prettier and Ruff from a language definition, and neither parses Twig.
- **Index (`aritti.symfony`):** project-wide Go to Declaration for service ids and route names (YAML and XML services, attribute, YAML and XML routes; `@service`, `path()`, `generate()` references), plus `command#`, `param#`, Twig extension filters, functions and tests, UX components, Stimulus controllers, events, macros, voters and validators; gutter markers moved onto the index (a template lists all its controllers, a block all overriding templates, a route its `path()` uses).
- **Checks and actions (text based, not verified in the IDE):** Twig (assets tags, `enum()` classes, content outside blocks in extending templates, hard-coded paths with a quick fix), container references (unknown `%parameters%`, missing service classes, deprecated classes), service wiring (argument count and types, tag interfaces, listener methods, `!php/const`), practice hints and version-gated deprecations for PHP, Doctrine removed-API checks, typo quick fixes (routes, templates, keys, domains), rewrites (Doctrine alias to `::class`, command to invokable, Twig extension to attributes), generators (service definition, argument resync, tags), Extract translation, Add form fields. Settings `extraServiceFiles`, `extraRoutingFiles`, `practiceChecks`. Classes outside the project's sources are unknown and silence a check.
- **Commands:** Run Console Command, Go to Route, Service, Console Command and Template, Extend Twig Template (pick lists from the index; a quick fix still cannot ask).
- **Not verified in the IDE:** the index key set against a real project size, the `pick` list type-ahead, the edits of the rewrite actions applied against original coordinates.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Commands can write files now:** *Make...* (asks for the maker and its arguments, then runs `bin/console make:*`; optional `process:php`), *Create Twig Template...* and *Insert Form Rows from a Form Type...* are commands, not quick fixes. *Create Twig Template* writes through the optional `process:tee` and copies the path without it. This narrows *Code actions can't create, rename or delete files* above: quick fixes still can't, commands can.
- **Added rows (symfony):** practice, wiring and deprecation checks (audit rows 10, 22, 24, 20, 15, 17, 3, 8, 11; 12 of 14 and 6 of 8 sub-checks of rows 10 and 18), index links I6, I10 and I11, and macros in the index. The exact check list is in the symfony change-notes.
- **Twig (`aritti.twig`):** macro name completion after `alias.` and in `from 'x' import`, Go to Declaration for macros and template names, filters, functions and tests registered in project PHP outside a Symfony project (setting `projectExtensions`), and an optional unbalanced-bracket check (setting `expressionErrors`).
- **Skipped:** ClearParameter, JoinWithToOn, AutowireControllerAsService, the `@Template` rows, Vite and the run configuration; vendor-registered Twig extensions stay invisible (library PHP is not in the index).
- **Not verified in the IDE:** the `make` prompt chain, the `process:tee` write and the bracket check against real templates.

## Laravel (`aritti.laravel`) and Blade (`aritti.blade`)

Port of `de.espend.idea.laravel` 0.15.4 (the free Laravel plugin, MIT) plus features that follow Laravel's documentation. Checked against the SDK, WIT and host source of v0.1.37 on 2026-11-06. The gaps in the Symfony section above apply here too (no PHP semantic model, no references, rename or hover, code actions can't create files, problems aren't re-checked while typing, one target per gutter marker); this section only adds what is specific to Laravel.

### Run configurations: artisan

- **Blocks:** choosing the artisan arguments (`serve --port`, `test --filter`), or a `./artisan` wrapper such as Sail.
- **Workaround:** run detectors are fixed commands matched by file name: `php artisan serve`, `test`, `queue:work` and `schedule:work` appear in every project that has an `artisan` file, Laravel or not. Use Laravel: Show Route List's terminal or the IDE's terminal for anything else. Laravel: Fresh Migration asks twice within 30 seconds because it drops all tables. Laravel: Run Artisan Command... lists `artisan list --raw` (falls back to a built-in list), asks for arguments split on spaces (no quoting) and opens a terminal.
- **Generators:** Generate with artisan make..., New Eloquent Model..., Generate for Model... and Create Missing View, Component or Controller (editor menu) pick and ask, validate names (letters, digits, `_ . / \ -`, no leading dash), then run `php artisan make:*` in a terminal so PHP creates the files; the plugin cannot create files itself. Output is Laravel's stock stub, not Laravel Idea's field and relation filling. They need process:php (and editor.read for Create Missing Target), and `make:view` needs Laravel 11+. Not done: PromoteRequestToFormRequest, Extract view/component (content for a new file, U2), Nova/Filament/Fractal resources, quick-fix surface (no dialog or process in a code action).

### Blocks: type-aware Laravel features

- **Blocks:**
  - `app('x')` / `App::make('x')` container aliases and the classes `@inject` names (the original resolves them with PHP type providers);
  - facade and Eloquent resolution: relations, accessor attributes, and `where('column')` on a query whose model is not named in the chain (`$user->posts()->where(…)`, a builder passed as an argument);
  - `providers` class completion in `config/app.php` (needs a project class index);
  - the _Extract partial view_ refactoring (no refactoring hooks).
- **Why:** a provider gets the document text only. A call is recognised by its function or method name (`route(`, `->middleware(`), never by the class it is called on, so `$request->route('id')` is excluded by a rule on its receiver (`redirect()` only) and other lookalikes are left out rather than guessed.
- **Would need:** the symbol queries described in the Symfony section (`classes`, `members_of(fqcn)`, `return_type(fqcn, method)`).

### Blade needs its own language and has no PHP or HTML injection

- **Blocks:** highlighting of the PHP inside `{{ }}`, `@if (…)` and `@php` by the IDE's PHP grammar, and HTML highlighting by its HTML grammar. `aritti.blade` lexes both itself, so it is an approximation: PHP keywords, strings, variables, numbers, calls and class names; HTML tags and attributes.
- **Why:** `syntax-provider` returns flat tokens for a language the plugin owns, and a plugin language can't take the built-in `php` id or extension. See "No language injection or template data language" above.
- **Not verified in the IDE:** that `*.blade.php` resolves to the `blade` language rather than `php`. The registry matches compound extensions longest first (`crates/ide-language/src/registry.rs:382`) and only drops a plugin extension that equals a built-in one (`blade.php` ≠ `php`), but this was read, not run. If the file stays `php`, `aritti.laravel` still reads it as Blade (it checks the `.blade.php` suffix) and everything but highlighting works.
- **Would need:** language injection, as described in "No language injection or template data language" above.

### Custom Blade directives are invisible, and components come from file names

- **Blocks:** highlighting of `Blade::directive('name', …)` directives, and completion of those registered in vendor code; components registered with `Blade::component()` or by a package (`<x-package::alert>`); the slots and attributes a component accepts (`<x-slot:title>`, `:prop`).
- **Why:** a directive or alias exists only in the PHP that registers it, which a plugin can't evaluate, and the plugin has no PHP class index to read a component's constructor.
- **Workaround in the plugins:** `@` completion offers Laravel's and Livewire's own directives plus the literal `Blade::directive('name'` calls found in `app/**`; vendor package directives are not offered, highlighted or checked (no unknown-directive warning, it would be false positives). The highlighter knows only the core directives. Components are found by Laravel's naming convention (`resources/views/components`, `app/View/Components`, `app/Livewire`) and by the literal aliases of `Blade::component()` in `app/Providers`. A missing component, Livewire component, Inertia page (`resources/js/Pages`, `resources/js/pages`) or `Model::factory()` target is reported; package namespaces (`<x-package::alert>`) are skipped, and so is every component when a provider calls `anonymousComponentPath()` or `componentNamespace()`. Inertia pages are only checked once the project has some, so a custom `page_paths` in `config/inertia.php` can give false warnings; each check has its own setting.

### Route names that packages register can't be checked

- **Blocks:** reporting `route('horizon.index')` as missing. Packages add routes in their service providers, which are PHP the plugin doesn't run, and `php artisan route:list` needs `process:php` and a working application.
- **Workaround in the plugins:** missing routes are an opt-in setting (`reportMissingRoutes`, off by default). Config and translation keys are only reported below an array or file the project itself defines, and views with a `package::` namespace aren't checked.
- **Would need:** reading `bootstrap/cache/routes-v7.php` (compiled by `route:cache`, serialised PHP), or a host function that runs an allowed command from a provider.

### New-menu file templates have a fixed namespace

- **Blocks:** New › Laravel Controller / Job / Event / … with the `namespace` of the folder the file is created in, and a prompt for the model, table or event class.
- **Why:** same limit as "Plugin file templates get no package or prompts" for JPA: a template gets only the file name (`$NAME$`) and `$END$`, no package and no extra prompts, and can't create more than one file.
- **Workaround:** the 16 templates (controller, resource controller, job, event, listener, mailable, middleware, form request, policy, observer, enum, seeder, factory, migration, Pest feature and unit test) use Laravel's default namespace for their type (`App\Http\Controllers`, `App\Jobs`, `Database\Seeders`, …); change it by hand in a subfolder. The migration is an anonymous class, and its file name needs the timestamp prefix typed by hand.
- **Would need:** `package-style` and prompted variables on `[[contributes.fileTemplates]]`.

### Not done, possible with the current API

- Dropped and renamed columns (`dropColumn`, `renameColumn`) and columns added by a package's published migrations outside `database/migrations`; irregular plurals for a model without `$table` (`Person` is not `people`).
- Custom validation rule classes (`app/Rules`) are objects, not strings, so only Laravel's own rule names are completed, and a rule's parameters (`exists:users,email`) are not.
- Gate resource abilities (`Gate::resource('posts', …)` registers `posts.view` and friends) are not read.
- Artisan `make:*` commands: closed in API 1.2, they ask for names and options and run in a terminal (see "Run configurations: artisan").
- Intentions on the open file are code actions (⌥⏎): validation string to array (not offered when a rule is `regex:`/`not_regex:`, whose pattern may hold pipes), `'UserController@index'` to `[UserController::class, 'index']` (only when the routes file is in the controllers namespace or imports the class with a plain `use`), and the Blade `{{ }}` / `{!! !!}` toggle. `Rule::exists/unique/in/notIn` calls are rewritten in the open file (see the gap F pass below); a custom rule class would need a new file, which plugins can't create.

### Added by the feasible-parity pass (2026-11-06)

- **Eloquent checks** (`checkEloquent`, PHP files, on open and save): `guarded-attribute-assignment` (`Class::create/make(...)` with request input or an array literal, and `(new Class)->fill(...)`, when the class resolves to a project model that declares `$fillable` or `$guarded`), `unscheduled-prunable`, and `simplify-order-by` (hint with a quick fix to `latest()`/`oldest()`). Diagnostic codes are bare, without the `laravel.` prefix of the older problems.
- **Limits:** the model is resolved by import or FQCN under `App\` and read from the PSR-4 path; no type inference, so `$post->fill()` is not checked; `forceFill`/`forceCreate` are not checked (they bypass `$fillable` by design); a model with neither property is not flagged; models using `getFillable`, `mergeFillable`, `#[Fillable]`, `#[Guarded]` or `unguard` stay quiet. `unscheduled-prunable` looks for the schedule in `routes/console.php` and `app/Console/Kernel.php`, so a schedule in a service provider gives a false positive; a global `Model::unguard()` makes the fillable warning wrong. Tests, seeders and factories are checked like other files. Model and schedule files are read on each check.
- **Intention:** a rules array to a pipe string, offered with the caret on a string element of a rules list for a key; not offered with `Rule` objects, closures, `|`, quotes, `$`, `\`, comments, keyed items or `regex`/`not_regex` rules.
- **Dropped:** the scope-call-to-attribute intention (needs types). Not done: Pest navigation and promoting `$request` to a FormRequest (both need types or a new file).
- **Ziggy in JS/TS/Vue:** `route('name')` and `route().current('name')` complete and open the route in `javascript`, `typescript`, `vue`, `javascriptreact` and `typescriptreact` files (also inside `:href="route('x')"`). The scanner is line-bounded for quotes and tracks only block comments across lines. Accepted behaviours, pinned by tests: a `/*` inside the text of a multi-line template literal opens a block comment and silences later route calls until the next `*/`; a stray quote can pair with a later quote of the same kind on its line; an apostrophe in Vue template text and a bare `//` in template text give false negatives; `<style>` is not excluded; `.cjs`, `.mts` and `.cts` are not matched; a template literal with `${` before the cursor is quiet. The scan starts at offset 0 on each call (unmeasured on 1 MB bundles), and `route(` must be written without a space. There is no Inertia page completion in JS: no documented client call takes a page name.
- **Blade inspections** (`aritti.blade`, `checkDirectivePairs`, `checkParentheses`): `unpaired-directive` and `unpaired-parenthesis-in-string` for files whose path ends `.blade.php`, published on open and save. A bare `@empty` and stray `@show`/`@stop`/`@append`/`@overwrite` are not reported; `<img @error=...>` reads as an `@error` block (as Laravel's compiler does); the parenthesis check counts `)` inside comments and heredocs. Directive completion stays in `aritti.laravel`, so a blade-only install has none.
- **Added by the gap F pass (2026-11-06):** `env-outside-config` (`checkFunctions`) on `env()` outside `config/`, `bootstrap/`, `vendor/` and `tests/` (strings, heredocs and `Ns\env(` are quiet); `latest(col)`/`oldest(col)` for literal `*_at`/`*_date`/`date` columns; code actions for `@class`, bulk rule arrays, bulk controller strings, `Rule::` objects (adds the `Illuminate\Validation\Rule` import) and facade imports (the `Context` facade imports `Illuminate\Support\Facades\Context`); the `controllersNamespace` setting; view namespaces from `ide-blade.json` at the project root and in `.idea/` only (a plain view root when `namespace` is blank; `\uXXXX` escapes are not decoded); about 45 Blade directive snippets in `aritti.laravel` (`aritti.blade` is unchanged; the `switch` snippet has no `@case`). The free Laravel plugin source was unreachable, so rules follow the plan text. `events` is still a required capability of `aritti.laravel`.
- **Gap F limits:** the facade import is silently skipped in rare code where a word pair looks like a class declaration (`$class = Auth::user()`).
- **Not verified in the IDE (gap F):** several edits of one code action applied against the original coordinates; the snippet `\$` escape, 4-space indentation and `$0`.
- **Not verified in the IDE:** the language ids `vue`, `javascriptreact` and `typescriptreact` (files are also matched by extension, but the host calls a provider only for declared ids); `on-file:resources/js/**` activation for a `.vue` file opened first; mid-word replacement in the editor; completion on `@`; the quick fix of `simplify-order-by` and the rules intention, which depend on prefilled code-action edits being applied without a resolve step; for Blade, that the file that first activates the plugin is checked only on its next open or save (events are subscribed in `activate`), and that `on-language:blade` loads the plugin before any `.blade.php` event.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Closed:** Artisan `make:*` generators ask for names and options (`host::input`/`pick`) and run `php artisan make:*` in a terminal (Run Artisan Command..., Generate with artisan make..., New Eloquent Model..., Generate for Model..., Create Missing View, Component or Controller); the file is created by PHP, not by the plugin. Needs the optional `process:php`; `editor.read` for Create Missing.
- **Added:** Livewire method and route controller/method problems (quiet for invokables, vendor controllers, classes with a trait or a parent other than `Component`), Find Unused Views, model/factory/policy/observer/event gutter links from a document indexer (a marker exists only when the counterpart is at its conventional path; the LA6 check that the counterpart exists was skipped), component attribute and slot completion, relation-aware column completion, container name completion (`app()`, `@inject`, config providers), intentions for scopes, constructors, facades, translations and rules (not offered when the variable is reassigned, captured by `compact`, or a use is in a static method), and 11 more file templates. Limits as for the rest of the plugin: no types, models resolved by import under `App\`.
- **Blade:** directive completion and its snippets are provided by `aritti.laravel`, not `aritti.blade` (Blade has no `completion` provider).
- **Not verified in the IDE:** the indexer markers, snippet escapes, and that the terminal receives `make:*` arguments unquoted.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Commands can write files now:** *Generate for Model* and the Nova, Filament and Livewire form generators write through the optional `process:tee`; *Remove IDE Helper PHPDoc from Models* edits many files. Quick fixes still edit existing files only.
- **Templates:** 20 more New-menu templates (Channel, Console Command, Http Exception, PHPUnit tests, Resource Collection, Class, Model, Dusk, Nova, Livewire Form, Fractal, Volt, three Inertia pages, four Blade files); they still get only the file name.
- **Checks and navigation:** one problem listing the missing `Route::resource` methods (`reportMissingResourceMethods`), *Use #[Scope] on all scope methods*, column completion through `$query = Model::query()` variables, route and controller gutter links from the document indexer, `modelDirectories` setting, *Create Missing* for Job, Mail, Event and similar.
- **Not done (B):** rows LA14, 21, 24, 25, 29 and 30. **Blade (`aritti.blade`)** is unchanged.
- **Not verified in the IDE:** template placeholders, the `process:tee` write paths and the indexer links.

## aritti.dotenv (port of ru.adelf.idea.dotenv)

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Hide values in `.env` files (`DotEnvValuesHiding`: each value folded to a placeholder, toggled with Expand/Collapse) | A syntax provider's `fold` is a whole-line range with a kind (`wit/exports.wit`, `record fold { start-line, end-line, kind }`): no byte range, no placeholder text, and there is no editor decoration or conceal API | A fold with byte offsets and a `placeholder` (folded by default when asked), or `editor.set-decorations` with a conceal style |
| Completion popup when typing `$` (`NestedVariableTypedHandlerDelegate.checkAutoPopup`) | Trigger characters are hard-coded in the editor (`src/editor/lsp/completionItems.ts`, `triggerCharacter`: `.`, `::`, `->`, `@`); a plugin can't declare its own and there is no typed handler | `trigger-characters = ["$", "{"]` on `[[contributes.providers]]` |
| `.env*.php` files opened as PHP (`PhpFileTypeOverrider`) | The built-in `dotenv` definition owns the `.env.*` file name pattern, and a plugin can't take a built-in definition's ids, extensions or file names (`crates/ide-language/src/registry.rs`, `build`); users can remap in Settings › File Types | A file type override contribution with a priority over built-in patterns |
| Find Usages tool window and the Rename refactoring (`DotEnvKeyReferenceUsageSearcher`, `DotEnvKeySymbolRenameUsageSearcher`, `DotEnvKeySymbol`) | Providers are only `completion`, `inline-completion`, `code-actions`, `definition`, `line-markers` (`wit/providers.wit`); there is no references or rename provider, and no input dialog to ask for the new name | `references(doc, at) -> list<location>` and `rename(doc, at, new-name) -> list<file-edit>` (plus `prepare-rename`) provider kinds |
| The plugin's own extension points (`environmentVariablesProvider`, `environmentVariablesUsagesProvider`), through which other plugins add files that declare or read keys | A plugin can't offer an extension point or a service to other plugins (CLAUDE.md: no dependencies on other plugins' APIs) | Plugin-to-plugin extension points or services |

### Implemented with a workaround

| Feature | Workaround | Limitation | API that would remove it |
|---|---|---|---|
| Highlighting of `.env` files | The IDE's built-in `dotenv` language has no grammar. The plugin declares `languages/dotenv.toml` with the same id: the built-in definition wins (detection, comments), but `PluginLanguages::owner_of` (`crates/ide-language/src/plugin_point.rs`) still names the plugin, so `syntax-provider` calls for `dotenv` reach it | Relies on undocumented host behaviour; if the host stops routing syntax calls for a shadowed id, `.env` files go back to plain text | A documented way to provide syntax for a built-in language without a grammar, e.g. `syntax-for = ["dotenv"]` in the manifest |
| Syntax provider and editor providers in one component | Hand-written registration in `src/lib.rs` from the SDK's `#[doc(hidden)]` macros (`__plugin_exports!`, `__providers_exports!`, `__rt::highlights`, `syntax_bindings::__export_syntax_world!`) | `register_plugin!` has arms for `providers`, `views`, `providers, views` and `syntax` only; the internals can change in any SDK release | `register_plugin!(P, providers, syntax)` (and every combination) |
| Inspections while typing (IntelliJ runs `localInspection`s on the fly) | Problems are published on `file-opened` / `file-saved` and from the `code-actions` call the lightbulb makes as the file changes | With no language server for `.env`, an empty plugin answer turns into the LSP's `unavailable` (`crates/ide-host/src/ops/lsp.rs`, `lsp_code_actions` and `lsp_completion`: `if plugin_actions.is_empty() { result? }`), and the editor then stops asking for that document for 15 s (`src/editor/lsp/availability.ts`, `DEFAULT_BACKOFF_MS`). Problems lag up to 15 s between saves | Treat a plugin provider's answer, even empty, as available; or a `diagnostics` provider export called once per document version, like `syntax-provider` |
| `${VARIABLE}` completion in `.env` files | `completion` provider for `dotenv` | Same back-off: after any empty answer (typing a key on another line) completion falls back to word completion for 15 s | Same host fix |
| Finding `.env` files (the original indexes every file of the `.env` file type) | `find_files` skips hidden and git-ignored files (`crates/ide-host/src/plugins/api.rs`, `WalkBuilder::new(root).hidden(true)`), so `.env*` can't be globbed: the plugin reads a fixed list of names at the root and in folders holding a project marker (`package.json`, `composer.json`, `go.mod`, `Dockerfile`…), and adds `.env` files when they are opened | A `.env` in a folder without a marker is unknown until opened, and forgotten when the idle instance is reclaimed | `find_files(glob, limit, options { hidden, ignored })` |
| The keys index and usages index (`DotEnvKeyValuesIndex`, `FileBasedIndex`, `PsiSearchHelper`) | The plugin walks the project (`find_files("**/*")` + `read_file`) on activation and on events, at most 3 s per run, and keeps the index in memory; saved and opened files update it | Rebuilt after every idle reclaim (5 min); cut short in very large projects; stale after changes made outside the editor (checkout, deletes, generated files): there is no file-changed or file-deleted event; editor providers (50 ms, interrupted at 500 ms) can't scan, so the first request after a new project opens only sees the root's env files | A host text search / index API (`workspace.search(pattern, glob) -> list<location>`), `file-changed` / `file-deleted` events, or persistent per-plugin caches larger than 1 MiB storage |
| Keeping the index fresh | Subscribes to every `file-opened` and `file-saved` | Wakes the plugin for every file the user opens or saves in any project; events can't be filtered by glob or language | `subscribe(kind, glob)` or `on-event:file-saved:<glob>` |
| Environment variable usages in code (PHP, Python, Java, Kotlin, JS/TS, Go, Ruby, Rust, C#, PHPUnit XML) | Text patterns before a string literal or member (`getenv(`, `$_ENV[`, `os.environ[`, `process.env.`, `env::var(`, `<env name=`…, `src/code.rs`) | The original resolves PSI (`Dotenv.get` only on the `Dotenv` class, `std::env::var` through name resolution); here any `…env(` call matches (`setenv`, `putenv`), keys built at runtime are missed, strings in comments count | Read-only access to the IDE's tree-sitter syntax tree of a document (`syntax.tree(doc)` or tree queries) |
| Find usages of a key | ⌘B (`definition`) on a key in a `.env` file returns its usages, the original's Ctrl+Click on a declaration | A flat list of locations: no usage view, grouping or preview | `references` provider |
| Rename a key and its usages | After the key is edited (and before saving), a `refactor.rewrite` code action "Rename usages of OLD to NEW" edits every usage, `${OLD}` included, in one change. The old name is the saved file's key on the same line, only when the value and the number of keys are unchanged | Only before saving; can't rename from a usage in code; misses a rename done together with other edits on the line | `rename` provider and an input dialog (`ui.show-input`) |
| Docker and docker-compose variables (`DockerfileVariablesProvider`, `DockerComposeYamlVariablesProvider`) | Line-based parsing of `ENV` and `environment:` (`src/docker.rs`) | Flow collections must fit on one line; YAML anchors, aliases and merge keys are skipped, not resolved; no access to the IDE's parsed YAML or Dockerfile | Syntax tree access, as above |
| Completion of `${KEY}` from `$` | Typing `"$DB` offers `${DB_HOST}` (`filter_text = "$DB_HOST"`) | The popup opens on the letter after `$`, not on `$` | `trigger-characters` |
| Settings `completionEnabled` and `storeValues` | `[contributes.settings]` booleans `completion` and `showValues` | `storeValues` controlled what IntelliJ writes to its on-disk index; here values are only in memory, so the setting just hides them in completion | — |

## aritti.spring-beans (clean-room, inspired by com.intellij.spring)

### Missing API

| Feature missing or partial | Why | API it would need |
|---|---|---|
| SQL/JPQL/HQL and SpEL highlighting, completion and errors inside Java/Kotlin strings (`@Query`, `@Value("#{…}")`, `@PreAuthorize`) | No language injection: a plugin can't mark a range of a host file as another language. `syntax-provider` only runs for languages a plugin owns, never for `java`/`kotlin`/`xml` | An injection extension point: (file, range) → language id, so the embedded language's highlighter, completion and diagnostics run on that range |
| Beans from libraries and Spring Boot auto-configuration; `application.properties` key completion from `spring-configuration-metadata.json` | `Project::read_file` reads UTF-8 text inside the project root only. Jars, `~/.m2` and `~/.gradle` can't be read, and there is no classpath API | The project's resolved classpath, plus reading entries of jars on it (or a host-side index of their classes and `META-INF` resources) |
| Correct type matching: generics, same-named classes in different packages, inherited library types (`MailNotifier extends JavaMailSenderImpl`) | No access to the IDE's code model or to jdtls results. Types are matched by simple name, in project sources only | A type-resolution query (position → qualified type, supertypes, members), backed by the language server the IDE already runs |
| Which candidates are actually active (`@ConditionalOn…`, `@Import`, component-scan packages; `@Profile` vs the run configuration's profiles) | No build model (dependencies, modules, source sets) and no run-configuration data. Change Active Spring Profiles is the plugin's own setting and feeds only the duplicate-name check | Project model API: modules, source roots (main/test/generated), dependencies; run configurations and their arguments/env |
| Generated sources and annotation-processor output (`target/classes/META-INF/spring-configuration-metadata.json`, Lombok/MapStruct output) | `find_files` skips hidden and gitignored paths (`target/`, `build/`); `read_file` works only with a known path | A `find_files` option to include ignored paths, or the generated source roots from the project model |
| Hover documentation: bean type, scope, profiles, candidates; SpEL variable types | No hover provider | Provider kind `hover`: (document, position) → markdown |
| Find usages of a bean, renaming a bean everywhere (`@Qualifier("x")`, XML `ref="x"`, SpEL `@x`) | No references or rename provider | Provider kinds `references` and `rename` (with workspace edits across files) |
| Choosing between candidate beans from the provider-path gutter | `LineMarker` has one `target`, so a provider marker with several candidate beans opens only the first (the document indexer's markers do open a chooser, see below). Icons are a fixed allowlist (`navigate`, `implemented`, …), so there is no bean icon or Spring-specific icon | `LineMarker` with several targets (shown as a chooser) and plugin-supplied icons (SVG from the package) |
| Advised-method markers (which methods a pointcut matches) and full AspectJ pointcut checking | Matching `execution(* com.acme..*Service.*(..))` needs every method's signature and the type hierarchy, libraries included | Same as type resolution, plus member/signature queries over the classpath |
| "Create bean class" / "Create missing XML bean" quick fixes | Code-action edits can only change files that already exist | Code-action workspace edits that create files (and open them). A command could write the file through `process:tee` |
| Live index after external changes (git checkout, generated files, edits in another tool) | Events cover only files opened/saved in the IDE; there is no file-system watcher event. The index re-scans every 120 s instead | A `files-changed` event (created/changed/deleted project paths) |
| Indexing large projects without blocking | Editor providers have 50 ms and commands 5 s; there is no background job a plugin can run between calls. The index reads files a little at a time inside provider calls, so markers show up only after a few calls | A background task API (runs outside provider budgets, can be cancelled) or a host-side text index the plugin can query |
| Native bean dependency diagram (zoom, layout, export) | No diagram/graph view type; the graph is drawn by hand as SVG in a sandboxed `Html` view | A graph view body (nodes, edges, layout) like the IDE's own diagrams |

### Implementation limits (could be done with the current API)

- Kotlin: SpEL, pointcut and bean-name string sites are only handled in Java files. `@Bean fun x() = …` without a declared return type is typed only for a bare `Foo(…)` constructor call (`someCall()`, `foo.bar()`, `Foo().with()` and `com.acme.Foo()` stay untyped); `val x = Foo()` has no type.
- No "ambiguous candidates" or "no bean of this type" warnings: without library and auto-configuration beans they would be mostly false positives.
- "Use constructor injection" adds a parameter and `this.name = name;` to a single existing Java constructor, or creates one when there is none. It refuses several constructors, `@Autowired`/`@Inject` setters, `this(...)` delegation, a field that is already assigned, a parameter name clash and files whose braces don't balance (braces in strings and comments count, so such a file just loses the fix). There is no Kotlin quick fix.
- XML: only `<beans>` files; custom namespaces (`<int:channel id>`) are seen as named beans without a type. `p:name-ref` and `c:name-ref` attributes are followed when the prefix is bound to the Spring `p` or `c` namespace on the element or an ancestor; `p:name="literal"` and other namespaces' `x:thing-ref` are ignored.

### Added by the feasible-parity pass (2026-11-06)

New codes, Java only, project sources only, published on open and save through `events` (optional: a denied subscription logs a warning and the checks still run from the line-marker path). Each group has a `springBeans.*` setting.

- `non-proxyable-method` (`@Transactional`, `@Async`, `@Cacheable`, `@Retryable`, security annotations on a private, final or static method), `scheduled-signature`, `duplicate-bean-name`, `configuration-proxy-method`, `bean-name-convention` (hint), `self-invocation` (off by default), and opt-in `unresolved-depends-on` and `component-scan-package-missing` (`checkUnresolvedReferences`, default off).
- **Limits:** duplicates see project beans only (no library or auto-configured beans); computed names and `@Conditional*` beans are skipped, but Kotlin and XML beans never set the conditional flag, so a Kotlin `@ConditionalOn*` bean can still clash with a Java one; a profile written `!x` is treated as not overlapping; other files' computed stereotype names read as the default class name, so a clash with a class of that name can be a false positive. Self-invocation misses inherited callees (same-class overloads are handled since the backlog pass), skips classes with `extends`, and cannot see `@EnableAspectJAutoProxy(mode = ASPECTJ)`, where private and self calls do work. The proxy checks do not require the class to be a bean. Unresolved references are quiet when the project uses auto-configuration unless the name equals a project class or bean; XML component-scan and Kotlin annotations are not checked. The edited, unsaved file is what the check sees.
- **Not done then:** Kotlin diagnostics from this pass (the `kotlin-spring` plugin check below was the first; Kotlin checks were added later, see the backlog and audit v2 blocks).

### Added by the gap F pass (2026-11-06)

Plan: [docs/superpowers/plans/2026-11-06-gap-f-spring-core.md](docs/superpowers/plans/2026-11-06-gap-f-spring-core.md). Same conventions as above (on by default, a setting each, quiet on dynamic input), tested against the mock host only.

- **XML** (`checkXml`, `checkAop`): `aop-structure`, `unsupported-pointcut-designator` (XML and Java `@Aspect`), `xml-bean-attributes`, `xml-injection-value`, `xml-scope`, `xml-abstract-reference`, XML `duplicate-bean-name` and `bean-name-convention`, the `xml-value-style` hint with quick fixes, and three intentions (p-namespace, introduce bean, update schema). XML diagnostics are published when an `.xml` file is opened or saved.
- **Code:** `static-injection`, `static-spel-injection`, `autowired-constructors` (`checkInjectionAnnotations`); `profile-expression` (`checkProfiles`); `async-return-type` (`checkAsync`); `cache-names-missing`, `cacheable-and-cache-put`, `cache-annotation-on-interface` (`checkCache`); `kotlin-spring-plugin` (`checkKotlinSpringPlugin`, Gradle quick fix only; Maven gets the diagnostic without a fix).
- **Narrowed on purpose:** scopes are flagged only as case variants or one-edit typos of known scopes; abstract beans are checked for `ref`-style references only, not `parent`; XML-versus-XML name clashes across files are quiet (a later file may override), dynamic profiles (`${...}`, `#{...}`) never overlap; cache checks are quiet when the class has a supertype, and the interface case is a hint that skips Spring Data repositories; the designator check is quiet when the module build, an ancestor build or `META-INF/aop.xml` signals AspectJ weaving.
- **Skipped:** `@EventListener` on a non-public method (Spring documents no rule).
- **Limits:** p-namespace skips dotted or dashed names and non-self-closing properties and has no `c:` form; introduce bean needs a `class`; `@Async` is checked on methods, not classes; static members are detected by the modifier; `@Resource`/`@Inject` count when imported from jakarta/javax or not imported; meta-annotations are not followed; the Kotlin build check is text-based (a commented-out line can count; a warning in another open `.kt` file stays until it is saved); aop.xml is read only from `src/main/resources/META-INF`; the import-check and XML helpers still have duplicated copies (refactor debt, no user effect).
- **Unverified in the IDE:** file events for `.xml` and `.kt`; code actions on XML (multi-edit order, `refactor` kind, an edit in a file other than the active one); the Gradle fix; diagnostic placement on annotation values, string literals and return types; `project.root()` with a trailing slash; real-host `read_file` of build files; introduce bean latency on a large index.

### Added with the document indexer (API 1.2)

`[[contributes.documentIndexers]]` (`java`, `kt`, `xml`) publishes `bean#name` keys (names, aliases, qualifiers of `@Component`-family classes, `@Bean` methods, Spring Data repositories and XML `<bean>`) and `type#Simple` keys (the bean's type and its direct supertypes from the same file). References are `@Qualifier`/`@Named`/`@DependsOn`/`@Resource(name)`/`getBean` strings (Java), XML `ref`-style attributes and `depends-on` lists, and untyped-by-name injection points by type. The host answers Go to Declaration and gutter markers (injection point to bean) project-wide from memory, so they no longer depend on the 50 ms provider budget or on the plugin's own scan.
- **Limits:** supertypes are linked through aliases published from every file's type declarations (not verified in the real host); name strings in Kotlin files are not references (the Kotlin site scanner does not exist); the bean-to-injection-points marker stays on the provider path because index markers go only to definitions; `@Primary`, profiles and `@Conditional*` are not part of keys, so the chooser lists every candidate.
- **Unverified in the IDE:** the real host's batching and invalidation; the combined providers, views and indexing component (built by hand because `register_plugin!` has no arm for it).

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Commands:** Check Spring Configuration and Dump Spring Configuration (Tools menu, results in the *Spring Configuration Report* view; the check runs over the indexed project files, so it is as incomplete as the index), Move Annotation String to Property (editor menu, `pick` the properties file). Generate Setter, Constructor, Autowired Dependency and Event Listener (editor menu).
- **Inspections:** direct `@Bean` calls in `proxyBeanMethods = false` classes, `@Cacheable` SpEL bracket and attribute conflicts, `initMethod`/`destroyMethod`, `@TestBean` and `@Required` targets, same-class overloads in self-invocation (reported only when every overload carries the annotation), XML `component-scan` filters, missing `<import resource>`, bean class checks (abstract, `factory-method`, `constructor-arg` count, init/destroy, `@Required`; skipped when a supertype is outside the project), inactive profile blocks, `META-INF/spring.handlers`/`spring.schemas` checks. Opt in (`springBeans.checkUnresolvedReferences`): unresolved `@Lookup`/`@Scheduled(scheduler)`/`@MockitoSpyBean` names, missing `@ImportResource`/`@PropertySource`/`@ContextConfiguration`/`@Sql` files (read from `src/main/resources` and `src/test/resources`; quiet for `classpath*:`, placeholders, wildcards), and unresolved XML `ref`/`parent`/`depends-on`/placeholders.
- **Navigation:** setter completion and definition for `<property name>`, index keys for profiles, resource files, packages and `@Value` placeholders (added, none renamed), markers between event publishers and listeners and between classes and XML beans, project-wide Go to Declaration from the host's document index.
- **Kotlin:** redundant `open` with kotlin-spring (Hint), router `pathVariable`, `getBean(Foo::class.java)` hint.
- **File templates:** component, service, configuration, Boot application, Boot test, configuration properties (Java and Kotlin). Not verified: `$PACKAGE$`, `$NAME$` and `$END$`.
- **Skipped:** the snippet items of the TPL package (completion snippets). Not verified in the IDE: all of the above.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Done in this round:** Spring Config, Spring Config with Component Scan and Spring Config with AOP XML file templates; live templates as snippet completions (Java `scomp`, `sserv`, `sconf`, `sbean`, `sauto`, `sval`, `sprops`, `slisten`, `ssched`, `stest`; Kotlin and beans-XML sets, XML only outside tags of a beans file); some XML checks (`xml-import-not-beans`, include/exclude filter expressions, `lookup-method` and `replaced-method` names, factory-method visibility, unresolved class) and `resource-not-found` for classpath resources.
- **Stopped midway:** round 2 of audit-v2-A1 was interrupted. The remaining rows are not done and the plugin's `change-notes` were not updated. `spring-osgi` is untouched.
- **Not verified in the IDE:** all of the above; which of the XML checks are on by default was not checked.

## Spring plugins (aritti.spring-*: ports of com.intellij.spring.*)

Covers `spring-beans` (com.intellij.spring), `spring-boot`, `spring-initializr`, `spring-data`, `spring-security`,
`spring-web` (com.intellij.spring.mvc), `spring-websocket`, `spring-messaging`, `spring-graphql`, `spring-cloud`,
`spring-integration`, `spring-batch`, `spring-webflow`, `spring-shell`, `spring-ws`, `spring-modulith`, `spring-osgi`
and `spring-runtime` (stands in for com.intellij.spring.debugger). All but `spring-osgi` are clean-room: the JetBrains
plugins are closed-source. The shared text scanners live in `crates/spring-text`. `spring-beans` also has its own
section above, with its plugin-specific details.

### Gaps that affect every Spring plugin

| Limitation | Effect on the Spring plugins | API that would remove it |
|---|---|---|
| No access to the Java/Kotlin semantic model. The IDE runs JDT LS, but plugins can't query it: no type resolution, type hierarchy, symbol search, classpath or annotation meta-model | Everything is text-based. Types match by simple name as written, supertypes are known only for project sources, and meta-annotations (custom `@Service`-like stereotypes, composed `@GetMapping`) are invisible. Beans from libraries and auto-configuration can't be seen. "Could not autowire", "no bean of type X" and ambiguity checks are left out because they would mostly be false positives; other checks stay silent unless every type involved is a project class | `lsp.request(language, method, params)` to the IDE's language servers, or a `java` interface: `resolve-type(path, offset) -> fqn`, `supertypes(fqn)`, `annotations(fqn)`, `find-types(query)` |
| Dependency jars and the classpath can't be read. `read_file` takes UTF-8 project files only, and jars are binary and outside the root | `META-INF/spring-configuration-metadata.json` of dependencies is unreachable, so `spring-boot` bundles Spring Boot 3.5.16 and Spring Cloud 2025.0.3 metadata, which is version-pinned. Keys of other starters (third-party, Cloud modules outside the train list) get no completion or validation. Library XSDs, `spring.factories` and `AutoConfiguration.imports` can't be read either | `project.classpath() -> list<string>` plus `read-archive-entry(archive, entry) -> result<list<u8>>` (or `read-file-bytes`), gated by `project.read` |
| No language injection | No SpEL, JPQL/HQL/SQL, AOP pointcut, cron or security-expression *language* inside Java strings and XML attributes: no highlighting, parsing, brace matching or formatting. Only completion, go-to-definition and diagnostics computed by hand inside the literal are possible | An `injections` contribution or provider: `(host language, range) -> injected language id`, so the injected range gets the injected language's grammar and providers |
| No hover / documentation provider | Property descriptions, defaults and deprecation notes, bean details, resolved queries and runtime values can't be shown on hover. Only line-marker tooltips (one line per marker) and completion documentation are available | A `hover` provider kind: `hover(doc, at) -> option<markdown>` |
| No inlay hints or editor decorations | Spring Debugger-style inline values (effective property value next to `@Value`, resolved bean next to an injection point) and query previews can't be drawn in the text. Line-marker tooltips are used instead | An `inlay-hints` provider kind: `inlay-hints(doc, range) -> list<(position, label, tooltip)>` |
| No references (Find Usages) provider | Bean usages, channel/destination usages, GraphQL field usages and Batch step references can't join Find Usages. Go to Definition is the only cross-reference. The IDE's own config-key usages are not extensible | A `references` provider kind: `references(doc, at) -> list<location>` |
| No rename / refactoring provider | Renaming a bean, property key, channel, destination, step id, flow state or command name doesn't update the string references | A `rename` provider: `prepare-rename(doc, at)` and `rename(doc, at, new-name) -> workspace edits` |
| Code actions can only edit existing files. No file creation from a code action; commands can write through the optional `process:tee` (spring-web does for HTTP request files) | No "create missing template / view / package-info.java / schema", no generators (Spring Data repository for an entity, controller from an entity). The Modulith "open module" fix only works when the target `package-info.java` already exists | Resource operations in code-action edits (`create-file(path, text)`), or `workspace.create-file` under a write capability |
| Line markers: one target each, and icons from a fixed allowlist (`implemented`, `implementing`, `overridden`, `overriding`, `run`, `navigate`) | Several candidate beans, listeners, producers or handlers per line: a provider marker opens the first one and the tooltip lists the rest (Go to Definition shows them all); markers from the document indexer (`spring-beans`) open a chooser. No Spring bean, autowire, endpoint or channel icons | A `targets: list<location>` field on line markers (the IDE shows a chooser) and package SVG icons for markers |
| No popup, quick pick or input API, and a command can't open or focus a view | Closed for commands in API 1.2 (`host::pick`, `pick_many`, `input`, `confirm`: used by the Generate commands, Change Active Spring Profiles, Check Spring Configuration, Move Annotation String to Property). Still open: they return `Invalid` in code actions, providers and events, so a quick fix can't ask; diagram and tool views can't be opened from a gutter or quick fix | `pick` allowed in `resolve_code_action`, `views.reveal(view)` |
| No structure-view / outline provider for built-in languages. The syntax provider's `outline` is only called for languages a plugin owns | Spring Modulith module structure, Web Flow states, Batch jobs/steps and Integration channels can't appear in the Structure tool window for `java` / `xml` files. Tool windows are used instead | An `outline` provider kind for any language id (merged with the built-in outline) |
| No Project view decorations | Spring Modulith open/closed module badges, bean/configuration file icons and "Spring facet" markers in the Project view are missing | A `project-view-decorations` provider: `(path) -> badge, icon, tooltip` |
| The built-in Endpoints tool window and HTTP client aren't extensible | GraphQL operations, STOMP destinations, Feign clients and messaging destinations can't be listed in Endpoints, and there is no "generate request" for GraphQL. Plugin tool windows are used instead | An `endpoints` provider contribution (kind, URL/destination, location) feeding the Endpoints view and the HTTP client |
| No diagram API | Bean dependency, Web Flow, Integration, Batch job and Modulith graphs are drawn as inline SVG in an `Html` tool-window body (`spring_text::diagram`): a fixed layered layout, no zoom/pan beyond scrolling, no export, no editing, node click → `open_location` | A graph view body (`Body::Graph { nodes, edges }`) rendered by the IDE with layout, zoom and export |
| No debugger API | Spring Debugger cannot be ported: no bean context or watches during a debug session, no evaluating bean methods or properties at a breakpoint, no transaction or DB-connection state | Debug-session events (`debug-paused`, frames), `debug.evaluate(expression)` and a variables/watches contribution |
| `net:` hosts are fixed in the manifest, and plain HTTP is allowed to `localhost` only | `spring-runtime` reads Spring Boot Actuator only from `localhost` (`net:localhost`, optional `net:127.0.0.1`), with a bearer or Basic token when the `actuatorAuth` setting asks for one. An app in a container on another host, on a remote dev machine or behind HTTPS with a custom host can't be read | A capability for user-entered hosts (`net:user`, approved per host on first use) |
| No database API | Spring Boot's datasource → database connection, JDBC driver checks and the schema behind Spring Data entities aren't available | `database.add-connection(url, user, driver)` / `database.schema(connection)` |
| No new-project or open-project API | `spring-initializr` can only open start.spring.io (`/starter.zip` downloads in the browser) or run the Spring Boot CLI in a terminal; the project must be unzipped (browser) or found in the directory the CLI wrote, and opened by hand | A new-project-wizard contribution, or `workspace.open-project(path)` plus file creation |
| Run configurations: `runDetectors` have a fixed command and arguments | Profile-, environment- or main-class-specific Spring Boot runs can't be offered by a plugin. The IDE's built-in Spring Boot run configurations are used | Run configurations computed by code (`run-configurations(project) -> list<…>`) or arguments taken from settings |
| No change events for files that aren't open in an editor (only `file-opened`, `file-saved`, `editor-focused`, `vcs-changed`, `action-performed`) and no persistent index | Each plugin rescans its globs at most every 120 s (`spring_text::index`) and applies the open document on top. A checkout, a generated file or a change made outside the IDE shows up late. Cross-file warnings are held back until the first full pass is done | A `files-changed` event with paths (created, changed, deleted), and per-project index storage |
| `find_files` skips hidden directories and `.gitignore`d paths such as `target/` and `build/`; the mock host doesn't, so tests pass anyway (see memory: find-files-skips-hidden) | Build-output metadata such as `target/classes/META-INF/spring-configuration-metadata.json` (generated by the configuration processor) isn't found by globbing. Known paths have to be probed with `read_file` | An `include-ignored` option on `find_files` |
| Diagnostics have no provider of their own: they are published with `publish_diagnostics`, here from the `line-markers` call, and there is no project-wide inspection run | Problems appear only for files that are open. No "Inspect Code" over the whole project, and no batch fix | A `diagnostics` provider kind called on open/change, and a project inspection hook |
| Editor providers get 50 ms and run in the plugin's single instance, queued behind commands and views | Large projects are indexed a little at a time (30 ms per call), so the first results after opening a project are incomplete. A long view render or refresh delays providers | Background work that doesn't block providers (a separate indexing instance or async calls, planned with WASI 0.3 / API 2.0) |
| A plugin can't call another plugin | Each Spring plugin keeps its own index of the same Java files. Configuration-key knowledge (`spring-boot`) can't be reused by `spring-cloud` and the others | Plugin-to-plugin services (`requires = ["aritti.spring-beans"]` plus exported functions) |
| Settings are scalars only (`boolean`, `string`, `number`, `integer`) | Lists such as extra stereotypes, extra broker prefixes or ignored bean names would need a multiline string. None are offered yet | `array` / `object` setting types |
| `host::storage` is 1 MiB per plugin, and starts empty for remote projects on `openidea-server` | `spring-runtime` stores a trimmed Actuator snapshot, and only beans + env when it's too large | Per-project storage, or a larger quota |

### Implemented with a workaround

| Feature of the original | Workaround | Limitation |
|---|---|---|
| Spring Debugger and the Spring Boot run dashboard's live data | `spring-runtime` fetches `/actuator/beans`, `/env`, `/conditions`, `/health`, `/mappings`, `/configprops`, `/scheduledtasks`, `/loggers` (ROOT and loggers with a configured level) and `/caches` over `net:localhost` (command or view Refresh only, never inside an editor provider). Results show in a tool window (groups include Loggers and Caches) and as line-marker tooltips. Endpoints behind a bearer or Basic token work through the `actuator-token` secret and the `actuatorAuth` setting (`none`, `bearer`, `basic`); the plugin never reads the token. A 401/403 adds "check actuatorAuth and the actuator-token secret" to the status line, and a denied `secrets` capability says so | The app needs `spring-boot-starter-actuator`, the endpoints exposed and, in Boot 3, `management.endpoint.env.show-values=ALWAYS` for unmasked values (development only). The data is a snapshot, not live debugger state. OAuth flows and client certificates are not supported. Loggers and caches are kept in memory only: the stored trimmed snapshot keeps beans and environment, so they come back after a restart only when the whole snapshot fits in 900 KiB. Cache items open nothing. Secret substitution was only exercised against the mock host |
| Spring Boot property completion and validation | Bundled Spring Boot 3.5.16 and Spring Cloud 2025.0.3 metadata (Apache-2.0; about 3,800 keys, 1,290 of them Cloud) plus the project's own metadata files and `@ConfigurationProperties` classes | Pinned to one Boot and one Cloud train. No starter metadata from jars, and starters that ship none (Kafka, AMQP, Session, OAuth2) rely on Boot's own auto-configuration metadata. "Unknown key" is only reported inside groups known to that metadata, which now includes a typo such as `spring.cloud.gateway.<typo>`. Map properties complete with a trailing dot (`eureka.client.service-url.`) |
| SpEL, security expressions, JPQL, AOP pointcuts and cron | Completion, go-to-definition and diagnostics computed in the string literal by the plugin | No highlighting or real parsing. Concatenated strings and constants are skipped |
| Bean, flow, integration, batch and module diagrams | Inline SVG in an `Html` tool-window body. Web Flow's existed first; `spring-integration` (Integration Channels: channels and endpoints, input channel to endpoint to output channel; endpoints of one kind are told apart as `(in -> out)`), `spring-batch` (Batch Jobs: jobs, steps, flows, splits and decisions with `next`, `on=`/`to=` and restart transitions) and `spring-modulith` (Module Diagram: modules, dependencies, violations as warning edges, and the command Copy Module Diagram (PlantUML) to the clipboard, optional) gained theirs on 2026-11-06. The diagrams render; clicking a node does not navigate in the IDE today (unverified: node clicks go through an inline script, which the desktop IDE is documented to block) | See "No diagram API" above. Integration: Java DSL chains (`IntegrationFlow.from("a").channel("b")`) show channels without endpoint nodes; Kotlin DSL is not read. Batch: transitions are drawn from XML only, Java `.next()` chains are not, and edges resolve only to states in the same file and scope. Modulith: no C4 or module canvas, no Documenter file export, no event publication registry, no import cycle detection. While the index is incomplete the views refresh themselves; that branch was not exercised by a test (the mock's budget always finishes the scan) |
| Spring Initializr new-project wizard | A `Form` tool window that builds a start.spring.io URL and opens it in the browser (default and fallback). A second button, Generate with Spring Boot CLI, runs `spring init … --extract ./<artifactId>` in an IDE terminal through the optional `process:spring`; every value is passed as its own argument and validated (no leading `-`, coordinates and versions limited to letters, digits, `_`, `.`, `-`, a dot-only artifact refused); without the capability a warning names `process:spring` and the browser fallback | No project creation or import in the IDE. The plugin can't see the CLI's result or open the generated project. The `spring` CLI must be on `PATH`, and the extract directory is relative to the terminal's directory. Whether the UI joins the arguments into a shell line was not verified |
| Bean navigation everywhere (injection points, XML `ref`, `@Qualifier`, SpEL `@bean`) | A per-project text index of Java/Kotlin stereotypes, `@Bean` methods and XML `<bean>`s | Library and auto-configured beans, `@Import`/`@ComponentScan` filters, profiles and conditions aren't evaluated |
| Kotlin | `spring_text::kotlin` scans Kotlin into the same declaration model as Java. `spring-beans`, `spring-data`, `spring-web`, `spring-security`, `spring-graphql`, `spring-cloud`, `spring-shell`, `spring-ws`, `spring-messaging` and `spring-websocket` index `*.kt`; Kotlin strings with a `$name` or `${…}` template are not treated as plain literals (a `\$` or a `$` before a digit, a space or the end stays plain) | Text-based only: no type inference, so `val x = Foo()` has no declared type, and DSL lambdas (`beans { }`, `http { }`) are only partly understood (`coRouter { }` is read by `spring-web`). Nested annotations in `spring-messaging` (`queuesToDeclare`, `bindings`) are Java-only, `spring-ws` still reads `namespace = "$x"` as a literal, and constants (`@RabbitListener(queues = [QUEUE])`) are unresolved. Indexing time with `**/*.kt` added was not measured on a large mixed project |
| JPQL entity and alias-property checks in `@Query` (`spring-data`) | Entity names after `from`/`join`/`update` and alias properties (`u.address.city`, joins and nested paths resolved) are completed. Checks run only once the project index is complete: an entity name warns only as a likely typo (at least 5 characters and 1 edit, or at least 8 and 2) of a project entity or class, and `alias.property` warns only when the whole type chain is project classes. Kotlin repositories, entities and `@Query` (raw strings too) are supported | Library entities, native queries (`nativeQuery = true`, `@NativeQuery`), SpEL, concatenated or multi-literal values, qualified names and function `from` (`extract`, `trim`) stay quiet. Subquery scoping is partial (an alias declared twice with different types is dropped) and `select new` aliases are not covered. An entity from a library or `orm.xml` can still give a false typo warning when the domain class is a project class. Not done: query language injection and highlighting, query preview, Mongo JSON checks |
| WebFlux functional routes and test URL links (`spring-web`) | Functional routes (`route().GET("/x", h)`, `RequestPredicates`, `.andRoute`, `.path`/`.nest` with a plain path prefix; Kotlin `router {}` / `coRouter {}` including `"/a".nest`, infix `nest` and `path("/a").nest`) are indexed as endpoints with the annotated mappings. Handler references (`this::list`, `Handler::list`, Kotlin `::list`) get a gutter arrow and Go to Definition. In files under `/src/test/`, MockMvc calls (`get("/u")`, `mockMvc.get`), `webTestClient.get().uri("/u")` (also `WebClient`, `RestClient`) and RestTemplate calls link to the mappings that serve them by verb and URI template (`{id}`, `{id:regex}`, `*`, `**`, `{*rest}`); Go to Definition lists every match and the marker targets the first and says "(+N more)"; HEAD falls back to GET | Routes under a `nest` whose predicate isn't a plain `path(...)` are skipped. `h::create` on a variable isn't resolved. Only literal paths count (a mapping with a constant path isn't an endpoint); absolute URLs are skipped and regex constraints ignored; RestTemplate calls need a receiver whose name contains `template` or `rest`; links appear only when a full index pass has completed. URL completion in tests is still not ported, and annotated mappings get no gutter marker of their own |
| XML namespace and Kotlin DSL checks (`spring-security`) | The security XML namespace is checked on `.xml` files (prefix-agnostic): unbalanced quotes or parentheses and `hasRole`/`hasAnyRole`/`hasAuthority`/`hasAnyAuthority`/`hasIpAddress` without an argument are errors; an unknown `has*`/`is*` function, a redundant `ROLE_`, a duplicate `(pattern, method, servlet-path)` within one `<http>`, a `/**` catch-all placed before another pattern, `use-expressions="false"` and `global-method-security` are warnings. Kotlin sources get the Java checks, plus the trailing-lambda `authorizeRequests {` with a rename fix; comments, strings, raw strings and char literals are blanked first | The unknown-function warning can be a false positive with a custom `SecurityExpressionRoot` or handler that adds its own `has*`/`is*` methods. The catch-all check is skipped when the catch-all has a method or servlet path, when `request-matcher` isn't `ant` or `mvc`, or when `request-matcher-ref` is set; `access` isn't checked when `use-expressions="false"`. The Kotlin `authorizeRequests` scan is textual and gated on an `org.springframework.security` import. Completion is Java-only and there are no quick fixes for the XML checks. No SpEL highlighting or navigation, no OAuth2 or SAML assistance |
| JavaScript and TypeScript STOMP clients (`spring-websocket`) | `.js`, `.jsx`, `.ts`, `.tsx` and `.vue` files are scanned for `subscribe`, `publish` and `send` with a plain literal destination starting with `/` (`node_modules`, `dist` and `build` paths are skipped). A client subscription links to `@SendTo`, `@SendToUser`, `convertAndSend` and `@SubscribeMapping`; a client send links to `@MessageMapping` (with the app prefix from the config). The Java marker's tooltip adds "also used at file:line" and its target is the first related use | The receiver must be named like `stomp` (the name contains `stomp`) or be assigned from `new Client(`, `new StompJs.Client(`, `Stomp.over(` or `Stomp.client(` in the same file (typed declarations `const client: Client = new Client(` and generics work); a client created in another file than the call is not recognised. Destinations built at run time (`${}`, concatenation) are skipped. One navigation target per marker, the tooltip lists the rest, and completion inside client literals offers only same-kind destinations. Large JS projects may hit the file cap or the 50 ms budget |
| `${…}` placeholders and XML listeners (`spring-messaging`) | `${k}`, `${k:default}` and nested placeholders (depth 2) in `@KafkaListener`, `@RabbitListener`, `@JmsListener` and `@SendTo` (Rabbit `@SendTo` excepted) resolve from `application*`/`bootstrap*` `.properties`/`.yml`/`.yaml` with relaxed keys. `<jms:listener destination>` and `<rabbit:listener queue-names>` (namespace checked, comma lists split) become consumers and link by destination | `application.*` beats profile files and there is no active-profile knowledge; an unresolvable or comma-list result is dropped. Only `queue-names` is read for Rabbit (`queues` holds bean refs), and `ref`/`method` are not linked to beans. Still not ported: SpEL, constants, `BindingBuilder`, Spring Cloud Stream. XML markers need the host to send `xml` documents to the plugin's providers, which was not verified |
| GraphQL client documents and Kotlin controllers (`spring-graphql`) | Go to Definition on a field in `.graphql`/`.gql` files under `graphql-documents/` and in `.document("…")` literals (Java and Kotlin, text blocks too) resolves the root field through the schema's root types (including `schema { query: X }`), nested fields by declared type, and adds the controller method for root fields; aliases, fragments (`fragment X on T`), mutations and subscriptions are handled. Kotlin controllers are indexed | No diagnostics and no completion inside client documents; an unknown field is quiet. Only documents under `graphql-documents/` count (other `.graphql` client files are not treated as documents); inline documents with backslash escapes are skipped; the `.document(` receiver is not checked |

### Not API limits: left out or not finished yet

These could be done with the current API; they are listed so they aren't mistaken for API gaps.

- Kotlin support is in `crates/spring-text`. The plugins listed in the *Kotlin* row above index `**/*.kt`; each plugin README says what its Kotlin support covers, and `spring-osgi` (XML only) and the plugins not listed there were not part of the 2026-11-06 sweep.
- The phase-2 items that were planned and unfinished on 2026-11-05 are done (see the rows above): JPQL checks in `@Query`, the security XML namespace, WebFlux functional routes, JavaScript STOMP clients, messaging XML listeners and placeholders, GraphQL client documents, bundled Spring Cloud metadata, Spring Boot CLI generation, the Integration, Batch and Modulith diagrams (Web Flow's existed) and the Modulith PlantUML copy. They are checked against the mock host only, not in the IDE.
- Deliberately skipped because they would mostly be false positives without a semantic model: unresolved-autowiring checks, "missing view" warnings, unmapped GraphQL fields above `Hint`, and messaging destinations nobody produces or consumes.
- Spring OSGi (source port): `osgi-attribute-value` (Error) checks `cardinality`, `greedy-proxying`, `ranking` and the other enumerated attributes; the original has no such check, so the values and messages come from the Spring DM reference, the check has no setting, and the host's rendering of the value range is unverified. Its checks that need real type resolution fall under the first gap above. `OsgiSpringBeanGenerateProvider` is a command, Generate OSGi Service or Reference Bean (editor context menu on XML): `pick` service or reference, `input` the interface (and the bean to export), then one `<osgi:service>` or `<osgi:reference>` line is inserted before the selection's line through `apply-edits`. It needs the osgi namespace declared in the file, adds no namespace, offers no interface completion and is not in a Generate menu. Prompts are only verified against the mock host.

### Added by the feasible-parity pass (2026-11-06)

New checks in the other Spring plugins (all on by default, each with a setting, published on open and save, quiet on library or dynamic input):

- **spring-security** (`checkMatchers`, `checkDebug`): `matchers-after-any-request`, `matcher-order` (a literal ant pattern shadowed by an earlier broader one; XML stays on the older `unreachable-patterns`), `debug-enabled`. Limits: literal ant patterns only, no cross-chain or cross-file analysis, no servlet-path or MVC semantics; `.debug(true)` warns in any file that mentions `WebSecurityCustomizer`.
- **spring-web:** `api-version-mismatch` is quiet unless every source and `application*` file was read and nothing mentions API versioning; `init-binder-signature` (Error for a non-void return). Java only.
- **spring-boot:** `test-constructor-autowire` cannot see system properties or environment set outside the scanned files and warns for constructor parameters such as `@TempDir` or `@Mock`; `event-listener-signature` reports more than one parameter and a plain String or primitive return.
- **spring-cloud:** `stream-channel-unbound` (hint) covers `StreamBridge.send("literal", ...)` and binder keys only, because `StreamBridge` creates bindings dynamically; legacy `@Input`/`@Output` channels are skipped. `bootstrap-config-missing-dependency` needs a literal Boot version of 2.4 or newer in every build file that states one.
- **spring-graphql:** the unmatched-mapping check is the existing `unknown-schema-field`, now behind `springGraphql.checkUnmatchedMapping`; it is quiet with no schema file or an unknown type.
- **spring-batch:** `chunk-missing-commit-interval` (XML only; `checkChunk`) is quiet with `parent` or `abstract` ancestors and `${}` values, and `abstract="false"` still suppresses it. The message was written from the reference documentation and not checked against the XSD. A Java `.start()`/`.next()` step-not-a-bean check is not done (helper methods and parameters make it false-positive prone).
- **spring-websocket:** the destination check is the existing `unrouted-destination` (`checkBrokerPrefixes`), only when broker prefixes are configured. A `@MessageMapping` without `@Controller` is not reported.
- **Shared:** `Finding` (range, code, message, severity) lives in `crates/spring-text`; the diagnostics carry no fix.

### Added by gap F, plan 2: rest of Spring (2026-11-06)

Checks and views for `spring-boot`, `spring-data`, `spring-web`, `spring-security`, `spring-graphql` and `spring-websocket` ([plan](docs/superpowers/plans/2026-11-06-gap-f-spring-rest.md)). Text-based like the rest, quiet on dynamic input, each with a setting. Nothing was run in the IDE.

- **spring-boot:** `config-properties-prefix` checks the prefix part of `@ConfigurationProperties` only. `early-event-listener`, `default-package-application`, `redundant-component-scan` and `redundant-enable-auto-configuration` are new. A file icon applies to `application*` and `bootstrap*` config files; it shows only under an icon theme that merges plugin icons (unverified), and the dashed-stroke gear SVG was never rendered, so at 16 px it may look like a dotted ring.
- **spring-data:** `page-without-pageable` flags a `Page` return without a `Pageable`; a `Slice` without one is valid and not flagged. A same-package custom `Page` with no import still warns. `query-needs-modifying` covers update and delete `@Query`. `jdbc-empty-sequence-name` checks only an empty `sequence` attribute of `@Sequence`. `mapped-collection-key-column` is a Hint.
- **spring-web:** `invalid-api-version` reads a lone literal in `version` of Java mapping annotations; Kotlin and functional routes are not covered. A bad literal can fire both `invalid-api-version` and `api-version-mismatch`. The `api_versioning || version_format` wiring has no test. The two generate actions (Java only) are refactor actions, do not move the caret and insert a placeholder `/path`; running one twice makes duplicate handlers. The Endpoints view lists literal annotated mappings and functional routes only. Copy Endpoint as curl (context menu of an endpoint item, optional `clipboard`) asks for the base URL (default `http://localhost:8080`) and copies `curl [-X VERB] '<base><path>'`; it does not read `server.port` or the context path, leaves `{variables}` unexpanded, sends no body or headers, and is not the IDE HTTP client. The `viewItem.context` menu was only verified against the mock host.
- **spring-security:** `obsolete-path-pattern` flags a `**` before the last path segment in `requestMatchers`, `mvcMatchers`, `securityMatcher` and Kotlin `authorize`. No quick fix. It can be a false positive when the project uses `ant_path_matcher`.
- **spring-graphql:** `batch-mapping-return-type` is a deny-list (Java and Kotlin). Raw `Mono` and custom wrappers stay quiet; `Collection` and `Set` are valid. A Kotlin expression body without a declared type is quiet, but a `= { ... }` lambda body still warns.
- **spring-websocket:** the WebSocket Destinations view is the plugin's own; a plugin cannot add items to the Endpoints view of `spring-web` (see the Endpoints row above).
- **Unverified in the IDE:** the removal quick fix on the annotation line; whether Java documents request code actions from `spring-boot`; the refactor actions with the caret in blank class space; how the Endpoints and Destinations views render.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **spring-boot:** `invalid-property-value` (`Integer`, `Long`, `Short`, `Double`, `Boolean`, `Duration`, `DataSize`; quiet for `${}`, `#{}`, lists, maps, enums and custom types); Go to Definition from `spring.config.import` (placeholders and other schemes skipped) and from class-valued keys; completion of logger packages (from `package` lines), profiles and `${}` keys; checks of `(additional-)spring-configuration-metadata.json` (unknown fields, malformed `type`, duplicates, missing description) with a define-key fix that appends to an existing additional file only (a quick fix cannot create one); Reactor debug agent hint; opt-in `springBoot.checkDriverDependency` (transitive drivers invisible); Spring Boot file templates. Not done: the `@EventListener` visibility rule (no documented source). Unverified: the `json` provider routing and the `$` placeholders.
- **spring-data:** `query-parameter-type` (Info; both sides well-known simple types of different kinds), return kinds per query verb, `mongo-unknown-field` (top-level keys of a Mongo `@Query`; nested documents not followed), `jdbc-shared-entity`, derived-query tooltips, Show Repository Inherited Members (static list of Spring Data's own interfaces), a repository file template.
- **spring-web:** URL gutter markers on mappings (no target; setting `springWeb.showUrlMarkers`), mapping URL completion in test URL literals, Go to Definition from static resource literals, `getm`/`postm`/`putm`/`delm`/`patm`/`reqm` snippets, Controller/REST Controller/Controller Advice templates (Java and Kotlin), Kotlin path variable checks, opt-in unresolved view hint (`springWeb.reportUnresolvedViews`), `<mvc:...>` attribute checks from a hard-coded table, `apiVersion()` hint in tests, Copy as curl on the Endpoints tree (optional `clipboard`). Unverified: completion inside string literals, the `xml` provider activation.
- **spring-security:** Info for bean references no project bean answers (`springSecurity.checkBeanReferences`), table checks of `<http>`, `<intercept-url>`, `<user>` and related XML, opt-in self-invocation of method-security methods (`springSecurity.checkSelfInvocation`, wrong with AspectJ mode), Go to Definition from request matcher patterns to the controller mappings they cover. Library beans are invisible, hence Info.
- **spring-cloud:** Feign Clients view, legacy `@StreamListener`/`@SendTo` channel checks (`springCloud.checkLegacyStream`, quiet unless every `@EnableBinding` type is a project interface or `Sink`/`Source`/`Processor`), hint for `@BootstrapConfiguration` classes that component scanning finds. Key hints (CLOUD-MISC) skipped: `spring-boot` covers them.
- **spring-graphql:** GraphQL Endpoints view (paths from `spring.graphql.*`, schema files), Info for schema fields with arguments that no controller or data fetcher serves (`springGraphql.reportUnmappedFields`).
- **spring-messaging:** destinations named by `static final String` / `const val` constants (project constants only), XML checks for Rabbit and JMS listener elements (hand-written tables), `ref`/`method` of XML listeners link to the bean and its methods.
- **spring-websocket:** `<websocket:...>` XML checks (required attributes, Info for unknown handler beans). **spring-batch:** required attributes and boolean values of batch XML elements (hand-written tables, not the XSD), `jobParameters['…']` key completion in XML and Java (unverified in Java strings). **spring-osgi:** unresolved `ref` as Info, `ref` completion sorts implementors of the exported interfaces first (never hides), `osgix` tag completion, Generate OSGi Service/Reference Bean in the XML editor menu.
- **Not verified in the IDE:** every item above (mock host only). Skipped or partial from the audit: Qute support (not in this repo), overload handling beyond same-class, rules with no public source.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **spring-boot:** *Show Configuration Problems* command (project configuration and metadata problems in a view), class, enum and classpath-resource completion in values, class, enum and list value checks, files named by `spring.config.import`, location and name, navigation from classpath resource values, constructor-binding checks for `@ConfigurationProperties`, deeper checks of metadata files (defaults, deprecation, unresolved classes, hint names). Boot row 16 is partial (not verified which part is missing).
- **spring-data:** parameter and return type checks for derived queries, MongoDB `@Query` field check and *Sort the query fields*, JDBC entity shared by two aggregate roots, derived query tooltips, *Show Repository Inherited Members* (Java and Kotlin), `repository` snippet and a repository file template.
- **spring-cloud:** Feign Clients view, legacy `@StreamListener`/`@SendTo` checks and `@StreamListener` signature rules, hint for `@BootstrapConfiguration` classes component scanning can find. Cloud row 6 skipped.
- **spring-web:** URL gutter markers, mapping URL completion in test URL literals, static resource Go to Definition, `getm`/`postm`/... snippets, Controller / REST Controller / Controller Advice templates, Kotlin controller checks, `<mvc:...>` attribute checks, *Copy as curl* (optional `clipboard`), and *Create HTTP Request File for Endpoint*, which writes `http/<verb>-<path>.http` through the optional `process:tee` (clipboard fallback). The unresolved-view hint is on by default but quiet when a source defines its own `ViewResolver`. Web 14 is partial.
- **spring-security:** wider hand-written XML tables, Info for bean references no project bean answers, opt-in self-invocation warning, Go to Definition from request matcher patterns to controller mappings. Security row 3 is partial.
- **spring-messaging:** simple SpEL destinations, constant destinations, Rabbit and JMS XML checks, `ref`/`method` links; Messaging 1 is partial. **spring-batch:** late-bound `#{jobParameters[...]}` error, more XML table checks, `jobParameters` key completion; Batch 1 is partial. **spring-initializr:** Pick Spring Boot Version, Pick Dependencies and *Add Spring Boot Starters* (open `pom.xml` or `build.gradle(.kts)`); Initializr 1 skipped.
- **Skipped:** GraphQL 2, Runtime 8 and the Cloud rows of the web agent. The overload and Kotlin-check claims in the spring-beans limits above are stale (see the corrected lines).
- **Not verified in the IDE:** all of the above (mock host only).

## aritti.devcontainer (clean-room, inspired by org.jetbrains.plugins.docker.gateway)

Drives the devcontainer CLI (`@devcontainers/cli`) and Docker: Start, Rebuild, Open Shell, Stop and
Show Configuration commands, and `devcontainer.json` templates (base, Node, Python, Java, PHP, Rust, Docker Compose). Checked against `l0ng-ai/openidea` `main` at
`f665beb1` (plugin API 1.1) on 2026-11-05. The original's main feature, running the IDE backend
inside the container, needs work in OpenIDEA itself: see _Upstream work_.

### Missing API

| Feature missing or partial | Why | API it would need |
|---|---|---|
| Opening the project inside the container after **Start Container** | A plugin can't open a project or a remote window, and can't run IDE actions such as `remote.connect`. The user has to open File › Remote Development themselves | `workspace.open-remote(target, path)`, or `ui.execute-action(id, arg)` |
| Knowing when `devcontainer up` has finished, and what it returned | `run_process` is capped at 5 s (60 s with an open task), but a build takes minutes, so it runs in `open_terminal`. A terminal returns no exit code and no output, so the plugin can't read `containerId` / `remoteWorkspaceFolder`, say the container is ready, or start the next step | A terminal handle with an exit event and its output, or a process API without the time cap that streams output |
| Choosing among several configurations (`.devcontainer/<folder>/devcontainer.json`) | With API 1.2, when both default files exist (`.devcontainer/devcontainer.json` and `.devcontainer.json`) a `host::pick` asks which to use; configurations in subfolders are still unknown because they can't be listed | `find_files(glob, limit, options { hidden, ignored })`, or a `read-dir` function |
| Finding the configurations | `find_files` skips hidden directories and `.gitignore`d paths (`WalkBuilder::hidden(true)`, `crates/ide-host/src/plugins/api.rs`), so nothing under `.devcontainer/` is ever listed, and there is no directory listing. The mock host doesn't skip them, so tests pass anyway | `find_files(glob, limit, options { hidden, ignored })`, or a `read-dir` function |
| Installing the devcontainer CLI from the IDE | Plugins can't add tools to Settings › Languages & Frameworks › Language Servers, or attach an install hint to a `process:` program. Without the CLI, the commands fail because the program can't be found | A manifest field declaring the installable tool behind each `process:` program |
| Completion and validation in existing `devcontainer.json` files | A plugin can't associate a JSON schema with a file name, and a plugin language can't override the built-in `jsonc` definition that owns `devcontainer.json`. Only files with a `"$schema"` key (like the bundled template) get completion | A `jsonSchemas` contribution (file pattern → schema URL) passed to the JSON language server |
| Using the Docker connection configured in the IDE | The IDE's Docker endpoint, context and TLS settings aren't exposed to plugins, so **Stop Container** runs `docker` with the CLI's default context | Read access to the Docker endpoint, or a host Docker API for plugins |
| Showing the commands only in projects with a dev container | `when` clauses can't test project files, so the commands are always listed | A `when` context key for files at the project root |
| Podman or another Docker-compatible CLI | A `process:` capability pins one program name, so `docker` and `devcontainer` are fixed and the CLI's `--docker-path` can't be offered | Capabilities whose program comes from a setting |

### Implemented with a workaround

| Feature of the original | Workaround | Limitation |
|---|---|---|
| Container management (list, start, stop, remove, logs, shell) | The IDE's built-in Docker support in the Services view lists dev containers like any other. The plugin only adds **Stop Container** (`docker ps` by the `devcontainer.local_folder` label, then `docker stop`) | Nothing groups containers by dev container in the Services view. Might close when merged: `l0ng-ai/openidea` #1760 (plugin `services` views with tabs, text and process bodies), if the plugin moves to one |
| Picking the configuration | A `configFile` setting; empty probes `.devcontainer/devcontainer.json`, then `.devcontainer.json`, with `read_file`; when both exist the user picks one (cancelling takes the first) | Configurations in subfolders must be typed in by hand |
| Building and starting the container | `devcontainer up` (with `--remove-existing-container` for Rebuild, after a `host::confirm`) in an IDE terminal | No progress, result or "ready" notification in the IDE, see above |
| Reading the resolved configuration | **Dev Container: Show Configuration** runs `devcontainer read-configuration --workspace-folder <root> --config <file>` through `run_process` inside a task (60 s cap) and summarises image or compose file, remote user, forwarded ports, number of features and whether there is a `postCreateCommand` in a notification | A CLI start slower than 60 s fails, and failures (non-zero exit, no JSON, CLI not found, denied) show no notification. The result is a notification, not a view. The real output shape (`{"configuration":…}`) was not run against a live CLI |
| New menu templates for `devcontainer.json` | `[[contributes.fileTemplates]]`: base, Node, Python, Java, PHP, Rust and Docker Compose, each with a `$schema` | One fixed starter per language; the image tags (`javascript-node:22`, `python:3.12`, `java:21`, `php:8.3`, `rust:1`) come from the public reference images, not from a tool, and the Compose template assumes the user supplies `docker-compose.yml` (a plugin can't create it) |

### Upstream work (OpenIDEA core, prepared locally, not submitted)

| Branch | What it adds | Known limits |
|---|---|---|
| `feat/tools-devcontainer-cli` (`~/Projects/openidea`) | A `Cli` tool kind and `@devcontainers/cli` (installed with npm) on the Language Servers page | One hard-coded entry: every other CLI needs its own core change until plugins can declare tools. A CLI belongs to no language, so its languages column is empty |
| `feat/remote-dev-containers` (`~/Projects/openidea-container`) | A container target next to SSH and WSL: `openidea-server` installed and started through `docker exec -i`, and a container picker in File › Remote Development | Running containers only: the dialog can't start, build or rebuild one. Default Docker connection only. Runs as the container's default user (often `root`), not the configuration's `remoteUser`. The workspace folder is guessed as `/workspaces/<folder name>`, so a `workspaceFolder` set in the configuration is ignored. Recent projects store the container's name, which changes when the container is rebuilt. No port forwarding (SSH only, as for WSL). Needs an `openidea-server` release for the container's Linux platform. Tested with unit tests and against a real container (install, start, `initialize`); not tested in the running app or on Windows |

### Not ported

- Creating a dev container from a Git repository URL: closed in part, *Clone Repository and Start…* clones with git and starts the container (below); it still does not open the folder as a project.
- Browsing Dev Container templates: the New menu offers fixed starter configurations (see the templates row above). Feature ids are completed from the public index since the backlog pass (below); there is no picker.
- Applying the configuration's IDE customisations (extensions, settings) inside the container: nothing in OpenIDEA maps them.

### Added by the feasible-parity pass (2026-11-06)

- **Inspections** (`checkProperties`, `checkFeatures`; `events` optional, activation `on-event:file-opened`/`file-saved`): `unknown-property` reports a top-level key only when it is within edit distance 2 of a known key (the message names the suggestion); any other unknown key, `x-*`, `$*` and everything inside `customizations` stay quiet. `features-malformed` reports a `features` value that is not an object, a feature id that is empty or has whitespace, and a feature value that is not an object, string or boolean (null is tolerated). The scanner is a tolerant JSONC reader (comments, trailing commas).
- **Limits:** the known-key list was written from memory of the specification, so a real key within distance 2 of another known key could be flagged if it is missing from the list; files over 256 KB publish an empty list; checks run only on opened or saved files, with no project-wide pass (`find_files` skips `.devcontainer`). Activation uses the event form because `on-file:` is not documented to match dot-directories.
- **Accepted:** for a file starting with a UTF-8 BOM, line-0 ranges are one UTF-16 column to the right if the editor document omits the BOM, so the squiggle on the first line is off by one. The manifest-activation test is a substring check on `plugin.toml`, not a TOML parse.
- **Not verified in the IDE:** `on-event:` activation in the real host.

### Added by the gap F pass (2026-11-06)

- **Folder structure** (`folder-structure`, Information, setting `checkFolderStructure`): a `devcontainer.json` that is not at `.devcontainer/`, `.devcontainer/<folder>/` or `.devcontainer.json`. It looks at the opened file's path only; it does not check that anything references the file.
- **Completion** (setting `completeProperties`): top-level property names not yet present, and value choices for known enum or boolean properties. Nested objects, arrays, feature ids and schema-driven values are not completed. The provider is declared for `json` and `jsonc`; the `jsonc` language id and whether the completion `Document.path` equals the path the file-opened event reports are not verified in the IDE.
- Files over 256 KB publish nothing for either the checks or the completion.

- **Clone sources into a container (DC7):** *Dev Container: Clone Repository and Start…* asks for a URL and a folder (default: next to the project), runs `git clone -- URL FOLDER` (optional `process:git`) and then `devcontainer up --workspace-folder FOLDER` in a terminal. It does not open the new folder as a project, does not clone into a Docker volume as JetBrains does, and does not check what the clone contains. Unverified: that the host accepts a clone target outside the project root.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Dev Containers view** (`on-view:containers`): running containers from `docker ps` by the `devcontainer.local_folder` label, with Open Shell and Stop (double-click opens a shell). Needs `process:docker`; the label is from the CLI's behaviour and not checked against every CLI version. It lists only containers started from this folder.
- **Feature id completion** in the `features` object from `containers.dev/static/devcontainer-index.json`, kept in plugin storage, refreshed by *Dev Container: Refresh Feature List* (optional `storage`, `net:containers.dev`; a setting switches it off). The index may lag the registry; the download is not verified in a live IDE.
- **Clone Repository and Start…:** see the gap F bullet above; unverified that the host accepts a clone target outside the project root.
- **Not done (DC-2 partial):** no gutter marker on `image` that starts the container, because a gutter marker can open a location but not run a command.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Dev Containers view:** lists the project's containers, running and stopped (`docker ps -a` by the `devcontainer.local_folder` label), with Open Shell, Stop and Start. The earlier "running containers only" wording is replaced by this. Needs `process:docker`.
- **Not done (B):** DC1, 3, 6, 8, 12 and 13. **Not verified in the IDE:** the Start action and the stopped-container rows.

## JPA Assist (`aritti.jpa-assist`)

Clean-room plugin for JPA and Spring Data in Java and Kotlin: entity and
repository checks, derived query completion, equals/hashCode/toString, DDL,
DTOs, entities from `CREATE TABLE`, and a JPA Structure tool window. Checked
against the WIT, SDK and host of v0.1.37 and `main` (`f665beb`) on 2026-11-05.
The plugin API is the same in both.

### Generated code can't be written to new files

- **Blocks:**
  - DTOs, entities from `CREATE TABLE` and repositories written as files;
  - a Flyway `V<n>__*.sql` migration or Liquibase changelog from the DDL;
  - showing generated code in an editor tab, ready to save.
- **Why:** same as *Code actions can't create, rename or delete files* in the Symfony section. Also, a command can't open an unsaved document: `views` has only `refresh`, `post-message`, `open-terminal` and `open-location` (existing files only).
- **Workaround:**
  - *Copy DDL*, *Copy DTO* and *Copy Entity for CREATE TABLE* put the text on the clipboard.
  - Without `clipboard` granted, the command shows the text instead.
- **Update 2026-11-06:** *New Entity* now writes the file through the optional `process:tee`; the other generators still copy.
- **Would need:** a `create-file` code-action edit, `project.write-file(path, text)` behind a `project.write` capability, or `views.open-document(language, text)` for an unsaved scratch document.

### Wizards are prompt chains, not forms

- **Blocks:**
  - one dialog with linked fields for a new entity or attribute (the choices depend on each other);
  - a visual derived query builder;
  - picking the target package for one run (a new entity takes it from the focused file's path under `java/` or `kotlin/`, else none).
- **Why:** a `Form` view could replace a dialog, but nothing opens a view from a command with content of its own. `host::pick`, `pick_many` and `input` ask one question each.
- **Workaround:**
  - *JPA: New Entity* (Tools menu) asks for the class name, table, ID strategy and extras (auditing fields, version, equals/hashCode). It copies the source and, when the focused editor is an empty Java or Kotlin file, also writes it there; the language is that file's, else the `entityLanguage` setting.
  - *JPA: New Attribute* (editor menu in an entity) asks for the association first (none, ManyToOne, OneToMany, OneToOne, ManyToMany), then the type (or enum or other type) or the target entity, then not null, unique, cascade and fetch where they apply. It appends the field and its imports at the end of the class body.
  - Writing into the editor needs `editor.write` (optional); without it the entity stays on the clipboard and a new attribute shows a notice. Dismissing any prompt changes nothing.
  - Completion builds derived query names. *Copy DDL* asks for the SQL dialect, *Copy DTO* for the fields and *Copy Entity* for the language.
- **Limits:** a new entity can't be created as a file (see above), so an empty file has to exist first; a new attribute adds no `mappedBy` or `@JoinColumn`, and Kotlin attributes are always nullable `var`s.
- **Would need:** `views.show(view)` for a form opened from a command.
- **Closed in v0.1.38:** the field and dialect pickers (`host::pick`, `pick_many`).

### No database access

- **Blocks:**
  - entities generated from a live schema;
  - Flyway or Liquibase changesets from a diff between entities and a database;
  - checking that mapped tables and columns exist.
- **Why:**
  - No database API, and the IDE's database connections aren't exposed.
  - `process.run(program, args, cwd, stdin)` can run `psql` or `mysql`, but it has no environment variables.
  - Secrets are only filled into `{{secret:…}}` in `net` request headers, so a password would have to go on the command line, where other processes can see it.
- **Workaround:** entities come from `CREATE TABLE` text at the caret; DDL is generated from entities only.
- **Would need:** read access to the IDE's database connections (schemas, tables, columns, keys) and a way to run queries on them, or secrets usable as `process.run` environment variables.

### No Java/Kotlin code model or build model

- **Blocks:**
  - resolving classes by fully qualified name (two `Order` classes in different packages);
  - generics;
  - attributes inherited from compiled or library superclasses;
  - accessors generated by Lombok;
  - what Kotlin `allopen`/`noarg` change;
  - telling enums from embeddables and converters when writing DDL;
  - `@Inheritance` strategies;
  - `jakarta` vs `javax`, Hibernate defaults and the naming strategy from the real dependency versions.
- **Why:**
  - Same as *No access to the semantic model of built-in languages* in the Symfony section: jdtls and kotlin-lsp can't be queried.
  - There's no Maven or Gradle model either: no modules, source roots or resolved dependencies. Versions set in parent POMs, BOMs or version catalogs can't be read as text.
- **Workaround:**
  - Comments and strings are masked, then the source text is parsed.
  - Classes are found by simple name: next to the file, then at the path stored for that name, then by searching the project.
  - Unknown attribute types become SQL comments.
  - A superclass's attributes are added to the subclass's table.
  - `jakarta` is assumed unless the file mentions `javax`, with Hibernate 6 and Spring Boot naming defaults.
- **Would need:**
  - an LSP request bridge (`workspace/symbol`, `typeDefinition`, type hierarchy) or a symbol index query;
  - a project model API (modules, source roots, resolved dependencies with versions).

### Repository completion can't search the project

- **Blocks:** derived query completion in a repository whose entity hasn't been loaded yet.
- **Why:**
  - Same root cause as *No file-change events, short budgets, little storage* in the Symfony section: providers get 50 ms and are interrupted at 500 ms.
  - `find_files` walks the whole tree on every call (no index), and `{a,b}` globs need one call per pattern to behave the same in `MockHost`.
  - An idle instance is reclaimed after 5 minutes and loses its cache.
- **Workaround:**
  - Opening or saving a repository loads its entity graph with a project search.
  - Each class's path is kept in `host::storage` so a new instance can read it directly.
  - Completion itself only reads files next to the repository or at a stored path.
- **Would need:** an indexed file-name or symbol lookup that is cheap enough for editor providers.

### Checks only run on open and save

- **Blocks:** live checks of entities and derived queries; a problem clearing as soon as its quick fix is applied.
- **Why:** same as *Problems aren't re-checked while typing* in the Symfony section.
- **Workaround:** checks run on `file-opened` and `file-saved`.
- **Would need:** a `diagnostics` provider kind or a debounced `document-changed` event.

### No refactoring hooks

- **Blocks:**
  - renaming an entity attribute while updating derived query names (`findByEmail` → `findByMail`), `mappedBy` strings and JPQL in `@Query`;
  - renaming an entity while updating JPQL.
- **Why:**
  - No rename participant, and no `rename` provider (see *No references, rename or hover providers* in the Symfony section).
  - `action-performed` reports only the action id, not what was renamed.
- **Would need:** a rename participant that receives `(old, new, symbol)` and returns extra edits.

### Missing provider kinds and editor control

- **Blocks:**
  - hover for an attribute (column, SQL type, constraints) or a derived query (the JPQL it stands for);
  - inlay hints with column names;
  - Find Usages of an attribute in derived queries and JPQL;
  - leaving the caret inside generated `equals`/`toString`.
- **Why:**
  - Provider kinds are `completion`, `inline-completion`, `code-actions`, `definition` and `line-markers`.
  - `text-editor` has `path`, `language`, `document`, `selections` and `apply-edits`, with no way to set selections.
  - Code-action edits can't contain snippets; only completion items can place the caret with `$0`.
- **Would need:** `hover`, `inlay-hints` and `references` provider kinds, plus `text-editor.set-selections` or snippet code-action edits.

### Plugin file templates get no package or prompts

- **Blocks:** New › JPA Entity / Spring Data Repository created with the right `package` line and the repository's entity type filled in.
- **Why:** contributed templates are registered without a `packageStyle` (`src/features/plugins/uiPoints.tsx`, `addContributedFileTemplate`), so `$PACKAGE$` is empty and its line is dropped. A template can't ask for anything except the name, or run code.
- **Workaround:** no `package` line; `$END$` puts the caret on the entity type.
- **Would need:** `package-style = "jvm"` on `[[contributes.fileTemplates]]`, and extra prompted variables.

### Menus and tree views depend on plugin state (closed in v0.1.38)

- **Closed in v0.1.38:**
  - *Copy DDL* and *Copy DTO* show only in entity files and *Copy Entity* only in SQL files that contain a `CREATE TABLE`: the plugin sets `plugin.aritti.jpa-assist.isEntity` and `.hasCreateTable` with `host::set_context` on file-opened, file-saved and editor-focused events (it subscribes to `EditorFocused` in `activate`).
  - Right-click an entity in JPA Structure for *Copy DDL* and *Copy DTO* (`viewItem.context`, `viewItemKind == entity`; tree keys are now `entity:<path>` and `attribute:<path>#<name>`).
- **Remaining limits:**
  - The context is set from the file as last opened or saved, so an unsaved edit that adds `@Entity` or `CREATE TABLE` shows the item only after saving. The key is per file, not per caret: *Copy Entity* still reports when the caret is outside a `CREATE TABLE` statement.
  - Attribute items have no menu of their own; double-clicking one opens its declaration.

### Generated sources and library entities are invisible

- **Blocks:**
  - entities and metamodel classes in generated sources (QueryDSL `Q*` types, MapStruct implementations under `target/` or `build/`);
  - entities shipped in dependency jars.
- **Why:**
  - Same as *`find_files` skips ignored files and there are no library roots* in the Symfony section.
  - `read_file` only reads UTF-8 text up to 16 MiB, so `.class` files and jars can't be read.
- **Would need:** a `find_files` option to include ignored files, and classpath symbols through a code model API.

### Added on 2026-11-06

- **Entity to repositories gutter arrow:** on an `@Entity` class line the marker (icon `navigate`, tooltip "Repositories: A, B") targets the first repository. Repository to entity is `spring-data`'s. A `RepositoryIndex` lists `*Repository.java|kt` once and reads at most 30 ms of unread files per provider call (partial results until done); saving or opening a repository file refreshes it. Limits: repositories not named `*Repository` are not found; a deleted or renamed repository file stays indexed until the instance is reclaimed; on a very large project the first calls return partial results.

### Possible with the current API, not done yet

- Go to Definition from a part of a derived query to the field is in `spring-data`; from a JPQL alias path (`u.address.zip`) it is not done, because `jpa-assist` doesn't parse `@Query` (`spring-data` checks and completes entity and alias properties there).

### Added by the feasible-parity pass (2026-11-06)

- **jpa-assist:** `entity-missing-id` replaces `jpa-missing-id`: it follows project superclasses and warns only when the whole chain is project entities without an id; a library or unfound superclass is quiet and the search is capped by `MAX_SOURCES`. `attribute-type` covers `@Temporal`, `@Enumerated` and `@Version` on well-known types only; `@Lob` is not checked. `entity-listener-signature` treats a class with callbacks, a `persistence` import and no entity marker as a listener (heuristic, no `@EntityListeners` cross-reference), and a file with both an entity and a listener holds the listener to the zero-parameter rule. Settings `checkEntityId`, `checkAttributeTypes`, `checkListenerSignatures`.

### Added by the gap F pass (2026-11-06)

- **Kotlin checks** (setting `jpaAssist.checkKotlinEntities`): `jpa-val-attribute`, `jpa-immutable-collection` (a read-only collection on an association) and `jpa-data-class` (now quiet when `equals`, `hashCode` and `toString` are all overridden). Only the first class in a file is checked and there is no type resolution: collection types are judged by simple name, so a user-defined `List` is flagged, and `@Version val` still warns.
- **Class signature** (`jpaAssist.checkClassSignature`): `jpa-entity-nested`, `jpa-entity-multiple-roles` and `jpa-entity-not-a-class`. The nested check applies to classes nested in a class only.
- **Listeners** (`jpaAssist.checkListenerUsage`): `jpa-listener-without-callbacks` on an `@EntityListeners` reference. The listener is found by simple file name and only when there is one match; callbacks inherited or supplied by an interface are not followed, so those cases stay quiet.
- **Callback actions:** seven "Add @X callback" source actions on the class line, skipped when a method of that name already exists (a call to it also counts). The import is added with the method, and CRLF files keep their line endings. The action on a `@MappedSuperclass` is not verified.
- **`parse_class` accepts classes only:** an entity on an interface or enum drops out of the structure view and the generators. A later entity after a non-class marker in the same file is not parsed.
- A preceding `Audit::class` in an annotation no longer counts as the class keyword.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **New checks (text based, quiet when a class is outside the project):** two associations on one attribute, an association on a basic or collection type, `@Id` with `@EmbeddedId`, mixed getter and field annotations; listener without a public no-arg constructor; the `kotlin-jpa` plugin read from `build.gradle(.kts)` and `pom.xml` up to the root (missing, redundant no-arg constructor, redundant `open` under allopen; quiet for version catalogs, convention plugins and `noArg`), with a quick fix that edits the Gradle file; unknown attribute names in `@AttributeOverride`, `@MapsId`, `@MapKey`, `@NamedAttributeNode`; `:parameters` of `@Query` and unknown named queries (quiet with `orm.xml`); `persistence.xml` and `orm.xml` classes and values, with class name completion in `<class>`.
- **Actions:** *Add inverse attribute* on ManyToOne, OneToOne and ManyToMany (writes the other file only; not offered when the target already mirrors the association).
- **Commands:** *New Entity* (class name, table, ID strategy, auditing, version, equals/hashCode) and *New Attribute* (type or association, cascade, fetch, nullability; appended to the end of the entity class with imports). A plugin cannot create a file: New Entity copies the text, or writes it into an empty file in focus.
- **Not verified in the IDE:** the empty-file write, the prompt chain order, the Gradle edit.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **New checks:** entity graph names (`@EntityGraph` and `getEntityGraph` against `@NamedEntityGraph`, `attributePaths` against the repository entity), missing setters with property access, association targets that are not entities (JPA8, 9, 15, 30 of the audit); `persistence.xml` property name and value completion.
- **New Entity writes a file:** when the optional `process:tee` is allowed, *New Entity* writes the class next to the focused file (a name collision is not handled by the plugin beyond what `tee` does: not verified); otherwise it works as before (empty file or clipboard). This narrows *Generated code can't be written to new files* above for this one command; the other copy commands (DDL, DTO) still use the clipboard. It still cannot open an unsaved document.
- **Not verified in the IDE:** the write location and the new checks on real projects.

## aritti.kube-explorer (clean-room, inspired by JetBrains Kubernetes 263.7307.101)

Checked against `l0ng-ai/openidea` `main` at `1186cdce` (plugin API 1.1, no plugin API change since
`v0.1.37`). The original is proprietary, so features come from its Marketplace description.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Path to `kubectl` (and `helm`) as a setting | A `process:<program>` grant pins the program named in the manifest, resolved on `PATH` when granted | A program path taken from a setting and approved when it is set |
| Clusters using exec credential plugins (`gke-gcloud-auth-plugin`, `aws`, `kubelogin`) from a GUI-started IDE | `process.run` has no environment parameter; the child gets the IDE's environment, often without the shell's `PATH` | `env: list<tuple<string, string>>` on `process.run` / `open-terminal` |
| Quick documentation, find usages and rename of labels, selectors and ConfigMap/Secret keys | Providers are `completion`, `inline-completion`, `code-actions`, `definition`, `line-markers` only | `hover`, `references` and `rename` provider kinds |
| Completion of ConfigMap/Secret names and keys, label selectors and Docker images from the cluster | Providers must answer within 50 ms and can't wait for `kubectl`; prefetching on `file-opened` is lost when the idle instance is reclaimed (5 min) | Asynchronous providers, or background work (`host::schedule`, a task that outlives the call) feeding a cache |
| Schemas of custom resources from the active cluster (`/openapi/v3`) (the resources themselves are listed, see "Custom resources" below) | No way to register schemas with the built-in `yaml-language-server`, and no file write to point a modeline at a generated schema | Schema associations for built-in language servers (`[[contributes.jsonSchemas]]` with file patterns or content matchers, or language-server settings for built-in languages) |
| Helm templates: Go template syntax, values and named-template completion, navigation and inspections; Werf | Go templates live inside YAML; a plugin can't inject a language into a built-in one or own `yaml` (built-in ids and extensions win) | Language injection / template data languages for built-in languages |
| Browsing a chart's `.tgz` dependencies in the project tree | No project-view contribution or virtual file system | Project-view node provider, or an archive file system |
| Cluster-wide log streaming with pattern matching, and watching resources (tree updates as the cluster changes) | `process.run` blocks until exit (5 s, 60 s inside a task, 4 MiB); no streaming spawn; `on-schedule` is at least 60 s; terminal output can't be read | `process.spawn(program, args) -> handle` with output events and `kill`, plus a log/console view body with filtering |

### Closed in v0.1.38

- **Right-click actions on cluster resources:** `viewItem.context` menus on resource items in the tree: Describe, Edit YAML, Logs, Shell, Attach, Debug, Port Forward, Show Secret and Delete (confirmed with `host::confirm`). Resource keys are now `res:<kind>/<row>` so `viewItemKind == res` excludes the kind folders. The menu shows every command on every resource and a command that doesn't fit the kind (Shell on a service) reports it, because `when` can only test the key's prefix.

### Implemented with a workaround

| Feature | Workaround | Limitation | API that would remove it |
|---|---|---|---|
| Custom resources | A "Custom Resources" root node, then API group, then kind (plural and group), then instances. Names come from one `kubectl api-resources -o name --verbs=list --request-timeout=4s` call; core resources and the built-in groups (`apps`, `batch`, `autoscaling`, `policy`, `networking.k8s.io`, `events`, `storage`, `storagemigration`, `rbac`, `coordination`, `discovery`, `apiextensions`, `metrics`, `scheduling`, `node`, `certificates`, `admissionregistration`, `flowcontrol`, `apiregistration`, `internal`, `resource`, `authentication`, `authorization`) are dropped, while `gateway.networking.k8s.io`, `snapshot.storage.k8s.io` and `*.x-k8s.io` stay. Instances take Describe, YAML and Delete (with confirmation); logs and shell are off. A failing call shows an error node and an empty list a placeholder | The group list is a fixed allowlist, so a built-in group added by a newer Kubernetes shows up as custom; `metrics.k8s.io` is excluded on purpose. Right-click actions are still missing (see above). Tested against the mock host only | `tree-item.actions` (see above) |
| Edit a resource's YAML in an editor, apply on save (IntelliJ: `LightVirtualFile`) | `process:tee` writes `/tmp/kube-explorer-<kind>-<name>-<random>.yaml`, `open_location` opens it, the `file-saved` event runs `kubectl replace -f` with the context and namespace it was opened from | No file-write or virtual-document API, so a high-risk capability writes one file; Unix only; one storage entry per file, never pruned. **`open_location` refuses paths outside the project (`resolve_in_root`, `wit_host.rs`), so in the IDE this falls back to the panel editor below**; writing into the project instead would leave files in the user's tree | `views.open-document(name, language, text) -> id` with a `document-saved(id, text)` event; or `workspace.write-temp-file(name, text) -> path` that `open_location` accepts |
| Fallback YAML editor, describe, diff, Kustomize/Helm output | An `Html` body: a textarea over a highlighted copy; Apply (⌘S) posts the text back, `kubectl replace -f -` | Not a real editor (no undo history shared with the IDE, no YAML language server, no search); shows blank in the IDE for a still-unknown reason while it renders in WebKit with the IDE's frame wrapper | Same as above, plus a read-only virtual document for command output |
| Saving the edited YAML more than once | `metadata.resourceVersion` is removed before editing | Each save overwrites changes made in the cluster meanwhile. Keeping it fails every save after the first: the plugin can't refresh the open editor with the new version without making it unsaved (`apply_edits` only targets the invocation's editor) | A document the plugin owns and can reset after a save (part of `open-document`), or `TextEditor::set_saved_text` |
| Kubernetes schema completion and validation | `# yaml-language-server: $schema=<yannh/kubernetes-json-schema URL>` written at the top of the temp file, and a ⌥⏎ code action on project manifests | Changes the user's file; single-document files of built-in kinds only; the language server fetches the schema from GitHub | Schema associations for built-in language servers (see above) |
| Kubernetes icons on tree items | Each SVG declared as a `[[contributes.fileIcons]]` entry on a pattern no file has (`*.kube-explorer-pod`); the tree names it `ext:fileIcon/aritti.kube-explorer/<id>` | `tree-item.icon` only resolves the IDE's icon names; this relies on the frontend's internal naming of file icons and may break | Package SVG paths for `tree-item.icon` and `button.icon`, or `[[contributes.icons]]` |
| Context and namespace switchers, confirmation of deletes, port-forward ports | Double-click a context/namespace node; the toolbar Delete needs a second click on "Confirm Delete …"; Delete Manifest Resources shows a server dry run with a Confirm Delete button; ports in a `Form` body. The right-click Delete asks with `host::confirm`, and right-click results (describe, YAML, port-forward form) bring the view forward with `host::reveal_view` | The switchers, the toolbar and manifest commands (Diff, Kustomize, Helm, the delete preview) still use the tree and buttons; the editor commands don't reveal the panel yet | Not needed any more: `host::pick`, `input`, `confirm` and `reveal_view` exist since v0.1.38; moving these flows onto them is not done. **Closed in v0.1.38:** confirmation and reveal for resource right-click actions |
| Pod logs, shell, attach, port forward | `host::open_terminal` running `kubectl` | The plugin can't read, filter or stop them (no handle); a port forward stays until the user closes its terminal | `process.spawn` handles (see above) |
| Cluster calls from the tree | `--request-timeout=4s` on reads; tasks for apply/delete | View and command calls get 5 s (60 s with a task); an unreachable cluster blocks the instance, and queued editor providers are dropped meanwhile | Asynchronous host calls, or a background worker per plugin |
| Helm commands on the chart around the open file | `Project::find_files("**/Chart.yaml")`, innermost directory containing the file | `find_files` skips hidden and gitignored directories, so such charts aren't found | `find_files` option to include ignored files, or `Project::exists(path)` / a stat call |

### Added by the feasible-parity pass (2026-11-06)

- **Manifest inspections** (six settings, `events` and `project.read` optional, activation `on-event:file-opened`/`file-saved`): `deprecated-api-version` (11 rows of removed groups with the release that removed them), `duplicate-env-name`, `missing-name`, `missing-selector`, `helm-whitespace-trim`, `chart-missing-key`. Intentions: List to separate documents and documents to List. Limits: block-style YAML only (flow-style `env`, `metadata` or `spec` keeps a check quiet); files containing `{{` get only the Helm group; the deprecation table lacks `autoscaling/v2beta1` and `v2beta2` and the `discovery`, `events`, `flowcontrol` and `storage` `v1beta1` rows; `missing-selector` is quiet for files in a directory with a `kustomization` file or a name containing `patch`, and only looks at the file's own directory; comments inside a List's items are kept but List-level metadata and header comments are dropped by the conversions; every `.yaml`, `.yml` and `.tpl` open or save costs a file read, capped at 1 MiB; changing a setting does not refresh published diagnostics until the next save.
- **Templates:** 15 New-menu templates (`k8s-<kind>`; 16 after the gap F pass) that pass the manifest checks.
- **Debug:** a button on a pod runs `kubectl debug -it <pod> --image=<debugImage> --target=<first container>` in a terminal (default image `busybox:stable`, setting `debugImage`). Only the first container is the target, the cluster must support ephemeral containers, and pod or image names with whitespace, control characters or a leading `-` are refused.
- **Secrets (setting `allowRevealSecrets`, default off):** Show Secret and Create Secret are tree-toolbar buttons, not palette commands. With the setting off, Show Secret shows nothing and runs no `kubectl`, and the YAML of a Secret (edit and open) is refused too; Describe is left alone because it shows no values. With it on, YAML of a Secret is unstripped. Show runs `kubectl get secret -o json`, decodes in the plugin and keeps values in the Form body in plugin memory only; they clear on Back (there is no view-close event) and are hidden if the setting is turned off. Non-UTF-8 values show as base64. Create builds an Opaque manifest with base64 `data` and runs `kubectl create -f -` with the manifest on stdin (never `apply`, so an existing secret is not overwritten and no last-applied copy of the values is stored); an existing name gives a value-free message. **The Form API has no masked field, so typed values in Create are visible on screen.** Values never go to logs, notifications, storage, the clipboard, terminals or process arguments; failure text is redacted of the raw and base64 values, and that redaction misses JSON-escaped and partial forms. A Secret's YAML already opened as a temp file stays on disk when the setting is turned off.
- **Not verified in the IDE:** the `$NAME$` and `$END$` placeholders in the templates; that Form field values persist as the plugin assumes; prefilled code-action edits (the intentions use whole-document edits).

### Added by the gap F pass (2026-11-06)

- **Deprecated APIs:** the table gained 13 entries (HPA `autoscaling/v2beta1` and `v2beta2`, and the `discovery`, `events`, `node`, `storage`, `certificates`, `coordination`, `scheduling`, `apiregistration`, `authentication` and `authorization` `v1beta1` groups), checked against the Kubernetes deprecation guide. It is still a fixed list.
- **Chart.yaml schema** (`chart-unknown-key`, `chart-invalid-value`, setting `checkChartSchema`): top-level keys against the fixed Helm key list, and scalar values (`apiVersion`, `type`, `deprecated`, a `version` that does not start with a digit after an optional `v`; `yes` and `no` count as booleans). Files with Helm `{{ }}` actions are skipped. `requirements.yaml` (Helm 2) is not covered.
- **Excessive trim** (`helm-excessive-trim`, setting `checkHelmExcessiveTrim`): text based, with no template parse, so a `}}` inside a string literal or a comment can end an action early and confuse it.
- **Commands:** Update Helm Repositories (`helm repo update`, needs the optional `process:helm`; denied gives a notification) and How to Install kubectl (static text in a notification). The hint cannot install kubectl or read the shell's `PATH`; it only reports that the IDE could not start it. It appears as a tree button only while no context is stored, because kubectl is not asked once one is, and a missing kubectl on the real host may show a permission message instead (not verified). A 16th template, CertificateSigningRequest, joins the New menu.
- **All namespaces:** the first entry of the Namespace list shows every namespaced kind as `namespace/name` rows; actions on a row run in that row's namespace. Create Secret, Apply Manifest, Apply Kustomization and Delete Manifest Resources are refused in this mode (pick a namespace first); dry run, Diff and Kustomize render still run against the kubeconfig's namespace. Cluster-scoped custom resources and custom resources whose names are not DNS names are listed, but every action on them is refused. Refresh already existed.
- **Known limit that predates this pass:** the port-forward `ports` text can carry extra kubectl flags.
- **Prompts (API 1.2):** Create Helm Chart (`input` a name, `helm create` in the project root), Configure Helm Parameters (release name, values file and `--set`, stored, appended to Preview Helm Template and Lint Helm Chart), Customize Namespaces (`pick_many` over `kubectl get namespaces`, stored; the Namespace node lists only those plus the current one, none picked shows all), Select Log Sources (`pick_many` over a pod's containers or a workload's pods, found through `spec.selector.matchLabels`; one terminal each) and Debug Resource, which asks for the image (default: the latest recent one, else the `debugImage` setting) and remembers the last 10; Edit Recent Debug Images removes entries with `pick_many`. Prompts need a command: the tree's Debug button still uses the setting without asking, and the Logs buttons still follow everything. Values are unvalidated beyond refusing a leading `-` and control characters; `--set` is passed as one `--set=` argument, so a value with commas is Helm's own syntax. Create Helm Chart needs `process:helm`, an open project and fails if the folder exists (Helm's message is shown). Tested against the mock host only.
- **Manifest references (document indexer on `yaml`/`yml`):** a resource's `metadata.name` defines `kube:<Kind>/<name>`; `secretName`, `serviceName`, `claimName`, `serviceAccountName`, `name` under `configMapKeyRef`/`configMapRef`/`configMap`/`secretKeyRef`/`secretRef`/`service`, and `scaleTargetRef` refer to it. Go to Declaration only (no find usages, rename or hover); the key ignores the namespace, so equal names in two namespaces clash; label selectors, `ownerReferences`, templated values (`{{ }}`, `${ }`) and CRD references are not indexed. The parser is line-based (indentation), so flow-style YAML (`{name: x}`) is missed. Capped at 4096 anchors per file.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Edit Secret** (needs `allowRevealSecrets`): a form of the decoded keys, saved with `kubectl replace` with only `data` changed; binary values are kept and not shown. Values are visible on screen. **Log Viewer:** a filtered snapshot of `kubectl logs` (text or regex, case, start time, wrap, copy), not a stream; *Select Log Sources* picks pods or containers.
- **Manifests:** `k*` abbreviations, Helm `.Values` and `include` completion, problems for unknown or deprecated keys, unknown kinds and server-owned fields of core, apps, batch, networking and rbac kinds only, hints for `.Values` paths and includes the chart does not define, Chart.yaml quick fixes (add a dependency, repository URL or alias; a quick fix cannot run helm or ask), Go to Declaration and gutter markers between a resource and the manifests that use it (open YAML files, not by namespace, so equal names in two namespaces can pick the wrong one). Settings `notifySuccess`, `nonKubernetesGlobs`.
- **Commands:** Create Helm Chart, Configure Helm Parameters, Customize Namespaces, Choose Kubeconfig (one file in use, not a merge), Telepresence Connect and Quit (optional `process:telepresence`), Edit Recent Debug Images. **Dropped (KE-7):** reload of the kubeconfig on change, because of the 50 ms provider budget.
- **Not verified in the IDE:** all of the above (mock host only).

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Reload Manifest from Cluster** is a command (optional `editor.write`) that replaces the open manifest with the live object. Commands have no 50 ms budget; the old note that reload does not fit the budget applies to providers only. It needs the focused editor.
- **Log Viewer:** Until, Time and Source fields, Scroll to end, Clear and Download (a file written through the optional `process:tee`). Still a snapshot of `kubectl logs`, not a stream.
- **Manifest hints:** missing required keys and invalid values (enums, integer and boolean fields; switches `checkRequiredKeys` and `checkFieldValues`), more kinds and groups known, non-editable and deprecated kinds. Kinds outside the tables stay unchecked (K-A5.3, K-A4.3/4/5, K-I1a/b/c, K-I2, K-I3; K-I4 partial).
- **Gutter markers cover the whole project** (not only open YAML files) and need the optional `project.read`; without it, or in an untrusted project, there are none. Corrects the old "open files only" wording.
- **Helm repo alias quick fixes** stay empty until *Read Helm Repositories* has run once, because a quick fix cannot run helm.
- **Not verified in the IDE:** project-wide marker cost on big repositories, the Download write, and the Reload write.

## aritti.tasks (port of com.intellij.tasks)

Port of `com.intellij.tasks` 263.7307.108 (intellij-community `idea/263.7307.112`). Checked against the
SDK, WIT and host source of v0.1.37 and `l0ng-ai/openidea` `main` at `f665beb` (no plugin API change
since v0.1.37) on 2026-11-05.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Legacy YouTrack InCloud hosts (`*.myjetbrains.com`) | The manifest declares `net:*.youtrack.cloud` and `net:*.atlassian.net` only; `*.myjetbrains.com` was left out | Add the host to the manifest and rebuild |
| Go to Task (⌥⇧N) popup, searching tasks and tracker issues | *Tasks: Switch Task* (API 1.2 `host::pick`) lists the open local tasks. *Tasks: Search Issues* (API 1.2) asks for text with `host::input`, searches every set-up tracker (GitHub, GitLab and Codeberg search text, YouTrack query, Jira `text ~`, Trello card search; at most 50 hits each) and `host::pick`s a hit, which then starts like an activated issue. Tasks and hits show in two places (Switch Task, Search Issues) rather than one popup; only commands can prompt, so the view has no search button | A popup extension point that filters as you type |
| Self-hosted trackers: Jira Server/Data Center, YouTrack Standalone, self-managed GitLab, Forgejo/Gitea, Redmine, Mantis, Bugzilla, Trac, FogBugz, the generic tracker | `net:<host>` is fixed in the manifest and `net:*` is rejected (`crates/ide-extension/src/capability.rs`, `FromStr`: only exact hosts or `*.domain`), so a server URL from settings can't be reached | A capability for hosts the user enters, e.g. `net:user` approved per host at first use, or hosts taken from a `string` setting and approved when it is set |
| Clearing the context on task switch (closing the editors of the task being left) | No API to list or close open editors, and no `file-closed` event (`EVENT_KINDS` in `crates/ide-extension/src/manifest.rs`: file-opened, file-saved, editor-focused, action-performed, vcs-changed) | `workspace.open-editors() -> list<string>`, `workspace.close-editor(path)` and a `file-closed` event. Closes when merged: `workspace.open-editors` only, in `l0ng-ai/openidea` #1754 (needs #1752 first); `close-editor` and `file-closed` stay open |
| Named contexts (Save / Load / Clear Context) | *Tasks: Save Context*, *Load Context* and *Remove Saved Context* keep named lists of the active task's last 10 recorded files (`input` a name, `pick` one, `confirm` before removing) and reopen them with `open_location` | Files only, at line 1; Load adds to the open editors and does not clear them; contexts are per project and share the 1 MiB storage | See the next row |
| Bookmarks and breakpoints in the task context (`WorkingContextManager`) | No bookmark or breakpoint API | `bookmarks.list/set`, `debug.breakpoints/set-breakpoints` |
| Several repositories per tracker (Configure Servers) | *Tasks: Add Repository* (`pick` GitHub, GitLab or Codeberg, `input` owner/name) and *Remove Added Repository* keep a stored list per tracker; the issues of all listed repositories are read (GitHub in one search with several `repo:` qualifiers) and replace the setting and the git remote. There is no *Select* step: all added repositories are used. Hosts stay fixed, so this is several repositories on one host, not several servers. YouTrack, Jira and Trello keep one site from the settings | Per-host capabilities (see the self-hosted row) |
| Changelists per task (`TaskChangelistSupport`, `createChangelist`) | Plugins see git only (`wit/workspace.wit`, `vcs`: `status`, `remotes`, `fetch`); there are no changelists | A changelist API, if the IDE gets changelists |
| Commit message in the commit dialog (`TaskCheckinHandlerFactory`, `CommitPlaceholderProvider`) and "commit changes" on close | No commit-message or commit-dialog extension point; `vcs` has no commit call | A `commit-message` provider export, or `vcs.set-commit-message(text)` and `vcs.commit` |
| Branch, merge and stash through the IDE's VCS layer (`VcsTaskHandler`, smart checkout, conflict dialogs; Mercurial too) | `vcs` can't change the repository except `fetch` | `vcs.checkout`, `vcs.create-branch`, `vcs.merge`, `vcs.stash` / `unstash` under `vcs.write` |
| JQL and YouTrack query languages (highlighting, completion, annotator) in the query fields | Settings and form fields are plain text; languages and completion only apply to editor documents | A form or settings field bound to a language id (`FieldValue::Code { language, text }`) |
| Analyze stack trace from a task description (`AnalyzeTaskStacktraceAction`) | No API to open the Analyze Stack Trace tool or a console with file links | `ui.analyze-stacktrace(text)`, or a console view body with hyperlinked positions |
| Trello: move a card to another list on close (`setTaskState`) | Needs the user to pick one of the board's lists. `host::pick` (API 1.2) could offer them; not built | Fetch the board's lists and pick one on close |
| Task description and comments in a documentation popup, issue links in commit messages (`ShowTaskDescription`, `TaskIssueLinkDocumentationTargetProvider`) | No hover or documentation provider for the VCS log or commit messages | A hover/documentation provider for commit messages and the VCS log |

### Implemented with a workaround

| Feature | Workaround | Limitation | API that would remove it |
|---|---|---|---|
| Open Task, Close Task and Edit Task dialogs | The Tasks view replaces its tree with a `Form` | Close Active Task (⌥⇧W) opens the Close form in the Tasks view and brings the view to the front with `host::reveal_view` (API 1.2); where that call is refused it falls back to a notification | `ui.show-form(fields) -> option<values>` for real dialogs |
| Branch per task, switch, merge and delete on close, shelving | Runs `git` through `process:git` (`Project::run_process`) | High-risk capability ("equivalent to running arbitrary code"), trusted projects only, git only; a failed checkout or merge is just a notification, with no smart checkout or conflict resolution | The `vcs` write calls above |
| Commit message from the active task | Opt-in `fillCommitTemplate`: writes `.git/openidea-task-message` with `git log -1 --format=… --output=…` (a plugin can't write files) and points `commit.template` at it, which the IDE's commit field starts from (`src/features/vcs/commitStore.ts`) | Changes `.git/config`; a stale template stays after uninstall; needs one commit in the repository; also shows up in `git commit` from a terminal | `vcs.set-commit-message`, or a commit-message provider |
| Context restore | Records `file-opened` paths per task (`events`) and reopens the last 10 with `open_location` | Files reopen at line 1; the previous task's files stay open; caret positions aren't kept | Open-editor list with carets, close-editor, `file-closed` event |
| Active task in the main toolbar | A status bar item (`set_status_item`) | One item for every window, showing the task of the project last acted in; text only, no drop-down | Per-window status items, or a toolbar contribution |
| Jira Cloud authentication | Secret `jira-token` holds the Base64 of `email:api-token`, entered by the user, sent as `Basic {{secret:jira-token}}` | Secrets are only substituted verbatim into headers; the plugin can't read or encode them | A `{{secret-basic:user,token}}` substitution, or Base64 transforms in the host |
| Trello authentication | Two secrets in one `Authorization: OAuth …` header | The user needs their own Power-Up API key (the original ships JetBrains' developer key) | — |
| Time tracking | Wall-clock time while a task is active | Idle time counts; noticing idleness would need `events.input` (every action the user runs, medium risk) | An `idle` / `active` workbench event without action details |
| Updating issues every 20 minutes (`updateInterval`) | `activation = ["on-schedule:1200"]` | The interval is fixed in the manifest; the setting can only turn it off | A schedule interval from a setting, or `host::schedule(secs)` |
| Task list per project (workspace state) | `host::storage` keyed by project root | 1 MiB per plugin for every project's tasks and issue caches; empty on `openidea-server` for remote projects | Per-project storage, or a larger quota |

## aritti.gitlab (port of org.jetbrains.plugins.gitlab)

Port of `org.jetbrains.plugins.gitlab` 263.7307.200 (intellij-community tag
`idea/263.7307.200`). Checked against the SDK and WIT of v0.1.37 on 2026-11-05.

### Network hosts are fixed when the package is built

- **Blocks:** self-hosted GitLab servers. Only gitlab.com works.
- **Why:**
  - `host::fetch` only reaches hosts granted as `net:<host>` in `plugin.toml`.
  - `net:*` is rejected, because each label must be alphanumeric or `-`.
  - `*.example.com` matches only subdomains of a domain known in advance.
  - A plugin can't ask for a new host at run time.
- **Workaround in the port:** none at run time. Add `net:<host>`, change `SERVER` / `SERVER_HOST` in `src/api.rs`, and rebuild.
- **Would need:** a run-time network grant, `net.request-host(host, reason)`, which asks the user and records the grant like an install-time capability. Or a `net:<setting:key>` capability whose host comes from one of the plugin's settings and is approved when that setting changes.

### Secrets can't be written by the plugin

- **Blocks:**
  - OAuth login (the original's authorization-code flow, or a device grant);
  - token refresh;
  - several accounts.
- **Why:**
  - Secrets are declared statically under `[[contributes.secrets]]` and entered only by the user. The host fills `{{secret:name}}` into request headers, so a token the plugin obtains has nowhere safe to go.
  - The original's OAuth flow also needs a local HTTP callback, and plugins can't listen on a port.
- **Workaround in the port:** one personal access token, entered in Settings › Plugins. The *Create Access Token* command opens GitLab's token page with the original's scopes.
- **Would need:** write-only `secrets.set(name, value)` and `secrets.delete(name)`, plus dynamically named secrets (`account:<id>`). Or a host-run `auth.oauth(provider)` where the IDE serves the redirect URI.

### Gutter markers can't run code

- **Blocks:** starting a thread from the editor gutter, as the original does on the merge request's branch.
- **Why:** A `line-marker` has a tooltip and one `target` location; a click can't run plugin code or pass the line, and a command doesn't receive the gutter line.
- **Closed with API 1.2:** Create Snippet asks for its title with `host::input` (the form view still has title, description and visibility fields); *Create Merge Request* and the status bar item reveal the form view, and the title and description are fields of that form.
- **Workaround in the port:**
  - Comments are made in the Merge Request Changes `Html` view.
  - Create Merge Request opens GitLab's new-MR page.
  - Snippets are private and use the file name.
  - Share uses the folder name, private, in your own namespace.
- **Would need:** `line-marker.command` (run with the line as argument).

### No editor decorations or document-changed event

- **Blocks:**
  - threads rendered under their line in the editor;
  - moving them to follow unsaved edits (the original diffs the merge request head against the live document).
- **Why:**
  - Plugins can't draw between editor lines.
  - There is no `document-changed` event, and providers get the text only within 50 ms.
- **Workaround in the port:**
  - Unresolved threads of the current branch's merge request show as gutter markers and as `Info` problems at their merge-request line.
  - They're hidden while the branch is missing commits from the merge request.
- **Would need:** `editor.set-decorations(path, list<decoration>)` (a block of HTML or markdown, or an inline hint, anchored to a range the host tracks across edits), or a debounced `document-changed` event.

### Comment fields have no completion or file picker

- **Blocks:**
  - `@mention` completion in comments;
  - attaching files to notes.
- **Why:**
  - `Form` fields have no completion hook, and completion providers only run in real editors.
  - The `Html` body has no `openFile()` (only `web` views have one) and no network access.
  - Uploading would mean building multipart bodies by hand for `host::fetch`.
- **Would need:** a markdown comment field that the IDE renders with its editor component and that calls the plugin's `completion` provider, and `openFile()` in `html` bodies.

### No access to the IDE's diff viewer

- **Blocks:**
  - the side-by-side diff with comment gutters;
  - per-commit diffs.
- **Why:** plugins can't open the diff viewer, nor add gutter actions or widgets to it.
- **Workaround in the port:** the cumulative diff is drawn as HTML, with threads and drafts on their lines, comment forms, and files marked viewed.
- **Would need:** `views.open-diff(title, left, right, decorations)`, where each side is a revision plus path or plain text, with diff-gutter markers that run commands.

### Notifications can't open a tool window

- **Closed with API 1.2:** the `!7 on feature` status item runs *Show Current Branch's Merge Request*, which selects it and calls `host::reveal_view` for the Merge Request view; *GitLab: Create MR* reveals the Create Merge Request form. Where `reveal_view` is refused the result text tells the user which windows to open. Only the Merge Request view comes to the front; the Changes and Commits views still have to be opened by hand.
- **Blocks:**
  - the original's "Create / View merge request" actions after a push.
- **Why:**
  - `ui.show-notification` has no actions.
  - There is no push event; `vcs-changed` gives only the repository root.
- **Workaround in the port:**
  - A pushed branch with no merge request shows *GitLab: Create MR* in the status bar.
- **Would need:** notification actions (`list<tuple<label, command>>`), and a `vcs-pushed` event (repository, remote, branch).

### The VCS API has no checkout, push, clone or commit

- **Blocks:**
  - Check Out Branch, Clone and Share Project without the high-risk `process:git` capability;
  - Share Project's `git init` and initial-commit file picker.
- **Why:**
  - `vcs` has only `status`, `remotes` and `fetch`.
  - `run_process` calls are capped at 60 s even inside a task, and can't prompt for credentials.
- **Workaround in the port:**
  - The plugin runs `git` (optional `process:git`).
  - Clone and push run in a terminal.
  - Share refuses repositories with no commits, rather than committing everything with `git add -A`.
- **Would need:** `vcs.checkout`, `vcs.fetch-ref`, `vcs.add-remote`, `vcs.push`, `vcs.clone`, `vcs.init` and `vcs.commit(paths, message)` under `vcs.write`, running through the IDE's git and credential helpers.

### Plugins can't open a project or pick a folder

- **Blocks:**
  - opening a cloned repository or a merge request worktree;
  - choosing the clone directory.
- **Why:** There is no `open-project` and no directory picker.
- **Workaround in the port:**
  - The clone target is the `cloneDirectory` setting, by default next to the current project, and the user opens the clone with File › Open.
  - Worktrees aren't ported.
- **Would need:** `workspace.open-project(path, new-window)` and `ui.pick-directory(title, initial)`.
- **Closed with API 1.2:** *GitLab: Create Snippet* is in `projectView.context` and uses `ctx.selected_paths()`: every readable selected file becomes a file of one private snippet (folders and unreadable or empty files are skipped; paths are relative to the repository). It needs `project.read`, which is optional, and falls back to the focused editor without it. Share Project still has no file-selection tree.

### View toolbars hold plain buttons only

- **Blocks:** an inline search field or toggle in the merge request list toolbar.
- **Why:** toolbars have no text field, choice or toggle. A `Tree` has speed search over visible labels only.
- **Workaround in the port:** the Filter button asks with `host::pick` (API 1.2) for a quick filter, or an author, reviewer or label from the project's members and labels, or text (`host::input`); typed filters are added to the quick filter's query and cleared with one entry. Unverified whether the host allows pickers from a view button: where it answers `Invalid` the button falls back to cycling the five quick filters.
- **Would need:** toolbar controls (search field, choice, toggle) that report `view-event::field`.

### Added on 2026-11-06

- **Apply suggestion:** *GitLab: Apply suggestion* is a quick fix (`code-actions`) when the selection covers a commented line that has a ```` ```suggestion[:-N+M] ```` block in an unresolved thread already loaded for the editor; it replaces lines `[line-N, line+M]` (CRLF documents get CRLF text, a last line without newline is handled, a suggestion reaching outside the file isn't offered). Limits: when the Changes view's diff is cached, every line of the replaced range that appears in it must still equal the open file, otherwise the action isn't offered; with no cached diff (Changes view never opened) it is offered without that check, so a wrong-line apply is possible; it doesn't mark the suggestion applied on GitLab; threads show only while the branch isn't behind the merge request.
- **Paging:** merge requests (50 per page), threads and member projects (100) and diffs are fetched page by page, up to 10 pages (500 merge requests, 1,000 threads, projects or diff files), with one 35 s budget shared by all the lists of a command, refresh or event. Lists after the budget are partial or empty, with a warning in the log (not in a status line); two long lists in one task can still pass the 60 s task limit by one fetch.
- **Viewed state:** a file stays viewed until a later commit touches it (one `git log --name-only` per render or mark through `process:git`). Without the merge request's commits in the local checkout, or without git, it falls back to the head commit. Marks stored by the previous version read as unviewed for files whose last commit isn't the head. Marking a file viewed needs page scripts, which the desktop IDE blocks (see *Scripts in `Html` view bodies*), so it is mainly reachable once scripts work.

### Not ported, but possible with the current API

- Fork merge requests are fetched from `refs/merge-requests/<iid>/head` of the target remote. A fork remote was judged not worth it: it needs the fork's URL (an extra call, maybe to a private project) and edits the user's git config for no gain.
- Markdown rendering of notes. Left out to keep the wasm small; a host `markdown-to-html` would make it free for every plugin.
- Usage statistics aren't ported, on purpose.

### Added by the feasible-parity pass (2026-11-06)

- **Merge request worktree and branch update:** *Open Merge Request in Worktree* fetches the branch (or `refs/merge-requests/<iid>/head` for forks) and runs `git worktree add --detach` into `<parent>/<repo>-<branch>`; it notifies the path and opens nothing. It refuses branch names starting with `-` and paths that sanitise to empty; "already exists" is left to git (no existence probe). *Update Merge Request Branch* reuses the existing rebase call and reads `merge_error` from one GET right after the PUT; rebase is asynchronous, so a conflict that surfaces later is not reported, and a closed merge request or a 403 may not give readable text.
- **`.gitlab-ci.yml` inspections:** `undefined-job`, `undefined-stage`, `duplicate-job-usage`, `single-quoted-shell-variable` (an `echo`/`printf` whose only argument is one single-quoted string), plus Go to Definition on `needs`, `dependencies`, `extends` and `stage`. Settings `ciJobReferences`, `ciStages`, `ciDuplicateJobUsage`, `ciSingleQuotedVariables`; activation `on-event:file-saved` and `on-file` globs. Quiet when `include:` is present (jobs; stages too when `stages:` is absent), for project, pipeline and trigger `needs`, `$VAR` names, anchors, merges, `!reference`, tabs, and stages given as a multi-line flow list, alias or tag. Not ported: schema validation, completion, following `include:` files, `rules`/`only` checks. Diagnostics refresh on open and save only. A plugin that declares a `yaml` provider loads for every YAML file; the provider returns first thing unless the path matches. The CI checks run before the token check, so no token is needed.
- **Not verified in the IDE:** `on-event:` activation in the real host and the mock's default `setting_bool` (true) against the real one.

### Added by the gap F pass on tooling (2026-11-06)

- **Form views:** *Create Merge Request*, *Create Snippet* and *Share Project on GitLab* are right-side views (setting `forms`; the commands of the same names still exist). Create Merge Request has title, description, target branch (at most 1000 branches), one assignee, reviewers by username, labels as text, and draft, squash and delete-source checkboxes; no pickers and no milestone. A `[Draft]` title gets a second prefix, and the branch cache is not keyed by project. Create Snippet takes only the focused file or selection (several files would need a file chooser, U5); a public snippet needs a second click bound to the file and content the warning showed. Share Project takes namespace, remote, visibility, optional `git init` (runs on the first click) and an optional initial commit behind a confirm bound to the listed files; it refuses secret-looking files and has no file-selection tree. The files staged by `git add -A` are compared with the confirmed list and unstaged again if they differ.
- **Notes (`noteActions`):** edit, delete (second click) and award emoji from a fixed list of 8 (`thumbsup`, `thumbsdown`, `heart`, `tada`, `laughing`, `confused`, `rocket`, `eyes`), on your own non-system notes. The edit box is not prefilled (form fields can't be prefilled from a note, see *Form fields can't be read-only*), and Save Edit overwrites on one click. A user whose id is 0 owns no note.
- **Commits (`commitsView`):** the merge request's commits; a double click shows that commit's diff read-only in the Changes view. In commit mode there are no comments and no viewed marks. Refresh can show a misleading status, and Open on GitLab opens the merge request page.
- **Pipelines (`pipelineView`):** the newest 20 pipelines of the merge request with their jobs by stage; retry a job or pipeline, cancel a pipeline (second click), open in browser. No job log, no manual play; stage order can change after a retry.
- **Merge request attributes (API 1.2):** the details button *Edit Labels, People, Milestone* and *GitLab: Edit Merge Request* (acts on the selected merge request, else the branch's) pick the attribute, then labels (`pick_many`, replaces the set), assignees and reviewers (`pick_many` of the project's members, replaces the set), milestone (`pick` of active milestones, or none) or the title (`input`). Not ported: target branch, new labels or milestones, group labels and members beyond the project's `members/all` (first 10 pages of 100). Unverified that a view button may prompt; where the host refuses, the status line names the command, which can.
- **Open on GitLab menus:** closed for the Project view by the backlog pass (Open on GitLab on `projectView.context`, below); Copy Link is not wired to it and the log menus don't exist.
- **Input guards (`src/safe.rs`):** branch and ref names, remote names, usernames, commit SHAs, discussion ids, emoji names, GitLab URLs, repository paths and clone URLs are checked before they reach a URL or a git argument, and a value that fails is refused with a message (at most 40 characters of it quoted). A branch or path outside `[A-Za-z0-9._/@+-]` can't be pushed or cloned through the plugin, SHA-256 repositories fail the 40-hex SHA cap, and a project path longer than 64 characters can't be cloned. The clone, fetch, remote and push calls put `--` before their arguments; `checkout` and `merge` can't, because git would read the names as paths.
- **Left alone in older code:** *Add Me as Reviewer* sends id 0 when `me.id` is 0, `open_url(mr.web_url)` in the details view isn't checked, `looks_secret` misses `.npmrc`, `*.p12` and `.netrc`, and error text after a bare `<` is dropped when stripping HTML.
- **Not verified in the IDE:** `ctx.editor()` during a view click (Create Snippet says "Focus a file first." without it); whether `open_terminal` joins its arguments into a shell line; whether a `Choice` field sends the option string and its field event arrives before the button click; `vcs_status` being None without a repository; tree `Activate` on a double click in a right-side view; icon names at runtime.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Open on GitLab** on the Project view menu (a file or folder). **CI:** completion of CI keywords and `CI_*` variables in `.gitlab-ci.yml` (the popup opens on letters, not on `$`); `include: local:` files count for job and stage checks, which stay quiet when an included file can't be read.
- **Edit Merge Request** (Details button or *GitLab: Edit Merge Request*): labels, assignees, reviewers, milestone and title chosen from the project's labels, members and milestones; the list *Filter* button picks a quick filter or an author, reviewer, label or text filter.
- **Skipped (P10, trackers):** discontinued trackers were not ported. **P7 partial:** snippet creation from the Project view takes single files; no file-selection tree, a form snippet holds one file.
- **Not verified in the IDE:** the Project view menu context, the filter picks.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Merge Request Files view:** Tree or List, double-click toggles Viewed, Mark All Viewed and Clear Viewed; needs no page scripts. *Create Snippet* takes more project files.
- **CI:** value completion (`when`, `stage`, `extends`, `needs`, `dependencies`, booleans, `!reference` job and key), warning for `!reference` naming a missing job, and `include: local:` files are followed (rows #3, #11, #4 done). The old claim that includes are not followed no longer holds for `local:`; `project:`, `remote:` and `template:` includes still are not.
- **Skipped:** #9 and #12. **Not verified in the IDE:** the new view and the `!reference` handling.

## Ideolog (`aritti.ideolog`)

Source port of `com.intellij.ideolog` 263.7307.101 (MIT, tag
`pycharm/263.7307.101`, `41beb53`): log highlighting, folding, an outline of
errors and warnings, custom Ideolog formats, Jump to Source, error and entry
navigation, links on stack frames. Checked against the WIT, SDK and host of
v0.1.37 and `main` (`f665beb`) on 2026-11-05.

Built-in format detection covers IDEA, TeamCity, Logcat, Loguru, pipe, Docker/ISO
(`2026-11-06T12:00:00Z ERROR`), Python `logging`, nginx error logs, syslog (level
guessed from keywords), Laravel/Monolog (`[date] channel.LEVEL:`, with `[stacktrace]`
and `#0 …` lines continuing the event) and Serilog text, plus custom formats; a
custom format wins ties over the built-ins. Level rules also match `err`, `fatal`,
`ftl`, `crit`, `alert`, `emerg`, `inf` and `wrn`. Jump to Source reads frames of
Java, Python, .NET, PHP, Ruby, Go, Node, Dart (`file://`, `package:`) and Swift
(`file X, line N`). Tested against the mock host only.

### The built-in `log` language can't get a plugin's syntax

- **Blocks:** highlighting `.log` files as the IDE's own `log` language, which has no grammar.
- **Why:**
  - `syntax_call` only asks the plugin that contributes the language (`PluginLanguages::owner_of`).
  - Built-in definitions keep their extensions and file names (`LanguageRegistry::build`).
  - The id-shadowing trick from the dotenv section wasn't used: it would leave detection with the built-in definition and rely on the same undocumented routing.
- **Workaround:** the plugin's own `ideolog` language claims `filenames = ["*.log"]`. Glob file names aren't filtered against built-ins and are matched before extensions. This goes against the registry's stated rule ("a plugin can't take over a built-in's extensions"), so a later IDE may close it; the fallback is Settings › File Types.
- **Would need:** `syntax-for = ["log"]`, as in the dotenv section.

### Syntax and providers in one component

- **Blocks:** Cmd+click on stack frames (`definition`) in the plugin that highlights the log.
- **Why:** same as the dotenv section: `register_plugin!` has no `syntax, providers` arm.
- **Workaround:** the `syntax` arm hand-expanded in `src/lib.rs` with `__providers_exports!`.
- **Would need:** `register_plugin!(P, syntax, providers)`.

### Highlight kinds are fixed theme categories

- **Blocks:**
  - Ideolog's red, orange and green lines, bold errors;
  - the colours of user highlighting patterns (`fg`/`bg` in the XML);
  - background colours per value ("highlight this column/value");
  - the green-to-red time-gap gradient.
- **Why:** a syntax token's kind is one of 24 names (`keyword`, `string`…). There's no colour, background, bold or italic, and a plugin can't declare its own text attributes.
- **Workaround:** error → `keyword`, warning → `tag`, info → `string`, time → `number`. A custom pattern's colour goes to the kind with the nearest hue.
- **Would need:** plugin-declared text attribute keys with light and dark defaults (foreground, background, bold, italic), editable in the colour scheme and usable as token kinds.

### Syntax results can't be refreshed

- **Blocks:**
  - a change to the custom patterns setting showing in open logs;
  - commands that change what's highlighted: highlight this value or column, toggle time-gap highlighting, remove highlightings.
- **Why:** results are cached per document version. Nothing lets a plugin invalidate them.
- **Workaround:** the setting applies after the log is edited or reopened. The highlighting commands weren't ported.
- **Would need:** `host::refresh_syntax(path | language)`.

### Syntax gets the whole document within 50 ms, and no path

- **Blocks:**
  - highlighting large logs;
  - per-file state in the syntax provider.
- **Why:**
  - `highlights(language, text)` gets the full text on the edit path, under the 50 ms provider budget. There's no viewport or range request and no progressive result.
  - It gets no path, so it can't tell files apart.
- **Workaround:** only the first 512 KiB of a log is parsed (16 ms natively per MiB; wasm is slower).
- **Would need:** range- or viewport-based highlighting, or chunked asynchronous results; the document path in syntax calls.

### No folding control or line hiding

- **Blocks:**
  - Ideolog's hide this / whitelist this / hide lines above / below / containing / not containing, and reset hidden items;
  - collapsing uninteresting events.
- **Why:** the `folding` export only offers whole-line fold ranges (see the dotenv section). A plugin can't collapse a fold, hide lines or keep them hidden.
- **Would need:** `text-editor.fold(ranges)` / `unfold`, or folds with an initial `collapsed` state plus a syntax refresh.

### No editor decorations or scrollbar marks

- **Blocks:**
  - highlighting a value or column on demand;
  - time-gap highlighting;
  - the error stripe heat map (`LogFileMapRenderer`).
- **Why:** no decoration, highlighter or scrollbar marker API. `publish_diagnostics` marks the stripe, but it also squiggles every line and fills the Problems view, so it wasn't used.
- **Would need:** `text-editor.set-decorations(id, list<(range, attributes, stripe)>)`, replaced as a set.

### The caret can't be moved

- **Blocks:**
  - extend selection (event → column → line, `ExtendsSelection`), time-gap highlighting and the heat map;
  - Go to Next Error, Next Warning, Next Exception and Previous / Next Entry in logs outside the project.
- **Why:** `text-editor` has no `set-selections` (see the JPA Assist section). `open_location` moves the caret but `resolve_in_root` refuses files outside the window's project, even the file of the invoking editor.
- **Workaround:** the navigation commands (Next Error, **Next Warning** and **Next Exception**, which have no default shortcut) call `host::open_location` on the current file, so they work only for logs inside the project. Next Exception picks the next event with an exception name (`FooException:`, `FooError(`), `Caused by:`, a `Traceback (most recent call last)` line, or a stack frame on a continuation line (on the first line only when the event is an error); the detection is heuristic, so a plain error message with a frame-like first line can still count.
- **Would need:** `text-editor.set-selections(ranges)` and `reveal(range)`, or `open_location` allowed for the invoking editor's file.

### Intentions need edits

- **Blocks:** Ideolog's Alt+Enter actions (Jump to Source, Show Ideolog settings, the hide and highlight intentions).
- **Why:** a code action must carry edits, directly or through `resolve_code_action`. None can run a command or plugin code that edits nothing.
- **Workaround:** commands in Find Action, with F7 and Shift+F7.
- **Would need:** a `command` (command id plus argument) on `code-action` instead of edits.

### Providers don't know their project

- **Blocks:** knowing which project a `definition` request on a stack frame belongs to.
- **Why:** provider calls get the document only, no `Invocation` or project.
- **Workaround:** the first `host::projects()` root that is a prefix of the document's path.
- **Would need:** the project root (or an `Invocation`) passed to provider calls.

### No text search for Jump to Source

- **Blocks:** Ideolog's ranking of candidate files with the IDE's trigram index; fast stack frame lookups within 50 ms on big projects.
- **Why:** same as the dotenv section's keys index: only `find_files` (a fresh walk, up to 200,000 entries) and `read_file`.
- **Workaround:** Jump to Source reads up to 5,000 source files inside a 60 s task and keeps the file with the longest substring shared with the message. Stack frames use `find_files("**/<file name>")`, which can take longer than 50 ms on large projects, and then a Cmd+click does nothing; F7 still works.
- **Would need:** `workspace.search(text | regex, glob, limit)` and an indexed file name lookup.
- **Closes when merged:** `l0ng-ai/openidea` #1754 adds `workspace.search-text` (needs #1752 first). It has no indexed file name lookup, so that part stays open.

### Pattern tables in a multiline string

- **Blocks:** Ideolog's settings page with tables of parsing and highlighting patterns.
- **Why:** same as *Settings are scalars only* in the Symfony section.
- **Workaround:** one multiline string setting that takes Ideolog's exported XML (`<LogParsingPattern/>`, `<LogHighlightingPattern/>`). Closed in part by the backlog pass: the *Ideolog Patterns* view adds and removes formats and patterns without XML (below).
- **Would need:** an `array` of objects setting type shown as an editable table.

### No editor banners

- **Blocks:** Ideolog's "log format not detected" and "file too large" notifications on the editor (`LogFileFormatNotificationProvider`, `LogLargeFileNotificationProvider`).
- **Why:** UI is limited to views, notifications and status items.
- **Would need:** an editor notification API (text plus buttons that run plugin commands).

### Shortcuts are dropped silently

- **Blocks:** nothing outright; F7 and Shift+F7, Ideolog's defaults, may never be bound.
- **Why:** a plugin shortcut never takes a keystroke the IDE already uses, and nothing tells the user or the plugin.
- **Would need:** a warning in Settings › Plugins or the keymap when a declared shortcut isn't applied.

### No console filters or large-file editor

- **Blocks:**
  - Ideolog's terminal module (log highlighting in the terminal) and links on stack traces in run consoles;
  - Ideolog's large file editor for logs too big to open.
- **Why:** no extension point for console or terminal output, and no paged or streamed document.
- **Would need:** a console filter provider (line in → ranges with kinds and link targets); a read-only paged document or a custom editor with streamed content.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Ideolog Patterns view:** add and remove custom formats and highlighting patterns without XML; stored in plugin storage (optional `storage`), the XML setting still works. **Status bar item** naming the detected log format (follows the focused editor, needs `events`). **`timeGapSeconds`** setting colours the time of events that follow a long pause (a colour on the time token, not a line or ruler mark).
- **Not verified in the IDE:** the view's forms, the status item refresh on editor focus.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Highlight Selection** (editor menu) adds the selected text as a highlighting pattern, *Remove Custom Highlightings* clears them; the Patterns view edits a pattern in place and reorders (Move Up). Rows #1 and #10 done; other rows stay blocked (see above).
- **Not verified in the IDE:** the editor menu entry and how the added pattern is stored.

## aritti.flyway (clean-room, inspired by JetBrains' Flyway support)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Project-local wrappers can't be run

- **Blocks:** Gradle Flyway tasks via `./gradlew`, and `./mvnw`.
- **Why:** `process:` takes a program name or an absolute path; relative paths are refused (`crates/ide-extension/src/capability.rs:155-159`), and the absolute path differs per checkout.
- **Workaround:** `flyway` or `mvn` from PATH (setting `runner`).
- **Would need:** a `process:./gradlew`-style grant resolved against and pinned per project root.

### No rename quick fix for misnamed migrations

- **Blocks:** renaming a migration to the name the checks expect.
- **Why:** same as *Code actions can't create, rename or delete files* in the Symfony section.

### Providers are chosen by language id only

- **Blocks:** completion limited to `flyway.conf` and `flyway.toml`.
- **Why:** `flyway.conf` is the `ini` language, so the plugin is loaded for every `.ini`, `.conf` and `.cfg` file (and every `toml` file) and filters by file name.
- **Would need:** `patterns` (file globs) on `[[contributes.providers]]`.

### No confirmation dialog (closed in API 1.2)

- **Closed:** Flyway: Clean asks with `host::confirm` before it runs (`engines.openidea = "^1.2"`).
- **Remaining:** `confirm` returns `Invalid` outside a command or event of a window; then `clean` falls back to the old rule of running it twice within 30 seconds.

### Problems only on open and save

- **Why:** same as *Problems aren't re-checked while typing* in the Symfony section.
- **Workaround:** for a rename done outside the IDE, run Flyway: Check Migrations.

### Flyway Migrations view

The `migrations` view (bottom, icon `database`) is a read-only tree of migrations with their state, from `flyway info` as JSON (`-outputType=json`, or `mvn flyway:info -Dflyway.outputType=json`).

- **Needs:** Flyway 10 or newer and a reachable database. It runs `flyway info` every time the view renders, including when it is first shown, and on Refresh (no live updates), inside a task, so a slow database blocks the instance meanwhile; it has no clean or migrate buttons and no database browser.
- **Rows:** Success, Pending, Failed, Missing, Out of order, Ignored, Undone, Baseline and others, matched by substring (an unknown state shows as Unknown), with icons from the host's allowlist (`ci-pending`, `ci-skipped`, …). Double-click opens the migration file found by version (versioned, `B` then `V` for a baseline), by description (repeatable) or by Flyway's `filepath`; a relative `filepath` joins the project root and is used only if readable; Flyway's synthetic `<< Flyway Baseline >>` row has no target; a failed open warns "Could not open <path>: <error>". The status line reads "N migrations, M pending".
- **Failures:** a denied capability, a spawn error, a non-zero exit, bad JSON or an empty list show a message with a Refresh button. 
- **Not verified:** whether JSON survives `mvn` output (Maven prefixes every line with `[INFO]`, which would break multi-line JSON, and it is unverified that the Maven plugin honours `outputType`), the exact JSON state spellings for the Missing and Out of order variants against a live Flyway, and the view in the IDE (mock host only). `serde_json` was added as a dependency of the plugin to parse the output.

### Added by the gap F pass (2026-11-06)

- **Run configurations:** `info`, `validate` and `migrate` are run detectors on `flyway.toml` and `flyway.conf`. They always run plain `flyway <goal>` and ignore the `runner` and `arguments` settings; URL, credentials and environment come from the config file. A config in `conf/` is not detected, and whether the host matches `files` at the project root only is not verified. `clean` and `repair` stay commands.
- **`migrate` writes to the database** with one click (the entry is labelled so), unlike the Liquibase detectors, which only read.

- **Cross-file Go to Declaration (indexer, FW8, FW11):** entity and migration tables (`@Entity`, `@Table(name)` or the snake_cased class name against `CREATE TABLE` in `V`/`U`/`R` migrations, gutter icons both ways) and `${placeholder}` in a migration to its `flyway.conf` or `flyway.toml` entry. Text rules, no entity model: `@Table` without a literal `name`, `@Id`/column mappings and naming strategies other than snake_case are not followed. Placeholders from `application.yml` (`spring.flyway.placeholders`) and custom prefixes and suffixes are not indexed. Only the first target is followed. 4096 anchors per file.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Templates and icon:** New › Flyway SQL Migration, Java Migration, Kotlin Migration, Java Callback and SQL Callback; a template can't take the migration version prompt, so name the file first; Kotlin templates have no `package` line. Flyway icon on `V*__*.sql`, `U*__*.sql` and `R__*.sql` files.
- **Clean:** closed in the confirmation limit above (`host::confirm`); kept as the 30-second rule on older hosts.
- **Not verified in the IDE:** the icon name patterns, the template placeholders.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **New Migration** asks for a description and creates `V<next>__<description>.sql` next to the latest migration through the optional `process:tee`; without it the file name is copied. Replaces the "no empty migration from a command" gap (FW2).
- **Not done (B):** FW4, 5, 7 and 10. **Not verified in the IDE:** the next-version rule against a real folder and the `tee` write.

## Liquibase Changelogs (aritti.liquibase, clean-room)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Run configurations can't confirm or ask

- **Closed in API 1.2:** the commands *Liquibase: Update (Asks First)…* (`host::confirm`, then `liquibase update` in a terminal) and *Liquibase: Rollback Count…* (`host::input` for the count, then `host::confirm`, then `liquibase rollback-count N`) need the optional `process:liquibase` capability. Where the host answers `Invalid` to `confirm`, `update-confirmed` does not run. They use the `liquibase` on `PATH` and start in the project root, so `liquibase.properties` is found there only.
- **Blocks:** per-changelog runs, and a confirmation on the *Liquibase: update* run configuration (a detector, so it still runs on one click).
- **Why:** `runDetectors` take fixed `args` and match marker files at the project root only (`crates/ide-host/src/plugins/run_detectors.rs:134-140`). `update` runs on one click, and a `liquibase.properties` outside the root isn't detected.
- **Would need:** run configurations computed by code, or `ui.show-input` / a confirm dialog.

### No file-exists API

- **Blocks:** a reliable "include not found" check.
- **Why:** include resolution reads each included file with `read_file`, and `includeAll` uses `find_files`, which skips hidden and gitignored directories.
- **Would need:** `workspace.project.exists(path)` or a `stat` call.

### Missing files are reported as `Invalid`

- **Blocks:** telling a missing file from a bad path by error kind.
- **Why:** the host's `read_file` returns `Error::Invalid("...: no such file")` for a missing path (`crates/ide-plugin/src/sandbox.rs:17`, via `crates/ide-plugin/src/wit_host.rs:323-326`), while MockHost returns `NotFound`. Existence checks must treat both as absent.
- **Would need:** `NotFound` for a missing file, or an `exists` / `stat` call.

### No source roots or classpath

- **Blocks:** resolving `classpath:` includes exactly.
- **Why:** they are resolved against `src/main/resources` and `src/test/resources` by path heuristics.
- **Would need:** `project.source-roots()`.

### Liquibase Changesets view

The `changesets` view (bottom, icon `database`) shows changesets in execution order across includes. It reads text only; no `liquibase` process runs.

- **Discovery:** changelogs are found with `find_files` (xml, yaml, yml and sql, 2,000 in total, 3 s cap), per render (the plugin has no `events` capability, so no cache). Hidden or gitignored files aren't listed, though a file reached by an include is read by path. A cut at 2,000 changelogs or 3 s is reported as `partial` in the status line.
- **No master changelog concept:** roots are the changelogs nobody includes, so `changeLogFile` in `liquibase.properties` isn't consulted; changelogs in pure cycles are shown after the roots.
- **Includes:** `includeAll` expands recursively in path order and ignores `maxDepth`, `filter` and `resourceComparator`. Includes with `${property}` are flagged unresolved; missing includes and cycles are flagged nodes. A changelog reached a second time is shown once as a leaf with the detail `already run`.
- **Rows:** each changeset is described by its comment, else its first change type or first SQL words; a click opens the file at the changeset's line. XML descriptions come from string scanning, not the element parser.
- **Not verified:** the view in the IDE (the open line is 0-based per the host docs; mock host only).

### Shared with other plugins

- Diagnostics come only from the `line-markers` provider, and there is no database API: see the Spring plugins rows "Diagnostics have no provider of their own" and "No database API".

### Added by the feasible-parity pass (2026-11-06)

- **`unresolved-property`** (`checkProperties`, new settings schema): a `${name}` in a changelog that no `property` declaration in a scanned changelog, no `liquibase.properties` or Spring `application*` config (`[spring.][liquibase.]parameter(s).<name>`, profile and module files too) defines. `database.*` is built in. Quiet when no config file is readable, on `includeAll` anywhere, an unresolvable include, `property file=`, `defaultsFile`, `spring.config.import|location|additional-location|activate`, dynamic names (`${:x}`, `${}`), `$${x}`, `\${x}`, XML comments and SQL `--` lines, names not starting with a letter or `_`, and scan caps (1,000 listed, 200 changelogs, 256 KB). Limits: JSON changelogs are unchecked; `LIQUIBASE_PARAMETER_*` environment variables are not read; the scan over likely changelogs repeats on each check with an undeclared placeholder (bounded, not cached); `find_files` skips hidden and ignored directories, so config there is best effort; `spring.profiles` is not a quiet trigger, profile files are read instead. The `${:name}` escape syntax is not verified in the documentation and is treated as dynamic.

- **References (indexer, LQ7):** Go to Declaration from `include file` to the changelog of that file name, from `rollback changeSetId`/`changeSetAuthor` to its changeSet, and from `tableName`, `baseTableName` and `referencedTableName` to `createTable`. Includes resolve by file name only (the same name in two folders gives both targets), tables are matched case-insensitively and a `createTable` written in formatted SQL is not indexed. Columns, contexts and labels, completion and rename stay out (the index has no usages or rename). 4096 anchors per file.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Gutter link** on each `include` to its changelog file (only for includes that resolve; `includeAll` has no single target); the marker only navigates, no run icon. Liquibase icon on `db.changelog-*` files.
- **JSON changelogs:** checks, changesets view, Go to Definition on `include` and project index.
- **Completion:** `include` file paths (XML, YAML); `changeSet` author names (setting or git `user.name`, optional `process:git`); in XML changesets without a declared schema, change type elements and attributes (quiet when the root declares a schema, whose XSD the editor's server uses).
- **Index:** include to changelog, `rollback changeSetId` to its changeSet, table names to `createTable` (see the LQ7 bullet above).
- **Commands:** *Update (Asks First)…* and *Rollback Count…* (see the closed limit above).
- **Not verified in the IDE:** all of the above.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **New Changeset** adds a changeset (next numeric id, configured author) to the open XML, YAML or SQL changelog (optional `editor.write`). **New Changelog...** creates a changelog file in the chosen format through the optional `process:tee` (content copied without it) and adds an `include` to the open changelog when the new file sits in a subfolder.
- **Not done (B):** LQ3-6, 7, 11-15 and 17. **Not verified in the IDE:** the include insertion and the SQL header text.

## aritti.openapi (clean-room, inspired by com.intellij.swagger)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Preview opened automatically for any OpenAPI file | `customEditors.patterns` match file names only; an OpenAPI file is a generic `.yaml`/`.json`. Other names need Open With | A content matcher on custom editors (first-line regex or top-level key), as languages have `first-line` |
| Schema validation by the language server, and without editing the file | No way to register a schema with the built-in `yaml-language-server` / `vscode-json-language-server`, so the plugin inserts a modeline into the YAML file (same gap as kube-explorer); JSON files get no modeline, only the plugin's own `$ref` checks | Schema associations for built-in language servers |
| Rename of a component everywhere, find usages of a schema, hover with the target's definition | No `rename`, `references` or `hover` provider kinds | Those provider kinds |
| Structure view of a spec | `syntax-provider` outline only runs for plugin-owned languages; `yaml` is built in | An outline provider for any language |
| Problems while typing | No `document-changed` event or diagnostics provider (see Symfony section) | Same |
| Transitive `$ref` files in the preview | The page can't read project files itself; the plugin posts the files the open document references directly, one level deep, at most 50 files and 512 KiB (extra files are skipped and counted) | A read-only project file API for web views |

### Implementation limits

- JSON specs (a text starting with `{` or `[`, and mentioning `openapi` or `swagger`) get `$ref` completion, navigation and the same problems as YAML, through a small scanner (`src/json.rs`, no `serde_json`). `package.json`, `tsconfig.json` and other non-spec JSON are ignored; a JSON file whose root is an array is treated as unreadable; string escapes beyond `\"`, `\\`, `\/`, `\uXXXX` and the common ones are not decoded. The endpoints view and the schema modeline code action stay YAML-only.
- Flow-style mappings are only seen for `$ref`.
- `$ref` file-name completion (`./x.yaml`, `../y.json`, relative to the spec) reads a file list cached on open and save of a yaml/yml/json file (`find_files`, 3 patterns, 500 files each; the host skips hidden and gitignored paths), because `find_files` is too slow for the 50 ms budget. The list is stale until the next open or save, and it is offered only when the typed value has no `#`.
- The preview resolves relative `$ref` files one level deep (see the preview row above). The page sends a hash and a random id (`crypto.getRandomValues`, `Math.random` fallback) with its request, the plugin answers only when the hash changed, and the page ignores replies whose id isn't its own, because `post_view_message` reaches every preview of the view. Files are re-read on every (300 ms debounced) edit. **Not verified in a live IDE:** the whole web side (the `fetch` override answering from the posted files, the postMessage round trip, the id filter) and `swagger-client` calling the global `fetch` at request time; only the Rust half is tested.
- The endpoints view re-scans the project on each render.
- The dark theme is a colour inversion.
- A missing `$ref` file is told apart from an unreadable one by treating `NotFound` and `Invalid` as missing: see "Missing files are reported as `Invalid`" in the Liquibase section.

### Added by the feasible-parity pass (2026-11-06)

- **Templates:** New-menu entries for OpenAPI 3.1 (YAML and JSON) and AsyncAPI 3.0 (YAML) with a `$NAME$` title only. The AsyncAPI file is not checked as OpenAPI (detection needs an `openapi` or `swagger` key). Not verified in the IDE: `$NAME$` expansion.

### Added by the gap F pass (2026-11-06)

- **`missing-openapi-root`** (Info on the `paths` key, setting `checkSpecCandidates`): a YAML or JSON file with a root `paths` object of `/...` items holding HTTP methods but no `openapi` or `swagger` key, such as a spec fragment. Files over 1 MiB are skipped by this hint; a file that parses as a spec is still checked at any size.
- **JSON in the endpoints view:** JSON specs are listed next to YAML ones. A JSON file is read in full before the 1 MiB cap applies, because the host has no size query.
- **Skeletons:** `openapi-path`, `openapi-operation` and `swagger2-root` are plain completion items (not live templates with abbreviations), offered anywhere in a spec file and not context aware, so `swagger2-root` also appears in an OpenAPI 3 file. Snippet tab stops are not verified in the IDE.
- **Templates:** Swagger 2.0 (YAML) and Swagger 2.0 (JSON) join the New menu.
- **Reload:** the preview has a Reload button. Cargo cannot run the page script, so it is not verified.

- **Compare with another spec (OA6):** *OpenAPI: Compare With Another Spec…* picks a base among the project's other specs (`find_files` skips dot-directories and ignored paths, files over 1 MiB are skipped) and lists added, removed and changed operations, `components`/`definitions` entries and top-level sections in the *OpenAPI Changes* view (base to current buffer; double-click opens it). The diff is on flat key paths and values: `$ref`s are not resolved, moved or renamed operations show as a removal plus an addition, and there is no side-by-side text diff or breaking-change classification. The view icon (`web`) is unverified.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Editing:** keyword and enum completion in OpenAPI 3 YAML (quiet in JSON, Swagger 2 and flow style); *Extract inline schema to components/schemas* (YAML only; rename the new schema afterwards); *Copy Self-Contained Spec* (bundles external `$ref` into components; the target must be a block mapping; a recursive reference is copied once); *Copy Reference* to the key at the caret; *Compare With Last Revision* (git, optional, a committed file needed); operation gutter markers that open the operation's first local schema; setting `specGlobs`; a file icon.
- **Preview:** a Redoc view (bundles Redoc 2.5.4, MIT) next to Swagger UI; not verified in a live IDE (it needs a worker, scripts in `Html` views may be blocked).
- **Skipped (OA-2 partial):** the JSON edits (extract, bundle, schema modeline) are YAML only.
- **Not verified in the IDE:** marker behaviour and all page scripts.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Live templates:** `oas3` (OpenAPI 3.1 root), `opnp`, `endp` and `swag`; key and enum completion in OpenAPI 3 JSON; *Extract inline schema* and *Self-Contained Spec* also for JSON (this closes the "YAML only" skip above).
- **Files are written:** *Write Requests for the HTTP Client* and *Write Self-Contained Spec* save a file beside the spec through the optional `process:tee` (clipboard without it).
- **`specGlobs`** files now get problems and appear in the endpoints view; the preview patterns stay fixed (use Open With). Earlier text said it fed completion and go-to-definition only.
- **Skipped:** OA10. **Not verified in the IDE:** the written file locations, JSON edits on real specs.

## aritti.dependency-audit (clean-room, inspired by JetBrains' package vulnerability checks)

Checks Maven, Gradle (incl. version catalogs), npm (package-lock, yarn, pnpm), Composer, Cargo and PyPI dependencies against OSV.dev. Checked against v0.1.37 on 2026-11-05.

### Missing API

- **No build model**
  - **Blocks:** versions managed by a BOM, a parent POM or a Gradle plugin (`libs.versions.toml` aliases with a version are read).
  - **Why:** a plugin sees text, not the resolved dependency graph.
  - **Workaround:** such dependencies are skipped; versioned ones are still checked.
  - **Would need:** the IDE's Maven/Gradle model through WIT, or accepting `process:mvn`.
- **Diagnostics can't link to the advisory**
  - **Blocks:** a click from the squiggle to the advisory page.
  - **Why:** `Diagnostic` has `code` but no URL (`sdk/rust/openidea-plugin/src/types.rs`, `Diagnostic`).
  - **Workaround:** the Vulnerable Dependencies view links each id to osv.dev.
  - **Would need:** LSP's `codeDescription.href`.
- **No lockfile-changed event**
  - **Blocks:** re-checking after `npm install` or `composer update` run outside the IDE.
  - **Why:** only opened and saved files raise events.
  - **Workaround:** reopen the file or run the scan.
  - **Would need:** `files-changed` (see Symfony, "No file-change events").
- **Every file open wakes the plugin**
  - **Why:** events can't be filtered by glob; see Symfony, "Events can't be filtered, and providers get no project". The plugin drops other paths itself.
- **Network is synchronous within the call budget**
  - **Blocks:** a large first scan finishing reliably.
  - **Why:** event handlers get 5 s, 60 s with a task; queries are cached for 24 h, so only the first scan of a big lockfile is slow.
  - **Would need:** a background fetch or task API.
- **Storage rewrites the whole file on each write**
  - **Blocks:** per-entry caches.
  - **Why:** `host::storage::set`/`delete` read, parse and rewrite the plugin's whole storage file (`crates/ide-host/src/plugins/api.rs:284-299,399-411`), so per-entry writes cost O(n²) I/O. The file is capped at 1 MiB, so a scan of a big project may not fit.
  - **Workaround:** one JSON blob for the query cache, written once per run; the scan notification says when results were too big to keep.
  - **Would need:** batched writes or a real key-value store.
- **Missing files are reported as `Invalid`**: the lockfile lookup treats `NotFound` and `Invalid` as absent; see the Liquibase section.

### Not done, but possible with the current API

- Go modules and other lockfiles (pnpm v5, and catalog entries in `libs.versions.toml` that span several lines, are not read).
- CVSS scoring (severity comes from the advisory's labels).

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- 12 manifest and lockfile formats are checked (also `libs.versions.toml`, `yarn.lock`, `pnpm-lock.yaml`, `Cargo.lock`, `poetry.lock`, `requirements.txt`), and the upgrade quick fix works in `libs.versions.toml` and `requirements.txt` (DA5). Replaces the older 7-format wording. DA8 is skipped.
- **Not verified in the IDE:** the version-catalog and requirements edits.

## aritti.quarkus (port of com.redhat.devtools.intellij.quarkus)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Quarkus language servers can't be used

- **Blocks:** running the original's LSP4MP (quarkus-ls) and qute-ls, so the plugin is text-based. Hover, go to definition, code lens, inlay hints, JAX-RS symbols, Java diagnostics and Qute support are dropped.
- **Why:** those servers get every project fact (`microprofile/projectInfo`, `microprofile/propertyDefinition`, `qute/template/project`) by sending requests to the client. OpenIDEA's LSP client answers any request it doesn't know with `METHOD_NOT_FOUND` (`crates/ide-lsp/src/handler.rs:274-310`), and jdtls bundles are hard-wired to java-debug, the usages bundle and the Kotlin bridge (`crates/ide-host/src/project.rs:195-198`), so a plugin can't add `com.redhat.microprofile.jdt.core`. A plugin's `languages` can't take over the built-in `properties`, `yaml` or `html` ids either, so it couldn't attach a server to those files. Without `projectInfo`, quarkus-ls offers no property completion.
- **Would need:** a way to answer server-to-client requests from the plugin, plugin-supplied jdtls bundles, and language servers attached to built-in language ids.

### Dev Mode run detectors are fixed commands matched by file name

- **Blocks:** `./mvnw quarkus:dev` and `./gradlew quarkusDev`; showing Dev Mode only in Quarkus projects; debugger auto-attach on port 5005.
- **Why:** wrappers can't be run, same as *Project-local wrappers can't be run* in the Flyway section. A detector matches its `files` markers by name only (`crates/ide-host/src/plugins/run_detectors.rs`), so it can't look inside `pom.xml` for `io.quarkus`: the three configurations show in every Maven or Gradle project once `process:mvn|gradle|quarkus` is granted. The command is fixed and there is no debugger API.
- **Would need:** a content predicate on detectors and the per-project wrapper grant from the Flyway section.

### No metadata from libraries

- **Blocks:** keys of Quarkus extensions beyond the bundled ones (about 170 of the most-used `io.quarkus` extensions, of which 86 contribute their own keys: 2,797 properties; Quarkus 3.40.1 metadata) and of dependency jars. Quarkiverse and third-party extensions stay unknown.
- **Why:** same as the row *Dependency jars and the classpath can't be read* under *Gaps that affect every Spring plugin* in the Spring plugins section. The bundled catalog covers a curated set, and the generator takes extra artifact ids.

### No project wizard

- **Blocks:** creating a Quarkus project in the IDE.
- **Why:** same as *Code actions can't create, rename or delete files* in the Symfony section (a plugin can't create files). The command opens code.quarkus.io in the browser instead (a command could write files through `process:tee`; not used here).

### Added by the feasible-parity pass (2026-11-06)

Settings `quarkus.checkSyntax`, `quarkus.checkValues`, `quarkus.checkExpressions`; the checks share the place of `deprecated-property`/`unknown-property`, so a fixed file publishes an empty list.

- **Syntax:** `duplicate-key` (properties and YAML, per `---` document), `missing-separator` (a line with a single token only; `key value` is valid Java properties syntax), `unassigned-value` (hint, only for a known non-map property whose metadata has a default).
- **`invalid-value`, conservative:** booleans are never flagged (SmallRye reads anything but a few spellings as false); closed enums from the metadata compare case-, `-`- and `_`-insensitively and accept the property's default; log levels take the JUL and common names or an integer; durations take an integer, ISO `P...` or `<number><unit>` runs; list, map and `${`/`\` values are skipped. The enum leniency is assumed, not verified. A rule that over-accepts on non-JUL closed enums is possible.
- **`undefined-expression`:** only in `application.properties|yaml|yml`, and only a typo candidate: a name of six or more characters, not environment-shaped, not a system prefix, catalog key or defined key, within edit distance 2 of a key defined in the same file. A typo rule can fire on sibling numbered names defined only elsewhere. The `quarkus.` prefix is treated as a system prefix, which silences that catalog check.
- **Not done:** required-property check (`missing-required`): the bundled metadata has no required or optional information. Closed in part by the backlog pass: the opt-in setting `quarkus.checkRequired` (below).

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Named instances:** `quarkus.datasource."users".jdbc.url` completes and is checked as the unnamed key, for the roots the metadata lists as named (datasource, hibernate-orm, flyway, liquibase, mongodb, redis, oidc, oidc-client, mailer…).
- **Required keys (opt-in `quarkus.checkRequired`):** an Info note on a key whose sibling is set while a required key (no default, not `Optional`) is missing from every `application*` file and `.env`. Keys of extensions that are not in the bundled catalog are unknown.
- **Java (text only):** completion in `@ConfigProperty(name)` and `@ConfigMapping(prefix)`, checks for `@RestClient` without `@RegisterRestClient` (`quarkus.checkAnnotations`) and `@Scheduled` expressions, opt-in `quarkus.checkUnassigned`, quick fixes to add a `defaultValue` or the key to `application.properties` (existing file only); Go to Declaration and gutter markers between config keys and `@ConfigProperty`/`@ConfigMapping` declarations, both ways.
- **Skipped (P5 #12):** Qute support (needs qute-ls, see above).
- **Not verified in the IDE:** the Java provider routing and the markers.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Java checks:** health checks (`@Liveness`/`@Readiness`/`@Wellness` without `HealthCheck`, `HealthCheck` without a qualifier, with quick fixes) and `@Gauge` (scope, return type), setting `quarkus.checkHealthMetrics`; quick fix *Add @RegisterRestClient* (and its import). Row #8 done, #6 partial (not verified which part), other rows stay blocked (see above).
- **Not verified in the IDE:** the quick fixes' import placement.

## aritti.ignore (port of mobi.hsz.idea.gitignore)

Source port (`.ignore`, Marketplace 7495). Audited on 2026-11-06 from the Marketplace page and the project's README (task 0 audit); our side from the repo, feasibility from this file. Not run side by side with the original.

| Not implemented | Why | API that would unlock it |
|---|---|---|
| Add to or unignore from the Project view or editor context menu (a menu command per selected path) | Partly closed with API 1.2: *Add Selected Paths to the Open Ignore File* (`projectView.context`) appends the selected paths, anchored and relative to the focused ignore file, to that file (any ignore dialect). It can only edit the focused editor, so it can't write a `.gitignore` that isn't open (*New Ignore File* creates one through `process:tee`), and there is no unignore variant (*Unignore Selected Paths* exists since the audit v2 pass) or trailing `/` for folders. *Add Selected Paths* now asks (`host::pick`, API 1.2) which ignore file: the focused one or a root `.gitignore`, `.dockerignore`, `.npmignore`, `.prettierignore`, `.eslintignore` that exists (found by probing with `read_file`, since `find_files` skips dot files). A command edits only the focused editor, so for another file it opens it with `open_location` and edits only if that file has become the focused editor; otherwise it says to run the command again. Where the pick is refused the focused file is used. The host's own Add to .gitignore covers the root file, and the code action "Add to .gitignore" / unignore works in any open file | A workspace-edit or write-file call |
| Highlighting of the built-in `ignore` language (`.gitignore`, `.dockerignore`) | Built-in language ids can't take a plugin's syntax; the plugin's own `dotignore` language covers `.ignore` | `syntax-for = ["ignore"]` (see the dotenv section) |
| Bracket matching | The language format has no brackets rules | `brackets` rules in the language format (see the Twig editing section) |
| Show ignored files for an ignore file; close open ignored editors; rename entries | No Project view hooks, no editor-close API, no rename provider | Project view extension, `workspace.close-editor`, `rename` provider |
| Import and export of user templates | Needs a file picker and popups | A file picker and popups |
| Template filter by name or content in the Ignore Templates tool window | Not built (the tree view has no filter yet) | — |
| "Unused entry" inspection | Not done: `find_files` skips ignored and hidden paths, so an entry that matches only ignored files would be a false positive | `find_files` option to include ignored files |
| New › Ignore File creating `.gitignore` | The New menu builds the file name from the typed name and the template's extension, so it creates `<name>.ignore` (the extension the plugin recognises); dot-named files are recognised when opened, not created. Whether the host accepts an empty name was not tested in the app. Superseded by the *New Ignore File* command (audit v2 pass), which writes the dot-named file through `process:tee` | — |

Implemented since: the redundant-entry check also flags a glob with literal leading directories when another entry matches that directory (`build/*.o` under `build/`, `/x/**/y` under `/x/`; `["*.log", "debug.log"]` flags `debug.log`, `["!debug.log", "*.log"]` flags nothing). It is deliberately conservative: unanchored globs (`x*` against `*.log`) are never flagged, and `.hgignore` files (`syntax: glob`) don't get the covered check. The New menu entry "Ignore File" ships a starter body.

Added by the gap F pass on tooling (2026-11-06): the setting `unignoreActions` (default on) turns the unignore variant of the quick fix off; starred templates are kept in storage and shown in a *Starred* group (how the group renders in the IDE is not verified), and deleting a user template drops its star; *Add to .gitignore* appends keep a CRLF file CRLF. The original's import and export of templates and the template filter are still not built.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Path completion** of entries in glob-syntax ignore files (visible files and folders only: `find_files` skips ignored and hidden paths; `.hgignore` `syntax:` lines get none).
- **Add Selected Paths** now asks which ignore file to add to (the open one or a root `.gitignore`, `.dockerignore`…); a file that is not open is opened first and the command must be run again, because a command edits only the focused editor.
- **Not verified in the IDE:** the open-then-rerun flow and the path popup.

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **New Ignore File** (Project view, Tools) creates a dot-named ignore file of the chosen type through the optional `process:tee` (clipboard fallback when denied). This supersedes the table row saying the New menu cannot create `.gitignore`; it is a command, not a New-menu entry. *Unignore Selected Paths* exists as a command, and both commands are also in the editor context menu (the current file when nothing is selected).
- **Notifications:** results of these commands are shown with `host::notify`; the older "run the command again" text returned from a command was dropped by the frontend and is replaced. Rows #3 and #4 done.
- **Not verified in the IDE:** that `tee` accepts a dot-named target and the menu context.

## aritti.properties (port of com.intellij.properties)

Navigation covers `.html`, `.htm` and `.xhtml` (host language `html`) and `.jsp` and `.jspx` (host language `jsp`). Key navigation and the missing-key warning are text scans, not type resolution. Source port (Apache-2.0); the HTML/JSP sites and the missing-key warning are clean-room from the Spring and Thymeleaf documentation.

| Not implemented | Why | Workaround |
|---|---|---|
| Go to Definition and warnings in FreeMarker (`.ftl`) | the host has no language id for `.ftl`, so a provider can't attach to it | none; use Search Everywhere on the key |
| Warnings for `#{key}` in `.xhtml` and JSP | there `#{...}` is Java EL (JSF beans), not a message; navigation still works | none needed |
| Warnings with several candidate bundles (more than one of `messages`, `ValidationMessages`, `i18n/*`, or the same name in two folders) | without types the plugin can't tell which bundle a call reads | turn on one bundle layout, or ignore: the check stays quiet |
| Warnings for `bundle.getString("k")` on a variable, or `message("k")` | the receiver's bundle is unknown | use `ResourceBundle.getBundle("name").getString("k")` inline |
| Keys supplied by library or jar bundles (`typeMismatch`, `javax.validation…`) or by non-`.properties` message sources (YAML, database) | `Project::read_file` can't read jars and the plugin only indexes `.properties` files, so it can't know these keys | none; this is why only `getMessage` on a receiver named like a message source (`messageSource`, `messages`, `…Accessor`) with a `Locale` third argument is checked |
| Rename of a key across bundles and code, references (Find Usages) | no rename or references API for plugins | Find Unused Properties, and Remove from every locale |
| Warnings refreshed when a `.properties` file changes | diagnostics are published on open and save of the code file | reopen or save the file |

Added by the gap F pass on tooling (2026-11-06): New › Properties File, declared in the manifest only, so it can't be switched off. Its template body is a single newline; `openidea-plugin check` accepted it, but whether the host accepts a file template with an empty body was not tested in the IDE.

### Added by the backlog pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

Live unused keys as hints (setting, off by default; they run on open and save, not while typing), Go to Declaration lists every locale of a bundle, the Resource Bundle editor opens another locale's file when its cell is edited (a plugin can't write to a file that isn't the focused editor) and can show incomplete keys only. Not verified in the IDE (mock host only).

### Added by the audit v2 pass (2026-11-06)

Branch `arnaud-ritti/implement-new-feature`. Mock host only; nothing below is verified in the IDE unless it says so. Checks stay quiet when a name or class is not in project sources.

- **Resource Bundle editor** writes the cells of other locales to their files through the optional `process:tee` (setting `bundle.writeOtherLocales`); without the permission it opens the file as before. This lifts the "single-file edits only" limit for the editor (row #7). Live unused keys, locale-wide Go to Declaration and incomplete-keys filter are as in the previous block.
- **Not verified in the IDE:** concurrent edits of the same bundle.

## All plugins

Checked against the host and frontend source of v0.1.37 on 2026-11-05.

### No translation support

- **Blocks:** translating any plugin into the IDE's other UI languages (Chinese, Korean, French, Polish, Portuguese, Spanish). Every `aritti.*` plugin shows its titles, buttons, tree items, form labels, settings, notifications and Html pages in English.
- **Why:** the manifest has no translation files and the WIT has no locale function, so a plugin can neither ship translations nor ask which language is selected. The IDE translates its own text through built-in catalogs (`src/platform/locales/<locale>.json` and `locales/text/<locale>.json`, English source text → translation) that plugins can't add to. Its UI primitives run every string through that catalog (`src/ui/Button.tsx:31`), so a plugin label that happens to match IDE text ("Refresh", "Cancel", "Delete") shows translated next to English plugin-specific text: a mixed UI.
- **Would need:**
  - a `locales/<tag>.json` catalog in the package (English source text → translation, same format as the IDE's text catalogs), applied by the host to manifest strings (command, view and setting titles, schema labels) and to view data (buttons, items, fields, status);
  - `host::locale() -> string` (BCP 47 tag) for text the plugin builds itself: notifications, error messages, Html bodies.

### Scripts in `Html` view bodies are blocked in the desktop app

- **Blocks:** everything in an `Html` body that needs its script: syntax colouring and Apply in Kube Explorer's YAML panel, clicking or keyboard-opening a node in the Spring Bean Graph, line comments in GitLab's Changes view. With the old pages the Kube Explorer panel was blank, because its text only showed through the script.
- **Why:** the host renders an `Html` body as an `srcdoc` iframe (`src/features/plugins/views/PluginView.tsx`). An `srcdoc` document inherits the CSP of the window that embeds it, and the Tauri window's CSP (`src-tauri/tauri.conf.json`) has `script-src 'self' 'wasm-unsafe-eval'` without `'unsafe-inline'`. The frame's own CSP allows inline scripts, but both policies apply, so every inline `<script>` is blocked. Inline styles and static HTML/SVG still work, and the reference documentation says inline scripts are allowed. The theme variables (`--openidea-*`) are set by the host's own bridge script in the frame, so they are missing too: SVG fills fell back to black and the Spring Bean Graph drew black boxes with invisible text and edges. Reproduced in WebKit with the IDE's CSP on the parent page.
- **Workaround in the plugins:** each page's stylesheet defines the `--openidea-*` variables with system colours (`Canvas`, `CanvasText`, `GrayText`, `Highlight`…), which follow light and dark through `color-scheme` (set from `data-theme` in the markup, no script needed); the bridge's inline values override them when it runs. Pages show their content without scripts and add interactivity only when the script runs (a `js` class set by the script's first line). Input that must reach the plugin goes through native bodies (`Form`) and toolbar buttons. Pages say when a feature needs scripts.
- **Would need:** serving `Html` bodies from the plugin protocol the web views already use (`openidea-plugin:` is in `frame-src`), so the frame doesn't inherit the window's CSP; or a per-frame nonce added to both policies.

### A command's result text is dropped

- **Blocks:** confirmation of what a command did ("DDL of Owner copied", a reindex summary, `bin/console` output).
- **Why:** `run-command` returns `option<string>`, but every frontend caller ignores the value (`src/features/plugins/points.tsx`, `runPluginCommand` in `store.ts`); only failures become notifications.
- **Workaround in the plugins:** `host::notify` instead of returning text (ignore switched to it in the audit v2 pass).
- **Would need:** the frontend showing a returned string as an info notification titled with the command, or the WIT documenting that the result is unused.

### Form fields can't be read-only

- **Blocks:** showing data (a merge request's title, author, description) in a `Form` without it looking editable.
- **Why:** `Text` and `Multiline` fields are always inputs; edits the plugin ignores are silently lost.
- **Would need:** a `readonly` flag on `field`, or a `Label`/`Info` field value.

### Not verified in the IDE (feasible-parity pass, 2026-11-06)

Built and tested against the mock host only; the IDE probe (plan Task 1) was not run. Open in the real host:

- the `$NAME$` and `$END$` placeholders in New-menu templates (Symfony, Kubernetes, OpenAPI/AsyncAPI);
- prefilled code-action edits applied without a resolve step (Laravel and Kubernetes intentions, the `simplify-order-by` fix);
- the language ids `vue`, `javascriptreact` and `typescriptreact`;
- `on-event:file-opened`/`file-saved` activation in the real host (gitlab, kube-explorer, devcontainer, symfony, spring-beans) and `on-file:` globs for dot-directories;
- completion on `@` in a plugin language;
- the first file that activates `blade` is checked on its next open or save;
- gap F pass: several edits of one code action applied against original coordinates, the snippet `\$` escape, indentation and `$0`, `$END$` in the new Symfony service templates, the `fileIcons` name patterns, the status item `reindex` command, the replace range of an empty YAML tag value, and the `Invalid` read-error kind.

- **spring-initializr:** Pick Spring Boot Version and Pick Dependencies (buttons) load `/metadata/client` and use `pick` / `pick-many`; they only work for the default `https://start.spring.io` because the optional `net:start.spring.io` is a fixed host (a custom server still means typing ids). `pick-many` cannot preselect, so picked ids are merged into the field. Add Spring Boot Starters (Tools menu command) is the Boot Add Starters feature, kept here beside the catalogue: it resolves coordinates through `/dependencies`, inserts into the open `pom.xml` (the project-level `<dependencies>`, before `<build>`/`<profiles>`, matching the existing indent) or `build.gradle(.kts)` (top-level `dependencies { }`, `implementation`/`runtimeOnly`/`testImplementation`) with `apply-edits`, and skips starters already present and those that need a BOM or repository (listed in a notification). It cannot create the dependencies block or edit another file than the focused one, and needs `editor.write`. Prompts and fetches are only verified against the mock host.
