# OpenIDEA plugins

Plugins for [OpenIDEA](https://github.com/l0ng-ai/openidea), each a sandboxed WebAssembly component (`wasm32-wasip2`) with a `plugin.toml` manifest. Some are original, some are ports of JetBrains Marketplace plugins. All ids use the `aritti.` prefix (`aritti.<directory name>`).

## Plugins

| Area | Plugins |
|---|---|
| Files and editing | `blade`, `dotenv`, `ignore`, `properties`, `twig`, `openapi`, `ideolog` |
| PHP frameworks | `laravel`, `symfony` |
| Spring | `spring-beans`, `spring-boot`, `spring-cloud`, `spring-data`, `spring-security`, `spring-web`, `spring-batch`, `spring-graphql`, `spring-integration`, `spring-messaging`, `spring-modulith`, `spring-osgi`, `spring-runtime`, `spring-shell`, `spring-webflow`, `spring-websocket`, `spring-ws`, `spring-initializr`, `jpa-assist` |
| Java frameworks | `quarkus` |
| Database migrations | `flyway`, `liquibase` |
| Tooling and workflow | `dependency-audit`, `devcontainer`, `kube-explorer`, `gitlab`, `tasks` |

Each plugin directory has its own `README.md` (features) and `LICENSE`.

- [PARITY.md](PARITY.md): how close each plugin comes to the IntelliJ plugin it stands in for.
- [LIMITATIONS.md](LIMITATIONS.md): what the OpenIDEA plugin API can't do yet, per port.

## Layout

```
plugins/<name>/
  plugin.toml     manifest
  Cargo.toml      own [workspace]; no root Cargo workspace
  src/lib.rs
  LICENSE [NOTICE]
crates/           shared Rust code (spring-text)
docs/             audits and design notes
```

## Build

```sh
# once
cargo install --locked --git https://github.com/l0ng-ai/openidea ide-host --bin openidea-plugin

cd plugins/<name>
cargo test                 # logic against the SDK's MockHost
openidea-plugin build      # → dist/plugin.wasm, then validates the package
openidea-plugin link       # load the folder in the IDE; rebuild, then Settings › Plugins › Reload
```

Build everything:

```sh
for d in plugins/*/; do (cd "$d" && openidea-plugin build) || echo "FAILED: $d"; done
```

## Release

Versions are managed by release-please (see [CONTRIBUTING](.github/CONTRIBUTING.md)). A release is packed and signed with

```sh
openidea-plugin pack plugins/<name> --key <path outside repo> --publisher aritti
```

and published to the community registry [`l0ng-ai/openidea-plugins`](https://github.com/l0ng-ai/openidea-plugins) with one pull request per plugin version. The signing key never goes in this repo.

## Licence

MIT for original code (see [LICENSE](LICENSE)). Ports keep the original plugin's licence in their own `LICENSE`/`NOTICE`, which takes precedence for that directory.
