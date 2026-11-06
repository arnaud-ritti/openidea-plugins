# Spring Runtime Insight

Reads the Actuator endpoints of a Spring Boot application running on this machine and shows what it loaded, in a Spring Runtime tool window and as editor markers.

## Setup

The application needs Spring Boot Actuator with these endpoints exposed over HTTP:

```properties
management.endpoints.web.exposure.include=beans,env,conditions,health,mappings,configprops,scheduledtasks,loggers,caches
```

Settings › Plugins › Spring Runtime Insight:

| Key | Default | |
|---|---|---|
| `actuatorUrl` | `http://localhost:8080/actuator` | Actuator base URL; host must be `localhost` or `127.0.0.1` (`http://` or `https://`). Change the port or path when the application uses another `server.port`, `management.server.port` or `management.endpoints.web.base-path`. |
| `actuatorAuth` | `none` | `none` sends no credentials. `bearer` sends `Authorization: Bearer <token>`, `basic` sends `Authorization: Basic <token>`, with `<token>` the `actuator-token` secret (store it in the IDE's secrets UI; for `basic`, `user:password` encoded in Base64). The plugin never reads or logs the token; the host substitutes it into the header. Needs the optional `secrets` capability. |
| `timeoutMs` | `2000` | Timeout per endpoint request, kept between 100 and 30000 ms. |
| `autoRefresh` | `false` | Refresh from Actuator every minute. |

Without `actuatorAuth`, requests carry no credentials. An endpoint that answers 401 or 403 adds "needs authentication: check actuatorAuth and the actuator-token secret" to the status line. An endpoint that answers 404 or anything other than JSON with status 200 is listed as not available, and the rest are still read. If the first endpoint (`beans`) cannot be reached at all, the refresh stops with an error. The plugin requires `net:localhost`. `net:127.0.0.1` is optional; without it, a URL on `127.0.0.1` fails with a message.

The plugin uses the network only on Refresh (the command, the tool window button or auto-refresh). The result is kept in memory and in plugin storage until Clear. When it is larger than about 900 KiB, only beans and environment are stored (loggers and caches are kept in memory only) and the status line says so.

## Spring Runtime tool window

The status line shows when and from which URL the data was read, the endpoints that were not available, and the last refresh error. Activating an item opens it in the project's sources when a matching class, method or key is found.

- Beans (`beans`): per application context, each bean with scope and type, expandable to its dependencies. Opens the bean's class.
- Health (`health`): the overall status, then components and nested components when Actuator includes them (`components`, or `details` on older Spring Boot versions).
- Configuration (`env`): the active profiles (or "default profile"), a Config files group listing the configuration file property sources (opens the file), then every property source in precedence order, highest first, with its properties and values as Actuator returns them (masked values appear as `******`). A property opens its key in the project's `application*`/`bootstrap*` files, preferring the file it came from.
- Auto-configuration (`conditions`): positive and negative matches, each with its condition messages. Opens the class or `@Bean` method.
- Mappings (`mappings`): per dispatcher servlet or handler, each request predicate with its handler. Opens the handler method.
- Configuration properties (`configprops`): each `@ConfigurationProperties` bean by prefix, with its bound properties and values. The bean opens its class, a property its key in the configuration files.
- Scheduled tasks (`scheduledtasks`): each task's target method with its kind (`cron`, `fixedDelay`, `fixedRate`, `custom`) and expression, interval or trigger. Opens the method.
- Loggers (`loggers`): `ROOT` and each logger with a configured level, with its configured and effective level.
- Caches (`caches`): each cache as `manager / cache` with its target type.

## Editor markers

Markers come from the last snapshot only; the editor never triggers a request.

- Java beans: a gutter marker on each stereotype class (`@Component`, `@Service`, `@Repository`, `@Controller`, `@RestController`, `@Configuration`, `@SpringBootApplication`, `@ControllerAdvice`, `@RestControllerAdvice`, `@Endpoint`, `@ShellComponent`, `@TestConfiguration`) and `@Bean` method loaded in the application, with its name, scope and number of dependent beans.
- Not loaded: an info diagnostic on a stereotype class or `@Bean` method missing from the application, when the application has beans in the same package. When a negative auto-configuration match names the class or bean, its condition messages are added.
- `@Value`: on each `@Value("${key:default}")`, the runtime value of every placeholder key and the property source it comes from.
- `@ConfigurationProperties`: on the class, whether the prefix is bound and how many properties it has; on each non-static field or record component, the runtime value and its source, or the bound value when no property is set (the field's default).
- Configuration files (`application*.properties|yml|yaml`, `bootstrap*.properties|yml|yaml`): on each key with a value, "Active (from file)" when this file's value wins, otherwise the winning value and its property source.

Keys are matched with relaxed binding (`spring.jpa.show-sql` and `spring.jpa.showSql` are the same key; the `SPRING_DATASOURCE_URL` environment variable is `spring.datasource.url`).

## Commands

| Command | Does |
|---|---|
| Spring Runtime: Refresh from Actuator | Reads the endpoints and reports the bean and property source counts and the endpoints not available |
| Spring Runtime: Clear | Drops the snapshot from memory and storage |

The tool window has the same Refresh and Clear buttons.

Java sources are parsed as text, and beans are matched to runtime beans by qualified class name, or by bean name and simple type. YAML block values (`|`, `>`) get no marker. The plugin reads only the Spring Boot 2.x and 3.x Actuator JSON formats and only applications on `localhost`/`127.0.0.1`. Source markers are for Java only.

Not covered: Actuator endpoints behind other authentication than a bearer or Basic token (OAuth flows, client certificates), applications on other hosts, Kotlin sources, SpEL in `@Value` (`#{…}`), starting or stopping the application, changing values at runtime.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
