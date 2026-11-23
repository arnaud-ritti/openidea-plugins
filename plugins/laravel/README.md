# Laravel

Laravel support for OpenIDEA in PHP and Blade files. It completes and links routes, views, config, translations, components and Eloquent columns, reports mistakes, and adds a **Laravel** tool window to browse your app and run Artisan. Features follow [Laravel's documentation](https://laravel.com/docs). MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or newer (`engines.openidea = "^1.14"`).
- A Laravel project: it has an `artisan` file or requires `laravel/framework`.
- Recommended: the [Blade](../blade/README.md) plugin (`aritti.blade`) for Blade highlighting, folding and the structure view. The IDE has no Blade grammar. Without it, Blade files open as PHP and everything below still works in them.
- PHP on the `PATH` the IDE sees, for anything that runs Artisan: commands, generators, the Artisan, Runtime and Database tabs, **Send Request**, route and metadata refreshes. Artisan only runs in trusted projects.
- Optional: Docker, to run Artisan in a container. Composer or the Laravel installer, for **Laravel: New Laravel Project…**.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a Laravel project. The plugin indexes it in the background.
3. In a PHP file, type `route('` or `view('` and pick a name. ⌘B on the name opens its definition.
4. Run **Laravel: Show Laravel Tool Window** to browse routes, views, config and more.
5. To run Artisan, allow `process:php` in Settings › Plugins › *Laravel*, then run **Laravel: Run Artisan Command…**.

## Features

### Complete and open Laravel names (⌘B)

Type inside one of these calls to get completion. ⌘B opens the definition.

| Names | Indexed from | Referenced in |
| --- | --- | --- |
| Routes | `->name()`, `['as' => …]`, `Route::resource` (`only`, `except`, `names`) in `routes/`, with the name prefixes of the groups around them | `route()`, `to_route()`, `redirect()->route()`, `Redirect::route()`, `URL::signedRoute()`, `Route::has()`, `routeIs()` |
| Views | `resources/views/**` (`.blade.php`, `.php`), `vendor/<package>/` overrides as `package::name`, extra directories from the settings and from `ide-blade.json` | `view()`, `View::make/exists/first`, `Route::view`, `->view()`, `->markdown()`, `new Content(view:)`; Blade `@extends`, `@include*`, `@each`, `@component` |
| Config | `return [...]` of `config/**/*.php`, nested keys joined with `.` | `config()`, `Config::get/has/set/string/…` |
| Translations | `lang/<locale>/<file>.php` (`file.key`), `lang/<locale>.json`, `lang/vendor/<package>/…`, main locale first | `__()`, `trans()`, `trans_choice()`, `Lang::get/has/choice`, Blade `@lang` |
| Env keys | `.env.example`, `.env` (names only, values are never read) | `env()`, `Env::get()` |
| Middleware | `$middleware->alias([...])` in `bootstrap/app.php`, `$routeMiddleware` / `$middlewareAliases` in `app/Http/Kernel.php`, the framework's own | `->middleware()`, `->withoutMiddleware()`, `'middleware' => […]`; the part after `:` (`throttle:60,1`) is ignored |
| Controllers | `app/Http/Controllers/**`, public instance methods | `Route::get('/x', 'UserController@index')`, `['uses' => …]`, `[UserController::class, '…']`, `Route::resource('x', '…')`, `action()` |
| Assets | files under `public/` | `asset()`, `secure_asset()`, `URL::asset()` |
| Blade components | `resources/views/components/**` (`card/index` is `card`), `app/View/Components/**` as kebab-case (`Forms/InputField` is `forms.input-field`), classes win | `<x-forms.input-field>` and `</x-…>` tags |
| Livewire components | `app/Livewire/**`, `app/Http/Livewire/**` as kebab-case | `<livewire:counter>`, `@livewire('counter')` |
| Vite entries | `resources/**` files with a `css`, `scss`, `sass`, `less`, `js`, `jsx`, `ts`, `tsx` or `vue` extension | Blade `@vite(...)`, `Vite::asset()` |
| Abilities | `Gate::define('name')` in `app/Providers/**`, public methods of `app/Policies/**` (except `before`, `after`) | `->can()`, `->cannot()`, `->authorize()`, `Gate::allows/denies/check/authorize/inspect/any/none`; Blade `@can`, `@cannot`, `@canany`, `@elsecan*` |
| Validation rules | Laravel's rule names (fixed list; custom rule objects aren't strings) | strings and lists in `->validate()`, `->validateWithBag()`, `Validator::make()` and a form request's `rules()`; completes the rule under the cursor, not after its `:` |
| Columns | `$table->string('name')` and the other Blueprint column methods in `Schema::create` / `Schema::table` of `database/migrations` (`id`, `timestamps`, `softDeletes`, `morphs`, `foreignIdFor`…) | `Post::where('…')`, `orderBy`, `select`, `pluck`, `whereIn`… on a static call or a model-starting chain (`Post::query()->with('x')->where('…')`), `DB::table('posts')->…`, and `$query->where('…')` or `$fillable` / `$guarded` / `$hidden` / `$visible` / `$casts` / `$dates` in the model file. The table is the model's `$table`, else the snake-case plural of its class name |
| Sections, stacks | the layouts a Blade view extends (its `@extends` chain) | Blade `@section`, `@yield`, `@hasSection`, `@push`, `@prepend`, `@stack` |

More completion:

- **Blade directives.** Typing `@` (or `@fo`) completes Laravel's and Livewire's directives, including the `@end…` closers. Custom directives registered with `Blade::directive('name'` in `app/**` are included. Common ones insert a snippet with arguments and closer.
- **Component attributes.** Inside `<x-name ` or `<livewire:name `: the `@props([...])` of an anonymous component, class constructor parameters, or a Livewire class's public properties and `mount()` parameters. `<x-slot:` completes the variables the enclosing component's view prints.
- **Container names.** `app('…')`, `resolve('…')`, `App::make/get/bound('…')` and `@inject('x', '…')` complete and open framework aliases, your `bind/singleton/scoped/instance/alias('x'` calls in `app/`, and your `app/` classes. In `config/app.php` (`'providers'`) and `bootstrap/providers.php`, `app/Providers` classes complete as `Name::class`.
- **Columns through relations and scopes.** `Post::find(1)->comments()->where('` and `Post::active()->comments()->orderBy('` complete `Comment`'s columns. So does `$query->where('` after `$query = Post::query()...;` earlier in the file.
- **Eloquent members.** `Purchase::active()->first()->customer->` suggests the related model's columns. `Purchase::get()->` suggests collection methods. Enum casts, traits, custom casts and typed methods are followed too.
- **JavaScript.** In JavaScript, TypeScript, Vue, JSX and TSX files, Ziggy route names complete and open in `route('...')` and `route().current('...')`, including `:href="route('...')"` in Vue templates.
- **Also:** Pest dataset and group names, Socialite drivers, `pint.json` and `phpstan.neon`, and 26 PHP snippets (routes, relations, fakes, validation) that add their imports.

### Hover and gutter markers

- Hover a route, view, config or translation name to see where it is defined, with a link to the Laravel docs.
- Gutter markers link controllers, routes, views, layouts, sections, stacks, models, migrations, listeners and events. Each relationship has its own icon.
- A controller method's Laravel icon lists the routes that point to it (`Route::get(...)`, `Route::resource()` actions). Click it to pick one from the list under the icon, or to go straight to the route when there is only one. Copy URL, Copy as cURL and Create HTTP Request stay as code lenses above the method.
- Route and controller hints show in PHP files (setting `routeHints`).
- The status bar shows the Laravel and PHP versions. Click it for a menu: Run Artisan Command…, Clear All Caches, Search Routes…, Send Request…, Show Laravel Tool Window and Reindex Project.

### Search Everywhere and Run consoles

- **Laravel Routes** tab in Search Everywhere (⇧⇧): type part of a URI, route name, method or controller. Routes appear once indexing finishes; registered routes from **Laravel: Refresh Registered Routes** are used when loaded.
- In Run and Debug consoles, while a Laravel project is open: log lines at error level and above in red, warnings in yellow, PHP fatal errors and warnings, Artisan `ERROR`/`WARN` and Pest `FAIL`/`PASS` labels, migration `DONE`/`FAIL`, exception headers, your own `app/`, `routes/` and `tests/` frames underlined and vendor frames dimmed.

### Find mistakes

Problems show as you type or on save. Each group has its own setting (see [Settings](#settings)).

- **Missing names:** a view that doesn't exist, a config key missing from a config file the project defines, a translation key missing from a lang file. Route names only when **Also report route names…** is on, because packages add routes the plugin can't see.
- **Unknown components and pages:** `<x-name>` with no file or class; `<livewire:name>` or `@livewire('name')` with no class or view; `Inertia::render('Page')`, `inertia('Page')` and `Route::inertia()` naming a missing page in `resources/js/Pages` or `resources/js/pages`; `Model::factory()` without `database/factories/<Model>Factory.php`.
- **Routes:** a controller in a route that has no file, a controller method that doesn't exist, and missing methods of a `Route::resource` / `apiResource` controller (`only` and `except` are honoured).
- **Livewire:** `wire:click`, `wire:submit`, `wire:keydown`… naming a method the component class doesn't have.
- **Eloquent:** mass assignment of a model with `$guarded = []` and no `$fillable`; a `create([...])` key that `$fillable` leaves out; `Prunable` models never scheduled with `model:prune`; `orderBy('created_at', 'desc')` with a quick fix to `latest()` / `oldest()`.
- **Enum values:** `where()` values the column's enum cast lacks, with a quick fix to the closest case. The values also complete.
- **`env()` outside config:** a warning on `env()` calls outside `config/`, `bootstrap/`, `vendor/` and `tests/`. `env()` returns null once configuration is cached.
- **Security:** unfiltered request data passed to `create()`, `update()` and `fill()` (quick fix to `$request->validated()` with a Form Request), variables in raw SQL, `$guarded = []`, `Model::unguard()` and URIs excluded from CSRF verification.
- **Upgrade:** code removed in a Laravel major the project hasn't reached yet, with a rename fix where one exists.
- **Translations:** keys some locales have and others lack.

Keys built at run time or with a wildcard aren't checked.

### Quick fixes and refactorings (⌥⏎)

- Add a missing `file.key` translation key (`'key' => 'key'`) to the main locale's lang file.
- Convert validation strings and arrays: `'required|email'` ⇄ `['required', 'email']`, one rule or the whole array.
- Convert `'exists:users,id'`, `'unique:users,email'`, `'in:a,b'` and `'not_in:a,b'` to `Rule::…` with the import.
- Convert `'UserController@index'` to `[UserController::class, 'index']`, one or file-wide.
- Toggle `{{ $x }}` ⇄ `{!! $x !!}` in Blade.
- Convert `class="btn btn-primary"` to `@class(['btn', 'btn-primary'])`.
- Add the `use` line for `Route::`, `Auth::`, `Str::` and other core facades.
- Convert `scopeActive()` to `#[Scope] protected function active()`, or **Use #[Scope] on all scope methods**.
- **Move $x to a constructor property.**
- **Inject … instead of the facade.**
- **Extract to translation key** on a selected string.
- **Add validation rules for the columns of Post** in a form request's `rules()`.
- **Add the missing columns to $fillable** in a model.
- **Remove IDE helper PHPDoc from model**. **Laravel: Remove IDE Helper PHPDoc from Models** does it for many models at once.
- **Generate a feature test for {route}…** creates a Pest or PHPUnit test for the route a controller method serves.

### Create files from code

All of these show a preview first and never overwrite a file. Undo reverts them.

- **Create missing literal Blade view**: on `view('admin.users')`, `@include`, `@includeIf` or `@extends`, creates the view under `resources/views` and opens it.
- **Extract reviewed HTML/simple-variable Blade view…**: turns a selected HTML element into a new view and an `@include`. Selected `{{ $name }}` reads become `['name' => $name]`.
- **Create reviewed mapped FormRequest …**: on a missing `NameRequest` parameter type, creates the class where Composer's PSR-4 mapping puts it. `authorize()` returns `false` and `rules()` is empty.
- **Move validation rules to a Form Request…**: moves a controller's `$request->validate([...])` rules into a new Form Request and replaces the call with `validated()`. `authorize()` returns `true`.
- **Laravel: Create Missing View, Component or Controller** (editor context menu): creates the view, component, controller or class named under the caret. For a view it asks once, naming the file, then writes it (needs `project.write`). That includes a Job, Mail, Event, Notification, Listener, Policy, Request or Resource named by `new X` or `X::class`.

### Rename and find matches

- Rename works on route, config, translation and Blade-alias literals you can prove. Renaming a view or anonymous component file moves it and updates its references in one step.
- **Laravel: Find Source Matches…** lists literal spellings that match across routes, views, config, lang and `src`.
- **Laravel: Find Unused Views** lists views no code names. Components, `errors.*`, `livewire.*`, `vendor.*`, `mail.*` / `emails.*` and package views are never listed. Views are marked *maybe used* when the project builds view names at run time.

### The Laravel tool window

Open it with **Laravel: Show Laravel Tool Window** or from the bottom bar. Tabs are grouped into four sections: **Browse**, **Data**, **Run** and **Build**. Switch section with the **View group** picker; each section remembers its last tab. Each tab keeps its own search and selection. Double-click a row to open its definition. Resource tabs have a Reindex button; the Routes tab's refresh captures registered routes instead.

| Tab | What you do there |
|---|---|
| About | Application details and captured Artisan information. |
| Routes (default) | Browse routes with method, path and controller. Filter by controller, switch Flat/Tree. Row actions: **Copy as curl**, **Create HTTP Request File**, **Send Request**. |
| Request | Run a route inside the app and see the response (see below). |
| Models | Browse models with their fields, relations and related classes. Generate helper PHPDoc, add fields, draft a model from a database table. |
| Database | List migration tables with models, columns, indexes and foreign keys. **Compare with database**, **Rows**. |
| Schema | A diagram of every table and its keys, joined by foreign keys (solid) and model relations (dashed). Pan, zoom, Fit, filter, click to open the migration. |
| Views | View / File / Folder / Namespace table with filters and sortable columns. |
| Components | Blade class, anonymous and alias components, and Livewire classes, with a type filter. |
| Middleware, Config, Env, Translations | Searchable lists. Config shows Key, Value and File. Translations shows Key, Locale and Value with a locale filter. |
| Generators | Plan models with fields, relations and indexes, then generate them and their factories, seeders and more. **Finish** shows every file and writes them once you confirm (needs `project.write`). |
| Artisan | Search commands, fill in arguments and options, run, and read the output. |
| Runtime | Capture Application, Events, Schedule, Failed Jobs, Queue Status, Workers and Logs on demand. |
| Processes | Start the development server, queue worker and schedule worker in a terminal tab. Stop or restart them from the tab. |

