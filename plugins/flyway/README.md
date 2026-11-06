# Flyway Migrations

Flyway support for SQL migrations in OpenIDEA. There is no database to ask:
the plugin reads migration file names and the project's Flyway configuration,
and runs the Flyway commands in a terminal.

## Checks

Migrations are checked when a `.sql` file or a config file opens or is saved,
and by **Flyway: Check Migrations**, which also reports how many were checked.

| Check | Severity |
|---|---|
| File name doesn't match the naming scheme (prefix, version, separator, description, suffix) | warning |
| Same version used twice (versioned, undo and baseline migrations, each kind on its own) | error |
| Same description used twice by repeatable migrations | error |

Duplicates are looked for within one module (the part of the path before
`src/main/resources`), so the same version in two modules is fine. Each
duplicate points at the other files. Callbacks (`beforeMigrate.sql`…) are
accepted.

A rename done outside the IDE isn't seen until the next open or save; run
**Flyway: Check Migrations**.

## Where migrations are found

In this order:

1. The `locations` setting (comma-separated), if not empty.
2. `flyway.locations` in `flyway.toml`, `flyway.conf`, `conf/flyway.toml` or
   `conf/flyway.conf` (the first one that exists), and
   `spring.flyway.locations` in `application*.properties`/`.yml`.
3. `classpath:db/migration`, plus `filesystem:sql` when a Flyway config file
   exists.

`classpath:` locations are searched in every `src/main/resources`;
`filesystem:` locations relative to the project root or as an absolute path
inside it. `{vendor}` matches any directory. Other schemes (`s3:`, `gcs:`)
are ignored. The same files give the prefixes (`sqlMigrationPrefix`,
`undoSqlMigrationPrefix`, `repeatableSqlMigrationPrefix`,
`baselineMigrationPrefix`), the separator and the suffixes; defaults are
`V`, `U`, `R`, `B`, `__` and `.sql`.

## Commands

- **Flyway: Copy Next Migration Version** copies the next versioned prefix
  (`V3__`) to the clipboard, and the notification names the folder of the
  newest migration. Without the `clipboard` permission, the notification
  shows the name instead.
- **Flyway: Info / Validate / Migrate / Repair** run the Flyway goal in a
  terminal.
- **Flyway: Clean (Drops All Objects)…** asks first: run it a second time
  within 30 seconds to start it. Flyway's own `cleanDisabled` setting still
  applies.

Run configurations: when `flyway.toml` or `flyway.conf` is at the project root, the IDE offers
**Flyway: info**, **Flyway: validate** and **Flyway: migrate (writes to the database)**. They always run
plain `flyway <goal>` and ignore the `runner` and `arguments` settings; the URL, credentials and
environment come from the config file. A config in `conf/` is not detected, and `migrate` changes the
database with one click. Clean and Repair stay commands.

The setting `runner` picks `flyway` (the CLI, goal `info`) or `mvn` (goal
`flyway:info`); `arguments` are added after the goal, split on spaces
(`-environment=dev`). Running a program needs the `process:flyway` or
`process:mvn` permission, which a notification asks you to allow in
Settings › Plugins.

## Migrations view

The bottom view Flyway Migrations runs `info` as JSON (`-outputType=json`; with
`mvn`, `-Dflyway.outputType=json`) in the project root and lists every
migration with its state: Applied, Pending, Failed, Missing (applied, file
gone), Out of order, Ignored, Superseded, Deleted. Double-click a row to open
its file, found by version (or description for repeatable ones) among the
migration locations, else by the `filepath` Flyway prints. The view runs `info` each time it
renders, and Refresh runs it again. It needs Flyway 10 or newer, a database Flyway can reach (the
configuration comes from the project or the `arguments` setting) and the
permission to run the program; otherwise the view says what failed. Not verified:
with `mvn`, whether the JSON survives Maven's `[INFO]` line prefixes and whether
the Maven plugin honours `outputType`; `flyway` is the reliable runner.

## Completion

- In SQL files, after `${`: Flyway's built-in placeholders
  (`flyway:defaultSchema`, `flyway:user`, `flyway:timestamp`…) and the
  `flyway.placeholders.*` names from the project's config.
- In `flyway.conf` (and `flyway-*.conf`): `flyway.*` keys, written as
  `key=`.
- In `flyway.toml`: keys under `[flyway]`, and the connection keys under
  `[environments.<name>]`, written as `key = `.

## Not supported

Anything that needs the database: applied-migration status, SQL execution,
schema diffs. Custom placeholder prefix and suffix, `src/test/resources`
locations, quoted values with spaces in `arguments`, and renaming a
misnamed file with a quick fix aren't supported either. See
`LIMITATIONS.md`.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
