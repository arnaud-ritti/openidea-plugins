# Reclass of A1 (Spring core, OSGi) and the B / F* rows of A2 against plugin API 1.2

Basis (OpenIDEA v0.1.38; `/tmp/oi/wit`, `/tmp/oi/docs/plugins/indexing.md`):
- `host.pick/pick-many/input/confirm`: valid only inside a window command or event; `invalid` in editor providers (code-action resolve included). No capability needed.
- A command can edit text: `editor::text-editor.apply-edits(version, edits)` (needs `editor.write`) on the focused editor, i.e. an EXISTING open file. So "command + pick + insert at selection" works; file creation (U2) does not.
- `document-indexer`: per document returns definitions / references / aliases / markers with plugin-chosen string keys. Host answers Go to Declaration and gutter markers project-wide; "Several targets open a chooser" (indexing.md). References are Go to Declaration only: no find usages, no rename (U1 unchanged).
- Still unchanged: U1 rename/find-usages/hover, U2 file creation, U3 find_files/ignored dirs/files-changed, U4 type resolution, injection, inlays, structure, diagrams, run configurations, classpath/jar access.

Skepticism rule: a row flips only when the new API supplies the missing piece. Rows whose block was a false-positive risk (library beans, auto-config), a missing classpath, or an unverified IDE behaviour do not flip. `set_context`, `viewItem.context`, `reveal_view`, `selected_paths`, manifest icon and `workspaceContains:` improve F rows already built (views, menus) but unblock no B / F* row.

A1 is the full table (every B and F* row). A2 lists only rows that change; every other A2 B / F* row is unchanged.

## A1 Spring core -> spring-beans (+ spring-boot)

Old class in the second column. Rows not listed in A1 were F and stay F (not in scope).

| Row | Old | New | Why | Sketch | Size |
|---|---|---|---|---|---|
| 3 MissingAspectjAutoproxy | F* | F* | unchanged (obsolete, needs Spring version) | | |
| 4 JdkProxiedBeanType | B | B | unchanged: U4 | | |
| 8 XML refs/placeholders/resource/profile (5) | F* | F* | unchanged: library beans invisible, U3 | | |
| 9 XML class-based (6) | F* | F* | unchanged: FP on library parents | | |
| 10 XML typed (5) | B | B | unchanged: U4 | | |
| 11 UnparsedCustomBean | B | B | unchanged: needs JVM/classpath | | |
| 12 Autowiring no/several beans | F* | F* | unchanged: auto-config beans invisible | | |
| 16 Proxy self-invocation | F* | F* | unchanged: U4 | | |
| 17 TransactionalComponent | F* | F* | unchanged: auto-config | | |
| 18 ConfigProxyMethods (false side) | F* | F* | unchanged: text only | | |
| 20 DependsOn / ComponentScan | F* | F* | unchanged: library beans | | |
| 21 Lookup/Scheduled/Testing/initMethod (5) | F* | F* | unchanged: library targets | | |
| 22 Config file paths (4) | F* | F* | unchanged: U3 (ignored/generated paths) | | |
| 24 CacheableComponents | F* | F* | unchanged: no CacheManager knowledge | | |
| 28 spring.handlers/schemas | F* | F* | unchanged: library classes | | |
| 29 SpEL syntax/refs | B | B | unchanged: injection | | |
| 31 Kotlin checks (3) | F* | F* | unchanged | | |
| 35 HardCodedStringToPlaceholder | F* | F* | unchanged: `pick` is `invalid` inside a code action; a command-form variant would need the same file-picking guess | | |
| 36 ParseCustomBean | B | B | unchanged: JVM | | |
| 38 Generate Setter/Constructor Dependency, Autowire Bean, Event Listener (4) | B | **F*** | `pick`/`pick-many`/`input` (command only) + `apply-edits` on focused editor. Risk: caret is not movable, edit lands at selection start; needs `editor.write`; menus are flat (`editor.context`) | commands in `editor.context` (when `editorLangId == java`): `pick` bean from the existing index, build field/ctor/setter text with `spring_text::java` class bounds, `apply-edits`; event listener: `input` event type | M |
| 39 Create Spring Component/Config | F* | F* | unchanged: U2, only `fileTemplates` (`input` cannot create a file) | | |
| 40 Templates (15) | F* | F* | unchanged: placeholders unverified | | |
| 41 Live templates (10) | F* | F* | unchanged: no tab stops | | |
| 42 ChangeActiveProfiles | B | **F*** | `pick-many` over profile names collected from the index (`@Profile`, `application-*.yml` names), result in plugin storage, `set_context` optional. Narrow: feeds only our own checks, not run configs | command in `tools` menu: `pick-many`, store; row 8 profile/duplicate-name checks read it | S |
| 43 Config check / dump actions (2) | F* | F* | unchanged: depends on rows 5-9 | | |
| 44 Implicit / XML infrastructure beans | B | B | unchanged: classpath | | |
| 45 Bean diagram | B | B | unchanged: no diagram API (our SVG view exists) | | |
| 46 Annotation-string references (40+) | F* | F* | stays F*. Indexer is now the right vehicle (`references` anchors with `bean#`/`profile#`/`resource#` keys give project-wide Go to Declaration across Java/XML/properties) but the unresolved-library and `@Value` limits remain; partly done already | | |
| 47 XML model name->setter | F* | F* | unchanged: library parents | | |
| 48 Extra line markers (24): event publish/listen, class -> XML definition, `@ComponentScan`, `@Bean` overrides | F* | **F** | indexer `markers` + `definitions` with plugin keys; multi-target allowed ("Several targets open a chooser"); this was the "one target per marker" block | add to `spring-beans/src/indexer.rs`: publisher `publishEvent(new X` -> marker `event#X`, `@EventListener(X)`/param -> definition `event#X`; XML `class=` -> marker `type#...` (type keys already exist) | M |
| 49a Multi-target marker chooser | B | **F** | indexer: several definitions for a key open a chooser (indexing.md) | already true for bean/type keys; add aliases for inherited `type#` | S |
| 49b Advised-method markers (aop.provider) | B | B | unchanged: per-document index cannot expand a pointcut over other documents' methods; needs U4 signature matching | | |
| 50 SpEL language | B | B | unchanged: injection | | |
| 51 Structure view | B | B | unchanged | | |
| 52 Rename / find usages / gotoRelated | B | B | unchanged: U1. Go to Declaration part is already covered (row 46) | | |
| 53 implicitUsageProvider | B | B | unchanged | | |
| 54 Folding / typedHandler | B | B | unchanged | | |
| 55 Unresolved-ref quick fixes (create bean/class) | B | B | unchanged: U2 | | |
| 56 Spring facet / contexts | B | B | unchanged: no project model; `workspaceContains:` only gates menus | | |
| 57 Diagram provider | B | B | unchanged | | |

