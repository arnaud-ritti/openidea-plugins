# Spring Endpoints Tools (aritti.spring-web)

One **Spring Endpoints** tool window (right) with six sections, plus the editor support of six former plugins: Spring Web (this one), Spring WebSocket, Spring Messaging, Spring for GraphQL, Spring Web Flow and Spring Cloud. `aritti.spring-websocket`, `aritti.spring-messaging`, `aritti.spring-graphql`, `aritti.spring-webflow` and `aritti.spring-cloud` are no longer maintained: uninstall them, otherwise their windows and completions show twice.

- **HTTP**: the route list of the Spring Web section below, with its controller filter, Flat/Tree toggle and row actions.
- **WebSocket**, **Messaging**, **GraphQL**: the STOMP destinations, the Kafka/RabbitMQ/JMS destinations and the GraphQL endpoints and schema files.
- **Web Flow**: the diagram of the flow in the active editor.
- **Feign**: the project's Feign client methods per file.

A module whose dependency is not in the root build file (multi-module builds and version catalogs count as unsure) does no work. Settings keep their keys (`springWeb.*`, `springWebsocket.*`, `springGraphql.*`, `springCloud.*`); the old plugins' values are not migrated.

## Spring Web

Spring MVC helpers for Java controllers in OpenIDEA:

- **Endpoints list** (HTTP section): a flat list: a method icon (GET, POST, PUT, DELETE, PATCH, other), the path, and grey text with `[METHOD] Controller` after it. *Filter* narrows the list to one controller (the status line says which), *Clear* restores it, and *Flat/Tree* groups the rows by controller. Row actions: *Copy as curl* and *Create HTTP Request File* (writes `http/<verb>-<path>.http`, needs `process:tee`; without it the request is copied). Limits: the grey text follows the path rather than sitting in a column, there is no request panel beside the list, and no gutter icon on the handler; these wait on upstream OpenIDEA requests.

- **Path variables**: in `@Controller`/`@RestController` handler methods, a `@PathVariable` whose name (`value`/`name`, else the parameter name) is missing from one of the method's mapped paths (class `@RequestMapping` prefix included) is an error; when exactly one template variable is left unbound, a quick fix renames the parameter's variable to it. A `{variable}` of a method mapping that no `@PathVariable` takes is a warning, unless a `@PathVariable Map` or a form object (`@ModelAttribute`, or an unannotated parameter of a non-framework type) may bind it. The `{name}`, `{name:regex}` and `{*name}` forms are supported.
- **API versions**: a request mapping that sets `version` (Spring Framework 7) is a warning when the project configures no API versioning. Quiet unless every source and `application*` file was read, at least one `application*` file exists, and none mentions `apiversion` (`spring.mvc.apiversion.*`) or `ApiVersion…`/`configureApiVersioning`. Setting `springWeb.checkApiVersioning`.
- **@InitBinder**: an `@InitBinder` method that returns a value (error, Spring throws) or takes no `WebDataBinder`/`DataBinder` parameter (warning). Setting `springWeb.checkInitBinder`.
- **View names**: in `@Controller` handler methods without `@ResponseBody`, `return "users/list";`, `new ModelAndView("…")` and `setViewName("…")` get a gutter arrow and Go to Definition to the template, and completion of template names. Templates are looked up in `src/main/resources/templates/` (`.html`, `.ftlh`, `.ftl`, `.mustache`, `.peb`, `.jte`, `.kte`), `src/main/webapp/WEB-INF/{views,jsp}/` (`.jsp`) and where `spring.mvc.view.prefix/suffix` or `spring.thymeleaf.prefix/suffix` point. `redirect:` and `forward:` views are skipped, and a Thymeleaf `name :: fragment` resolves to `name`. An unknown view name is not reported, since another view resolver may handle it.
- **Functional routes**: `RouterFunctions.route().GET("/x", handler)` (also `POST`/`PUT`/`DELETE`/`PATCH`/`HEAD`/`OPTIONS`), `route(RequestPredicates.GET("/x"), handler)`, `.andRoute(…)`, and the prefixes of `.path("/api", b -> …)` and `.nest(path("/api"), b -> …)` are read as endpoints, as are the Kotlin `router {}`/`coRouter {}` DSL (`GET("/x")`, `"/api".nest { … }`, `nest(path("/api")) { … }`). A handler written `this::list`, `Handler::list` or Kotlin `::list` gets a gutter arrow and Go to Definition to the project method. Kotlin sources are indexed for this (`**/*.kt`); the other checks stay Java-only. A route under a `nest` whose predicate is not a plain `path("…")` (including Kotlin `("/a" and accept(…)).nest { }`) is skipped. Kotlin `"/a".nest { }`, `"/a" nest { }` and `path("/a").nest { }` are read.
- **Test URLs**: in `src/test/` sources, a URL literal of `mockMvc.perform(get("/users/{id}", 1))` (any HTTP method, also `MockMvcRequestBuilders.get`), `webTestClient.get().uri("/users")` (also `WebClient`/`RestClient`) and `restTemplate.getForObject("/users", …)`/`exchange("/users", HttpMethod.GET, …)` gets a gutter arrow and Go to Definition to the request mappings that serve it, matched by HTTP method and URI template (`{id}` matches any segment; `*`, `**`, `{*rest}` and mixed segments like `by-{name}` work). Every matching mapping is offered; `{id:regex}` constraints are ignored when matching, so more mappings than Spring would pick may be offered; absolute URLs (`http://…`) are skipped; a URL no mapping serves is left alone. The RestTemplate calls need a receiver named like `restTemplate`/`template`. Mappings are those with literal paths in `@Controller`/`@RestController` classes and functional routes.

