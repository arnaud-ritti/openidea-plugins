# What to verify in the real IDE

Everything built so far was tested with the SDK's `MockHost` only. Load each plugin with `openidea-plugin link` (build, then Settings › Plugins › Reload) and check the items below. Tick or file a note in `LIMITATIONS.md`.

Known mock/host differences: `read_file` of a missing file is `Error::Invalid` in the host (`NotFound` in the mock); `find_files` skips dot-directories and gitignored paths in the host only; `pick`/`input`/`confirm` return `Invalid` outside a command or event of a window (providers, code-action resolve); editor providers have a 50 ms budget.

## Cross-cutting (gate many packages, check first)

1. File template placeholders `$NAME$`, `$END$` and `$PACKAGE$` (empty package drops its line), and the name prompt: spring-beans, spring-boot, spring-data, spring-web, jpa-assist, flyway, symfony, laravel.
2. Snippet tab stops (`$1`, `${1:x}`, `$0`) in completion items: twig, kube-explorer, laravel, openapi, symfony, spring-web.
3. Completion inside string literals, on `@` and inside tag names (`<osgix:`, `<x-comp `).
4. Activation of the `json`, `xml` and `yaml` providers and `on-language:*` events (spring-boot metadata, spring-web (websocket, messaging), jpa-assist persistence.xml, liquibase, openapi, quarkus on java).
5. Document indexers: Go to Declaration, gutter icons, the chooser for several targets, a marker whose key has no definition (spring-beans, symfony, laravel, quarkus, kube-explorer, liquibase, flyway); alias direction in spring-beans; hand-combined `register_plugin!` exports load (ideolog, twig, symfony, laravel, kube-explorer, spring-beans, liquibase, flyway).
6. Code actions whose edits touch a second file (symfony SY6/9/10, laravel LA4, jpa-assist JPA-1/5, spring-boot metadata fix), several edits at the same offset, and whether `read_file` returns unsaved buffers.
7. `pick`/`input` from a view button (gitlab edit attributes and filters, kube-explorer log viewer, ideolog patterns) and from Form buttons.
8. Icons: plugin `icon.svg` in Settings › Plugins (light and dark theme), coloured view icons at 16 px.

## Per plugin

- **symfony:** Twig `include ... with` reading within the 3 s budget; attribute-argument completion (manual invoke only); `#[Argument]`/`#[Option]`/`#[AsTwigFilter]` output; YAML/XML insertion points; `unclosed-tag` now comes from twig only.
- **twig:** snippet rendering, 50 ms budget, two plugins publishing diagnostics on one path.
- **laravel:** `Find Unused Views` pick then open; attribute completion inside tags; new problems under 50 ms; `selection` shape in the new code actions; `artisan` terminal runs.
- **spring-beans:** the Report section of the Spring Beans window; file events on `META-INF/spring.*`; absolute paths from `find_files`.
- **spring-boot / cloud / data:** `json` line-markers provider; `pick` in `show-repository-members`; marker without target.
- **spring-web / security / graphql / websocket:** `**/static/…` globs in `find_files`; gutter marker without target; XML provider activation.
- **spring-osgi / messaging / batch:** tables are from memory (XSD not checked; least sure: `id` on `<split>`, `decider` on `<decision>`); completion in names and Java strings.
- **jpa-assist:** build-file quick fix; `begin_task` inside file-opened; `xml` registration.
- **kube-explorer:** snippet syntax; log viewer rendering (`<mark>`, `color-mix`); markers with no target; same name in two namespaces; `secret-edit:<key>` form events; Telepresence terminal.
- **liquibase / flyway / devcontainer:** `process:git` in a completion provider (50 ms); containers.dev index download; `devcontainer.local_folder` label; `jsonc` language id.
- **openapi:** `git show HEAD:./<path>`; Redoc 2.5.4 in the frame; `fileIcons` globs; gutter marker click.
- **gitlab:** CI keyword and `CI_*` lists (typed from memory of the docs, check them).
- **quarkus:** indexer on Java; budget on large Java files; `on-language:java`.
- **properties / ideolog / ignore:** live unused keys on a large project; combined WASM export in ideolog; completion in ignore files (host hides dot and ignored paths).

