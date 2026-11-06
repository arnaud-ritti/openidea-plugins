# Parity with IntelliJ IDEA plugins

How far each plugin in `plugins/` reaches the feature set of the IntelliJ plugin it stands in for.

Method: derived from each `plugin.toml` and from [LIMITATIONS.md](LIMITATIONS.md), which records the gaps found while porting (last checked 2026-11-05 against OpenIDEA v0.1.37). Nothing here was measured by running the original plugins side by side, so the ratings are judgement calls, not test results. Closed-source originals were judged from their Marketplace descriptions only.

Rating: **High** = the everyday workflow works, gaps are edge features. **Medium** = core navigation and checks work, an important piece is missing or approximated. **Low** = a small slice of the original.

Path: **Source** = ported from an open-source repo, licence files kept. **Clean-room** = behaviour only, original is proprietary.

## Summary

| Plugin | IntelliJ counterpart | Path | Parity | Main gaps |
|---|---|---|---|---|
| `dotenv` | .env files support (`ru.adelf.idea.dotenv`) | Source | High | Hide values, `$` auto-popup, Find Usages/Rename UI, plugin extension points; compose anchors and multi-line flow collections |
| `ignore` | .ignore (`mobi.hsz.idea.gitignore`) | Source | High | Not reviewed in LIMITATIONS.md: check against the original's rename/generate actions |
| `properties` | Properties (`com.intellij.properties`) | Source | Medium | No references/rename between bundles, no structure view for built-in `properties` language beyond the plugin's own |
| `tasks` | Tasks & Contexts (`com.intellij.tasks`) | Source | Medium | Switch/Go-to-Task popups, self-hosted trackers (fixed `net:` hosts), changelists, context clearing, bookmarks/breakpoints, commit dialog |
| `gitlab` | GitLab (`org.jetbrains.plugins.gitlab`) | Source | Medium | Fixed hosts (gitlab.com only), no clone/push/commit API, no IDE diff viewer, no inline comment decorations, no secret writes |
| `ideolog` | Ideolog (log viewer) | Source | Medium | Log language can't take a plugin syntax, no line hiding, no scrollbar marks, caret can't move, no banners |
| `quarkus` | Quarkus Tools (`com.redhat.devtools.intellij.quarkus`) | Source | Low-Medium | LSP4MP and qute-ls unusable (hover, code lens, Qute, JAX-RS, Java diagnostics), no wrapper runs, bundled metadata only (42 extensions), no project wizard |
| `spring-osgi` | Spring OSGi (`com.intellij.spring.osgi`) | Source | Medium | Checks needing real type resolution, no Generate-menu API |
| `laravel` | Laravel Plugin (`de.espend.idea.laravel` 0.15.4, free) | Source (partial) + Laravel docs | High vs the free plugin; Medium vs Laravel Idea (paid) | Service container and facades, Eloquent relations and accessors, `Blade::directive()` in vendor code (project `app/**` ones complete), component slots/attributes, `make:*` artisan commands (no input dialog), `artisan` run configurations are fixed commands, 16 New-menu file templates (fixed namespaces, no prompts), Extract partial view, `ide-blade.json` namespaces. Adds features the free plugin lacks: env, middleware, Vite, validation rules, gates, migration columns, tool window, missing-name problems for Blade/Livewire components, Inertia pages and model factories, intentions (validation string to array, controller string to class array, Blade `{{ }}` / `{!! !!}` toggle) |
| `blade` | Blade support (PhpStorm) | Clean-room | Low-Medium | Highlight, structure, folding; `@` directive completion (core, Livewire, project `Blade::directive`) comes from `aritti.laravel`. No PHP/HTML injection (own approximate lexer), custom directives not highlighted, no formatter, `*.blade.php` → `blade` language resolution read in the host source but not run |
| `symfony` | Symfony Support (Espendiller/Brault) | Source | Medium | No references/rename/hover, no Twig-in-PHP injection, no profiler, no twig.yaml namespaces, no file-creating quick fixes |
| `twig` | Twig support (PhpStorm) | Clean-room | Low | Highlighting, structure and folding only: no completion, inspections or editing behaviour |
| `spring-beans` | Spring core (`com.intellij.spring`) | Clean-room | Medium | Text-matched types, no library/auto-configured beans, no profiles/conditions, no SpEL/JPQL injection, single-target gutter, no rename |
| `spring-boot` | Spring Boot (`com.intellij.spring.boot`) | Clean-room | Medium | Metadata pinned to Boot 3.5, no starter/jar metadata, no hover docs, no run-config awareness |
| `spring-data` | Spring Data (`com.intellij.spring.data`) | Clean-room | Medium | No JPQL in `@Query`, no repository generators, simple-name entity matching |
| `spring-security` | Spring Security | Clean-room | Low-Medium | Migration checks only; no XML security namespace, no expression language |
| `spring-web` | Spring MVC (`com.intellij.spring.mvc`) | Clean-room | Low-Medium | PathVariable and view-name links only; no Endpoints window, no WebFlux functional routes, no request generation |
| `spring-websocket` | Spring WebSocket | Clean-room | Medium | No JS STOMP clients, gutter one target |
| `spring-messaging` | Spring Kafka/AMQP/JMS | Clean-room | Medium | No XML containers, no `${…}` placeholder resolution, no unmatched-destination warnings |
| `spring-graphql` | Spring GraphQL | Clean-room | Medium | No client documents, no GraphQL endpoints list |
| `spring-cloud` | Spring Cloud | Clean-room | Low-Medium | Feign/Stream links only; no bundled Spring Cloud metadata |
| `spring-integration` | Spring Integration | Clean-room | Medium | No diagram in IDE format, no `int:` namespace typing |
| `spring-batch` | Spring Batch | Clean-room | Medium | No diagram, Java config only partly |
| `spring-webflow` | Spring Web Flow | Clean-room | Medium | No flow diagram |
| `spring-shell` | Spring Shell | Clean-room | Medium | Checks on availability/duplicates only |
| `spring-ws` | Spring Web Services | Clean-room | Medium | XSD/WSDL top-level elements only |
| `spring-modulith` | Spring Modulith | Clean-room | Medium | No module structure in Structure view, no Project view badges, no docs export |
| `spring-initializr` | Spring Initializr wizard | Clean-room | Low | Builds a URL and opens a browser: no project creation or import in the IDE |
| `spring-runtime` | Spring Debugger / Boot dashboard | Clean-room | Low | Actuator snapshot only, localhost only, no debugger integration, no inline values |
| `jpa-assist` | JPA support (Ultimate) + JPA-generator tooling | Clean-room | Low-Medium | No new-file generation (output goes to clipboard), no wizards, no database/schema access, no refactoring hooks, no JPQL |
| `flyway` | Flyway support (Database Tools) | Clean-room | Low-Medium | Naming checks and CLI commands; no wrapper runs, no rename fix, no confirm dialog, no DB connection |
| `liquibase` | Liquibase support (Database Tools) | Clean-room | Low-Medium | Changelog checks and CLI commands; fixed run-config args, no classpath resolution |
| `openapi` | OpenAPI Specifications (`com.intellij.swagger`) | Clean-room | Medium | YAML only, no rename/usages/hover, no structure view, preview only by Open With |
| `dependency-audit` | Vulnerable dependency checks | Clean-room | Medium | No resolved build model (BOM/parent skipped), no advisory link from the squiggle, no Go/other ecosystems |
| `kube-explorer` | Kubernetes (`com.intellij.kubernetes`) | Clean-room | Medium | No context-menu actions (until upstream #1746), no streaming logs or watch, no custom-resource schemas, no Helm template language |
| `devcontainer` | Dev Containers (Gateway) | Clean-room | Low | Start/rebuild/stop/shell in a terminal; no open-inside-container, no result tracking, no features browser |

## Cross-cutting gaps (largest effect first)

These explain most of the "Medium" and "Low" ratings. Each is an OpenIDEA API gap, not a plugin bug.

1. **No semantic model of Java/Kotlin/PHP.** Every Spring and Symfony plugin is text-based: simple-name type matching, no meta-annotations, no library beans. Affects ~22 plugins, Laravel and Blade included (no container/facade/Eloquent types).
2. **No references, rename or hover providers.** Find Usages, Rename and quick documentation are missing everywhere. Providers are only completion, inline-completion, code-actions, definition and line-markers.
3. **No file creation from plugins.** No new-class/new-file quick fixes, generators fall back to the clipboard, no project wizards (Initializr, Quarkus, JPA, Flyway).
4. **No popup, quick-pick or input dialog, and commands cannot open a view.** Switch Task, bean choosers, confirmations and wizard-style flows become forms or fixed commands.
5. **No language injection.** SpEL, JPQL/SQL, Helm/Go templates and Qute cannot be highlighted inside Java strings, YAML or XML.
6. **Diagnostics only on open/save.** No provider of their own and no document-changed event; problems lag edits.
7. **No file-change events, `find_files` skips hidden/ignored paths, no classpath or jar reading.** Indexes go stale (120 s rescan), `target/`/`build/` metadata and library metadata are unreachable.
8. **Fixed capabilities.** `net:` hosts and `process:` programs are pinned in the manifest: no self-hosted trackers or GitLab, no `./mvnw`/`./gradlew`, no user-chosen `kubectl` path.
9. **No streaming processes or background work.** Provider budget 50 ms, commands 5 s; no log tailing, no async indexing.
10. **Single-target gutter markers with a fixed icon set.** Multi-candidate beans/listeners open only the first.

## Not covered by any plugin here

Original features with no OpenIDEA equivalent, so no port is possible yet: Spring Debugger inspection, bean/flow diagrams as IDE diagrams, database console integration, inlay hints, Structure view for built-in languages, Project view decorations, Endpoints tool window and HTTP client integration, run configurations computed by code.

## Laravel Idea 13.4.1 feature inventory (paid baseline)

Source: the extension descriptors, action ids, inspection/intention names and template names of `laravel-idea-13.4.1.261.jar`, read at the user's request on 2026-11-06 (no decompiled code was read or kept). Only feature names are recorded here; no code, text or icons were copied. The extraction lives outside the repo in `/tmp/li`.

Scale: about 3,300 classes, 15 descriptors (Blade, JS/Inertia, Twig, YAML, CSS, dotenv, database, terminal, endpoints, MCP, LLM, Qodana, git), 33 actions, 11 intentions, 14 inspections, ~50 file templates.

| Area in Laravel Idea | In `aritti.laravel` / `aritti.blade` today | Possible with the current API? |
|---|---|---|
| Completion and navigation: routes, views, config, translations, env, middleware, assets, validation, gates, columns, Blade/Livewire components, Vite | Done (see README) | Done |
| Eloquent "magic" (relations, accessors, scopes, `$user->posts()->where`), container and facade types, `typeProvider`s | Missing | **No**: needs a PHP type model |
| Inspections: unknown view, config key, route name | Done | Done |
| Inspections: unknown Inertia page, Eloquent factory, Blade component, Livewire component/directive/method | Missing | **Yes**, text-based, same pattern as the view check |
| Inspections: guarded-attribute assignment, unscheduled prunable model, simplify `orderBy`, routes inspection | Missing | Partly: the text-matchable ones |
| Intentions: validation string to array, controller call to class array, rule string to rule object | Missing | **Yes**: edits to the open file |
| Intentions: Blade braces escaped/unescaped | Missing | **Yes**: trivial text edit |
| Intentions: facade to dependency injection, move injection to constructor, scope call to attribute | Missing | No: needs types |
| Intention: promote request to FormRequest, ~29 generators (controller, model, migration, policy, observer, resource, factory, listener, Livewire form, Nova/Filament/Fractal) | Missing | **No** for generation: plugins can't create files. **Partly** via `[[contributes.fileTemplates]]` (New menu) written from scratch |
| ~50 file templates (controllers, jobs, events, mailables, middleware, Pest/Dusk tests, Livewire/Volt, Inertia pages in Vue/React/Svelte) | Missing | **Yes** as New-menu templates; no package/name prompts |
| Extract view / extract Blade component, rename of views, usages of a view | View usages via gutter only | No (needs refactoring and file creation) |
| Go to symbol contributors, route search action (⇧ search) | Tool window lists routes | No Search Everywhere hook; tool window covers it |
| Code vision, declarative inlay hints, documentation provider, folding | Missing | No (no hover, inlay or code-vision API) |
| Database: models from tables, DB connections from `.env`/YAML, schema dumps | Missing | No (no database API) |
| Artisan: run command, terminal command specs | Fixed commands only | Partly: one command per common task; arbitrary command needs an input dialog |
| Postfix templates (`dd`, `dump`, `ray`), surround, spellcheck dictionary, reader mode | Missing | No |
| Blade: unknown-component and Livewire checks, custom directives, Vue-in-Blade context, PHP/HTML injection | Highlight, outline, fold only | Inspections yes; injection and custom directives no |
| JS side: Ziggy/route and Inertia completion, line markers, library roots | Missing | Partly: completion in `javascript`/`typescript` providers is possible; library roots no |
| Twig, YAML, CSS, dotenv integration | Missing | Partly (dotenv link via `aritti.dotenv` is a separate plugin) |
| Project wizard, module builder, Endpoints, MCP server, LLM hooks, Qodana, git hook | Missing | No |

Cheapest wins from this list, in order: the Blade/Livewire/Inertia/factory unknown-name inspections, the string-to-array and controller-call intentions, the braces conversions, then a set of from-scratch New-menu file templates.

## Caveats

- `laravel` is rated against two baselines. The free Espendiller plugin is the one it ports and the only one whose source was read; Laravel Idea is proprietary and was judged from its Marketplace description, so that rating is rougher.
- The Laravel and Blade rows are backed by the new section in LIMITATIONS.md (checked 2026-11-06) and the READMEs, not by running a Laravel project in the IDE.

- `ignore` and `properties` have no section in LIMITATIONS.md, so their gaps above are from their `plugin.toml` descriptions and the original's known features, not a documented audit. Worth a follow-up pass.
- `twig` is rated from its manifest (highlighting, structure, folding); the PhpStorm original is proprietary and its exact feature list was not re-checked.
- To turn the ratings into measured numbers, list the original's features per plugin and tick each as done, workaround or missing; LIMITATIONS.md already holds most of the missing/workaround rows.
