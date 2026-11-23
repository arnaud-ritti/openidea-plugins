# Liquibase Changelogs

Liquibase changelog support for XML, YAML, JSON and formatted SQL in OpenIDEA. It checks changelogs as you type, completes change types and attributes, lists changesets in execution order, and runs Liquibase commands when you ask. MIT; see `LICENSE`.

The plugin reads source text only. A file counts as a changelog when it is:

- an XML file with a `databaseChangeLog` root,
- a YAML or JSON file with a `databaseChangeLog` key, or
- a `.sql` file that starts with `--liquibase formatted sql` or contains a `--changeset` line.

Other files, such as a `pom.xml`, `application.yml` or a plain SQL script, get no checks and no completion.

## Requirements

- OpenIDEA with plugin API 1.14 or newer.
- To run Liquibase: the `liquibase` CLI on the `PATH` the IDE sees, and a `liquibase.properties` in the project root with `changeLogFile`, `url` and credentials.
- The offline snapshot diff targets Liquibase 4.29.2.
- Optional: `git`, to suggest your name as changeset author.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a project with Liquibase changelogs.
3. Open a changelog. Duplicate or incomplete changesets are underlined as you type.
4. Open the **Liquibase** tool window (bottom) to see every changeset in execution order.
5. To add a changeset, press ⌥⏎ in the changelog and choose **Add Liquibase changeset…**.

## Features

### Check changelogs as you type

The current text is checked, even unsaved. Hover shows facts about the changeset or include under the cursor. Explicit foreign XML namespaces are left alone.

| Code | Severity | Meaning |
|---|---|---|
| `duplicate-changeset` | error | A changeset with the same id and author is already in the file. The first one is linked. Ids compare as text, so `id: 1` and `id: "1"` are the same. |
| `incomplete-changeset` | error | A changeset without an id, an author, or both. In SQL the message shows `--changeset author:id`. |
| `unknown-change-type` | warning | A YAML or JSON entry under `changes` that isn't a built-in change type (`createTabel`). YAML and JSON only. |
| `missing-header` | warning | A formatted SQL changelog whose first non-blank line isn't `--liquibase formatted sql`. |
| `include-without-path` | error | An `include` or `includeAll` without a path. |
| `sqlfile-without-path` | error | A `sqlFile` without a `path` attribute. |
| `invalid-precondition-action` | error | A `preConditions` `onFail` or `onError` value that isn't allowed there. The root changelog accepts only `HALT` or `WARN`; a changeset accepts `HALT`, `CONTINUE`, `MARK_RAN` or `WARN`. |
| `unknown-precondition` | warning | An unknown XML precondition element. |
| `preconditions-shape` | warning | YAML `preConditions` that isn't a list. |
| `invalid-changeset-attribute` | warning | A formatted SQL changeset attribute that should be `true` or `false`. |
| `rollback-outside-changeset` | warning | A `--rollback` before the first `--changeset`. It belongs to no changeset and is ignored. |
| `unresolved-include` | error | An `include` or `includeAll` target that can't be found in the project. |
| `unresolved-sql-file` | error | A `sqlFile` path that can't be found in the project. |
| `unresolved-property` | warning | A `${name}` placeholder that no project changelog declares and the Liquibase configuration doesn't set as a parameter. Turn off with **Warn about undeclared properties**. |

A quoted SQL author with spaces (`--changeset "Jane Doe":2`) counts as one author.

### Browse changesets

The **Liquibase** tool window lists changesets in execution order.

