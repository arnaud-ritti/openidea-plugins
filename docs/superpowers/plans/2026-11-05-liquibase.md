# Liquibase Changelogs (`aritti.liquibase`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An OpenIDEA plugin that checks, completes and links Liquibase changelogs (XML, YAML, formatted SQL), adds changelog templates and offers Liquibase CLI run configurations.

**Architecture:** One Rust provider plugin. `changelog.rs` turns a document into a small model (changesets, includes, change types). It reuses the XML scanner and YAML key walker in `crates/spring-text`. `checks.rs` turns the model into diagnostics. These are published from the `line-markers` provider call, the same live-check workaround `plugins/spring-boot` uses. `complete.rs` serves YAML and formatted-SQL completion. Go to Definition resolves `include` paths. The CLI, file templates and run configurations are manifest data only.

**Tech Stack:** Rust 2024, `openidea-plugin` SDK `v0.1.37` (WIT `openidea:plugin@1.1.0`), `spring-text` (path dependency), `wasm32-wasip2`.

**Spec:** the _Scope_ section below. There is no separate spec document.

## Global Constraints

- Plugin id `aritti.liquibase`, directory `plugins/liquibase/`, display name `Liquibase Changelogs`, crate `openidea-plugin-aritti-liquibase`, `version = "0.1.0"`, `license = "MIT"`, `categories = ["tools", "linters"]`, `engines.openidea = "^1.0"`.
- Clean-room: the only inputs are docs.liquibase.com, the Liquibase **4.29** XSD (`dbchangelog-4.29.xsd`, Apache-2.0) and the Marketplace page text. Never download or decompile JetBrains jars, and never copy JetBrains UI strings or names. Liquibase 5.x is FSL-1.1-ALv2 (not open source), so nothing is taken from 5.x artifacts. Only change-type and attribute *names* are taken from the 4.29 XSD; the XSD itself is not embedded.
- `description` ends with: `Independent implementation, not affiliated with Liquibase Inc. or JetBrains.`
- SDK dependency: `openidea-plugin = { git = "https://github.com/l0ng-ai/openidea", tag = "v0.1.37" }` (the same as every plugin in the repo), plus `spring-text = { path = "../../crates/spring-text" }`. No other crates.
- Capabilities: `required = ["project.read"]` only. `process:liquibase` comes from the run detectors (optional by construction).
- cargo is not on PATH: use `~/.cargo/bin/cargo` and `~/.cargo/bin/openidea-plugin`. Every command runs from `/Users/aritti/Projects/openidea-plugins/plugins/liquibase` (`cd` in the same Bash call; the shell's cwd resets).
- Internal offsets are UTF-8 bytes. Convert with `spring_text::range(text, from, to)` and `openidea_plugin::text::{offset, position}`.
- Work on branch `liquibase` (`git switch -c liquibase` before Task 1). Commit only `plugins/liquibase/` (and `docs/` if asked). `LIMITATIONS.md` is untracked: edit it in Task 6, but don't commit it without the user's say-so.
- Code style: the user's guidelines apply. Early returns, no `else` after `return`, no comments that restate code. Doc comments on items follow the style of `crates/spring-text`.

## Review Focus

1. A Spring Boot include written `classpath:db/changes/001.yaml`, or a non-relative include in a changelog under `src/main/resources/`, must resolve against `src/main/resources`, not the changelog's directory. A false "Cannot find" on every include is the failure users would see first. (Task 2 `include_candidates`, Task 3 `reports_yaml_changelog_problems`.)
2. Include paths with Liquibase properties (`${base}/x.yaml`) must never be flagged. (Task 2 `include_candidates`, Task 3 fixture.)
3. Ordinary `pom.xml`, `application.yml` and plain `.sql` files must get no diagnostics and no completion. The providers see every XML/YAML/SQL file. (Task 2 `ignores_other_files`, Task 3 `stays_silent_outside_changelogs`, Task 5 asserts.)
4. `id: "1"` and `id: 1` are the same id, and a quoted SQL author with spaces (`--changeset "Jane Doe":2`) is one author. (Task 2 tests, Task 3 duplicate test.)
5. Half-typed documents (every prefix of a changelog) must not panic in parse or completion. A panic kills the instance, and three in a row mark the plugin Failed. (Task 2 `survives_every_prefix`, Task 5 `completion_survives_every_prefix`.)

---

## Scope

**In (v0.1.0):**

- **Changelog detection by content, not by name.** These count as changelogs:
  - `.xml` whose root element is `databaseChangeLog`;
  - `.yaml`/`.yml` with a top-level `databaseChangeLog` key;
  - `.sql` whose first non-blank line is `--liquibase formatted sql`, or that has a `--changeset` line.
- **Diagnostics, re-checked live on every document version:**
  - `duplicate-changeset`: the same `id` and `author` twice in one file. Liquibase's identity is id + author + file path, so only one file needs checking.
  - `incomplete-changeset`: no `id` and/or no `author`.
  - `unknown-change-type`: YAML only, a warning.
  - `unresolved-include`: an `include file` or `includeAll path` that points nowhere.
  - `missing-header`: a formatted SQL file without the header.
- **Go to Definition** on an `include` `file` value (XML and YAML).
- **YAML completion:** top-level entries, changeSet attributes, change types under `changes`/`rollback`, the attributes of common change types, `column` and `constraints` attributes, `include`/`includeAll` attributes. `changeSet` is a snippet with `id`, `author` and `changes`.
- **Formatted SQL completion:** `--liquibase formatted sql` on line 0, then the `--changeset author:id` snippet, `--rollback` and `--comment`.
- **New menu templates:** Liquibase Changelog (XML, YAML, formatted SQL).
- **Run configurations (`runDetectors`, data only):** `liquibase status --verbose`, `validate`, `update-sql` and `update`, offered in projects with `liquibase.properties` at the root. `update` is named as writing to the database.

**Dropped, and why:**

- **XML completion and the unknown-element check:** lemminx already provides both from the XSD that every changelog names in `xsi:schemaLocation`; plugin items would show up twice. The XML template names the XSD so this works for new files too.
- **JSON changelogs:** rarely used, and nothing in the repo scans JSON with byte offsets (`serde_json` gives no spans). Add when asked.
- **Detection through `spring.liquibase.change-log` / `changeLogFile`:** content detection finds every changelog, including included ones that no setting names. The settings only matter for running Liquibase, and the CLI reads `liquibase.properties` itself.
- **Find Action commands (`process:liquibase` + `open_terminal`), `rollback-count`, Maven/Gradle goals:**
  - The run detectors cover the CLI with no code.
  - `rollback-count` needs a count and a confirmation, and the API has no input or confirm dialog.
  - The Maven/Gradle goals need plugin configuration in the build file, and run through the IDE's build tooling already.
- **Clipboard snippet generator:** the completion snippets and the New-menu templates cover it without the `clipboard` capability.
- **Checks of `sqlFile`/`loadData` paths, quick fixes (rename a misspelled change type, add the SQL header) and database-aware features (diff, generate-changelog, table/column completion):**
  - the checks and quick fixes are YAGNI for 0.1.0;
  - the database-aware features need a database API, which plugins don't have.

## File Structure

```
plugins/liquibase/
  plugin.toml            # manifest: providers, run detectors, templates
  Cargo.toml             # scaffolded; + spring-text path dependency
  .gitignore             # scaffolded (/target)
  LICENSE                # MIT, Arnaud RITTI
  NOTICE                 # names from the Liquibase 4.29 XSD (Apache-2.0), trademark note
  README.md              # what it does, limits, commands
  templates/changelog.xml
  templates/changelog.yaml
  templates/changelog.sql
  src/lib.rs             # Liquibase plugin struct, Providers glue, include resolution against the host
  src/catalog.rs         # static tables: change types and attribute names
  src/changelog.rs       # parse(): document → Changelog model; candidates(): include paths
  src/checks.rs          # problems(): Changelog → Vec<Diagnostic>
  src/complete.rs        # yaml() and sql() completion
```

Reused, not rewritten:
- `spring_text::xml::{elements, root}` (elements with attribute value spans);
- `spring_text::config::{yaml, yaml_parent}` (YAML key paths like `databaseChangeLog[1].changeSet.changes[0].createTable`, value spans, and the parent of the key being typed);
- `spring_text::index::project_of`;
- `spring_text::range`.

The 4-line `line_bounds` helper is copied from `plugins/spring-boot/src/lib.rs:174`.

---

### Task 1: Package skeleton and change-type catalogue

**Files:**
- Create (scaffold): `plugins/liquibase/{plugin.toml,Cargo.toml,.gitignore,README.md,src/lib.rs}`
- Create: `plugins/liquibase/{LICENSE,NOTICE}`, `plugins/liquibase/templates/changelog.{xml,yaml,sql}`, `plugins/liquibase/src/catalog.rs`

**Interfaces:**
- Produces: `catalog::{TOP_LEVEL, CHANGE_SET, INCLUDE, INCLUDE_ALL, COLUMN, CONSTRAINTS, LISTS, CHANGE_TYPES, PRO_CHANGE_TYPES}` (all `&[&str]`, except `CHANGE_TYPES: &[(&str, &[&str])]`), `catalog::change_type(name: &str) -> Option<&'static [&'static str]>` (the attributes of a community type), `catalog::is_change_type(name: &str) -> bool` (community or Pro); `pub struct Liquibase;` (`#[derive(Default)]`) in `lib.rs` implementing `Plugin` (`run_command` → `Err(Error::NotFound(format!("command {id}")))`) and `Providers` (defaults for now), `register_plugin!(Liquibase, providers);`.

- [ ] **Step 1: Scaffold**

Run: `cd /Users/aritti/Projects/openidea-plugins && ~/.cargo/bin/openidea-plugin new plugins/liquibase --id aritti.liquibase --name "Liquibase Changelogs"`
Then, in `Cargo.toml`, set the SDK tag to `v0.1.37`, add `spring-text = { path = "../../crates/spring-text" }` and `publish = false`, and copy `[profile.release]` from `plugins/jpa-assist/Cargo.toml` (adds `codegen-units = 1`). Add `mod catalog;` to `src/lib.rs`.

- [ ] **Step 2: Write the manifest**

`plugin.toml` (exact):

```toml
id = "aritti.liquibase"
name = "Liquibase Changelogs"
version = "0.1.0"
description = "Liquibase changelog support for XML, YAML and formatted SQL. Flags duplicate and incomplete changesets, unknown change types, missing includes and missing formatted SQL headers while you type; completes change types and their attributes in YAML and changeset comments in SQL; Go to Definition on include; New › Liquibase Changelog templates; status, validate, update-sql and update run configurations for projects with a liquibase.properties. Independent implementation, not affiliated with Liquibase Inc. or JetBrains."
authors = ["Arnaud RITTI <arnaud.ritti@gmail.com>"]
license = "MIT"
change-notes = "0.1.0: first release. Live checks (duplicate and incomplete changesets, unknown change types in YAML, unresolved include and includeAll, formatted SQL header), YAML and formatted SQL completion, Go to Definition on include, changelog templates and Liquibase CLI run configurations. Change type and attribute names from the Liquibase 4.29 XSD (Apache-2.0)."
categories = ["tools", "linters"]
engines.openidea = "^1.0"
wasm = "dist/plugin.wasm"
activation = ["on-language:xml", "on-language:yaml", "on-language:sql"]

[capabilities]
required = ["project.read"]

[[contributes.providers]]
kinds = ["line-markers"]
languages = ["xml", "yaml", "sql"]

[[contributes.providers]]
kinds = ["definition"]
languages = ["xml", "yaml"]

[[contributes.providers]]
kinds = ["completion"]
languages = ["yaml", "sql"]

[[contributes.runDetectors]]
id = "status"
name = "Liquibase: status"
files = ["liquibase.properties"]
command = "liquibase"
args = ["status", "--verbose"]

[[contributes.runDetectors]]
id = "validate"
name = "Liquibase: validate"
files = ["liquibase.properties"]
command = "liquibase"
args = ["validate"]

[[contributes.runDetectors]]
id = "update-sql"
name = "Liquibase: update-sql (preview, no changes)"
files = ["liquibase.properties"]
command = "liquibase"
args = ["update-sql"]

[[contributes.runDetectors]]
id = "update"
name = "Liquibase: update (writes to the database)"
files = ["liquibase.properties"]
command = "liquibase"
args = ["update"]

[[contributes.fileTemplates]]
id = "changelog-xml"
name = "Liquibase Changelog (XML)"
extension = "xml"
path = "templates/changelog.xml"

[[contributes.fileTemplates]]
id = "changelog-yaml"
name = "Liquibase Changelog (YAML)"
extension = "yaml"
path = "templates/changelog.yaml"

[[contributes.fileTemplates]]
id = "changelog-sql"
name = "Liquibase Changelog (Formatted SQL)"
extension = "sql"
path = "templates/changelog.sql"
```

The run detector fields and the root-only marker match are in `crates/ide-host/src/plugins/run_detectors.rs:36-45,134-140` (upstream checkout `d3d0fd2`). The template variables `$USER$` and `$END$` are listed in `src/features/templates/files/model.ts:1-11`.

- [ ] **Step 3: Templates, LICENSE, NOTICE (exact copy)**

`templates/changelog.xml`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<databaseChangeLog
        xmlns="http://www.liquibase.org/xml/ns/dbchangelog"
        xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
        xsi:schemaLocation="http://www.liquibase.org/xml/ns/dbchangelog
                            https://www.liquibase.org/xml/ns/dbchangelog/dbchangelog-latest.xsd">

    <changeSet id="1" author="$USER$">
        $END$
    </changeSet>
</databaseChangeLog>
```
`templates/changelog.yaml`:
```yaml
databaseChangeLog:
  - changeSet:
      id: 1
      author: $USER$
      changes:
        - $END$
```
`templates/changelog.sql`:
```sql
--liquibase formatted sql

--changeset $USER$:1
$END$
```
`LICENSE`: the MIT text of `plugins/kube-explorer/LICENSE` (Copyright (c) 2026 Arnaud RITTI).
`NOTICE`:
```
Liquibase Changelogs
Copyright (c) 2026 Arnaud RITTI. Licensed under the MIT License (LICENSE).

The change type and attribute names in src/catalog.rs were taken from the
Liquibase 4.29 changelog schema (dbchangelog-4.29.xsd), Copyright Liquibase
Inc., licensed under the Apache License 2.0. The schema itself is not included.

Liquibase is a trademark of Liquibase Inc. This plugin is not affiliated with
or endorsed by Liquibase Inc. or JetBrains.
```

- [ ] **Step 4: Write the failing catalogue test** (in `src/catalog.rs`, `#[cfg(test)] mod tests`)

```rust
#[test]
fn knows_every_change_type_once() {
    let mut names: Vec<&str> = CHANGE_TYPES.iter().map(|(n, _)| *n).chain(PRO_CHANGE_TYPES.iter().copied()).collect();
    assert_eq!(CHANGE_TYPES.len(), 47);
    assert_eq!(PRO_CHANGE_TYPES.len(), 18);
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 65);
    assert!(change_type("createTable").unwrap().contains(&"tableName"));
    assert!(change_type("createTable").unwrap().contains(&"columns"));
    assert_eq!(change_type("createTrigger"), None);
    assert!(is_change_type("createTrigger"));
    assert!(!is_change_type("createTabel"));
}
```

- [ ] **Step 5: Run it to see it fail**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test knows_every_change_type_once`
Expected: compile error (`CHANGE_TYPES` not found).

- [ ] **Step 6: Implement `src/catalog.rs` (data fixed by the spec, exact)**

```rust
//! Change types and attribute names of Liquibase changelogs, from the
//! Liquibase 4.29 XSD (Apache-2.0, see NOTICE). Attributes are listed for
//! the common change types only; the rest are completed by name.

pub const TOP_LEVEL: &[&str] = &["changeSet", "include", "includeAll", "property", "preConditions"];
pub const CHANGE_SET: &[&str] = &[
    "id", "author", "changes", "rollback", "comment", "preConditions", "validCheckSum", "context", "contextFilter",
    "labels", "dbms", "runOnChange", "runAlways", "failOnError", "runInTransaction", "logicalFilePath", "created", "ignore",
];
pub const INCLUDE: &[&str] = &["file", "relativeToChangelogFile", "errorIfMissing", "context", "labels", "ignore"];
pub const INCLUDE_ALL: &[&str] = &[
    "path", "relativeToChangelogFile", "errorIfMissingOrEmpty", "filter", "minDepth", "maxDepth", "endsWithFilter",
    "context", "labels", "ignore",
];
pub const COLUMN: &[&str] = &[
    "name", "type", "value", "valueNumeric", "valueBoolean", "valueDate", "valueComputed", "defaultValue",
    "defaultValueNumeric", "defaultValueBoolean", "defaultValueDate", "defaultValueComputed", "autoIncrement",
    "startWith", "incrementBy", "remarks", "constraints",
];
pub const CONSTRAINTS: &[&str] = &[
    "nullable", "primaryKey", "primaryKeyName", "unique", "uniqueConstraintName", "foreignKeyName", "references",
    "referencedTableName", "referencedColumnNames", "deleteCascade", "notNullConstraintName", "checkConstraint",
    "deferrable", "initiallyDeferred",
];
/// Keys whose YAML value is a sequence: completed as `key:` plus a `- ` item.
pub const LISTS: &[&str] = &["changes", "rollback", "columns", "preConditions"];

pub const CHANGE_TYPES: &[(&str, &[&str])] = &[
    ("addAutoIncrement", &[]),
    ("addColumn", &["catalogName", "schemaName", "tableName", "columns"]),
    ("addDefaultValue", &["catalogName", "schemaName", "tableName", "columnName", "columnDataType", "defaultValue", "defaultValueNumeric", "defaultValueBoolean", "defaultValueDate", "defaultValueComputed"]),
    ("addForeignKeyConstraint", &["baseTableName", "baseColumnNames", "constraintName", "referencedTableName", "referencedColumnNames", "onDelete", "onUpdate", "deleteCascade", "deferrable", "initiallyDeferred", "validate"]),
    ("addLookupTable", &[]),
    ("addNotNullConstraint", &["catalogName", "schemaName", "tableName", "columnName", "columnDataType", "defaultNullValue", "constraintName", "validate"]),
    ("addPrimaryKey", &["catalogName", "schemaName", "tableName", "columnNames", "constraintName", "tablespace", "validate"]),
    ("addUniqueConstraint", &["catalogName", "schemaName", "tableName", "columnNames", "constraintName", "tablespace", "deferrable", "initiallyDeferred", "validate"]),
    ("alterSequence", &[]),
    ("createIndex", &["catalogName", "schemaName", "tableName", "indexName", "unique", "tablespace", "columns"]),
    ("createProcedure", &[]),
    ("createSequence", &["catalogName", "schemaName", "sequenceName", "startValue", "incrementBy", "minValue", "maxValue", "cycle", "cacheSize", "dataType"]),
    ("createTable", &["catalogName", "schemaName", "tableName", "tablespace", "remarks", "ifNotExists", "columns"]),
    ("createView", &["catalogName", "schemaName", "viewName", "selectQuery", "replaceIfExists", "fullDefinition", "remarks"]),
    ("customChange", &[]),
    ("delete", &["catalogName", "schemaName", "tableName", "where"]),
    ("dropAllForeignKeyConstraints", &[]),
    ("dropColumn", &["catalogName", "schemaName", "tableName", "columnName", "columns"]),
    ("dropDefaultValue", &[]),
    ("dropForeignKeyConstraint", &["baseTableName", "constraintName"]),
    ("dropIndex", &["catalogName", "schemaName", "tableName", "indexName"]),
    ("dropNotNullConstraint", &["catalogName", "schemaName", "tableName", "columnName", "columnDataType"]),
    ("dropPrimaryKey", &[]),
    ("dropProcedure", &[]),
    ("dropSequence", &[]),
    ("dropTable", &["catalogName", "schemaName", "tableName", "cascadeConstraints"]),
    ("dropUniqueConstraint", &[]),
    ("dropView", &[]),
    ("empty", &[]),
    ("executeCommand", &[]),
    ("insert", &["catalogName", "schemaName", "tableName", "columns"]),
    ("loadData", &["catalogName", "schemaName", "tableName", "file", "relativeToChangelogFile", "encoding", "separator", "quotchar", "columns"]),
    ("loadUpdateData", &[]),
    ("mergeColumns", &[]),
    ("modifyDataType", &["catalogName", "schemaName", "tableName", "columnName", "newDataType"]),
    ("output", &[]),
    ("renameColumn", &["catalogName", "schemaName", "tableName", "oldColumnName", "newColumnName", "columnDataType", "remarks"]),
    ("renameSequence", &[]),
    ("renameTable", &["catalogName", "schemaName", "oldTableName", "newTableName"]),
    ("renameView", &[]),
    ("setColumnRemarks", &[]),
    ("setTableRemarks", &[]),
    ("sql", &["sql", "dbms", "splitStatements", "stripComments", "endDelimiter", "comment"]),
    ("sqlFile", &["path", "relativeToChangelogFile", "dbms", "encoding", "splitStatements", "stripComments", "endDelimiter"]),
    ("stop", &[]),
    ("tagDatabase", &["tag"]),
    ("update", &["catalogName", "schemaName", "tableName", "columns", "where"]),
];

/// Liquibase Pro change types: known (never flagged), not offered.
pub const PRO_CHANGE_TYPES: &[&str] = &[
    "addCheckConstraint", "createFunction", "createPackage", "createPackageBody", "createSynonym", "createTrigger",
    "disableCheckConstraint", "disableTrigger", "dropCheckConstraint", "dropFunction", "dropPackage",
    "dropPackageBody", "dropSynonym", "dropTrigger", "enableCheckConstraint", "enableTrigger", "markUnused",
    "renameTrigger",
];
```

Add `// ponytail: extension change types (MongoDB, Hibernate…) are flagged as unknown; add their names when reported` above `PRO_CHANGE_TYPES`. Implement `change_type` and `is_change_type` as linear scans.

- [ ] **Step 7: Run the test and validate the package**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test && ~/.cargo/bin/openidea-plugin build`
Expected: `knows_every_change_type_once ... ok`. `build` ends with the `check` summary: no errors, three file templates, four run configurations, and `process:liquibase` listed among the capabilities to grant.

- [ ] **Step 8: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "feat(liquibase): package skeleton, templates, run configurations, change type catalogue"
```

---

### Task 2: Changelog model (`changelog.rs`)

**Files:**
- Create: `plugins/liquibase/src/changelog.rs` (tests inside)
- Modify: `plugins/liquibase/src/lib.rs` (`mod changelog;`)

**Interfaces:**
- Consumes: `spring_text::xml::{elements, root}`, `spring_text::config::yaml`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Xml, Yaml, Sql }

