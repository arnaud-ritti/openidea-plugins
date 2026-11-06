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

The plugin uses `.devcontainer/devcontainer.json`, or else `.devcontainer.json`.
For a configuration in `.devcontainer/<folder>/`, set Settings › Plugins ›
Dev Container CLI › Configuration file. New › Dev Container Configuration adds a
starter `devcontainer.json`; New › Dev Container (Node, Python, Java, PHP, Rust, Docker Compose)
adds one based on the matching containers.dev reference image.

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
`shutdownAction`, `userEnvProbe`, `waitFor` and the boolean properties. Nested objects, arrays and feature
ids are not completed. A setting switches it off. The provider is declared for the `json` and `jsonc`
language ids; `jsonc` is not verified in a live IDE.

Requires the devcontainer CLI (`npm i -g @devcontainers/cli`) and Docker. The IDE
keeps running on the host: OpenIDEA's remote development connects over SSH or
WSL, not into a container.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
