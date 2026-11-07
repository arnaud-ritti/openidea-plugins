# Dev Container CLI

Runs the project's [Dev Container](https://containers.dev) with the
[devcontainer CLI](https://github.com/devcontainers/cli):

| Command | Runs |
|---|---|
| Dev Container: Start Container | `devcontainer up` in a terminal |
| Dev Container: Rebuild Container | `devcontainer up --remove-existing-container` |
| Dev Container: Open Shell in Container | `devcontainer exec … bash -l` (or `sh -l`) |
| Dev Container: Show Configuration | `devcontainer read-configuration`, summarised in a notification (image or compose file, remote user, forwarded ports, feature count, `postCreateCommand`) |
| Dev Container: Stop Container | `docker stop` on the containers labelled with the project folder |
| Dev Container: Refresh Feature List | Downloads the feature index from containers.dev into the plugin's storage (see Completion) |

The plugin uses `.devcontainer/devcontainer.json`, or else `.devcontainer.json`.
For a configuration in `.devcontainer/<folder>/`, set Settings › Plugins ›
Dev Container CLI › Configuration file. New › Dev Container Configuration adds a
starter `devcontainer.json`; New › Dev Container (Node, Python, Java, PHP, Rust, Docker Compose)
adds one based on the matching containers.dev reference image.

## Dev Containers view

The bottom panel lists the project's containers, running and stopped (`docker ps -a` filtered by the
`devcontainer.local_folder` label the CLI sets, so only containers started from this folder show). Select a
row for Open Shell (`docker exec -it`) or Stop on a running container, or Start (`docker start`) on a stopped one;
double-click opens a shell, or starts a stopped container. Needs `process:docker`. The label
is taken from the CLI's behaviour and has not been checked against every CLI version. A gutter marker on
`image` that starts the container is not offered: a gutter marker can open a location but not run a command.

## Inspections

Run when `.devcontainer/devcontainer.json`, `.devcontainer/<folder>/devcontainer.json`
or `.devcontainer.json` is opened or saved (comments and trailing commas are
read as JSONC; files above 256 KB are skipped). Each group has a setting.

- `unknown-property` (Check properties): a top-level key that is not in the
  [specification](https://containers.dev/implementors/json_reference) but is within edit distance 2 of
  one, as a likely typo (`postCreateCommnad`). Other unknown keys, `x-*`, `$`-prefixed keys and
  everything under `customizations` stay quiet, since the property list may be incomplete.
- `features-malformed` (Check features): `features` that is not an object, a feature id that is empty or
  contains whitespace, or a feature value that is not an options object, a version string or a boolean.
- `folder-structure` (Check folder structure, information): a `devcontainer.json` that is not at
  `.devcontainer/`, `.devcontainer/<folder>/` or `.devcontainer.json`. It looks at the file's path only.

## Completion

In a `devcontainer.json`, completion offers the top-level properties not yet present and the values of
`shutdownAction`, `userEnvProbe`, `waitFor` and the boolean properties. Nested objects and arrays are not
completed. Keys of the `features` object are completed with feature ids (`ghcr.io/devcontainers/features/node:1`)
from the public index at `containers.dev/static/devcontainer-index.json`. The index is downloaded only by
Dev Container: Refresh Feature List and kept in storage, so completion is empty until you run it once and
may lag behind the registry. It needs the optional `storage` and `net:containers.dev` permissions. The index
layout was read from the live file; the download itself is not verified in a live IDE. A setting switches it off. The provider is declared for the `json` and `jsonc`
language ids; `jsonc` is not verified in a live IDE.

Requires the devcontainer CLI (`npm i -g @devcontainers/cli`) and Docker. The IDE
keeps running on the host: OpenIDEA's remote development connects over SSH or
WSL, not into a container.

## Permissions

| Permission | Used for |
|---|---|
| `process:devcontainer` (required) | Run the devcontainer CLI: up, rebuild, exec (shell in the container) |
| `process:docker` (optional) | List and stop containers labelled with the project folder (`docker ps`, `docker stop`) and open a shell in one (`docker exec`). Without it, Stop Container and the view are unavailable |
| `storage`, `net:containers.dev` (optional) | Keep the feature id list and refresh it from containers.dev |
| `project.read` (required) | Find and read `devcontainer.json` |
| `events` (optional) | Check `devcontainer.json` when opened or saved |

Programs are started with separate arguments, never through a shell line.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
