# Spring Boot Tools

Completion and checks for Spring Boot's `application*.properties|yml|yaml` and `bootstrap*` files in OpenIDEA:

- Key completion: full dotted keys in `.properties`. In YAML, the next segment under the current parent (a property inserts `name: `, a group or map inserts `name:` and a new indented line). Each item shows the property's type, and its documentation gives the description, the default and any deprecation. Map keys hinted by the metadata (`logging.level.root`) are offered too.
- Value completion: `true`/`false` for booleans and the values hinted by the metadata (`logging.level.*` levels, `spring.jpa.hibernate.ddl-auto`…), in both formats.
- Deprecated keys: a warning (struck through) that names the replacement, and a quick fix that renames the key. In YAML the fix is offered only when the replacement keeps the key's written parent (otherwise the key would have to move).
- Unknown keys: a warning when a key isn't a property but its parent is a known group of two segments or more (`spring.datasource.urll`). These are never reported: keys under maps (`logging.level.*`, `spring.jpa.properties.*`), list items (`[0]`) of known properties, unknown groups (third-party starters, Spring Cloud modules not listed below, your own keys) and unknown sub-groups of known groups (`spring.datasource.druid.*`, `management.health.consul.*`). The check runs only once the whole project has been read.
- Cron expressions: Spring's six-field syntax (second minute hour day-of-month month day-of-week), macros (`@daily`…), `-`, `?`, ranges, steps, lists, `L`, `L-n`, `LW`, `nW`, `nL`, `n#k`, month and day names. Checked in `@Scheduled(cron = "…")` literals and in config values whose key ends with `cron` (`app.cleanup.cron`, `app.report-cron`). Placeholders (`${…}`) and SpEL (`#{…}`) are skipped.

- Test constructors: a Spring test class (`@SpringBootTest`, `@WebMvcTest`, `@DataJpaTest`, `@ContextConfiguration`…) whose only constructor takes parameters without `@Autowired` (on the constructor or a parameter, also `@Qualifier`/`@Value`) gets a warning, since Spring autowires them only with `@Autowired`, `@TestConstructor(autowireMode = ALL)` or `spring.test.constructor.autowire.mode=all` in `spring.properties`/a system property; otherwise JUnit fails with a ParameterResolutionException. Quiet until the whole project is read and while any `spring.properties`, `application*`, build file or `gradle.properties` mentions the key. Setting `springBoot.checkTestConstructors`.
- `@EventListener` methods: more than one parameter is an error (Spring throws), a plain `String`/primitive return is a hint (the value is published as a new event). `void`, event objects, collections and async types are not reported. Setting `springBoot.checkEventListeners`.
- Property values: a warning (`invalid-property-value`) when a value cannot convert to the key's metadata type: `Integer`, `Long`, `Short`, `Double`, `Boolean`, `Duration` (`10s`, `PT10S`) or `DataSize` (`10MB`). `${…}`, `#{…}`, lists, maps, enums and custom types are never checked. Setting `springBoot.checkPropertyValues`.
- Navigation: Go to Definition from `spring.config.import` (`classpath:`, `file:`, plain paths; placeholders and other schemes are skipped) to the file, and from a `Class` valued key to the project class. Completion of logger packages under `logging.level` (from the project's `package` lines), profile names (from `application-*` files) in `spring.profiles.active|include`, and keys inside `${…}`.
- Metadata files: `(additional-)spring-configuration-metadata.json` is checked for unknown fields, a malformed `type`, duplicate names and a missing description (hint). Library classes are never judged. A quick fix on an unknown key adds it to an existing `additional-spring-configuration-metadata.json` (the plugin cannot create files). Needs the host to give JSON files to plugins; not verified in the IDE. Setting `springBoot.checkMetadataFiles`.
- Build files: `Hooks.onOperatorDebug()` gets a hint when `reactor-tools` is a dependency (`ReactorDebugAgent.init()`). Opt-in (`springBoot.checkDriverDependency`, off): an info when `spring.datasource.url` names a JDBC driver no build file declares; transitive drivers are invisible, hence off by default.
- File templates: Spring Boot Application, Spring Boot Test and Spring Boot ConfigurationProperties for Java and Kotlin in the New File menu.

The IDE itself provides Go to Definition and Find Usages between keys and Java code, Spring Boot run configurations and HTTP endpoints.

Where keys come from:

- Spring Boot 3.5.16's own metadata (`spring-boot`, `spring-boot-autoconfigure`, `spring-boot-actuator-autoconfigure`) and Spring Cloud 2025.0.3 (Northfields, the train for Boot 3.5; commons, context, loadbalancer, config, Netflix Eureka, gateway, OpenFeign, Stream and its Kafka binders, circuit breaker Resilience4j, bus, Consul, Zookeeper, Kubernetes, Vault, contract, task, function: commons 4.3.2, config 4.3.2, netflix 4.3.2, gateway 4.3.4, openfeign 4.3.2, stream 4.3.2, circuitbreaker 3.3.2, bus 4.3.1, consul 4.3.2, zookeeper 4.3.2, kubernetes 3.3.2, vault 4.3.2, contract 4.3.3, task 3.3.1, function 4.3.2), bundled in `data/boot-properties.tsv` (Apache-2.0, see `data/NOTICE`). Boot's keys win over Cloud's. Regenerate it with `python3 -I scripts/gen-metadata.py 3.5.16 data/boot-properties.tsv 2025.0.3`; the script reads the artifact versions from the train's BOM and reports artifacts without metadata on stderr.
- The project's `META-INF/spring-configuration-metadata.json` and `additional-spring-configuration-metadata.json`. Project search honours `.gitignore`, so in practice these are the files under `src/main/resources`, not the build output.
- `@ConfigurationProperties(prefix|value = "…")` classes and records in the project's Java sources: the prefix plus the dashed name of each field, record component and setter, and one level down into classes of the same file. A prefix on a `@Bean` method, or on a class with a superclass, accepts any key below it, because its properties aren't visible.

The plugin reads source text and Java only. It doesn't read the metadata of libraries (other starters), and offers enum values only when the metadata hints them.

Not ported: datasource/database tool integration, Flyway/Liquibase support, Spring MVC view resolution, the run dashboard's live data (beans, health, environment), SpEL in `@ConditionalOnExpression`, Kotlin configuration classes, repository query preview.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
