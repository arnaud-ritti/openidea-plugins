# Upstream: plugin right-click menus (index)

Four upstream contributions to `l0ng-ai/openidea` that remove the right-click
menu limitations in `LIMITATIONS.md` (kube-explorer l.538, JPA Assist l.500–511,
gitlab l.711, devcontainer l.354). One issue and one PR each, as `CONTRIBUTING.md`
requires.

| # | Plan | WIT change | API bump | Unblocks |
|---|---|---|---|---|
| A | [`when` tests project files](2026-11-05-upstream-workspace-contains.md) — issue [#1612](https://github.com/l0ng-ai/openidea/issues/1612), PR [#1620](https://github.com/l0ng-ai/openidea/pull/1620); merged in fork `main` | no | no | devcontainer |
| B | [right-click menu on plugin view items](2026-11-05-upstream-view-item-menu.md) — issue [#1618](https://github.com/l0ng-ai/openidea/issues/1618), PR [#1746](https://github.com/l0ng-ai/openidea/pull/1746) | no | decision (see plan) | kube-explorer, JPA Assist |
| C | [`invocation.selected-paths`](2026-11-05-upstream-selected-paths.md) — issue [#1621](https://github.com/l0ng-ai/openidea/issues/1621), PR [#1744](https://github.com/l0ng-ai/openidea/pull/1744) | yes | **1.2.0** | gitlab |
| D | [`ui.set-context`](2026-11-05-upstream-set-context.md) — issue [#1622](https://github.com/l0ng-ai/openidea/issues/1622), PR [#1762](https://github.com/l0ng-ai/openidea/pull/1762) | yes | 1.2.0 (shares C's bump) | JPA Assist |

Order: A, B, C, D. A and B touch no WIT; C carries the 1.2.0 bump; D rebases on C
(or carries the bump itself if it lands first; see C, Task 1).

## Contribution workflow (every plan)

1. **Search for duplicates.** Done 2026-11-05 (`gh issue list -R l0ng-ai/openidea --state all --search …`
   for "context menu plugin", "tree item", "when clause", "selected paths", "context key",
   "workspaceContains"): no match. Search again right before you open the issue.
2. **Open the feature issue** with the *Feature request* form, using the draft in the plan:
   Problem, Proposed solution, Acceptance criteria (one observable behaviour per line),
   Alternatives. Opening it publishes text, so ask the user first.
3. **Wait for `planned`.** `needs-decision` → answer the questions in the issue and stop.
   Precedent: #1349 (`needs-decision`) → PR #1366 (API 1.1).
4. **Worktree from upstream**, not the fork's `main` (it is behind):
   `git fetch upstream && git worktree add ../openidea-<slug> -b feat/<slug> upstream/main && cd ../openidea-<slug> && pnpm install`.
5. **TDD**, following the plan's tasks. Commits: Conventional Commits, scope `plugins`.
6. **Checks before the PR** (one at a time; cold builds take ~5 min):
   - Rust: `cargo fmt --all --check`; `cargo clippy -p ide-extension -p ide-plugin -p ide-host --all-targets -- -D warnings`;
     `cargo nextest run -p ide-extension -p ide-plugin -p ide-host`.
   - SDK (C, D): `cargo test --manifest-path sdk/rust/openidea-plugin/Cargo.toml`.
   - WIT or SDK changed (C, D): `plugins/build.sh`, then `plugins/build.sh --check`.
   - IPC types changed (C, D): `pnpm gen:bindings`, commit `src/bindings.ts`.
   - Frontend: `pnpm typecheck`, `pnpm lint`, `pnpm format:check`, `pnpm knip`,
     `pnpm vitest run src/features/plugins`, `pnpm build`.
   - Runtime: `/verify` skill (Tauri MCP), before/after screenshots of the menu.
7. **Registry side.** Like #1366, link the plugin that uses the new API: release the
   `aritti.*` plugin with `engines.openidea` set to the new version and open its
   registry PR on `l0ng-ai/openidea-plugins` after the upstream PR merges.
8. **PR** with `.github/pull_request_template.md`: `Closes #<n>`, *What and why*
   (2–4 sentences), *How it was verified* (commands + results, screenshots), *Review focus*.
   Title e.g. `feat(plugins): right-click menu on plugin view items`.
9. **Clean up:** stop the worktree's app, delete `target/`, `git worktree remove`.
10. **Update `LIMITATIONS.md`** once merged: drop or rewrite the entries.

## API rules that shaped the plans (`plugins/README.md` §10)

- Released records, variants and signatures never change. `tree-item` can't gain an
  `actions` field, and `run-command` can't gain a parameter.
- Host interfaces may gain functions (resource methods included); plugin exports may not.
- `wit/` version, `ENGINE_VERSION` (`crates/ide-extension/src/manifest.rs:35`) and
  `API_VERSION` (`crates/ide-plugin/src/runtime.rs:36`) agree; the SDK's `wit/` copy is identical.
- Manifest validation is all or nothing: an IDE that doesn't know a menu id refuses the
  whole plugin (`crates/ide-host/src/plugins/ui_points.rs:110`).
