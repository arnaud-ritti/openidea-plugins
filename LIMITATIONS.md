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
  - Twig `paths:` namespaces outside `templates/`;
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
- **Would need:** `host::pick(items) -> option<key>` and `host::prompt(label, default) -> option<string>`.

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

### Not ported, but possible with the current API

These aren't API limits; they were left out for size:
- the Symfony profiler (it reads `var/cache/dev/profiler/index.csv`);
- Twig `paths:` namespaces from `config/packages/twig.yaml`;
- Twig function and filter completion from a static list;
- the rest of the text-only inspections (duplicate XML service keys, YAML quoting).

## Laravel (`aritti.laravel`) and Blade (`aritti.blade`)

Port of `de.espend.idea.laravel` 0.15.4 (the free Laravel plugin, MIT) plus features that follow Laravel's documentation. Checked against the SDK, WIT and host source of v0.1.37 on 2026-11-06. The gaps in the Symfony section above apply here too (no PHP semantic model, no references, rename or hover, code actions can't create files, problems aren't re-checked while typing, one target per gutter marker); this section only adds what is specific to Laravel.

### Run configurations: artisan

- **Blocks:** choosing the artisan arguments (`serve --port`, `test --filter`), or a `./artisan` wrapper such as Sail.
- **Workaround:** run detectors are fixed commands matched by file name: `php artisan serve`, `test`, `queue:work` and `schedule:work` appear in every project that has an `artisan` file, Laravel or not. Use Laravel: Show Route List's terminal or the IDE's terminal for anything else. Laravel: Fresh Migration asks twice within 30 seconds because it drops all tables.

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
- Artisan `make:*` commands: there is no input dialog for the name.
- Intentions on the open file are code actions (⌥⏎): validation string to array (not offered when a rule is `regex:`/`not_regex:`, whose pattern may hold pipes), `'UserController@index'` to `[UserController::class, 'index']` (only when the routes file is in `App\Http\Controllers` or imports the class with a plain `use`; imports are never inserted), and the Blade `{{ }}` / `{!! !!}` toggle. Rule string to rule object needs a new class file, which plugins can't create.

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
| Which candidates are actually active (`@ConditionalOn…`, `@Profile` vs active profiles, `@Import`, component-scan packages) | No build model (dependencies, modules, source sets) and no run-configuration data (active profiles) | Project model API: modules, source roots (main/test/generated), dependencies; run configurations and their arguments/env |
| Generated sources and annotation-processor output (`target/classes/META-INF/spring-configuration-metadata.json`, Lombok/MapStruct output) | `find_files` skips hidden and gitignored paths (`target/`, `build/`); `read_file` works only with a known path | A `find_files` option to include ignored paths, or the generated source roots from the project model |
| Hover documentation: bean type, scope, profiles, candidates; SpEL variable types | No hover provider | Provider kind `hover`: (document, position) → markdown |
| Find usages of a bean, renaming a bean everywhere (`@Qualifier("x")`, XML `ref="x"`, SpEL `@x`) | No references or rename provider | Provider kinds `references` and `rename` (with workspace edits across files) |
| Choosing between candidate beans from the gutter | `LineMarker` has one `target`, so a marker with several candidate beans opens only the first. Icons are a fixed allowlist (`navigate`, `implemented`, …), so there is no bean icon or Spring-specific icon | `LineMarker` with several targets (shown as a chooser) and plugin-supplied icons (SVG from the package) |
| Advised-method markers (which methods a pointcut matches) and full AspectJ pointcut checking | Matching `execution(* com.acme..*Service.*(..))` needs every method's signature and the type hierarchy, libraries included | Same as type resolution, plus member/signature queries over the classpath |
| "Create bean class" / "Create missing XML bean" quick fixes | Code-action edits can only change files that already exist | Code-action workspace edits that create files (and open them) |
| Live index after external changes (git checkout, generated files, edits in another tool) | Events cover only files opened/saved in the IDE; there is no file-system watcher event. The index re-scans every 120 s instead | A `files-changed` event (created/changed/deleted project paths) |
| Indexing large projects without blocking | Editor providers have 50 ms and commands 5 s; there is no background job a plugin can run between calls. The index reads files a little at a time inside provider calls, so markers show up only after a few calls | A background task API (runs outside provider budgets, can be cancelled) or a host-side text index the plugin can query |
| Native bean dependency diagram (zoom, layout, export) | No diagram/graph view type; the graph is drawn by hand as SVG in a sandboxed `Html` view | A graph view body (nodes, edges, layout) like the IDE's own diagrams |

### Implementation limits (could be done with the current API)