/// A changeSet; `at..end`: its `changeSet` tag name or key, or the whole `--changeset` line. Empty values are `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeSet { pub id: Option<String>, pub author: Option<String>, pub at: usize, pub end: usize }

/// An `include` (`dir: false`, `path` = `file`) or `includeAll` (`dir: true`); `at..end`: the value as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include { pub path: String, pub relative: bool, pub dir: bool, pub at: usize, pub end: usize }

/// A change type key directly under `changes` (YAML only); `at..end`: the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change { pub name: String, pub at: usize, pub end: usize }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changelog {
    pub format: Format,
    /// Formatted SQL only: the first non-blank line is the header (`true` for XML and YAML).
    pub header: bool,
    pub change_sets: Vec<ChangeSet>,
    pub includes: Vec<Include>,
    pub changes: Vec<Change>,
}

/// Where an include points, as far as the host can tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target { Found(String), Missing, Unknown }

/// `None` when `text` isn't a Liquibase changelog (format by `path`'s extension).
pub fn parse(path: &str, text: &str) -> Option<Changelog>;

/// Absolute paths `include` may mean, most likely first; empty when it can't be known
/// (a `${property}`, or a path leaving `root`).
pub fn candidates(changelog: &str, root: &str, include: &Include) -> Vec<String>;
```

Parsing rules (the spec values):
- **XML:**
  - Detection: `root(&elements) == Some("databaseChangeLog")`.
  - `changeSet` elements: `id`/`author` from `value()`; the span is the tag name, `e.at..e.at + e.name.len()`.
  - `include`: `file`. `includeAll`: `path`. Either may carry `relativeToChangelogFile`, which is `eq_ignore_ascii_case("true")`. The span is the attribute's `value_at..value_end`.
- **YAML:**
  - Detection: a key equal to `databaseChangeLog`.
  - From `config::yaml`, group keys by their `databaseChangeLog[n]` prefix:
    - `…[n].changeSet` is a ChangeSet span, with `.id`/`.author` as its values;
    - `…[n].include.file`, `…[n].includeAll.path` and their `.relativeToChangelogFile` are includes;
    - keys of the exact shape `databaseChangeLog[n].changeSet.changes[m].<name>` are Changes.
- **SQL:**
  - The header is the first non-blank line, which after stripping `--` and surrounding blanks equals `liquibase formatted sql` (ASCII case-insensitive).
  - A changeset line is a line that, after stripping `--` and blanks, starts with `changeset` (case-insensitive) followed by whitespace. The first token is `author:id`; a quoted author (`"Jane Doe":2`) may contain spaces. With no `:`, the token is the author and the id is `None`.
  - Detection: the header, or at least one changeset line.
- `candidates`:
  - Strip `classpath:` and then leading `/`.
  - If the path contains `${`, return empty.
  - If `relative`, return the changelog's directory joined with the path.
  - Otherwise, return `<resources root>/<path>` (when the changelog path contains `/src/main/resources/` or `/src/test/resources/`, the prefix up to and including `resources`) followed by `<root>/<path>`.
  - Normalise `.` and `..` segments and trim a trailing `/`. If a result would leave `root`, return empty: `read_file` refuses `..`, and the target is unknown rather than missing.

- [ ] **Step 1: Write the failing tests** (fixtures exact)

```rust
const XML: &str = r#"<?xml version="1.0"?>
<databaseChangeLog xmlns="http://www.liquibase.org/xml/ns/dbchangelog">
    <include file="db/changes/001.xml"/>
    <include file="002.xml" relativeToChangelogFile="true"/>
    <includeAll path="db/more/"/>
    <changeSet id="1" author="bob"><createTable tableName="t"/></changeSet>
    <changeSet id="2"/>
</databaseChangeLog>
"#;

const YAML: &str = "databaseChangeLog:
  - include:
      file: classpath:db/changes/001.yaml
  - changeSet:
      id: \"1\"
      author: bob
      changes:
        - createTable:
            tableName: t
        - createTabel:
            tableName: u
  - changeSet:
      id: 1
      author: bob
";

const SQL: &str = "--liquibase formatted sql\n\n--changeset bob:1\ncreate table t (id int);\n--rollback drop table t;\n\n-- changeset \"Jane Doe\":2 runOnChange:true\nselect 1;\n--changeset nobody\n";

fn sets(log: &Changelog) -> Vec<(Option<&str>, Option<&str>)> {
    log.change_sets.iter().map(|c| (c.id.as_deref(), c.author.as_deref())).collect()
}

#[test]
fn reads_xml_changelogs() {
    let log = parse("/p/db.xml", XML).unwrap();
    assert_eq!(log.format, Format::Xml);
    assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("2"), None)]);
    let includes: Vec<_> = log.includes.iter().map(|i| (i.path.as_str(), i.relative, i.dir)).collect();
    assert_eq!(includes, [("db/changes/001.xml", false, false), ("002.xml", true, false), ("db/more/", false, true)]);
    assert_eq!(&XML[log.includes[0].at..log.includes[0].end], "db/changes/001.xml");
    assert_eq!(&XML[log.change_sets[1].at..log.change_sets[1].end], "changeSet");
    assert!(log.changes.is_empty());
}

