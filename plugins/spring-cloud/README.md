# Spring Cloud Tools

Spring Cloud OpenFeign, Spring Cloud Stream and Spring Cloud Gateway support for Java projects and `application*`/`bootstrap*` `.properties|yml|yaml` files in OpenIDEA:

- **Feign client → endpoint**: a gutter arrow on each method of a `@FeignClient` interface whose request matches a `@RestController`/`@Controller` method of the project, and Go to Definition from the method name. A client's request is its `@GetMapping`/`@PostMapping`/`@PutMapping`/`@DeleteMapping`/`@PatchMapping`/`@RequestMapping` (a `@RequestMapping` without `method` counts as GET) or Feign's `@RequestLine("VERB /path")` (the query string is dropped), prefixed by the client's `path`.
- **Endpoint → Feign client**: a gutter arrow on each controller method called by a Feign client method.
- **Feign check**: a warning on an abstract Feign client method with no request mapping, lowered to an info when the `@FeignClient` sets `configuration` (which may bring another contract).
- **Function bean → bindings**: a gutter arrow on each `Function`, `Consumer` or `Supplier` bean (a `@Bean` method returning one, or a `@Component`/`@Service`/… class implementing one) to the binding keys named after it (`process-in-0`, `process-out-0`) under `spring.cloud.stream.bindings.` and `spring.cloud.stream.function.bindings.`, then to its entries in `spring.cloud.function.definition`. A function that is part of a composed definition (`uppercase|process`) also links to the bindings of the composition (`uppercaseprocess-in-0`).
- **Binding → function bean**: Go to Definition on a binding name or on a `spring.cloud.function.definition` entry opens the bean (each part of a composed function).
- **Completion** of bean names in `spring.cloud.function.definition` (after `;` and `|`), of binding names after `spring.cloud.stream.bindings.` (`<bean>-in-0` for a `Consumer`, `<bean>-out-0` for a `Supplier`, both for a `Function` and for composed definitions), and of the predicates and filters shipped with Spring Cloud Gateway in a route's `predicates`/`filters` list items (inserted as `Name=`).
- **Stream checks** (warnings): a `<name>-in-N`/`<name>-out-N` binding whose `<name>` is no function bean, definition entry, composed definition, `spring.cloud.stream.output-bindings`/`input-bindings`/`source` name, `spring.cloud.stream.function.bindings` alias or literal passed first to a `send(…)` call (`StreamBridge`); a `spring.cloud.function.definition` entry naming no function bean. These checks run once the project's Java and Kotlin files have been read, and only when the project declares at least one function bean (functions may come from libraries).
- **Gateway checks**: a route predicate or filter that Spring Cloud Gateway doesn't ship and no class of the project provides (a class named `…RoutePredicateFactory`/`…GatewayFilterFactory`, or extending a `…PredicateFactory`/`…FilterFactory`). It is a warning for `spring.cloud.gateway.routes` and `spring.cloud.gateway.server.webflux.routes`, and an info for the WebMVC lists `spring.cloud.gateway.server.webmvc.routes` and `spring.cloud.gateway.mvc.routes`. These checks run once the project's Java and Kotlin files have been read.

Text-based, Java only: a Feign method matches an endpoint by HTTP method and path segments (`{…}` matches any segment), across every service of the project since the client's `name` isn't compared; paths holding `${…}` are skipped, as are `${…}` binding and definition names. A bean's name is the `@Bean` `name`/`value` or the method name, or for a stereotype class its `value`, else the class name with a lower-case first letter. Gateway predicates and filters are recognised only in the shortcut form (`- Path=/users/**`).

Not covered: the expanded `name:`/`args:` form of Gateway predicates and filters, routes built with the Java DSL, completion of the project's own Gateway factories, Stream binding properties (`destination`, `group`, binder settings), service discovery, load balancer, circuit breaker and Spring Cloud Config settings.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
