# Spring Endpoints Tools

See every endpoint of a Spring project in one tool window, and jump between handlers, templates, clients and configuration. Covers Spring MVC, WebSocket (STOMP), messaging (Kafka, RabbitMQ, JMS), Spring for GraphQL, Spring Web Flow and Spring Cloud (OpenFeign, Stream, Gateway), in Java and Kotlin. MIT; see `LICENSE`.

This plugin replaces five older plugins: `aritti.spring-websocket`, `aritti.spring-messaging`, `aritti.spring-graphql`, `aritti.spring-webflow` and `aritti.spring-cloud`. They are no longer maintained. Uninstall them, or their windows and completions show twice.

## Requirements

- OpenIDEA with plugin API 1.14 or newer (`engines.openidea = "^1.14"`).
- A Spring project. A module stays idle when the root build file (`pom.xml`, `build.gradle`, `build.gradle.kts`) does not mention its dependency.
- For the **Spring Endpoints** window and its Tools menu entries: a `pom.xml`, `build.gradle(.kts)` or `settings.gradle(.kts)` at the project root.
- Node.js 22 or newer, only for **Start GraphQL Subscription…**.
- For SpEL inside Thymeleaf templates: the Spring Beans plugin (it supplies the `spel` language).

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)). Uninstall the five older Spring plugins listed above if you have them.
2. Open a Spring project.
3. Open the **Spring Endpoints** tool window on the right side and pick the **HTTP** section.
4. Click a route to open its handler. Right-click it for **Copy Endpoint as cURL** or **Create HTTP Request File**.
5. In a controller, press ⌥⏎ on a method for generators and quick fixes.

## Features

### Browse endpoints in the Spring Endpoints window

The window has six sections:

- **HTTP**: every route as `path [GET] Controller`. **Filter…** narrows the list to chosen controllers; *Clear* restores it. *Flat/Tree* groups routes by controller. Literal `@RequestMapping` names show as `class#method`.
- **WebSocket**: STOMP destinations.
- **Messaging**: Kafka, RabbitMQ and JMS destinations, grouped broker › destination › consumers, producers and declarations.
- **GraphQL**: the GraphQL HTTP and WebSocket paths, GraphiQL, the schema printer and the schema files.
- **Web Flow**: a diagram of the flow in the active editor.
- **Feign**: the project's Feign client methods per file.

Click a row to open its source. **Refresh** rescans. The window remembers the last section you used.

To jump to a route without the window, press Shift twice and open the **Spring Endpoints** tab. Type part of the path, the HTTP method or the controller (`get users`, `/orders`, `UserController`); Enter opens the mapping. In a large project the tab first lists what is already indexed and **Show more** finds the rest.

### Try an endpoint

- **Copy Endpoint as cURL** (right-click a route) copies a curl command.
- **Create HTTP Request File** asks for the base URL, then writes `http/<verb>-<path>.http` under the project root and opens it. An existing file is opened unchanged.
- Run either from Search Everywhere and it first asks which endpoint to use.
- Above each mapping method, the **HTTP request: GET /path** code lens does the same from the source. Mappings without a fixed method ask you to pick one.
- Above GraphQL query and mutation handlers, the **GraphQL request: …** lens writes a JSON POST request with sample arguments.
- **Copy as OpenAPI** (Tools menu) copies an OpenAPI 3.0.3 document built from your source. A notification confirms the copy and lists anything left out.

Sending requests stays with the IDE's built-in HTTP client.

### Catch mapping mistakes

- A `@PathVariable` whose name is not in the mapped path is an error. ⌥⏎ **Bind the `{name}` variable** fixes it when only one template variable is left.
- A `{variable}` that no parameter takes is a warning.
- A mapping with `version` (Spring Framework 7) warns when the project sets up no API versioning. A malformed version value also warns.
- An `@InitBinder` method that returns a value, or takes no `WebDataBinder`, is flagged.
- Spring MVC XML (`<mvc:…>`) gets attribute and bean reference checks.

### Jump between controllers and templates

- A view name returned by a `@Controller` (`return "users/list";`) gets a gutter arrow, Go to Definition and completion of template names. Any gutter arrow with several targets (templates, handlers, destinations, schema fields) opens a chooser.
- A Hint marks a view name that matches no template. It stays quiet when the project defines its own view resolver.
- In Thymeleaf templates, fragment names (`th:insert`, `th:replace`) and model attribute names complete and navigate.
- Static resource strings (`"/css/app.css"`) and HTML `src`/`href` links open the file.

### Navigate from tests and request files to handlers

- URL strings in MockMvc, WebTestClient, RestClient and RestTemplate test calls link to the handler and complete known paths.
- `.http`/`.rest` request files navigate request paths to local handlers.
- Each mapping shows its URL in the gutter (`URL: GET /users/{id}`).

### Write controllers faster

- ⌥⏎ in a controller: **Add @RequestMapping method** and **Add @ExceptionHandler method**.
- New › **Spring Controller**, **Spring REST Controller** and **Spring Controller Advice**, in Java and Kotlin.
- Snippets: `getm`, `postm`, `putm`, `delm`, `patm`, `reqm`, `exh`, `initb`, `router`, `restc`, `ctrl`, `advice`.
- ⌥⏎ on a route variable: **Rename route variable and explicit binding** renames `{id}` and its `@PathVariable("id")` together.

### Follow WebSocket and messaging destinations

- STOMP handlers (`@MessageMapping`) link to the places that send to them, and to JavaScript/TypeScript clients.
- Broker destinations outside the configured broker prefixes get a warning.
- Kafka, RabbitMQ and JMS listeners link to their producers and declarations, and back.
- Destination names complete inside those strings.

### Link GraphQL handlers to the schema

- Handler methods (`@QueryMapping`, `@SchemaMapping`…) link to their schema field, and fields link back.
- Client documents under `graphql-documents/` navigate to the schema.
- Handlers for fields the schema doesn't declare get a warning.
- **Prepare GraphQL Subscription from Handler** writes an editable `.graphql` subscription document. **Start GraphQL Subscription…** runs it and shows a review screen before connecting. **Read GraphQL Subscription Output** and **Cancel GraphQL Subscription** are available only while a subscription runs. Once the output says the subscription ended, everything it printed has been shown.

### Edit Web Flow definitions

- `to`, `then`, `else`, `start-state`, `subflow` and `parent` complete and navigate.
- Unknown states, duplicate state ids and unknown subflows are reported.
- ⌥⏎ **Add Web Flow transition…** adds a transition to a view state after you review it.

### Work with Spring Cloud

- Feign client methods link to the controller methods they call, and back.
- `Function`/`Consumer`/`Supplier` beans link to their Stream bindings. Binding and definition names complete and are checked.
- Gateway route predicates and filters complete and are checked.
- A `bootstrap.yml` that Spring Boot 2.4+ ignores gets a warning.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Spring Endpoints**; bind keys in Settings › Keymap. No shortcuts are set by default.

