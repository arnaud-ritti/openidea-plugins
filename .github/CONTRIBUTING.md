# Contributing

## Setup

```sh
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin
rustup target add wasm32-wasip2
```

## Workflow

Each plugin is its own Cargo project. Run commands inside `plugins/<name>`:

```sh
cargo test
openidea-plugin build
```

- New plugin: `openidea-plugin new plugins/<name>`. Id is `aritti.<name>`; never `openidea.*`.
- Request only the capabilities the code uses.
- Ports: check the original's licence first; keep its `LICENSE`/`NOTICE` and credit the authors. Proprietary originals are clean-room only.
- Commits follow Conventional Commits (`feat(spring-web): ...`); release-please derives versions and changelogs from them. Don't bump `version` by hand.
- The plugin API is upstream. Needing a new extension point means a pull request to [`l0ng-ai/openidea`](https://github.com/l0ng-ai/openidea), not a patch here.
- Never commit signing keys.
