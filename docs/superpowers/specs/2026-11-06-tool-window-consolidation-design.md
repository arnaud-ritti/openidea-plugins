# Tool window consolidation: GitLab and Spring

## Why

The tool window bars are overloaded. The GitLab plugin alone adds 10 windows, all with the same fox icon, and the 15 Spring windows share one leaf. A user can't tell what a click will open or where it will land (left, right or bottom). The goal is that every icon says what it does, and that there are far fewer of them.

Source: the user, after looking at the real IDE (OpenIDEA 0.1.38) with `ma-station-essence` and `baby-bob` open. They chose option B (fewer windows) of the two shown mockups, and the Spring grouping below.

## Goals

1. **GitLab: 10 windows become 3.**
   - `GitLab Projects` (left): the projects list, unchanged.
   - `Merge Requests` (left): the MR list, unchanged.
   - `Merge Request` (right): everything about the selected MR, in one window with five sections chosen by buttons at the top: Overview, Changes, Commits, Files, Pipelines. Each button carries a badge (number of files, commits, pipeline state). A click replaces the content in place.
   - **Changes moves to the right**, into this window (decided by the user). The wide bottom window goes away.
2. **GitLab forms stop being windows.** `Create Merge Request`, `Create Snippet` and `Share Project on GitLab` become commands that ask in a sequence of prompts (`input`, `pick`, `confirm`), then run and show the result.
3. **Spring: 15 windows become 5.** Today's 15: Endpoints, WebSocket destinations, Messaging, GraphQL endpoints, Web Flow, Feign clients; Integration channels, Batch jobs, Modules, Module diagram; Beans, Bean graph, Configuration report; Runtime; Initializr. The three Beans windows and the two Modulith windows also collapse into sections, so `spring-beans` and the Structure window need the same in-window section switching as GitLab.

   | Window | Plugin that owns it | Sections |
   |---|---|---|
   | Spring Endpoints | `aritti.spring-web` (absorbs websocket, messaging, graphql, webflow, cloud) | HTTP endpoints, WebSocket destinations, Messaging, GraphQL endpoints, Web Flow, Feign clients |
   | Spring Structure | `aritti.spring-integration` (absorbs batch, modulith) | Integration channels, Batch jobs, Modules, Module diagram |
   | Spring Beans | `aritti.spring-beans` (alone) | Beans, Bean graph, Configuration report |
   | Spring Runtime | `aritti.spring-runtime` (alone) | unchanged |
   | Spring Initializr | `aritti.spring-initializr` (alone) | unchanged |

4. **Every remaining window icon is distinct**: the family logo plus a small glyph for its function (list, arrows, info, diff, commit, files, pipeline for GitLab; globe, structure, beans for Spring). The two surviving GitLab list windows also differ from each other.

## Non-goals

- Other plugins' windows (kube-explorer, jpa-assist, flyway, liquibase, openapi, devcontainer, ideolog, symfony, laravel, ignore, tasks, dependency-audit) are out of this round.
- `spring-boot`, `spring-data`, `spring-security`, `spring-osgi`, `spring-shell`, `spring-ws` have no windows and are not merged.
- One plugin for all of Spring is not wanted.
- No upstream change to OpenIDEA is required. View groups and view visibility conditions (`when`) would help but aren't needed here; they stay a possible separate request to `l0ng-ai/openidea`.

## Anti-goals

- Losing a feature: every action reachable today stays reachable (menus, status bar item, commands, palette).
- Making completion or checks slower. Providers have a 50 ms budget; a merged plugin must not blow it.
- Silently dropping users' settings. The merged-away plugin ids disappear; the README and change-notes say so.

## Constraints (verified in the host source, v0.1.38)

- A view is one tool window with its own id, `plugin.<plugin id>.<view id>`, drawn by its plugin. A view can't live in another plugin's window. It has only `id`, `title`, `icon`, `location`, `web`: no group, no condition. Plugins run isolated and can't call each other.
- A view body is replaced on each render, so section buttons are plain toolbar buttons that change plugin state and refresh the view.
- Prompts (`pick`, `input`, `confirm`) work only inside a command or event of a window; they return `Invalid` elsewhere. `input` is one line.
- A plugin instance serves its calls one at a time; a long command delays providers.
- An icon is an SVG of at most 16 KiB; colours are kept; `#6C707E` alone is tinted by the theme. Symfony's black logo is already a tinted glyph for that reason.
- The repository keeps one Cargo project per plugin, no root workspace.