Sections: **Browse** has About, Routes, Views, Components, Middleware, Config, Env and Translations. **Data** has Models and Database. **Run** has Request, Artisan, Runtime and Processes. **Build** has Generators.

A tab that fails to load shows **Retry**. A tab that needs a permission names it.

### Run Artisan

- **Laravel: Run Artisan Command…** opens the Artisan tab. Pick a command, fill in its form, and check the exact command line before you run it.
- Use **Run in Terminal** for interactive commands and long-running workers. **Laravel: Run Artisan in Terminal…** opens the older terminal picker.
- **Cancel** stops the running command. A successful run reindexes the project.
- The command list comes from your app. Run **Laravel: Refresh Artisan Command Metadata** once, and again after installing packages. You review the exact command before anything runs.
- One-click commands clear caches, migrate, seed and link storage. See [Commands](#commands). **Fresh Migration** and **Seed Database** ask first, naming the project and, when `.env` sets them, the database connection and environment.

### Generate classes

- **Laravel: Generate with Artisan make…** offers 30 `make:*` generators with their flags (`--resource`, `--api`, `--requests`, `--inline`), plus your package and custom `make:*` commands.
- **Laravel: New Eloquent Model…** creates a model with any of migration, factory, seeder, controller and policy (`-m -f -s -c --policy --all`).
- **Laravel: Generate for Model…** creates a policy, observer, factory, resource or controller for a model. Right-click a model file in the Project view to skip the model pick. With `project.write`, factories and resources are filled from the model's migration columns.
- Nova, Filament and Livewire form generators appear when `composer.json` requires `laravel/nova`, `filament/filament` or `livewire/livewire`.
- **New** › file templates add 47 Laravel, Livewire, Inertia, Pest, PHPUnit and Blade files (see Technical details for the list).

### Send a request to a route

1. In the Routes tab, choose **Send Request** on a row.
2. Fill in URI parameters, query, JSON body (prefilled from Form Request rules or inline validation), **Act as** user id and **Accept JSON**.
3. Leave **Roll back database changes** on (the default) to undo writes. With it off, a write needs a second click.
4. Read the status, time, query count, size, redirect, headers (cookies masked) and body. **Copy as curl** copies the same request.

No server is needed: the route runs through the app's HTTP kernel.

### Inspect the database and runtime

- **Compare with database** boots the app. It lists pending migrations, tables only in the migrations or only in the database, and missing columns and indexes.
- **Rows** shows a table read-only and paged. Secret and personal data columns are masked.
- Runtime captures run only when you click. **Laravel: Retry Failed Job…** retries one failed job after you confirm. **Laravel: Inspect Queue…** accepts a connection or queue.
- **Laravel: Add Application Database Connection…** registers the app's database in the IDE. Connect, enter credentials or remove it with **Laravel: Database Connections…**.

### Run Artisan in Docker

1. Run **Laravel: Choose Docker Target…** and pick a running container (Compose, Sail and DDEV are detected). The project must be bind-mounted into it.
2. Allow `process:docker`.
3. Run **Laravel: Refresh Docker Artisan Commands**.
4. In the Artisan tab, switch the PHP runner from Local PHP to Docker.

You can also set the socket, container ID and working directory by hand in Settings. **Laravel: Clear Chosen Docker Target** goes back to those settings.

### Run configurations

**Laravel: Serve**, **Laravel: Test**, **Laravel: Queue Worker** and **Laravel: Schedule Worker** are offered for any project with an `artisan` file.

### AI assistant tools (MCP)

Seven tools are declared to OpenIDEA's built-in MCP server, prefixed `aritti_laravel__`: `inventory`, `artisan_commands`, `runtime_snapshot`, `runtime_refresh`, `artisan_execute`, `artisan_job` and `artisan_cancel`. Tools that run code ask for confirmation and respect Restricted Mode. An agent gets no permissions beyond the plugin's.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Laravel**; change keys in Settings › Keymap. Default shortcuts: **Search Routes…** ⌥⇧Q, **Search Names…** ⌥⇧E (Alt+Shift on Windows and Linux); a shortcut your keymap already uses is skipped. Commands show only in projects with an `artisan` file, except **Show Laravel Tool Window**, **New Laravel Project…** and **Recover Project Installation…**.

**Project and search**

| Command | What it does |
|---|---|
| **Laravel: Show Laravel Tool Window** | Opens the Laravel tool window. |
| **Laravel: Reindex Project** | Rebuilds the project index. |
| **Laravel: Show Index Issues** | Lists indexing problems in the *Laravel Index Issues* output. |
| **Laravel: Search Names…** | Searches route names, views, components, config, translations and env keys in one list. |
| **Laravel: Search Routes…** | Fuzzy search on route names, URIs, methods and controllers. |
| **Laravel: Show Route List** | Runs `php artisan route:list`. |
| **Laravel: Refresh Registered Routes** | Captures the real route list from Artisan, including package routes. Runs the project's PHP. |
| **Laravel: Find Source Matches…** | Lists literal spellings that match across project sources. Read-only. |
| **Laravel: Find Unused Views** | Lists views no code names. |
| **Laravel: New Laravel Project…** | Creates a new Laravel project in a terminal. |
| **Laravel: Recover Project Installation…** | Reopens or forgets a new-project installation that didn't finish. |

**Artisan**

| Command | What it does |
|---|---|
| **Laravel: Run Artisan Command…** | Opens the Artisan tab. |
| **Laravel: Run Artisan in Terminal…** | Picks a command and runs it in a terminal. |
| **Laravel: Refresh Artisan Command Metadata** | Reads the app's command list after review. Runs the project's PHP. Also in the Tools menu. |
| **Laravel: Run Command From Class…** | Opens the Artisan form for a console command class (from its Run marker, or the class open in the editor). Otherwise opens the Artisan tab. |
| **Laravel: Clear All Caches** | Runs `optimize:clear`. |
| **Laravel: Clear Application Cache** | Runs `cache:clear`. |
| **Laravel: Clear Config Cache** | Runs `config:clear`. |
| **Laravel: Clear Route Cache** | Runs `route:clear`. |
| **Laravel: Clear Compiled Views** | Runs `view:clear`. |
| **Laravel: Run Migrations** | Runs `migrate`. |
| **Laravel: Fresh Migration (drops all tables)** | Runs `migrate:fresh` after you confirm. It names the project, and the database connection and environment from `.env`. |
| **Laravel: Seed Database** | Runs `db:seed` after you confirm. It names the project, and the database connection and environment from `.env`. |
| **Laravel: Link Storage** | Runs `storage:link`. |

**Docker**

| Command | What it does |
|---|---|
| **Laravel: Choose Docker Target…** | Picks a running container for Artisan. |
| **Laravel: Clear Chosen Docker Target** | Clears the chosen container and uses the Settings values. |
| **Laravel: Refresh Docker Artisan Commands** | Reads the command list inside the container after you confirm. Runs PHP in the container. |
| **Laravel: Show Docker Artisan Commands** | Shows the saved container command list. Never runs Docker. |

**Generators and models**

| Command | What it does |
|---|---|
| **Laravel: Generate with Artisan make…** | Runs a `make:*` generator with its options. |
| **Laravel: New Eloquent Model…** | Creates a model and its related classes. |
| **Laravel: Generate for Model…** | Creates a policy, observer, factory, resource or controller for a model. Also in the Project view context menu. |
| **Laravel: Create Missing View, Component or Controller** | Creates what the name under the caret refers to. Also in the editor context menu. |
| **Laravel: New Model from Database Table…** | Drafts a model from a table read by **Refresh Database Metadata**. |
| **Laravel: Create Model Fields…** | Adds fields to an existing model, with a migration. |
| **Laravel: Open Model Source** | Opens a model. From the palette it asks which one. |
| **Laravel: Remove IDE Helper PHPDoc from Models** | Removes `@property`/`@method`/`@mixin` lines from every model after one confirmation that lists the files. Needs `project.write`. Also in the Project view context menu. |
| **Laravel: Configure Model Helper Parameters…** | Saves helper storage, count and date preferences for the project. |
| **Laravel: Regenerate Model Helpers…** | Regenerates helper PHPDoc for the models you pick, shows each file, then writes them. Needs `project.write`. |
| **Laravel: Configure Helper Source and Automation…** | Sets advanced helper options and commands to run after generation. |
| **Laravel: Show Model Helper Command Status** | Shows the commands that run after helper generation and their results. |
| **Laravel: Run Pending Model Helper Commands** | Runs the commands still waiting after helper generation. |
| **Laravel: Stop Model Helper Commands** | Stops the commands still waiting after helper generation. |

**Routes and requests**

| Command | What it does |
|---|---|
| **Laravel: Copy as cURL** | Copies the selected route as a curl command. |
| **Laravel: Create HTTP Request File** | Writes `http/<verb>-<path>.http` for the selected route. |
| **Laravel: Send Request** | Opens the Request tab for the selected route. Sending runs the project's PHP. |
| **Laravel: Open Route Declaration** | Opens the route behind a marker in the editor. From the palette, opens **Search Routes…**. |
| **Laravel: Route Request Actions…** | From a route in the editor: go to the declaration, copy the URL or curl, or create an HTTP request. From the palette, opens **Search Routes…**. |

Run from Search Everywhere, **Copy as cURL**, **Create HTTP Request File** and **Send Request** first ask which route.

**Database and runtime**

| Command | What it does |
|---|---|
| **Laravel: Refresh Database Metadata** | Captures the database schema. Runs the project's PHP. |
| **Laravel: Add Application Database Connection…** | Registers the app's database as an IDE database source. |
| **Laravel: Database Connections…** | Lists registered connections to connect or remove. |
| **Laravel: Connect Database…** | Enters credentials for a registered connection. |
| **Laravel: Remove Database Connection…** | Removes a connection and its saved credentials. |
| **Laravel: Read Model Database Connections…** | Reads the database connection and table of up to 32 models. Runs the project's PHP. |
| **Laravel: Retry Failed Job…** | Retries one failed job after you confirm. From the palette it asks which one. |
| **Laravel: Inspect Queue…** | Captures queue status for a connection or queue. |
| **Laravel: Open Error Stack Frame…** | Opens the file of a stack frame from a log entry read in the Runtime tab. From the palette it asks which entry. |
| **Laravel: Connect Application MCP Server…** | Connects to your app's own MCP server on `localhost` or `127.0.0.1` and reports how many tools, resources and prompts it has. |

## Settings

Settings › Plugins › *Laravel*.

**Inspections**

| Setting | Key | Default |
|---|---|---|
| Report views, config keys and translation keys that don't exist | `diagnostics` | on |
| Also report route names that aren't defined in routes/ (packages add routes the plugin can't see) | `reportMissingRoutes` | off |
| Report <x-…> Blade components that have no file or class | `reportUnknownComponents` | on |
| Report <livewire:…> components that have no class or view | `reportUnknownLivewire` | on |
| Report @livewire('…') components that have no class or view | `reportUnknownLivewireDirectives` | on |
| Report unknown wire:click and wire:submit methods | `reportUnknownLivewireMethods` | on |
| Report Route::resource() controllers that lack resource methods | `reportMissingResourceMethods` | on |
| Report controllers and controller methods named in routes/ that the project does not have | `reportUnknownRouteControllers` | on |
| Report Inertia pages that have no file under resources/js/Pages | `reportUnknownInertiaPages` | on |
| Report Model::factory() calls of models without a factory | `reportUnknownFactories` | on |
| Check Eloquent models and queries | `checkEloquent` | on |
| Mass assignment inspection exceptions by model class | `massAssignmentExceptions` | `{}` |
| Report env() calls outside the config directory | `checkFunctions` | on |
| Report request data passed unfiltered to create(), update() or fill() and variables in raw SQL | `checkSecurity` | on |
| Report where() values that the column's enum cast does not have | `enumValueInspection` | on |
| Show code that breaks on the next Laravel major version | `upgradeInspection` | on |
| Report translation keys that some locales have and others lack | `missingTranslationInspection` | on |

