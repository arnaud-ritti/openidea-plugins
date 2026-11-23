# Flyway Migrations

Flyway support for SQL migrations in OpenIDEA. It checks migration file names, finds duplicate versions, helps you create and rename migrations, and runs Flyway commands when you ask. MIT; see `LICENSE`.

The plugin never connects to a database on its own. It reads migration file names and the project's Flyway configuration. Flyway commands run only when you start them.

## Requirements

- OpenIDEA with plugin API 1.14 or newer.
- To run Flyway commands: the Flyway CLI (`flyway`) or Maven (`mvn`) with the Flyway plugin, on the `PATH` the IDE sees.
- For the migrations list in the tool window: Flyway 10 or newer (it needs JSON output).
- For the optional Hibernate schema generator: a JDK (`java`) and the pinned Hibernate runtime. See the [adapter setup](adapters/README.md).

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a project with Flyway migrations, for example in `src/main/resources/db/migration`.
3. Open any migration `.sql` file. Bad names and duplicate versions are underlined.
4. Run **Flyway: Check Migrations** to check every migration at once.
5. To run Flyway, allow `process:flyway` (or `process:mvn`) in Settings › Plugins › Flyway Migrations, then run **Flyway: Run Info…**.

## Features

### Check migration names and duplicates

Migrations are checked when a `.sql` file or a Flyway config file opens or is saved. **Flyway: Check Migrations** checks them all and reports how many it checked.

| Check | Severity |
|---|---|
| File name doesn't match the naming scheme (prefix, version, separator, description, suffix) | warning |
| Same version used twice (versioned, undo and baseline migrations, each kind on its own) | error |
| Same description used twice by repeatable migrations | error |

- Duplicates are looked for within one module: the part of the path before `src/main/resources`. The same version in two modules is fine.
- Each duplicate points at the other files.
- Callbacks (`beforeMigrate.sql`…) are accepted.
- A rename done outside the IDE isn't seen until the next open or save. Run **Flyway: Check Migrations** to catch up.

### Where migrations are found

The plugin looks in this order:

1. The **Migration locations** setting (comma-separated), if not empty.
2. `flyway.locations` in `flyway.toml`, `flyway.conf`, `conf/flyway.toml` or `conf/flyway.conf` (the first one that exists). Also `spring.flyway.locations` in `application*.properties` and `.yml`.
3. `classpath:db/migration`, plus `filesystem:sql` when a Flyway config file exists.

- `classpath:` locations are searched in every `src/main/resources`.
- `filesystem:` locations are relative to the project root, or an absolute path inside it.
- `{vendor}` matches any directory.
- Other schemes (`s3:`, `gcs:`) are ignored.

The same files give the naming scheme: `sqlMigrationPrefix`, `undoSqlMigrationPrefix`, `repeatableSqlMigrationPrefix`, `baselineMigrationPrefix`, the separator and the suffixes. Defaults are `V`, `U`, `R`, `B`, `__` and `.sql`.

### Create a migration

- **Flyway: New Migration…** opens a naming form in the tool window. Pick the directory, category (Versioned, Undo, Baseline, Repeatable), version, description and suffix.
  - Description words use ASCII letters and digits joined by underscores. The preview shows any change the plugin makes.
  - **Prepare…** checks the plan without writing a file. It opens the newest migration in the target folder.
  - Then press ⌥⏎ in that file (or any project SQL or Flyway config file) and choose **Create Flyway migration from prepared form…**. Confirm the target and content. The file is created in one undoable step and never overwrites an existing file.
- **Create Flyway SQL migration…** (⌥⏎ in the editor) is a shorter Versioned form: version, description and directory.
- ⌥⏎ also offers Java and Kotlin migrations, and Java and SQL callbacks. You set the version, description, directory, package, events, class name and transaction handling.
- **Flyway: Copy Next Migration Version** copies the next versioned prefix (`V3__`) and names the folder of the newest migration. Without the `clipboard` permission, the notification shows the name instead.