- Kotlin: SpEL, pointcut and bean-name string sites are only handled in Java files; `@Bean fun x() = …` without a declared return type has no type.
- No "ambiguous candidates" or "no bean of this type" warnings: without library and auto-configuration beans they would be mostly false positives.
- "Use constructor injection" is offered only for classes without a constructor; it doesn't merge into an existing one.
- XML: only `<beans>` files; custom namespaces (`<int:channel id>`) are seen as named beans without a type; `p:`/`c:` namespace references are not followed.

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
| Dependency jars and the classpath can't be read. `read_file` takes UTF-8 project files only, and jars are binary and outside the root | `META-INF/spring-configuration-metadata.json` of dependencies is unreachable, so `spring-boot` bundles Spring Boot 3.5.x metadata, which is version-pinned. Keys of other starters (Spring Cloud, third-party) get no completion or validation. Library XSDs, `spring.factories` and `AutoConfiguration.imports` can't be read either | `project.classpath() -> list<string>` plus `read-archive-entry(archive, entry) -> result<list<u8>>` (or `read-file-bytes`), gated by `project.read` |
| No language injection | No SpEL, JPQL/HQL/SQL, AOP pointcut, cron or security-expression *language* inside Java strings and XML attributes: no highlighting, parsing, brace matching or formatting. Only completion, go-to-definition and diagnostics computed by hand inside the literal are possible | An `injections` contribution or provider: `(host language, range) -> injected language id`, so the injected range gets the injected language's grammar and providers |
| No hover / documentation provider | Property descriptions, defaults and deprecation notes, bean details, resolved queries and runtime values can't be shown on hover. Only line-marker tooltips (one line per marker) and completion documentation are available | A `hover` provider kind: `hover(doc, at) -> option<markdown>` |
| No inlay hints or editor decorations | Spring Debugger-style inline values (effective property value next to `@Value`, resolved bean next to an injection point) and query previews can't be drawn in the text. Line-marker tooltips are used instead | An `inlay-hints` provider kind: `inlay-hints(doc, range) -> list<(position, label, tooltip)>` |
| No references (Find Usages) provider | Bean usages, channel/destination usages, GraphQL field usages and Batch step references can't join Find Usages. Go to Definition is the only cross-reference. The IDE's own config-key usages are not extensible | A `references` provider kind: `references(doc, at) -> list<location>` |
| No rename / refactoring provider | Renaming a bean, property key, channel, destination, step id, flow state or command name doesn't update the string references | A `rename` provider: `prepare-rename(doc, at)` and `rename(doc, at, new-name) -> workspace edits` |
| Code actions can only edit existing files. No file creation and no other write API (see memory: plugins can't create files) | No "create missing template / view / package-info.java / schema", no generators (Spring Data repository for an entity, controller from an entity). The Modulith "open module" fix only works when the target `package-info.java` already exists | Resource operations in code-action edits (`create-file(path, text)`), or `workspace.create-file` under a write capability |
| Line markers: one target each, and icons from a fixed allowlist (`implemented`, `implementing`, `overridden`, `overriding`, `run`, `navigate`) | Several candidate beans, listeners, producers or handlers per line: the gutter opens the first one and the tooltip lists the rest (Go to Definition shows them all). No Spring bean, autowire, endpoint or channel icons | A `targets: list<location>` field on line markers (the IDE shows a chooser) and package SVG icons for markers |
| No popup, quick pick or input API, and a command can't open or focus a view | Chooser- and dialog-style actions (pick a profile, pick a bean, pick a run target, enter a name) are only possible as `Form` views the user opens themselves. Diagram and tool views can't be opened from a gutter or quick fix | `ui.show-quick-pick`, `ui.show-input`, `views.reveal(view)` |
| No structure-view / outline provider for built-in languages. The syntax provider's `outline` is only called for languages a plugin owns | Spring Modulith module structure, Web Flow states, Batch jobs/steps and Integration channels can't appear in the Structure tool window for `java` / `xml` files. Tool windows are used instead | An `outline` provider kind for any language id (merged with the built-in outline) |
| No Project view decorations | Spring Modulith open/closed module badges, bean/configuration file icons and "Spring facet" markers in the Project view are missing | A `project-view-decorations` provider: `(path) -> badge, icon, tooltip` |
| The built-in Endpoints tool window and HTTP client aren't extensible | GraphQL operations, STOMP destinations, Feign clients and messaging destinations can't be listed in Endpoints, and there is no "generate request" for GraphQL. Plugin tool windows are used instead | An `endpoints` provider contribution (kind, URL/destination, location) feeding the Endpoints view and the HTTP client |
| No diagram API | Bean dependency, Web Flow, Integration, Batch job and Modulith graphs are drawn as inline SVG in an `Html` tool-window body (`spring_text::diagram`): a fixed layered layout, no zoom/pan beyond scrolling, no export, no editing, node click → `open_location` | A graph view body (`Body::Graph { nodes, edges }`) rendered by the IDE with layout, zoom and export |
| No debugger API | Spring Debugger cannot be ported: no bean context or watches during a debug session, no evaluating bean methods or properties at a breakpoint, no transaction or DB-connection state | Debug-session events (`debug-paused`, frames), `debug.evaluate(expression)` and a variables/watches contribution |
| `net:` hosts are fixed in the manifest, and plain HTTP is allowed to `localhost` only | `spring-runtime` reads Spring Boot Actuator only from `localhost` (`net:localhost`, optional `net:127.0.0.1`). An app in a container on another host, on a remote dev machine or behind HTTPS with a custom host can't be read | A capability for user-entered hosts (`net:user`, approved per host on first use) |
| No database API | Spring Boot's datasource → database connection, JDBC driver checks and the schema behind Spring Data entities aren't available | `database.add-connection(url, user, driver)` / `database.schema(connection)` |
| No new-project or open-project API | `spring-initializr` can only open start.spring.io (`/starter.zip` downloads in the browser); the project must be unzipped and opened by hand | A new-project-wizard contribution, or `workspace.open-project(path)` plus file creation |
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
| Spring Debugger and the Spring Boot run dashboard's live data | `spring-runtime` fetches `/actuator/beans`, `/env`, `/conditions`, `/health`, `/mappings`, `/configprops` and `/scheduledtasks` over `net:localhost` (command or view Refresh only, never inside an editor provider). Results show in a tool window and as line-marker tooltips | The app needs `spring-boot-starter-actuator`, the endpoints exposed and, in Boot 3, `management.endpoint.env.show-values=ALWAYS` for unmasked values (development only). The data is a snapshot, not live debugger state |
| Spring Boot property completion and validation | Bundled Spring Boot 3.5.x metadata (Apache-2.0) plus the project's own metadata files and `@ConfigurationProperties` classes | Pinned to one Boot version. No starter metadata from jars. "Unknown key" is only reported inside groups known to that metadata |
| SpEL, security expressions, JPQL, AOP pointcuts and cron | Completion, go-to-definition and diagnostics computed in the string literal by the plugin | No highlighting or real parsing. Concatenated strings and constants are skipped |
| Bean, flow, integration, batch and module diagrams | Inline SVG in an `Html` tool-window body | See "No diagram API" above |
| Spring Initializr new-project wizard | A `Form` tool window that builds a start.spring.io URL and opens it in the browser | No project creation or import in the IDE |
| Bean navigation everywhere (injection points, XML `ref`, `@Qualifier`, SpEL `@bean`) | A per-project text index of Java/Kotlin stereotypes, `@Bean` methods and XML `<bean>`s | Library and auto-configured beans, `@Import`/`@ComponentScan` filters, profiles and conditions aren't evaluated |
| Kotlin | `spring_text::kotlin` scans Kotlin into the same declaration model as Java | Text-based only: no type inference, so `val x = Foo()` has no declared type, and DSL lambdas (`beans { }`, `coRouter { }`, `http { }`) are only partly understood |

### Not API limits: left out or not finished yet

These could be done with the current API; they are listed so they aren't mistaken for API gaps.

- Kotlin support is in `crates/spring-text`, but not every plugin indexes `**/*.kt` yet (each plugin README says which).
- Phase-2 items that were planned and not finished when the work stopped:
  - JPQL entity/property completion and checks in `@Query` (`spring-data`);
  - the XML security namespace (`spring-security`);
  - WebFlux functional routes (`spring-web`);
  - JavaScript STOMP clients (`spring-websocket`);
  - XML listener containers and `${…}` placeholder resolution from config files (`spring-messaging`);
  - GraphQL client documents (`spring-graphql`);
  - bundled Spring Cloud metadata (`spring-boot`);
  - Spring Boot CLI generation through an optional `process:spring` (`spring-initializr`);
  - diagrams in `spring-webflow`, `spring-integration`, `spring-batch` and `spring-modulith`;
  - Modulith documentation export to the clipboard.
- Deliberately skipped because they would mostly be false positives without a semantic model: unresolved-autowiring checks, "missing view" warnings, unmapped GraphQL fields above `Hint`, and messaging destinations nobody produces or consumes.
- Spring OSGi (source port): its checks that need real type resolution fall under the first gap above. `OsgiSpringBeanGenerateProvider` has no Generate-menu API.

## aritti.devcontainer (clean-room, inspired by org.jetbrains.plugins.docker.gateway)

Drives the devcontainer CLI (`@devcontainers/cli`) and Docker: Start, Rebuild, Open Shell and Stop
commands, and a `devcontainer.json` template. Checked against `l0ng-ai/openidea` `main` at
`f665beb1` (plugin API 1.1) on 2026-11-05. The original's main feature, running the IDE backend
inside the container, needs work in OpenIDEA itself: see _Upstream work_.

### Missing API

| Feature missing or partial | Why | API it would need |
|---|---|---|
| Opening the project inside the container after **Start Container** | A plugin can't open a project or a remote window, and can't run IDE actions such as `remote.connect`. The user has to open File › Remote Development themselves | `workspace.open-remote(target, path)`, or `ui.execute-action(id, arg)` |
| Knowing when `devcontainer up` has finished, and what it returned | `run_process` is capped at 5 s (60 s with an open task), but a build takes minutes, so it runs in `open_terminal`. A terminal returns no exit code and no output, so the plugin can't read `containerId` / `remoteWorkspaceFolder`, say the container is ready, or start the next step | A terminal handle with an exit event and its output, or a process API without the time cap that streams output |
| Choosing among several configurations (`.devcontainer/<folder>/devcontainer.json`) | No popup, quick pick or input API | `ui.show-quick-pick`, `ui.show-input` |
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
| Picking the configuration | A `configFile` setting; empty probes `.devcontainer/devcontainer.json`, then `.devcontainer.json`, with `read_file` | Configurations in subfolders must be typed in by hand |
| Building and starting the container | `devcontainer up` (with `--remove-existing-container` for Rebuild) in an IDE terminal | No progress, result or "ready" notification in the IDE, see above |

### Upstream work (OpenIDEA core, prepared locally, not submitted)

| Branch | What it adds | Known limits |
|---|---|---|
| `feat/tools-devcontainer-cli` (`~/Projects/openidea`) | A `Cli` tool kind and `@devcontainers/cli` (installed with npm) on the Language Servers page | One hard-coded entry: every other CLI needs its own core change until plugins can declare tools. A CLI belongs to no language, so its languages column is empty |
| `feat/remote-dev-containers` (`~/Projects/openidea-container`) | A container target next to SSH and WSL: `openidea-server` installed and started through `docker exec -i`, and a container picker in File › Remote Development | Running containers only: the dialog can't start, build or rebuild one. Default Docker connection only. Runs as the container's default user (often `root`), not the configuration's `remoteUser`. The workspace folder is guessed as `/workspaces/<folder name>`, so a `workspaceFolder` set in the configuration is ignored. Recent projects store the container's name, which changes when the container is rebuilt. No port forwarding (SSH only, as for WSL). Needs an `openidea-server` release for the container's Linux platform. Tested with unit tests and against a real container (install, start, `initialize`); not tested in the running app or on Windows |

### Not ported

- Creating a dev container from a Git repository URL (clone, then open in the container): no API clones or opens a project.
- Browsing and adding Dev Container Features or templates: no picker. The New menu template is one fixed starter configuration.
- Applying the configuration's IDE customisations (extensions, settings) inside the container: nothing in OpenIDEA maps them.

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
- **Would need:** a `create-file` code-action edit, `project.write-file(path, text)` behind a `project.write` capability, or `views.open-document(language, text)` for an unsaved scratch document.

### No wizards: no input, picker or view opened from a command

- **Blocks:**
  - new entity and new attribute wizards (type, association kind, cascade, fetch, nullability);
  - choosing which fields go into a DTO;
  - a visual derived query builder;
  - picking the SQL dialect or target package for one run.
- **Why:**
  - Same as *No popups, choosers or input dialogs* in the Symfony section.
  - A `Form` view could replace a dialog, but nothing opens a view from a command. `views` has no `show`.
- **Workaround:**
  - Completion builds derived query names.
  - The SQL dialect (`ddlDialect`) and the language for entities generated from `CREATE TABLE` (`entityLanguage`) are settings.
- **Would need:** `ui.quick-pick(items) -> option<list<string>>`, `ui.input(prompt, value) -> option<string>` and `views.show(view)`.

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

### Menus and tree views can't depend on plugin state

- **Blocks:**
  - context-menu items for *Copy DDL* and *Copy DTO* only in entity files, and *Copy Entity for CREATE TABLE* only on a `CREATE TABLE`;
  - right-click actions on an entity in JPA Structure.
- **Why:**
  - `when` clauses use the IDE's context keys (`editorLangId`, `editorPath`, `editorFocus`…; `src/platform/context.ts`), so items can be limited to Java, Kotlin or SQL files. A plugin can't set its own keys, such as "this file is an entity".
  - Tree items only send `select`, `activate`, `expand` and `collapse`. View buttons belong to the toolbar.
- **Workaround:**
  - There are no context-menu items; the commands report when the open file doesn't fit.
  - In JPA Structure, double-clicking an item opens its declaration.
- **Would need:** `ui.set-context(key, value)` for keys prefixed with the plugin id, and context actions on tree items.
- **Closes when merged:** `l0ng-ai/openidea` #1762 (`set-context`, keys `plugin.<id>.<key>` for `when` clauses) and #1746 (`view.item.context` menu on tree and list items).

### Generated sources and library entities are invisible

- **Blocks:**
  - entities and metamodel classes in generated sources (QueryDSL `Q*` types, MapStruct implementations under `target/` or `build/`);
  - entities shipped in dependency jars.
- **Why:**
  - Same as *`find_files` skips ignored files and there are no library roots* in the Symfony section.
  - `read_file` only reads UTF-8 text up to 16 MiB, so `.class` files and jars can't be read.
- **Would need:** a `find_files` option to include ignored files, and classpath symbols through a code model API.

### Possible with the current API, not done yet

- gutter icons linking an entity and its repositories (`line-markers`);
- ⌘B from a part of a derived query to the field (`definition`);
- context-menu items limited by `editorLangId == java` / `kotlin` / `sql`.

## aritti.kube-explorer (clean-room, inspired by JetBrains Kubernetes 263.7307.101)

Checked against `l0ng-ai/openidea` `main` at `1186cdce` (plugin API 1.1, no plugin API change since
`v0.1.37`). The original is proprietary, so features come from its Marketplace description.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Right-click actions on cluster resources | `view-event` has no menu event; the IDE opens a context menu only on editable `Table` bodies (insert/delete row, `PluginView.tsx`) | `tree-item.actions: list<button>` shown as a context menu, with `view-event::item-action(key, id)`. Closes when merged: `l0ng-ai/openidea` #1746 (`view.item.context` menu, declared in `plugin.toml`) |
| Path to `kubectl` (and `helm`) as a setting | A `process:<program>` grant pins the program named in the manifest, resolved on `PATH` when granted | A program path taken from a setting and approved when it is set |
| Clusters using exec credential plugins (`gke-gcloud-auth-plugin`, `aws`, `kubelogin`) from a GUI-started IDE | `process.run` has no environment parameter; the child gets the IDE's environment, often without the shell's `PATH` | `env: list<tuple<string, string>>` on `process.run` / `open-terminal` |
| Quick documentation, find usages and rename of labels, selectors and ConfigMap/Secret keys | Providers are `completion`, `inline-completion`, `code-actions`, `definition`, `line-markers` only | `hover`, `references` and `rename` provider kinds |
| Completion of ConfigMap/Secret names and keys, label selectors and Docker images from the cluster | Providers must answer within 50 ms and can't wait for `kubectl`; prefetching on `file-opened` is lost when the idle instance is reclaimed (5 min) | Asynchronous providers, or background work (`host::schedule`, a task that outlives the call) feeding a cache |
| Schemas of custom resources from the active cluster (`/openapi/v3`) | No way to register schemas with the built-in `yaml-language-server`, and no file write to point a modeline at a generated schema | Schema associations for built-in language servers (`[[contributes.jsonSchemas]]` with file patterns or content matchers, or language-server settings for built-in languages) |
| Helm templates: Go template syntax, values and named-template completion, navigation and inspections; Werf | Go templates live inside YAML; a plugin can't inject a language into a built-in one or own `yaml` (built-in ids and extensions win) | Language injection / template data languages for built-in languages |
| Browsing a chart's `.tgz` dependencies in the project tree | No project-view contribution or virtual file system | Project-view node provider, or an archive file system |
| Cluster-wide log streaming with pattern matching, and watching resources (tree updates as the cluster changes) | `process.run` blocks until exit (5 s, 60 s inside a task, 4 MiB); no streaming spawn; `on-schedule` is at least 60 s; terminal output can't be read | `process.spawn(program, args) -> handle` with output events and `kill`, plus a log/console view body with filtering |

### Implemented with a workaround

| Feature | Workaround | Limitation | API that would remove it |
|---|---|---|---|
| Edit a resource's YAML in an editor, apply on save (IntelliJ: `LightVirtualFile`) | `process:tee` writes `/tmp/kube-explorer-<kind>-<name>-<random>.yaml`, `open_location` opens it, the `file-saved` event runs `kubectl replace -f` with the context and namespace it was opened from | No file-write or virtual-document API, so a high-risk capability writes one file; Unix only; one storage entry per file, never pruned. **`open_location` refuses paths outside the project (`resolve_in_root`, `wit_host.rs`), so in the IDE this falls back to the panel editor below**; writing into the project instead would leave files in the user's tree | `views.open-document(name, language, text) -> id` with a `document-saved(id, text)` event; or `workspace.write-temp-file(name, text) -> path` that `open_location` accepts |
| Fallback YAML editor, describe, diff, Kustomize/Helm output | An `Html` body: a textarea over a highlighted copy; Apply (⌘S) posts the text back, `kubectl replace -f -` | Not a real editor (no undo history shared with the IDE, no YAML language server, no search); shows blank in the IDE for a still-unknown reason while it renders in WebKit with the IDE's frame wrapper | Same as above, plus a read-only virtual document for command output |
| Saving the edited YAML more than once | `metadata.resourceVersion` is removed before editing | Each save overwrites changes made in the cluster meanwhile. Keeping it fails every save after the first: the plugin can't refresh the open editor with the new version without making it unsaved (`apply_edits` only targets the invocation's editor) | A document the plugin owns and can reset after a save (part of `open-document`), or `TextEditor::set_saved_text` |
| Kubernetes schema completion and validation | `# yaml-language-server: $schema=<yannh/kubernetes-json-schema URL>` written at the top of the temp file, and a ⌥⏎ code action on project manifests | Changes the user's file; single-document files of built-in kinds only; the language server fetches the schema from GitHub | Schema associations for built-in language servers (see above) |
| Kubernetes icons on tree items | Each SVG declared as a `[[contributes.fileIcons]]` entry on a pattern no file has (`*.kube-explorer-pod`); the tree names it `ext:fileIcon/aritti.kube-explorer/<id>` | `tree-item.icon` only resolves the IDE's icon names; this relies on the frontend's internal naming of file icons and may break | Package SVG paths for `tree-item.icon` and `button.icon`, or `[[contributes.icons]]` |
| Context and namespace switchers, confirmation of deletes, port-forward ports | Double-click a context/namespace node; Delete needs a second click on "Confirm Delete …"; Delete Manifest Resources shows a server dry run with a Confirm Delete button; ports in a `Form` body | No quick pick, confirm or input dialog; a command can't open or focus the view, so results of Diff, Kustomize, Helm and the delete preview wait in a possibly hidden panel | `ui.show-quick-pick`, `ui.confirm(title, message) -> bool`, `ui.show-input`, `views.reveal(view)` |
| Pod logs, shell, attach, port forward | `host::open_terminal` running `kubectl` | The plugin can't read, filter or stop them (no handle); a port forward stays until the user closes its terminal | `process.spawn` handles (see above) |
| Cluster calls from the tree | `--request-timeout=4s` on reads; tasks for apply/delete | View and command calls get 5 s (60 s with a task); an unreachable cluster blocks the instance, and queued editor providers are dropped meanwhile | Asynchronous host calls, or a background worker per plugin |
| Helm commands on the chart around the open file | `Project::find_files("**/Chart.yaml")`, innermost directory containing the file | `find_files` skips hidden and gitignored directories, so such charts aren't found | `find_files` option to include ignored files, or `Project::exists(path)` / a stat call |

