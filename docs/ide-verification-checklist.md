# What to verify in the real IDE

Everything built so far was tested with the SDK's `MockHost` only. Load each plugin with `openidea-plugin link` (build, then Settings › Plugins › Reload) and check the items below. Tick or file a note in `LIMITATIONS.md`.

Known mock/host differences: `read_file` of a missing file is `Error::Invalid` in the host (`NotFound` in the mock); `find_files` skips dot-directories and gitignored paths in the host only; `pick`/`input`/`confirm` return `Invalid` outside a command or event of a window (providers, code-action resolve); editor providers have a 50 ms budget.

## Cross-cutting (gate many packages, check first)

1. File template placeholders `$NAME$`, `$END$` and `$PACKAGE$` (empty package drops its line), and the name prompt: spring-beans, spring-boot, spring-data, spring-web, jpa-assist, flyway, symfony, laravel.
2. Snippet tab stops (`$1`, `${1:x}`, `$0`) in completion items: twig, kube-explorer, laravel, openapi, symfony, spring-web.
3. Completion inside string literals, on `@` and inside tag names (`<osgix:`, `<x-comp `).
4. Activation of the `json`, `xml` and `yaml` providers and `on-language:*` events (spring-boot metadata, spring-websocket, spring-messaging, jpa-assist persistence.xml, liquibase, openapi, quarkus on java).
5. Document indexers: Go to Declaration, gutter icons, the chooser for several targets, a marker whose key has no definition (spring-beans, symfony, laravel, quarkus, kube-explorer, liquibase, flyway); alias direction in spring-beans; hand-combined `register_plugin!` exports load (ideolog, twig, symfony, laravel, kube-explorer, spring-beans, liquibase, flyway).
6. Code actions whose edits touch a second file (symfony SY6/9/10, laravel LA4, jpa-assist JPA-1/5, spring-boot metadata fix), several edits at the same offset, and whether `read_file` returns unsaved buffers.
7. `pick`/`input` from a view button (gitlab edit attributes and filters, kube-explorer log viewer, ideolog patterns) and from Form buttons.
8. Icons: plugin `icon.svg` in Settings › Plugins (light and dark theme), coloured view icons at 16 px.

## Per plugin

- **symfony:** Twig `include ... with` reading within the 3 s budget; attribute-argument completion (manual invoke only); `#[Argument]`/`#[Option]`/`#[AsTwigFilter]` output; YAML/XML insertion points; `unclosed-tag` now comes from twig only.
- **twig:** snippet rendering, 50 ms budget, two plugins publishing diagnostics on one path.
- **laravel:** `Find Unused Views` pick then open; attribute completion inside tags; new problems under 50 ms; `selection` shape in the new code actions; `artisan` terminal runs.
- **spring-beans:** `spring-report` view and icon; file events on `META-INF/spring.*`; absolute paths from `find_files`.
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
