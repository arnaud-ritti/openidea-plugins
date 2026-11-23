# OpenAPI

Edit OpenAPI 3 and Swagger 2 files in YAML and JSON: `$ref` completion and navigation, problems, an endpoints list, a Swagger UI preview, HTTP requests, spec comparison and code generation. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- Optional tools:
  - Git, for **OpenAPI: Compare With Last Revision**.
  - `swagger-codegen` 3.x (stable), installed by you, for code generation.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a file with a top-level `openapi` or `swagger` key. Or create one with New › **OpenAPI 3.1 (YAML)**.
3. Files named `openapi.*`, `swagger.*`, `*.openapi.*` or `*.swagger.*` open with the Swagger UI preview beside the text.
4. Type inside a `$ref` value to complete component names. Use Go to Definition to jump to the target.
5. Open the **OpenAPI Endpoints** window (right) to see every endpoint in the project.

## Features

### Which files count

A YAML or JSON file is an OpenAPI document when it has a top-level `openapi` or `swagger` key with a value. Other files get no completion, actions or problems.

To include more files, such as fragments without an `openapi` key, set **Extra file name globs…** (`specGlobs`). Example: `api/*.yaml, *.contract.json`. A pattern without `/` matches the file name; with `/` it matches the end of the path.

### Complete and follow `$ref`

- Inside a `$ref`, completion offers the document's components (`#/components/schemas/Pet`, or `#/definitions/Pet` in Swagger 2).
- For a relative file (`../common.yaml#/Pet`), it offers that file's pointers. Before you type `#`, it offers the project's YAML, YML and JSON files.
- **Go to Definition** on a `$ref` opens the target, in the same file or another one. It also resolves pointers into `paths` (`#/paths/~1pets/get`).
- Slashes and tildes in names are escaped for you (`a/b` becomes `a~1b`).
- `https://` references are left alone.

### Complete keys, values and snippets

- In OpenAPI 3 (block-style YAML or JSON), completion offers the keys allowed at that spot: `info`, `paths`, HTTP methods, `summary`, `parameters`, `responses` with common status codes, schema keywords and more.
- After `key: ` it offers enum values for `in`, `style`, `type`, `format`, `scheme`, booleans and so on. `null` as a type is offered for 3.1 only.
- Snippets anywhere in an OpenAPI or Swagger file: `oas3` (OpenAPI 3.1 root), `opnp` (path item), `endp` (operation), `swag` (Swagger 2.0 root).

### See problems

Checked when you open or save the file:

| Code | Severity | Message |
|---|---|---|
| `unresolved-ref` | error | ``Cannot find `<ref>` `` for a pointer that doesn't exist; ``Cannot find file `<file>` `` for a missing file |
| `duplicate-operation-id` | error | ``operationId `<id>` is used by more than one operation`` |
| `path-parameter` | error | ``Path parameter `<name>` is not declared for <METHOD> <path>`` |
| `path-parameter` | warning | ``Path parameter `<name>` is not in the path <path>`` |
| `missing-openapi-root` | info | On `paths` in a YAML/JSON file that looks like a spec but has no `openapi` or `swagger` key. Turn off with **Hint at a YAML or JSON file…**. |

JSON files also get live structure checks as you type (`openapi-structure-*` codes) for Swagger 2.0, OpenAPI 3.0 and 3.1.

### Hover

Hover on a component, an operation, or a local `$ref` to a schema shows its title, summary, description, type/format, operationId and deprecated flag, as written in the document.

### Validate with the official schema (YAML)

Press ⌥⏎ and choose **Add the OpenAPI schema for the YAML language server**. It adds a `# yaml-language-server: $schema=...` line for your file's version (2.0, 3.0, 3.1 or 3.2). The built-in YAML language server then validates the file.

### Quick fixes

Press ⌥⏎ to add missing structure:

