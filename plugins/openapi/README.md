# OpenAPI

Editing support for OpenAPI 3 and Swagger 2 files in YAML and JSON, with a
Swagger UI preview. A file is an OpenAPI document when it has a top-level `openapi` or
`swagger` key with a value. Every other YAML or JSON file gets no completion, no
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
alone. When the value has no `#` yet, the YAML, YML and JSON files of the
project are offered as relative paths (`./schemas/pet.yaml`, `../common/pet.json`).
That list is read when a YAML or JSON file is opened or saved (at most 500 files
per extension, hidden and ignored paths excluded by the host), so a file created
since then shows up after the next open or save.

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

A YAML or JSON file that has a root `paths` object of `/...` items with HTTP methods
but no `openapi` or `swagger` key (a spec fragment) gets an info hint,
`missing-openapi-root`, on `paths`. The setting `checkSpecCandidates` switches it
off, and files over 1 MiB are skipped.

A `$ref` into a file that exists but can't be read (a directory, too large, not
UTF-8) is not flagged. JSON files are checked like YAML ones, except a JSON file whose root
is an array. Path parameters are skipped when the parameter list can't be
read reliably: flow style, a `$ref` that isn't local, or a local `$ref` that
can't be resolved or lacks `name` or `in`.

## Schema modeline

When the file has no `# yaml-language-server: $schema=...` line, the code action
"Add the OpenAPI schema for the YAML language server" inserts it at the top, for
the official schema of the file's version (2.0, 3.0, 3.1 or 3.2). The built-in
YAML language server then validates the file.

## Endpoints view

The OpenAPI Endpoints view (right side) lists `METHOD /path` for every OpenAPI
YAML or JSON file (up to 1 MiB) of the current window's project, with the operationId and file name as
detail. Activating an entry opens the operation. Refresh re-scans the project.

## Preview

Swagger UI 5.34.1 beside the text. It opens automatically for `openapi.*`,
`swagger.*`, `*.openapi.*` and `*.swagger.*` with a `.yaml`, `.yml` or `.json`
extension. For any other YAML or JSON file, use Open With and pick API Preview.
The preview follows edits as you type. A Reload button in the preview asks for the
referenced files again (not verified in a live IDE).

- Relative `$ref`s to other files: implemented, not yet verified in a live IDE
  (an `openidea-plugin link` check is pending). The plugin reads the files the
  open document references and hands them to the page, which serves them to
  Swagger UI. Only one level is loaded (files referenced from those files are
  not), at most 50 files and 512 KiB in total; larger ones are skipped. The
  files are re-sent only when they changed.
- A Redoc button switches the preview to Redoc 2.5.4 (and back). Redoc loads on first use and has its search
  turned off (it needs a worker). Not verified in a live IDE.
- There is no Try it out: the frame has no network.
- The dark theme is a colour inversion of the light one.

## Write Requests for the HTTP Client

The command "OpenAPI: Write Requests for the HTTP Client" writes one request per
operation of the focused file to `<name>.http` beside it and opens it (it asks before
replacing an existing file). Writing needs the optional `tee` permission; without it the
requests are copied to the clipboard. The base URL comes from
`servers[0].url` (OpenAPI 3) or `schemes`, `host` and `basePath` (Swagger 2), and
defaults to `http://localhost`. Path parameters become `{{name}}` variables.
Without both permissions the plugin shows a warning instead.

## Live templates

In an OpenAPI or Swagger file, completion offers four snippets: `oas3` (an OpenAPI 3.1 root with
info and a GET path), `opnp` (a path item), `endp` (an operation with parameters and a response)
and `swag` (a Swagger 2.0 root). They are plain completion items, offered anywhere in the file, and
the tab stops are not verified in a live IDE.

## Keyword and enum completion