- **URL markers**: every request mapping and functional route shows its URL (`URL: GET /users/{id}`) in the gutter, unless a link marker already uses the line. Setting `springWeb.showUrlMarkers`. The marker has no target.
- **URL completion in tests**: inside the URL literal of `mockMvc.perform(get("…"))`, `webTestClient.get().uri("…")` and `restTemplate.getForObject("…")`, the project's mapping paths for that HTTP method are offered. Unverified in the IDE: completion inside string literals.
- **Static resources**: Go to Definition on a string literal such as `"/css/app.css"` or `"classpath:/static/js/a.js"` opens the file under a `static/`, `public/` or `META-INF/resources/` folder of the project (`css`, `js`, images, fonts, `html`).
- **Snippets** (Java and Kotlin, with tab stops): in a controller class body `getm`, `postm`, `putm`, `delm`, `patm`, `reqm` (mapping methods), `exh` (exception handler) and `initb` (init binder); in any class body `router` (functional `RouterFunction` bean, Kotlin `router {}`); outside every type `restc`, `ctrl` and `advice` (controller classes). Snippets add no imports.
- **File templates**: New menu entries Spring Controller, REST Controller and Controller Advice, for Java and Kotlin (`$PACKAGE$`, `$NAME$` and `$END$`; the host drops the `package` line when there is no package).
- **Kotlin controllers**: the path variable, API version and `@InitBinder` checks run on Kotlin controllers, and the Add `@RequestMapping`/`@ExceptionHandler` method actions generate Kotlin (`fun handle(): String`, no semicolons in the import).
- **HTTP request files**: on an endpoint in the Endpoints view, Create HTTP Request File for Endpoint asks for the base URL and writes `http/<verb>-<path>.http` under the project root (`### GET /users` plus the request line) and opens it; an existing file is opened untouched. Needs the optional `process:tee` capability; without it the request goes to the clipboard (optional `clipboard`). Sending a request stays with the IDE's HTTP client. Unverified in the IDE.
- **Unresolved views** (`springWeb.reportUnresolvedViews`, default on): a Hint on a returned view name that no template of the known folders answers, once the project has at least one template. Quiet until every source was read, and for good when any source mentions `ViewResolver`, `ViewResolverRegistry`, `configureViewResolvers`, `TilesConfigurer` or `AbstractView`, since another resolver may serve the name.
- **`<mvc:…>` XML**: required attributes of `view-controller`, `redirect-view-controller`, `status-controller`, `resources`, `mapping`, `exclude-mapping` and `resource-chain`, and `true`/`false` values of `annotation-driven` (`ignore-default-model-on-redirect`), `message-converters` (`register-defaults`), `redirect-view-controller` (`context-relative`, `keep-query-params`) and `resource-chain` (`resource-cache`), from a hard-coded table of elements listed completely; the other elements of the namespace are not checked. Unverified in the IDE: the `xml` provider activation.
- **API version in tests**: a Hint on `apiVersion(...)` in a test when the project mentions neither `ApiVersionInserter` nor `spring.mvc.apiversion.*` in any file.