- On the root `openapi`/`swagger` key: add missing `info`, `paths` and (OpenAPI 3) `components`.
- On `paths`: **Add path**.
- On a path item or operation: **Add query parameter**.
- On a path key: **Add get/post/put/delete operation**.
- On an operation's `responses` key: **Add 200/400/404/default response**.
- On a `schema:`, `items:`, `additionalProperties:` or property line with an inline schema: **Extract inline schema to components/schemas**. The schema moves under a generated name (`ListPetsResponse`, `Owner`) and is replaced by a `$ref`.

### Browse endpoints

The **OpenAPI Endpoints** window lists `METHOD /path` for every OpenAPI file in the project, with the operationId and file name. Click a row to open the operation.

- **Filter** narrows the list to one tag. **Clear** shows all again.
- **Flat/Tree** groups the rows by tag.
- **Refresh** rescans the project.
- Row actions: **Copy as curl** and **Create HTTP Request File** (writes `http/<verb>-<path>.http` beside the spec).

Search Everywhere (⇧⇧) has an **OpenAPI Operations** tab: type a method, path, tag or operationId (`listPets`, `get /pets`) to find an operation in any spec of the project and open it. An exact operationId comes first. The first query in a large project may list only part of the operations; **Show more** reads the rest.

### Jump between operations

An **Operations** panel sits beside the editor of an OpenAPI YAML or JSON file. It lists the file's operations (`METHOD /path`) grouped by tag.

- Type in the filter to match a method, path, tag or operationId. Escape clears it.
- Click an operation, or press Enter on it, to move the caret there. Arrow keys move between rows.
- The list follows your edits after a short pause.
- The operation holding the caret is highlighted, and a dot marks operations with errors (red) or warnings (yellow).

### Preview the API

Swagger UI 5.34.1 opens beside the text for `openapi.*`, `swagger.*`, `*.openapi.*` and `*.swagger.*` files (`.yaml`, `.yml`, `.json`). For other files, use Open With › **API Preview**.

- The preview follows your edits as you type. **Reload** fetches referenced files again.
- Files the spec references directly are loaded too.
- The **Redoc** button switches to Redoc 2.5.4 and back. The preview remembers your choice (with the `storage` permission).
- **Try…** starts **OpenAPI: Try Local GET/HEAD Request…** for the spec shown. The preview itself has no network: the request is prepared in the **Request / Response** tab and sent only when you click **Send once**.

### Write HTTP requests

- **OpenAPI: Write Requests for the HTTP Client** writes one request per operation to `<name>.http` beside the spec and opens it. It asks before replacing a file. The base URL comes from `servers[0].url` (OpenAPI 3) or `schemes`, `host` and `basePath` (Swagger 2), default `http://localhost`. Path parameters become `{{name}}` variables.
- **OpenAPI: Copy as cURL** and **OpenAPI: Create HTTP Request File** do the same for one endpoint: the selected row in **OpenAPI Endpoints**, or one you pick from the project when run from Search Everywhere. They ask for the base URL, suggesting the spec's server the same way.

### Try a local request

**OpenAPI: Try Local GET/HEAD Request…** prepares a GET or HEAD request from an OpenAPI 3.0/3.1 spec in JSON or YAML. Run it from the Command Palette or the preview's **Try…** button, or hover the left edge of a `get:` or `head:` line and click **Try GET /path…** to skip choosing the operation. It shows in the **Request / Response** tab. Nothing is sent until you click **Send once**.

It needs one literal `127.0.0.1` server with a port, and `net:127.0.0.1`. With `net:ask`, a single literal `https://host` server works too; the IDE asks before the first request to each host.

### Bundle, copy and find

- **OpenAPI: Write Self-Contained Spec** writes `<name>.bundled.<ext>` with every external `$ref` copied into `components`.
- **OpenAPI: Copy Reference to the Key at the Caret** copies `$ref: '#/components/schemas/Pet'` (or the JSON form) for the key under the caret.
- **OpenAPI: Find Component Occurrences in This Document** lists the declaration and local references of a component. The Referenced by gutter icon (the OpenAPI Initiative logo) on a used component declaration opens the same local references.
- **OpenAPI: Find Component Usages in Project** does the same across local and relative-file `$ref`s.
- On a component key or `$ref`, ⌥⏎ › **Rename component across the project…** renames it everywhere after you review the edit.

