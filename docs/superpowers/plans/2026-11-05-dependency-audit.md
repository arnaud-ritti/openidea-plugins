# Dependency Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An OpenIDEA plugin `aritti.dependency-audit` that checks Maven, Gradle, npm and Composer dependencies against OSV.dev, marks vulnerable ones in the manifest, offers an "upgrade to the nearest fixed version" quick fix and lists every vulnerable dependency of a project in a tool window.

**Architecture:** `manifest.rs` turns a manifest's text (plus its sibling lockfile) into `Dep`s with byte offsets. `osv.rs` asks `POST https://api.osv.dev/v1/querybatch` which exact versions are affected (OSV does the version matching server-side), fetches `GET /v1/vulns/{id}` once per advisory for summary, severity and fixed versions, and caches both in `host::storage`. `lib.rs` wires it to `file-opened`/`file-saved` (diagnostics), the `code-actions` provider (cache only, no network), a `scan` command and a `Tree` view.

**Tech Stack:** Rust 2024, `wasm32-wasip2`, `openidea-plugin` SDK v0.1.37 (MockHost for tests), `serde` + `serde_json`, the repo's `crates/spring-text` (`xml::elements`, `range`, `index::project_of`, `tree::Expansion`).

**Spec:** the Scope section below (there is no separate spec document).

## Scope

Clean-room plugin. Inputs: the OSV API docs (google.github.io/osv.dev/api/, `post-v1-querybatch`, `get-v1-vulns`), the OSV schema (ossf.github.io/osv-schema/), the public manifest formats, and the Marketplace page *description* of JetBrains' closed-source package-vulnerability plugin. No jar downloads, no decompiling, no copied strings, and never its name.

**In (MVP):**
- Manifests: Maven `pom.xml` (`<dependency>` with a literal version or `${property}` from `<properties>`, including `<dependencyManagement>`), Gradle `build.gradle` / `build.gradle.kts` string coordinates (`'g:a:v'`, `"g:a:v"`), npm `package.json` (versions from the sibling `package-lock.json` v2/v3, else exact versions only), `package-lock.json` (every installed package), Composer `composer.json` (versions from the sibling `composer.lock`, else exact versions only) and `composer.lock` (every package).
- On `file-opened` / `file-saved` of those files: query OSV, publish one diagnostic per vulnerable dependency on its name, severity taken from the advisory (`database_specific.severity`, else `affected[].ecosystem_specific.severity`).
- Quick fix (`code-actions`, cache only): replace the version (or the Maven property value, or the npm/Composer constraint keeping its `^`/`~` operator) with the nearest fixed version. Not offered in lockfiles.
- Command *Dependency Audit: Scan Project* + tool window *Vulnerable Dependencies* (Tree: file › dependency › advisory; activate opens the line or osv.dev).
- Cache in `host::storage`: query results 24 h, advisories until OSV's `modified` changes.
- Setting `autoCheck` (default on). Privacy notice in the description and README.

**Dropped, and why:**
- Versionless Maven/Gradle deps (Spring Boot BOM, parent POM, `io.spring.dependency-management`): resolving them needs the build model or `process:mvn`/`gradle` (high-risk capability). Skipped silently; README says so. Open question for the user.
- `gradle/libs.versions.toml`, `Cargo.toml`/`Cargo.lock`, `requirements.txt`, `yarn.lock`, `pnpm-lock.yaml`, `package-lock.json` v1: not the user's main stacks or not JSON; add per demand.
- Version-range resolution (`^1.2`, `[1.0,2.0)`): only exact or locked versions are sent; OSV matches them.
- CVSS vector scoring: severity uses the advisory's own label; a CVSS-only advisory is a warning.
- Pagination (`next_page_token`): OSV only paginates past 1,000 vulns per query / 3,000 per batch.
- Checks while typing: no `document-changed` event (LIMITATIONS.md "Problems aren't re-checked while typing").
- Opening the tool window from the command: no API (LIMITATIONS.md gitlab "Commands and notifications can't open a tool window"); the notification says where to look.

**Verified platform facts** (upstream checkout `~/.cargo/git/checkouts/openidea-7faf5f9c9e6a9886/d3d0fd2/` = tag v0.1.37):
- Fetch is allowed from event handlers and commands: `crates/ide-plugin/src/runtime.rs:851` runs `handle_events` with `limits.timeout` (5 s, `types.rs:219`); `host::begin_task` extends the call to 60 s (`wit_host.rs:493-505`). A request's timeout is `min(timeout_ms or 30 s, remaining call budget)` (`wit_host.rs:594-600`).
- Request and response bodies are capped at 8 MiB (`wit_host.rs:133`, `crates/ide-host/src/plugins/api.rs` `MAX_RESPONSE`); redirects to another host are refused (`api.rs:482-530`). OSV states no rate limits.
- Providers get 50 ms (`types.rs:221`): the quick fix never fetches.
- `register_plugin!(T, providers, views)` exists (`sdk/rust/openidea-plugin/src/lib.rs:178`).
- MockHost: `on_fetch`, `requests()`, `diagnostics()`, `storage()`, `set_setting`, `opened_urls`, `opened_locations`, `refreshed_views`, `notifications`, `logs` (`sdk/rust/openidea-plugin/src/mock.rs`). `MockHost::install()` grants no `net:` capability: tests use `.with_capabilities(CAPS)`.
- Language ids: `pom.xml` → `xml`, `build.gradle` → `groovy`, `*.kts` → `kotlin`, `package.json`/`composer.json`/`*-lock.json`/`composer.lock` → `json` (`languages/*.toml`).
- Mock `find_files` over-matches `**/pom.xml` (it also matches `foopom.xml`); the host skips gitignored dirs (`node_modules/`, `vendor/`). Filter results with `manifest::kind`.

## Global Constraints

- Work on branch `dependency-audit` (`git switch -c dependency-audit` from `main`).
- Directory `plugins/dependency-audit/`, id `aritti.dependency-audit`, name `Dependency Audit`, version `0.1.0`, `license = "MIT"`, `authors = ["Arnaud Ritti <arnaud.ritti@gmail.com>"]`, `categories = ["linters", "tools"]`, `engines.openidea = "^1.0"`.
- `LICENSE`: copy of `plugins/devcontainer/LICENSE` (MIT, "Copyright (c) 2026 Arnaud Ritti"). No `NOTICE`.
- Clean-room: the string "Package Checker" appears in no file of the plugin; no text copied from JetBrains.
- Capabilities: `required = ["project.read", "storage", "net:api.osv.dev"]`, `optional = ["events"]`. No others.
- Dependencies exactly: `openidea-plugin = { git = "https://github.com/l0ng-ai/openidea", tag = "v0.1.37" }`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `spring-text = { path = "../../crates/spring-text" }`. No semver/XML/TOML crates. `[profile.release]` as in `plugins/jpa-assist/Cargo.toml`.
- Network: only `https://api.osv.dev/v1/querybatch` and `https://api.osv.dev/v1/vulns/{id}`, `timeout_ms: Some(10_000)`, header `Content-Type: application/json` on the POST, at most 1000 queries per POST. Never fetch from a provider.
- Copy fixed by this plan: command title `Dependency Audit: Scan Project`, view title `Vulnerable Dependencies`, diagnostic `source` `OSV`.
- Commands: `cargo` is not on PATH. Run everything from `/Users/aritti/Projects/openidea-plugins/plugins/dependency-audit` with `~/.cargo/bin/cargo` and `~/.cargo/bin/openidea-plugin`.
- Style: follow `plugins/jpa-assist` / `plugins/gitlab` (Rust, `?`-propagation, `let … else`). Comments only for *why*; a deliberate shortcut gets a `ponytail:` comment naming its ceiling.
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. A Spring Boot `pom.xml` / `build.gradle` where most deps have no version: those are skipped without error, versioned ones are still checked (Task 2 `pom_reads_versions_and_properties`, Task 3 `gradle_reads_string_coordinates`).
2. `package.json` / `composer.json` with ranges and no lockfile, or a `dev-main` lock entry: a range is never sent to OSV (Task 4 `package_json_without_lockfile_keeps_exact_versions_only`, `composer_json_takes_locked_versions_without_v_prefix`).
3. OSV unreachable, slow, or answering 5xx while the user opens a manifest: no crash, no notification balloon, a log line; the scan command does report the error (Task 6 `audit_reports_osv_errors`, Task 7 `an_osv_outage_logs_and_keeps_quiet`).
4. A `package-lock.json` with more than 1000 packages: split into several POSTs (Task 6 `audit_sends_at_most_1000_queries_per_request`).
5. Non-ASCII text before a dependency on the same line: the diagnostic and quick fix ranges use UTF-16 columns (Task 7 `diagnostic_range_counts_utf16_columns`).

