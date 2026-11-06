# Laravel

Laravel support for OpenIDEA in PHP and Blade files. Partial port of the
[Laravel Plugin](https://github.com/Haehnchen/idea-php-laravel-plugin) for
IntelliJ (`de.espend.idea.laravel` 0.15.4, revision `3a07f4a5`, MIT, by Daniel
Espendiller). The port uses only the free code of the public repository; the
features that plugin doesn't have follow [Laravel's documentation](https://laravel.com/docs).

Blade highlighting, folding and the structure view come from the `aritti.blade`
plugin (the IDE has no Blade grammar). Without it, Blade files open as PHP, and
everything below still works in them.

## Completion and go to definition (⌘B)

Typing `@` (or `@fo`) in a Blade file completes Laravel's and Livewire's own
directives (`@if`, `@foreach`, `@section`, `@livewire`, `@vite`, the `@end…`
closers) and the custom ones the project registers with `Blade::directive('name'`
in `app/**`. The common ones insert a snippet with their arguments and closing
directive (`@if(…)…@endif`, `@foreach`, `@section`, `@include`, `@vite`); the
others and the custom ones insert their name only. `@@if`, `user@example.com`
and `@` inside `{{ }}` are left alone. Directives registered by vendor packages aren't known.

| Names           | Indexed from                                                                                    | Referenced in                                                                                                                                 |
| --------------- | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Routes          | `->name()`, `['as' => …]`, `Route::resource` (`only`, `except`, `names`) in `routes/`, with the name prefixes of the groups around them | `route()`, `to_route()`, `redirect()->route()`, `Redirect::route()`, `URL::signedRoute()`, `Route::has()`, `routeIs()`                       |
| Views           | `resources/views/**` (`.blade.php`, `.php`), `vendor/<package>/` overrides as `package::name`, extra directories from the settings and from `ide-blade.json` (see below) | `view()`, `View::make/exists/first`, `Route::view`, `->view()`, `->markdown()`, `new Content(view:)`; Blade `@extends`, `@include*`, `@each`, `@component` |
| Config          | `return [...]` of `config/**/*.php`, nested keys joined with `.`                                | `config()`, `Config::get/has/set/string/…`                                                                                                    |
| Translations    | `lang/<locale>/<file>.php` (`file.key`), `lang/<locale>.json`, `lang/vendor/<package>/…`, main locale first | `__()`, `trans()`, `trans_choice()`, `Lang::get/has/choice`, Blade `@lang`                                                                  |
| Env keys        | `.env.example`, `.env` (names only, values are never read)                                      | `env()`, `Env::get()`                                                                                                                         |
| Middleware      | `$middleware->alias([...])` in `bootstrap/app.php`, `$routeMiddleware` / `$middlewareAliases` in `app/Http/Kernel.php`, the framework's own | `->middleware()`, `->withoutMiddleware()`, `'middleware' => […]`; the part after `:` (`throttle:60,1`) is ignored                             |
| Controllers     | `app/Http/Controllers/**`, public instance methods                                              | `Route::get('/x', 'UserController@index')`, `['uses' => …]`, `[UserController::class, '…']`, `Route::resource('x', '…')`, `action()`         |
| Assets          | files under `public/`                                                                           | `asset()`, `secure_asset()`, `URL::asset()`                                                                                                   |
| Blade components | `resources/views/components/**` (`card/index` is `card`), `app/View/Components/**` as kebab-case (`Forms/InputField` is `forms.input-field`), classes win | `<x-forms.input-field>` and `</x-…>` tags |
| Livewire components | `app/Livewire/**`, `app/Http/Livewire/**` as kebab-case                                | `<livewire:counter>`, `@livewire('counter')`                                                                                                   |
| Vite entries    | `resources/**` files with a `css`, `scss`, `sass`, `less`, `js`, `jsx`, `ts`, `tsx` or `vue` extension | Blade `@vite(...)`, `Vite::asset()`                                                                                                            |
| Abilities       | `Gate::define('name')` in `app/Providers/**`, public methods of `app/Policies/**` (except `before`, `after`) | `->can()`, `->cannot()`, `->authorize()`, `Gate::allows/denies/check/authorize/inspect/any/none`; Blade `@can`, `@cannot`, `@canany`, `@elsecan*` |
| Validation rules | Laravel's rule names (fixed list; custom rule objects aren't strings)                         | strings and lists in `->validate()`, `->validateWithBag()`, `Validator::make()` and a form request's `rules()`; completes the rule under the cursor, not after its `:` |
| Columns         | `$table->string('name')` and the other Blueprint column methods in `Schema::create` / `Schema::table` of `database/migrations` (`id`, `timestamps`, `softDeletes`, `morphs`, `foreignIdFor`…) | `Post::where('…')`, `orderBy`, `select`, `pluck`, `whereIn`… on a static call or a chain that starts with the model (`Post::query()->with('x')->where('…')`), `DB::table('posts')->…`, and `$query->where('…')` or `$fillable` / `$guarded` / `$hidden` / `$visible` / `$casts` / `$dates` in the model file itself. The table is the model's `$table`, else the snake-case plural of its class name |
| Sections, stacks | the layouts a Blade view extends (its `@extends` chain)                                         | Blade `@section`, `@yield`, `@hasSection`, `@push`, `@prepend`, `@stack`                                                                      |