### Compare specs

- **OpenAPI: Compare With Another Spec…** lists added, removed and changed operations, schemas and sections in the **Changes** tab.
- **OpenAPI: Compare With Last Revision** compares the file with its committed `HEAD` version.
- **OpenAPI: Export Comparison as Markdown** saves the comparison to `<spec>.comparison.md`.
- **OpenAPI: Breaking-Change Report Against Another Spec…** writes a Markdown report that marks each change as breaking, non-breaking or info.

### Draft a spec from your code

**OpenAPI: Draft Spec from Source Mappings…** scans Java/Kotlin controllers (Spring MVC, JAX-RS/Quarkus, Micronaut). After you review it, it writes a new `openapi-draft.yaml` with paths, parameters, request bodies, response codes, operationIds and schemas. What it couldn't read is listed as TODO at the top.

### Generate code

**OpenAPI: Generate Client, Server or Documentation…** runs your installed `swagger-codegen` 3.x:

1. Pick a generator from the installed list and an output folder.
2. Review the plan. Generation runs in a staging folder (`.openidea/codegen-stage-*`) in a terminal.
3. When it finishes, a notification says so. Press ⌥⏎ in the spec › **Apply generated code…** to move it into place. One Undo reverses the move.

**OpenAPI: Generate With Options or Templates…** adds generator options, a custom template folder, a template engine and an optional JSON config file. **OpenAPI: Stop Tracking Generated Code** forgets a finished generation you don't want to apply, so you can start another; its staging files stay.

Input must be Swagger 2.0 or OpenAPI 3.0.

### File templates

New › **OpenAPI 3.1 (YAML)**, **OpenAPI 3.1 (JSON)**, **Swagger 2.0 (YAML)**, **Swagger 2.0 (JSON)** and **AsyncAPI 3.0 (YAML)** create a minimal valid document. OpenAPI templates include a `GET /health`. AsyncAPI files aren't treated as OpenAPI, so checks and preview skip them.

OpenAPI files get an OpenAPI file icon.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **OpenAPI**; bind keys in Settings › Keymap. None has a default shortcut. Commands that work on the open spec are enabled only while an OpenAPI spec is focused.

| Command | What it does |
|---|---|
| **OpenAPI: Write Requests for the HTTP Client** | Writes a `.http` file with one request per operation. |
| **OpenAPI: Copy as cURL** | Copies a curl command for the selected endpoint, or one you pick. |
| **OpenAPI: Create HTTP Request File** | Writes a `.http` file for the selected endpoint, or one you pick. |
| **OpenAPI: Try Local GET/HEAD Request…** | Prepares a local GET/HEAD request to review and send once. |
| **OpenAPI: Write Self-Contained Spec** | Writes a bundled copy with external `$ref`s inlined into `components`. |
| **OpenAPI: Copy Reference to the Key at the Caret** | Copies a `$ref` to the key under the caret. |
| **OpenAPI: Find Component Occurrences in This Document** | Lists a component's declaration and references in this file. |
| **OpenAPI: Find Component Usages in Project** | Lists a component's references across the project. |
| **OpenAPI: Compare With Another Spec…** | Shows the differences with another spec. |
| **OpenAPI: Compare With Last Revision** | Shows the differences with the committed version. |
| **OpenAPI: Export Comparison as Markdown** | Saves the last comparison as Markdown. |
| **OpenAPI: Breaking-Change Report Against Another Spec…** | Writes a breaking-change report against an older spec. |
| **OpenAPI: Draft Spec from Source Mappings…** | Writes `openapi-draft.yaml` from your controllers. |
| **OpenAPI: Generate Client, Server or Documentation…** | Runs `swagger-codegen` into a staging folder. |
| **OpenAPI: Generate With Options or Templates…** | Same, with generator options and custom templates. |
| **OpenAPI: Stop Tracking Generated Code** | Forgets a finished generation so you can start another. Files stay. |

## Settings

