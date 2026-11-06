# Twig

Twig templates for OpenIDEA, which has no Twig grammar of its own. The plugin
detects `.twig` files and highlights them with a hand-written lexer: Twig
comments, prints and tags (inside HTML attributes too) and the HTML tags and
attributes around them. The contents of `verbatim` and `raw` are not read as Twig.
The structure view lists `block`s and `macro`s, and `{% x %}…{% endx %}` pairs
and multi-line comments fold.

Completion offers Twig's tags, filters, functions and tests by position
(after `{%`, `|`, `is`, or a bare name), and in HTML text thirteen snippets
that expand after typing a prefix: `blk` block, `mcr` macro, `esc`
autoescape, `emb` embed, `ext` extends, `for`, `from`, `if`, `imp` import,
`inc` include, `sbx` sandbox, `spl` spaceless, `verb` verbatim.

Inspections (on open and save, setting `diagnostics`) report block tags
without their `{% end… %}` (`unclosed-tag`), `{% end… %}` without an opening
tag (`unexpected-end-tag`) and a `{{`, `{%` or `{#` never closed
(`unterminated-delimiter`). A stray `}}` is not reported: it is common in
inline JavaScript and CSS.

In a Symfony project (`symfony.lock`, or `symfony/framework-bundle` in
`composer.json`) the `aritti.symfony` plugin owns name completion, so this
plugin leaves it out there. Route, template and translation completion also
comes from `aritti.symfony`.

There is no formatter: OpenIDEA only runs Prettier and Ruff from a language
definition, and neither understands Twig.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