## aritti.tasks (port of com.intellij.tasks)

Port of `com.intellij.tasks` 263.7307.108 (intellij-community `idea/263.7307.112`). Checked against the
SDK, WIT and host source of v0.1.37 and `l0ng-ai/openidea` `main` at `f665beb` (no plugin API change
since v0.1.37) on 2026-11-05.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Switch Task (⌥⇧T) and Go to Task (⌥⇧N) popups, searching tasks and tracker issues | No popup, quick pick or input API (`wit/host.wit`, `ui`: notifications, status items, clipboard, `open-url` only), and a command can't open or focus a view | `ui.show-quick-pick(items, placeholder) -> option<string>` and `ui.show-input(prompt, value) -> option<string>`; or `views.reveal(view)` so a command can open the Tasks view |
| Self-hosted trackers: Jira Server/Data Center, YouTrack Standalone, self-managed GitLab, Forgejo/Gitea, Redmine, Mantis, Bugzilla, Trac, FogBugz, the generic tracker | `net:<host>` is fixed in the manifest and `net:*` is rejected (`crates/ide-extension/src/capability.rs`, `FromStr`: only exact hosts or `*.domain`), so a server URL from settings can't be reached | A capability for hosts the user enters, e.g. `net:user` approved per host at first use, or hosts taken from a `string` setting and approved when it is set |
| Clearing the context on task switch (closing the editors of the task being left) | No API to list or close open editors, and no `file-closed` event (`EVENT_KINDS` in `crates/ide-extension/src/manifest.rs`: file-opened, file-saved, editor-focused, action-performed, vcs-changed) | `workspace.open-editors() -> list<string>`, `workspace.close-editor(path)` and a `file-closed` event. Closes when merged: `workspace.open-editors` only, in `l0ng-ai/openidea` #1754 (needs #1752 first); `close-editor` and `file-closed` stay open |
| Bookmarks and breakpoints in the task context (`WorkingContextManager`) | No bookmark or breakpoint API | `bookmarks.list/set`, `debug.breakpoints/set-breakpoints` |
| Changelists per task (`TaskChangelistSupport`, `createChangelist`) | Plugins see git only (`wit/workspace.wit`, `vcs`: `status`, `remotes`, `fetch`); there are no changelists | A changelist API, if the IDE gets changelists |
| Commit message in the commit dialog (`TaskCheckinHandlerFactory`, `CommitPlaceholderProvider`) and "commit changes" on close | No commit-message or commit-dialog extension point; `vcs` has no commit call | A `commit-message` provider export, or `vcs.set-commit-message(text)` and `vcs.commit` |
| Branch, merge and stash through the IDE's VCS layer (`VcsTaskHandler`, smart checkout, conflict dialogs; Mercurial too) | `vcs` can't change the repository except `fetch` | `vcs.checkout`, `vcs.create-branch`, `vcs.merge`, `vcs.stash` / `unstash` under `vcs.write` |
| JQL and YouTrack query languages (highlighting, completion, annotator) in the query fields | Settings and form fields are plain text; languages and completion only apply to editor documents | A form or settings field bound to a language id (`FieldValue::Code { language, text }`) |
| Analyze stack trace from a task description (`AnalyzeTaskStacktraceAction`) | No API to open the Analyze Stack Trace tool or a console with file links | `ui.analyze-stacktrace(text)`, or a console view body with hyperlinked positions |
| Trello: move a card to another list on close (`setTaskState`) | Needs the user to pick one of the board's lists, and there is no picker | The quick pick above |
| Task description and comments in a documentation popup, issue links in commit messages (`ShowTaskDescription`, `TaskIssueLinkDocumentationTargetProvider`) | No hover or documentation provider for the VCS log or commit messages | A hover/documentation provider for commit messages and the VCS log |