Settings › Plugins › OpenAPI.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Extra OpenAPI file globs (comma-separated) | `specGlobs` | empty | Adds more files to the editor features and the endpoints list. Not to the preview. |
| Hint at a YAML or JSON file with an OpenAPI paths object but no openapi or swagger key | `checkSpecCandidates` | on | Shows the `missing-openapi-root` hint. |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › OpenAPI.

| Permission | Used for | Without it |
|---|---|---|
| `project.write` | Writing `.http`, bundled, comparison and report files, the draft spec and generated code. | Text-producing commands copy to the clipboard instead. Generation needs it. |
| `clipboard` | Copy commands, and the fallback when writing is denied. | A warning asks you to allow writing or the clipboard. |
| `process:git` | **OpenAPI: Compare With Last Revision**. | A warning asks you to allow git. |
| `storage` | Remembers the endpoints list's Flat/Tree mode and tag filter, and the preview's Swagger UI or Redoc choice. | The list and the preview reset each session. |
| `net:127.0.0.1` | Sending a **Try Local GET/HEAD Request…**. | The request isn't sent. |
| `net:ask` | Try requests to a single `https://` server; the IDE asks per host. | Only `127.0.0.1` servers work. |
| `process:swagger-codegen` | Code generation. | Generation commands say which permission to allow. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| No completion or problems in a YAML/JSON file | Add a top-level `openapi` or `swagger` key, or add the file to **Extra file name globs…**. |
| A problem stays after fixing another file | Problems refresh when the spec itself is opened or saved. Save it again. |
| A new file isn't offered in `$ref` completion | The file list updates on the next open or save of a YAML/JSON file. |
| The preview doesn't open | Only fixed names open it automatically. Use Open With › **API Preview**. |
| "Writing the file needs the project.write permission. Allow it in Settings › Plugins › OpenAPI." | Allow `project.write` (to write files) or `clipboard` (to copy). |
| "Comparing with the last revision needs the process:git permission. Allow it in Settings › Plugins › OpenAPI." | Allow `process:git`. |
| "… has no committed version in HEAD to compare with." | Commit the file first. |
| "No other OpenAPI file in this project to compare with." | Add the other spec to the project. |
| "Request denied by the host: it needs the net:127.0.0.1 permission (net:ask for other hosts)…" | Allow `net:127.0.0.1` (or `net:ask` for https), then prepare the request again. |
| "Only literal 127.0.0.1 with an explicit port, or an https host when net:ask is granted, is supported." | Make `servers` a single literal URL such as `http://127.0.0.1:8080`. |
| "Generated code is waiting to be applied…" | Apply it with ⌥⏎ › **Apply generated code…** in the spec, or run **OpenAPI: Stop Tracking Generated Code**. |
| "No exportable Spring, JAX-RS or Micronaut mappings found…" | The draft found no controllers it can read. |

## Limitations

- Problems appear on open and save, not while typing (except JSON structure checks).
- Keyword completion is OpenAPI 3 only. The schema modeline is YAML only. JSON structure checks skip OpenAPI 3.2.
- Flow-style YAML mappings are only read for `$ref`.
- A component name with dots can make a shorter wrong pointer resolve (`#/components/schemas/com` when `com.acme.Error` exists).
- No indexed Rename or Find Usages, and no structure view.
- The preview loads only files the spec references directly. Its dark theme is a colour inversion of the light one.
- The endpoints list has no request panel and no gutter icon on the handler. The grey detail text follows the path instead of sitting in a column.
- Try requests: no authentication, header/cookie parameters, request bodies or Swagger 2. YAML with anchors, aliases, tags or merge keys is refused.
- Code generation doesn't accept OpenAPI 3.1/3.2 and never installs tools.

## Technical details

<details>
<summary>Parsing rules, limits and safety checks</summary>

### File detection

- Problems left from an earlier edit are cleared when a non-OpenAPI file is opened or saved.
- In a `specGlobs` fragment, unresolved local `#/` pointers aren't reported (they may point into the root file) and the missing-root hint is off.
- The preview patterns are fixed in the manifest.
- Required permissions: `project.read`, `editor.read`, `events`.