## Problems, quick fix and gutter

- Problems: a view that doesn't exist, a config key missing from an array (or
  config file) the project defines, a translation key missing from a lang file
  the project has. Keys built at run time or with a wildcard aren't checked.
  Route names are only checked when _Settings › Plugins › Laravel_ asks for it,
  because packages add routes the plugin can't see. Files are checked when
  opened or saved.
- Eloquent problems (setting `checkEloquent`, PHP files): `Post::create($request->all())`
  or `(new Post)->fill($request->all())` when `Post` in `app/Models` or `app/` has
  `$guarded = []` and no `$fillable`; a literal key of `Post::create([...])` that
  a non-empty `$fillable` leaves out (mass assignment discards it); a model using
  `Prunable` or `MassPrunable` while none of `routes/console.php`,
  `app/Console/Kernel.php` and `bootstrap/app.php` mentions `model:prune` (quiet
  when none can be read); and `orderBy('created_at', 'desc')` /
  `orderBy('created_at')` (also any literal column ending in `_at` or `_date`,
  or `date`), a hint with a quick fix to `latest()` / `oldest()` (with the
  column when it is not `created_at`).
  Models with `getFillable()`, `#[Fillable]` or `unguard` stay quiet, as do
  classes without a file in the project and dynamic class names.
- Function problems (setting `checkFunctions`, PHP files): `env-outside-config`,
  a warning on `env()` calls outside `config/`, `bootstrap/`, `vendor/` and
  `tests/`, because `env()` returns null once the configuration is cached.
- Quick fix (⌥⏎) on a missing `file.key` translation key: add `'key' => 'key'`
  to the main locale's lang file.
- Intentions (⌥⏎): `'required|email'` in a validation call or `rules()` becomes
  `['required', 'email']` (not offered when a rule is `regex:`) and back: a list of
  plain string rules becomes `'required|email'` (not offered when an element is
  a `Rule` object, closure or contains `|`, or a regex rule contains `,`);
  `'UserController@index'` becomes `[UserController::class, 'index']` when the
  file is in `App\Http\Controllers` or already imports the class (no import is
  added); in a `.blade.php` file `{{ $x }}` and `{!! $x !!}` toggle.
- More intentions (⌥⏎), each editing the open file only:
  - Blade `class="btn btn-primary"` becomes `@class(['btn', 'btn-primary'])`, and
    `class="btn {{ $size }}"` becomes `@class(['btn', $size])` (not offered with
    `{!! !!}`, `@if`/`@php`, a single quote, an empty value or an existing `@class`).
  - "Convert all validation rules in this array to arrays": every `'a|b'` of the
    array that can be converted (at least two; `regex:` and non-string items stay).
  - "Convert all controller strings in this file": every `'Name@action'` and
    `Route::resource('x', 'Name')` string whose class is in the controllers
    namespace or imported (at least two). A single `Route::resource` string
    converts to `Name::class`.
  - `'exists:users,id'`, `'unique:users,email'` (at most two arguments),
    `'in:a,b'` and `'not_in:a,b'` in a rule list become `Rule::exists(...)`,
    `Rule::unique(...)`, `Rule::in([...])`, `Rule::notIn([...])` with the
    `Illuminate\Validation\Rule` import (not offered when another `Rule` is imported).
  - `Route::`, `Auth::`, `Str::` and the other core facades get their `use` line
    when the file has a namespace and does not import the name.
- Setting `controllersNamespace` (default `App\Http\Controllers`): the namespace
  that `'Name@action'` strings resolve in.
- Gutter: `view()` → view file; controller method → its route; view → views
  extending or including it and the controllers rendering it; `@section` →
  parent section and the views filling it; `@push` ↔ `@stack`.

## Tool window and commands

The _Laravel_ tool window lists routes (method, URI, name and action), views,
config keys, translations, middleware and env keys; double-click opens the
definition. Commands, all under the _Laravel_ category: Reindex Project, Clear
All Caches (`optimize:clear`), Clear Application / Config / Route Cache,
Clear Compiled Views, Run Migrations, Link Storage, Seed Database, Fresh
Migration and Show Route List. Fresh Migration drops all tables, so it asks
for a second run within 30 seconds. They run `php artisan` (optional
`process:php` capability, trusted projects only) and all but Show Route List
reindex afterwards. Run configurations (`php artisan serve`, `test`,
`queue:work`, `schedule:work`) are offered for any project with an `artisan`
file. Saving a file under `routes/`,
`config/`, `lang/`, `resources/views/`, `app/Http/Controllers/` or `app/Mail/`,
`.env`, `.env.example`, `bootstrap/app.php`, `app/Http/Kernel.php` or
`composer.json` also rebuilds the index.