Matching is text-based. Mapping paths written as constants, concatenations or `${…}` placeholders are not checked.

Not ported: HTTP client (built into OpenIDEA), URL completion in HTTP clients other than the test calls above, model variable resolution in JSP/Thymeleaf templates; static resource references inside templates (Java string literals are covered, see above).

## Spring WebSocket

STOMP messaging support for Spring Java and Kotlin projects in OpenIDEA:

- **Index** of the project's broker configuration (`enableSimpleBroker`, `enableStompBrokerRelay`, `setApplicationDestinationPrefixes`, `setUserDestinationPrefix`, `addEndpoint` in a `WebSocketMessageBrokerConfigurer`), handlers (`@MessageMapping`, `@SubscribeMapping`, class-level `@MessageMapping` prefix joined) and destinations sent to (`@SendTo`, `@SendToUser`, `convertAndSend`, `convertAndSendToUser`, `send` of a messaging template).
- **Gutter arrows**: a handler shows the destination a client uses (`SEND /app/chat/send`). A destination sent to links to the other places that use it, and Go to Definition on the literal lists them all. User destinations match as the client sees them (`/user/queue/replies`).
- **Completion** in destination literals: broker prefixes and known destinations.
- **Warning** on a broker destination that no configured broker prefix covers. It only appears when the project configures a broker, and never on user destinations.

Matching is text-based, on destination literals as written; `${…}` placeholders and destinations built from variables are skipped. A `convertAndSend`/`send` call counts when its receiver is named like a messaging template (`messagingTemplate`, `simp…`), or when the destination starts with `/` and the receiver isn't a Kafka, Rabbit or JMS template.

- **JavaScript/TypeScript STOMP clients** (`.js`, `.jsx`, `.ts`, `.tsx`, `.vue`, outside `node_modules`, `dist`, `build`): `client.subscribe('/topic/x', …)` links to the matching `@SendTo`/`convertAndSend`/`@SubscribeMapping`, and `client.publish({ destination: '/app/x' })` or `client.send('/app/x', …)` links to the `@MessageMapping` handler (application prefix applied). The Java side lists the client usages in its gutter tooltip. A call counts when its receiver name contains `stomp`, or was assigned `new Client(`, `new StompJs.Client(`, `Stomp.over(` or `Stomp.client(` in the same file (`httpClient.send` and the like are ignored). Only plain string literals starting with `/` count; template strings with `${…}` and concatenations are skipped. One target opens per marker; the tooltip lists the rest.

  JS/TS/Vue documents are only read and linked in a project whose root `pom.xml`, `build.gradle` or `build.gradle.kts` mentions `spring-boot-starter-websocket`, `spring-messaging` or `spring-websocket`, or once a Java or Kotlin file of the project has been opened in the session; elsewhere they are left alone (no scan of the project). A backend in a sub-module with no such root build file is picked up only after one of its Java/Kotlin files is opened.

- **Spring XML** (`http://www.springframework.org/schema/websocket`): required attributes of `stomp-endpoint` (`path`), `mapping` (`path`, `handler`), `simple-broker` and `stomp-broker-relay` (`prefix`) and `handshake-handler` (`ref`); a `handler`/`ref` that names no bean of the project is an Info (beans of libraries are invisible; quiet until the whole project was read). The tables are hard-coded since no schema is at hand. Unverified in the IDE: the `xml` provider activation.

Not ported: raw `WebSocketHandler` registration checks, SockJS options.

## Spring Messaging