#[test]
fn reads_yaml_changelogs() {
    let log = parse("/p/db.yaml", YAML).unwrap();
    assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("1"), Some("bob"))]);
    assert_eq!(log.includes[0].path, "classpath:db/changes/001.yaml");
    let names: Vec<&str> = log.changes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["createTable", "createTabel"]);
    assert_eq!(&YAML[log.changes[1].at..log.changes[1].end], "createTabel");
    assert_eq!(&YAML[log.change_sets[0].at..log.change_sets[0].end], "changeSet");
}

#[test]
fn reads_formatted_sql() {
    let log = parse("/p/db.sql", SQL).unwrap();
    assert!(log.header);
    assert_eq!(sets(&log), [(Some("1"), Some("bob")), (Some("2"), Some("Jane Doe")), (None, Some("nobody"))]);
    assert_eq!(&SQL[log.change_sets[2].at..log.change_sets[2].end], "--changeset nobody");
    assert!(!parse("/p/a.sql", "--changeset bob:1\nselect 1;").unwrap().header);
}

#[test]
fn ignores_other_files() {
    assert_eq!(parse("/p/pom.xml", "<project><modelVersion>4.0.0</modelVersion></project>"), None);
    assert_eq!(parse("/p/application.yml", "spring:\n  liquibase:\n    change-log: classpath:db/master.yaml\n"), None);
    assert_eq!(parse("/p/schema.sql", "create table t (id int);"), None);
    assert_eq!(parse("/p/a.json", "{\"databaseChangeLog\": []}"), None);
}