---

## File Structure

```
plugins/dependency-audit/
  plugin.toml            # manifest: capabilities, provider, view, command, menu, settings
  settings.schema.json   # autoCheck
  Cargo.toml             # own [workspace]
  LICENSE                # MIT
  README.md              # what is checked, privacy, what is skipped
  src/lib.rs             # DependencyAudit: events, code actions, scan command, view
  src/manifest.rs        # Dep, kind, lockfile, parse (pom, gradle, npm, composer), bumped
  src/osv.rs             # Advisory, Finding, compare, nearest_fix, level, audit + storage cache
  src/tests.rs           # MockHost tests with a fake OSV (audit, events, code actions, scan, view)
LIMITATIONS.md           # (repo root) new section for aritti.dependency-audit
```

---

### Task 1: Scaffold the package

**Files:**
- Create: `plugins/dependency-audit/` via `~/.cargo/bin/openidea-plugin new plugins/dependency-audit`
- Modify: `plugins/dependency-audit/Cargo.toml`, `plugins/dependency-audit/plugin.toml`, `plugins/dependency-audit/src/lib.rs`
- Create: `plugins/dependency-audit/settings.schema.json`, `plugins/dependency-audit/LICENSE`

**Interfaces:**
- Produces: `#[derive(Default)] pub struct DependencyAudit` implementing `Plugin`, `Providers` (defaults), `Views` (`render` returns `View::message("…")`), registered with `register_plugin!(DependencyAudit, providers, views);` and `#[cfg(test)] mod tests;` added once `src/tests.rs` exists (Task 6).

- [ ] **Step 1: Branch and scaffold**

Run from the repo root: `git switch -c dependency-audit && ~/.cargo/bin/openidea-plugin new plugins/dependency-audit`
Expected: the directory with `plugin.toml`, `Cargo.toml` (own `[workspace]`), `src/lib.rs`.

- [ ] **Step 2: Fill `Cargo.toml`** with the Global Constraints dependencies, package name `openidea-plugin-dependency-audit`, `publish = false`, `crate-type = ["cdylib", "rlib"]`.

- [ ] **Step 3: Write `plugin.toml`**

Header fields from Global Constraints, plus:

```toml
description = "Checks Maven, Gradle, npm and Composer dependencies against the OSV.dev vulnerability database. Vulnerable dependencies are marked in pom.xml, build.gradle, build.gradle.kts, package.json, composer.json and their lockfiles when you open or save them, with a quick fix that upgrades to the nearest fixed version. Dependency Audit: Scan Project lists the project's vulnerable dependencies in the Vulnerable Dependencies tool window. Privacy: package names and versions (never source code) are sent to api.osv.dev, a free service run by Google. Turn off Check on open and save under Settings › Plugins › Dependency Audit to contact it only when you run a scan."
change-notes = "0.1.0: first release. OSV.dev checks of pom.xml, build.gradle, build.gradle.kts, package.json, package-lock.json, composer.json and composer.lock on open and save, an upgrade quick fix, and Scan Project with the Vulnerable Dependencies tool window."
wasm = "dist/plugin.wasm"
activation = ["on-event:file-opened", "on-event:file-saved", "on-view:vulnerabilities"]

[[contributes.providers]]
kinds = ["code-actions"]
languages = ["xml", "groovy", "kotlin", "json"]

[[contributes.commands]]  # id "scan", title "Dependency Audit: Scan Project", category "Dependency Audit"
[[contributes.menus]]     # menu "tools", command "scan"
[[contributes.views]]     # id "vulnerabilities", title "Vulnerable Dependencies", icon "warning", location "bottom"
[contributes.settings]    # title "Dependency Audit", schema "settings.schema.json"
```

- [ ] **Step 4: Write `settings.schema.json`**: one property `autoCheck`: `boolean`, title `Check on open and save (sends package names and versions to api.osv.dev)`, default `true`.

- [ ] **Step 5: Copy `plugins/devcontainer/LICENSE`** to `plugins/dependency-audit/LICENSE`.

- [ ] **Step 6: Stub `src/lib.rs`** with the Produces above; `run_command` returns `Err(Error::NotFound(format!("command {id}")))`.

- [ ] **Step 7: Build and check**

Run: `~/.cargo/bin/openidea-plugin build`
Expected: `dist/plugin.wasm` written, `check` reports no errors.

- [ ] **Step 8: Commit**

```bash
git add plugins/dependency-audit
git commit -m "Scaffold aritti.dependency-audit"
```

---

### Task 2: Maven `pom.xml` dependencies

**Files:**
- Create: `plugins/dependency-audit/src/manifest.rs` (tests inline in `#[cfg(test)] mod tests`)
- Modify: `plugins/dependency-audit/src/lib.rs` (add `mod manifest;`)

**Interfaces:**
- Produces:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)] pub enum Ecosystem { Maven, Npm, Packagist }` with `pub fn osv(self) -> &'static str` → `"Maven"`, `"npm"`, `"Packagist"`.
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Kind { Pom, Gradle, PackageJson, PackageLock, ComposerJson, ComposerLock }`.
  - `pub fn kind(path: &str) -> Option<Kind>` by file name: `pom.xml`, `build.gradle`, `build.gradle.kts`, `package.json`, `package-lock.json`, `composer.json`, `composer.lock`.
  - `pub fn lockfile(path: &str) -> Option<String>`: the sibling `package-lock.json` / `composer.lock` path for `PackageJson` / `ComposerJson`, else `None`.
  - `pub const GLOBS: &[&str]`: `**/` + each of the seven file names, manifests before lockfiles.
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Dep { pub path: String, pub ecosystem: Ecosystem, pub name: String, pub version: String, pub at: (usize, usize), pub version_at: Option<(usize, usize)> }`: `at` = bytes the diagnostic underlines; `version_at` = bytes the quick fix replaces (`None` in lockfiles).
  - `pub fn parse(path: &str, text: &str, lock: Option<&str>) -> Vec<Dep>`: dispatches on `kind(path)`; `lock` is the sibling lockfile text (ignored for other kinds).