## Spring OSGi -> spring-osgi

| Row | Old | New | Why | Sketch | Size |
|---|---|---|---|---|---|
| 2 unresolved refs as errors | F* | F* | unchanged: library beans | | |
| 3 comparator-ref type | B | B | unchanged: U4 | | |
| 4 ServiceCommon beyond direct implements | B | B | unchanged: U4 | | |
| 5 Listener method vs service class | B | B | unchanged: U4 | | |
| 6 ref completion filtered by interface | F* | F* | unchanged | | |
| 8 ReferenceEffectiveTypeProvider | B | B | unchanged: U4 | | |
| 9 Compendium DOM | F* | F* | unchanged, low value | | |
| 10 Generate OSGi service/reference bean | B | **F*** | same mechanism as A1 row 38: `pick` + `input` + `apply-edits` into the open XML | command in `editor.context` for xml: `pick` service/reference, `input` interface, insert `<osgi:service>` at selection | S |

## A2 rows that change (all other A2 B / F* rows: unchanged, same Ux or limit as audited)

| Row | Old | New | Why | Sketch | Size |
|---|---|---|---|---|---|
| Boot 11 Add Starters | B | **F*** | U5 gone: `pick-many` for dependencies; edit of the open `pom.xml`/`build.gradle` via `apply-edits`. Risk: catalogue needs `net:start.spring.io` (fixed host, only the default server), dependency block placement by text | command in `spring-initializr` (already has the net host): fetch `/metadata/client`, `pick-many`, insert `<dependency>` text into focused build file | M |
| Initializr 2 dependency/version pick lists | F* | **F** | `pick`/`pick-many` replace the missing multi-select UI; default `start.spring.io` is already allowed. Custom server URLs still cannot be fetched (fixed `net:`) | `pick` boot version, `pick-many` deps, `input` group/artifact, pass to the existing `spring init` run | S |
| MVC 8 Open endpoint in HTTP client | B | **F*** (reduced) | `viewItem.context` on the Endpoints tree item + `copy-to-clipboard`/`open_terminal`. Not the IDE HTTP client file (U2/no API), so a substitute: copy URL / copy as curl / run curl in a terminal | context-menu command on endpoint items: `input` host:port, build curl, clipboard | S |
| Boot 10, 12, 14, 18-22; Data 8, 11, 15-17; MVC 12, 15, 16; Debugger 1-7; Initializr 1; Batch 3; Cloud 5 | B | B | unchanged: database/run-config APIs, U1, U2 (Initializr wizard, repository generator, Data 16), U4, injection, inlays, debugger | | |

## Summary

Flipped: 8 rows. B -> F: 1 (A1 49a). F* -> F: 2 (A1 48, Initializr 2). B -> F*: 5 (A1 38, 42; OSGi 10; Boot 11; MVC 8).
- A1 (the 49 row is split into 49a/49b): B 26 -> 23, F* 68 -> 66 after the 2 B->F* and 1 F*->F moves (counts are per original feature in A1's own summary, so approximate).
- The other rows stay: their blocker is U1/U2/U3/U4, a missing classpath, injection or a false-positive risk, not the dialog or indexing gap.
- 1.2 mostly helps built F rows (menus, context, views), not the blocked ones.

Five best-value rows to implement:
1. A1 38 Generate Setter / Constructor Dependency / Autowire / Event Listener (and OSGi 10): command + `pick` + `apply-edits`; four user-visible actions, M.
2. A1 48 + 49a Event publish/listen, class <-> XML markers with chooser: extend the existing `spring-beans` indexer, M.
3. Initializr 2 + Boot 11 dependency picker (and Add Starters into an open build file): S/M.
4. A1 42 Active profiles command: S, makes the profile-scoped duplicate-name check (A1 row 6) and any future profile checks accurate.
5. MVC 8 copy endpoint as curl from the Endpoints tree context menu: S, tiny and visible.