Producer ↔ consumer navigation for Spring Kafka, Spring AMQP (RabbitMQ) and Spring JMS (ActiveMQ/Artemis) in Java and Kotlin projects in OpenIDEA:

- **Index** of every destination literal:
  - consumers: `@KafkaListener(topics = …)`, `@RabbitListener(queues = …)` (plus `queuesToDeclare = @Queue(…)` and `bindings = @QueueBinding(value = @Queue(…), exchange = @Exchange(…))`, which consume the queue and the exchange), `@JmsListener(destination = …)`;
  - producers: `send` of a Kafka template, `new ProducerRecord<>("topic", …)`, `convertAndSend`/`send`/`convertSendAndReceive` of a Rabbit template (`(routingKey, msg)` goes to the queue of that name through the default exchange, `(exchange, routingKey, msg)` to the exchange), `convertAndSend`/`send` of a JMS template, `@SendTo` on a listener method (`exchange/routingKey` for Rabbit);
  - declarations: `TopicBuilder.name`, `new NewTopic`, `new Queue`, `QueueBuilder.durable/nonDurable`, `new TopicExchange`/`DirectExchange`/`FanoutExchange`/`HeadersExchange`, `ExchangeBuilder.*Exchange`, `new ActiveMQQueue`/`ActiveMQTopic`.
- **Gutter arrows**: a consumer links to the producers and declarations of its destination, a producer to its consumers. Go to Definition on a destination literal lists every other place naming it.
- **Completion** of known destination names (same broker, same kind: topic, queue, exchange or JMS destination) inside those literals.
- **Messaging** section of the Endpoints window: broker → destination → consumers, producers and declarations; a click opens the place, Refresh rescans.

Matching is text-based, by exact name. The broker of a `send`/`convertAndSend` comes from the receiver's name (`kafkaTemplate`, `rabbitTemplate`, `jmsTemplate`…) or, failing that, the declared type of the field or parameter it names; calls on anything else (`streamBridge`, a messaging template) are left out. SpEL is skipped. Destinations written as a `static final String` (Java) or `const val` (Kotlin) constant, `Queues.ORDERS`, are read through the string constants of the project (literals and `+` chains in one file; a bare `NAME` only inside its own class); a constant the project does not define is dropped. There are no warnings for destinations, because a destination may be produced or consumed outside the project.

