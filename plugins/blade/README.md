# Blade

Laravel Blade templates for OpenIDEA, which has no Blade grammar of its own. The
plugin detects `.blade.php` files and highlights them with a hand-written lexer:
Blade comments, `{{ }}` and `{!! !!}` echoes, directives and their arguments,
`@php` blocks, `<?php ?>` blocks, PHP inside them, and the HTML tags and
attributes around them. The contents of `@verbatim` are not read as Blade, and
`@@directive`, `@{{ }}` and email addresses are left alone.

The structure view lists `@section`, `@push`, `@prepend` and `@fragment`
blocks. `@if … @endif` style pairs (about 35 of Laravel's and Livewire's block
directives) and multi-line comments fold.

Only the directives Laravel and Livewire ship are known to the highlighter;
directives registered with `Blade::directive()` stay plain text, since a plugin
can't see the PHP that registers them.

Directive, route, view, config and translation completion in Blade files comes
from the `aritti.laravel` plugin (it also completes custom directives found in
`app/`).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