### Implemented with a workaround

| Feature | Workaround | Limitation | API that would remove it |
|---|---|---|---|
| Open Task, Close Task and Edit Task dialogs | The Tasks view replaces its tree with a `Form` | Close Active Task (⌥⇧W) opens the Close form in the Tasks view and says so in a notification, but can't bring the view to the front: the host has `refresh_view` and `post_view_message` and no call that shows or focuses a tool window | `ui.show-form(fields) -> option<values>`, or `views.reveal(view)` |
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

### Gutter markers can't run code, and there is no input box

- **Blocks:**
  - starting a thread from the editor gutter, as the original does on the merge request's branch;
  - the title and description fields of Create Merge Request, Share Project and Create Snippet.
- **Why:**
  - A `line-marker` has a tooltip and one `target` location; a click can't run plugin code or pass the line.
  - There is no input, popup or dialog API, and a command doesn't receive the gutter line.
- **Workaround in the port:**
  - Comments are made in the Merge Request Changes `Html` view.
  - Create Merge Request opens GitLab's new-MR page.
  - Snippets are private and use the file name.
  - Share uses the folder name, private, in your own namespace.
- **Would need:** `line-marker.command` (run with the line as argument) and `ui.input(prompt, multiline, initial) -> option<string>`.

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

### Commands and notifications can't open a tool window