| Command | What it does | Shortcut |
|---|---|---|
| **Spring Endpoints: Copy Endpoint as cURL** | Copies a curl command for a route. From a route's right-click menu, or from Search Everywhere, where it asks which endpoint. | — |
| **Spring Endpoints: Create HTTP Request File for Endpoint** | Writes `http/<verb>-<path>.http` for a route and opens it. From a route's right-click menu (**Create HTTP Request File**), or from Search Everywhere, where it asks which endpoint. | — |
| **Spring Endpoints: Copy as OpenAPI** | Copies an OpenAPI 3.0.3 document of all routes and confirms in a notification. Also in the Tools menu. | — |
| **Spring Endpoints: Create HTTP Request from Endpoint Source** | Writes a request file for a mapping method in the active editor. Asks which endpoint when there are several. | — |
| **Spring Endpoints: Create HTTP Request from GraphQL Handler** | Writes a GraphQL JSON POST request for a query or mutation handler. | — |
| **Spring Endpoints: Create HTTP Request from Feign Client** | Writes a request file for a Feign client method. | — |
| **Spring Endpoints: Prepare GraphQL Subscription from Handler** | Writes an editable `.graphql` subscription document from a subscription handler. | — |
| **Spring Endpoints: Start GraphQL Subscription…** | Connects the current `.graphql`/`.gql` subscription document after a review screen. Also in the Tools menu. | — |
| **Spring Endpoints: Read GraphQL Subscription Output** | Shows the output received so far. Available only while a subscription runs; also in the Tools menu. | — |
| **Spring Endpoints: Cancel GraphQL Subscription** | Ends the running subscription. Available only while a subscription runs; also in the Tools menu. | — |

## Settings

Settings › Plugins › Spring Endpoints Tools (section **Spring Endpoints**). All settings are on except the Thymeleaf SpringEL one.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Warn when a request mapping uses the version attribute but the project configures no API versioning | `springWeb.checkApiVersioning` | on | API versioning warning and the `apiVersion(...)` test hint. |
| Warn when the version attribute of a request mapping is not a major.minor.patch version | `springWeb.checkVersionFormat` | on | Version format check. |
| Warn when an @InitBinder method returns a value or takes no WebDataBinder | `springWeb.checkInitBinder` | on | `@InitBinder` checks. |
| Hint at view names with no template | `springWeb.reportUnresolvedViews` | on | Unresolved view hint. |
| Show the URL of each request mapping in the gutter | `springWeb.showUrlMarkers` | off | URL gutter markers. The IDE already shows an HTTP client icon on every mapping and route. |
| Enable Thymeleaf SpringEL support (requires Spring Beans) | `springWeb.thymeleafSpelInjection` | off | SpEL in Thymeleaf `${…}`/`*{…}` and in `defaultValue` of request parameters. |
| Warn when a @SendTo or messaging template destination is outside the configured broker prefixes | `springWebsocket.checkBrokerPrefixes` | on | STOMP broker prefix warning. |
| Warn when a mapping handler names a field the project's GraphQL schema does not declare | `springGraphql.checkUnmatchedMapping` | on | Unknown schema field warning. |
| Warn when a @BatchMapping method returns a type Spring for GraphQL does not accept | `springGraphql.checkBatchMappingReturn` | on | `@BatchMapping` return type check. |
| Report schema fields with arguments that no controller serves | `springGraphql.reportUnmappedFields` | on | Unmapped field Info. |
| Flag Stream bindings that no configuration declares | `springCloud.checkStreamBindings` | on | Unbound channel hint. |
| Warn about ignored bootstrap files | `springCloud.checkBootstrap` | on | Ignored bootstrap file warning. |
| Check legacy @StreamListener / @SendTo channels, return values and @BootstrapConfiguration classes | `springCloud.checkLegacyStream` | on | Legacy Stream checks and handler signature checks. |

