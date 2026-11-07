# Reclassification of group C (PHP frameworks) against plugin API 1.2

Source: [C-php.md](C-php.md) (judged against API 1.1). Only B and F* rows are re-judged; F rows stay F. Row ids are the table/row numbers of C-php.md ("Sym insp 11" = Symfony inspections table row 11, "Sym other 7" = Symfony other-extension row 7, "LI 19" = Laravel Idea gap row 19, "FL 3" = free Laravel row 3).

## Facts from 1.2 that decide the flips

- `host::pick/pick_many/input/confirm` work only in a window command or event. Invalid in editor providers and in `resolve_code_action` (README line 360, `LIMITATIONS.md` "No popups"). So every code-action intention that wanted a dialog is unchanged.
- A command can edit only the active editor (`TextEditor::apply_edits`, needs `editor.write`). It cannot edit another file or create one (U2). `PickItem` is `{key,label,description}`, no filter/preview is documented.
- `process:php` is already declared (optional) in `aritti.laravel` and `aritti.symfony`, and both already call `run_process("php", ...)`. A command can therefore ask (pick/input) and then run `php artisan make:*` / `php bin/console ...`. PHP then creates the file, not the plugin. This is not the U2 API, but it removes the need for it for scaffolders that exist as artisan/console commands. Decision for the owner: the brief said make:* generators stay clipboard-only; this route contradicts that, so every row that uses it is marked F* (needs `process:php` approval, Laravel 11+ for `make:class`/`make:view`, maker-bundle for Symfony) and listed separately.
- Document indexers (`indexing.md`): per document, plugin-chosen string keys: definitions, references (Go to Declaration / Ctrl-click), aliases, markers (gutter icon to the key's definitions; several definitions open a chooser). Project-wide, background, so no 50 ms budget and no per-request `find_files`. It does not give find usages, rename, hover, completion, symbol search or diagnostics, and the plugin cannot query the graph. Limits 4096 defs/refs/markers per doc.
- Current code: `aritti.symfony` has `indexer.rs` with `service#` and `route#` only (php/yaml/xml/twig; skips `vendor/`, `var/cache/`). `aritti.laravel`, `aritti.blade`, `aritti.twig` have no `documentIndexers` entry. Symfony already shipped these codes since the audit: `twig-route-compare`, `twig-for-condition`, `twig-component-*`, `twig-missing-*`, `command-invokable-return-type`, `console-input-mode`, `orm-*`, `yaml-unquoted-colon`, `yaml-unescaped-backslash`, `filesystem-mirror-copy-on-windows`, `from-callable-option`, `tagged-collection-default-method` (these were F rows, not in scope here). Intentions "Add #[Route]", "Add parameter", "form type constants" and "Override block" exist in `intentions.rs`.

## Rows that flipped

| Row | Old | New | 1.2 feature | Sketch | Size |
|---|---|---|---|---|---|
| Sym other 26 Run configuration for console commands | F* | F | `pick` + existing `process:php` ("arbitrary command pick is U5" is gone) | Command "Symfony: Run Console Command...": list names from the index (`#[AsCommand]`, `$defaultName`) or `bin/console list --format=json`, `pick`, optional `input` for arguments, `open_terminal("console x", "php", ["bin/console", name, ...])` | S |
| Sym other 7 Console command names | F* | F | indexer keys `command#name` | Definitions: `#[AsCommand(name:)]`, `$defaultName`, `console.command` tag; references: `bin/console x`, `CommandTester`/`find('x')` strings, YAML/composer scripts. Same file shape as `indexer.rs` | S |
| Sym other 2 Service parameters (`%x%`) goto | F* | F | indexer keys `param#name` | Defs: YAML/XML `parameters:` and `<parameter key>`; refs: `%name%` in YAML/XML, `getParameter('x')`. Completion stays text-based; the old risk (bundle/env params invisible) now only means "no jump", not a false report | S |
| Sym other 10 Twig filters/functions/tests of project extensions | F* | F (project code only) | indexer keys `twig-filter#x`, `twig-function#x`, `twig-test#x` | Defs: `new TwigFilter('x'`, `#[AsTwigFilter('x')]`; refs: `\|x`, `x(` inside `{{ }}`/`{% %}` in `.twig`. Vendor extensions stay invisible (U3), so unchanged for them | M |
| Sym other 14 UX Twig components, Stimulus | F* | F | indexer keys `twig-component#name`, `stimulus#name` (extensions `php`,`twig`,`js`) | Defs: `#[AsTwigComponent]` classes and `templates/components/*.twig`, `assets/controllers/*_controller.js`; refs: `<twig:X>`, `{{ component('X') }}`, `data-controller="x"` | M |
| Sym other 15 Line markers (16 providers) | F* | F | indexer `markers` with several definitions open a chooser (removes "one target") | Move the 5 existing markers into `indexer.rs` and add route name -> `path()` usages, template -> all rendering controllers, block -> all overriding children. Custom icons still not possible (fixed allowlist) | M |
| Sym act 8 SymfonyTwigExtendsGenerator | B (U5) | F | `pick` + `apply_edits` on the active editor | Command in `editor.context` for `.twig`: `pick` over indexed templates, insert `{% extends '...' %}` at the top. Needs `editor.write` in the manifest | S |
| LI 23 RunArtisanCommand (arbitrary name) | B | F | `input`/`pick` + `process:php` | Command "Laravel: Run Artisan Command...": `run_process("php", ["artisan","list","--format=json"])` -> `pick` -> `input` for args -> `open_terminal`. Reuses `lib.rs:425` and `:509` | S |
| Sym other 17 Goto symbol/file contributors | B (U1) | F* | `pick` list + `open_location` | Commands "Go to Route/Service/Command...", `pick` over the index, `open_location`. Not Search Everywhere; no type-ahead filter documented, so large projects (hundreds of services) are unverified | S |
| LI 5 "Create missing component/view/factory/Livewire" quick fixes | B (U2, U5) | F* | `input` + `process:php` (not as a quick fix: `resolve_code_action` cannot ask or run) | Editor-context command on the flagged name: `confirm`, then `php artisan make:view` / `make:component` / `make:livewire` / `make:factory`, then `open_location` on the new file. Quick-fix surface itself stays B | S |
| LI 19 Model-aware Generate (ResourceController, Policy, Observer, Listener, Factories) | B | F* | `pick` model from the index + `input` name + `process:php` | `make:policy --model=X`, `make:observer --model=X`, `make:factory --model`, `make:controller --resource --model`, `make:listener --event`. Output is the stock stub, not the field/relation filling of Laravel Idea | M |
| LI 20 Generate JsonResource, Nova/Filament/Fractal/LivewireForm | B | F* | same | `make:resource` (core). Nova/Filament/Livewire only if their artisan commands exist in the project (`artisan list` check); Fractal has none | M |
| LI 22 NewEloquentModel, LaravelActionChooser | B | F* | `input`/`pick_many` + `process:php` | `pick_many` flags (migration, factory, seeder, controller, policy, resource) -> `php artisan make:model X -mfsc...`. ActionChooser = the pick over these commands | M |

## Rows reviewed and unchanged

| Row | Old | New | Why |
|---|---|---|---|
| Sym insp 2, 3, 4, 5, 6, 7, 8, 10, 13, 15, 18, 22, 24 | F* | F* | Diagnostics, no new data source: unchanged (U4/U3, vendor invisible, receiver type unknown). Indexer cannot be queried by the plugin and has no diagnostics |
| Sym insp 11 AutowireWrongClass, 14 TwigVariablePath, 19 mapping ArrayAccess | B | B | unchanged: U4 |
| Sym insp 27 create-template quick fix | B | B | unchanged: U2 (and `resolve_code_action` cannot ask) |
| Sym int 5, 6, 7, 8, 9 (service sync, tags, repo constants, command/ext rewrites) | F* | F* | Code actions: dialog-free; pick not allowed there. Class risks unchanged |
| Sym int 10, 11 | B | B | unchanged: U4 |
| Sym int 12, Sym act 7 compiler pass file | B | B | unchanged: U2; no maker command for compiler passes either |
| Sym act 4, 5, 6 (service/translation/form field generators) | F* | F* | A command may edit only the active editor; targets are other files, so they stay code actions |
| Sym act 9, Sym other 27 database | B | B | unchanged: no database API |
| Sym other 3 events/listeners | F* | F* | Indexer can do listener -> event goto, but `dispatch($e)` references need types; stays text with a risk |
| Sym other 8, 9, 12, 29 | F* | F* | unchanged: text-only data, no types; attribute auto-popup is U6 |
| Sym other 11 Twig macros | F* | F* | Goto already possible with the definition provider on known files; indexer adds nothing needed |
| Sym other 16 go-to-related | F* | F* | Markers with chooser approximate it (row 15), no related-items popup |
| Sym other 5, 6, 13, 21, 22, 24, 25 | B | B | unchanged: U4, injection, inlay, implicit-usage hooks |
| Sym other 18, 19, 20 find usages / move-rename / hover | B | B | unchanged: U1. Indexer references only drive Go to Declaration |
| Sym other 23 PHP folding, 28 MCP tools | B | B | unchanged: no folding control / no API |
| Twig 1, 2, 3, 5 (live templates, completion, syntax errors, formatter) | F* | F* | unchanged (no 1.2 feature touches them; `aritti.twig` has no indexer) |
| Twig 4, 6, 7, 8 | B | B | unchanged: U6, injection, U1, no API |
| Blade 3 formatter | F* | F* | unchanged: external formatter only |
| Blade 2, 4, 5, 6, 7, 8 | B | B | unchanged: U6, injection, U1 (usages/rename; view navigation is done), U1, U4, no API |
| LI 1, 2 (Livewire method, routes inspections) | F* | F* | unchanged: diagnostics, vendor/traits false positives |
| LI 11, 12, 13, 15, 16 | F* | F* | unchanged: code actions or other-file edits (pick not allowed in providers; commands edit active editor only) |
| LI 17 FindUnusedViews / RemoveModelPhpDocs | F* | F* | FindUnusedViews alone could be a command listing results in a view plus `reveal_view` (would be F, S); bulk phpDoc removal across files stays a risk. Split the row when implementing |
| LI 18 GenerateNewClass, LI 27 Blade components (companion view) | F* | F* | `artisan make:class` / `make:component` creates class+view, which removes the "companion file is U2" risk, but needs `process:php`; class unchanged |
| LI 31, 32, 33 (column completion, props/slots, 11 line markers) | F* | F* | Unchanged. Indexer (Laravel has none yet) would give multi-target markers for model/migration/factory/policy/observer, but convention matching is still the risk |
| LI 14 PromoteRequestToFormRequest | B | B | Creates a file and moves code into it: command cannot edit the new file (U2) |
| LI 21 ExtractView/ExtractBladeComponent/DTO/Override vendor/NewPivotTable | B | B | Need content written to a new file (U2); only NewPivotTable (`make:migration create_a_b_table`) could be F* |
| LI 24, 25, 29, 30, 37, 38 | B | B | unchanged: U4, no database API, no tooling hooks |
| LI 34, 35, 36 | B | B | unchanged: U1, decorations, injection |
| FL 1, 2, 4 (providers array, container names, `@inject` class) | F* | F* | unchanged: vendor classes not findable (U3), dynamic bindings |
| FL 3 container/`@inject` types, FL 5 Extract partial view, FL 7 string auto-popup | B | B | unchanged: U4; U2 (new file content); U6 |

## Summary

Counts (rows as grouped in C-php.md): B or F* rows re-judged, about 120 (Symfony 76, Twig 8, Blade 7, Laravel Idea 31, free Laravel 6 after grouping the listed multi-id lines). Flipped 13 rows.
- B -> F: 2 (Sym act 8 TwigExtendsGenerator, LI 23 RunArtisanCommand).
- B -> F*: 5 (Sym other 17 go-to commands, LI 5, LI 19, LI 20, LI 22). All but Sym other 17 rely on `process:php` artisan scaffolding (contradicts "make:* stay clipboard"; decide before building).
- F* -> F: 6 (Sym other 2, 7, 10, 14, 15, 26).
- Unchanged: everything else. Inspections and code-action intentions are untouched by 1.2; U1, U4, U6, injection, database rows are all still blocked.

Five best-value rows to implement:
1. Sym other 26 + LI 23 together: "Run Console/Artisan Command..." with pick/input, S each, daily-use, reuses existing `run_process`/`open_terminal`.
2. Sym other 15 line markers via the indexer (multi-target chooser: template to all controllers, block to all overrides, route to all `path()` users): M, replaces a documented LIMITATION.
3. Sym other 7 + 2 (console command and `%param%` goto) as two more key kinds in `indexer.rs`: S each, same pattern as `service#`/`route#`.
4. LI 19/20/22 generator commands (`make:model -mfsc`, `make:policy --model`, ...): M, the largest Laravel Idea action gap (about 26 B rows), pending the process-vs-clipboard decision.
5. Sym other 10 and 14 (Twig filters/functions from project extensions, UX components, Stimulus) as indexer keys: M, makes `aritti.symfony` navigate `.twig` against PHP and JS across the project. Runner-up: Sym act 8 (S).

Not covered by 1.2 and worth reporting upstream unchanged: pick inside `resolve_code_action`, or a command editing a second existing file (would unlock the translation/service/form generator rows, the largest remaining F* block).