#[test]
fn survives_every_prefix() {
    for (path, text) in [("/p/a.xml", XML), ("/p/a.yaml", YAML), ("/p/a.sql", SQL)] {
        for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
            parse(path, &text[..end]);
        }
    }
}

#[test]
fn include_candidates() {
    let inc = |path: &str, relative: bool| Include { path: path.into(), relative, dir: false, at: 0, end: 0 };
    let log = "/project/src/main/resources/db/changelog/master.yaml";
    assert_eq!(candidates(log, "/project", &inc("changes/001.yaml", true)), ["/project/src/main/resources/db/changelog/changes/001.yaml"]);
    assert_eq!(candidates(log, "/project", &inc("../shared/x.yaml", true)), ["/project/src/main/resources/db/shared/x.yaml"]);
    assert_eq!(
        candidates(log, "/project", &inc("classpath:/db/changes/001.yaml", false)),
        ["/project/src/main/resources/db/changes/001.yaml", "/project/db/changes/001.yaml"]
    );
    assert_eq!(candidates("/project/db/master.xml", "/project", &inc("db/changes/001.xml", false)), ["/project/db/changes/001.xml"]);
    assert!(candidates(log, "/project", &inc("${base}/x.yaml", false)).is_empty());
    assert!(candidates(log, "/project", &inc("../../../../../../etc/passwd", true)).is_empty());
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test changelog::`
Expected: compile errors (`parse`, `candidates` not found).

- [ ] **Step 3: Implement `parse` and `candidates` in `src/changelog.rs`** following the rules in Interfaces. One private function per format (`xml`, `yaml`, `sql`). An empty `id`/`author` string becomes `None`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test changelog::`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "feat(liquibase): changelog model for XML, YAML and formatted SQL"
```

---

### Task 3: Live diagnostics (`checks.rs`, `line_markers`, include resolution)

**Files:**
- Create: `plugins/liquibase/src/checks.rs`
- Modify: `plugins/liquibase/src/lib.rs` (`mod checks;`, `fn target`, `Providers::line_markers`, tests)

**Interfaces:**
- Consumes: `changelog::{parse, candidates, Changelog, Include, Target, Format}`, `catalog::is_change_type`, `spring_text::{range, index::project_of}`.
- Produces:
  - `checks::problems(path: &str, text: &str, log: &Changelog, target: &dyn Fn(&Include) -> Target) -> Vec<Diagnostic>`, sorted by range start;
  - `lib.rs`: `fn target(changelog: &str, include: &Include) -> Target`. No project, or no candidates: `Unknown`. A file include: the first candidate whose `project.read_file` succeeds, as `Found(candidate)`; otherwise `Missing`. A directory: `find_files("<candidate relative to root>/**/*", 1)` non-empty for some candidate gives `Found(candidate)`; otherwise `Missing`;
  - `Providers::line_markers(doc)` publishes `problems` for a changelog, or an empty list for any other document (that clears stale problems), with `let _ = host::publish_diagnostics(..)`. It returns `Vec::new()`. This is the live-check workaround of `plugins/spring-boot/src/lib.rs:321-336`.

Diagnostics (codes, severities and messages are exact):

| Code | Severity | Range | Message |
|---|---|---|---|
| `duplicate-changeset` | Error | the later ChangeSet span; `related` = `[RelatedInformation { location: Location { path, range: <first span> }, message: "First definition".into() }]` | ``Changeset `{id}` by `{author}` is already defined in this file.`` |
| `incomplete-changeset` | Error | ChangeSet span | `Changeset has no id.` / `Changeset has no author.` / `Changeset has no id and no author.`; for `Format::Sql` append `` Write `--changeset author:id`.`` |
| `unknown-change-type` | Warning | Change span | ``` `{name}` is not a built-in Liquibase change type.``` |
| `unresolved-include` | Error | Include span, only for `Target::Missing` | file: ``Cannot find `{path}`.``; dir: ``No files under `{path}`.`` |
| `missing-header` | Warning | line 0 (`0..` end of the first line) | ``Formatted SQL changelogs start with `--liquibase formatted sql`.`` |

Duplicates compare `(id, author)` exactly, and only when both are present.

- [ ] **Step 1: Write the failing tests** (in `lib.rs` `#[cfg(test)] mod tests`)

```rust
use super::*;
use openidea_plugin::mock::MockHost;
use openidea_plugin::providers::Document;

const MASTER: &str = "/project/src/main/resources/db/changelog/master.yaml";
const MASTER_YAML: &str = "databaseChangeLog:
  - include:
      file: classpath:db/changes/001.yaml
  - include:
      file: db/changes/missing.yaml
  - include:
      file: ${changes}/x.yaml
  - includeAll:
      path: changes/
      relativeToChangelogFile: true
  - changeSet:
      id: \"1\"
      author: bob
      changes:
        - createTabel:
            tableName: t
  - changeSet:
      id: 1
      author: bob
  - changeSet:
      author: ann
";

fn doc(path: &str, text: &str) -> Document {
    let language = match path.rsplit('.').next() { Some("xml") => "xml", Some("sql") => "sql", _ => "yaml" };
    Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn checked(host: &MockHost, path: &str, text: &str) -> Vec<(String, String)> {
    Liquibase.line_markers(&doc(path, text));
    host.diagnostics().get(path).cloned().unwrap_or_default().into_iter()
        .map(|d| (d.code.unwrap_or_default(), d.message)).collect()
}

fn host() -> MockHost {
    let host = MockHost::install();
    host.add_file("src/main/resources/db/changes/001.yaml", "databaseChangeLog: []\n");
    host.add_file("src/main/resources/db/changelog/changes/002.yaml", "databaseChangeLog: []\n");
    host
}

#[test]
fn reports_yaml_changelog_problems() {
    let host = host();
    assert_eq!(checked(&host, MASTER, MASTER_YAML), [
        ("unresolved-include".into(), "Cannot find `db/changes/missing.yaml`.".into()),
        ("unknown-change-type".into(), "`createTabel` is not a built-in Liquibase change type.".into()),
        ("duplicate-changeset".into(), "Changeset `1` by `bob` is already defined in this file.".into()),
        ("incomplete-changeset".into(), "Changeset has no id.".into()),
    ]);
    let duplicate = host.diagnostics()[MASTER].iter().find(|d| d.code.as_deref() == Some("duplicate-changeset")).cloned().unwrap();
    let first = MASTER_YAML.find("changeSet").unwrap();
    assert_eq!(duplicate.related[0].location.range, spring_text::range(MASTER_YAML, first, first + "changeSet".len()));
}

#[test]
fn reports_xml_changelog_problems() {
    let host = host();
    let text = r#"<databaseChangeLog>
    <include file="nope.xml" relativeToChangelogFile="true"/>
    <changeSet id="1" author="bob"/>
    <changeSet id="1" author="bob"/>
    <changeSet id="2"/>
    <changeSet/>
</databaseChangeLog>"#;
    assert_eq!(checked(&host, "/project/db/master.xml", text), [
        ("unresolved-include".into(), "Cannot find `nope.xml`.".into()),
        ("duplicate-changeset".into(), "Changeset `1` by `bob` is already defined in this file.".into()),
        ("incomplete-changeset".into(), "Changeset has no author.".into()),
        ("incomplete-changeset".into(), "Changeset has no id and no author.".into()),
    ]);
}

#[test]
fn reports_formatted_sql_problems() {
    let host = host();
    let text = "--changeset bob:1\nselect 1;\n--changeset \"Jane Doe\":1\n--changeset nobody\n";
    assert_eq!(checked(&host, "/project/db/a.sql", text), [
        ("missing-header".into(), "Formatted SQL changelogs start with `--liquibase formatted sql`.".into()),
        ("incomplete-changeset".into(), "Changeset has no id. Write `--changeset author:id`.".into()),
    ]);
}

#[test]
fn stays_silent_outside_changelogs() {
    let host = host();
    assert!(checked(&host, "/project/pom.xml", "<project><changeSet/></project>").is_empty());
    assert!(checked(&host, "/project/src/main/resources/application.yml", "spring:\n  liquibase:\n    change-log: x\n").is_empty());
    assert!(checked(&host, "/project/schema.sql", "create table t (id int);").is_empty());
}
```

Note that `bob:1` and `"Jane Doe":1` are different authors, so no duplicate is reported. Line 0 is a changeset, and the header diagnostic shares its range; sorting by start puts `missing-header` first because it is pushed first at equal starts (use a stable sort).

- [ ] **Step 2: Run them to see them fail**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test tests::`
Expected: FAIL. `line_markers` is the default and publishes nothing, so the assertions on codes fail.

- [ ] **Step 3: Implement `checks::problems` in `src/checks.rs`, and `target` plus `line_markers` in `src/lib.rs`**, as specified in Interfaces and the table. Mark the include lookup with `// ponytail: reads each included file to test it exists (no exists API); cap or cache if master changelogs with hundreds of includes miss the 50 ms budget`.

- [ ] **Step 4: Run the tests to see them pass**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test`
Expected: all pass (catalogue + changelog + 4 new).

- [ ] **Step 5: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "feat(liquibase): live changeset, change type, include and header checks"
```

---

### Task 4: Go to Definition on `include`

**Files:**
- Modify: `plugins/liquibase/src/lib.rs` (`Providers::definition`, one test)

**Interfaces:**
- Consumes: `changelog::parse`, `fn target` (Task 3), `openidea_plugin::text::offset`.
- Produces: `Providers::definition(doc, at) -> Vec<Location>`. For the non-`dir` include whose `at..=end` holds the caret offset and whose target is `Found(p)`, return `[Location { path: p, range: Range::new(Position::new(0, 0), Position::new(0, 0)) }]`; otherwise return empty.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn goes_to_included_changelogs() {
    let host = MockHost::install();
    host.add_file("db/changes/001.xml", "<databaseChangeLog/>");
    let text = "<databaseChangeLog>\n    <include file=\"changes/001.xml\" relativeToChangelogFile=\"true\"/>\n    <changeSet id=\"1\" author=\"a\"/>\n</databaseChangeLog>";
    let d = doc("/project/db/master.xml", text);
    let start = Position::new(0, 0);
    assert_eq!(Liquibase.definition(&d, Position::new(1, 25)), [Location { path: "/project/db/changes/001.xml".into(), range: Range::new(start, start) }]);
    assert!(Liquibase.definition(&d, Position::new(2, 8)).is_empty());
    let yaml = "databaseChangeLog:\n  - include:\n      file: db/changes/001.xml\n";
    assert_eq!(Liquibase.definition(&doc("/project/master.yaml", yaml), Position::new(2, 14)).len(), 1);
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test goes_to_included_changelogs`
Expected: FAIL (the default `definition` returns an empty list).

- [ ] **Step 3: Implement `definition`** as specified.

- [ ] **Step 4: Run it to see it pass**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test goes_to_included_changelogs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "feat(liquibase): go to definition on include"
```

---

### Task 5: YAML and formatted SQL completion (`complete.rs`)

**Files:**
- Create: `plugins/liquibase/src/complete.rs`
- Modify: `plugins/liquibase/src/lib.rs` (`mod complete;`, `Providers::completion`, tests)

**Interfaces:**
- Consumes: `catalog::*`, `changelog::parse`, `spring_text::config::yaml_parent`, `spring_text::range`, `line_bounds` (copied from `plugins/spring-boot/src/lib.rs:174-178`).
- Produces:
  - `complete::yaml(text: &str, at: usize) -> Vec<CompletionItem>`;
  - `complete::sql(text: &str, at: usize, changelog: bool) -> Vec<CompletionItem>`;
  - `Providers::completion(doc, at, _trigger)`:
    - yaml: `complete::yaml` only when `parse(..)` is `Some`;
    - sql: `complete::sql(text, at, parse(..).is_some())`;
    - anything else: empty.

YAML rules:
- `(parent, typed) = yaml_parent(text, at)`.
- If `typed` contains `:` or starts with `#`, there are no items.
- Drop the `[n]` indices from `parent` and match its ending:

| `parent` ends with | Items |
|---|---|
| `databaseChangeLog[]` | `TOP_LEVEL` |
| `.changeSet` | `CHANGE_SET` |
| `.changes[]` or `.rollback[]` | the names of `CHANGE_TYPES` (community only) |
| `.changes[].<type>` or `.rollback[].<type>` with `change_type(type)` = `Some(attrs)` | `attrs` |
| `.columns[]` | `["column"]` |
| `.column` | `COLUMN` |
| `.constraints` | `CONSTRAINTS` |
| `.include` / `.includeAll` | `INCLUDE` / `INCLUDE_ALL` |

- `indent = typed_at - line_start` (the typed key's column).
- `insert_text`:
  - `{key}:\n{indent+2 spaces}- ` for keys in `LISTS`;
  - `{key}:\n{indent+2 spaces}` for change types, `column`, `constraints`, `changeSet` (see below), `include`, `includeAll`, `property` and `preConditions` at top level;
  - `{key}: ` otherwise.
- `changeSet` is a snippet (`snippet = true`): `changeSet:\n{i}id: ${1:id}\n{i}author: ${2:author}\n{i}changes:\n{i}  - $0` with `i` = indent + 2.
- `replace` = `typed_at..word end` (as `spring-boot`'s `yaml_completion` computes it).
- `kind`: `Module` for nested keys, `Property` for scalars, `Snippet` for `changeSet`.

SQL rules:
- The line text before the caret, trimmed at the start, must match `-` or `--` followed by ASCII letters only. Otherwise there are no items.
- `replace` = from that `-` to the caret.
- On line 0: `--liquibase formatted sql` (offered even when `changelog` is false).
- When `changelog` is true: `--changeset` (snippet `--changeset ${1:author}:${2:id}\n$0`), `--rollback` (insert `--rollback `) and `--comment` (insert `--comment: `), in that order. Set `filter_text` = label.

- [ ] **Step 1: Write the failing tests** (in `lib.rs` tests)

```rust
fn complete_at_end(path: &str, text: &str) -> Vec<CompletionItem> {
    Liquibase.completion(&doc(path, text), openidea_plugin::text::position(text, text.len()), None)
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

fn item<'a>(items: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
    items.iter().find(|i| i.label == label).unwrap_or_else(|| panic!("no {label}"))
}

const SET: &str = "databaseChangeLog:\n  - changeSet:\n      id: 1\n      author: bob\n      changes:\n";

#[test]
fn completes_yaml_changelogs() {
    let _host = MockHost::install();
    let y = "/project/db/master.yaml";

    let text = format!("{SET}        - creat");
    let items = complete_at_end(y, &text);
    assert!(labels(&items).contains(&"createTable") && labels(&items).contains(&"createIndex"));
    assert!(!labels(&items).contains(&"createTrigger"), "Pro types are not offered");
    assert_eq!(item(&items, "createTable").insert_text, "createTable:\n            ");
    assert_eq!(item(&items, "createTable").replace, Some(spring_text::range(&text, text.len() - 5, text.len())));

    let items = complete_at_end(y, &format!("{SET}        - createTable:\n            tab"));
    assert_eq!(item(&items, "tableName").insert_text, "tableName: ");
    assert_eq!(item(&items, "columns").insert_text, "columns:\n              - ");

    let in_columns = format!("{SET}        - createTable:\n            columns:\n              - ");
    assert_eq!(labels(&complete_at_end(y, &in_columns)), ["column"]);
    let in_column = format!("{in_columns}column:\n                  ");
    let items = complete_at_end(y, &in_column);
    assert!(labels(&items).contains(&"type"));
    assert_eq!(item(&items, "constraints").insert_text, "constraints:\n                    ");
    assert!(labels(&complete_at_end(y, &format!("{in_column}constraints:\n                    "))).contains(&"primaryKey"));

    let items = complete_at_end(y, "databaseChangeLog:\n  - changeSet:\n      ");
    assert!(labels(&items).contains(&"runOnChange"));
    assert_eq!(item(&items, "changes").insert_text, "changes:\n        - ");

    let items = complete_at_end(y, "databaseChangeLog:\n  - ");
    assert_eq!(item(&items, "changeSet").insert_text, "changeSet:\n      id: ${1:id}\n      author: ${2:author}\n      changes:\n        - $0");
    assert!(item(&items, "changeSet").snippet);
    assert!(labels(&items).contains(&"includeAll"));

    assert!(complete_at_end(y, "databaseChangeLog:\n  - changeSet:\n      author: ").is_empty());
    assert!(complete_at_end("/project/src/main/resources/application.yml", "spring:\n  ").is_empty());
}

#[test]
fn completes_formatted_sql_comments() {
    let _host = MockHost::install();
    assert_eq!(labels(&complete_at_end("/project/db/a.sql", "--")), ["--liquibase formatted sql"]);

    let text = "--liquibase formatted sql\n\n--ch";
    let items = complete_at_end("/project/db/a.sql", text);
    assert_eq!(labels(&items), ["--changeset", "--rollback", "--comment"]);
    assert_eq!(item(&items, "--changeset").insert_text, "--changeset ${1:author}:${2:id}\n$0");
    assert_eq!(item(&items, "--changeset").replace, Some(spring_text::range(text, text.len() - 4, text.len())));

    assert!(complete_at_end("/project/db/a.sql", "--liquibase formatted sql\nselect 1 -- ").is_empty());
    assert!(complete_at_end("/project/schema.sql", "select 1;\n--").is_empty());
}

#[test]
fn completion_survives_every_prefix() {
    let _host = MockHost::install();
    let text = format!("{SET}        - createTable:\n            columns:\n              - column:\n                  constraints:\n                    nullable: false\n");
    for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
        complete_at_end("/project/db/master.yaml", &text[..end]);
    }
    let sql = "--liquibase formatted sql\n--changeset a:1\n--rollback x\n";
    for end in 0..=sql.len() {
        complete_at_end("/project/db/a.sql", &sql[..end]);
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test complet`
Expected: FAIL (the default `completion` returns an empty list).

- [ ] **Step 3: Implement `complete::yaml`, `complete::sql` and `Providers::completion`** following the rules above.

- [ ] **Step 4: Run all tests to see them pass**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "feat(liquibase): YAML and formatted SQL completion with changeset snippets"
```

---

### Task 6: README, limitations, build and IDE check

**Files:**
- Modify: `plugins/liquibase/README.md` (replace the scaffold text)
- Modify: `LIMITATIONS.md` (append a section; don't commit it, see Global Constraints)

- [ ] **Step 1: Write `README.md`.** Cover:
  - **Checks:** a table of the five codes, with severities and what each means.
  - **Completion:** the YAML contexts table and the SQL items.
  - **Definition:** Go to Definition on `include`.
  - **Templates:** the three New-menu templates.
  - **Run configurations:** they need `liquibase` on PATH, `process:liquibase` granted, and a `liquibase.properties` at the project root with `changeLogFile`, `url` and credentials. Name the CLI's `--search-path` setting, so that changesets keep the file paths the application records.
  - **Limits:**
    - XML completion comes from lemminx and the changelog's XSD;
    - JSON changelogs aren't supported;
    - includes resolve against `src/main|test/resources` or the project root;
    - only open files are checked;
    - `rollback-count` isn't offered.
  - The `cargo test` / `openidea-plugin build` / `openidea-plugin link` block of `plugins/jpa-assist/README.md`.

- [ ] **Step 2: Append to `LIMITATIONS.md`** a section `## Liquibase Changelogs (aritti.liquibase, clean-room)` with:
  - **Run configurations can't confirm or ask:** `runDetectors` take fixed `args` and match marker files at the project root only (`crates/ide-host/src/plugins/run_detectors.rs:134-140`). `update` runs on one click, `rollback-count` (it needs a count) and per-changelog runs aren't offered, and a `liquibase.properties` outside the root isn't detected. Would need: run configurations computed by code, or `ui.show-input`/confirm.
  - **No file-exists API:** include resolution reads each included file with `read_file`, and `includeAll` uses `find_files`, which skips gitignored directories. Would need: `workspace.project.exists(path)` / `stat`.
  - **No source roots or classpath:** `classpath:` includes are resolved against `src/main/resources`/`src/test/resources` by path heuristics. Would need: `project.source-roots()`.
  - A pointer to the existing Spring rows for "diagnostics only from `line-markers`" and "no database API" instead of repeating them.

- [ ] **Step 3: Build and check**

Run: `cd /Users/aritti/Projects/openidea-plugins/plugins/liquibase && ~/.cargo/bin/cargo test && ~/.cargo/bin/openidea-plugin build`
Expected: all tests pass, and `check` reports the package valid, asking for `project.read` (required) and `process:liquibase` (from run configurations).

- [ ] **Step 4: IDE smoke check** (`~/.cargo/bin/openidea-plugin link`, then Settings › Plugins › Reload), in a Spring Boot project with `src/main/resources/db/changelog/db.changelog-master.yaml`:
  - typing `createTabel:` under `changes:` shows the warning without saving;
  - the duplicate changeset error links to the first definition;
  - ⌘B on an `include` `file:` value opens the file;
  - Ctrl+Space under `changes:` lists change types;
  - New › Liquibase Changelog (YAML) creates the template.

  Also check that `pom.xml` and `application.yml` show no Liquibase problems. With a `liquibase.properties` at the root, the four "Liquibase: …" configurations appear once `process:liquibase` is granted. Record anything that differs in `LIMITATIONS.md`.

- [ ] **Step 5: Commit**

```bash
cd /Users/aritti/Projects/openidea-plugins && git add plugins/liquibase && git commit -m "docs(liquibase): README"
```
