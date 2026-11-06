# Spring Web Tools

Spring MVC helpers for Java controllers in OpenIDEA:

- **Path variables**: in `@Controller`/`@RestController` handler methods, a `@PathVariable` whose name (`value`/`name`, else the parameter name) is missing from one of the method's mapped paths (class `@RequestMapping` prefix included) is an error; when exactly one template variable is left unbound, a quick fix renames the parameter's variable to it. A `{variable}` of a method mapping that no `@PathVariable` takes is a warning, unless a `@PathVariable Map` or a form object (`@ModelAttribute`, or an unannotated parameter of a non-framework type) may bind it. The `{name}`, `{name:regex}` and `{*name}` forms are supported.
- **View names**: in `@Controller` handler methods without `@ResponseBody`, `return "users/list";`, `new ModelAndView("…")` and `setViewName("…")` get a gutter arrow and Go to Definition to the template, and completion of template names. Templates are looked up in `src/main/resources/templates/` (`.html`, `.ftlh`, `.ftl`, `.mustache`, `.peb`, `.jte`, `.kte`), `src/main/webapp/WEB-INF/{views,jsp}/` (`.jsp`) and where `spring.mvc.view.prefix/suffix` or `spring.thymeleaf.prefix/suffix` point. `redirect:` and `forward:` views are skipped, and a Thymeleaf `name :: fragment` resolves to `name`. An unknown view name is not reported, since another view resolver may handle it.
- **Functional routes**: `RouterFunctions.route().GET("/x", handler)` (also `POST`/`PUT`/`DELETE`/`PATCH`/`HEAD`/`OPTIONS`), `route(RequestPredicates.GET("/x"), handler)`, `.andRoute(…)`, and the prefixes of `.path("/api", b -> …)` and `.nest(path("/api"), b -> …)` are read as endpoints, as are the Kotlin `router {}`/`coRouter {}` DSL (`GET("/x")`, `"/api".nest { … }`, `nest(path("/api")) { … }`). A handler written `this::list`, `Handler::list` or Kotlin `::list` gets a gutter arrow and Go to Definition to the project method. Kotlin sources are indexed for this (`**/*.kt`); the other checks stay Java-only. A route under a `nest` whose predicate is not a plain `path("…")` is skipped.
- **Test URLs**: in `src/test/` sources, a URL literal of `mockMvc.perform(get("/users/{id}", 1))` (any HTTP method, also `MockMvcRequestBuilders.get`), `webTestClient.get().uri("/users")` (also `WebClient`/`RestClient`) and `restTemplate.getForObject("/users", …)`/`exchange("/users", HttpMethod.GET, …)` gets a gutter arrow and Go to Definition to the request mappings that serve it, matched by HTTP method and URI template (`{id}` matches any segment; `*`, `**`, `{*rest}` and mixed segments like `by-{name}` work). Every matching mapping is offered; a URL no mapping serves is left alone. Mappings are those with literal paths in `@Controller`/`@RestController` classes and functional routes.

Matching is text-based. Mapping paths written as constants, concatenations or `${…}` placeholders are not checked.

Not ported: Endpoints tool window and HTTP client (built into OpenIDEA), URL completion in tests and HTTP clients, Kotlin controllers, model variable resolution in JSP/Thymeleaf templates, static resource references.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
