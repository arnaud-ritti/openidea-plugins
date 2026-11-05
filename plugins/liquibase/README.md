# Liquibase Changelogs

An OpenIDEA plugin.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```

`plugin.toml` declares the commands, so they appear in Find Action before the
code is loaded, and the capabilities the code needs. `src/lib.rs` implements
them with the `openidea-plugin` SDK.