Placeholders: `${key}` and `${key:default}` in `topics`, `queues`, `destination`, `@SendTo` and in send calls are resolved from `application*`/`bootstrap*` `.properties`, `.yml` and `.yaml` files of the project (keys match relaxed: `app.topic-name` = `app.topicName`), a default or a value holding another placeholder down to two levels. `application.*` wins over profile files (`application-dev.*`), then the rest in path order; the active profile is unknown to plugins, so a value that differs per profile takes the first file's. A placeholder that can't be resolved, or that resolves to a comma-separated list, gets no link. Rabbit `@SendTo("${…}")` is skipped (the `exchange/routingKey` form can't be told apart).

XML listeners: `<jms:listener destination>` and `<rabbit:listener queue-names>` (bean-name `queues` are not read) in Spring XML files link their destination like annotated consumers; `ref` and `method` go to the bean (XML `<bean>`, stereotypes, `@Bean` methods) and its class's methods, and complete. Spring Kafka has no XML listener.

XML checks (hand-written tables, not read from the XSDs; only these elements): Rabbit `listener` needs `ref`, `listener-container` `acknowledge` is `auto|manual|none`, the four exchange elements need `name`, `queue`/exchange booleans (`durable`, `exclusive`, `auto-delete`) are `true|false`; JMS `listener` needs `destination` and `ref`, `listener-container` `acknowledge`, `container-type`, `destination-type` and `cache` take their schema values. A `ref` naming no bean of the project is Info only (library and container beans are invisible); a `method` the project class does not have is a warning, quiet when the class extends or implements anything. Values with `${}`/`#{}` are skipped.

Kotlin sources are indexed too (array literals, `suspend` and expression-bodied functions), except the nested `queuesToDeclare`/`bindings` annotations, which are read in Java only.

SpEL destinations: a whole-value template `"#{'orders'}"`, `"#{'a' + 'b'}"`, `"#{'${app.topic}'}"` (placeholders resolve as usual) and `"#{T(com.acme.Queues).ORDERS}"` (a constant of the project) is read as that name. Anything that needs evaluating (`@bean.method()`, conditions, a constant joined to a literal) is skipped.

More XML checks: `true`/`false` values of Rabbit `connection-factory` (`publisher-confirms`, `publisher-returns`), `admin` (`auto-startup`, `ignore-declaration-exceptions`) and `listener-container` (`auto-declare`, `mismatched-queues-fatal`). Other namespace elements (`template`, `binding`, `annotation-driven`, `jca-listener-container`) are not modelled: their schemas were not at hand to list them completely.

Not ported: broker connection and live topics, Spring Cloud Stream bindings (a separate plugin), SpEL beyond the destinations above, `BindingBuilder` routing, constants held in non-constant expressions or other projects' classes.

## Spring for GraphQL

Links Spring for GraphQL controllers to their schema in OpenIDEA. The GraphQL language itself (highlighting, the optional language server) is built into the IDE.

- **Handler → schema**: a gutter arrow on each `@QueryMapping`, `@MutationMapping`, `@SubscriptionMapping`, `@SchemaMapping` and `@BatchMapping` method of a `@Controller` opens its schema field; Go to Definition works on the method name and the annotation.
- **Schema → handler**: in `.graphqls`/`.graphql` files, a gutter arrow on each field served by a controller method, and Go to Definition from the field name.
- **Kotlin controllers** (`.kt`) are read like the Java ones, `suspend fun` and expression bodies included.
- **Client documents**: Go to Definition on a field of a query, mutation or subscription in `.graphql`/`.gql` files under `graphql-documents/` and in `.document("…")` literals (text blocks too, Java and Kotlin) opens the schema field, and for a root field its controller method. Nested fields resolve through the declared field types. Fragments and spreads are only followed for `fragment X on Type`; inline fragments, and literals with escapes, are skipped. No problems are reported for unknown fields.
- **Completion** of type names in `typeName = "…"` and of field names in `field`/`name`/`value = "…"`.
- **Checks**: a warning on a handler whose `Type.field` the schema doesn't declare (only when the schema declares that type, or for a root type when the project has a schema), and on an `@Argument` whose name isn't one of the field's arguments; a hint on `Query`/`Mutation`/`Subscription` fields no controller method serves (they may come from `DataFetcher` beans or Querydsl/Query by Example repositories).

How matching works: every `*.graphqls`/`*.graphql` file outside `node_modules` is read as SDL (`type`, `interface`, `input`, `extend type` and the `schema { query: … }` block; operations, enums, unions and scalars are skipped), files under `resources/graphql/` first. A handler's type is the root type of its annotation, else `typeName`, else the class-level `@SchemaMapping(typeName)`, else the simple type of the source parameter (the first one that isn't an `@Argument`/`@ContextValue`/… or an environment type such as `DataFetchingEnvironment`), or the element type of a `@BatchMapping`'s `List<T>`. Its field is the annotation's `field`/`name`/`value`, else the method name. Text-based: types match by simple name, as written.

## Not ported

- HTTP client request generation for GraphQL.
- Checks of client documents (unknown fields, arguments), completion inside them, and documents built from variables or concatenation.
- Federation directives (`@key`, `@external`…) are skipped, not understood.

- **GraphQL** section of the Endpoints window, with the HTTP path (`spring.graphql.path`, default `/graphql`), the WebSocket path, GraphiQL and the schema printer when enabled, prefixed with `server.servlet.context-path`, and the schema files. Rows from a configuration file open it at the key.
- **Unmapped fields** (Info, setting `springGraphql.reportUnmappedFields`): in a schema file, a field with arguments on an object type that no controller method serves (`@SchemaMapping`, `@BatchMapping`) is reported, because the default property fetcher ignores arguments. Quiet when any source mentions `DataFetcher`, `RuntimeWiring`, `GraphQlSourceBuilderCustomizer` or `DataLoader`, and for root types.