### `$ref` completion and navigation

- In a file without an `openapi`/`swagger` key, completion offers its top-level keys. A `$ref` still being typed completes with or without a closing quote; only the value is replaced.
- Canonical `~0`/`~1` object-key pointers are supported. Percent-encoded and array target pointers stay quiet.
- Relative-file pointers resolve against the spec's directory. Targets must be physical siblings in the same innermost open project (unsaved target text wins when supported), within the 1 MiB scan bound. Absolute/URI/percent/query aliases, unavailable or foreign-project targets and documents with `$id` rebasing are withheld.
- Project file suggestions are read when a YAML or JSON file is opened or saved: at most 500 files per extension, hidden and ignored paths excluded by the host.
- JSON pointer definitions need a unique object-key chain in valid target JSON (512 KiB, 8,192 nodes, 48 levels). Duplicate/dotted impostors, encoded fragments, malformed tilde escapes, array-owned pointers and `$id` rebasing are withheld.
- YAML pointer definitions validate one bounded document (same limits) and prove each unique string-key owner against its exact source span. Plain/quoted block keys with canonical escaping work. Duplicates, aliases/anchors/tags, merge keys, malformed/multiple documents, transformed/complex keys, percent fragments and arrays stay quiet. Literal tilde/slash names stay navigable.
- Definition selects actual literal `$ref` fields (including supported array entries and singleton YAML flow maps). Description text and dotted-key impostors don't count.
- Completion accepts ordinary incomplete JSON/YAML with partial typing hints. Models that exhaust the scan budgets stay unknown.

### Problems

- Files over 1 MiB are skipped by the missing-root hint. A `$ref` into a file that exists but can't be read (a directory, too large, not UTF-8) isn't flagged.
- JSON files are checked like YAML, except a JSON file whose root is an array.
- Path parameters are skipped when the list can't be read reliably: flow style, a non-local `$ref`, or a local `$ref` that can't be resolved or lacks `name` or `in`.
- The reference checker distinguishes Present, Missing and Unknown pointers. Valid missing targets keep their diagnostic and proved missing files keep the missing-file error. Ambiguous duplicate/flattened owners, malformed or rebased models, unsupported encoded/array/transformed/flow forms and invalid pointer syntax stay quiet.
- Each check processes at most 1,024 refs, 32 unique file reads, 4 MiB of captured target text, a 16 MiB retained-model limit, 512 KiB / 8,192-node / 48-depth proofs and 4 MiB of flattened-path storage. Skipped, over-budget or unreadable targets are Unknown.
- Path-parameter checks use the same source proof: real parameter arrays, owned name/in/reference fields, canonical local reusable parameter references, operation-level `(name, in)` overriding path-item entries, lists up to 1,024 items and fields up to 4 KiB.
- Duplicate-operationId diagnostics count only real operation owners and owned string fields, up to 4 KiB IDs and 256 diagnostics, with exact UTF-16 ranges.
- Every operation whose body has a resolvable local `$ref` gets a gutter marker (packaged icon). One target opens directly; several (request body, responses...) open a chooser listing each one. Markers need a real operation owner and an exact source-backed target; a narrow singleton YAML flow reference is supported.
- A component declaration (under `components/schemas`, `responses`, `parameters`...) that is used by local `$ref`s in the same document gets a Referenced by marker; it opens the use, or a chooser when there are several. Other files are not searched; use **OpenAPI: Find Component Usages in Project** for that.
- At most 256 marked lines and 200 targets per line; a longer list says how many were left out.

### Live JSON structure diagnostics