## Decisions taken with the user

- Option B (fewer windows), Changes on the right, the five-window Spring grouping, and merging only the groups above.

## Decided by the user on the builder's question

- **Old plugin ids:** the published versions of the seven absorbed plugins (`aritti.spring-websocket`, `-messaging`, `-graphql`, `-webflow`, `-cloud`, `-batch`, `-modulith`) stay where they are (GitHub releases, tags, registry entries). They are no longer maintained: no tombstone release, no registry withdrawal, no further versions. Consequence to document, not to engineer around: a user who keeps an old plugin installed next to the new `aritti.spring-web` or `aritti.spring-integration` gets duplicate windows and duplicate completions until they uninstall it. The README and change-notes of the two survivors say to uninstall the old ids.

## My calls (marked as mine, to check)

- **Multi-line text:** a prompt is one line. Create Merge Request asks for title and a one-line description, then offers "Open on GitLab" for longer text. A dedicated multi-line field would need a form window again.
- **Which plugin id survives a merge:** `spring-web` and `spring-integration` keep their ids (fewest migrations). The seven absorbed plugins are removed from `plugins/`, `release-please-config.json`, `.release-please-manifest.json`, the CI matrix and the docs.
- **Merging mechanism:** each absorbed module stays a library crate (`rlib`) and the surviving plugin depends on it by path, with a cargo feature that turns off the module's own `register_plugin!`. A small dispatch layer calls the modules for each provider, command and event, and merges their results. Modules that don't apply to the project (no `spring-batch` dependency, say) are skipped, and the document is parsed once and shared, to protect the 50 ms budget.
- **Name collisions:** the view id `diagram` (batch, integration, modulith), command ids and storage keys are prefixed by module.
- **Capabilities:** the surviving plugin asks for the union of its modules' capabilities. As far as possible they stay in `optional`.
- **Order of work:** GitLab first (self-contained), then Spring Structure (3 small modules), then Spring Endpoints (6), then the Spring Beans sections, then the icons for all.

- **Section switcher:** the same pattern everywhere: toolbar buttons that set the active section in plugin state and refresh the view. State lives in memory with a copy in plugin storage under one key per window, global (not per project). The Spring helper goes in `crates/spring-text`; GitLab keeps its own copy because its plugin doesn't depend on that crate.
- **Settings and stored data:** the surviving plugins keep their own settings and storage keys unchanged. Only the absorbed modules' keys are prefixed, and their old values are not migrated: the settings of the seven removed plugins reset, and the README and change-notes say so. Release-please drives the version; the merge commits use `feat!:` so the survivors take a minor bump (pre-1.0).
- **The 50 ms budget:** a measured gate comes first. Spring Structure (the smallest merge) is built and run on `ma-station-essence`, and the log is read for slow calls and CPU-budget crashes, before Spring Endpoints starts. Each merge also gets a native test that times the merged providers on a representative file against a loose threshold (native runs several times faster than WASM, so the threshold is a tripwire, not a proof). If a merge can't hold the budget, the fallback is a different split.
- **Create Merge Request failures:** a dismissed prompt anywhere cancels with no effect. If the GitLab call fails after all prompts, the plugin shows the error and offers "Open on GitLab"; it doesn't keep the answers.
- **References to old window ids:** a repository-wide grep of manifests, commands, status item actions, menus and docs is enough; the plan lists the hits before editing.

## Left to the builder

- Which widget switches sections (toolbar buttons with state, or a tree root), badge wording, exact glyph shapes, prompt wording and order, how the diff and its comments render inside the Changes section (it is an HTML view today, and scripts in HTML bodies are blocked in the desktop app, so it must work script-free as now).
- How the dispatch layer is shaped inside the surviving plugin.
- Fixing the Spring GraphQL view icon, which currently points at the 64 px package icon instead of a 16 px view icon.

## How we'll know it works

- In OpenIDEA 0.1.38 with `ma-station-essence` (Spring Boot, Liquibase): the stripe shows 5 Spring windows with distinct icons; Endpoints still shows 18 endpoints; no plugin crashes or exceeds its CPU budget (check the log).
- In a GitLab project: the stripe shows 3 GitLab windows; Changes opens on the right; each of the three forms works as a prompt sequence and a dismissed prompt does nothing.
- `cargo test`, `openidea-plugin build` and `check` stay green for every remaining plugin; new tests cover the dispatch layer and the section switching.
