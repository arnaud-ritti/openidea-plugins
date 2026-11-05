# Spring Boot Tools

Completion and checks for Spring Boot's `application*.properties|yml|yaml` and `bootstrap*` files in OpenIDEA:

- Key completion: full dotted keys in `.properties`. In YAML, the next segment under the current parent (a property inserts `name: `, a group or map inserts `name:` and a new indented line). Each item shows the property's type, and its documentation gives the description, the default and any deprecation. Map keys hinted by the metadata (`logging.level.root`) are offered too.
- Value completion: `true`/`false` for booleans and the values hinted by the metadata (`logging.level.*` levels, `spring.jpa.hibernate.ddl-auto`…), in both formats.
- Deprecated keys: a warning (struck through) that names the replacement, and a quick fix that renames the key. In YAML the fix is offered only when the replacement keeps the key's written parent (otherwise the key would have to move).
- Unknown keys: a warning when a key isn't a property but its parent is a known group of two segments or more (`spring.datasource.urll`). These are never reported: keys under maps (`logging.level.*`, `spring.jpa.properties.*`), list items (`[0]`) of known properties, unknown groups (third-party starters, `spring.cloud.*`, your own keys) and unknown sub-groups of known groups (`spring.datasource.druid.*`, `management.health.consul.*`). The check runs only once the whole project has been read.
- Cron expressions: Spring's six-field syntax (second minute hour day-of-month month day-of-week), macros (`@daily`…), `-`, `?`, ranges, steps, lists, `L`, `L-n`, `LW`, `nW`, `nL`, `n#k`, month and day names. Checked in `@Scheduled(cron = "…")` literals and in config values whose key ends with `cron` (`app.cleanup.cron`, `app.report-cron`). Placeholders (`${…}`) and SpEL (`#{…}`) are skipped.

The IDE itself provides Go to Definition and Find Usages between keys and Java code, Spring Boot run configurations and HTTP endpoints.

Where keys come from:

- Spring Boot 3.5.16's own metadata (`spring-boot`, `spring-boot-autoconfigure`, `spring-boot-actuator-autoconfigure`), bundled in `data/boot-properties.tsv` (Apache-2.0, see `data/NOTICE`). Regenerate it with `python3 -I scripts/gen-metadata.py <version> data/boot-properties.tsv`.
- The project's `META-INF/spring-configuration-metadata.json` and `additional-spring-configuration-metadata.json`. Project search honours `.gitignore`, so in practice these are the files under `src/main/resources`, not the build output.
- `@ConfigurationProperties(prefix|value = "…")` classes and records in the project's Java sources: the prefix plus the dashed name of each field, record component and setter, and one level down into classes of the same file. A prefix on a `@Bean` method, or on a class with a superclass, accepts any key below it, because its properties aren't visible.

The plugin reads source text and Java only. It doesn't read the metadata of libraries (other starters), and offers enum values only when the metadata hints them.

Not ported: datasource/database tool integration, Flyway/Liquibase support, Spring MVC view resolution, the run dashboard's live data (beans, health, environment), SpEL in `@ConditionalOnExpression`, Kotlin configuration classes, repository query preview.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
