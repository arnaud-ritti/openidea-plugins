# Dependency Audit

Checks the dependencies of a project against [OSV.dev](https://osv.dev), the open
vulnerability database. A vulnerable dependency is underlined in its manifest when you open
or save the file, with a quick fix that upgrades it.

| File | Versions checked |
|---|---|
| `pom.xml` | `<dependency>` entries with a literal version or a `${property}` defined in the same file |
| `build.gradle`, `build.gradle.kts` | `'group:artifact:1.2.3'` string coordinates with a literal version |
| `package.json` | the version in `package-lock.json` next to it; else an exact version (`"4.17.21"`) in `dependencies` or `devDependencies` |
| `package-lock.json` | every locked package (lockfile v2 and v3) |
| `composer.json` | the version in `composer.lock` next to it; else an exact version in `require` or `require-dev` |
| `composer.lock` | every locked package |
| `libs.versions.toml` | `[libraries]` entries as Maven dependencies: `module` or `group`+`name` with a literal `version` or a `version.ref` from `[versions]`, or `"g:a:version"`; entries without a resolvable version are skipped |
| `yarn.lock` | every locked npm package (v1 and Berry) |
| `pnpm-lock.yaml` | every locked npm package in `packages:` (lockfile v6 and v9) |
| `Cargo.lock` | every registry crate (workspace members and path or git packages are skipped) |
| `requirements.txt` | `name==version` pins (PyPI) |
| `poetry.lock` | every locked PyPI package |

A manifest with a lockfile is checked at the locked versions, so `^4.17.15` is judged by what is
installed, not by the range.

## Quick fix

⌥⏎ on an underlined dependency offers **Upgrade … to …**: the nearest fixed version above the
current one (a `^` or `~` in front stays). It is offered only for a single constraint, not for
ranges like `>=1 <2`, and only once per Maven property shared by several dependencies, with
the highest fix among them. Lockfile entries have no quick fix: update them with your package manager.
After upgrading a range in `package.json` or `composer.json`, the underline stays until the lockfile
follows: run `npm install` / `composer update`, then reopen the file.

## Command and view

**Tools › Dependency Audit: Scan Project** checks every supported file in the project and shows a
notification with the totals. The **Vulnerable Dependencies** tool window lists each vulnerable
dependency once, under the first file that declares it, with its advisories below (ids link to
osv.dev, and the fix version is shown next to the dependency). The scan marks every
manifest and lockfile that contains a vulnerable dependency. If the results are too big to keep
for the tool window, the notification says so.

## Settings

Settings › Plugins › Dependency Audit › **Check on open and save** (`autoCheck`, on by default).
Off: nothing is sent until you run a scan.

**Ignored advisories** (`ignoredAdvisories`): one advisory id per line (`GHSA-…`, `CVE-…`), `#` starts a comment.
Ids are case-insensitive and match the advisory's id or any of its aliases (a `CVE-…` works for a `GHSA-…` advisory once its details are fetched). Listed ids are not reported, neither as squiggles nor in the Vulnerable Dependencies view.

## Privacy

Package names and versions, never source code, are sent to `api.osv.dev`, a free service run by
Google. Nothing else leaves your machine. Answers are kept in the plugin's storage for 24 hours
(one cache key), so reopening a file doesn't ask again. Advisory ids that look unsafe are shown
but never fetched. Turn off the setting to contact OSV only when you run a scan.

## Not checked

- Maven and Gradle dependencies without a version (managed by a BOM, a parent POM or a
  plugin such as Spring Boot's), and versions taken from variables.
- Version ranges with no lockfile, and `package-lock.json` v1.
- Catalog entries declared over several lines, `version` ranges (`strictly`, `require`), and `version.ref` keys missing from `[versions]`.
- pnpm lockfile v5 (`/name/1.2.3` keys), and `requirements.txt` lines that aren't an exact `==` pin.
- Other ecosystems (Go…).

No new dependencies: the new formats are read by hand.

See `LIMITATIONS.md` at the repository root for the API gaps behind these.

## Permissions

| Permission | Used for |
|---|---|
| `net:api.osv.dev` (required) | Query the OSV.dev vulnerability database. Only package names and versions are sent (see Privacy). No other host is contacted |
| `project.read` (required) | Read the manifests and lockfiles in the table above |
| `storage` (required) | Cache scan results so unchanged dependencies are not queried again |
| `events` (optional) | Re-check when a manifest is opened or saved. Without it, run the scan from the command |

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
