---
name: publishing-openidea-plugins
description: Use when releasing, publishing or listing an aritti.* OpenIDEA plugin in the community registry l0ng-ai/openidea-plugins, adding a new version of an already listed plugin, or registering the aritti publisher, signing key, publishers/*.toml or plugins/*.toml entries.
---

# Publishing to the community registry

Registry: `l0ng-ai/openidea-plugins`. It lists packages, it does not host source. CI downloads each package, checks SHA-256, runs `openidea-plugin check`, verifies the signature against the publisher's keys, then merges into the marketplace within the hour. Re-read its README if a step seems stale.

## Release a version

1. Bump `version` and `change-notes` in `plugins/<name>/plugin.toml`. Set `categories` (max 3 of `languages themes icon-themes linters vcs ai frameworks tools other`).
2. Verify and sign (key lives outside the repo):
   ```sh
   (cd plugins/<name> && cargo test && openidea-plugin build)
   openidea-plugin pack plugins/<name> --key <path outside repo> --publisher aritti   # from repo root
   openidea-plugin check aritti.<name>-<version>.oiplugin
   ```
3. Push the commit, then attach the `.oiplugin` to a GitHub release of **this** repo. Monorepo: tag per plugin, `<name>-v<version>`, never a bare `v<version>`.
   ```sh
   gh release create <name>-v<version> aritti.<name>-<version>.oiplugin --title "<name> <version>" --notes "<change-notes>"
   shasum -a 256 aritti.<name>-<version>.oiplugin
   ```
4. Fork `l0ng-ai/openidea-plugins`, branch, add or edit the files below, open a PR using its template (plugin id + repo, what changed, the two checkboxes).

## Automated path

Two workflows. Use conventional commits (`feat(quarkus): …`, `fix:`, `feat!:`); release-please attributes them to plugins by changed path.

1. `release-please.yml` (push to main): opens one release PR per changed plugin, bumping `version` in `plugin.toml` (the `# x-release-please-version` marker, keep it) and writing `plugins/<name>/CHANGELOG.md`. Merging the PR tags `<name>-v<version>`, creates the GitHub release and dispatches `release.yml`.
2. `release.yml` (tag `<name>-v<version>`, or dispatched): fills `change-notes` from commit subjects since the previous tag (CI only, not committed), tests, builds, packs, checks, uploads the `.oiplugin` to the release, opens the registry PR.

First release of a plugin never tagged (manifest already at its version, e.g. quarkus 0.1.0): push the tag by hand, `git tag <name>-v<version> && git push origin <name>-v<version>`.

Setup: secrets `OIPLUGIN_KEY` (key file contents), `REGISTRY_TOKEN` (PAT that can fork and open PRs on the registry). `publishers/aritti.toml` PR stays manual. New plugin: add it to `release-please-config.json` and `.release-please-manifest.json`, and put the marker on its `version` line.

## Registry files

`plugins/aritti.<name>.toml`: new plugin writes `repository`; every release **appends** a `[[versions]]` table and keeps the older ones (one per plugin API requirement is what the index keeps).

```toml
repository = "https://github.com/<owner>/<this repo>"

[[versions]]
version = "0.1.0"
package = "https://github.com/<owner>/<this repo>/releases/download/<name>-v0.1.0/aritti.<name>-0.1.0.oiplugin"
sha256 = "<shasum output>"
```

`publishers/aritti.toml`, first time only (public key from `openidea-plugin keygen`):

```toml
name = "aritti"
owners = ["<github login>"]
keys = ["<public key>"]
revoked-keys = []
```

## Review rules

New publisher or new plugin gets a maintainer review. A PR body must say why each `network:`/`process:` or other high-risk capability is needed, and the plugin README must too. Source must be public and open-source licensed; package must be built from it. Ports keep `LICENSE`/`NOTICE` in the package (see `porting-intellij-plugins-to-openidea`).

## Mistakes

| Mistake | Fix |
|---|---|
| Replaced the `[[versions]]` table | Append; older OpenIDEA users depend on the old one |
| Packed from a dirty tree / stale `dist/` | `openidea-plugin build` first, pack the committed state |
| sha256 of a different file than the uploaded asset | Hash the exact file attached to the release; re-upload means re-hash |
| Release URL differs from `package =` | `gh release view <tag>` and copy the asset URL |
| Package not signed or wrong `--publisher` | Id prefix must be `aritti.`; CI verifies against `keys` |
| Key committed or pasted into a PR | Never; only the **public** key goes in `publishers/` |
| Key rotation | Add new key first, keep old, `openidea-plugin endorse "<new>" --key old.key --publisher aritti`, add `[endorsements]` |
| Plugin broken by a new OpenIDEA release | Listing is delisted after 30 days without a fixed version |

Opening the PR, creating the release and pushing publish externally: confirm with the user before running `gh release create` / `gh pr create`.