The settings of the five older plugins keep their keys (`springWeb.*`, `springWebsocket.*`, `springGraphql.*`, `springCloud.*`). Their old values are not migrated, so set them again here.

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › Spring Endpoints Tools. See the [repository guide](../../README.md#permissions).

| Permission | Used for | Without it |
|---|---|---|
| `project.write` | Writing `.http` request files and `.graphql` subscription drafts. | The request or document is copied to the clipboard instead (if `clipboard` is allowed). |
| `clipboard` | **Copy Endpoint as cURL**, **Copy as OpenAPI**, and the fallback above. | Copy Endpoint as cURL shows "Copy Endpoint as cURL needs the clipboard permission…". The OpenAPI export fails. |
| `editor.read` | The **from … Source/Handler/Client** commands and the request code lenses, which read the active editor. | Those commands can't find the source. |
| `events` | Keeping the indexes fresh when files change, open or save. | Checks that report something *missing* (unresolved views, unmapped fields, Stream bindings) stay quiet. |
| `storage` | Remembering the last section of the Spring Endpoints window. | The window opens on its default section. |
| `process:node` | Running **Start GraphQL Subscription…** with Node.js 22+. | GraphQL subscriptions can't start. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| Windows or completions appear twice | Uninstall `aritti.spring-websocket`, `aritti.spring-messaging`, `aritti.spring-graphql`, `aritti.spring-webflow` and `aritti.spring-cloud`. |
| The **Spring Endpoints** window or its Tools menu entries are missing | They appear only when the project root has a `pom.xml`, `build.gradle(.kts)` or `settings.gradle(.kts)`. Open the folder that holds the build file. |
| "No Spring endpoints found. This project doesn't look like a Spring project…" | The project root has no `pom.xml` or `build.gradle`. Open the module's folder. |
| "No Spring endpoints found. Add a @RestController…" when copying curl or creating a request file | The endpoint list is empty. See the next row. |
| "Still indexing endpoints. Try again in a moment." | The project is still being read. Wait a few seconds and run the command again. |
| The HTTP section says "No routes found in this project." | Routes need literal paths in a `@Controller`/`@RestController`, or functional routes. Paths built from constants, concatenation or `${…}` are not read. Check the root build file mentions Spring. |
| "Copy Endpoint as cURL needs the clipboard permission. Allow it in Settings › Plugins › Spring Endpoints Tools." | Allow `clipboard` in Settings › Plugins › Spring Endpoints Tools. |
| **Create HTTP Request File** copies the request instead of creating a file | Allow `project.write`. |
| "Spring routes are still being indexed; try again in a moment." from **Copy as OpenAPI** | The command reads the project for up to 50 seconds and was cancelled or ran out of time. Wait until the **Spring Endpoints** window stops showing "scanning…", then run the command again. |
| The **Spring Endpoints** window shows "scanning…" on a large project | It reads a few hundred files at a time and fills in as it goes. Leave the window open; the list completes on its own. |
| The IDE's own endpoint list misses functional routes (`RouterFunction`) right after opening a large project | The IDE keeps the first, partial list until a Java or Kotlin file changes. Once the **Spring Endpoints** window has finished scanning, save any Java or Kotlin file. |
| "GraphQL schema index is incomplete; refresh before generating a request." | Wait for indexing, then try again. |
| "Endpoint changed; refresh its HTTP request lens." | The source changed since the lens appeared. Use the refreshed lens. |
| "The OpenAPI document is too large to copy (over 1 MiB)." | The project has too many routes for one export. |
| "Install/approve Node22 or newer with its built-in WebSocket available." | Install Node.js 22+ on the `PATH` the IDE sees and allow `process:node`. |
| "A subscription is already running…" | Run **Read GraphQL Subscription Output** or **Cancel GraphQL Subscription** first. |
| A "missing" check never fires (unresolved view, unmapped field, unknown binding) | These wait until the whole project is read. Allow `events`. They also stay quiet when the project could provide the item another way (a custom `ViewResolver`, a `DataFetcher`…). |
| JavaScript STOMP clients are not linked | Open one Java or Kotlin file of the backend once. This is needed when the root build file doesn't mention the WebSocket dependency. |
| SpEL is not highlighted in Thymeleaf templates | Turn on **Enable Thymeleaf SpringEL support** and install the Spring Beans plugin. |

## Limitations

- Matching is text-based. Paths and names from constants, concatenation or `${…}` placeholders are mostly skipped.
- The plugin doesn't send requests; the IDE's HTTP client does.
- No raw `WebSocketHandler` registration checks or SockJS options.
- No live broker connection or live topics.
- No GraphQL subscription transports beyond `graphql-transport-ws` (no legacy protocol, SSE or RSocket).
- Web Flow flows registered in libraries or other modules aren't seen.
- Gateway routes built with the Java DSL and Spring Cloud Config, discovery, load balancer and circuit breaker settings are not covered.

## Technical details

<details>
<summary>Window, indexing and diagnostics</summary>

- The window sits on the right. **HTTP** is the route list described under Spring Web below, with its controller filter, Flat/Tree toggle and row actions.
- A module whose dependency is not in the root build file does no work. Multi-module builds and version catalogs count as unsure, so the module runs.
- The registered `window::Endpoints` wrapper owns API 1.3 current-document diagnostics. It returns one merged response for the active HTTP, WebSocket, messaging, GraphQL, Web Flow and Cloud modules. Embedded marker providers no longer publish or recompute these checks.
- Diagnostics and hover read current unsaved text and cached facts only. Dependency applicability and module indexes warm during activation and file events, outside editor requests.
- Optional event denial, cold/stale/incomplete reads, failed reads and oversized project inputs suppress absence-based findings.
- Current input is limited to 128 KiB and the merged response to 500 findings.

</details>

<details>
<summary>Spring Web (MVC)</summary>

Spring MVC helpers for Java controllers, Kotlin controllers included.

- **Endpoints list** (HTTP section): a flat list with the generic native endpoint icon, the path, then grey text `[METHOD] Controller`. *Filter* narrows the list to one controller (the status line says which); *Clear* restores it; *Flat/Tree* groups rows by controller. Row actions: *Copy Endpoint as cURL* and *Create HTTP Request File*. Limits: the grey text follows the path rather than sitting in a column; there is no request panel beside the list and no gutter icon on the handler. These wait on upstream OpenIDEA requests.
- **Path variables**: in `@Controller`/`@RestController` handler methods, a `@PathVariable` whose name (`value`/`name`, else the parameter name) is missing from one of the method's mapped paths (class `@RequestMapping` prefix included) is an error. When exactly one template variable is left unbound, a quick fix renames the parameter's variable to it. A `{variable}` of a method mapping that no `@PathVariable` takes is a warning, unless a `@PathVariable Map` or a form object (`@ModelAttribute`, or an unannotated parameter of a non-framework type) may bind it. The `{name}`, `{name:regex}` and `{*name}` forms are supported.
- **API versions**: a request mapping that sets `version` (Spring Framework 7) is a warning when the project configures no API versioning. It stays quiet unless every source and `application*` file was read, at least one `application*` file exists, and none mentions `apiversion` (`spring.mvc.apiversion.*`) or `ApiVersion…`/`configureApiVersioning`. Setting `springWeb.checkApiVersioning`. In tests, a Hint on `apiVersion(...)` when no file of the project mentions `ApiVersionInserter` or `spring.mvc.apiversion.*`.
- **@InitBinder**: an `@InitBinder` method that returns a value is an error (Spring throws). One that takes no `WebDataBinder`/`DataBinder` parameter is a warning. Setting `springWeb.checkInitBinder`.
- **View names**: in `@Controller` handler methods without `@ResponseBody`, `return "users/list";`, `new ModelAndView("…")` and `setViewName("…")` get a gutter arrow, Go to Definition to the template, and completion of template names. Templates are looked up in `src/main/resources/templates/` (`.html`, `.ftlh`, `.ftl`, `.mustache`, `.peb`, `.jte`, `.kte`), `src/main/webapp/WEB-INF/{views,jsp}/` (`.jsp`) and where `spring.mvc.view.prefix/suffix` or `spring.thymeleaf.prefix/suffix` point. `redirect:` and `forward:` views are skipped. A Thymeleaf `name :: fragment` resolves to `name`. An unknown view name is not reported as an error, since another view resolver may handle it. XML `<mvc:view-controller view-name>` and Java `addViewController(...).setViewName(...)` navigate to their templates too.
- **Unresolved views** (`springWeb.reportUnresolvedViews`, default on): a Hint on a returned view name that no template of the known folders answers, once the project has at least one template. Quiet until every source was read. Quiet for good when any source mentions `ViewResolver`, `ViewResolverRegistry`, `configureViewResolvers`, `TilesConfigurer` or `AbstractView`, since another resolver may serve the name. The unresolved-template inventory needed for a stronger check is not modeled.
- **Functional routes**: `RouterFunctions.route().GET("/x", handler)` (also `POST`/`PUT`/`DELETE`/`PATCH`/`HEAD`/`OPTIONS`), `route(RequestPredicates.GET("/x"), handler)`, `.andRoute(…)`, and the prefixes of `.path("/api", b -> …)` and `.nest(path("/api"), b -> …)` are read as endpoints. So is the Kotlin `router {}`/`coRouter {}` DSL (`GET("/x")`, `"/api".nest { … }`, `nest(path("/api")) { … }`). A handler written `this::list`, `Handler::list` or Kotlin `::list` gets a gutter arrow and Go to Definition to the project method. Kotlin sources (`**/*.kt`) are indexed for routes, controller checks and literal view navigation. A route under a `nest` whose predicate is not a plain `path("…")` is skipped, including Kotlin `("/a" and accept(…)).nest { }`. Kotlin `"/a".nest { }`, `"/a" nest { }` and `path("/a").nest { }` are read.
- **Test URLs**: in `src/test/` sources, the URL literal of `mockMvc.perform(get("/users/{id}", 1))` (any HTTP method, also `MockMvcRequestBuilders.get`), `webTestClient.get().uri("/users")` (also `WebClient`/`RestClient`) and `restTemplate.getForObject("/users", …)`/`exchange("/users", HttpMethod.GET, …)` gets a gutter arrow and Go to Definition to the mappings that serve it. Matching uses the HTTP method and URI template: `{id}` matches any segment; `*`, `**`, `{*rest}` and mixed segments like `by-{name}` work. Every matching mapping is offered. `{id:regex}` constraints are ignored when matching, so more mappings than Spring would pick may be offered. Absolute URLs (`http://…`) are skipped. A URL no mapping serves is left alone. Mappings are those with literal paths in `@Controller`/`@RestController` classes, plus functional routes. MockMvc/WebTestClient/RestClient/TestRestTemplate/RestTemplate/WebClient test-URL completion and navigation require a receiver declared in the same file with an exact import; the older name heuristics (a receiver named like `restTemplate`/`template`) are removed.
- **URL markers** (off by default): every request mapping and functional route shows its URL (`URL: GET /users/{id}`) in the gutter, unless a link marker already uses the line. Setting `springWeb.showUrlMarkers`. The marker has no target.
- **Endpoints published to the IDE**: functional routes are published for Search Everywhere and the HTTP client. `@Controller`/`@RestController` mappings are not, because the IDE indexes them itself and already draws its own icon on them; publishing them too gave each mapping two icons and duplicate search results.
- **URL completion in tests**: inside the URL literal of `mockMvc.perform(get("…"))`, `webTestClient.get().uri("…")` and `restTemplate.getForObject("…")`, the project's mapping paths for that HTTP method are offered.
- **Static resources**: Go to Definition on a string literal such as `"/css/app.css"` or `"classpath:/static/js/a.js"` opens the file under a `static/`, `public/` or `META-INF/resources/` folder of the project (`css`, `js`, images, fonts, `html`). Literal HTML/JSP `src`/`href` and Thymeleaf `@{/asset}` links navigate to Spring static roots, including query/fragment suffixes. Dynamic expressions and commented markup are skipped.
- **Snippets** (Java and Kotlin, with tab stops): in a controller class body `getm`, `postm`, `putm`, `delm`, `patm`, `reqm` (mapping methods), `exh` (exception handler) and `initb` (init binder). In any class body `router` (functional `RouterFunction` bean, Kotlin `router {}`). Outside every type `restc`, `ctrl` and `advice` (controller classes). Snippets add no imports.
- **File templates**: New menu entries Spring Controller, REST Controller and Controller Advice, for Java and Kotlin. They use `$PACKAGE$`, `$NAME$` and `$END$`; the host drops the `package` line when there is no package.
- **Kotlin controllers**: the path variable, API version and `@InitBinder` checks run on Kotlin controllers. The Add `@RequestMapping`/`@ExceptionHandler` method actions generate Kotlin (`fun handle(): String`, no semicolons in the import).
- **HTTP request files**: on an endpoint in the Endpoints view, Create HTTP Request File for Endpoint asks for the base URL, writes `http/<verb>-<path>.http` under the project root (`### GET /users` plus the request line) and opens it. An existing file is opened untouched. The old README named `project.write` for this; the current code writes with `project.write`, and without it the request goes to the clipboard (optional `clipboard`). Sending a request stays with the IDE's HTTP client.
- **Request file navigation**: HTTP request files (`.http`/`.rest`) complete literal Spring paths and navigate request paths to local handlers. Absolute HTTP(S) and `{{host}}` authorities and query text stay outside replacement ranges. Request bodies are not scanned as requests. HEAD completion includes GET mappings. Native HTTP-file endpoint completion is owned by the host through `EndpointsProvider`. This plugin supplies path definition navigation and only supplements missing HEAD-to-GET completion, using native `{{name}}` variable syntax and excluding paths already supplied by explicit HEAD/ANY mappings.
- **`<mvc:…>` XML**: source-owned schema profiles cover 54 element rows across official 3.0–4.3, 5.3, 6.2 and current namespaces, with required attributes, boolean/integer values and local unique bean references. MVC XML bean refs resolve through local `<import>` closures and `<beans profile>` blocks (Info for profile-only and undeclared beans). `<mvc:resources>` locations are checked under `src/main/resources` and `src/main/webapp`. Unknown namespace versions and unsupported contracts remain unclaimed.
- **Thymeleaf fragments and models**: literal fragment expressions (`th:insert`, `th:replace`, legacy `th:include` and HTML5 data attributes) offer known template/fragment completion and definition navigation. Named fragments resolve to exact `th:fragment` declarations; same-template fragments use unsaved source; `th:replace '~{layout :: #id}'` selectors jump to the unique element with that id. Dynamic selectors, parameterized invocations and runtime template-stack resolution are not inferred. Source-declared MVC model names complete and navigate in matching templates: literal `Model`/`ModelMap.addAttribute` values and explicitly named `@ModelAttribute` methods are tied to the handler's returned view. `@ModelAttribute` names from a `@ControllerAdvice` and `@SessionAttributes` names complete too, as do argument-bearing fragment calls. Local `th:each`/`th:with` names are excluded. This is positive name navigation, without member typing or claims that unknown/runtime attributes are missing.
- **SpEL injection**: the optional **Enable Thymeleaf SpringEL support** injects exact `${...}`/`*{...}` attribute ranges into the `spel` language supplied by the Spring Beans plugin. It defaults off because standalone Spring Web does not provide that language. Entity-escaped or composite expression mappings are withheld. No embedded semantic LSP or rendered expression colorization is claimed. Spring MVC `#{...}` SpEL in `@RequestParam`, `@RequestHeader` and `@CookieValue` `defaultValue` is injected as `spel` behind this opt-in. JSP EL injection is not implemented.

Matching is text-based. Mapping paths written as constants, concatenations or `${…}` placeholders are not checked. Not implemented: HTTP client execution (built into OpenIDEA), URL completion in other HTTP clients beyond these request files and test calls, typed template-model members and JSP EL model resolution, a versioned template inventory for a stronger unresolved-view check.

</details>

<details>
<summary>Spring WebSocket</summary>

STOMP messaging support for Spring Java and Kotlin projects.

- **Index** of the broker configuration (`enableSimpleBroker`, `enableStompBrokerRelay`, `setApplicationDestinationPrefixes`, `setUserDestinationPrefix`, `addEndpoint` in a `WebSocketMessageBrokerConfigurer`), of handlers (`@MessageMapping`, `@SubscribeMapping`, with the class-level `@MessageMapping` prefix joined) and of destinations sent to (`@SendTo`, `@SendToUser`, `convertAndSend`, `convertAndSendToUser`, `send` of a messaging template).
- **Gutter arrows**: a handler shows the destination a client uses (`SEND /app/chat/send`). A destination sent to links to the other places that use it; Go to Definition on the literal lists them all. User destinations match as the client sees them (`/user/queue/replies`).
- **Completion** in destination literals: broker prefixes and known destinations.
- **Warning** on a broker destination that no configured broker prefix covers. It only appears when the project configures a broker, and never on user destinations.
- **Broker-prefix/XML checks** run through the merged current-document diagnostics.

Matching is text-based, on destination literals as written. `${…}` placeholders and destinations built from variables are skipped. A `convertAndSend`/`send` call counts when its receiver is named like a messaging template (`messagingTemplate`, `simp…`), or when the destination starts with `/` and the receiver isn't a Kafka, Rabbit or JMS template.

- **JavaScript/TypeScript STOMP clients** (`.js`, `.jsx`, `.ts`, `.tsx`, `.vue`, outside `node_modules`, `dist`, `build`): `client.subscribe('/topic/x', …)` links to the matching `@SendTo`/`convertAndSend`/`@SubscribeMapping`. `client.publish({ destination: '/app/x' })` or `client.send('/app/x', …)` links to the `@MessageMapping` handler (application prefix applied). The Java side lists the client usages in its gutter tooltip. A call counts when its receiver name contains `stomp`, or was assigned `new Client(`, `new StompJs.Client(`, `Stomp.over(` or `Stomp.client(` in the same file; `httpClient.send` and the like are ignored. Only plain string literals starting with `/` count; template strings with `${…}` and concatenations are skipped. With several targets, the gutter arrow opens a chooser.
- JS/TS/Vue documents are only read and linked in a project whose root `pom.xml`, `build.gradle` or `build.gradle.kts` mentions `spring-boot-starter-websocket`, `spring-messaging` or `spring-websocket`, or once a Java or Kotlin file of the project has been opened in the session. Elsewhere they are left alone (no scan of the project). A backend in a sub-module with no such root build file is picked up only after one of its Java/Kotlin files is opened.
- **Spring XML** (`http://www.springframework.org/schema/websocket`): required attributes of `stomp-endpoint` (`path`), `mapping` (`path`, `handler`), `simple-broker` and `stomp-broker-relay` (`prefix`) and `handshake-handler` (`ref`). A `handler`/`ref` that names no bean of the project is an Info; beans of libraries are invisible, and it stays quiet until the whole project was read. The tables are hard-coded since no schema is at hand. `path`/`prefix` false positives were removed; enum values and bean refs are checked.

Not implemented: raw `WebSocketHandler` registration checks, SockJS options.

</details>

<details>
<summary>Spring Messaging</summary>

Producer ↔ consumer navigation for Spring Kafka, Spring AMQP (RabbitMQ) and Spring JMS (ActiveMQ/Artemis) in Java and Kotlin projects.

- **Index** of every destination literal:
  - consumers: `@KafkaListener(topics = …)`, `@RabbitListener(queues = …)` (plus `queuesToDeclare = @Queue(…)` and `bindings = @QueueBinding(value = @Queue(…), exchange = @Exchange(…))`, which consume the queue and the exchange), `@JmsListener(destination = …)`;
  - producers: `send` of a Kafka template, `new ProducerRecord<>("topic", …)`, `convertAndSend`/`send`/`convertSendAndReceive` of a Rabbit template (`(routingKey, msg)` goes to the queue of that name through the default exchange, `(exchange, routingKey, msg)` to the exchange), `convertAndSend`/`send` of a JMS template, `@SendTo` on a listener method (`exchange/routingKey` for Rabbit);
  - declarations: `TopicBuilder.name`, `new NewTopic`, `new Queue`, `QueueBuilder.durable/nonDurable`, `new TopicExchange`/`DirectExchange`/`FanoutExchange`/`HeadersExchange`, `ExchangeBuilder.*Exchange`, `new ActiveMQQueue`/`ActiveMQTopic`.
- **Gutter arrows**: a consumer links to the producers and declarations of its destination, a producer to its consumers. Go to Definition on a destination literal lists every other place naming it. A literal `RabbitTemplate` `convertAndSend`/`send` call also links to the exchange's `BindingBuilder` routing key (Info for an unbound key on a complete index). `BindingBuilder` chains link queue, exchange and routing key.
- **Completion** of known destination names (same broker, same kind: topic, queue, exchange or JMS destination) inside those literals.
- **Messaging** section of the Endpoints window: broker → destination → consumers, producers and declarations. A click opens the place; Refresh rescans.

Matching is text-based, by exact name. The broker of a `send`/`convertAndSend` comes from the receiver's name (`kafkaTemplate`, `rabbitTemplate`, `jmsTemplate`…) or, failing that, the declared type of the field or parameter it names. Calls on anything else (`streamBridge`, a messaging template) are left out. SpEL is skipped. Destinations written as a `static final String` (Java) or `const val` (Kotlin) constant, such as `Queues.ORDERS`, are read through the project's string constants: literals and `+` chains in one file, and a bare `NAME` only inside its own class. A constant the project does not define is dropped. There are no warnings for destinations, because a destination may be produced or consumed outside the project.

Placeholders: `${key}` and `${key:default}` in `topics`, `queues`, `destination`, `@SendTo` and in send calls are resolved from the project's `application*`/`bootstrap*` `.properties`, `.yml` and `.yaml` files. Keys match relaxed (`app.topic-name` = `app.topicName`). A default, or a value holding another placeholder, resolves down to two levels, and only for unambiguous property keys. `application.*` wins over profile files (`application-dev.*`), then the rest in path order. The active profile is unknown to plugins, so a value that differs per profile takes the first file's. A placeholder that can't be resolved, or that resolves to a comma-separated list, gets no link. Rabbit `@SendTo("${…}")` is skipped, since the `exchange/routingKey` form can't be told apart.

XML listeners: `<jms:listener destination>` and `<rabbit:listener queue-names>` in Spring XML files link their destination like annotated consumers (bean-name `queues` are not read). `ref` and `method` go to the bean (XML `<bean>`, stereotypes, `@Bean` methods) and its class's methods, and complete.

XML checks use hand-written tables, not read from the XSDs, and cover only these elements. Rabbit: `listener` needs `ref`; `listener-container` `acknowledge` is `auto|manual|none`; the four exchange elements need `name`; `queue`/exchange booleans (`durable`, `exclusive`, `auto-delete`) are `true|false`. JMS: `listener` needs `destination` and `ref`; `listener-container` `acknowledge`, `container-type`, `destination-type` and `cache` take their schema values. A `ref` naming no bean of the project is Info only (library and container beans are invisible). A `method` the project class does not have is a warning, quiet when the class extends or implements anything. Values with `${}`/`#{}` are skipped. Also checked: `true`/`false` values of Rabbit `connection-factory` (`publisher-confirms`, `publisher-returns`), `admin` (`auto-startup`, `ignore-declaration-exceptions`) and `listener-container` (`auto-declare`, `mismatched-queues-fatal`). Other namespace elements (`template`, `binding`, `annotation-driven`, `jca-listener-container`) are not modelled, because their schemas were not at hand to list them completely.

Kotlin sources are indexed too (array literals, `suspend` and expression-bodied functions), except the nested `queuesToDeclare`/`bindings` annotations, which are read in Java only.

SpEL destinations: a whole-value template `"#{'orders'}"`, `"#{'a' + 'b'}"`, `"#{'${app.topic}'}"` (placeholders resolve as usual) and `"#{T(com.acme.Queues).ORDERS}"` (a constant of the project) is read as that name. Anything that needs evaluating (`@bean.method()`, conditions, a constant joined to a literal) is skipped.

Not implemented: broker connection and live topics, Spring Cloud Stream bindings in this section (see Spring Cloud), SpEL beyond the destinations above, `BindingBuilder` routing beyond the Rabbit exchange link, constants held in non-constant expressions or other projects' classes.

</details>

<details>
<summary>Spring for GraphQL</summary>

Links Spring for GraphQL controllers to their schema. The GraphQL language itself (highlighting, the optional language server) is built into the IDE.

- **Handler → schema**: a gutter arrow on each `@QueryMapping`, `@MutationMapping`, `@SubscriptionMapping`, `@SchemaMapping` and `@BatchMapping` method of a `@Controller` opens its schema field. Go to Definition works on the method name and the annotation.
- **Schema → handler**: in `.graphqls`/`.graphql` files, a gutter arrow on each field served by a controller method, and Go to Definition from the field name.
- **Kotlin controllers** (`.kt`) are read like the Java ones, `suspend fun` and expression bodies included. Aliased Spring GraphQL imports work; `@BatchMapping` return types are checked strictly.
- **Client documents**: Go to Definition on a field of a query, mutation or subscription in `.graphql`/`.gql` files under `graphql-documents/` and in `.document("…")` literals (text blocks too, Java and Kotlin) opens the schema field, and for a root field its controller method. Nested fields resolve through the declared field types. Fragments and spreads are only followed for `fragment X on Type`. Inline fragments and literals with escapes are skipped. No problems are reported for unknown fields.
- **Completion** of type names in `typeName = "…"` and of field names in `field`/`name`/`value = "…"`.
- **Checks**: a warning on a handler whose `Type.field` the schema doesn't declare (only when the schema declares that type, or for a root type when the project has a schema), and on an `@Argument` whose name isn't one of the field's arguments. A hint on `Query`/`Mutation`/`Subscription` fields no controller method serves; they may come from `DataFetcher` beans or Querydsl/Query by Example repositories. Literal `RuntimeWiringConfigurer` data fetcher registrations count as handlers. They relax the unmapped-field gate only when every `RuntimeWiring` mention is a recognised registration on a proven `RuntimeWiring.Builder` receiver (configure parameter or lambda parameter), verified against Spring for GraphQL and graphql-java source. An Info hint marks unhandled argument fields on served object types when the handler set is provably complete.
- **Unmapped fields** (Info, setting `springGraphql.reportUnmappedFields`): in a schema file, a field with arguments on an object type that no controller method serves (`@SchemaMapping`, `@BatchMapping`) is reported, because the default property fetcher ignores arguments. Quiet when any source mentions `DataFetcher`, `RuntimeWiring`, `GraphQlSourceBuilderCustomizer` or `DataLoader`, and for root types.
- **GraphQL** section of the Endpoints window: the HTTP path (`spring.graphql.path`, default `/graphql`), the WebSocket path, GraphiQL and the schema printer when enabled, prefixed with `server.servlet.context-path`, and the schema files. Rows from a configuration file open it at the key.

How matching works: every `*.graphqls`/`*.graphql` file outside `node_modules` is read as SDL (`type`, `interface`, `input`, `extend type` and the `schema { query: … }` block; operations are skipped; enums/scalars and union return shapes are recorded), files under `resources/graphql/` first. A shortcut handler's type is its fixed annotation parent (`Query`, `Mutation` or `Subscription`), even when SDL declares a different operation root. Other mapping parents use the method's `typeName`, else the class-level `@SchemaMapping(typeName)`, else the simple type of the source parameter, else the element type of a `@BatchMapping`'s `List<T>`. The source parameter is the first one that isn't an `@Argument`/`@ContextValue`/… or an environment type such as `DataFetchingEnvironment`. The field is the annotation's `field`/`name`/`value`, else the method name. Text-based: types match by simple name, as written.

Not implemented: subscription protocols and transports beyond the optional `graphql-transport-ws` adapter (legacy protocol, SSE and RSocket); checks of client documents (unknown fields, arguments), completion inside them, and documents built from variables or concatenation. Federation directives (`@key`, `@external`…) are skipped, not understood.

</details>

<details>
<summary>Spring Web Flow</summary>

Spring Web Flow 2 and 3 XML flow definitions: files whose root is `<flow>` in the `http://www.springframework.org/schema/webflow` namespace.

- **States**: Go to Definition and completion on `to` of `<transition>`, `then` and `else` of `<if>` in a `<decision-state>`, and `start-state` of `<flow>`. They lead to the state of the same flow, then to a state inherited from a parent flow.
- **Flows**: `subflow` of `<subflow-state>` and `parent` of `<flow>` (comma-separated) open the flow file and complete flow ids. `parent="flowId#stateId"` on a state opens the parent flow or its state, and completes that flow's states. A flow's id is its file name without `.xml`. A flow registry in the project's XML (`webflow-config` namespace) adds ids: `<flow-location id path>` names the file at that path, and `<flow-registry base-path>` names a flow in a subfolder after that folder (`/WEB-INF/flows/booking/booking-flow.xml` is `booking`). Java-config registries (`addFlowLocation`/`setBasePath`) are read too.
- **Checks**: a transition, `then` or `else` naming no state of the flow or its parents is an error. So is a `start-state` naming no state, and a state id used twice. A `subflow` naming no known flow is a warning, once the whole project has been read. A flow without an `<end-state>` gets a note, unless it is abstract or has a parent. Expressions (`${…}`, `#{…}`) are never reported. State checks are skipped while a parent flow can't be found.
- **Web Flow** section of the Endpoints window: the flow of the active editor as a diagram. Each state shows its id and kind, the start state is highlighted, and transitions are arrows labelled with their `on` event. A decision's arrows show the `test` of each `<if>` (cut short, full text on hover) and `else`. It follows the focused editor and the edits to the flow; Refresh redraws it. Global transitions aren't drawn. The graph renders bounded, well-formed flow XML with unique state ids (128 states/256 transitions). Unresolved, global and self edges omitted by the layout remain outside drawn-edge coverage. State clicks open their current source declaration; drawn transition/decision edges open their current target attribute. Forged or unknown ids, missing or changed project ownership and malformed or over-budget sources are refused. These are source links, not a runtime flow, inherited-state or reachability model.
- **Add Web Flow transition…**: a lazy file action (⌥⏎) in an XML editor on a supported view-state. Choose a direct state in the same file, enter a new literal event, then review the exact source replacement. It returns one undoable text edit, expanding a self-closing state when needed. No direct source write, file creation, process, network, build or database action occurs. Namespace prefixes, current unsaved text, UTF-16 ranges and CRLF are preserved. Ownership, full text, document version and project-visible source are checked around every prompt and the final confirmation. Inherited/abstract flows, global transitions, ambiguous ids, dynamic/wildcard or duplicate events, unknown state targets and complex state bodies stay quiet. The action only admits empty view-states or existing empty literal transitions. It keeps the XSD's child ordering (transitions before `on-exit` and `exception-handler`) without moving unknown content. This is a conservative source-local subset, not full XSD/schema/runtime validation, and it does not edit action, subflow, decision or global transitions.

Matching is text-based: ids as written in attributes. Flows registered with `FlowDefinitionRegistryBuilder`, in libraries or in other modules aren't seen. Not covered: view and bean references (`view`, `expression`, `evaluate`), flow input/output mappings and SpEL in expressions.

</details>

<details>
<summary>Spring Cloud</summary>

Spring Cloud OpenFeign, Spring Cloud Stream and Spring Cloud Gateway support for Java and Kotlin projects and `application*`/`bootstrap*` `.properties|yml|yaml` files.

- **Feign client → endpoint**: a gutter arrow on each method of a `@FeignClient` interface whose request matches a `@RestController`/`@Controller` method of the project, and Go to Definition from the method name. A client's request is its `@GetMapping`/`@PostMapping`/`@PutMapping`/`@DeleteMapping`/`@PatchMapping`/`@RequestMapping` (a `@RequestMapping` without `method` counts as GET) or Feign's `@RequestLine("VERB /path")` (the query string is dropped), prefixed by the client's `path`.
- **Endpoint → Feign client**: a gutter arrow on each controller method called by a Feign client method. Spring `@HttpExchange`/`@GetExchange`/`@PostExchange`/`@PutExchange`/`@DeleteExchange`/`@PatchExchange` interfaces are listed as client endpoints with request drafts. Feign `contextId`/`configuration` are shown.
- **Feign check**: a warning on an abstract Feign client method with no request mapping. It is lowered to an info when the `@FeignClient` sets `configuration`, which may bring another contract.
- **Function bean → bindings**: a gutter arrow on each `Function`, `Consumer` or `Supplier` bean (a `@Bean` method returning one, or a `@Component`/`@Service`/… class implementing one) to the binding keys named after it (`process-in-0`, `process-out-0`) under `spring.cloud.stream.bindings.` and `spring.cloud.stream.function.bindings.`, then to its entries in `spring.cloud.function.definition`. A function that is part of a composed definition (`uppercase|process`) also links to the bindings of the composition (`uppercaseprocess-in-0`).
- **Binding → function bean**: Go to Definition on a binding name or on a `spring.cloud.function.definition` entry opens the bean (each part of a composed function).
- **Completion** of bean names in `spring.cloud.function.definition` (after `;` and `|`); of binding names after `spring.cloud.stream.bindings.` (`<bean>-in-0` for a `Consumer`, `<bean>-out-0` for a `Supplier`, both for a `Function` and for composed definitions); and of the predicates and filters shipped with Spring Cloud Gateway in a route's `predicates`/`filters` list items (inserted as `Name=`). Gateway factory completion includes project-defined predicate/filter factories in their own kind. Expanded `.name` properties are completed without shortcut `=` separators and checked against the same kind.
- **Stream checks** (warnings): a `<name>-in-N`/`<name>-out-N` binding whose `<name>` is no function bean, definition entry, composed definition, `spring.cloud.stream.output-bindings`/`input-bindings`/`source` name, `spring.cloud.stream.function.bindings` alias or literal passed first to a `send(…)` call (`StreamBridge`). Also a `spring.cloud.function.definition` entry naming no function bean. These checks run once the project's Java and Kotlin files have been read, and only when the project declares at least one function bean (functions may come from libraries).
- **Unbound channels** (hint): `streamBridge.send("name", …)` with a literal name, or a binder-specific key (`spring.cloud.stream.kafka.bindings.<name>.…`), where no `spring.cloud.stream.bindings`, declared binding, alias or function of the project matches `<name>`. StreamBridge would create the binding on the fly with default settings, so this is most likely a typo. Quiet until the whole project is read, while the project has no explicit bindings, and for `${…}` names. Setting `springCloud.checkStreamBindings`.
- **Bootstrap file** (warning): a `bootstrap*.yml|yaml|properties` file while the build files (`pom.xml`, `build.gradle[.kts]`) state a Spring Boot version of 2.4 or later and none mentions `spring-cloud-starter-bootstrap`, `spring.cloud.bootstrap.enabled=true` or legacy config processing. Since Boot 2.4 such a file is ignored; use `spring.config.import` or add the starter. Quiet when the Boot version is not a literal, when any module has the starter, or when a version below 2.4 is found. Setting `springCloud.checkBootstrap`.
- **Legacy Stream** (`springCloud.checkLegacyStream`): a literal channel of `@StreamListener` or `@SendTo` that no `@Input`/`@Output` method of the project declares (warning). It is quiet unless every `@EnableBinding` type is a project interface or `Sink`/`Source`/`Processor`, and the project is fully read. A `@StreamListener` method that returns a value without `@SendTo`/`@Output`, and `@SendTo` on a void method (info). A `@BootstrapConfiguration` class that is also `@Configuration`/`@Component` (hint: component scanning may load it twice).
- **Stream handler signatures** (`springCloud.checkLegacyStream`, warning): a `@StreamListener` method that names its input twice (on the annotation and on an `@Input` parameter) or not at all; that has both `@SendTo`/`@Output` on the method and an `@Output` parameter; that returns a value while taking an `@Output` parameter; or that sets `condition` with a `Flux`/`Mono`/`Publisher` parameter. Spring Cloud Stream rejects each of these at startup.
- **Feign** section of the Endpoints window: a tree of the project's Feign client methods per file; double-click opens the method. Spring Cloud key value hints come from the Spring Boot Tools plugin's bundled metadata, so there is nothing to add here.
- **Gateway checks**: a route predicate or filter that Spring Cloud Gateway doesn't ship and no class of the project provides (a class named `…RoutePredicateFactory`/`…GatewayFilterFactory`, or extending a `…PredicateFactory`/`…FilterFactory`). It is a warning for `spring.cloud.gateway.routes` and `spring.cloud.gateway.server.webflux.routes`, and an info for the WebMVC lists `spring.cloud.gateway.server.webmvc.routes` and `spring.cloud.gateway.mvc.routes`. These checks run once the project's Java and Kotlin files have been read. Recognised predicates/filters include the shortcut form (`- Path=/users/**`). Argument count and argument names are checked for the factories (verified against spring-cloud-gateway 4.1.5): ignored extra or missing required values, and unknown expanded arguments, get warnings. The Weight expanded keys are `weight.group`/`weight.weight` with group required. The Retry shortcut limit is 9 (jitter and timeout on latest main are not modelled). Pure `.or()` predicate chains in the Java DSL are read.

**Feign navigation by service:** a literal `@FeignClient(name = "users")`, `value = "users"` or `@FeignClient("users")` is compared with the destination module's `spring.application.name`. Controllers of a module that declares a different name are left out of Go to Definition and of the gutter arrows in both directions, even when the method and path match. The name is read from one plain `application.properties`, `application.yml` or `application.yaml` in that module's `src/main/resources` (or its `config/` folder). When a module's name is missing, dynamic, set by a profile or an import, declared twice, or in a multi-document file, nothing is left out; the same holds for clients with a `url`. `contextId` isn't a service name, and active profiles, environment overrides and discovery servers aren't resolved.

Text-based, Java and Kotlin: a Feign method matches an endpoint by HTTP method and path segments (`{…}` matches any segment), narrowed by service when the client names one (below). Paths holding `${…}` are skipped, as are `${…}` binding and definition names. A bean's name is the `@Bean` `name`/`value` or the method name; for a stereotype class it is its `value`, else the class name with a lower-case first letter.

Not covered: expanded Gateway `args` semantics, routes built with the Java DSL, Stream binding properties (`destination`, `group`, binder settings), service discovery, load balancer, circuit breaker and Spring Cloud Config settings.

</details>

<details>
<summary>Hover, rename, OpenAPI export and request generation</summary>

- **Hover**: a path variable bound once within one literal route, with explicitly imported known controller/mapping/PathVariable annotations. Multiple routes, dynamic paths, foreign/shadow annotations and duplicate bindings stay quiet. GraphQL hover describes the current SDL field's named type and arguments, without inventing list/nullability/resolver semantics. Java source hover documents exact imported `@QueryMapping` and `@SchemaMapping` annotations in `@Controller` classes: a literal field or the documented method-name default, Query's composed annotation contract, and an explicitly declared parent type. Missing SchemaMapping type inference is UNRESOLVED. Messaging hover covers literal destinations of exact Spring JMS/Rabbit XML namespaces (not broker inference from Java receiver names) and literal `@KafkaListener(topics=…)`, `@RabbitListener(queues=…)` and `@JmsListener(destination=…)` declarations, including literal arrays. Patterns/partitions/bindings, placeholders, concatenation, foreign annotations and unknown forms stay quiet. Container registration, inherited or composed metadata, installed versions, broker/subscription/delivery state and JMS queue/topic mode remain unresolved. These source-only providers perform no discovery, file reads, requests or processes.
- **Rename route variable and explicit binding**: a lazy file action on a literal method route variable or its explicit `@PathVariable` name. It previews and returns two atomic source-token edits: `{id}` → `{newName}` and the annotation literal `"id"` → `"newName"`. The Java parameter identifier and body stay unchanged. It requires exact Spring imports, one method-level literal route, an explicit unique binding, no class-level route, ordinary closed Java source and a collision-free literal name. Kotlin, implicit parameter-name binding, inherited/class routes, regex/wildcard template variables, constants/concatenation and ambiguous aliases stay quiet. Selection and resolution check full current text, version, path, innermost owner and project-visible source around every prompt; cancellation returns no edits. Unsaved buffers, CRLF and UTF-16 are supported. No source write or process occurs. This changes this route's external variable spelling. It is not Java symbol Rename and does not update clients, other routes or runtime metadata.
- **OpenAPI source export**: **Tools › Copy as OpenAPI** copies a declaration-only OpenAPI 3.0.3 document from a complete, fresh route index (optional `clipboard` permission). The document is capped at 1 MiB. It includes explicit HTTP operations, literal path parameters, all matching handler identities, declared query/header parameters, literal `@ResponseStatus`/`ResponseEntity` codes, literal Bean Validation constraints and regex path-variable patterns, consumes/produces media types and literal mapping conditions. ANY methods, wildcard paths, regex templates and ant-pattern mappings remain visible in `x-openidea-unexported-mappings`. Conflicting handlers keep separate source metadata rather than guessed merged conditions. Runtime conditions and request/response schemas are not inferred. No server is contacted. Positive source DTO shapes accompany the export: Java records, public fields, ordinary JavaBean getters, simple enum constants and public Kotlin data-class properties resolve through exact local package/import identities. Arrays, supported collections/maps and recursive object references keep their written types. The export includes schema components and source provenance. Only declared media types receive content entries; media-unknown body/response shapes stay in source extensions. Custom serialization annotations, inheritance, generic DTO bindings, ambiguous names and Kotlin alias contexts remain unresolved. Validation, requiredness and effective serializer naming/inclusion are not inferred. DTO schemas otherwise remain unspecified.
- **HTTP request lenses**: Java/Kotlin mapping methods offer a source-backed **HTTP request** code lens. Invocation checks the active source and version again. Unrestricted mappings ask for an explicit HTTP method before generating a request or curl command. Named annotations imported from unrelated packages are not treated as Spring mappings. Generation uses source-qualified required query/header examples, literal positive mapping conditions and declared media types. Query text is percent-encoded. Negative or contradictory conditions are shown for review or refused, and wildcard body media requires a concrete choice. Source, project and version are checked again after Base URL selection. Existing request files are preserved. The source HTTP/GraphQL actions also work from Find Action: the active source supplies a native endpoint chooser when a lens payload is absent.
- **GraphQL request lenses**: root GraphQL query/mutation handlers offer schema-qualified HTTP request lenses. Generation requires a fresh, complete local SDL/configuration model. It preserves declared argument list/non-null types, checks shortcut annotation parents against the operation root, supports explicit `@SchemaMapping` custom roots and uses the configured servlet/GraphQL path. It refuses conflicting signatures or profile paths. The generated JSON POST uses editable argument samples and marks null custom-input values for review. Source, schema and configuration are checked again after origin selection. Subscription source lenses prepare a separate editable `.graphql` document from a proved SDL subscription root. Native source actions are shown only for positively matched complete local operation roots.
- **GraphQL subscriptions**: **Start GraphQL Subscription** consumes one current `.graphql`/`.gql` subscription document, including documents prepared by source lenses. It requires Node 22 or newer with built-in WebSocket (optional `process:node`). It then reviews the exact selected ws(s) endpoint, document, variables and connection initialization JSON before connecting. No application or Actuator credentials are reused. Documents are capped at 8 KiB; variables and the initialization payload at 4 KiB each. **Read GraphQL Subscription Output** polls bounded managed output. **Cancel GraphQL Subscription** requests protocol Complete/close, with a bounded process-kill fallback. Sessions last at most 120 seconds and end with plugin disposal. The helper uses `graphql-transport-ws`; it emits no unsupported native HTTP-file WebSocket syntax.

</details>

<details>
<summary>Not implemented</summary>

- Sending requests from the plugin (the IDE's HTTP client owns execution).
- A versioned template inventory for the deferred unresolved-view check; GraphQL's broader non-root `unmapped-field` runtime-fetcher inference.
- Rename anchors and automatic rename behavior; framework resolution, composed annotations, schema/runtime wiring from libraries, full URI/type conversion, runtime routing/authorization.
- JSP EL injection and curl lenses for client calls.
- The source/cache subset here is narrower than full PSI/framework-based models.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