- **Blocks:**
  - clicking the current branch's `!7 on feature` status item to show the merge request;
  - the original's "Create / View merge request" actions after a push.
- **Why:**
  - There is no `views.show`.
  - `ui.show-notification` has no actions.
  - There is no push event; `vcs-changed` gives only the repository root.
- **Workaround in the port:**
  - The status item selects the merge request, and the command result tells the user which tool windows to open.
  - A pushed branch with no merge request shows *GitLab: Create MR* in the status bar.
- **Would need:** `views.show(view)`, notification actions (`list<tuple<label, command>>`), and a `vcs-pushed` event (repository, remote, branch).

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

### Plugins can't open a project, pick a folder or see selected files

- **Blocks:**
  - opening a cloned repository or a merge request worktree;
  - choosing the clone directory;
  - Create Snippet from files selected in the Project view.
- **Why:**
  - There is no `open-project` and no directory picker.
  - The invocation carries only the window's project and focused editor, so commands in `projectView.context` get no selection.
- **Workaround in the port:**
  - The clone target is the `cloneDirectory` setting, by default next to the current project, and the user opens the clone with File › Open.
  - Snippets come from the editor only.
  - Worktrees aren't ported.
- **Would need:** `workspace.open-project(path, new-window)`, `ui.pick-directory(title, initial)` and `invocation.selected-paths()`.
- **Closes when merged:** only `invocation.selected-paths()`, in `l0ng-ai/openidea` #1744. `open-project` and `pick-directory` stay open.