In an OpenAPI 3 file (YAML in block style, or JSON), completion on a blank or partly typed key offers the keys of the
object at that indentation (`info`, `paths`, a path item's methods, an operation's `summary`, `parameters`,
`responses` with common status codes, schema keywords, and so on), taken from a fixed tree of the OpenAPI
Specification. After `key: ` it offers the enum values of `in`, `style`, `type`, `format`, `scheme`, the
booleans, and so on (`null` as a type only for 3.1). It is quiet in Swagger 2, flow style, inside a
multi-line scalar, and under keys it doesn't know (such as `x-` extensions, `example`, `callbacks`).
Keys already present in the object are offered again. The skeletons stay available next to the keys.
In JSON the keys come from the objects around the caret (arrays are transparent); typing inside quotes
completes the name only, outside quotes it inserts `"name": `, and enum values are inserted quoted.

## Extract, bundle and copy

- Code action "Extract inline schema to components/schemas", with the caret on the line of a `schema:`,
  `items:`, `additionalProperties:` or a property name whose value is an inline block mapping (or a JSON
  object). The block moves to `components.schemas` (created if missing, with the comma handled in JSON) under
  a generated name (`ListPetsResponse`, `Owner`; a number is appended when taken) and is replaced by a
  `$ref`. Rename the new schema afterwards.
- "OpenAPI: Write Self-Contained Spec" writes `<name>.bundled.<ext>` beside the file (clipboard without the
  optional `tee` permission) with every `$ref` to another file replaced by a pointer
  into `components` and the referenced objects copied there (including what they reference, recursively;
  a recursive reference is copied once). A YAML spec bundles YAML files (target a block mapping), a JSON spec
  bundles JSON files; mixing them is refused. An object that is not under `/components/<section>/` lands in `schemas`.
- "OpenAPI: Copy Reference to the Key at the Caret" copies `$ref: '#/components/schemas/Pet'` (or the JSON form)
  for the key on the caret's line.
- "OpenAPI: Compare With Last Revision" compares the open file with its version in `HEAD` in the
  OpenAPI Changes view. It needs the optional `git` permission and a committed file.

## Markers, icons and detection

Every operation whose body has a local `$ref` that resolves gets a gutter marker that opens the first such
target. `openapi.*` and `swagger.*` files (and `*.openapi.*`, `*.swagger.*`) get the OpenAPI icon. The setting
`specGlobs` (comma-separated, for example `api/*.yaml, *.contract.json`; a pattern without `/` matches the file
name, with `/` the end of the path) makes completion, go to definition, problems and the endpoints view work
in more files, including fragments without an `openapi` key (in a fragment, unresolved local `#/` pointers
are not reported, since they may point into the root file, and the "missing root key" hint is off). The
preview is not affected: its file patterns are fixed in the manifest, so use Open With and pick API Preview
for other names. The icon and
marker behaviour is not verified in a live IDE.

## Templates

New › Swagger 2.0 (YAML) and Swagger 2.0 (JSON) create a minimal Swagger 2.0 document too. New › OpenAPI 3.1 (YAML), OpenAPI 3.1 (JSON) and AsyncAPI 3.0 (YAML) create a minimal valid document:
`info` with title (`$NAME$`) and version, and for OpenAPI a `GET /health` with a 200 response; for AsyncAPI
one channel, one operation and one message. AsyncAPI files are not OpenAPI documents, so the plugin's checks and
preview leave them alone.

## Licences

The plugin code is MIT. It bundles Swagger UI (Apache-2.0) and Redoc (MIT), whose licence and
notice files are in `web/vendor/`; see `NOTICE.md`.

## Limits

- JSON is read by a small scanner: string escapes other than `\"`, `\/` and
  `\\` in keys are not decoded, and a document that stops being valid JSON is
  checked only up to that point.
- Flow-style mappings are only seen for `$ref`.
- Problems appear on open and save, not while typing.
- Problems caused by another file's content refresh only when the spec itself
  is opened or saved.
- A component name containing dots can make a shorter wrong pointer resolve
  (`#/components/schemas/com` when `com.acme.Error` exists).
- No rename, find usages, hover or structure view.
- The endpoints view re-scans the project on each render.
- The schema modeline is YAML only; JSON specs get no schema validation. Keyword completion is OpenAPI 3 only.
- The endpoints view reads a JSON file in full before the 1 MiB cap applies.
- The preview isn't opened automatically for other file names, and loads only
  the files the open document references directly.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