## Left out on purpose

TW3 formatter (host accepts only prettier or ruff), DC-2 gutter markers that start a container, P7 writing other locale files, P9 #4 New menu per ignore type, P10 tasks trackers (discontinued), P5 #12 Qute, KE-7 reload from cluster (50 ms budget), CLOUD-MISC key hints (spring-boot covers them). About 210 audit rows stay blocked by missing OpenIDEA APIs (references/rename/hover, file creation, type resolution, `find_files` including ignored paths, `pick` in code-action resolve).

## Window consolidation and route list

None of this was run in OpenIDEA; every item comes from the plans [GitLab](superpowers/plans/2026-11-06-gitlab-windows.md), [Spring](superpowers/plans/2026-11-06-spring-windows.md) and [route lists](superpowers/plans/2026-11-07-route-lists.md). Link each plugin, grant it, reload.

- **gitlab (Task 6, step 3; needs a token under Settings › Plugins › GitLab, or run only the checks without network):**
  - the stripe shows 3 GitLab icons (`projects.svg`, `merge-requests.svg`, `merge-request.svg`), all distinct and readable at 16 px;
  - selecting a merge request fills the right window; Overview, Changes, Commits, Files and Pipelines switch and keep the active-section dot; a failing section keeps the buttons;
  - Create Merge Request, Create Snippet and Share Project run their prompts, and a dismissed prompt creates nothing; the Open on GitLab offer after an API failure opens the new-MR page;
  - `host::refresh_view` on the Merge Request window redraws whichever section is active; `on-view:` activation still fires without the removed view ids.
- **spring (Task 3 step 5 and Task 4 step 5, 50 ms budget gate, then Task 6 step 3), on `ma-station-essence`:**
  - link `aritti.spring-integration`, then `aritti.spring-web`, then open the project and a non-Spring project; in `~/Library/Logs/ai.l0ng.openidea/openidea.<date>.log` find no `exceeded its CPU budget` and no slow calls; if one appears, revisit the split;
  - the Endpoints section still lists 18 endpoints in 6 controllers; `baby-bob` (Laravel) costs nothing and shows no Spring window errors;
  - the stripe shows 5 Spring windows (Endpoints, Structure, Beans, Runtime, Initializr) with distinct icons, including the new `endpoints.svg`, `structure.svg` and `beans.svg`;
  - Beans, Graph and Report switch; Check and Dump Spring Configuration open the Report section; the Endpoints and Structure sections switch and remember the choice;
  - no plugin warnings in the log, and with the seven retired ids (`aritti.spring-websocket`, `-messaging`, `-graphql`, `-webflow`, `-cloud`, `-batch`, `-modulith`) still installed the user sees duplicates until they uninstall them.
- **route list (Task 6, step 3):**
  - `ma-station-essence` shows 18 flat rows with method icons; the controller filter narrows to one controller and the status line says so; Clear restores; Flat/Tree toggles;
  - Copy as curl and Create HTTP Request File work (grant `process:tee` first; check that `http/` is created when missing, since `tee` may fail without it); a GET copies `curl -X GET '...'`;
  - `baby-bob`: the Laravel Routes section lists the routes, the filter works, the row context menu actions (`viewItem.context`) appear on list rows;
  - a Symfony project: the same, including an invokable controller and an ANY route (copied as GET);
  - an OpenAPI spec: rows group by first tag (also with flow-style `tags: [Pets]`), and the HTTP file is written beside the spec;
  - the six method icons render at 16 px in the list (registered as `fileIcons` patterns in `spring-web` and `laravel`).