### View toolbars hold plain buttons only

- **Blocks:** filtering merge requests by author, label or search text.
- **Why:** toolbars have no text field, choice or toggle. A `Tree` has speed search over visible labels only.
- **Workaround in the port:** the original's five quick filters, cycled with one button.
- **Would need:** toolbar controls (search field, choice, toggle) that report `view-event::field`.

### Not ported, but possible with the current API

- Applying suggestions. The Marketplace text mentions it, but 263.7307.200 has no code for it; it could be a `code-actions` quick fix.
- Markdown rendering of notes. Left out to keep the wasm small; a host `markdown-to-html` would make it free for every plugin.
- These shortcuts are marked `ponytail:` in the source:
  - only the first page of results: 50 merge requests, 100 threads, 100 member projects;
  - fork checkout through `refs/merge-requests/<iid>/head` instead of a fork remote;
  - a file counts as viewed if it was marked at the current head commit (the original compares with the last commit that touched it).
- Usage statistics aren't ported, on purpose.

## Ideolog (`aritti.ideolog`)

Source port of `com.intellij.ideolog` 263.7307.101 (MIT, tag
`pycharm/263.7307.101`, `41beb53`): log highlighting, folding, an outline of
errors and warnings, custom Ideolog formats, Jump to Source, error and entry
navigation, links on stack frames. Checked against the WIT, SDK and host of
v0.1.37 and `main` (`f665beb`) on 2026-11-05.

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
  - extend selection (event → column → line, `ExtendsSelection`);
  - Go to Next Error and Previous / Next Entry in logs outside the project.
