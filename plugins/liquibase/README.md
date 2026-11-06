# Liquibase Changelogs

Liquibase changelog support for XML, YAML and formatted SQL in OpenIDEA. The
plugin reads the source text only. A file is treated as a changelog when it is
an XML file with a `databaseChangeLog` root, a YAML file with a
`databaseChangeLog` key, or a `.sql` file that starts with `--liquibase
formatted sql` or contains a `--changeset` line. Other files, such as a
`pom.xml`, `application.yml` or a plain SQL script, get no checks and no
completion.

Change type and attribute names come from the Liquibase 4.29 XSD (Apache-2.0).
This is an independent implementation, not affiliated with Liquibase Inc. or
JetBrains.

## Liquibase Changesets view

The bottom panel lists the project's changesets in execution order. Every
changelog that no other changelog includes is a root node (an include cycle with
no root is shown from its first file). A changelog node holds its changesets,
labelled `id (author)` with its comment or first change type, and the changelogs
it includes, nested where the `include` sits. `includeAll` expands to the
changelogs under the directory, in path order (`maxDepth`, `filter` and
`resourceComparator` are ignored). A changelog reached a second time is a leaf
flagged `already run`. Double-click a node to open the
file at the changeset. A missing include is a node flagged `missing`, an include
with a `${property}` is flagged `unresolved`, and a file that includes one of its
ancestors is flagged `include cycle` and not expanded again. Nothing is run:
the view reads the text of XML, YAML and formatted SQL changelogs (up to 2000
changelogs and 3 seconds; when either limit hits, the status line says
`partial`).

## Checks

Changelogs are checked as you type, without saving.

| Code | Severity | Meaning |
|---|---|---|
| `duplicate-changeset` | error | A changeset with the same id and author is already defined in the file. The first definition is linked. Ids compare as text, so `id: 1` and `id: "1"` are the same. |
| `incomplete-changeset` | error | A changeset without an id, without an author, or without both. In SQL the message shows `--changeset author:id`. |
| `unknown-change-type` | warning | A YAML entry under `changes:` that isn't a built-in Liquibase change type (`createTabel`). |
| `unresolved-include` | error | An `include` file or an `includeAll` directory that can't be found. Not reported when `errorIfMissing` (`errorIfMissingOrEmpty` for `includeAll`) is `false`. |
| `missing-header` | warning | A formatted SQL changelog whose first non-blank line isn't `--liquibase formatted sql`. |

A quoted SQL author with spaces (`--changeset "Jane Doe":2`) is one author.

Includes are resolved as the changelog's directory (with
`relativeToChangelogFile`), or else against `src/main/resources` or
`src/test/resources` (and, from a test changelog, `src/main/resources` too) when the
changelog sits under one, then the project root.
`classpath:` is stripped first. An include is not flagged when:

- its path contains a `${property}`;
- its path leaves the project root;
- its path is empty, because it is half-typed;
- an `includeAll` directory exists but is empty (only a missing directory is reported);
- it sets `errorIfMissing: false` (`errorIfMissingOrEmpty: false`);
- the file exists but can't be read for any reason other than being missing.

## Completion

In YAML changelogs, keys are offered by where the caret is:

| Under | Offers |
|---|---|
| `databaseChangeLog:` list item | `changeSet`, `include`, `includeAll`, `property` and the other top-level keys |
| `changeSet:` | `id`, `author`, `changes`, `rollback`, `runOnChange`, … |
| `changes:` and `rollback:` | the built-in change types |
| a change type | that change type's attributes |
| `columns:` / `column:` / `constraints:` | the column and constraint attributes |
| `include:` / `includeAll:` | their attributes |

`changeSet` inserts a skeleton with id, author and `changes`. A key that is
already followed by a colon is replaced without adding a second one.

In formatted SQL, typing `--` offers `--liquibase formatted sql` on the first
line and, in a changelog, `--changeset author:id`, `--rollback` and
`--comment`. XML gets no completion from this plugin.

## Definition

Go to Definition on the value of an `include` file (XML or YAML) opens the
included changelog. `includeAll` directories have no single target.

## Templates

New › Liquibase Changelog (XML), (YAML) and (Formatted SQL).

## Run configurations

Four configurations appear for a project with a `liquibase.properties` at its
root: `status --verbose`, `validate`, `update-sql` (a preview that changes
nothing) and `update` (writes to the database). They need:

- the `liquibase` CLI on PATH;
- the `process:liquibase` permission, granted in Settings › Plugins;
- `changeLogFile`, `url` and the credentials in `liquibase.properties`.

Set `searchPath` (the CLI's `--search-path`) in `liquibase.properties` to
`src/main/resources`, so changesets keep the file paths your application
records, rather than paths relative to wherever the CLI runs.

## Limits

- XML completion comes from lemminx and the changelog's XSD, not from this
  plugin. Only the checks above run on XML.
- JSON changelogs aren't supported.
- Includes resolve against `src/main/resources`, `src/test/resources` or the
  project root. Other classpath layouts aren't modelled.
- Only open files are checked.
- After you create a missing included file, the master's "Cannot find" stays until the master is edited or reopened.
- Unknown change types are reported in YAML only, under `changes:`.
- Run configurations take fixed arguments, so `rollback-count` and per-changelog
  runs aren't offered.

```sh
cargo test                       # logic, against the SDK's mock host
openidea-plugin build            # → dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings › Plugins)
```
