---
name: writing-openidea-plugins
description: Use when creating, building, testing, debugging, packaging or publishing an OpenIDEA plugin (plugin.toml, openidea-plugin CLI, openidea-plugin Rust SDK, WIT openidea:plugin world, .oiplugin, wasm32-wasip2 component), or when a plugin is refused, Inactive, Failed, denied a capability, or drops editor results.
---

# Writing OpenIDEA plugins

## Overview

OpenIDEA plugins are sandboxed WebAssembly components (wasmtime, component model, WASI p2) plus a `plugin.toml` manifest. Contributions declared in the manifest (commands, menus, keymaps, themes, icon themes, languages, settings, web views…) work **without code**; code is loaded lazily only when needed.

Full upstream doc: `reference.md` in this directory (source: `l0ng-ai/openidea` `plugins/README.md`). Grep it by section heading; do not guess API names, read the table.

## Workflow

```sh
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin
openidea-plugin new my-plugin                 # --template theme | icon-theme | language
cargo test                                    # logic against the SDK's MockHost
openidea-plugin build                         # → dist/plugin.wasm, then runs `check`
openidea-plugin link                          # load folder in IDE; edit loop: build, then Settings › Plugins › Reload
openidea-plugin check [pkg.oiplugin]          # why a package would be refused
openidea-plugin keygen acme.key && openidea-plugin pack <dir> --key acme.key --publisher acme
```

Debug logs: start IDE with `RUST_LOG=info,ide_plugin=trace`. `host::log` and `eprintln!` go to the IDE log.

## Minimal Rust plugin

```rust
use openidea_plugin::{Error, Invocation, NotificationLevel, Plugin, host, register_plugin};

#[derive(Default)]
struct WordTools;

impl Plugin for WordTools {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("no file is open")?;
        match id {
            "count" => {
                let words = editor.document()?.text.split_whitespace().count();
                host::notify(NotificationLevel::Info, "Word count", &format!("{words} words"));
                Ok(None)
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }
}

register_plugin!(WordTools); // add `providers`, `views`, `syntax` when implementing those traits
```

Test: `let host = openidea_plugin::mock::MockHost::install();` then `host.open_editor(...)`, `host.invocation()`, `host.notifications()`, `host.editor_text()`. `.with_capabilities(&[...])` enforces grants like the IDE.

## Where to look in reference.md

| Need | Section |
|------|---------|
| Manifest fields, contribution points table | §2 The package, Contribution points |
| `activation` events (`on-language:`, `on-file:`, `on-schedule:`…) | §2 Activation |
| Capability names and what they unlock | §2 Capabilities |
| Host API (SDK ↔ WIT table) | §3 The API |
| Events, editor providers, network + secrets, views + custom editors, web views, settings, remote projects | §3 headings: Events, Providers, Network and secrets, Views and custom editors, Web views, Settings, Remote projects |
| Time/CPU/memory limits, crash policy | §3 Sandbox and limits |
| Themes / icon themes / languages / run detectors | §4, §5, §6 |
| Install, project plugins, troubleshooting | §7 |
| Signing, marketplace index, key rotation, community registry | §8 |
| Rebuilding official plugins and fixtures, `plugins/build.sh` | §9 (inside the openidea repo) |
| Adding a host function / changing WIT | §10 |

## Rules that bite

- **Instances are disposable.** Idle 5 min → reclaimed. Keep state in `host::storage` (1 MiB), not memory.
- **Budgets:** commands 5 s (60 s with an open `host::begin_task`), editor providers 50 ms (result dropped), interrupted at 500 ms, 256 MiB memory. Calls are serialized per instance: a long command delays providers.
- **Missing capability** → `Error::Denied("<cap>")`; degrade, don't fail. Declare it in `[capabilities] required`/`optional`.
- **Edits are versioned:** `apply_edits(version, …)` returns `Error::Stale` if the document changed; `transform_selections` retries once.
- **Positions:** editor API uses 0-based lines, UTF-16 columns; syntax provider uses UTF-8 byte offsets.
- **Forward compat:** SDK event/body enums are `#[non_exhaustive]`; always add a `_` arm. Unknown kinds arrive as `Other { kind, data }`.
- **One invalid contribution rejects the whole manifest.** Run `check`.
- **Providers are only called when declared:** `[[contributes.providers]]` `kinds` + `languages` must match the file's language id (or `"*"`), and `register_plugin!(P, providers)`. Works in `cargo test`, silent in IDE → check this first.
- **Secrets are never readable:** declare `[[contributes.secrets]]`, request `secrets` + `net:<host>`, put `{{secret:<name>}}` in a request header of `host::fetch`; host fills it.
- **Web views:** sandboxed iframe, no network, only files under the page's directory; build with relative paths (Vite `base: "./"`).
- **Released WIT is frozen:** never add fields/cases/params to released types or functions to exports; add new types, functions, interfaces.
