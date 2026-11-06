# Gap audit v2 (2026-11-06, after the 1.2 work and the backlog pass)

Re-audit of every row of the five detail files against the code at `ac0edf6`, by grep and reading `src/` and `plugin.toml` (not READMEs). Nothing was run in the IDE; "Done" means the code path exists and is wired. Detail, evidence and next rows per area: [A1](audit-v2-A1-spring-core.md) · [A2](audit-v2-A2-spring-rest.md) · [B](audit-v2-B-infra-data.md) · [C](audit-v2-C-php.md) · [D](audit-v2-D-tooling.md). The originals were not downloaded again; the inventories in the old detail files are the baseline, so features missing from those inventories stay unknown.

Units differ per area (rows, weighted rows, features), so the totals are comparable within a line, not across lines.

| Area | Units | Done | Partial | Missing | Open F | Open F* | Open B |
|---|---|---|---|---|---|---|---|
| A1 Spring core + OSGi | 66 rows | 25 | 22 | 19 | 2 | 19 | 20 |
| A2 Spring rest | 90 rows | 37 | 28 | 25 | 0 | 22 | 31 |
| B infra and data | 289 weighted | 150 | 42 | 97 | 12 | 26 | 101 |
| C PHP | 144 rows | 59 | 32 | 51 (+2 n/a) | 5 | 32 | 46 |
| D tooling | 88 gap rows | 22 | 28 | 38 | 1 | 12 | 53 |

Before the work the same files had, for example, A2: 0 Done, 18 Partial, 72 Missing; B: 62 Done, 24 Partial, 204 Missing; D: 9 F, 26 F*, 53 B open. Every old row called Done was re-verified: none regressed. B rows moved only where a new capability applies (below).

## What the re-audit found about the API

- **Plugins can write files.** An optional `process:tee` capability lets a command write a file anywhere the user approves (`host::run_process("tee", [path], Some(content))`; kube-explorer does it in `open_temp_file`, `lib.rs:427`). This is not a code-action edit, so quick fixes still can't create files, but commands can: generator rows that only need file output (Flyway empty migration, OpenAPI HTTP requests, log download, make-style generators) move from B to F*. `api-facts.md` and the old memory note ("no API creates a file") were wrong on this.
- **`editor.write` has no 50 ms budget in commands.** A command can edit the open file (jpa-assist does); `api-facts.md` still says it cannot. "Reload from cluster" in kube-explorer moves from F* to F.
- Still blocked: references/rename/hover (U1), type resolution (U4), `pick`/`input` inside code-action resolve, decorations, injection, formatters other than prettier/ruff, Qute as a language (would replace built-in HTML).

## Problems found in our own work

1. **Icons:** 24 plugins shipped `icon.svg` without the `icon =` manifest line (fixed in `770a781`). Only blade declared it before.
2. **Stale docs that understate the code:** spring-osgi README (Generate provider, `bundle` scope), spring-web README (static resources), spring-boot README/change-notes (four checks), gitlab README line 223 (local includes), dependency-audit change-notes (7 vs 12 formats), Symfony README (Twig extension names), `LIMITATIONS.md` lines 398-399 (overloads, Kotlin checks), kube-explorer README ("Reload from cluster does not fit 50 ms" is wrong for a command; markers "across all open YAML files" is wrong).
3. **Claims the code doesn't back:**
   - JPA-3 is marked DONE but only `@NamedAttributeNode` names are checked, not `@NamedEntityGraph`/`@EntityGraph`.
   - openapi `specGlobs` feeds completion and go-to-definition only, not previews, problems or the endpoints view.
   - SY2 covers little of its scope: 12 of 13 named constructor/controller checks have no code, and `http-kernel-event-argument` is a different check from the original of that name.
   - SY5: only the Criteria ordering check of 9.
   - The reclass predicted `@ComponentScan` and `@Bean` override markers; they were skipped, with no API limit behind it.
4. **A ruling that was wrong:** `RouteAttributeNamespaceDeprecated` does have a public basis (Symfony 6.4 UPGRADE notes).
5. **Likely dead behaviour:** ignore `add_selected` tells the user to "run the command again" through a returned string; the frontend drops returned strings, so the user sees nothing (use `host::notify`).
6. **Weak "Done" rows:** security self-invocation is Java-only and off by default; `unresolved-view` is an opt-in Hint; quarkus required keys and properties live unused keys are opt-in; Show Inherited Members is limited to `editorLangId == java`; feature-id completion (devcontainer) and Helm repo alias fixes are empty until a refresh command runs (READMEs say so, change-notes don't).
7. **Unused 1.2 features:** symfony, twig, laravel and blade don't use `viewItem.context` menus, `set_context`, `selected_paths` or `reveal_view`.

## Next work, by value

1. Fix the findings above (icons are done): the stale docs, `add_selected` notify, the JPA-3 claim, the specGlobs claim. All S.
2. Use `process:tee` for the generator rows that moved to F* (Flyway empty migration, OpenAPI HTTP requests, kube log download, Spring and Laravel generators that currently use the clipboard).
3. The small F rows: Reload from cluster, log viewer leftovers (Clear, Scroll to end), JPA entity graph names, OpenAPI live-template names and root skeleton, devcontainer recent containers, Spring live templates as snippet completions and the three beans-XML templates.
4. The ten best rows listed in each `audit-v2-*.md`.
5. Verify in the IDE ([checklist](../ide-verification-checklist.md)), then the upstream requests for the B rows.