## Spring Web Flow

Spring Web Flow 2 and 3 XML flow definitions in OpenIDEA (files whose root is `<flow>` in the `http://www.springframework.org/schema/webflow` namespace):

- **States**: Go to Definition and completion on `to` of `<transition>`, `then` and `else` of `<if>` in a `<decision-state>`, and `start-state` of `<flow>`. They lead to the state of the same flow, then to a state inherited from a parent flow.
- **Flows**: `subflow` of `<subflow-state>` and `parent` of `<flow>` (comma-separated) open the flow file and complete flow ids; `parent="flowId#stateId"` on a state opens the parent flow or its state, and completes that flow's states. A flow's id is its file name without `.xml`. A flow registry in the project's XML (`webflow-config` namespace) adds ids: `<flow-location id path>` names the file at that path, and `<flow-registry base-path>` names a flow in a subfolder after that folder (`/WEB-INF/flows/booking/booking-flow.xml` is `booking`).
- **Checks**: a transition, `then` or `else` naming no state of the flow or its parents is an error, so is a `start-state` naming no state and a state id used twice. A `subflow` naming no known flow is a warning, once the whole project has been read. A flow without an `<end-state>` gets a note, unless it is abstract or has a parent. Expressions (`${…}`, `#{…}`) are never reported, and state checks are skipped while a parent flow can't be found.
- **Web Flow** section of the Endpoints window: the flow of the active editor as a diagram. Each state shows its id and kind, the start state is highlighted, and transitions are arrows labelled with their `on` event. A decision's arrows show the `test` of each `<if>` (cut short, full text on hover) and `else`. It follows the focused editor and the edits to the flow; Refresh redraws it. Global transitions aren't drawn.

Matching is text-based: ids as written in attributes. Flows registered in Java configuration (`FlowDefinitionRegistryBuilder`), in libraries or in other modules aren't seen.

Not covered: Java flow registry configuration, view and bean references (`view`, `expression`, `evaluate`), flow input/output mappings, SpEL in expressions, and clicking diagram nodes (the desktop IDE blocks page scripts).

## Spring Cloud

Spring Cloud OpenFeign, Spring Cloud Stream and Spring Cloud Gateway support for Java and Kotlin projects and `application*`/`bootstrap*` `.properties|yml|yaml` files in OpenIDEA:

- **Feign client → endpoint**: a gutter arrow on each method of a `@FeignClient` interface whose request matches a `@RestController`/`@Controller` method of the project, and Go to Definition from the method name. A client's request is its `@GetMapping`/`@PostMapping`/`@PutMapping`/`@DeleteMapping`/`@PatchMapping`/`@RequestMapping` (a `@RequestMapping` without `method` counts as GET) or Feign's `@RequestLine("VERB /path")` (the query string is dropped), prefixed by the client's `path`.
- **Endpoint → Feign client**: a gutter arrow on each controller method called by a Feign client method.
- **Feign check**: a warning on an abstract Feign client method with no request mapping, lowered to an info when the `@FeignClient` sets `configuration` (which may bring another contract).
- **Function bean → bindings**: a gutter arrow on each `Function`, `Consumer` or `Supplier` bean (a `@Bean` method returning one, or a `@Component`/`@Service`/… class implementing one) to the binding keys named after it (`process-in-0`, `process-out-0`) under `spring.cloud.stream.bindings.` and `spring.cloud.stream.function.bindings.`, then to its entries in `spring.cloud.function.definition`. A function that is part of a composed definition (`uppercase|process`) also links to the bindings of the composition (`uppercaseprocess-in-0`).
- **Binding → function bean**: Go to Definition on a binding name or on a `spring.cloud.function.definition` entry opens the bean (each part of a composed function).
- **Completion** of bean names in `spring.cloud.function.definition` (after `;` and `|`), of binding names after `spring.cloud.stream.bindings.` (`<bean>-in-0` for a `Consumer`, `<bean>-out-0` for a `Supplier`, both for a `Function` and for composed definitions), and of the predicates and filters shipped with Spring Cloud Gateway in a route's `predicates`/`filters` list items (inserted as `Name=`).
- **Stream checks** (warnings): a `<name>-in-N`/`<name>-out-N` binding whose `<name>` is no function bean, definition entry, composed definition, `spring.cloud.stream.output-bindings`/`input-bindings`/`source` name, `spring.cloud.stream.function.bindings` alias or literal passed first to a `send(…)` call (`StreamBridge`); a `spring.cloud.function.definition` entry naming no function bean. These checks run once the project's Java and Kotlin files have been read, and only when the project declares at least one function bean (functions may come from libraries).
- **Unbound channels** (hint): `streamBridge.send("name", …)` with a literal name, or a binder-specific key (`spring.cloud.stream.kafka.bindings.<name>.…`), where no `spring.cloud.stream.bindings`, declared binding, alias or function of the project matches `<name>`. StreamBridge would create the binding on the fly with default settings, so this is most likely a typo. Quiet until the whole project is read, while the project has no explicit bindings, and for `${…}` names. Setting `springCloud.checkStreamBindings`.
- **Bootstrap file** (warning): a `bootstrap*.yml|yaml|properties` file while the build files (`pom.xml`, `build.gradle[.kts]`) state a Spring Boot version of 2.4 or later and none mentions `spring-cloud-starter-bootstrap`, `spring.cloud.bootstrap.enabled=true` or legacy config processing. Since Boot 2.4 such a file is ignored; use `spring.config.import` or add the starter. Quiet when the Boot version is not a literal, any module has the starter or a version below 2.4 is found. Setting `springCloud.checkBootstrap`.
- **Legacy Stream** (`springCloud.checkLegacyStream`): a literal channel of `@StreamListener` or `@SendTo` that no `@Input`/`@Output` method of the project declares (warning; quiet unless every `@EnableBinding` type is a project interface or `Sink`/`Source`/`Processor`, and the project is fully read); a `@StreamListener` method that returns a value without `@SendTo`/`@Output`, and `@SendTo` on a void method (info); a `@BootstrapConfiguration` class that is also `@Configuration`/`@Component` (hint: component scanning may load it twice).
- **Stream handler signatures** (`springCloud.checkLegacyStream`, warning): a `@StreamListener` method that names its input twice (on the annotation and on an `@Input` parameter) or not at all, that has both `@SendTo`/`@Output` on the method and an `@Output` parameter, that returns a value while taking an `@Output` parameter, or that sets `condition` with a `Flux`/`Mono`/`Publisher` parameter. Spring Cloud Stream rejects each of these at startup.
- **Feign** section of the Endpoints window: a tree of the project's Feign client methods per file; double-click opens the method. Spring Cloud key value hints come from the Spring Boot Tools plugin's bundled metadata, so there is nothing to add here.
- **Gateway checks**: a route predicate or filter that Spring Cloud Gateway doesn't ship and no class of the project provides (a class named `…RoutePredicateFactory`/`…GatewayFilterFactory`, or extending a `…PredicateFactory`/`…FilterFactory`). It is a warning for `spring.cloud.gateway.routes` and `spring.cloud.gateway.server.webflux.routes`, and an info for the WebMVC lists `spring.cloud.gateway.server.webmvc.routes` and `spring.cloud.gateway.mvc.routes`. These checks run once the project's Java and Kotlin files have been read.

Text-based, Java and Kotlin: a Feign method matches an endpoint by HTTP method and path segments (`{…}` matches any segment), across every service of the project since the client's `name` isn't compared; paths holding `${…}` are skipped, as are `${…}` binding and definition names. A bean's name is the `@Bean` `name`/`value` or the method name, or for a stereotype class its `value`, else the class name with a lower-case first letter. Gateway predicates and filters are recognised only in the shortcut form (`- Path=/users/**`).

Not covered: the expanded `name:`/`args:` form of Gateway predicates and filters, routes built with the Java DSL, completion of the project's own Gateway factories, Stream binding properties (`destination`, `group`, binder settings), service discovery, load balancer, circuit breaker and Spring Cloud Config settings.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
