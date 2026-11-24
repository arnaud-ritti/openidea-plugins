# .env files

Highlighting and 11 inspections for `.env` files, plus completion and Go to Declaration for environment variables in your code. Keys come from `.env` files, Dockerfiles and Docker Compose files. Works with PHP, PHPUnit, Python, JavaScript/TypeScript, Go, Ruby, Rust, Java, C# and more. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or newer (`engines.openidea = "^1.14"`).
- No external tools.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a project that has a `.env` file.
3. In code, type `getenv('` (or `process.env.`, `env('`, `os.environ['`…) and pick a key.
4. Put the caret on the key and use Go to Declaration to open its `.env` line.

## Features

### Highlighting

The `dotenv` language covers `.env`, `.env.*`, `.flaskenv` and `*.env`. Keys, values, comments, `export`, the `=` or `:` separator and `${…}` in double-quoted values are highlighted.

### Fix problems in `.env` files

Problems show as you type. Press ⌥⏎ on one to apply its quick fix.

| Check | Severity | Quick fix |
|---|---|---|
| Key containing `-` | warning | none |
| Key not starting with a letter or `_` (a leading `-` falls under the check above) | warning | none |
| Key with lowercase letters | warning | none |
| Duplicate key (every occurrence) | warning | none |
| Spaces around `=` | warning | **Remove spaces around '='** |
| Unquoted value containing a space | warning | **Add quotes** (the file's more common quote style, `'` on a tie) |
| Trailing spaces or tabs | warning | **Remove trailing whitespace** |
| More than one blank line in a row | warning | **Remove extra blank lines** |
| `${…}` in a value that isn't double-quoted | error | **Put the value in double quotes** |
| `${KEY}` that no `.env` file, Dockerfile or compose file declares | warning | none |
| Empty `${}` | warning | none |

### Complete environment variables

- In code, inside a recognised call (see [Where keys are found](#where-keys-are-found)): the project's declared keys.
- In a `.env` file, inside `${…}`: the same keys.
- In a double-quoted `.env` value, typing `$DB` offers `${DB_HOST}`. The popup opens on the letters after `$`, never after `\$` or outside double quotes.
- Each key is listed once, with its value as detail (`= value`). Multi-line values show the first line, then `...`.
- The value shown comes from a plain `.env`, a Dockerfile or a compose file before `.env.example`, `.env.local` and other variants.

Turn off completion in code, or hide values, in [Settings](#settings).

### Jump to declarations and usages

- Go to Declaration on a key in code or in `${KEY}` opens its declarations in plain `.env` files, Dockerfiles and compose files. If none of these declares it, you get the declarations in other `.env.*` files.
- Go to Declaration on a key where a `.env` file declares it lists its usages, in code and in `${KEY}` of `.env` files.
- A gutter icon on each key of a `.env` file lists its usages and the same key in the project's other env files. Click it to pick one; with a single target it opens right away. Keys with nothing to list get no icon.

### Hover

Hover a declared `.env` key, a `${VARIABLE}` reference, or a supported reference in code. You see where it is declared (path, line and column) and whether that is an env file, a Dockerfile or a Compose file. Several declarations are listed as candidates; hover doesn't guess which one wins at run time. Unsaved edits in the current `.env` file are used.

Hover also shows which Compose services load an env file through `env_file`, and that the project `.env` only feeds Compose interpolation.

### Masked values

Values of keys containing `PASS`, `SECRET`, `TOKEN`, `KEY` or `CREDENTIAL` show as `••••` in completion and hover. To see one, put the caret on the key and run **.env: Show Value at Caret** (Search Everywhere › Actions). It lists the real value of each declaration in a picker that closes without a trace (it isn't kept in the notification history); pick one to open it. It needs the optional `editor.read` permission.

### Docker Compose and Dockerfile links

- In compose files, `$VAR`, `${VAR}`, `${VAR:-d}`, `${VAR-d}`, `${VAR:?e}`, `${VAR?e}` and `${VAR:+r}` link to keys in the project-directory `.env`.
- Info hints mark a plain `${VAR}` missing from that `.env`, and `.env` keys nothing references.
- A Dockerfile `ARG` supplied by compose `build.args` from `${VAR}` links to that `.env` key.
- `ARG` uses (`${K}`, `$K`) in a Dockerfile link to the `ARG` declaration in that stage, following Docker's scoping rules.

### Update `${…}` references after renaming a key

After you edit a key in a `.env` file without saving, ⌥⏎ may offer **Update ${OLD} → ${NEW} in this file…**. It shows each replacement and asks you to confirm. It changes only `${OLD}` tokens in the current file. Cancelling changes nothing.

### Where keys are found

**Declarations:**

- `.env` files: `KEY=value`, `KEY: value`, `export KEY=value`, `#` comments, single- and double-quoted values (multi-line too), `\ ` in keys.
- Dockerfiles (`Dockerfile`, `Dockerfile.*`, `*.dockerfile`): `ENV KEY=value OTHER="x y"` and `ENV KEY value`, continued with `\`.
- Compose files (`docker-compose.yml`, `docker-compose.yaml`, `compose.yml`, `compose.yaml`): `environment:` as a list (`- KEY=value`), a mapping (`KEY: value`) or a flow collection (`[A=1, B]`, `{A: 1, B: ~}`, multi-line too).
- The files named by a compose `env_file:` (a scalar, a list or the `- path: x` long form, relative to the compose file). They are read when the compose file is indexed. A missing file is ignored.
- YAML anchors and merges (`environment: *env`, `<<: *common`) are followed.

**Usages in code.** The key must follow a quote (`"`, `'` or `` ` ``) right after one of these calls, or come unquoted after a member access:

| Form | Examples |
|---|---|
| `…env(` | `getenv(` (PHP, Python `os.getenv`, Java/Kotlin `System.getenv`), Go `os.Getenv(`, Laravel `env(` |
| `…env[` | `$_ENV[`, Ruby `ENV[`, `process.env[` |
| `…env.get(`, `…env.fetch(` | `Deno.env.get(`, `dotenv.get(`, Ruby `ENV.fetch(` |
| `environ[`, `environ.get(` | Python `os.environ[`, `os.environ.get(` |
| `lookupenv(` | Go `os.LookupEnv(` |
| `env!(`, `env::var(`, `env::var_os(`, `env::set_var(`, `env::remove_var(`, `dotenvy::var(` | Rust |
| `$_server[` | PHP |
| `getenvironmentvariable(` | C# `Environment.GetEnvironmentVariable(` |
| `system.getproperty(` | Java, the key may hold dots (`app.name`) |
| `<env name=`, `<server name=` | PHPUnit XML |
| `process.env.`, `import.meta.env.`, `bun.env.` | JavaScript/TypeScript, unquoted key |

Completion and Go to Declaration work in PHP, Python, Java, Kotlin, Scala, Groovy, JavaScript, TypeScript (and JSX/TSX), Vue, Svelte, Go, Ruby, Rust, C# and XML.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **.env**; bind keys in Settings › Keymap.

| Command | What it does | Shortcut |
|---|---|---|
| .env: Show Value at Caret | Lists the unmasked values of the key under the caret; pick one to open its declaration. | none |

## Settings

Settings › Plugins › *.env files*.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Complete environment variables in code | `completion` | on | Completion of keys in code. `.env` file completion stays on. |
| Show each key's value in completion and hover | `showValues` | on | When off, completion and hover show keys and source locations only. Secret-looking keys are masked either way. |
| Check key names | `checkKeyNames` | on | Warns about `-`, lowercase letters or a wrong first character in a key. |
| Warn about duplicate keys | `checkDuplicateKeys` | on | Warns when a key is declared more than once in a file. |
| Check spacing and blank lines | `checkFormatting` | on | Warns about spaces around `=`, trailing whitespace and extra blank lines. |
| Check value quotes | `checkValueQuotes` | on | Warns about unquoted values with spaces and `${…}` values outside double quotes. |
| Check ${…} references | `checkVariableReferences` | on | Warns about an empty `${}` and variables declared in no `.env`, Dockerfile or compose file. |

`showValues` hides output only. It doesn't remove values from the index or hide values already visible in source files.

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › *.env files*.

| Permission | Used for | Without it |
|---|---|---|
| `project.read` (required) | Reading `.env`, Dockerfile and compose files, and publishing problems | The plugin can't run. |
| `events` (required) | Checking env files when they are opened, saved or changed | The plugin can't run. |
| `editor.read` (optional) | **.env: Show Value at Caret** reads the caret position | That command asks you to allow it; everything else works. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| A key from an unusual env file is missing | Open or save that file once. Only a fixed list of env file names is found automatically (see Technical details). |
| Some usages are missing in a large project | The project walk stops at 50,000 paths and reads about 1 second per activation or file change, resuming on the next. Open or save the files you need. |
| "Variable '…' is not declared in any .env file, Dockerfile or compose file." | Declare the key, or ignore it if it comes from the real environment. |
| No hover on a key in code right after opening the project | Wait for indexing to finish; hover reads only the built index. |
| A key held in a variable (`process.env[name]`) isn't recognised | Expected: only literal keys are matched. |
| Go to Declaration does nothing in a Dockerfile or compose file | Expected: keys there are declarations only. |
| Values show in completion and you don't want them | Turn off **Show each key's value in completion and hover**. |
| A value shows as `••••` | The key looks like a secret. Run **.env: Show Value at Caret** on it; allow `editor.read` in Settings › Plugins › *.env files* if asked. |

## Limitations

- Text-based: the plugin matches the text before the key instead of parsing each language. A call is recognised anywhere its text appears, comments and strings included. Any name ending in `env(` counts.
- Lines longer than 2,000 bytes (minified code) aren't searched for usages.
- No semantic Rename or host Find Usages. Navigation returns candidates, not a proven runtime owner.
- In Dockerfiles and compose files, `$KEY` isn't counted as a code usage.
- Runtime environment values and interpolation are not evaluated.

## Technical details

<details>
<summary>Indexing, limits, extension point and safety checks</summary>

**Language.** The IDE's own `dotenv` definition still handles detection and comments. This plugin's copy only makes it the language's owner, so its highlighting is used.

**Inspections.** Problems are returned by the registered current-document diagnostics provider. The host versions these results against the editor text; code actions are a separate request. Unknown cache completeness suppresses absence warnings.

**Usage matching.** Usages in code are matched on the text right before the key, compared lowercased. Blanks before the quote are allowed. The usages index reads files with the supported languages' extensions plus `phpunit.xml*`.

**Project index.** The index is built by a walk (at most 50,000 paths, about 1 s per activation or event batch, resuming on the next, declarations first) and updated when a file is opened or saved. Until the walk is done, only the root's env files are known. A cut-off walk leaves some usages out. The walk can't list hidden files, so it looks for a fixed list of names (`.env`, `.env.local`, `.env.example`, `.env.dist`, `.env.production`, …, `.flaskenv`) in the project root and in every folder holding a project marker (`package.json`, `composer.json`, `go.mod`, `Cargo.toml`, `Dockerfile`, …). Any other env file is known only once it has been opened or saved.

**Extension point.** Other plugins can register additional env declaration files or env readers through the `aritti.dotenv.sources` extension point (exact file names, role `declares` or `reads`); changes apply without a restart.

**Source-only reference update.** The lazy FileAction previews and confirms only current-file interpolation token replacements through atomic host edits. It requires cached prior declaration evidence, the same value/key count/source line, one current NEW declaration, no remaining OLD declaration, and literal ASCII names of at most 128 bytes. It uses no provider file discovery and writes no other env, Compose, Docker or language file. Values never enter the confirmation, which lists only exact target/key-token ranges and replacement names. The action is single-use and revalidates the exact path/language/version/text, innermost currently open project root, cached suggestion and current unsaved text before and after the confirmation. Bounds: 12 KiB text, 256 declarations, 8,192 cached declarations, 128 token replacements and eight pending actions; the shared 16 KiB exact-preview bound rejects oversized confirmations. CRLF and other source text are preserved. This is not runtime-aware Rename or proof that the interpolation uses resolve to the edited declaration.

**Hover.** Current `.env` buffer declarations replace that file's cached entries, including unsaved edits. Hover never warms the index, probes env files, runs a process or accesses the network. It only looks up the current project identity, reads an already-built cache, and parses the current `.env` text. Cold code-reference requests return no hover until activation/events have populated the cache. Cross-file values can lag edits/deletions until open/save event indexing runs. Cache incompleteness and bounded results are labelled. Limits: current documents up to 256 KiB, code lines up to 2,000 bytes, keys up to 256 bytes, the first 8,192 cached declarations and up to 32 matching sources; displayed keys/values are capped at 256 characters and source paths at 1,024. The complete response is capped at 16 KiB of UTF-8, including Markdown fences and provenance notes; whole entries are omitted with a bounded-results label rather than clipped. Markdown values are code spans with control characters escaped. Compose-service hover reads scalar, list and path/required object `env_file` forms and stays quiet on aliases, merges and non-literal paths.

**Compose and Dockerfile links.** Compose interpolation links are cache-only, and quiet when the cache is incomplete or the file is an `env_file` target. Dockerfile `ARG` links cover a global `ARG` re-declared as bare `ARG K` inside a stage, and `${K}`/`$K` uses after the declaration in that stage or in stages based on it. Only instructions Docker expands count (`ADD`, `COPY`, `ENV`, `EXPOSE`, `LABEL`, `STOPSIGNAL`, `USER`, `VOLUME`, `WORKDIR`, `ONBUILD` of those) and shell-form `RUN`/`CMD`/`ENTRYPOINT` outside single quotes. A global `ARG` is not inherited by stages; a stage `ARG` is inherited by stages based on it. Saving, creating or deleting a Dockerfile that a Compose service builds re-derives its `ARG` links.

**Gutter icons.** The `line-markers` provider gives one marker per target with the same icon on the key's line, which the IDE shows as one icon. It reads the cached index only, for the same project as Go to Declaration, and the unsaved `${KEY}` of the current file; until activation has built the index there are no icons. Dockerfile and Compose declarations aren't listed. At most 200 targets per key; the tooltip says how many were left out. The icon is the dotenv logo from `icon.svg` with the white tile removed.

**Semantic tokens.** A SemanticTokens export covers variable declarations and `${KEY}` references recognised by the dotenv lexer, with declaration modifiers only on parsed keys. It uses supplied unsaved text only, with no project/index/type lookup. Single-quoted/comment text is not read as a reference. Tokens use real `Variable` semantics; the lexical fallback remains for the full syntax. Work is capped at 256 KiB / 4,096 tokens, with sorted non-empty single-line UTF-16 ranges computed in one forward pass. Other languages and oversized documents return no tokens.

**Compose `env_file` discovery.** Discovery expands local YAML anchors, aliases and nested merge graphs, including scalar/list/long-form paths. Flow lists of `env_file` paths may span multiple lines and contain quoted commas/comments. Explicit mapping entries keep YAML merge precedence. Missing long-form `required: false` files don't make discovery incomplete unless another source requires the same path. Paths stay confined to the project; interpolated, unresolved or unsupported inputs keep discovery incomplete. This discovers source candidates and doesn't infer active Compose services or effective runtime variable precedence.

**Compose `environment`.** Flow collections support multiple lines, comments and quoted commas/closing delimiters while keeping physical source ranges. Nested collections are rejected. Local scalar mapping/list anchors are resolved when used by `environment: *env` or `<<: *common` (including merge sequences). Explicit environment keys override merged keys; the first anchor wins duplicate merge keys. Declaration locations point to the physical anchored key. Forward, cyclic, nested and alias-dependent environment declarations are not expanded. Anchor expansion is capped at 256 KiB, 4,096 lines and 128 names.

**YAML parser limits.** 256 KiB source, depth 32, 20,000 nodes, 100,000 events, 128 anchors/aliases, 1 MiB total scalar/retained anchor bytes, 20,000 replay events and 128 expansions per anchor. Cycles, undefined aliases, duplicate mapping keys, unsupported tags and budget exhaustion refuse complete discovery. Filesystem includes and property interpolation are not enabled.

**Rename.** Semantic Rename/host Find Usages indexing is not implemented: lexical matches across env variants, Compose services, Docker build/run stages and code don't establish one complete runtime owner set.

**Settings layout.** Settings › Plugins › *.env files* groups the options under **Completion** and **Value visibility**.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