- **Why:** `text-editor` has no `set-selections` (see the JPA Assist section). `open_location` moves the caret but `resolve_in_root` refuses files outside the window's project, even the file of the invoking editor.
- **Workaround:** the navigation commands call `host::open_location` on the current file, so they work only for logs inside the project.
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
- **Workaround:** one multiline string setting that takes Ideolog's exported XML (`<LogParsingPattern/>`, `<LogHighlightingPattern/>`).
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

### No confirmation dialog

- **Blocks:** a "Drop all objects?" prompt before `clean`.
- **Why:** same as *No popups, choosers or input dialogs* in the Symfony section.
- **Workaround:** `clean` must be run twice within 30 seconds.

### Problems only on open and save

- **Why:** same as *Problems aren't re-checked while typing* in the Symfony section.
- **Workaround:** for a rename done outside the IDE, run Flyway: Check Migrations.

## Liquibase Changelogs (aritti.liquibase, clean-room)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Run configurations can't confirm or ask

- **Blocks:** `rollback-count` (it needs a count), per-changelog runs, and any confirmation before `update`.
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

### Shared with other plugins

- Diagnostics come only from the `line-markers` provider, and there is no database API: see the Spring plugins rows "Diagnostics have no provider of their own" and "No database API".

## aritti.openapi (clean-room, inspired by com.intellij.swagger)

