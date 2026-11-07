# Implementation plan for the rows that API 1.2 unblocks (2026-11-06)

Source: the four `reclass-*.md` files in this directory. 40 rows changed class against OpenIDEA v0.1.38 (API 1.2): A1/A2 8, infra/data 12, PHP 13, tooling 7. Nothing here is verified in the IDE.

## Facts the plan relies on

- `host::pick`, `pick_many`, `input` and `confirm` work only inside a command or event of a window. They return `Invalid` in editor providers and in `resolve_code_action`, so a quick fix can't ask. Every flow that asks is a command.
- A command can edit only the focused editor (`apply_edits`, needs `editor.write`). Plugins still can't create files.
- Document indexers give Go to Declaration and gutter markers, with a chooser when a key has several definitions. They give no find usages, rename or hover.
- Unverified: whether a `Form` button click counts as "an event of a window" for `pick`/`input` (gitlab rows depend on it).

## Open decision

Laravel generators (`make:*`) and create-missing-target could run `php artisan make:*` through `process:php` and `open_terminal`. PHP then creates the file, so no file-creation API is needed. This contradicts the earlier rule "generators go to the clipboard", so those rows stay F* until this is settled.

## Phase 1: small, no new infrastructure (all S)

| Plugin | Row | Work |
|---|---|---|
| kube-explorer | K-A6.2 Create Chart, K-A6.3 Helm parameters | `input` plus `helm create` / parameter prompts |
| kube-explorer | K-A8.2 Attach custom image, K-A8.3 Edit recents | `input` replaces the `debugImage` setting; recents list |
| kube-explorer | K-A2.3 Customize namespaces, K-A4.2 Select log sources | `pick_many` over `kubectl get ns` / pods |
| tasks | #4 issue search, #8 named contexts | `input` query, tracker call, `pick`; contexts in storage |
| symfony | other 26 run console command, other 7 command names | `pick` over indexed commands, then `open_terminal` |
| laravel | Idea 23 RunArtisanCommand | `input`/`pick` plus `open_terminal` |
| spring-beans | A1 row 42 active profiles | `pick_many`, feeds our own profile checks |
| spring-web | MVC row 8 copy as curl | tree context menu on endpoints |
| spring-initializr | row 2 dependency and version lists | `pick_many` against start.spring.io |
| ignore | #3 | `pick` of the target ignore file |

## Phase 2: extend the existing indexers (M)

- **symfony** `indexer.rs`: add `command#`, `param#`, Twig extension and UX component keys (other 7, 2, 10, 14), then move line markers onto the indexer (other 15) for the multi-target chooser.
- **spring-beans** `indexer.rs`: event publish/listen and class-to-XML markers (A1 row 48), multi-target marker chooser (49a).
- **liquibase** (LQ7): first indexer there, `changeset#` keys for include and rollback references. Keep a shared key helper for the next two.
- **flyway** (FW8, FW11) and **kube-explorer** (K-X3): reuse the helper from LQ7. Go to Declaration and markers only.

## Phase 3: generators as commands (M)

- spring-beans: Generate Setter, Constructor Dependency, Autowire, Event Listener (A1 row 38) and OSGi row 10, through `pick` plus `apply_edits` on the focused file.
- spring-boot: Add Starters (Boot row 11).
- symfony: TwigExtendsGenerator (action 8), go-to commands (other 17).
- gitlab: MR attribute editing (#14) and typed list filters (#15). Check the Form-event question first.
- tasks: several repositories per tracker (#5).
- openapi OA6 compare two specs, devcontainer DC7 clone sources.

## Phase 4: blocked on the open decision

Laravel Idea 5, 19, 20, 22 (model-aware generators, NewEloquentModel, create missing target) via `php artisan make:*`.

## Each task

1. MockHost test for success and every dismissal (`answer_next_prompt`); direct `index()` tests for indexers.
2. `cargo test` and `openidea-plugin build` in the plugin dir.
3. Change-notes in `plugin.toml` (version is release-please's), `engines.openidea = "^1.2"` already set.
4. Update the plugin's `LIMITATIONS.md` section and flip the row's class in the matching `reclass-*.md`.

## Upstream asks (would unlock the remaining F* rows)

- Allow `pick`/`input` in `resolve_code_action`.
- Let a command edit a second existing file.
- File creation in code-action edits (U2), `find_files` including ignored paths (U3), references/rename/hover (U1), type resolution (U4).
