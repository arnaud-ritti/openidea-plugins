# Dev Container CLI

Runs the project's [Dev Container](https://containers.dev) with the
[devcontainer CLI](https://github.com/devcontainers/cli):

| Command | Runs |
|---|---|
| Dev Container: Start Container | `devcontainer up` in a terminal |
| Dev Container: Rebuild Container | `devcontainer up --remove-existing-container` |
| Dev Container: Open Shell in Container | `devcontainer exec … bash -l` (or `sh -l`) |
| Dev Container: Stop Container | `docker stop` on the containers labelled with the project folder |

The plugin uses `.devcontainer/devcontainer.json`, or else `.devcontainer.json`.
For a configuration in `.devcontainer/<folder>/`, set Settings › Plugins ›
Dev Container CLI › Configuration file. New › Dev Container Configuration adds a
starter `devcontainer.json`; New › Dev Container (Node, Python, Java, PHP, Rust, Docker Compose)
adds one based on the matching containers.dev reference image.

Requires the devcontainer CLI (`npm i -g @devcontainers/cli`) and Docker. The IDE
keeps running on the host: OpenIDEA's remote development connects over SSH or
WSL, not into a container.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