- Consumes: `spring_text::xml::elements(text) -> Vec<Element>` (`crates/spring-text/src/xml.rs`).

- [ ] **Step 1: Write the failing tests**

```rust
const POM: &str = r#"<project>
  <properties><jackson.version>2.9.10</jackson.version></properties>
  <dependencies>
    <dependency><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>${jackson.version}</version></dependency>
    <dependency><groupId>org.yaml</groupId><artifactId>snakeyaml</artifactId><version> 1.33 </version></dependency>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-web</artifactId></dependency>
    <dependency><groupId>x</groupId><artifactId>y</artifactId><version>${missing}</version></dependency>
    <dependency><groupId>x</groupId><artifactId>z</artifactId><version>[1.0,2.0)</version></dependency>
  </dependencies>
  <build><plugins><plugin><groupId>org.apache.maven.plugins</groupId><artifactId>maven-jar-plugin</artifactId><version>3.0.0</version></plugin></plugins></build>
</project>"#;

fn found(deps: &[Dep]) -> Vec<(&str, &str)> {
    deps.iter().map(|d| (d.name.as_str(), d.version.as_str())).collect()
}

#[test]
fn pom_reads_versions_and_properties() {
    let deps = parse("/p/pom.xml", POM, None);

    assert_eq!(found(&deps), [("com.fasterxml.jackson.core:jackson-databind", "2.9.10"), ("org.yaml:snakeyaml", "1.33")]);
    assert_eq!(deps[0].ecosystem, Ecosystem::Maven);
    assert_eq!(deps[0].path, "/p/pom.xml");
    let (from, to) = deps[0].version_at.unwrap();
    assert_eq!(&POM[from..to], "2.9.10");
    let (from, to) = deps[1].version_at.unwrap();
    assert_eq!(&POM[from..to], "1.33");
    assert_eq!(&POM[deps[1].at.0..deps[1].at.1], "snakeyaml");
}

#[test]
fn kinds_and_lockfiles_by_file_name() {
    assert_eq!(kind("/p/api/build.gradle.kts"), Some(Kind::Gradle));
    assert_eq!(kind("/p/composer.lock"), Some(Kind::ComposerLock));
    assert_eq!(kind("/p/foopom.xml"), None);
    assert_eq!(lockfile("/p/web/package.json").as_deref(), Some("/p/web/package-lock.json"));
    assert_eq!(lockfile("/p/pom.xml"), None);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test manifest::tests`
Expected: FAIL (compile errors: `parse`, `kind`, `lockfile` not defined).

- [ ] **Step 3: Implement `kind`, `lockfile`, `GLOBS`, `Dep`, `Ecosystem`, `parse` and the private `pom(path, text) -> Vec<Dep>`**