- Every changelog that no other changelog includes is a root. An include cycle with no root starts from its first file.
- A changelog holds its changesets, labelled `id (author)` with its comment or first change type. Included changelogs nest where the `include` sits.
- `includeAll` expands to the changelogs under the directory, in path order.
- Flags: `already run` (reached a second time), `missing` (include not found), `unresolved` (include path has a `${property}`), `include cycle` (includes one of its ancestors; not expanded again).
- Double-click a row, or press Enter, to open the file at the changeset.
- **Filter…** searches rows and keeps their parents. **Clear** removes the filter. Filters belong to the current project.
- **Pending…** runs **Liquibase: Show Pending Changesets…** (see [Run Liquibase](#run-liquibase)).
- **More…** opens Choose Sources…, Browse All Changelogs, Update Database…, Rollback Count… and the offline diff. Database writes still ask for confirmation.
- **New…** opens the selected changelog so you can add a changeset with ⌥⏎.
- Browsing never runs Liquibase. The status line says `partial` when a size or time limit is hit.

### Find a changeset

Press Shift twice and open the **Liquibase Changesets** tab. Type part of a changeset id, its author or the changelog path to open the changelog at that changeset. In a large project the first queries show what has been read so far, and **Show more** reads further.

### Choose which changelog to browse

By default the tool window shows every changelog. To focus on one root:

1. Press **More…** › **Choose Sources…** in the toolbar, or run **Liquibase: Choose Sources…**.
2. Pick a changelog root, and optionally a configuration file.

The status line names the configuration file, the key and line it found, and whether that path matches the root. Supported keys: `changeLogFile`, `liquibase.command.changelogFile`, `liquibase.command.changeLogFile` and `spring.liquibase.change-log`.

Press **Browse All Changelogs** (or run **Liquibase: Browse All Changelogs**) to go back. The choice is saved per project (needs the `storage` permission). Cancelling saves nothing.

This choice only affects the tool window. It does not configure CLI commands or pick a database.

### Complete changelog content

**YAML** (keys depend on where the caret is):

| Under | Offers |
|---|---|
| `databaseChangeLog:` list item | `changeSet`, `include`, `includeAll`, `property` and the other top-level keys |
| `changeSet:` | `id`, `author`, `changes`, `rollback`, `runOnChange`, … |
| `changes:` and `rollback:` | the built-in change types |
| a change type | that change type's attributes |
| `columns:` / `column:` / `constraints:` | the column and constraint attributes |
| `include:` / `includeAll:` | their attributes |

- `changeSet` inserts a skeleton with id, author and `changes`.
- A key already followed by a colon is replaced without adding a second colon.

**JSON:** the plugin completes keys the same way. It replaces the current quoted property name, even in an unfinished document. Existing values are kept; new keys get a value placeholder. See [Limitations](#limitations).

**Formatted SQL:**

- Typing `--` offers `--liquibase formatted sql` on the first line. In a changelog it also offers `--changeset author:id`, `--rollback` and `--comment`.
- After `--changeset author:id `, attribute names are completed as `key:value`, including execution, context, label and delimiter options. Attributes already present are skipped.

**XML:** inside a `<changeSet>` only.

- After `<`: change types, `comment`, `rollback`, `column`, `constraints`.
- Attribute names of `changeSet`, `include`, `includeAll`, `column`, `constraints` and the change types.
- Quiet when the root declares a `schemaLocation`. The XML language server completes those files.

**Every format:**

- An `include` file value is completed with the project's changelog files (`.xml`, `.yaml`, `.yml`, `.json`, `.sql`).
- An `author` value is completed with the **Author** setting. When that is empty, it uses `git config user.name` (needs `process:git`).

Completion describes syntax. It does not enable licensed executors or check database-specific SQL.

### Jump to definitions

- **Includes:** Go to Definition on an `include` file value (XML, YAML or JSON) opens the included changelog; when the path matches files under several resource roots, a chooser lists each one. A gutter link on the `include` line does the same once the target is known. On a relative `includeAll` line, the gutter link lists every known changelog in that directory and its subdirectories (up to 200).
- **Properties:** Go to Definition opens a unique property declaration in the current changelog.
- **Rollback references:** a rollback with a literal `changeSetId` and `changeSetAuthor` jumps to that changeset in the same file.
- **Resource files:** `sqlFile.path`, `loadData.file` and `loadUpdateData.file` values open the file when `relativeToChangelogFile="true"`.

If a gutter link is missing, open the target file or refresh the **Liquibase** tool window.

### Create changesets and changelogs

Use ⌥⏎ in a changelog editor. Each action shows the exact change before applying it, and is one undoable step.

- **Add Liquibase changeset…** asks for an id and author, then inserts an empty changeset. Works in XML, YAML, JSON and formatted SQL.
- **Create Liquibase changelog…** creates a new XML, YAML, formatted SQL or JSON changelog from the bundled templates. From an XML, YAML or JSON parent, you can also add an `include` for it in the same step. The new file opens afterwards.
- **Include existing changelog…** adds an `include` to an existing changelog beside or below the parent.

**Liquibase: New Changeset** and **Liquibase: New Changelog…** run the same actions on the open changelog and apply them after you confirm. That is not a single undo step.

Generated changesets are empty skeletons. Fill them in and review them before running.

### Templates and icons

**New › Liquibase Changelog (XML)**, **(YAML)** and **(Formatted SQL)**. Files named `db.changelog-*`, `db.changelog.*` and `liquibase.properties` get the Liquibase icon.

### Run Liquibase

With a `liquibase.properties` in the project root, the Run menu offers **Liquibase: status** (`status --verbose`), **Liquibase: validate** and **Liquibase: update-sql**. They run from the project root and support Stop and Rerun.

- Set `searchPath` (the CLI's `--search-path`) to `src/main/resources`. Changesets then keep the file paths your application records.
- **Liquibase: Update Database…** runs `update` in a terminal after a final confirmation.
- **Liquibase: Rollback Count…** asks how many changesets to roll back, then confirms.
- **Liquibase: Show Pending Changesets…** (or **Pending…** in the tool window) runs `status --verbose` after the same confirmation. It changes nothing in the database but connects with your configured credentials. Each changeset is then marked `Pending`, `Applied` or `State unknown`, the status line reads `N pending changesets on <target> · checked <time>`, and changelog files in the Project tree get a `N pending` or `up to date` note.
  - `Applied` and `up to date` only cover the changelogs reached from `changeLogFile` in `liquibase.properties`. Other changesets are `State unknown`.
  - The result stays until a changelog or `liquibase.properties` changes, or a run fails. It is never refreshed on its own: run the command again after deploying.
- The confirmation is titled with the database and host, for example `Update app on db.example:5432?`. It reads them from the `url` in `liquibase.properties` (PostgreSQL, MySQL, MariaDB, SQL Server, Oracle, Db2, H2 and SQLite URLs) and also shows the changelog, the command and the project folder. URL parameters, usernames and passwords are never shown.
- When the database can't be read (no `liquibase.properties`, a `${variable}` or credentials in the URL), Update and Rollback Count also ask you to type the command name (`update` or `rollback-count`) before running.
- To see what Update will apply, run **Liquibase: Show Pending Changesets…** first.
- Database-writing commands are never Run configurations: you review them each time.

All of these need `process:liquibase`.

Once the project has a changelog or a `liquibase.properties`, Run consoles colour Liquibase's messages, from the CLI or from an application log (Spring Boot runs Liquibase at startup): `ERROR:` lines, `Unexpected error running Liquibase` and `Validation Failed` in red, successful commands, updates, validations and changesets in green, and `Running Changeset:` in cyan.

### Compare two offline snapshots

The **Offline Diff** tab compares two Liquibase JSON snapshot files in your project. No database is contacted.

1. Run **Liquibase: Run Offline Diff…** (or **Run Offline Diff…** in the tab).
2. Pick the reference and target snapshots, and the object types: tables, columns, indexes, views, or all four.
3. Review the exact command, then confirm.
4. Press **Refresh Status** to read progress. **Stop Diff** stops it.
5. Select a difference to see its detail, or **Choose Differences to Preview…** to view several. **Back to Report** returns to the list.

**Liquibase: Review Offline Snapshot Diff** reopens the last report. Both snapshots must be the same database type: h2, postgresql, mysql or mariadb.

The report is what Liquibase printed. It is not SQL or a changelog. Zero rows does not prove the snapshots match.

### Generate a changelog from a snapshot diff

1. In the **Offline Diff** tab, press **More Actions…** › **Generate Changelog from Snapshots…** and give an author.
2. Review the exact `diff-changelog` command, then confirm. Use **Refresh Status** until it finishes.
3. **More Actions…** › **Show Generated Changelog**, then **Choose Generated Changesets…**, picks the changesets to keep.
4. Optional: **More Actions…** › **Organize Migrations…** groups changesets, moves them between groups, or ignores and restores them.
5. In a project XML, YAML, JSON or SQL editor, press ⌥⏎ and choose **Save reviewed offline changesets…**. With several groups, choose **Save grouped offline migrations…** to save one file per group.

Each save creates new files in one undoable step and never overwrites. The generated XML is not redacted, so review it for sensitive defaults. Check order and dependencies before running it.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Liquibase**; bind keys in Settings › Keymap. No shortcuts are set by default.

| Command | What it does |
|---|---|
| **Liquibase: Choose Sources…** | Picks the changelog root (and config file) the tool window shows. |
| **Liquibase: Browse All Changelogs** | Goes back to showing every changelog. |
| **Liquibase: Update Database…** | Runs `liquibase update` in a terminal after confirmation. |
| **Liquibase: Show Pending Changesets…** | Confirms, runs `liquibase status --verbose`, then marks pending and applied changesets and notes changelog files. |
| **Liquibase: New Changeset** | Adds an empty changeset to the open changelog after you confirm (the ⌥⏎ **Add Liquibase changeset…** steps; not a single undo step). |
| **Liquibase: New Changelog…** | Creates the changelog file directly after you confirm, optionally included from the open parent (the ⌥⏎ **Create Liquibase changelog…** steps; not a single undo step). Without an open changelog it asks for a path (default `src/main/resources/db/changelog/db.changelog-master.yaml`) and creates a standalone changelog there. |
| **Liquibase: Rollback Count…** | Asks for a count, confirms, then runs `rollback-count`. |
| **Liquibase: Review Offline Snapshot Diff** | Opens the last offline diff report. |
| **Liquibase: Run Offline Diff…** | Compares two snapshot files after confirmation. |

Editor actions (⌥⏎): **Add Liquibase changeset…**, **Create Liquibase changelog…**, **Include existing changelog…**, **Save reviewed offline changesets…**, **Save grouped offline migrations…**.

## Settings

Settings › Plugins › Liquibase Changelogs.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Author offered when completing a changeSet author | `author` | empty | The name suggested for `author`. Empty: `git config user.name`. |
| Warn about undeclared properties | `checkProperties` | on | The `unresolved-property` warning. |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › Liquibase Changelogs.

| Permission | Used for | Without it |
|---|---|---|
| `process:liquibase` | Update, Rollback Count, Show Pending Changesets, Run configurations, offline diff and changelog generation | Commands ask you to allow it. Nothing falls back to an unreviewed terminal. |
| `process:git` | Suggesting `git config user.name` as author | No author is suggested unless **Author** is set. |
| `storage` | Remembering the chosen browser root | The choice isn't saved; browsing all changelogs still works. |
| `events` | Refreshing cached facts when files are opened, saved or changed | Checks on the current file still work; gutter links and hover may be missing until you refresh. |
| `editor.read`, `editor.write` | **Liquibase: New Changeset** and **New Changelog…** reading and editing the open changelog | Those commands say which permission to allow. ⌥⏎ still works. |
| `project.write` | **Liquibase: New Changelog…** creating the file | That command says which permission to allow. ⌥⏎ still works. |

## Troubleshooting

| Problem | What to do |
|---|---|
| "Running Liquibase needs the process:liquibase permission. Allow it in Settings › Plugins › Liquibase Changelogs." | Allow `process:liquibase` in Settings › Plugins › Liquibase Changelogs. |
| "No Liquibase changelogs in this project yet." | Press **New Changelog…** to create one, or check the file is a changelog (see the top of this page) and pick a root with **Choose Sources…**. |
| The status line says `partial` | The project has more changelogs than the scan limits. Choose a root with **More…** › **Choose Sources…**. |
| "The changelog you chose was changed, moved or deleted." | Press **Choose Sources…** to pick another, or **Browse All Changelogs**. |
| No completion in an XML changelog | The root declares a `schemaLocation`; the XML language server completes it instead. |
| An include has no gutter link | Open the included file, or refresh the tool window. Only literal relative paths get links; an `includeAll` link lists only the changelogs already known. |
| "Cannot find …" stays after creating the missing file | Edit or reopen the parent changelog. |
| "This host cannot ask for confirmation." | Use the **Liquibase: update-sql** or **status** Run configuration instead. |
| "Open a Liquibase changelog (XML, YAML, JSON or formatted SQL) first." | Focus a changelog editor, then run **Liquibase: New Changeset** again. |
| Offline diff: "belongs to another project" | Switch back to the project that started it. |
| "Could not start liquibase." | Install the Liquibase CLI and put it on the `PATH` the IDE uses, then run **Show Pending Changesets…** again. |
| "Liquibase status failed (exit …). Liquibase could not reach the database" | Start the database and check the `url` and credentials Liquibase uses. The plugin never shows the CLI output, which can contain them: run the **Liquibase: status** Run configuration to read it. |
| Every changeset says `State unknown` except pending ones | `changeLogFile` in `liquibase.properties` is missing, dynamic, or matches no changelog (or several) in the project. Use a literal path. |
| Pending notes disappeared | A changelog or `liquibase.properties` changed, so the result was stale. Run **Show Pending Changesets…** again. |

More: [repository troubleshooting](../../README.md#troubleshooting).

## Limitations

- XML files with a `schemaLocation` get completion from lemminx and the changelog's XSD, not from this plugin.
- JSON changelogs get the checks, the changesets view, Go to Definition and the project index. The completion provider is registered for XML, YAML and SQL only, so JSON completion may not appear. There is no run icon on the gutter marker.
- The gutter marker only navigates; the run icon is not used.
- Includes resolve against `src/main/resources`, `src/test/resources` or the project root. Other classpath layouts aren't modelled.
- Only open files are checked.
- Unknown change types are reported in YAML and JSON only, under `changes`.
- Run configurations take fixed arguments, so `rollback-count` and per-changelog runs aren't offered.
- No automatic table Rename or Find Usages.
- Duplicate changesets across included files, property precedence, contexts, extension schemas and database history are not checked.

## Technical details

<details>
<summary>Limits, caching and safety checks</summary>

### Browser source selection

- Choices persist in optional plugin storage under a concrete project-root identity. Anonymous contexts can't save or clear one.
- The configuration file gives source provenance only. Paths are compared with project, main-resource and test-resource paths. Multiple declarations, placeholders and profile/runtime precedence stay unresolved. URLs, usernames, passwords and other connection values are never shown or stored.
- Metadata or declaration changes show as changed selections; the current source stays browsable. Missing, unreadable, oversized or no-longer-recognized files stop selected browsing with a reselect/clear message.
- Corrupt stored records need explicit recovery. Records owned by another project are not reused, overwritten or deleted. Ambiguous decoded JSON owners are rejected before adoption, replacement or deletion.
- Modification timestamps have second resolution. The status reports matching metadata or unverified freshness, not byte equality.
- Bounds: root candidates use bounded XML/YAML/JSON/formatted-SQL parsing. Configuration candidates are at most 64 project `*liquibase*.properties` and `application*.properties/yaml/yml` paths. Saved paths are project-relative, at most 1,024 bytes; stored records at most 16 KiB.
- Discovery and reading: up to 2,000 source reads (2,000 changelogs), 512 KiB per file, three seconds. Tree depth up to 64; rows and include resolutions up to 4,096.
- The chosen root follows explicit includes lazily. The broader catalog is discovered only for `includeAll`, which reports partial results and never replaces the chosen root. Symlinks and unknown file kinds are unavailable.
- `includeAll` ignores `maxDepth`, `filter` and `resourceComparator`.
- Unresolved: active profiles, build/environment/property precedence, module dependency scopes, data-source association, runtime target resolution and CLI defaults-file binding.

### Live checks and hover

- Apart from the cross-file checks below, live diagnostics (API 1.3, versioned) and hover read only the current document. No file reads, discovery, configuration inspection, CLI, database or network. These are tolerant source checks, not full schema or Liquibase validation.
- Unresolved-include, unresolved-SQL-file and unresolved-property diagnostics answer from facts warmed by open, save and file-change events (one deadline per event batch; changelogs it doesn't reach are forgotten, not left stale). A changelog open before the plugin starts is warmed by its first diagnostics. With `events` denied, only the open file is warmed. An exhausted budget leaves facts unknown and the checks quiet; they never guess.
  - Includes resolve against the changelog's directory (with `relativeToChangelogFile`), else `src/main/resources` or `src/test/resources` (and, from a test changelog, `src/main/resources` too) when the changelog sits under one, then the project root. `classpath:` is stripped first.
  - An include isn't flagged when its path contains `${property}`, leaves the project root, is empty (half-typed), when an `includeAll` directory exists but is empty, when it sets `errorIfMissing: false` (`errorIfMissingOrEmpty: false`), or when the file exists but can't be read for a reason other than being missing.
  - `unresolved-property` is quiet when no config file can be read, when a scan cap is hit, when any changelog uses `includeAll`, an unresolvable include or `property file=`, when the configuration uses `defaultsFile` or `spring.config.import`/`location`/`activate`, for escaped (`$${x}`, `\${x}`), numeric or dynamic names, and inside comments. `database.*` names are built in. Parameters passed with `-D` or the environment are invisible.
- Hover shows changeset id/author, literal include options and, when unambiguous, where a property is declared through cached relative includes. It never shows property values.
- Include targets are positive cached facts only for literal relative paths. Classpath, absolute and dynamic paths, runtime precedence and missing targets stay unestablished. A relative `includeAll` gutter link lists the cached changelogs under its directory, in path order, excluding the changelog itself; it ignores `maxDepth`, `filter` and `resourceComparator`. Cold or expired targets have no gutter link.
- The cache belongs to exact verified project roots and full paths. It is warmed only from supplied documents, optional open/save events or already-read browser sources. Positive facts expire after 120 seconds. Files-changed events invalidate facts; current text replaces or removes a document's facts.
- Limits: 256 KiB per document, 4,096 parsed records per document, 1,024 placeholder markers and 8,192 XML tag markers, eight roots, 128 files and 4 MiB per root, 32 files / 128 include edges per property traversal, the first 256 include candidates for markers, 256 diagnostics. Browser warming: 128 files / 4 MiB / 30 ms between documents. Running out of the property edge budget withholds the property hover.
- Cached paths count only while the host's innermost project owner still matches the cache root. A nested project can't lend files to an outer root. Caches are partial; they never prove an include or property absent. Denied events or project access leave current-source checks working, with no cross-project fallback.

### Completion

- Core change attributes include sequence, procedure, lookup, merge, constraint-removal, remarks and CSV-update controls, checked against the public latest XSD.
- Include paths are written relative to the changelog with `relativeToChangelogFile`, otherwise relative to `src/main/resources` or the project root. The list comes from the host's file search, which skips hidden directories and ignored paths.
- Author completion covers the XML attribute, the YAML key and the `--changeset ` line before the colon.

### Navigation

- Include navigation validates the full current XML document (including legacy unnamespaced core changelogs) before scanner-backed navigation. Malformed declarations, attributes or namespaces and opaque `xml:base` or `logicalFilePath` owners stay unresolved. Cold, dynamic and classpath candidates are still available through direct navigation.
- Property definitions work in namespaced XML, validated YAML/JSON and formatted SQL. Conditional precedence, inherited values and runtime usage are unknown.
- Rollback references navigate only with no includes, opaque logical paths or unknown identities. Explicit `changeSetPath`, duplicate names, dynamic values and ambiguous scanner paths stay quiet.
- Resource file navigation covers direct core XML changeset children and YAML/JSON `changes` entries. The source must be structurally valid, namespace-supported and free of `xml:base`/`logicalFilePath` ambiguity. YAML/JSON needs a literal Boolean `true` and uniquely located unescaped scalars (aliases, tags and duplicate keys are refused). It uses the physical changelog directory, needs an existing file in the same innermost project, and refuses project escapes, denied reads, substitutions, encoded aliases and URI/search-path interpretation. SQL and CSV contents are never read or run. Rollback-nested resource changes are not covered.
- No replaceable index anchors are published. A file-local table name can't prove a closed set of references, so automatic table Rename and Find Usages are withheld. The index export returns an empty graph; existing persisted symbols disappear when the host reindexes.

### Generators

- Generation runs through editor code actions because the SDK applies FileActions only through intentions; there is no command-side apply.
- **Add Liquibase changeset…** shows the exact file, editor version, UTF-16 range and insertion before returning one undoable edit. XML supports the standard `http://www.liquibase.org/xml/ns/dbchangelog` namespace, including prefixed and self-closing roots. Formatted SQL needs its header and safely representable author/id values. Existing incomplete or dynamic identities and duplicate id/author pairs in the file are refused.
- YAML/JSON appends preserve the source: strict parsing rejects duplicate keys, aliases, tags, multiple documents and depth/event limits before editing. Block/flow sequences, Unicode and CRLF are preserved. The result is reparsed before returning an edit. Existing bytes and comments are not rewritten.
- **Create Liquibase changelog…** uses escaped literal values. Linked creation previews the new contents and parent include edit, then returns one transaction: a non-overwriting Create plus the parent edit. If either fails, neither applies. From other editor formats, creation is standalone.
- **Include existing changelog…** refuses a child that has includes or opaque ownership, rather than claiming to resolve an arbitrary include graph or cycle.
- Each action rechecks the owning project and the full current source before and after prompts. Destinations can't leave the project or overwrite files, directories or open buffers. Duplicate includes, `includeAll`, unresolved include paths, `xml:base` and `logicalFilePath` ambiguity block linked editing.
- Namespace-less, foreign-namespace, malformed and DTD-based XML is not edited. Namespace admission checks qualified names (XML 1.0 NCName ranges), reserved XML/XMLNS bindings, reserved processing-instruction targets and duplicate expanded attribute names. Prefixed namespace undeclarations and declarations after the root are outside the supported subset and withhold edits.
- Source editing is bounded to 256 KiB, 50,000 XML events and depth 64.
- Resolution uses only pick/input/confirm prompts and read APIs. It never launches Git, reveals a view or writes a file while resolving. Cancelling changes nothing; the host owns atomic application and undo.
- This is namespace, well-formedness and local identity validation, not full XSD, effective root configuration or database-history validation.

### Run configurations and terminal commands

- Run configurations (API 1.4) appear for an owned open project with a root `liquibase.properties`. The CLI resolves the effective target and credentials.
- **Show Pending Changesets…** uses the same review, then runs `status --verbose` as a captured process (not a terminal) for up to 60 seconds. It lists unrun changesets as `path::id::author`; output that doesn't match the count line exactly is rejected. Paths match a project changelog by unique path suffix (`classpath:` dropped). Contexts and labels follow your configuration, so changesets they filter out can show as `Applied`.
- **Update Database…** and **Rollback Count…** keep their cancel, count and final-confirmation steps and exact terminal argv. The confirmation shows the database parsed from the root `liquibase.properties` (parameters dropped; URLs with user info or variables are not shown), the changelog, the command and the project folder. A changed `liquibase.properties` between the confirmation and the run cancels it.
- A unique literal changelog and a recognized PostgreSQL/MySQL/MariaDB JDBC host/database are shown as hints only. Query/semicolon parameters and credential-bearing, ambiguous or dynamic values are withheld. Raw configuration, usernames and passwords are never shown.
- The effective database, contexts and labels stay unresolved: environment, JVM/defaults/extension precedence and unsaved-versus-disk differences can override hints. Missing project scope or configuration drift across prompts refuses startup. Root configuration text is bounded to 128 KiB.

### Offline snapshot diff

- Snapshots need the same supported shortName (h2/postgresql/mysql/mariadb), a recognized core object-map/array structure, safe project-relative paths, and at most 128 KiB / 4,096 objects each. JSON decoding rejects duplicate keys, depth over 64, more than 50,000 values and trailing data before any staging or process work. Discovery is bounded to 128 JSON candidates / two seconds; unknown snapshots are not inferred absent.
- No online URL, username, password, datasource selection, JPA model or live snapshot command is offered.
- The argument and report contract is the Liquibase v4.29.2 text diff. The installed CLI binary, version and edition are not verified.
- Before start, the confirmation shows project, source paths, editor-text SHA256, program and argv. The reviewed text (including unsaved buffers) is copied into owner-only plugin-private staging as UTF-8 JSON with connection identity fields removed. Empty private defaults, UTF-8 output and a private search path are supplied. Project files are unchanged.
- Command-specific URL/diff-type environment variables (and output/author for generation) are reserved to the reviewed values. A collision with an inherited key is rejected before spawning. Source, root and staged-byte membership are rechecked before start, during **Refresh Status** and before accepting output.
- The process inherits environment, JVM and extensions. Process permission is not a database, network or filesystem sandbox. Denial or cancellation never falls back to an unscoped terminal command.
- One instance-owned child at a time, stdin closed. **Refresh Status** reads up to eight output chunks per click. **Stop Diff** kills only the owning project's child. A 120-second polling lease and idle reclamation can also end it. No automatic polling, durable job or live tailing.
- Output limits: 64 KiB combined stdout/stderr, 512 observations, 1,024-byte labels, 4,096-byte details. Oversized output is rejected and the last good report kept. Raw stderr is withheld; known snapshot connection strings are redacted.
- The text adapter accepts only requested Missing/Unexpected/Changed headers, known indentation and bounded shapes. Unknown, duplicate or missing categories keep the old report. Combined preview is up to 128 KiB. Picker IDs bind the owning report, root, content and generation; cancelled, forged, duplicate or stale keys keep the prior review. No object name is turned into inferred DDL. Rendering and selection never run or poll Liquibase.
- Staged inputs are overwritten empty on completion, Stop and observed failure, only while their bytes still match. Changed or unreadable slots are kept with a cleanup warning. A crash or reclaim may leave owner-only copies until manual cleanup or uninstall. There is no SDK delete: at most 96 staging files (32 input sets). No snapshot or report content is saved to settings or user files.

### Generated changelogs

- **Generate Changelog from Snapshots…** reuses the two snapshot choices and categories, asks for a literal author, and previews the `diff-changelog` argv, input SHA256 and a new private XML output path. A private output that already exists is refused. It requests offline source generation, never update/rollback or a project output path. The same child, staging, Refresh Status/Stop Diff, limits and foreign-project rules apply.
- Accepted output: UTF-8 core Liquibase XML, at most 128 KiB / 512 unique literal changesets / 8,192 elements, known 4.29 change types, the requested author, and no opaque `logicalFilePath`, external-file/include/property dependencies, DTDs, custom execution or foreign elements. Missing, empty, invalid or oversized output keeps the previous candidate and never proves no differences.
- The stock serializer writes an XML 1.1 declaration. The reader accepts its XML 1.0-compatible subset, keeps exact source bytes and rejects XML 1.1-only line/control semantics.
- **Choose Generated Changesets…** keeps complete fragments in original order, with root namespace declarations, comments and untouched bytes. Dependencies across omitted changesets, SQL correctness, runtime parameters and full schema coverage are not inferred. The candidate lives in memory only; reload or reclamation discards it. A changed snapshot, selection or owner makes an old save action stale.
- **Organize Migrations…**: a changeset belongs to at most one group; each group keeps source order. Operation-class selection splits add/create, drop/delete and other/SQL by the serialized change names (lexical classes, not a risk score or dependency analysis). Grouping starts no CLI and writes no files.
- Saving rechecks the cached selection, project, snapshots, editor and destination before and after prompts, and returns non-overwriting FileAction Creates (never a direct write; the CLI never runs on selection or save). Confirmations over 16 KiB are refused: choose a smaller subset. Grouped save asks for distinct XML paths and returns all Creates in one atomic action. Changed group names, selection or source invalidate old save actions. Ignored changesets are excluded.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
