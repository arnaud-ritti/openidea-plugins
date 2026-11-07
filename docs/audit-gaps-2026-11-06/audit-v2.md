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

## Round 2 (implementation of the open F/F* rows)

Second implementation round on branch `arnaud-ritti/implement-new-feature` (commits `3907ace` to `a7eed2c`, one per plugin) and the fixes of the findings above. Status comes from the agent reports and from the commit `change-notes` and READMEs; "not verified" means the change-notes did not let me confirm the detail. Nothing was run in the IDE; the mock host proves logic only. Row ids are those of the `audit-v2-*.md` files. B rows were not touched unless listed.

| Area | DONE | PARTIAL | SKIPPED / still B |
|---|---|---|---|
| A1 spring-beans | three beans-XML file templates, live templates as snippet completions, some XML and class checks | the round stopped midway; the remaining rows of audit-v2-A1 are not done and the change-notes were not updated | spring-osgi untouched |
| A2 spring-boot | Boot 13, 15, 17, 1, 3, 7 | Boot 16 | |
| A2 spring-data | Data 13, 9, 10 | | |
| A2 spring-cloud | Cloud 2 | | Cloud 6; the other Cloud rows of the web agent |
| A2 spring-web, security, messaging, batch | Web 6, 10, 1 and the extra row 8; Messaging 3; Batch 2 | Web 14; Security 3; Messaging 1; Batch 1 | |
| A2 graphql, runtime, initializr | | | GraphQL 2, Runtime 8, Initializr 1 |
| B kube-explorer | K-A5.3, K-A4.3, K-A4.4, K-A4.5, K-I1a, K-I1b, K-I1c, K-I2, K-I3 (Reload Manifest from Cluster, Log Viewer leftovers, hints, project-wide markers) | K-I4 | |
| B flyway | FW2 (New Migration through `process:tee`) | | FW4, 5, 7, 10 (B) |
| B liquibase | New Changeset, New Changelog | | LQ3-6, 7, 11-15, 17 (B) |
| B devcontainer | DC9 | | DC1, 3, 6, 8, 12, 13 (B) |
| B openapi | OA19, 20, 3, 9, 4, 16 | | OA10 |
| B jpa-assist | JPA8, 9, 15, 30; New Entity writes the file | | |
| C symfony | rows 10 (12 of 14), 22, 24, 20, 15, 17, 18 (6 of 8), 3, 8, 11, I6, I10, I11; macros; Make, Create Twig Template, Insert Form Rows from a Form Type | rows 10 and 18 (sub-checks counted as reported) | ClearParameter, JoinWithToOn, AutowireControllerAsService, `@Template`, Vite, run config |
| C laravel | LA26, 27, 2, 11, 31, 33, 17, 19, 20, 5, 39 | | LA14, 21, 24, 25, 29, 30 (B) |
| C twig | macro completion and navigation, project extensions, unbalanced-bracket check | | vendor extensions |
| C blade | | | unchanged (B) |
| D gitlab | #3, #11, #4 | | #9, #12 |
| D dependency-audit | DA5 (12 formats, quick fix in `libs.versions.toml` and `requirements.txt`) | | DA8 |
| D tasks, dotenv | | | no change (B or discontinued) |
| D ignore | #3, #4 | | other rows B |
| D ideolog | #1, #10 | | |
| D quarkus | #8 | #6 | other rows B |
| D properties | #7 | | |

Fixes from "Problems found in our own work", checked against the diff: `add_selected` in ignore now uses notifications instead of a returned string; gitlab README and change-notes mention local includes; dependency-audit change-notes list 12 formats; kube-explorer change-notes describe Reload as a command and the project-wide markers with `project.read`; the openapi `specGlobs` claim is corrected (problems and endpoints view, not previews). Not verified: the JPA-3 `@NamedEntityGraph` claim (the new entity graph checks are in the jpa-assist change-notes), the spring-osgi, spring-web (static resources) and spring-boot README corrections, and the Symfony README Twig extension names.

Still true after the round: no references, rename or hover providers, no type resolution, quick fixes cannot create files (commands can, through optional `process:tee`), all B rows. Details per plugin: the "Added by the audit v2 pass" blocks in [LIMITATIONS.md](../../LIMITATIONS.md) and the section in [PARITY.md](../../PARITY.md).

### Round 2b: spring-beans and spring-osgi (A1)

The A1 table above predates the last spring-beans commits, so its rows read Partial or Missing where the code is done. Verified in code and tests: rows 8 (`xml-import-not-beans`, `unresolved-class`, `xml-filter-expression`), 9 (`xml-factory-method-visibility`, `xml-lookup-method`), 21 (`@MockitoBean` names), 22 (relative resource paths, default context file), 24 (`cache-config-unused`, `cacheManager`/`cacheResolver` conflict), 40 and 41 (three beans-XML templates, live templates), 46 and 47 (scope, profile and `constructor-arg name` completion), 48 (`@ComponentScan` and `@Bean` override markers, Kotlin publishers and listeners), and the OSGi `comparator-ref`/`depends-on` Info are DONE. Row 35 is DONE now: `externalize-annotation-string` appends `key=value` to the chosen properties file through `process:tee` (path inside the project root, clipboard fallback). Nothing in A1 remains F or F* except rows that depend on blocked APIs.