`massAssignmentExceptions` is a JSON object mapping model classes to field-name arrays. Use `["*"]` to ignore a model.

**Project layout**

| Setting | Key | Default |
|---|---|---|
| Extra view directories (project-relative, comma-separated) | `viewPaths` | empty |
| Extra model directories (project-relative, comma-separated), for models outside app/Models | `modelDirectories` | empty |
| Main locale: its translations are the ones shown and navigated to | `locale` | `en` |
| Application language directory (project-relative; empty uses framework defaults) | `languageDirectory` | empty |
| Public directory that asset() paths start from (project-relative) | `publicDirectory` | `public` |
| Namespace that 'Name@action' controller strings resolve in | `controllersNamespace` | `App\Http\Controllers` |

**Editor and interface**

| Setting | Key | Default |
|---|---|---|
| Show route and controller hints in the editor | `routeHints` | on |
| Offer to convert query scopes to the #[Scope] attribute (Laravel 12.6+) | `scopeAttributeIntentions` | on |
| Laravel interface language (`auto`, `en`, `ja`, `ko`, `zh-CN`) | `uiLanguage` | `auto` |

**Docker Artisan**

| Setting | Key | Default |
|---|---|---|
| Docker: socket address used to run Artisan in a container (unix:// path) | `artisanDockerSocket` | `unix:///var/run/docker.sock` |
| Docker: container ID that runs Artisan (leave empty to pick one per project with **Choose Docker Target…**) | `artisanDockerContainer` | empty |
| Docker: project folder inside the container (absolute path, mounted from this project) | `artisanDockerWorkdir` | empty |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › *Laravel*.

| Permission | Used for | Without it |
|---|---|---|
| `process:php` | Everything that runs Artisan or project PHP: commands, generators, Artisan, Runtime and Database tabs, Send Request, route and metadata refreshes. | Completion, navigation, checks and browsing still work. Commands that run PHP say "Allow the process:php capability in Settings › Plugins." |
| `process:php-win` | On Windows, background PHP runs through `php-win` with no console window. | Background PHP uses `php`. |
| `process:docker` | Choosing a container and running Artisan in it. | Artisan runs with local PHP only. |
| `process:composer`, `process:laravel` | **Laravel: New Laravel Project…** with Composer or the Laravel installer. | You can't create a project from the IDE. |
| `project.write` | Writing generated files (Generators tab, factory and resource bodies from migration columns), **Remove IDE Helper PHPDoc from Models**, **Regenerate Model Helpers…**, creating a missing view from **Create Missing View, Component or Controller**, and **Create HTTP Request File**. | Those commands say the permission is needed and write nothing; editor quick fixes still work. Artisan makes empty stubs. The HTTP request is copied to the clipboard instead. |
| `editor.read` | **Laravel: Create Missing View, Component or Controller** reads the name under the caret. | That command can't see the name. |
| `clipboard` | **Copy as curl**, Copy Output, and the fallback of **Create HTTP Request File**. | Nothing is copied. |
| `storage` | Caching Artisan command metadata and saving helper preferences. | Artisan metadata and helper preferences can't be cached. |
| `database.sources` | Registering the app's database as an IDE database source. | The database connection commands don't work. |
| `secrets` | The **Application MCP bearer token** and saved database credentials, in the system keychain. | You can't save those credentials. |
| `net:127.0.0.1`, `net:localhost` | **Laravel: Connect Application MCP Server…** to your app's local MCP server. | You can't connect to it. |
| `fs.read:~` | Listing empty directories for a `directoryFiles` source in your `ide.json`. | Everything works. Empty directories are left out, and an index issue says the grant is unavailable. Leave it denied if you don't use `directoryFiles`. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| Nothing completes and the tool window is empty | The project must have an `artisan` file or require `laravel/framework`. Run **Laravel: Reindex Project**. |
| Laravel commands are missing from Search Everywhere | Most commands show only when the project has an `artisan` file at its root. Open the folder that holds it. |
| "This project is not a Laravel application." / "Open a Laravel project first." | Open the folder that holds `artisan` or the `composer.json` requiring `laravel/framework`. |
| "Running PHP needs the process:php permission. Allow it in Settings › Plugins › Laravel." (sometimes with "…and make sure php is on your PATH", or "Running artisan needs the process:<program> permission…") | Allow `process:php` in Settings › Plugins › *Laravel*. Check that `php` is on the `PATH` the IDE sees. |
| The Artisan tab shows no commands | Run **Laravel: Refresh Artisan Command Metadata**. Run it again after `composer.json` or `composer.lock` changes. |
| "Refresh Artisan command metadata to configure …" | Same as above, then run the command again. |
| "Laravel route index is incomplete; reindex before searching." | Run **Laravel: Reindex Project**. Very large projects may hit the 5,000-file scan cap. |
| A missing view, config or translation is not reported | Checks pause while the index is partial. Reindex. Keys built at run time aren't checked. |
| Package routes are missing from the Routes tab | Run **Laravel: Refresh Registered Routes**. |
| A new file under `resources/js` or `resources/css` doesn't complete in `@vite` | Save a watched file or reindex. |
| "Project writes denied; HTTP request copied to clipboard." | Allow `project.write` to write `.http` files. |
| "No registered Laravel database sources. Add a Database Connection from Models first." | Run **Laravel: Add Application Database Connection…** first. |
| "No errors have been read yet. Click Logs in the Runtime tab and pick a log first." | Do that, then run **Open Error Stack Frame…** again. |
| "Laravel is still indexing. Try again in a moment." | Wait for the indexing progress to finish, then run the command again. |
| **Run Command From Class…** opens the Artisan tab | From Search Everywhere it uses the console command class open in the editor; with none open it shows the Artisan tab. |
| "No routes found. Open a Laravel project and let indexing finish, then try again." | A route command ran from the palette before the index had routes. Wait for indexing or run **Laravel: Reindex Project**. |
| Docker Artisan refuses the container | The container must be running and have exactly one bind mount of this project root. Volume-only, remote and Mutagen-style mounts are refused. |
| Component checks are silent | They are off when `composer.json` lists a package that ships un-namespaced components (`*/blade-*`, `laravel/jetstream`, `filament/*`, `livewire/flux`). |

## Limitations

- Checks compare against the last saved and indexed definitions. Save or reindex after changing a view, config, translation or route file; unsaved changes in another file and names registered at run time aren't seen.
- Works from a text index, not a full PHP type model. Container types, full Eloquent/facade semantics, imports and alias resolution, runtime registrations, effective runtime configuration and locale are not resolved.
- No framework-wide Rename or Find Usages. `Post`, `PostFactory` and `posts` are not treated as one symbol.
- Form Request promotion and view creation handle literal, simple cases only. Custom or namespaced view roots aren't supported there. Module resolution is incomplete.
- Vite entries refresh only after a reindex or watched save.
- Route names from vendor packages aren't in the static index. Inertia page names don't complete in JavaScript.
- Vendor-package Blade directives and run-time-registered container names are unknown.
- File templates use the default namespace of their type, with no prompts. Type the timestamp prefix of a migration file name by hand.
- No terminal autocomplete. Monolog format/timezone coverage and compiled-Blade frame mapping are partial.

## Technical details

<details>
<summary>Indexing, limits, caching and safety checks</summary>

### Index

Everything works from a text index of the project. Saving a file under `routes/`, `config/`, `lang/`, `resources/views/`, `app/Http/Controllers/` or `app/Mail/`, `.env`, `.env.example`, `bootstrap/app.php`, `app/Http/Kernel.php` or `composer.json` rebuilds the index. Scanning has a three-second budget and a 5,000-file discovery cap. Route group handling is textual: a group whose body is a file (`->group(base_path('routes/admin.php'))`) doesn't add its prefix to the routes in that file, and `Route::resource` guesses the singular of a parameter by dropping a trailing `s`. Endpoints compose route-file paths with literal `withRouting` API prefixes (including custom and empty) or literal `RouteServiceProvider` mounts; unsupported dynamic and unmounted API routes are omitted. Source-verified captured routes also appear in native endpoints discovery; source-mounted Search Everywhere endpoints refresh on routing configuration changes.

View namespaces and extra view roots come from `ide-blade.json` at the project root or `.idea/ide-blade.json` (both read when present): `{"namespaces": [{"namespace": "admin", "path": "modules/admin/views"}]}` indexes those views as `admin::users.index`. A blank or missing `namespace` adds a plain view root; an entry without `path` and malformed JSON are ignored; only these two paths are read. Vite entries, components and Livewire components are indexed with the project (a new file under `resources/js` or `resources/css` shows up after a reindex or the next watched save). Supported `ide.json` roots, custom literal call sites, package overrides, Passport scopes, additional gates/middleware, FQN model/table mappings, declared custom Route facade methods, bounded Wayfinder references, Vite config/manifest sources, consumed helper mixins/constructor forwarding with signature help, route request actions, proven imported/qualified/absolute controller-string conversions and additional rule-object conversions, and Blade component `viewPaths`/`phpNamespaces`/explicit list/`fromConfig` declarations feed the owned inventories and namespaced/bare-tag props consumers. Literal Composer `autoload`/`autoload-dev` PSR-4 roots extend the owned class/trait/cast/value-object/enum inventory.

Cache ownership follows the document's current innermost open project: a nested project can't inherit an outer app's inventory, unknown/denied ownership withholds cached results without discarding another root's cache, and view configuration inputs or targets owned by a separately opened nested project are refused. Candidate navigation targets are re-read through the project API so unsaved changes update ranges or remove stale links; file-change events refresh cached relationship keys. Navigation computes UTF-16 target widths from the recorded source token (a multiline spelling becomes a point destination), and Blade section/stack completion parses the current target text rather than projecting saved offsets onto a dirty buffer. Model/factory/policy/observer/seeder, migration/table, controller/route and listener/event relationships come from the project-owned navigation model; they are not published as host Rename/Find Usages anchors. Opaque PHP and Blade data (raw PHP blocks, Blade comments, stored `@php`/`@verbatim` blocks, heredocs, ambiguous delimiters) never becomes literal inventory facts, and PHP comments stay trivia in literal adapters.

### Completion details

- **Blade directives.** `@@if`, `user@example.com` and `@` inside `{{ }}` are left alone; vendor-package directives aren't known. Ranking: exact prefix first, then case-folded prefix, then substring. Known unmatched block context promotes closers (section closers keep `endsection` → `show` → `stop` order), and an opener before an already present unmatched closer inserts the opening line only. Comments, escaped at-signs, echoes, PHP blocks/strings, directive arguments and verbatim content are excluded. Bounded to 512 KiB, 8,192 directive tokens and 128 typed prefix bytes; core completion works before the cache is warm or without project access.
- **Component attributes.** Attribute names are kebab-case and keep the `:` prefix.
- **Containers.** `app/` classes are found PSR-4 by namespace line. Vendor or run-time-registered names are unknown.
- **Relations and scopes.** A relation is a model method whose body calls `$this->hasMany(Comment::class)` (or `hasOne`, `belongsTo`, `belongsToMany`, `morphMany`, `morphOne`, `morphToMany`, `hasManyThrough`, `hasOneThrough`); a scope is `scopeActive` or a `#[Scope]` method; anything else stays silent. The query-variable hint stays near its binding: the nearest exact variable occurrence must be a simple assignment (4 KiB heuristic), so assignments from other methods or nearer parameter/foreach/unset uses are not borrowed. Hints don't cross method/brace boundaries.
- **Eloquent members.** Model aliases, nullable typed parameters, simple local assignments, base-model inheritance, local scopes and known SoftDeletes methods are supported. Enum casts expose `name`, backed `value` and public instance methods. Traits from `app/Traits`, `app/Concerns`, model roots and `app/Models/Concerns` contribute relations, scopes, casts and typed methods (8 nesting levels, 32 traits per model; class methods win; cycles, conflicts and adaptation blocks stay quiet). Custom casts in `app/Casts` implementing `CastsAttributes` with a typed `get()` return expose that value object's public properties and methods (value objects in `app/ValueObjects`, `app/Data`), and public methods with one explicit class return type support further chains. Trait aliases, visibility adaptations and `insteadof` choices, nullable unions with one concrete target and supported intersections keep inference; ambiguous unions, DNF types, dynamic casts and arbitrary control flow don't. Documents are bounded to 128 KiB / 8,192 tokens; models use the configured model directories, enums `app/Enums`; navigation opens the owned declaration. No source reads, PHP, database or network access happen during completion.
- **Ziggy.** Works from the in-memory route index only and returns at once without `route(` / `.current(`. Page names are not completed in JavaScript (no Inertia client call takes one). Scanning is line-bounded for quotes and backticks; block comments carry across lines, so a `/*` in a multi-line template literal hides later route calls in the file.

### Check details

- **Inventory checks** run live on unsaved text through the diagnostics provider against the complete cached inventory. Sources over 32 KiB are refused, at most 200 findings are returned, and nothing is reported while the index is partial (the 3 s reading budget ran out). Config files with unsupported arrays or reached bounds are marked incomplete and their missing-key diagnostics are withheld.
- **Eloquent.** Mass assignment covers `Post::create($request->all())` / `(new Post)->fill($request->all())` when `Post` in `app/Models` or `app/` has `$guarded = []` and no `$fillable`, and a literal key of `Post::create([...])` that a non-empty `$fillable` leaves out. `Prunable`/`MassPrunable` is checked against `model:prune` in `routes/console.php`, `app/Console/Kernel.php` or `bootstrap/app.php` (quiet when none can be read). The `orderBy` check also covers any literal column ending in `_at`, `_date` or `date`. Models with `getFillable()`, `#[Fillable]` or `unguard` stay quiet, as do classes without a project file and dynamic class names.
- **Unknown names.** Package-namespace names, run-time-built names and providers registering component paths/namespaces (`anonymousComponentPath`, `componentNamespace`) stay quiet. Component checks are off when `composer.json` lists any `*/blade-*` package (icon packs such as `blade-ui-kit/blade-heroicons`), `laravel/jetstream`, `filament/*` or `livewire/flux`. Livewire checks are off when a provider calls `Livewire::component()`, `componentNamespace()`, `addNamespace()` or `addLocation()`. `Blade::component(Alert::class)` registers `alert`; a call with no literal alias or class makes the check quiet. Inertia pages may be `vue`, `jsx`, `tsx` or `svelte`.
- **Routes.** Codes `laravel.unknown-route-controller`, `laravel.unknown-route-method` and `laravel.missing-resource-methods`. The controller namespace comes from `controllersNamespace`. Quiet for invokables, vendor controllers, traits, `__call`, other parents than `Controller`, and `Route::controller(...)` groups.
- **Livewire.** Code `laravel.unknown-livewire-method`. Quiet for classes with a trait, a parent other than `Component`, `__call`, `#[Computed]`, `#[On]` or Volt, and for expressions (`$set`, `form.save`, `a = 1`).
- **Functions.** Code `env-outside-config`.
- Source-reading Eloquent, function, Livewire-method and route-controller checks run on open/save, not through the live provider.

### Quick fix and refactoring details

- Validation string ⇄ array conversion is not offered with `regex:` rules, `Rule` objects, closures, embedded `|` or regex values with `,`. `Rule::` conversion takes at most two arguments for `exists`/`unique` and adds the `Illuminate\Validation\Rule` import.
- `'UserController@index'` conversion is offered when the file is in `App\Http\Controllers` or imports the class (no import added).
- `@class` conversion is not offered with `{!! !!}`, `@if`/`@php`, a single quote, an empty value or an existing `@class`.
- **Move $x to a constructor property** creates a promoted property and rewrites `$x` to `$this->x`. Not offered when reassigned, captured by `compact`, or the class extends something other than `Controller`.
- **Inject … instead of the facade** replaces every use of a core facade with an injected property of the documented underlying class and removes the import.
- **Extract to translation key** adds the entry to the existing main-locale lang file (`messages` first; nothing is created).
- **Add validation rules for the columns of Post** appends missing columns (`'title' => ['required', 'string', 'max:255']`). It and **Add the missing columns to $fillable** need a multi-line or empty array.
- **Remove IDE helper PHPDoc from model** removes the `@property`/`@method`/`@mixin` lines. The bulk command covers up to 256 indexed models with one atomic, undoable workspace edit.
- Scope-to-attribute intentions need `composer.lock` to establish Laravel 12.6 or newer.
- **Create missing literal Blade view.** From a plain first argument of `view(...)` / `\view(...)` or non-overridden `@include`, `@includeIf`, `@extends`. Previews one new file under `resources/views` and applies one non-overwriting Create that opens it. Names are at most 128 ASCII bytes in up to 16 dot-separated alphanumeric/underscore/hyphen segments. Namespaces, escapes, interpolation, concatenation, heredocs, custom `viewPaths` and non-conventional `config/view.php` are refused, as are targets owned by a separately opened nested project. Sources are limited to 128 KiB, view configuration to 64 KiB; runs no PHP.
- **Extract reviewed HTML/simple-variable Blade view…** A complete selected HTML element up to 4 KiB in a bounded plain HTML/simple-echo Blade document becomes one non-overwriting Create plus one source Edit, applied atomically, preserving child bytes/CRLF and UTF-16 selection. PHP, directives, comments, components, script/style, computed expressions and custom include directives stay quiet.
- **Create reviewed mapped FormRequest …** An absolute `\Prefix\Http\Requests\NameRequest` parameter type (or a short type with one exact unaliased `use`) creates the missing PHP file through one exact production Composer PSR-4 mapping. Namespace prefixes and source roots up to 8 segments / 256 bytes; `App\` → `app/` and explicit roots like `domain/` supported; overlapping, fallback, dev, case-variant and opaque mappings refused; a source-root scan past 4,096 results refuses the operation, including case-equivalent targets. No controller imports or source change, no PHP.
- **Move validation rules to a Form Request…** Works on an explicit `\Illuminate\Http\Request` parameter of a plain controller method whose body is exactly `$request->validate([...])` (or `$validated = $request->validate([...]); return $validated;`). After review it creates the request class, transfers the literal rule array, and replaces the call with `validated()`. No PHP runs.
- **Rename.** Typed rename actions exist for proven route, config, translation and explicit Blade-alias literals; filename-based view/anonymous-component rename uses one reviewed move-and-reference transaction.
- **Find Source Matches…** scans conventional sources (routes, resources/views, config, lang, src and the current document; 2,048 paths, 8 MiB total, 512 matches, 128 KiB per source) and opens results; it adds no Rename anchors.
- **Find Unused Views** counts `view()`, `@include`, `@extends`, `Route::view`, mail and `markdown()` calls; a literal prefix (`'reports.' . $type`) counts as used.

### Hover

Hover on a closed-literal route, view, config or translation match shows the cached source location, bounded route/key/locale details and a Laravel documentation link. Documents over 128 KiB and names over 512 bytes are refused; displayed fields are capped at 512 characters. Unqualified helper names or one explicit global prefix (`view(...)` and `\view(...)`) receive Laravel features; explicitly foreign namespaces (`Vendor\view(...)`, `\Vendor\Config::...`) and foreign qualified classes don't.

### Generators

Generator commands ask for names and options in dialogs, then run `php artisan` in a native keyed terminal session (trusted projects only). A matching successful exit refreshes only the currently owned project index; failed, closed, foreign and duplicate completions don't. They are interactive terminal operations, not persisted Run configurations. Livewire generators appear only when Livewire is installed. Package/custom `make:*` commands come from a captured snapshot. Names are validated before they reach the command line. Generator entry points open the Artisan workspace's captured form when current local metadata matches; opening a form runs no PHP.

**New Eloquent Model...** has independent factory, resource and relation output selections. Factories map foreign keys ending in `_id` to declared owned factory classes (unknown mappings stay explicit placeholders); resources can include proven relations through `whenLoaded()`, optionally wrapped in an associated declared resource. Per-kind directories, namespaces, bases and postfixes flow through the reviewed plan and Composer proofs.

The Generators workspace adds model field/relation/index planning with a graphical existing-model editor (reviewed casts and mass-assignment edits), module-local factory/seeder generation, configurable generation classes/postfixes and per-kind destinations. Project custom presets and module mappings use `.openidea/laravel-generators.json` and `.openidea/laravel-templates/`; project/user preset CRUD and private user storage are available. Primary keys include numeric, UUID/ULID and custom strategies with timezone-aware timestamps. **New Laravel Project…** uses installed CLI help to gate options and launches an explicitly reviewed native terminal; failed installations are kept for inspection and never cleaned up automatically.

The Models workspace lists source and captured-schema model inventories with reviewed helper PHP, model-from-captured-table drafts, handwritten-PHPDoc-preserving model documentation and new-field/migration actions. Cast/accessor/inherited members and proven factory/resource/migration/policy/observer/seeder associations feed the grouped tree. Helper storage/count/date preferences persist per project; **Configure Model Helper Parameters…** saves them after review; **Regenerate Model Helpers…** supports multi-model selection, helper files or preserving/overwriting PHPDoc, proven cast/relation/scope signatures and optional counts. Helper post-generation jobs start only once their outputs exist unchanged; reading their state never starts project PHP.

File templates: Laravel Controller, Resource Controller, Job, Event, Listener, Mailable, Middleware, Form Request, Policy, Observer, Enum, Seeder, Factory, Migration (anonymous class), Pest Feature and Unit Test, Action, Cast, Global Scope, Notification, Validation Rule, Trait, JSON Resource, Service Provider, View Composer, Exception, Livewire Component, Broadcast Channel, Console Command, Http Exception, PHPUnit Feature and Unit Test, Resource Collection, plain Class, Eloquent Model, Dusk Test, Nova Resource, Livewire Form, Fractal Transformer, Livewire Volt Component, Inertia Vue / React / Svelte Page and Blade View, Layout, Anonymous Component and View Extending a Layout. They are written from Laravel's documentation and default stubs. The host gives a template only the file name and caret position.

### Tool window tabs

- **Routes.** Rows show the endpoint icon, path and grey `[METHOD] Controller` text. Method names appear in row details; multi-method routes ask which method for curl/HTTP generation. **Create HTTP Request File** writes `http/<verb>-<path>.http`. **Refresh Registered Routes** runs `php artisan route:list --json --no-interaction --no-ansi` (bounded to 4 MiB, 5,000 routes, four seconds of polling) and captures real URIs, names, methods and controllers including loops, conditional registration and package routes. It first rebuilds the selected project's source index. Snapshots stay in memory for up to eight projects and are retired by reindex; failed or cancelled refreshes keep the previous snapshot; the static source index remains the fallback, and unresolved dynamic URIs are omitted rather than shown as `/?`.
- **Request.** With rollback on, nothing is sent unless a transaction really started on the default connection. Act as user is not part of the copied curl request, and web routes still need their CSRF token against a real server.
- **Views.** Folder labels follow the physical path below the indexed root; vendor overrides and configured namespaces keep their Laravel namespace.
- **Config.** Values are bounded source previews (including `env(...)`, booleans, numbers, null, quoted strings), not evaluated configuration; array nodes stay labelled `array`. Indexing traverses list offsets, explicit integer keys and `ClassName::class` keys, supports 64 nesting levels, and marks files with unsupported arrays or reached bounds incomplete. Each source is limited to 2 MiB and 20,000 entries.
- **Translations.** The counter counts locale-specific entries, while completion and navigation prefer the configured main locale.
- **Database.** Compare and Rows run read-only PHP through the existing Artisan runner and need no new capability. Columns are masked by whole key words. **Refresh Database Metadata** captures schema explicitly (four-second polling, 4 MiB, 5,000 rows); connection names are kept in model and migration output, including migration create/drop calls. Runtime database sources support Connect through the native credential dialog and reviewed removal of connections and saved credentials.
- **Components.** Cached Blade class, anonymous and registered alias components with Livewire classes; template tag, component type and source file with sortable columns. Class components win over anonymous templates with the same tag; rows exclude sources owned by another open project; browsing never runs PHP.
- **Artisan.** Commands are grouped by namespace (General for unprefixed, up to eight Recent). The form shows an exact PHP argument-vector preview, contextual stdout/stderr and session history. Defaults are shown as hints and left to Artisan when inputs are omitted; required arguments and unsupported optional-argument gaps are validated before launch; repeated values use one entry per line; optional-value flags can be enabled without a value. Captured runs use an argument vector without a shell, with `--no-interaction` and `--no-ansi`; opt-in additional options and trailing arguments use quoted argv parsing, with extra options before the structured `--` boundary and trailing data after it. Captured execution uses local `php` or the configured Docker target, one captured run per project, up to 64 KiB stdout / 16 KiB stderr with explicit truncation, a five-minute polling limit (paused while the tab is hidden), and up to 20 history runs bounded to 128 KiB kept in memory. On Windows, background PHP runs through `php-win` when `process:php-win` is granted. Catalogs, drafts and recent commands stay separate per runner, and each Docker run re-inspects the target.
- **Runtime.** Snapshots: Application (`about --json`), Events (`event:list --json`), Schedule (`schedule:list --json`), Failed Jobs (`queue:failed --json`), Queue Status, Workers and Logs (one selected owned `storage/logs/*.log` file, at most 64 KiB / 200 lines). Queue statistics report unavailable metrics explicitly; worker visibility uses readable Linux `/proc`, other platforms report unavailable. Each capture has a four-second polling / 4 MiB output limit, a 5,000-row bound and owner checks before adoption; failed captures keep previous snapshots; the session cache is bounded to 32 captures / 8 MiB. Nothing runs merely by opening the tab; refresh explicitly after database/environment changes. Failed Jobs lists metadata, not pending counts, payloads or exceptions. Application information uses a fixed allowlist excluding config/environment secret values. Retry issues a single `queue:retry`; bulk retry is withheld.
- Reindex and saved source changes refresh mounted pages without scanning; the pages never run Artisan, CLI or HTTP work on their own.

### Artisan metadata

**Refresh Artisan Command Metadata** shows a complete review before anything runs: the verified project/working directory, the literal `php` runner (host PATH) and the exact argv `["artisan","list","--format=json","--no-interaction","--no-ansi"]`. Dismissing the review runs nothing. It needs `process:php`, `project.read` and optional `storage`. Discovery uses a kept process with closed stdin, at most 256 waits within a four-second budget, and rejects output beyond 4 MiB. The accepted snapshot (1–512 commands with unique bounded names and argument/option definitions, up to 128 each; order and bounded defaults preserved; aliases not used to build forms) is cached in schema v2 under a 5 MiB envelope / 4 MiB metadata bound. It is keyed to the project root, logical `php` runner, argv and fingerprints of `artisan`, `composer.json` and `composer.lock`. Failed, cancelled, stale or malformed refreshes keep the previous snapshot; changed fingerprints make the snapshot unavailable to the picker. Browsing, rendering, providers and typing never execute PHP.

**Docker.** **Refresh Docker Artisan Commands** reviews the exact project/CLI directory, endpoint, target, workdir, inspect and exec argv before any Docker process starts. Every invocation passes `--host unix:///...` explicitly (no ambient Docker context or `DOCKER_HOST`). Settings take a local `unix:///absolute/socket`, a full 64-hex running container ID and an absolute workdir. Before exec, fresh inspection must match the full ID, running/unpaused/not-restarting state, image/start identity and exactly one bind mapping this project root to the container directory; overlapping or shadowing mounts are refused. CLI processes share a four-second / 16-wait budget and a 4 MiB combined-output bound. Accepted metadata is stored under its own bounded 5 MiB envelope with image/start/mount proof and the same `artisan`/Composer fingerprints; failed or stale candidates keep the previous snapshot. **Choose Docker Target…** discovers running local containers (Compose/Sail/DDEV candidates from curated labels) and accepts only verified project bind mappings. Cancelling a Docker run stops the client; container PHP may continue.

### MCP tools

- `inventory`: paginated cached routes/views/components/models/config key names and related inventories, without config/environment values.
- `artisan_commands`: captured definitions and a revision token; never runs PHP.
- `runtime_snapshot`: captured section rows and timestamps; raw log tails require explicit `includeLogs: true`.
- `runtime_refresh`: executes one inspection command.
- `artisan_execute`: starts a captured command with structured values; returns a job.
- `artisan_job`: polls the owned job and bounded output.
- `artisan_cancel`: cancels the owned Artisan process.

Mutating tools use the host's MCP confirmation and Restricted Mode policy; project grants still apply. Example name: `aritti_laravel__inventory`.

### Commands that run Artisan

The one-click commands run `php artisan`, and all but **Show Route List** reindex afterwards.

### Why `fs.read:~` is requested (optional)

`project.read` is enough for everything except one case: a `directoryFiles` source in a project's `ide.json`, which offers the files of a directory as the names for a custom call site's argument. The project API only returns files, so an empty directory can't appear in that list; `host::fs::read_dir` can. That call needs a filesystem grant, and the only scope that covers a project wherever it lives is the user's home directory, so the plugin asks for `fs.read:~` rather than a narrower path it can't know in advance. Enumeration only happens while a `directoryFiles` source is being indexed. Each directory is first checked to be inside the current project (not a symlink, not a nested project's), the walk is bounded (64 listings, 4,096 entries, 31 levels deep), and the plugin never lists, reads or scans other files under the home directory. Only entry names and kinds (file or directory) are read, never file contents. If you deny the grant, nothing breaks: the inventory is built from the project's files as usual, empty directories are left out, and the plugin reports a partial-inventory diagnostic saying the scoped filesystem grant is unavailable.

### Capabilities

Required: `project.read` and `events`. Every optional capability is listed under [Permissions](#permissions); denied capabilities degrade rather than fail loudly.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