### Rename a migration

In a migration SQL file, press ⌥⏎ and choose **Rename Flyway migration…**. Type the full corrected file name, check the old and new paths, and confirm. The file is renamed in one step. Its contents and any references are not changed.

The plugin does not check the database history. Think twice before renaming a migration that is already applied.

### New file templates and icons

The **New** menu offers **Flyway SQL Migration**, **Flyway Java Migration**, **Flyway Kotlin Migration**, **Flyway Java Callback** and **Flyway SQL Callback**.

- The class is named after the file (`V2__Add_users`), so name the file first.
- Take the version from **Flyway: Copy Next Migration Version**.
- Java and Kotlin templates have no `package` line.
- `V*__*.sql`, `U*__*.sql` and `R__*.sql` files get the Flyway icon. Every placeholder on a line gets its own gutter marker; with several, the marker opens a list to pick from.

### Run Flyway commands

**Flyway: Run Info…**, **Flyway: Validate**, **Flyway: Migrate**, **Flyway: Repair** and **Flyway: Clean (Drops All Objects)…** run Flyway on your project.

- Before launch, each command asks for confirmation. The title names the target (for example **Clean db.example/app: drop all objects?**), and the body gives the effect, the target database, the project and how the command runs.
- The target database comes from the project's Flyway config files, without credentials or URL parameters: PostgreSQL, MySQL, MariaDB, SQL Server, Oracle, DB2, Redshift, H2 and SQLite URLs are recognised. With several `[environments.*]`, the one chosen by `-environment=<name>` in **Extra arguments** (or `environment` in `[flyway]`, else `default`) is used. The target is unknown when command-line arguments set the URL, the chosen environment has no URL, or the URL can't be read; the confirmation says why. The CLI or environment can still override it.
- **Run Flyway commands with** picks `flyway` (the CLI, goal `info`) or `mvn` (goal `flyway:info`).
- **Extra arguments** are added after the goal, split on spaces, for example `-environment=dev`.
- **Clean** warns that it drops objects and always asks for confirmation. When the target is unknown, you also type `clean` to go ahead. Flyway's own `cleanDisabled` setting still applies.
- By default (**terminal** mode), Info and Validate use the IDE's Run, and the other commands open a terminal.
- In **managed** mode, the output appears in the **Command Output** tab instead. See [Managed mode](#managed-mode).
- In managed mode, a successful Migrate, Repair or Clean shows a notification with a **Run Info** button.
- Running a program needs `process:flyway` or `process:mvn`. A notification asks you to allow it.

The Run menu also offers **Flyway: info** and **Flyway: validate** for a `flyway.toml` or `flyway.conf` in the project root or in `conf/`. They follow **Run Flyway commands with**, run from the project root and support Stop and Rerun. They disappear when **Extra arguments** is set, so credentials are not saved in Run configurations. Migrate, Clean and Repair are never Run configurations: you review them each time.

### Browse migrations

The **Flyway** tool window (bottom) has three tabs: **Flyway**, **Command Output** and **Comparison**. Switching tabs never starts a command or a database connection.

The **Flyway** tab lists the project's migrations.

- The list is a table. Before Info has run, it shows the local migrations with **Version**, **Description**, **Type** and **Problems** columns. After Info, it shows **Version**, **Description**, **Type**, **State** and **Installed**. Filter by **Type** and **State** beside the search box. The status line says when table navigation indexed only part of the migrations.
- **Run Info…** runs Flyway Info (after you confirm it) and fills the list. A finished Info appears without another press.
- Double-click a row, or press Enter, to open its file.
- **Open** opens the selected migration. **New…** starts a new migration in the same folder.
- **Filter…** searches the rows. **Clear** removes the filter. Filters belong to the current project.
- **More…** opens the other review and CLI actions.
- The list is cached and shows how old it is. Changing a config or migration file clears it.

After Info, migration files in the project tree show a short note with their state: `applied 2026-11-10`, `pending`, `failed` or `outdated`. The notes disappear when the Info result goes stale.

### See the target database

When the project has a Flyway config file (`flyway.toml` or `flyway.conf`), the status bar shows the target database from it, for example **Flyway: db.example:5432/app**. It shows just **Flyway** when the target is unknown. Click it for a menu: **Show Migrations**, **New Migration…**, **Run Info…**, **Validate**, **Migrate** and **Repair**. After Info, the menu starts with the number of pending migrations. Migrate and Repair still ask before they run.

### Find a migration

Press Shift twice and open the **Flyway Migrations** tab. Type part of a version or description, for example `V12` or `add users`, to open the migration file. The list shows naming problems next to the path.

### Coloured console output

Once a project uses Flyway, Run consoles colour Flyway's messages, from the CLI or from an application log (Spring Boot runs Flyway at startup): `ERROR:` lines in red, `WARNING:` lines in yellow, failed migrations and `Validate failed` in bold red, applied, validated and repaired results in green, and `Migrating schema …` in cyan.

### Complete placeholders and config keys

- In SQL files, after `${`: Flyway's built-in placeholders (`flyway:defaultSchema`, `flyway:user`, `flyway:timestamp`…) and the `flyway.placeholders.*` names from your config.
  - Custom `placeholderPrefix` and `placeholderSuffix` delimiters work, including multi-character and comma delimiters.
  - `placeholderReplacement=false` turns suggestions off.
  - This is editor help. It does not prove what the CLI will use.
- In `flyway.conf` (and `flyway-*.conf`): `flyway.*` keys, written as `key=`.
- In `flyway.toml`: keys under `[flyway]`, and connection keys under `[environments.<name>]`, written as `key = `.

### Jump to definitions

- **Placeholders:** Go to Definition on `${name}` in SQL jumps to where the placeholder is declared in `flyway.conf` or `flyway.toml`. Names match case-insensitively, as Flyway does. Values and credentials are never shown.
- **Tables:** in migration SQL, table names after `FROM`, `JOIN`, `UPDATE`, `INSERT INTO`, `ALTER/DROP/TRUNCATE TABLE` and `REFERENCES` jump to the `CREATE TABLE` in your migrations.
- **Entities:** when a JPA `@Entity` maps to that table, the entity class is offered too. This covers Java and Kotlin classes with `@Table`, and `orm.xml` / `persistence.xml` mapping files.
- **Table gutter icon:** a line with table references gets the Flyway icon listing the same targets: the `CREATE TABLE` migrations and the mapped entities. Click it to pick one; with a single target it opens right away.

### Review a Flyway comparison report

**Flyway: Review Local Comparison Report…** opens a Flyway `diffText` JSON report that is already in your project.

- Select change IDs and see each source and target definition with its Add, Edit or Delete label.
- For a Delete, the target `CREATE` is the old object's definition, not a `DROP`.
- Nothing is run, combined or written.

The toolbar has **Load Report…** and **Select Changes…**, plus **Start Comparison**, **Check Progress** and **Stop Comparison** while a comparison runs. **More…** holds **Show CLI Commands…**, **Prepare Comparison…** and **Clear**. **Show CLI Commands…** shows the exact Flyway commands to produce such a report. It supports only Flyway **13.12.1 Enterprise**. It is a review aid; it runs nothing.

### Generate SQL from Hibernate entities (optional)

**Flyway: Generate Offline Hibernate Schema SQL…** builds `CREATE` SQL from compiled JPA entity classes you select. It uses a pinned Hibernate 6.6.49 or 7.4.15 runtime and a dialect you choose (16 per runtime, including PostgreSQL, MySQL and MariaDB). The Hibernate 7 profile accepts Jakarta Persistence 3.2.

- You review the classpath and a trusted-code warning before anything runs. Project converters and custom types can run code with side effects.
- No application is started and no database is contacted.
- The SQL stays a preview until you choose **Save Hibernate schema SQL as migration…**. That creates one new, undoable migration file.
- Generation runs as a background task and needs the `process:java` permission.

Setup, profile format and fixtures: [adapters/README.md](adapters/README.md).

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Flyway**; bind keys in Settings › Keymap. No shortcuts are set by default.

| Command | What it does |
|---|---|
| **Flyway: Check Migrations** | Checks every migration's name and looks for duplicate versions. |
| **Flyway: Copy Next Migration Version** | Copies the next version prefix (`V3__`) and names the target folder. |
| **Flyway: New Migration…** | Opens the new-migration form in the tool window. |
| **Flyway: Run Info…** | Runs `flyway info` after you confirm. |
| **Flyway: Validate** | Runs `flyway validate`. |
| **Flyway: Migrate** | Runs `flyway migrate`. |
| **Flyway: Repair** | Runs `flyway repair`. |
| **Flyway: Clean (Drops All Objects)…** | Runs `flyway clean` after confirmation. Drops all objects in the target schemas. |
| **Flyway: Refresh Command Output** | Reads new output from a command running in managed mode. |
| **Flyway: Show Migrations** | Opens the **Flyway** tab of the tool window. |
| **Flyway: Cancel Command** | Stops the command running in managed mode. |
| **Flyway: Review Local Comparison Report…** | Opens a `diffText` JSON report for review. |
| **Flyway: Generate Offline Hibernate Schema SQL…** | Builds SQL from compiled Hibernate entities. |

Editor actions (⌥⏎): **Create Flyway SQL migration…**, **Create Flyway migration from prepared form…**, **Rename Flyway migration…**, **Save Hibernate schema SQL as migration…**, plus Java/Kotlin migration and callback creation.

## Settings

Settings › Plugins › Flyway Migrations.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Migration locations (comma-separated) | `locations` | empty | Where to look for migrations. Empty: read `flyway.toml`, `flyway.conf` and `spring.flyway.locations`. |
| Run Flyway commands with | `runner` | `flyway` | `flyway` (CLI) or `mvn` (Maven plugin). |
| Extra arguments for Flyway commands | `arguments` | empty | Added after the goal, for example `-environment=dev` or `-Dflyway.user=…`. |
| Show command output in | `executionMode` | `terminal` | `terminal` uses the IDE's terminal and Run. `managed` shows output in the **Command Output** tab. |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › Flyway Migrations.

| Permission | Used for | Without it |
|---|---|---|
| `clipboard` | **Flyway: Copy Next Migration Version** | The notification shows the next name instead. |
| `process:flyway` | Running Flyway commands with the CLI, and the migrations list | Commands and the **Flyway** tab ask you to allow it. |
| `process:mvn` | Running Flyway commands with Maven | Same, when **Run Flyway commands with** is `mvn`. |
| `process:java` | The Hibernate schema generator | The generator can't run. |

Creating and renaming files needs no permission.

## Troubleshooting

| Problem | What to do |
|---|---|
| "No Flyway migrations found in …" | Set **Migration locations** in Settings › Plugins › Flyway Migrations. `src/test/resources` is not searched. |
| "Running Flyway needs the process:flyway permission. Allow it in Settings › Plugins › Flyway Migrations." | Allow `process:flyway` or `process:mvn` in Settings › Plugins › Flyway Migrations. |
| "Next migration: … Copying it needs the clipboard permission…" | Allow `clipboard`, or copy the name from the notification. |
| "Flyway info did not return JSON" | Check **Extra arguments** and use Flyway 10 or newer. |
| The **Flyway** tab says there is no Info result | Press **Run Info…**. The list only fills after an explicit Info run. |
| The **Command Output** tab says there is no output | Run a Flyway command first. In managed mode, press **Refresh Output** to read new output. |
| Managed output stopped updating | Managed commands can end after about five minutes idle, or when the IDE closes. Use terminal mode for long work. |
| The Run menu has no **Flyway: info** | It needs a `flyway.toml` or `flyway.conf` in the root or `conf/`, and an empty **Extra arguments**. |
| A rename made outside the IDE isn't reflected | Run **Flyway: Check Migrations**. |
| Go to Definition on a placeholder does nothing | It needs a single root `flyway.conf` or `flyway.toml`. Spring overlays, custom delimiters and dynamic names are not followed. |

More: [repository troubleshooting](../../README.md#troubleshooting).

## Limitations

- No live database connection, live schema diff, or SQL generated from a running model.
- `src/test/resources` locations are not searched.
- **Extra arguments** can't hold quoted values with spaces, or custom placeholder prefixes and suffixes.
- Rename changes the file name only, not references or contents. No Rename refactoring for tables or placeholders.
- The comparison recipe supports only Flyway 13.12.1 Enterprise. Managed comparison runs stay unavailable.
- The plugin reads a text subset of the configuration, not Flyway's effective configuration.

## Technical details

<details>
<summary>Limits, caching and safety checks</summary>

### Generation and rename

- Generation and rename recheck project ownership, the source snapshot, naming and locations before returning file operations.
- Destinations stay inside configured locations. They cannot traverse paths or contain unresolved wildcards.
- Collision checks keep module, vendor and category rules, with normalized version equality.
- Java/Kotlin and callback creation: one non-overwriting undoable Create. Numeric versions are compared across SQL, Java and Kotlin in the same known module. Package and class names must be literal and not keywords. No files are read and no processes start while resolving.
- Rename refuses callbacks and unrelated SQL, same-name moves, existing or open-buffer targets, invalid names and unsaved source buffers.
- The New Migration form shows a target filename and SQL preview, and says when a template is empty. Selecting a source-linked migration seeds the directory. Stale or foreign selections can't open or seed another source.

### Commands and provenance

- Before launch, the review records the invoking project, approved runner, option-token keys and observed source paths. All argv and config values are hidden.
- Sources reviewed: four conventional Flyway config files, project-local `flyway.user.toml` overlays, up to 16 explicit owned `-configFiles=` (CLI) or `-Dflyway.configFiles=` (Maven) paths, and up to 32 discovered Spring application configs (64 KiB each). They are rechecked after confirmation.
- The target database is a hint read from `flyway.url` or `[environments.*] url` in the project's Flyway config files. A JDBC URL with a `${…}` placeholder or an Oracle `(DESCRIPTION=…)` address is not resolved. The CLI still resolves the effective configuration, environment and target.
- There is no unscoped "run twice" fallback for Clean.
- Native Run configurations (API 1.4) cover Info and Validate only. They use the owning project root as working directory. Custom `arguments` withdraw them.

### Managed mode

- Managed mode is finite, **instance-lifetime** ownership. Shutdown or about five minutes of idle time may end the child. Status is refreshed on demand, not streamed.
- **Refresh Output** reads up to 16 KiB per stream and completion. Rendering the view also polls the owned child, so finished output appears without another press.
- **Cancel Command** kills only the stored child. Neither exit nor cancel implies a database rollback.
- Status and cancel refuse another project's handle. A second start cannot silently replace a running child. A failed project-list query counts as an unavailable ownership check, not as proof of closure.
- Unresolved or foreign-owned sources suppress managed output.
- Output buffers complete lines per stream. Connection and auth lines, URLs and JDBC strings, control/ANSI sequences and oversized lines are suppressed. Known credential, URL and user literals are masked. This is conservative classification, not universal secret detection.
- Caps: 16 KiB per stream, 4 KiB per line, 1 MiB raw JSON (capped JSON is refused), extra argv 8 KiB / 128 tokens with whitespace tokenization. No bare additional goals: use `-key=value`.

### Migrations list (Info)

- **Run Info…** starts a managed JSON Info command (`-outputType=json`, or Maven `-Dflyway.outputType=json`) and opens **Command Output**.
- A successful JSON result updates the owning project's rows, labelled cached with capture age and provenance. Failures and cancellation never produce a fake success.
- Observed config, SQL and source-parent changes retire cached rows. Automatic render checks only project ownership and settings metadata: no CLI or HTTP.
- Accepted Info needs one migrations-bearing JSON object with unique keys. Maven banners are scanned within bounds. Conflicting roots, duplicate keys and error objects are rejected.

### Placeholder navigation

- Needs one canonical root configuration, literal in-project filesystem or main-resource classpath locations, and literal migration names. Competing files, Spring overlays, custom delimiters and non-ASCII, escaped or dynamic names give no result.
- Bounds: 128 KiB SQL / 32 KiB config, 512 declarations or references per file, 128 SQL files, 4,096 references per root, eight roots, at most 256 reverse locations.
- Facts expire after 120 seconds. Open, save or **Flyway: Check Migrations** warms them again.

### Table and entity navigation

- Schema-qualified names match exactly. Quoted identifiers (double quote, backtick, bracket) with doubled-delimiter escapes and per-dialect case folding are supported. The dialect comes only from an explicit datasource or a Flyway JDBC URL or driver; otherwise quoted names stay unresolved.
- Declarations, comments and string literals don't link. Malformed or oversized sources are withheld.
- Entities: literal Jakarta/javax `@Entity` + `@Table(name=…, schema=…)`, and literal `orm.xml` / `persistence.xml` mapping-file names. Kotlin named-argument classes and same-file constants are covered under the Spring Boot gate.
- Gutter icons use the same cache as Go to Definition, one marker per target on the line, at most 200 per line (the tooltip says how many were left out). A cold or expired cache gives no icon; open, save or **Flyway: Check Migrations** warms it.
- This is not runtime introspection or a Rename graph. The former document-index graph (table/JPA interchangeability, automatic indexed Rename and usages) is deliberately withheld.

### Comparison review

- The parser accepts the documented `differences[]` shape: unique ids, Add/Edit/Delete, literal definitions.
- Bounds: 1 MiB / 500 changes, 64 KiB per SQL side, 32 ids / 64 KiB selection. Banners, logs, error payloads and malformed input are rejected. Nothing is truncated into a "complete" preview.
- Rendering reads only the imported snapshot. The session is in memory and project-scoped. Report edits retire selections; stale reports clear visibly.
- **Show CLI Commands…** records exact argv arrays after a bounded confirmation: snapshot a schema model, compare against `empty` or a local snapshot, request `diffText` with explicit change IDs. It uses the `flyway` program, an explicit project working directory, concrete distinct in-project paths and absent outputs. Version and edition are self-reported, not probed. An imported report is not proven to belong to a future artifact's IDs.
- **Prepare Comparison…** captures up to 64 owned schema-model files (1 MiB total), an optional target snapshot and fresh output paths, and reviews the stage argv. After you confirm, it detects the installed CLI with `flyway --version` (optional `process:flyway`) and shows the edition, version, project configuration and locations.
- **Start Comparison** appears only when a CLI was detected. The stages (one child, closed stdin, separate snapshot/diff/diffText, explicit Check/Continue, bounded output, refused publication on nonzero exit, stderr, oversized or stale output) read the schema model and snapshot files only.

### Hibernate adapter

- An exact classpath and source inspection comes before the trusted-code approval. No application bootstrap or JDBC operation is requested.
- Before saving, without relaunching java, it rechecks jar sizes and dates, the class-root folders, the declared entity and converter class files, class files the project file search can see (gitignored build folders are not listed) and the declared sources. An undeclared class rebuilt in an ignored folder is only caught when the 15-minute preview expires.
- Live DB diff and automatic effective application models need separate support.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
