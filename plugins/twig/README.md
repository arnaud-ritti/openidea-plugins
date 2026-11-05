# Twig

Twig templates for OpenIDEA, which has no Twig grammar of its own. The plugin
detects `.twig` files and highlights them with a hand-written lexer: Twig
comments, prints and tags (inside HTML attributes too) and the HTML tags and
attributes around them. The contents of `verbatim` and `raw` are not read as Twig.
The structure view lists `block`s and `macro`s, and `{% x %}…{% endx %}` pairs
and multi-line comments fold.

Route, template and translation completion in Twig files comes from the
`aritti.symfony` plugin.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