- Check the unsaved JSON document without project access, discovery, network or external reads (API 1.3 diagnostics). They don't duplicate the open/save checks. Malformed JSON, unrelated documents, ambiguous root version fields and unsupported versions get none; generic JSON syntax stays with the JSON language server.
- Recognizes `swagger: "2.0"`, `openapi: "3.0.<patch>"` and `openapi: "3.1.<patch>"`.
- Validates: root info/title/version; required paths in 2.0/3.0 and at least one of paths/components/webhooks in 3.1; path/operation shapes, operationId strings, response presence and descriptions, content media objects and request-body content/required fields; parameters (shape, `name`/`in`, version-specific locations, required path parameters, boolean flags, schema/content conflicts); reusable responses/parameters (2 and 3), request bodies and headers (3) like inline objects; server objects (required URL and variable defaults, string enums; 3.1 also nonempty choices/default membership; 3.0 SHOULD advice not reported); schema shapes, supported type forms, array items in 2.0/3.0, properties, selected composition and schema keywords (required name arrays, enum arrays, numeric/size bounds, scalar annotations, version-specific exclusive bounds; boolean schemas and type unions/null in 3.1).
- Reference targets and schema-reference siblings aren't interpreted. Custom 3.1 JSON Schema dialects and unknown `jsonSchemaDialect`/local `$schema` skip the schema checks. A clean result means only that this subset found no issue.
- Limits: 512 KiB, 8,192 nodes, depth 48, 256 diagnostics; larger documents are skipped. Locations use tree-owned source-key spans with UTF-16 conversion. Missing fields and ambiguous flattened paths use the nearest unambiguous parent key; ambiguous dotted locations stay quiet.

### Hover

- JSON: component declarations and operation method/operationId/summary fields show title, summary, description, scalar type/format, operation id, deprecated flag and boolean schema value from the current unsaved document. Local fragment references and nested pointers through `properties`, `items`, `additionalProperties` and `not` show the target's facts. Swagger definitions and 3.1 boolean schemas follow their version.
- Quiet for duplicate roots, duplicate ancestor keys, malformed/percent-encoded pointers, flattened-key impostors, array-index paths, extension-object targets, external references and unknown versions.
- YAML: a literal local `$ref` to a Swagger 2.0 definition or OpenAPI 3.0/3.1 component schema, with the same ownership proof for source and target, canonical pointers and exact UTF-16 ranges. Escaped names and 3.1 boolean schemas work. Aliases/tags/anchors/merges, base-URI declarations, duplicate or flattened owners, transformed scalars, unsupported flow spans and encoded pointers stay quiet.
- Only source declarations are reported; reference chains, merged semantics and JSON Schema evaluation aren't inferred.
- Limits: 512 KiB source, 8,192 nodes/keys, depth 48, 8 KiB output, bounded strings. Descriptions show as escaped text, not Markdown/HTML.

### Completion details

- Keyword completion is quiet in Swagger 2, flow style, inside a multi-line scalar and under unknown keys (`x-` extensions, `example`, `callbacks`). Keys already present are offered again. Keys come from a fixed tree of the OpenAPI Specification.
- In JSON, keys come from the surrounding objects (arrays are transparent). Inside quotes it completes the name only; outside it inserts `"name": `. Enum values are inserted quoted.

### Quick fixes

- Root section additions preserve existing source; ambiguous duplicate owners don't offer them.
- **Add query parameter** merges only into block-style or JSON lists.
- Add operation/response also work in strict JSON (exact token scanner, single reviewed edit; duplicate keys, comments and non-strict JSON refused).
- YAML operation/response fixes insert at the exact end of an existing block mapping. Flow style, anchors, multi-document, JSON and `$ref` path items are refused.
- Extract inline schema: the value must be an inline block mapping or JSON object. `components.schemas` is created if missing (JSON commas handled). A number is appended when a name is taken.

### Endpoints view

- Scans OpenAPI YAML/JSON files up to 1 MiB in the current window's project, and re-scans on each render. A JSON file is read in full before the 1 MiB cap applies.
- Without `project.write`, **Create HTTP Request File** copies the request.

### Preview

- The plugin reads the files the open document references and hands them to the page. One level only, at most 50 files and 512 KiB total; larger ones are skipped. Files are re-sent only when changed.
- Redoc loads on first use, with search off.

### Try requests

