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

## Privacy

Package names and versions, never source code, are sent to `api.osv.dev`, a free service run by
Google. Nothing else leaves your machine. Answers are kept in the plugin's storage for 24 hours
(one cache key), so reopening a file doesn't ask again. Advisory ids that look unsafe are shown
but never fetched. Turn off the setting to contact OSV only when you run a scan.

## Not checked

- Maven and Gradle dependencies without a version (managed by a BOM, a parent POM or a
  plugin such as Spring Boot's), and versions taken from variables.
- Version ranges with no lockfile, and `package-lock.json` v1.
- Gradle version catalogs (`libs.versions.toml`).
- `yarn.lock` and `pnpm-lock.yaml`.
- Other ecosystems (Cargo, PyPI, Go…).

See `LIMITATIONS.md` at the repository root for the API gaps behind these.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