A project counts as Laravel when it has an `artisan` file or requires
`laravel/framework`. Settings: problems on/off, route problems, one switch each
for unknown Blade components, Livewire components (tag and `@livewire`),
Inertia pages and `Model::factory()` without a factory, extra view
directories, main locale, public directory.

Problems for names that don't exist (warnings): `<x-name>` with no file under
`resources/views/components` or class under `app/View/Components`;
`<livewire:name>` / `@livewire('name')` with no class under `app/Livewire` and
no `livewire.name` view; `Inertia::render('Page')`, `inertia('Page')` and
`Route::inertia()` naming a page missing from `resources/js/Pages` or
`resources/js/pages` (`vue`, `jsx`, `tsx`, `svelte`); `Model::factory()` of a
model of the project without `database/factories/<Model>Factory.php`. Names in
a package namespace (`<x-package::alert>`), names built at run time, and
projects where a provider registers component paths or namespaces
(`anonymousComponentPath`, `componentNamespace`) stay quiet. Component
checks are also off when `composer.json` lists a package that ships
un-namespaced components: any `*/blade-*` package (icon packs such as
`blade-ui-kit/blade-heroicons`), `laravel/jetstream`, `filament/*` or
`livewire/flux`. Livewire checks are off when a provider calls
`Livewire::component()`, `componentNamespace()`, `addNamespace()` or
`addLocation()`. `Blade::component(Alert::class)` registers `alert`; a
`Blade::component()` call with no literal alias or class makes the component
check quiet. None of the five checks reports while the index is partial (the
3 s reading budget ran out).

## New file templates

New › Laravel Controller, Resource Controller, Job, Event, Listener, Mailable,
Middleware, Form Request, Policy, Observer, Enum, Seeder, Factory, Migration
(anonymous class), Pest Feature Test and Pest Unit Test, written from Laravel's
documentation and default stubs. The host gives a template only the file name
and the caret position, so each uses the default namespace of its type
(`App\Http\Controllers`, `App\Jobs`, ...) and has no prompts for a model or
table; edit the namespace by hand when the file lives in a subfolder. A
migration file name needs its timestamp prefix typed by hand (see
`LIMITATIONS.md`).

## JavaScript, TypeScript and Vue

In `javascript`, `typescript`, `vue`, `javascriptreact` and `typescriptreact`
files the plugin completes and opens (Go to) Ziggy route names in
`route('...')` and `route().current('...')`, including `:href="route('...')"`
in a Vue template. It works from the in-memory route index only and returns
at once when the file has no `route(` / `.current(`. Page names are not
completed in JavaScript: no Inertia client call takes one (the page name is
resolved by `resolvePageComponent` and glob imports).

Declaring a provider for these languages makes the host load the plugin for
them, so activation is limited to `on-file:resources/js/**`. The language ids
`vue`, `javascriptreact` and `typescriptreact` are declared but unverified in
the IDE; files are also recognised by extension. Scanning is line-bounded for quotes and backticks; block comments carry across
lines, so a `/*` in the text of a multi-line template literal hides later route
calls in the file, and a stray quote hides route calls to the end of its line.
Comments and strings are tracked with a simple scan (no regex literals, no Vue text awareness; a stray quote can pair with a later one on the same line).

## Not ported

These rely on PhpStorm's PHP PSI and type inference, which the OpenIDEA plugin
API has no equivalent for (see `LIMITATIONS.md`):

- the service container (`app('…')`, `App::make`, facade resolution, `@inject`
  classes), custom Blade directives registered outside `app/`, and component
  slots and attributes;
- Eloquent relations and attributes added by accessors, and `where('column')`
  on a query whose model is lost (`$user->posts()->where(...)`); dropped and
  renamed columns aren't tracked; config `providers` class completion;
- the _Extract partial view_ refactoring;
- artisan `make:*` commands, which need a name and the plugin API has no
  input dialog.

View namespaces and extra view roots come from `ide-blade.json` at the project
root or `.idea/ide-blade.json` (both are read when present):
`{"namespaces": [{"namespace": "admin", "path": "modules/admin/views"}]}`
indexes those views as `admin::users.index`; a blank or missing `namespace`
adds a plain view root, an entry without `path` is ignored, and malformed JSON
is ignored. Limit: only these two paths are read, not an `ide-blade.json`
elsewhere in the project, and saving either file reindexes.

Vite entries, components and Livewire components are indexed when the project
is, so a new file under `resources/js` or `resources/css` shows up after a
reindex or the next save of a watched file.

Route group handling is textual: a group whose body is a file
(`->group(base_path('routes/admin.php'))`) doesn't add its prefix to the
routes in that file, and `Route::resource` guesses the singular of a
parameter by dropping a trailing `s`.