- Preparation never sends. **Send once** checks the fresh project/editor, exact source text/version and a single-use 120-second ticket. Editing the spec or switching source invalidates the preview.
- OpenAPI 3.0/3.1 up to 128 KiB, in a `.json` or `.yaml`/`.yml` editor. YAML is converted to the same JSON value, so every check below applies unchanged; one document only, and duplicate keys (including `200` and `'200'`), anchors, aliases, tags and merge keys are refused. GET/HEAD and declared scalar path/query values. Refuses additional network grants. Referenced/variable servers are unsupported.
- API 1.4 may follow up to ten redirects within granted hosts; other hosts and HTTPS-to-HTTP downgrades are denied. Five-second timeout. At most 256 KiB accepted and 64 KiB shown as plain text, headers omitted.
- After a failed Send, only a constant status (permission denial, unsupported request/response or generic failure) is kept for that source/version. Drift or a new preparation clears it. Nothing is sent automatically.

### Bundling and references

- Self-contained bundling is recursive; a recursive reference is copied once. A YAML spec bundles YAML files, a JSON spec bundles JSON files; mixing is refused. An object not under `/components/<section>/` lands in `schemas`.
- Copy Reference uses actual mapping names and array indices, JSON Pointer escaping and percent-encoded UTF-8 URI fragments. Literal dots/brackets are names; literal percent signs are encoded. Values, blank space, ambiguous owners, unsupported forms and pointers over 4096 bytes leave the clipboard alone.
- Current-document occurrences include references into the component. External incoming references are outside it.
- Project usages follow exact physical component identity through local and relative-file `$ref`s, including array references and descendants. Read failures, unsupported models and discovery limits are marked partial. Remote/encoded identities aren't treated as local paths.
- Component rename checks every discovered JSON/YAML source, collisions, exact reference spans, discovery limits and unchanged source. Unsupported/dynamic/discriminator ownership, security-scheme names, transformed/encoded names and unknown models refuse the whole edit. New names use letters, digits, underscore and dash. Ignored files, external projects and generated references are outside it.
- Indexed usages and automatic Rename aren't registered: exact token spelling alone doesn't prove complete cross-file ownership.

### Comparison

- The comparison snapshot belongs to its open project, innermost document owner and exact revision text/version. The visible cache is capped at 1,000 changes with a partial label.
- Export has an overwrite review. Editing either source invalidates the export until you compare again.
- The breaking-change report uses local `$ref`s only, bounded size and depth. It covers a documented subset of openapi-diff/oasdiff rules for paths, operations, parameters, request bodies, responses, enums, schema properties, constraints, nullability, oneOf/anyOf members, readOnly/writeOnly, deprecated and security requirements, with direction-aware severity. `allOf` is flattened, `additionalProperties` isn't analysed, and the rule set isn't complete.

### Draft from source

- Scans bounded files and bytes. Never overwrites `openapi-draft.yaml`. Constants, ant patterns, regex path variables and unreadable types are skipped or left as TODO.

### Code generation

- The installed `langs` catalog supplies the generator picker.
- The review shows source, generator, staging and final paths. Apply works after a matching terminal exit and expected CLI metadata, as a reviewed native directory Rename; an existing final destination is refused.
- Stop a running generator in its keyed native terminal. Reloading the plugin loses in-memory stage tracking. `--skip-overwrite true` protects staged files.
- The external process can resolve referenced inputs on its own; review them first.
- With options: `config-help -l` supplies option names/descriptions; templates must be project-owned; engines are default/Mustache/Handlebars. Additional-property strings with comma/equals are refused. A project-relative JSON config (at most 16 KiB, known scalar options only, no secrets) is shown in the review.

### Parsing

- JSON is read by a small scanner. Key escapes other than `\"`, `\/` and `\\` aren't decoded. A document that stops being valid JSON is checked only up to that point.
- Strict JSON/YAML proofs reserve key, flattened-path (4 MiB) and depth (48) budgets before building models, so long or duplicate-prefixed documents can't allocate oversized models. Completion separates syntactic incompleteness from budget exhaustion.

</details>

## Development

    cargo test                       # logic, against the SDK's mock host
    openidea-plugin build            # → dist/plugin.wasm, validated like an install
    openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
