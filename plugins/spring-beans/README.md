# Spring Beans Navigator

Spring bean support for Java, Kotlin and Spring XML files in OpenIDEA:

- **Beans**: stereotype classes (`@Service("name")`…), `@Bean` methods (names and aliases), Spring Data repository interfaces, XML `<bean>` definitions and `<alias>`es, with `@Primary`, `@Profile` (and `<beans profile>`) and `@Qualifier`.
- **Injection navigation**: a gutter arrow on each injection point opens the bean Spring would inject, and Go to Definition on the type lists the candidates. Candidates are matched by type through the project's type hierarchy, then narrowed by `@Qualifier`/`@Named`/`@Resource(name)`, `@Primary` and the parameter or field name. `List<T>`, `Map<String, T>` and arrays take every `T`; `Optional<T>`, `ObjectProvider<T>` and `Provider<T>` take a `T`. Covered: constructor parameters of beans, `@Autowired`/`@Inject`/`@Resource` fields and setters, Lombok-constructor `final` fields, `@Bean` parameters, Kotlin primary and secondary constructors and `lateinit` properties, XML `ref`/`parent`/`depends-on`/`factory-bean`.
- **Where a bean is injected**: a gutter icon on each bean lists the injection points that receive it.
- **Bean names in strings**: navigation and completion in `@Qualifier`, `@Named`, `@Resource(name)`, `@DependsOn`, `getBean("…")`, XML references, SpEL `@bean` references; XML `class` attributes navigate to and complete project classes.
- **SpEL**: `#parameter` completion in `@Cacheable`/`@CachePut`/`@CacheEvict` keys and conditions, `@EventListener` conditions and `@PreAuthorize`-style expressions.
- **AspectJ pointcuts** in `@Pointcut`/`@Before`/`@After…`/`@Around`: designator completion, named pointcut completion and navigation (`inApp()`, `Aspects.inApp()`), bean names in `bean(…)`.
- **Spring Beans** tool window, grouped by kind, and a **Spring Bean Graph** of who depends on whom.
- **Field injection** warning on `@Autowired`/`@Inject` fields, with a _Use constructor injection_ quick fix that adds the constructor, or extends the class's only one (not offered with several constructors, `@Autowired` setters or an unparsable body). XML `p:`/`c:` `*-ref` attributes (prefixes bound to the Spring `p`/`c` namespaces) are followed like `ref`; a Kotlin `@Bean fun x() = Foo(…)` is typed `Foo`.
- **Bean inspections** on Java files, run on open, save and typing, each group switchable in the settings: `@Transactional`/`@Async`/`@Cacheable`-style annotations on private, final or static methods, `@Scheduled` methods with parameters, bean names shared by two beans of the project (profiles and `@Conditional*` considered), private or final `@Bean` methods in proxied `@Configuration` classes, bean names starting with a capital, and (off by default) self-invocation of such annotated methods. Text-based: annotations imported from a non-Spring package and computed names are left alone; beans from libraries are not seen.

Endpoints, Spring Boot run configurations and `@Value` key navigation are built into the IDE.

Matching is text-based, by simple type name. Beans from libraries and auto-configuration aren't seen, and conditions (`@ConditionalOn…`) are ignored, so the gutter may list beans that would not be active.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