`pom`: take `elements(text)`; properties = children of every `properties` element (name → trimmed text and its byte range); for each `dependency` element read its `groupId`/`artifactId`/`version` children (element text = `text[el.tag_end..]` up to the `</` before `el.end`, trimmed, offsets adjusted). A `version` of exactly `${name}` resolves through properties (its `version_at` is the property value's range). Skip: no version, an unresolved `${…}`, a version not starting with an ASCII digit (ranges). `plugin` elements are never read (only `dependency`).

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test manifest::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: read pom.xml dependencies"
```

---

### Task 3: Gradle string coordinates

**Files:**
- Modify: `plugins/dependency-audit/src/manifest.rs`

**Interfaces:**
- Consumes: `Dep`, `parse` from Task 2.
- Produces: private `gradle(path, text) -> Vec<Dep>` reached through `parse` for `Kind::Gradle`.

- [ ] **Step 1: Write the failing test**

```rust
const GRADLE: &str = "plugins {\n    id 'org.springframework.boot' version '3.1.0'\n}\ndependencies {\n    implementation 'org.yaml:snakeyaml:1.33'\n    implementation(\"com.google.guava:guava:31.0-jre\")\n    implementation \"org.x:y:$v\"\n    implementation 'org.springframework.boot:spring-boot-starter'\n}\n";

#[test]
fn gradle_reads_string_coordinates() {
    let deps = parse("/p/build.gradle.kts", GRADLE, None);

    assert_eq!(found(&deps), [("org.yaml:snakeyaml", "1.33"), ("com.google.guava:guava", "31.0-jre")]);
    let (from, to) = deps[1].version_at.unwrap();
    assert_eq!(&GRADLE[from..to], "31.0-jre");
    assert_eq!(&GRADLE[deps[0].at.0..deps[0].at.1], "org.yaml:snakeyaml:1.33");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `~/.cargo/bin/cargo test manifest::tests::gradle_reads_string_coordinates`
Expected: FAIL (empty list).

- [ ] **Step 3: Implement `gradle`**: every `'…'` or `"…"` literal on one line whose content splits on `:` into exactly three non-empty parts, group and artifact made of `[A-Za-z0-9_.-]`, version starting with an ASCII digit and containing no `$`. `at` = the literal's content, `version_at` = the third part.

- [ ] **Step 4: Run to verify it passes**

Run: `~/.cargo/bin/cargo test manifest::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src/manifest.rs
git commit -m "dependency-audit: read Gradle string coordinates"
```

---

### Task 4: npm and Composer manifests and lockfiles

**Files:**
- Modify: `plugins/dependency-audit/src/manifest.rs`

**Interfaces:**
- Consumes: `Dep`, `parse`, `found` (test helper) from Task 2.
- Produces: private `package_json`, `package_lock`, `composer_json`, `composer_lock`, and `fn find_entry(text: &str, key: &str, value: &str) -> Option<((usize, usize), (usize, usize))>` (byte ranges of the key's and the value's contents, without quotes, for the first `"key"\s*:\s*"value"`). `pub fn bumped(spec: &str, fix: &str) -> String`: the spec's leading non-digit characters (`^`, `~`, `>=`, `v`) followed by `fix`.

- [ ] **Step 1: Write the failing tests**

```rust
const PACKAGE: &str = r#"{"dependencies": {"lodash": "^4.17.15", "vue": "3.2.0", "local": "file:../x"},
 "devDependencies": {"@babel/traverse": "~7.22.0"}}"#;
const PACKAGE_LOCK: &str = r#"{"lockfileVersion": 3, "packages": {"": {"name": "app"},
 "node_modules/lodash": {"version": "4.17.15"}, "node_modules/@babel/traverse": {"version": "7.22.5"},
 "node_modules/foo/node_modules/minimist": {"version": "1.2.0"}, "node_modules/linked": {"link": true}}}"#;
const COMPOSER: &str = r#"{"require": {"php": ">=8.1", "ext-json": "*", "symfony/http-kernel": "^5.4", "guzzlehttp/guzzle": "7.4.0"},
 "require-dev": {"phpunit/phpunit": "^9.5"}}"#;
const COMPOSER_LOCK: &str = r#"{"packages": [{"name": "symfony/http-kernel", "version": "v5.4.20"}, {"name": "guzzlehttp/guzzle", "version": "7.4.0"}],
 "packages-dev": [{"name": "phpunit/phpunit", "version": "dev-main"}]}"#;

fn sorted(deps: &[Dep]) -> Vec<(&str, &str)> {
    let mut all = found(deps);
    all.sort();
    all
}

fn named<'a>(deps: &'a [Dep], name: &str) -> &'a Dep {
    deps.iter().find(|d| d.name == name).unwrap()
}

#[test]
fn package_json_takes_locked_versions() {
    let deps = parse("/p/package.json", PACKAGE, Some(PACKAGE_LOCK));

    assert_eq!(sorted(&deps), [("@babel/traverse", "7.22.5"), ("lodash", "4.17.15"), ("vue", "3.2.0")]);
    let lodash = named(&deps, "lodash");
    assert_eq!(lodash.ecosystem, Ecosystem::Npm);
    assert_eq!(&PACKAGE[lodash.at.0..lodash.at.1], "lodash");
    let (from, to) = lodash.version_at.unwrap();
    assert_eq!(&PACKAGE[from..to], "^4.17.15");
}

#[test]
fn package_json_without_lockfile_keeps_exact_versions_only() {
    assert_eq!(found(&parse("/p/package.json", PACKAGE, None)), [("vue", "3.2.0")]);
}

#[test]
fn package_lock_lists_every_installed_package() {
    let deps = parse("/p/package-lock.json", PACKAGE_LOCK, None);

    assert_eq!(sorted(&deps), [("@babel/traverse", "7.22.5"), ("lodash", "4.17.15"), ("minimist", "1.2.0")]);
    assert!(deps.iter().all(|d| d.version_at.is_none()));
    let lodash = named(&deps, "lodash");
    assert_eq!(&PACKAGE_LOCK[lodash.at.0..lodash.at.1], "node_modules/lodash");
}

#[test]
fn composer_json_takes_locked_versions_without_v_prefix() {
    let deps = parse("/p/composer.json", COMPOSER, Some(COMPOSER_LOCK));

    assert_eq!(sorted(&deps), [("guzzlehttp/guzzle", "7.4.0"), ("symfony/http-kernel", "5.4.20")]);
    assert_eq!(deps[0].ecosystem, Ecosystem::Packagist);
    assert_eq!(found(&parse("/p/composer.json", COMPOSER, None)), [("guzzlehttp/guzzle", "7.4.0")]);
}

#[test]
fn composer_lock_lists_every_package() {
    let deps = parse("/p/composer.lock", COMPOSER_LOCK, None);

    assert_eq!(sorted(&deps), [("guzzlehttp/guzzle", "7.4.0"), ("symfony/http-kernel", "5.4.20")]);
    let kernel = named(&deps, "symfony/http-kernel");
    assert_eq!(&COMPOSER_LOCK[kernel.at.0..kernel.at.1], "symfony/http-kernel");
}

#[test]
fn bumped_keeps_the_constraint_operator() {
    assert_eq!(bumped("^4.17.15", "4.17.21"), "^4.17.21");
    assert_eq!(bumped("2.12.3", "2.12.7.1"), "2.12.7.1");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test manifest::tests`
Expected: FAIL (the new tests).

- [ ] **Step 3: Implement**

Parse with `serde_json::Value`, locate offsets in the text with `find_entry` (a dep whose entry isn't found is skipped).
- `package_json`: `dependencies` + `devDependencies`. Version = `packages["node_modules/<name>"].version` of the lock, else the spec when it is exact (starts with a digit and holds only digits, `.`, `-`, `+` and letters after a `-`). Skip specs with `:` (`file:`, `workspace:`, `npm:`, git URLs). `at` = key, `version_at` = spec.
- `package_lock`: every `packages` key starting with `node_modules/` that has a `version` and no `link`; name = the part after the last `node_modules/`; `at` = the key's contents (find `"<key>"` in the text).
- `composer_json`: `require` + `require-dev`, names containing `/` only. Version from the lock's `packages`/`packages-dev` entry with that `name` (leading `v` stripped, `dev-` versions skipped), else the spec when exact.
- `composer_lock`: every entry of `packages` + `packages-dev`, same version rule; `at` = the value range from `find_entry(text, "name", <name>)`.

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test manifest::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src/manifest.rs
git commit -m "dependency-audit: read npm and Composer manifests and lockfiles"
```

---

### Task 5: Advisories, versions and findings (pure)

**Files:**
- Create: `plugins/dependency-audit/src/osv.rs` (tests inline)
- Modify: `plugins/dependency-audit/src/lib.rs` (add `mod osv;`)

**Interfaces:**
- Consumes: `Dep`, `Ecosystem` from Task 2.
- Produces:
  - `#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)] pub struct Advisory { pub id: String, pub modified: String, pub summary: String, pub severity: Option<String>, pub fixed: BTreeMap<String, Vec<String>> }`: `fixed` keyed `"{ecosystem}:{name}"` (OSV spelling), every `fixed` event of that package's ranges, in record order.
  - `pub fn parse_advisory(json: &str) -> Option<Advisory>`: severity = top-level `database_specific.severity`, else the first `affected[].ecosystem_specific.severity`, else `affected[].database_specific.severity`, uppercased.
  - `pub fn level(severity: Option<&str>) -> Severity`: `CRITICAL`/`HIGH` → `Error`, `MODERATE`/`MEDIUM` → `Warning`, `LOW` → `Info`, anything else or `None` → `Warning`.
  - `pub fn compare(a: &str, b: &str) -> std::cmp::Ordering`.
  - `pub fn nearest_fix(version: &str, fixed: &[String]) -> Option<String>`: the smallest fixed version greater than `version`.
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Finding { pub dep: Dep, pub advisories: Vec<Advisory> }` with `pub fn fix(&self) -> Option<String>` (the greatest of each advisory's `nearest_fix` for this dep; `None` when no advisory has one), `pub fn severity(&self) -> Severity` (the most severe level: Error over Warning over Info) and `pub fn message(&self) -> String`.

- [ ] **Step 1: Write the failing tests**

```rust
pub(crate) const JACKSON: &str = r#"{"id": "GHSA-57j2-w4cx-62h2", "modified": "2024-03-15T05:00:00Z",
 "summary": "Deeply nested JSON exhausts the stack", "database_specific": {"severity": "HIGH"},
 "affected": [{"package": {"ecosystem": "Maven", "name": "com.fasterxml.jackson.core:jackson-databind"},
   "ranges": [{"type": "ECOSYSTEM", "events": [{"introduced": "0"}, {"fixed": "2.12.7.1"}]},
              {"type": "ECOSYSTEM", "events": [{"introduced": "2.13.0"}, {"fixed": "2.13.4.1"}]}]}]}"#;

pub(crate) fn dep(ecosystem: Ecosystem, name: &str, version: &str) -> Dep {
    Dep { path: "/project/x".into(), ecosystem, name: name.into(), version: version.into(), at: (0, 0), version_at: None }
}

#[test]
fn compares_versions_numerically() {
    assert_eq!(compare("1.10.0", "1.9.0"), Ordering::Greater);
    assert_eq!(compare("2.0.0-rc1", "2.0.0"), Ordering::Less);
    assert_eq!(compare("1.0", "1.0.1"), Ordering::Less);
    assert_eq!(compare("31.0-jre", "31.0-jre"), Ordering::Equal);
}

#[test]
fn nearest_fix_is_the_smallest_fixed_version_above() {
    let fixed = vec!["2.9.10.8".to_string(), "2.12.7.1".into(), "2.13.4.1".into()];

    assert_eq!(nearest_fix("2.12.3", &fixed).as_deref(), Some("2.12.7.1"));
    assert_eq!(nearest_fix("2.14.0", &fixed), None);
}

#[test]
fn parses_an_advisory() {
    let a = parse_advisory(JACKSON).unwrap();

    assert_eq!(a.severity.as_deref(), Some("HIGH"));
    assert_eq!(a.fixed["Maven:com.fasterxml.jackson.core:jackson-databind"], ["2.12.7.1", "2.13.4.1"]);
    assert_eq!(level(a.severity.as_deref()), Severity::Error);
}

#[test]
fn severity_falls_back_to_ecosystem_specific_then_warning() {
    let a = parse_advisory(r#"{"id": "OSV-1", "modified": "m", "affected": [{"ecosystem_specific": {"severity": "low"}}]}"#).unwrap();
    assert_eq!(level(a.severity.as_deref()), Severity::Info);

    let b = parse_advisory(r#"{"id": "OSV-2", "modified": "m", "severity": [{"type": "CVSS_V3", "score": "CVSS:3.1/AV:N"}]}"#).unwrap();
    assert_eq!((b.severity, level(None)), (None, Severity::Warning));
}

#[test]
fn a_finding_upgrades_past_every_advisory_and_says_so() {
    let jackson = parse_advisory(JACKSON).unwrap();
    let older = Advisory {
        id: "GHSA-older".into(),
        severity: Some("MODERATE".into()),
        fixed: BTreeMap::from([("Maven:com.fasterxml.jackson.core:jackson-databind".into(), vec!["2.12.6.1".into()])]),
        ..Advisory::default()
    };
    let finding = Finding { dep: dep(Ecosystem::Maven, "com.fasterxml.jackson.core:jackson-databind", "2.12.3"), advisories: vec![jackson, older] };

    assert_eq!(finding.fix().as_deref(), Some("2.12.7.1"));
    assert_eq!(finding.severity(), Severity::Error);
    assert_eq!(
        finding.message(),
        "com.fasterxml.jackson.core:jackson-databind 2.12.3 has known vulnerabilities: GHSA-57j2-w4cx-62h2 (HIGH) Deeply nested JSON exhausts the stack; GHSA-older (MODERATE). Upgrade to 2.12.7.1."
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test osv::tests`
Expected: FAIL (compile errors).

- [ ] **Step 3: Implement**

`compare`: split both on `.`, `-`, `+`, `_`; compare segment pairs: both numeric → as `u64`; numeric vs text → numeric is greater; text vs text → ASCII case-insensitive. When one runs out: if the longer one's next segment is text, the longer is smaller (`2.0.0-rc1` < `2.0.0`), else it is greater (`1.0` < `1.0.1`). Mark with `// ponytail: dotted-number compare; Maven qualifiers (Final, SP) and SemVer build metadata aren't ranked, use the ecosystem's rules if fixes come out wrong`.
`message`: `"{name} {version} has known vulnerabilities: "` + advisories joined by `"; "` as `"{id} ({SEVERITY}) {summary}"` (omit the parenthesis without severity, omit a trailing space without summary) + `". Upgrade to {fix}."` or `". No fixed version is known."`.

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test osv::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: parse OSV advisories and pick the nearest fix"
```

---

### Task 6: Query OSV with a storage cache

**Files:**
- Modify: `plugins/dependency-audit/src/osv.rs`
- Create: `plugins/dependency-audit/src/tests.rs`
- Modify: `plugins/dependency-audit/src/lib.rs` (add `#[cfg(test)] mod tests;`)

**Interfaces:**
- Consumes: `Dep`, `Advisory`, `parse_advisory`, `Finding`, test helpers `JACKSON`, `dep` (Task 5).
- Produces:
  - `pub const QUERY_TTL_SECS: u64 = 86_400;`, `pub const BATCH: usize = 1000;`
  - `pub fn audit(deps: &[Dep], online: bool, now: u64) -> Result<Vec<Finding>, Error>`: one `Finding` per dep with at least one advisory, in `deps` order. Offline: cache only, never fetches.
  - Storage keys: `q:{ecosystem}:{name}@{version}` → `{"at": <secs>, "vulns": [["<id>", "<modified>"], …]}`; `v:{id}` → `Advisory` JSON.
  - Test fixtures in `src/tests.rs` used by Tasks 7–9: `CAPS`, `LODASH` (advisory `GHSA-35jh-r3h4-6jhm`, npm `lodash`, fixed `4.17.21`, severity `HIGH`), `fn osv(req: &HttpRequest) -> Result<HttpResponse, Error>` (fake OSV), `fn posts(host: &MockHost) -> usize`.

- [ ] **Step 1: Write the fake OSV and the failing tests in `src/tests.rs`**

```rust
use super::*;
use crate::manifest::Ecosystem;
use crate::osv::tests::{JACKSON, dep};
use openidea_plugin::mock::MockHost;
use openidea_plugin::{HttpRequest, HttpResponse};
use serde_json::{Value, json};

pub const CAPS: &[&str] = &["project.read", "storage", "events", "net:api.osv.dev"];

pub const LODASH: &str = r#"{"id": "GHSA-35jh-r3h4-6jhm", "modified": "2024-01-01T00:00:00Z", "summary": "Command injection in lodash",
 "database_specific": {"severity": "HIGH"},
 "affected": [{"package": {"ecosystem": "npm", "name": "lodash"}, "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": "4.17.21"}]}]}]}"#;

const VULNERABLE: &[(&str, &str, &str, &str)] = &[
    ("Maven", "com.fasterxml.jackson.core:jackson-databind", "2.12.3", "GHSA-57j2-w4cx-62h2"),
    ("npm", "lodash", "4.17.15", "GHSA-35jh-r3h4-6jhm"),
];

fn ok(body: String) -> Result<HttpResponse, Error> {
    Ok(HttpResponse { status: 200, headers: vec![], body: body.into_bytes() })
}

pub fn osv(req: &HttpRequest) -> Result<HttpResponse, Error> {
    match (req.method.as_str(), req.url.as_str()) {
        ("POST", "https://api.osv.dev/v1/querybatch") => {
            let body: Value = serde_json::from_slice(req.body.as_deref().unwrap()).unwrap();
            let results: Vec<Value> = body["queries"].as_array().unwrap().iter().map(|q| {
                let hit = VULNERABLE.iter().find(|(e, n, v, _)| q["package"]["ecosystem"] == *e && q["package"]["name"] == *n && q["version"] == *v);
                match hit {
                    Some((.., id)) => json!({"vulns": [{"id": id, "modified": if id.contains("57j2") { "2024-03-15T05:00:00Z" } else { "2024-01-01T00:00:00Z" }}]}),
                    None => json!({}),
                }
            }).collect();
            ok(json!({"results": results}).to_string())
        }
        ("GET", "https://api.osv.dev/v1/vulns/GHSA-57j2-w4cx-62h2") => ok(JACKSON.into()),
        ("GET", "https://api.osv.dev/v1/vulns/GHSA-35jh-r3h4-6jhm") => ok(LODASH.into()),
        _ => Ok(HttpResponse { status: 404, headers: vec![], body: vec![] }),
    }
}

pub fn posts(host: &MockHost) -> usize {
    host.requests().iter().filter(|r| r.method == "POST").count()
}

fn jackson() -> Dep {
    dep(Ecosystem::Maven, "com.fasterxml.jackson.core:jackson-databind", "2.12.3")
}

#[test]
fn audit_queries_osv_and_caches_results() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let deps = [jackson(), dep(Ecosystem::Maven, "org.yaml:snakeyaml", "2.2")];

    let findings = osv::audit(&deps, true, 1_000).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].dep.name, "com.fasterxml.jackson.core:jackson-databind");
    assert_eq!(findings[0].fix().as_deref(), Some("2.12.7.1"));
    assert_eq!(host.requests().len(), 2, "one batch, one advisory");
    assert_eq!(host.requests()[0].headers, [("Content-Type".to_string(), "application/json".to_string())]);

    assert_eq!(osv::audit(&deps, true, 2_000).unwrap(), findings);
    assert_eq!(host.requests().len(), 2, "answered from the cache");

    osv::audit(&deps, true, 1_000 + osv::QUERY_TTL_SECS + 1).unwrap();
    assert_eq!(host.requests().len(), 3, "the query expired; the advisory is unchanged");
}

#[test]
fn offline_audit_uses_only_the_cache() {
    let host = MockHost::install().with_capabilities(CAPS);

    assert_eq!(osv::audit(&[jackson()], false, 0).unwrap(), vec![]);
    assert!(host.requests().is_empty());
}

#[test]
fn audit_sends_at_most_1000_queries_per_request() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    let deps: Vec<Dep> = (0..1001).map(|i| dep(Ecosystem::Npm, &format!("pkg{i}"), "1.0.0")).collect();

    osv::audit(&deps, true, 0).unwrap();
    assert_eq!(posts(&host), 2);
}

#[test]
fn audit_reports_osv_errors() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));

    let error = osv::audit(&[jackson()], true, 0).unwrap_err();
    assert!(error.to_string().contains("503"), "{error}");
}

#[test]
fn a_failed_advisory_still_reports_its_id() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|r| if r.method == "GET" { Err(Error::Failed("timed out".into())) } else { osv(r) });

    let findings = osv::audit(&[jackson()], true, 0).unwrap();
    assert_eq!(findings[0].advisories[0].id, "GHSA-57j2-w4cx-62h2");
    assert_eq!(findings[0].advisories[0].severity, None);
    assert!(!host.storage().contains_key("v:GHSA-57j2-w4cx-62h2"), "a failed advisory isn't cached");
}
```

Make `osv.rs`'s test module `pub(crate) mod tests` so `JACKSON` and `dep` are importable.

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test tests::audit tests::offline tests::a_failed`
Expected: FAIL (`audit` not defined).

- [ ] **Step 3: Implement `audit`**

1. Unique `(ecosystem, name, version)` keys; a `q:` entry with `now - at < QUERY_TTL_SECS` is fresh.
2. If `online`, POST the missing ones in chunks of `BATCH` as `{"queries": [{"package": {"ecosystem", "name"}, "version"}]}`; a non-200 answer is `Err(Error::Failed(format!("OSV answered {status}")))`, a fetch error is returned as is. Store each result as a `q:` entry. `// ponytail: next_page_token ignored; OSV pages only past 1,000 vulns per query or 3,000 per batch`.
3. Per vuln id: a cached `v:` `Advisory` with the same `modified` is used; else (online) `GET /v1/vulns/{id}`, parse and store; on failure use `Advisory { id, modified, ..Default }` without storing. Offline and not cached: the dep is skipped.
4. A `storage::set` error: delete every `q:` and `v:` key (`storage::keys`) once and retry; ignore a second error (`// ponytail: 1 MiB storage; the whole cache is dropped when it fills`).

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test`
Expected: PASS (all tests so far).

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: query OSV in batches with a storage cache"
```

---

### Task 7: Diagnostics on open and save

**Files:**
- Modify: `plugins/dependency-audit/src/lib.rs`, `plugins/dependency-audit/src/tests.rs`

**Interfaces:**
- Consumes: `manifest::{kind, lockfile, parse}`, `osv::{audit, Finding}`, `spring_text::index::project_of`, `spring_text::range`.
- Produces (in `lib.rs`):
  - `fn now() -> u64` (`SystemTime::now()` since `UNIX_EPOCH`, seconds).
  - `fn read_deps(project: &Project, path: &str) -> Result<(String, Vec<Dep>), Error>`: the file's text and its deps (reads the sibling lockfile with `read_file`; a missing lockfile is `None`).
  - `fn diagnostic(finding: &Finding, text: &str) -> Diagnostic`: range `range(text, at.0, at.1)`, `finding.severity()`, `finding.message()`, `code` = first advisory id, `source` = `"OSV"`.
  - `Plugin::handle_events`: for each `FileOpened`/`FileSaved` with `kind(path).is_some()`, unless `host::setting_bool("autoCheck") == Some(false)`: `begin_task("Checking dependencies with OSV.dev", false)`, `audit(&deps, true, now())`, publish (empty list clears). On `Err`: `host::log(LogLevel::Warn, &format!("OSV check of {path}: {e}"))`, publish nothing.
  - Test fixture `POM` in `src/tests.rs` (below), reused by Tasks 8–9.

- [ ] **Step 1: Write the failing tests**

```rust
pub const POM: &str = "<project>\n  <dependencies>\n    <dependency><groupId>org.yaml</groupId><artifactId>snakeyaml</artifactId><version>2.2</version></dependency>\n    <dependency><!-- 😀 --><groupId>com.fasterxml.jackson.core</groupId><artifactId>jackson-databind</artifactId><version>2.12.3</version></dependency>\n  </dependencies>\n</project>\n";
pub const PACKAGE: &str = "{\n  \"dependencies\": {\n    \"lodash\": \"^4.17.15\"\n  }\n}\n";
pub const PACKAGE_LOCK: &str = r#"{"lockfileVersion": 3, "packages": {"": {}, "node_modules/lodash": {"version": "4.17.15"}}}"#;

fn opened(host: &MockHost, path: &str) {
    DependencyAudit::default().handle_events(&[Event::FileOpened(path.into())], &host.invocation());
}

#[test]
fn opening_a_pom_publishes_a_diagnostic_on_the_dependency() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    let problems = &host.diagnostics()["/project/pom.xml"];
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].severity, Severity::Error);
    assert_eq!(problems[0].code.as_deref(), Some("GHSA-57j2-w4cx-62h2"));
    assert_eq!(problems[0].source.as_deref(), Some("OSV"));
    assert!(problems[0].message.ends_with("Upgrade to 2.12.7.1."), "{}", problems[0].message);
}

#[test]
fn diagnostic_range_counts_utf16_columns() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    let range = host.diagnostics()["/project/pom.xml"][0].range;
    assert_eq!((range.start.line, range.start.character, range.end.character), (3, 84, 100));
}

#[test]
fn package_json_uses_the_sibling_lockfile() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("web/package.json", PACKAGE);
    host.add_file("web/package-lock.json", PACKAGE_LOCK);

    opened(&host, "/project/web/package.json");

    assert_eq!(host.diagnostics()["/project/web/package.json"][0].range.start.line, 2);
}

#[test]
fn auto_check_off_sends_nothing() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.set_setting("autoCheck", "false");
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    assert!(host.requests().is_empty());
    assert!(host.diagnostics().is_empty());
}

#[test]
fn other_files_are_ignored() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);

    DependencyAudit::default().handle_events(&[Event::FileSaved("/project/src/App.java".into())], &host.invocation());

    assert!(host.requests().is_empty());
}

#[test]
fn an_osv_outage_logs_and_keeps_quiet() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Err(Error::Failed("offline".into())));
    host.add_file("pom.xml", POM);

    opened(&host, "/project/pom.xml");

    assert!(host.diagnostics().is_empty());
    assert!(host.notifications().is_empty());
    assert!(host.logs().iter().any(|(_, line)| line.contains("OSV check of /project/pom.xml: offline")));
}
```

(`84`: 4 spaces + `<dependency>` 12 + `<!-- ` 5 + 😀 2 UTF-16 units + ` -->` 4 + `<groupId>` 9 + 26 + `</groupId>` 10 + `<artifactId>` 12; `100` = 84 + `jackson-databind` 16.)

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test tests::`
Expected: the six new tests FAIL (no diagnostics published).

- [ ] **Step 3: Implement `now`, `read_deps`, `diagnostic` and `handle_events`** per the Interfaces block.

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: mark vulnerable dependencies on open and save"
```

---

### Task 8: Upgrade quick fix

**Files:**
- Modify: `plugins/dependency-audit/src/lib.rs`, `plugins/dependency-audit/src/tests.rs`

**Interfaces:**
- Consumes: `manifest::{kind, parse, lockfile, bumped}`, `osv::audit` (offline), `spring_text::{range, index::project_of}`, fixtures `POM`, `PACKAGE`, `PACKAGE_LOCK`, `opened` (Task 7).
- Produces: `Providers::code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction>`: for `kind(doc.path)` set, parse `doc.text` (lockfile read through `project_of(&doc.path)`), `audit(&deps, false, now())`, and for each finding with `version_at` and `fix()` whose `at` lines overlap `selection`'s lines: `CodeAction { title: format!("Upgrade {short} to {fix}"), kind: Some("quickfix"), preferred: true, edits: vec![(doc.path, vec![TextEdit { range: range(text, version_at), text: bumped(&text[version_at], &fix) }])], data: None }`, `short` = the name after the last `:` (Maven/Gradle), else the full name.

- [ ] **Step 1: Write the failing tests**

```rust
fn document(path: &str, language: &str, text: &str) -> openidea_plugin::providers::Document {
    openidea_plugin::providers::Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
}

fn line(n: u32) -> Range {
    Range::new(Position::new(n, 0), Position::new(n, 0))
}

#[test]
fn quick_fix_upgrades_to_the_nearest_fixed_version() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    opened(&host, "/project/pom.xml");
    let sent = host.requests().len();

    let actions = DependencyAudit::default().code_actions(&document("/project/pom.xml", "xml", POM), line(3));

    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "Upgrade jackson-databind to 2.12.7.1");
    assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
    let (path, edits) = &actions[0].edits[0];
    assert_eq!(path, "/project/pom.xml");
    assert_eq!(edits[0].text, "2.12.7.1");
    assert_eq!(edits[0].range.start, Position::new(3, 122));
    assert_eq!(host.requests().len(), sent, "no network on the edit path");
}

#[test]
fn quick_fix_keeps_the_npm_range_operator() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("package.json", PACKAGE);
    host.add_file("package-lock.json", PACKAGE_LOCK);
    opened(&host, "/project/package.json");

    let actions = DependencyAudit::default().code_actions(&document("/project/package.json", "json", PACKAGE), line(2));

    assert_eq!(actions[0].title, "Upgrade lodash to 4.17.21");
    assert_eq!(actions[0].edits[0].1[0].text, "^4.17.21");
}

#[test]
fn no_quick_fix_without_a_cached_result() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);

    let actions = DependencyAudit::default().code_actions(&document("/project/pom.xml", "xml", POM), line(3));

    assert!(actions.is_empty());
    assert!(host.requests().is_empty());
}

#[test]
fn no_quick_fix_off_the_dependency_line_or_in_lockfiles() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    host.add_file("package-lock.json", PACKAGE_LOCK);
    opened(&host, "/project/pom.xml");
    opened(&host, "/project/package-lock.json");
    let mut plugin = DependencyAudit::default();

    assert!(plugin.code_actions(&document("/project/pom.xml", "xml", POM), line(2)).is_empty());
    assert!(plugin.code_actions(&document("/project/package-lock.json", "json", PACKAGE_LOCK), line(0)).is_empty());
}
```

(`122` = 100 (end of `jackson-databind`, Task 7) + `</artifactId>` 13 + `<version>` 9.)

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test tests::quick_fix tests::no_quick_fix`
Expected: FAIL (no actions).

- [ ] **Step 3: Implement `code_actions`** per the Interfaces block.

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: quick fix to the nearest fixed version"
```

---

### Task 9: Scan Project and the Vulnerable Dependencies view

**Files:**
- Modify: `plugins/dependency-audit/src/lib.rs`, `plugins/dependency-audit/src/tests.rs`

**Interfaces:**
- Consumes: `manifest::GLOBS`, `read_deps`, `diagnostic`, `osv::{audit, level, Advisory}`, `spring_text::tree::Expansion`, fixtures from Tasks 6–7.
- Produces:
  - `#[derive(Serialize, Deserialize)] struct Row { path: String, line: u32, name: String, version: String, fix: Option<String>, advisories: Vec<Advisory> }`, stored as a JSON array under storage key `scan`.
  - `DependencyAudit { expansion: Expansion, targets: HashMap<String, Target> }`, `enum Target { Location(String, u32), Url(String) }` (filled by `render`, like `jpa-assist`'s `locations`).
  - `fn scan(project: &Project) -> Result<String, Error>`: returns the notification body. `begin_task("Scanning dependencies", true)`; `find_files(glob, 200)` per `GLOBS`, keep `kind(path).is_some()`; `read_deps` each; drop deps already seen as `(ecosystem, name, version)` (manifests come first, so a lockfile only adds transitive ones); one `audit(&all, true, now())`; publish diagnostics per scanned file (empty clears); store rows; `refresh_view("vulnerabilities")`. Between files: `task.is_cancelled()` → `Err(Error::Cancelled)`.
  - Notification: `host::notify(Info, "Dependency Audit", body)` with body `"Vulnerable dependencies: {v} of {n} ({f} files). See the Vulnerable Dependencies tool window."`, or `"No known vulnerabilities in {n} dependencies ({f} files)."`.
  - `run_command("scan")`: `ctx.project().ok_or("Open a project first.")?`, `scan`, notify, `Ok(None)`.
  - `Views::render("vulnerabilities")`: no `scan` key → `View::message("Run Dependency Audit: Scan Project to check this project's dependencies against OSV.dev.")` with button `scan` "Scan". Empty rows → `View::message("No known vulnerabilities found.")` with the same button labelled "Rescan". Else `Body::Tree`: per file an item (key = path, label = path relative to the project root, icon `file`), per row a child (label `"{name} {version}"`, detail `"fix: {fix}"` or `"no fix"`, icon `package`), per advisory a grandchild (label = id, detail = summary, icon `error`/`warning`/`info` from `level`); `expansion.apply(&mut items)`; status `"{rows} vulnerable dependencies"`; button `scan` "Rescan" icon `refresh`.
  - `Views::handle`: `expansion.handle(&event)` → `refresh_view`; `Button("scan")` → `scan` + notify; `Activate(key)` → `Target::Location(path, line)` → `host::open_location(&path, line, 0)`, `Target::Url(url)` → `host::open_url(&url)` with `https://osv.dev/vulnerability/{id}`.

- [ ] **Step 1: Write the failing tests**

```rust
fn scanned(host: &MockHost) -> DependencyAudit {
    host.on_fetch(osv);
    host.add_file("pom.xml", POM);
    host.add_file("web/package.json", PACKAGE);
    host.add_file("web/package-lock.json", PACKAGE_LOCK);
    let mut plugin = DependencyAudit::default();
    plugin.run_command("scan", &host.invocation(), None).unwrap();
    plugin
}

fn tree(plugin: &mut DependencyAudit, host: &MockHost) -> Vec<TreeItem> {
    match plugin.render("vulnerabilities", None, &host.invocation()).unwrap().body {
        Body::Tree(items) => items,
        other => panic!("a tree, got {other:?}"),
    }
}

#[test]
fn scan_lists_vulnerable_dependencies_in_the_view() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut plugin = scanned(&host);

    assert_eq!(host.notifications()[0].body, "Vulnerable dependencies: 2 of 3 (3 files). See the Vulnerable Dependencies tool window.");
    assert!(host.refreshed_views().contains(&"vulnerabilities".to_string()));
    assert_eq!(host.diagnostics().keys().collect::<Vec<_>>(), ["/project/pom.xml", "/project/web/package.json"]);

    let labels: Vec<String> = tree(&mut plugin, &host).into_iter().map(|i| i.label).collect();
    assert_eq!(labels.iter().filter(|l| l.starts_with("lodash")).count(), 1, "the lockfile adds no duplicate");
    assert!(labels.contains(&"com.fasterxml.jackson.core:jackson-databind 2.12.3".to_string()));
    assert!(labels.contains(&"GHSA-35jh-r3h4-6jhm".to_string()));
}

#[test]
fn the_view_survives_a_new_instance() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut first = scanned(&host);
    let before = tree(&mut first, &host);

    assert_eq!(tree(&mut DependencyAudit::default(), &host), before);
}

#[test]
fn activating_items_opens_the_line_or_osv_dev() {
    let host = MockHost::install().with_capabilities(CAPS);
    let mut plugin = scanned(&host);
    let items = tree(&mut plugin, &host);
    let key = |label: &str| items.iter().find(|i| i.label == label).unwrap().key.clone();

    plugin.handle("vulnerabilities", ViewEvent::Activate(key("GHSA-57j2-w4cx-62h2")), &host.invocation()).unwrap();
    plugin.handle("vulnerabilities", ViewEvent::Activate(key("com.fasterxml.jackson.core:jackson-databind 2.12.3")), &host.invocation()).unwrap();

    assert_eq!(host.opened_urls(), ["https://osv.dev/vulnerability/GHSA-57j2-w4cx-62h2"]);
    assert_eq!(host.opened_locations(), [("/project/pom.xml".to_string(), 3, 0)]);
}

#[test]
fn before_a_scan_the_view_offers_to_scan() {
    let host = MockHost::install().with_capabilities(CAPS);

    let view = DependencyAudit::default().render("vulnerabilities", None, &host.invocation()).unwrap();

    assert!(matches!(view.body, Body::Message(_)));
    assert_eq!(view.toolbar[0].id, "scan");
}

#[test]
fn a_failed_scan_tells_the_user() {
    let host = MockHost::install().with_capabilities(CAPS);
    host.on_fetch(|_| Ok(HttpResponse { status: 503, headers: vec![], body: vec![] }));
    host.add_file("pom.xml", POM);

    let error = DependencyAudit::default().run_command("scan", &host.invocation(), None).unwrap_err();

    assert!(error.to_string().contains("OSV answered 503"));
}
```

(3 deps: snakeyaml, jackson-databind, lodash; the lockfile's lodash is the same key and is dropped.)

- [ ] **Step 2: Run to verify they fail**

Run: `~/.cargo/bin/cargo test tests::scan tests::the_view tests::activating tests::before_a_scan tests::a_failed_scan`
Expected: FAIL (`command scan` not found).

- [ ] **Step 3: Implement `Row`, `Target`, `scan`, `run_command("scan")`, `render`, `handle`** per the Interfaces block.

- [ ] **Step 4: Run to verify they pass**

Run: `~/.cargo/bin/cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add plugins/dependency-audit/src
git commit -m "dependency-audit: Scan Project and the Vulnerable Dependencies view"
```

---

### Task 10: README, LIMITATIONS, build and try it in the IDE

**Files:**
- Create: `plugins/dependency-audit/README.md`
- Modify: `LIMITATIONS.md` (append a section)

- [ ] **Step 1: Write `README.md`** in the style of `plugins/devcontainer/README.md`: a table of checked files (manifest → versions used), the quick fix, the command and view, the `autoCheck` setting, a **Privacy** paragraph (package names and versions go to `api.osv.dev`, run by Google; nothing else; turn off the setting to contact it only on a scan), **Not checked** (versionless BOM/parent-managed Maven and Gradle deps, ranges without a lockfile, `libs.versions.toml`, yarn/pnpm lockfiles, `package-lock.json` v1), and the build commands.

- [ ] **Step 2: Append `## aritti.dependency-audit (clean-room, inspired by JetBrains' package vulnerability checks)` to `LIMITATIONS.md`**, checked against v0.1.37, with `### Missing API` entries (Blocks / Why / Workaround / Would need, like the sections above it):
  - **No build model:** BOM/parent-managed versions can't be resolved; would need the IDE's Maven/Gradle model through WIT, or accepting `process:mvn`.
  - **Diagnostics can't link to the advisory:** `Diagnostic` has `code` but no URL (`sdk/rust/openidea-plugin/src/types.rs` `Diagnostic`); workaround: the view links to osv.dev; would need LSP's `codeDescription.href`.
  - **No lockfile-changed event:** `npm install` / `composer update` outside the IDE don't trigger a re-check; workaround: re-open or scan; would need `files-changed` (already proposed under Symfony).
  - **Every file open wakes the plugin:** events can't be filtered by glob (already under Symfony "Events can't be filtered"); add a one-line pointer.
  - **Network is synchronous within the call budget:** 5 s per event batch, 60 s with a task; a very large first scan may time out; would need a background fetch/task API.
  - `### Not done, but possible with the current API`: `libs.versions.toml`, yarn/pnpm lockfiles, Cargo and PyPI, an ignore list, CVSS scoring.

- [ ] **Step 3: Full build**

Run: `~/.cargo/bin/cargo test && ~/.cargo/bin/openidea-plugin build`
Expected: all tests PASS; `check` reports no errors.

- [ ] **Step 4: Clean-room check**

Run: `grep -ri "package checker" /Users/aritti/Projects/openidea-plugins/plugins/dependency-audit --exclude-dir=target --exclude-dir=dist`
Expected: no output.

- [ ] **Step 5: Manual check in the IDE**

Run: `~/.cargo/bin/openidea-plugin link`, then in OpenIDEA open a project with a `pom.xml` containing `jackson-databind` `2.12.3` and a `package.json` + `package-lock.json` with `lodash` `4.17.15`.
Expected: approve the capabilities; squiggles on both names within a few seconds; ⌥⏎ offers "Upgrade … to …" and applies it; Tools › Dependency Audit: Scan Project shows the notification and fills Vulnerable Dependencies. Note in the report whether ⌥⏎'s `selection` was the caret (LIMITATIONS.md "Code actions don't get the diagnostic").

- [ ] **Step 6: Commit**

```bash
git add plugins/dependency-audit/README.md LIMITATIONS.md
git commit -m "dependency-audit: README and API limitations"
```
