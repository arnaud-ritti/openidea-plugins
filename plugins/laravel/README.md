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

| Names           | Indexed from                                                                                    | Referenced in                                                                                                                                 |
| --------------- | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| Routes          | `->name()`, `['as' => …]`, `Route::resource` (`only`, `except`, `names`) in `routes/`, with the name prefixes of the groups around them | `route()`, `to_route()`, `redirect()->route()`, `Redirect::route()`, `URL::signedRoute()`, `Route::has()`, `routeIs()`                       |
| Views           | `resources/views/**` (`.blade.php`, `.php`), `vendor/<package>/` overrides as `package::name`, extra directories from the settings | `view()`, `View::make/exists/first`, `Route::view`, `->view()`, `->markdown()`, `new Content(view:)`; Blade `@extends`, `@include*`, `@each`, `@component` |
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
- Quick fix (⌥⏎) on a missing `file.key` translation key: add `'key' => 'key'`
  to the main locale's lang file.
- Gutter: `view()` → view file; controller method → its route; view → views
  extending or including it and the controllers rendering it; `@section` →
  parent section and the views filling it; `@push` ↔ `@stack`.

## Tool window and commands

The _Laravel_ tool window lists routes (method, URI, name and action), views,
config keys, translations, middleware and env keys; double-click opens the
definition. Commands, all under the _Laravel_ category: Reindex Project, Clear
All Caches (`optimize:clear`), Clear Application / Config / Route Cache,
Clear Compiled Views, Run Migrations, Link Storage and Show Route List. They
run `php artisan` (optional `process:php` capability, trusted projects only)
and the first seven reindex afterwards. Saving a file under `routes/`,
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

## Not ported

These rely on PhpStorm's PHP PSI and type inference, which the OpenIDEA plugin
API has no equivalent for (see `LIMITATIONS.md`):

- the service container (`app('…')`, `App::make`, facade resolution, `@inject`
  classes), custom Blade directives from `Blade::directive()`, and component
  slots and attributes;
- Eloquent relations and attributes added by accessors, and `where('column')`
  on a query whose model is lost (`$user->posts()->where(...)`); dropped and
  renamed columns aren't tracked; config `providers` class completion;
- the _Extract partial view_ refactoring, and the `ide-blade.json` view
  namespaces;
- artisan `make:*` commands, which need a name and the plugin API has no
  input dialog.

Vite entries, components and Livewire components are indexed when the project
is, so a new file under `resources/js` or `resources/css` shows up after a
reindex or the next save of a watched file.

Route group handling is textual: a group whose body is a file
(`->group(base_path('routes/admin.php'))`) doesn't add its prefix to the
routes in that file, and `Route::resource` guesses the singular of a
parameter by dropping a trailing `s`.
