# Backlog for the remaining F and F* rows (2026-11-06)

Merged from the four per-area backlogs. Detail, row ids and designs are in each file:
[spring](backlog-spring.md) · [infra and data](backlog-infra-data.md) · [php](backlog-php.md) · [tooling](backlog-tooling.md).

88 work packages, about 210 open rows. Nothing here is verified in the IDE; every package carries that risk.

| Area | Packages | S | M | L | Wave 1 | Wave 2 | Wave 3 |
|---|---|---|---|---|---|---|---|
| Spring | 27 | 9 | 18 | 0 | 8 | 12 | 7 |
| Infra and data | 24 | 7 | 15 | 2 | 10 | 9 | 5 |
| PHP | 27 | 6 | 18 | 3 | 12 | 8 | 7 |
| Tooling | 10 | 2 | 5 | 2 (+1 S/M) | 4 | 3 | 3 |

## Before wave 1: verify in the IDE

Three behaviours gate several packages and are untested. Spike them first with `openidea-plugin link`:

1. File template placeholders (`$NAME$`, `$END$`) and the name prompt (Spring TPL, Symfony and Laravel templates).
2. Completion inside string literals and on `@` (Spring WEB-URL, Symfony and Laravel completions).
3. Activation of `json`, `xml` and `yaml` providers (Spring BOOT-META and WS-XML, Liquibase, OpenAPI).

## Wave 1: small, independent (34 packages)

One agent per plugin. Plugins don't share files, so these run in parallel.

- **Spring (5.6 days):** SP-FND first (shared helpers in `crates/spring-text`), then TPL, BOOT-VAL, BEANS-ANN-a, OSGI, GQL, WEB-VIEW, CLOUD-MISC. SP-FND blocks eight wave 2 packages.
- **Infra and data:** FW-1, LQ-3, LQ-1, OA-4, OA-3, DC-1, KE-4, KE-8, KE-2, JPA-2.
- **PHP:** SY1, SY13, TW3, BL1, LA9, LA5, SY2, SY3, SY5, TW1, LA1, LA2.
- **Tooling:** P1 (gitlab Open on GitLab), P8 (ideolog), P6 #1 (unused property keys), P9 #4 (ignore New menu).

## Wave 2: reuse the wave 1 foundations (32 packages)

Roughly one stream per plugin. Spring: 12 packages, the two XML packages in `spring-beans` stay apart. Infra: KE-3, KE-6, KE-7, LQ-2, OA-1, OA-2, DC-2, JPA-1, JPA-3. PHP: SY4, LA6 (Laravel indexer), SY6, SY7, SY8, TW2, LA3, LA4. Tooling: P2, P4, then P3 (quarkus, in that order).

## Wave 3: large, risky or low value (22 packages)

Spring BEANS-CMD (after both XML packages). Infra KE-1 (own Kubernetes schema diagnostics, L), KE-5 (log snapshot viewer, L), OA-5, JPA-4 (after JPA-3), JPA-5. PHP SY9 (L), SY10, SY11, SY12, LA7, LA8, LA10. Tooling P5 (quarkus key-to-Java indexer and Qute), P6 #11, P7, P9 #7, P10.

## Rules for running them

- **Same-file conflicts inside one plugin:** KE-1 with KE-2, LQ-1 with LQ-2, openapi OA-4/OA-3 then OA-1/OA-2, DC-1 with DC-2, JPA-2/3/5, SY8/SY9/SY10, SY2/SY9, LA3/LA4, quarkus P4 before P3. Run these one after the other.
- **Shared lines:** every package of a plugin edits its `change-notes`; merge those last. Don't touch `version` (release-please).
- **False positives:** packages that depend on library or vendor beans, classes or keys the plugin can't see (BEANS-XML-REF, BEANS-ANN-b, SEC-REFS, BOOT-BUILD, KE-1, KE-2, JPA-2/3, quarkus P3) ship as Info or behind a setting, quiet unless sure.
- **Licence:** rules for proprietary originals (Laravel Idea, Blade, Kubernetes) come from public documentation only.
- **Tests:** MockHost plus direct `index()` calls. Remember the mock/real divergences: `read_file` missing is `Invalid`, `find_files` skips dot-dirs.

## Cleanups found on the way

- `gitlab/README.md` still says "no pickers", "five quick filters" and "no commit list".
- `laravel/README.md` "Not ported" still says `make:*` needs an input dialog (LA9 fixes it).
- Liquibase `line_markers` only publishes problems (why LQ-3 is open).
- TW2 needs one owner for `unclosed-tag` so it isn't reported twice when `aritti.symfony` and `aritti.twig` are both installed.
- Dropped on purpose: Spring A1 #3, #12, #17, Boot #13, Runtime #8, and three PHP rows with no public rule basis (RouteAttributeNamespaceDeprecated, LifecycleMethodVisibility, discriminator-map duplicates).