Checked against the SDK, WIT and host of v0.1.37 on 2026-11-05.

### Not implemented

| Feature of the original | Why | API that would unlock it |
|---|---|---|
| Preview opened automatically for any OpenAPI file | `customEditors.patterns` match file names only; an OpenAPI file is a generic `.yaml`/`.json`. Other names need Open With | A content matcher on custom editors (first-line regex or top-level key), as languages have `first-line` |
| Schema validation for JSON specs, and without editing the file | No way to register a schema with the built-in `yaml-language-server` / `vscode-json-language-server`, so the plugin inserts a modeline into the YAML file (same gap as kube-explorer) | Schema associations for built-in language servers |
| Rename of a component everywhere, find usages of a schema, hover with the target's definition | No `rename`, `references` or `hover` provider kinds | Those provider kinds |
| Structure view of a spec | `syntax-provider` outline only runs for plugin-owned languages; `yaml` is built in | An outline provider for any language |
| Problems while typing | No `document-changed` event or diagnostics provider (see Symfony section) | Same |
| Multi-file specs in the preview | Web views have no network and can't read project files | A read-only project file API for web views (or posting files from code, possible today, not done) |

### Implementation limits

- Editing features cover YAML only (no JSON scanner).
- Flow-style mappings are only seen for `$ref`.
- No file-name completion in `$ref`: `find_files` is too slow for the 50 ms budget.
- The endpoints view re-scans the project on each render.
- The dark theme is a colour inversion.
- A missing `$ref` file is told apart from an unreadable one by treating `NotFound` and `Invalid` as missing: see "Missing files are reported as `Invalid`" in the Liquibase section.

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

- Go modules and other lockfiles.
- CVSS scoring (severity comes from the advisory's labels).

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

- **Blocks:** keys of Quarkus extensions beyond the bundled ones (42 extensions) and of dependency jars.
- **Why:** same as the row *Dependency jars and the classpath can't be read* under *Gaps that affect every Spring plugin* in the Spring plugins section. The bundled catalog covers a curated set, and the generator takes extra artifact ids.

### No project wizard

- **Blocks:** creating a Quarkus project in the IDE.
- **Why:** same as *Code actions can't create, rename or delete files* in the Symfony section (a plugin can't create files). The command opens code.quarkus.io in the browser instead.

## aritti.properties (port of com.intellij.properties)

Key navigation and the missing-key warning are text scans, not type resolution. Source port (Apache-2.0); the HTML/JSP sites and the missing-key warning are clean-room from the Spring and Thymeleaf documentation.

| Not implemented | Why | Workaround |
|---|---|---|
| Go to Definition and warnings in FreeMarker (`.ftl`) | the host has no language id for `.ftl`, so a provider can't attach to it | none; use Search Everywhere on the key |
| Warnings for `#{key}` in `.xhtml` and JSP | there `#{...}` is Java EL (JSF beans), not a message; navigation still works | none needed |
| Warnings with several candidate bundles (more than one of `messages`, `ValidationMessages`, `i18n/*`, or the same name in two folders) | without types the plugin can't tell which bundle a call reads | turn on one bundle layout, or ignore: the check stays quiet |
| Warnings for `bundle.getString("k")` on a variable, or `message("k")` | the receiver's bundle is unknown | use `ResourceBundle.getBundle("name").getString("k")` inline |
| Rename of a key across bundles and code, references (Find Usages) | no rename or references API for plugins | Find Unused Properties, and Remove from every locale |
| Warnings refreshed when a `.properties` file changes | diagnostics are published on open and save of the code file | reopen or save the file |

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
- **Workaround in the plugins:** `host::notify` instead of returning text.
- **Would need:** the frontend showing a returned string as an info notification titled with the command, or the WIT documenting that the result is unused.

### Form fields can't be read-only

- **Blocks:** showing data (a merge request's title, author, description) in a `Form` without it looking editable.
- **Why:** `Text` and `Multiline` fields are always inputs; edits the plugin ignores are silently lost.
- **Would need:** a `readonly` flag on `field`, or a `Label`/`Info` field value.
