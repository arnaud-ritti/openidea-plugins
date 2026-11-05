# .env files

Port of the ".env files" plugin (`ru.adelf.idea.dotenv` 263.7307.209,
[JetBrains/idea-php-dotenv-plugin](https://github.com/JetBrains/idea-php-dotenv-plugin)
revision `a1e60501`, MIT) by Adel Fayzrakhmanov and JetBrains. The Docker,
PHP, PHPUnit, Go, Ruby and Rust support comes from revision `c1484553`, before
it was removed upstream.

`.env` files get highlighting and the original's 11 inspections. Environment
variables get completion and go to declaration in `${…}` and in code, with the
keys of `.env` files, Dockerfiles and docker-compose files as declarations.

## Files

The `dotenv` language covers `.env`, `.env.*`, `.flaskenv` and `*.env`. The
IDE's own `dotenv` definition still handles detection and comments. This
plugin supplies the highlighting: keys, values, comments, `export`, the `=` /
`:` separator and `${…}` in double-quoted values.

## Inspections

Problems are published when a `.env` file is opened or saved, and again each
time the editor asks for code actions, so they follow typing between saves.

| Check | Severity | Quick fix |
|---|---|---|
| Key containing `-` | warning | none |
| Key not starting with a letter, `_` or `-` (`-` is the check above) | warning | none |
| Key with lowercase letters | warning | none |
| Duplicate key (every occurrence) | warning | none |
| Spaces around `=` | warning | Remove spaces around '=' |
| Unquoted value containing a space | warning | Add quotes (the file's more common quote style, `'` on a tie) |
| Trailing spaces or tabs | warning | Remove trailing whitespace |
| More than one blank line in a row | warning | Remove extra blank lines |
| `${…}` in a value that isn't double-quoted | error | Put the value in double quotes |
| `${KEY}` that no `.env` file, Dockerfile or compose file declares | warning | none |
| Empty `${}` | warning | none |

## Completion

- In code, inside a recognised call (below): the project's declared keys.
- In a `.env` file, inside `${…}`: the same keys.
- In a double-quoted `.env` value, typing `$DB` offers `${DB_HOST}`. The popup
  opens on the letters after the `$`, not on the `$` itself, and never after
  `\$` or outside double quotes.

Each key is listed once, with its value as detail (`= value`: the first line,
then `...` if the value spans more lines). The value shown comes from a plain
`.env`, a Dockerfile or a compose file before `.env.example`, `.env.local` and
the other variants.

## Navigation and usages

- Go to Declaration on a key in code or in `${KEY}`: its declarations in plain
  `.env` files, Dockerfiles and compose files. If none of these declares it,
  the declarations in the other `.env.*` files.
- Go to Declaration on a key where a `.env` file declares it: its usages, in
  code and in `${KEY}` of `.env` files.
- After you edit a key in a `.env` file without saving, ⌥⏎ on that line
  offers **Rename usages of OLD to NEW**. It rewrites every indexed usage of
  the old key, in code and in `${…}`. It only appears when the value, the
  number of keys and the line are unchanged from the saved file, and the old
  key is gone from it.

## Where keys are found

Declarations:

- `.env` files: `KEY=value`, `KEY: value`, `export KEY=value`, `#` comments,
  single- and double-quoted values (multi-line too), `\ ` in keys.
- Dockerfiles (`Dockerfile`, `Dockerfile.*`, `*.dockerfile`): `ENV KEY=value
  OTHER="x y"` and `ENV KEY value`, continued with `\`.
- Compose files (`docker-compose.yml`, `docker-compose.yaml`, `compose.yml`,
  `compose.yaml`): `environment:` as a list (`- KEY=value`) or a mapping
  (`KEY: value`).

Usages in code are matched on the text right before the key, compared
lowercased. The key follows a `"`, `'` or `` ` `` (blanks before the quote
are allowed), or comes unquoted after a member access:

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

Completion and go to declaration work in PHP, Python, Java, Kotlin, Scala,
Groovy, JavaScript, TypeScript (and JSX/TSX), Vue, Svelte, Go, Ruby, Rust,
C# and XML. The usages index reads files with those languages' extensions
plus `phpunit.xml*`.

## Settings

Settings › Plugins › .env files:

| Key | Default | Effect |
|---|---|---|
| `completion` | `true` | Complete environment variables in code (`.env` files always complete) |
| `showValues` | `true` | Show each key's value in completion |

## Limits

- Text-based: the original plugin walks each language's syntax tree; this port
  matches the text before the key. A call is recognised anywhere its text
  appears, comments and strings included, and any name ending in `env(`
  counts. A key held in a variable (`process.env[name]`) isn't seen.
- The project index is built by a walk (at most 50,000 paths, about 3 s per
  activation or event batch, declarations first) and updated when a file is
  opened or saved. Until the walk is done, only the root's env files are
  known. A cut-off walk leaves some usages out.
- The walk can't list hidden files, so it looks for a fixed list of names
  (`.env`, `.env.local`, `.env.example`, `.env.dist`, `.env.production`, …,
  `.flaskenv`) in the project root and in every folder holding a project
  marker (`package.json`, `composer.json`, `go.mod`, `Cargo.toml`,
  `Dockerfile`, …). Any other env file is known only once it has been opened
  or saved.
- Compose files are read line by line: no flow style (`environment: [A=1]`),
  no anchors, no `env_file:`. Keys in Dockerfiles and compose files are
  declarations only: `$KEY` there isn't a usage, and Go to Declaration doesn't
  work from those files.
- Rename isn't a refactoring. The quick fix above covers it after the edit,
  using the usages known to the index.
- Lines longer than 2,000 bytes (minified code) aren't searched for usages.
- When the editor gets an empty code action answer for a `.env` file, it can
  stop asking for 15 s. Problems shown while typing may lag behind until the
  next save or request.

`src/lib.rs` registers the component by hand: `register_plugin!` has no arm
for syntax with providers.

The original plugin is MIT, copyright 2017 Adel Fayzrakhmanov (see `LICENSE`).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
