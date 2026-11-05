# OpenAPI

Editing support for OpenAPI 3 and Swagger 2 files in YAML, with a Swagger UI
preview. A file is an OpenAPI document when it has a top-level `openapi` or
`swagger` key with a value. Every other YAML file gets no completion, no
actions and no problems, and any problems left from an earlier edit are cleared
when it is opened or saved.

This is an independent implementation, not affiliated with JetBrains or
SmartBear.

## `$ref` completion and definition

Inside a `$ref` value, completion offers the reusable components of the document
(`#/components/schemas/Pet`, or `#/definitions/Pet` in Swagger 2); in a file
without an `openapi`/`swagger` key, its top-level keys. A `$ref` still being
typed completes, with or without a closing quote, and only the value is
replaced. Slashes and tildes are escaped (`a/b` becomes `a~1b`). Go to
Definition also resolves pointers into `paths`
(`#/paths/~1pets/get`), and decodes `%7B`-style escapes.

For a relative file (`../common.yaml#/Pet`) the pointers of that file are
offered, resolved against the spec's directory. `https://` references are left
alone. File names aren't completed.

Go to Definition on a `$ref` opens the target, in the same file or the
referenced one.

## Problems

Checked when a file is opened or saved.

| Code | Severity | Message |
|---|---|---|
| `unresolved-ref` | error | ``Cannot find `<ref>` `` for a pointer that doesn't exist, ``Cannot find file `<file>` `` for a missing file |
| `duplicate-operation-id` | error | ``operationId `<id>` is used by more than one operation`` |
| `path-parameter` | error | ``Path parameter `<name>` is not declared for <METHOD> <path>`` |
| `path-parameter` | warning | ``Path parameter `<name>` is not in the path <path>`` |

A `$ref` into a file that exists but can't be read (a directory, too large, not
UTF-8) is not flagged. Pointers into `.json` files aren't checked, only the
file's existence. Path parameters are skipped when the parameter list can't be
read reliably: flow style, a `$ref` that isn't local, or a local `$ref` that
can't be resolved or lacks `name` or `in`.

## Schema modeline

When the file has no `# yaml-language-server: $schema=...` line, the code action
"Add the OpenAPI schema for the YAML language server" inserts it at the top, for
the official schema of the file's version (2.0, 3.0, 3.1 or 3.2). The built-in
YAML language server then validates the file.

## Endpoints view

The OpenAPI Endpoints view (right side) lists `METHOD /path` for every OpenAPI
YAML file of the current window's project, with the operationId and file name as
detail. Activating an entry opens the operation. Refresh re-scans the project.

## Preview

Swagger UI 5.34.1 beside the text. It opens automatically for `openapi.*`,
`swagger.*`, `*.openapi.*` and `*.swagger.*` with a `.yaml`, `.yml` or `.json`
extension. For any other YAML or JSON file, use Open With and pick API Preview.
The preview follows edits as you type.

- There is no Try it out: the frame has no network.
- The dark theme is a colour inversion of the light one.
- Only the open file is shown; references to other files aren't loaded.

## Copy Requests for the HTTP Client

The command "OpenAPI: Copy Requests for the HTTP Client" copies one request per
operation of the focused file, ready for a `.http` file. The base URL comes from
`servers[0].url` (OpenAPI 3) or `schemes`, `host` and `basePath` (Swagger 2), and
defaults to `http://localhost`. Path parameters become `{{name}}` variables.
Without the clipboard permission the plugin shows a warning instead.

## Licences

The plugin code is MIT. It bundles Swagger UI (Apache-2.0), whose licence and
notice files are in `web/vendor/`; see `NOTICE.md`.

## Limits

- Editing features cover YAML only. JSON files get the preview but no
  completion, definition or problems.
- Flow-style mappings are only seen for `$ref`.
- Problems appear on open and save, not while typing.
- Problems caused by another file's content refresh only when the spec itself
  is opened or saved.
- A component name containing dots can make a shorter wrong pointer resolve
  (`#/components/schemas/com` when `com.acme.Error` exists).
- No rename, find usages, hover or structure view.
- The endpoints view re-scans the project on each render.
- The schema modeline is YAML only; JSON specs get no schema validation.
- The preview isn't opened automatically for other file names, and shows a
  single file.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
