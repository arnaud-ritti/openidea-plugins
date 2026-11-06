# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this repo is

A monorepo of plugins for [OpenIDEA](https://github.com/l0ng-ai/openidea): sandboxed WebAssembly components (`wasm32-wasip2`) plus a `plugin.toml` manifest each. Some plugins are original, some are ports of JetBrains Marketplace plugins.

Project skills (in `.claude/skills/`) hold the platform knowledge. Load them instead of guessing APIs:
- `writing-openidea-plugins`: manifest, capabilities, Rust SDK, host API, limits, packaging. `reference.md` is the full upstream doc.
- `publishing-openidea-plugins`: release flow and pull request to the community registry `l0ng-ai/openidea-plugins`.
- `porting-intellij-plugins-to-openidea`: Marketplace lookup, licence gate, `plugin.xml` → OpenIDEA mapping.

## Layout

```
plugins/<name>/        # one self-contained plugin package each
  plugin.toml          # id = "aritti.<name>"
  Cargo.toml           # code plugins only; has its own empty [workspace]
  src/lib.rs
  LICENSE [NOTICE]     # required for ports: the original plugin's licence files
```

- Create plugins with `openidea-plugin new plugins/<name>` (`--template theme|icon-theme|language` for data-only). Directory name = id suffix.
- Every plugin is its own Cargo project (the scaffold adds `[workspace]` to its `Cargo.toml`). Don't add a root Cargo workspace; run cargo commands inside the plugin directory.
- Publisher prefix is `aritti.` for every plugin id. Never use `openidea.*` (reserved for official plugins, signed with the official key).

## Commands

```sh
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin   # once

cd plugins/<name>
cargo test                       # logic against the SDK's MockHost
cargo test <test_name>           # one test
openidea-plugin build            # cargo build --release --target wasm32-wasip2 → dist/plugin.wasm, then check
openidea-plugin check            # validate the package as the IDE's installer would
openidea-plugin link             # load the folder in the IDE; loop: build, then Settings › Plugins › Reload

for d in plugins/*/; do (cd "$d" && openidea-plugin build) || echo "FAILED: $d"; done             # all plugins
```

Release: `openidea-plugin pack plugins/<name> --key <path outside repo> --publisher aritti` → `aritti.<name>-<version>.oiplugin`. Publish through the community registry (`l0ng-ai/openidea-plugins`: register the publisher once, then one pull request per plugin version) or a static index made with `openidea-plugin index`.

## Rules

- The signing key never enters the repo (`*.key` is gitignored). A leaked key lets anyone publish updates to installed `aritti.*` plugins; rotating means `index --previous-key` or a revocation.
- Bump `version` (semver) and `change-notes` in `plugin.toml` for every release; the marketplace keeps only the newest version per API requirement.
- Request only the capabilities the code uses; put degradable ones in `optional` and handle `Error::Denied`.
- Ports: run the licence gate before any work. Source/decompile ports keep `LICENSE`/`NOTICE`, credit original authors and record source plugin, version and revision in `description` or `change-notes`. Paid/proprietary plugins are clean-room only: no jar download, no copied code, strings, icons or name.
- The plugin API (WIT, SDK, host) is upstream. A plugin needing a new extension point goes to `l0ng-ai/openidea` as its own pull request; don't vendor or patch the SDK here.
