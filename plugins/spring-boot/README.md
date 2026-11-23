# Spring Boot Tools

Completion and checks for Spring Boot configuration files (`application*` and `bootstrap*` `.properties`, `.yml`, `.yaml`), plus Spring Boot checks in Java and Kotlin sources. For anyone writing a Spring Boot application in OpenIDEA. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- A Java or Kotlin project with Spring Boot configuration files.
- Maven (`mvn`) or Gradle (`gradle`) on the `PATH` the IDE sees, only to run the app from the IDE.
- A local Maven repository (`~/.m2/repository`) or Gradle cache, only to read metadata from a dependency JAR.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a Spring Boot project and an `application.properties` or `application.yml` file.
3. Start typing a key such as `server.` and pick from the completion list.
4. Run **Spring Boot: Show Configuration Problems** to list every problem in the project's configuration.
5. Optional: allow `events` in Settings › Plugins › *Spring Boot Tools* so checks that need the whole project can run.

## Features

### Complete keys and values

- In `.properties`, completion offers full dotted keys.
- In YAML, it offers the next segment under the current parent. A property inserts `name: `. A group or map inserts `name:` and a new indented line.
- Each item shows the property's type. Its documentation gives the description, the default and any deprecation.
- Map keys hinted by the metadata are offered too (`logging.level.root`).
- Values: `true`/`false` for booleans, and the values hinted by the metadata (`logging.level.*` levels, `spring.jpa.hibernate.ddl-auto`…), in both formats.
- Logger packages under `logging.level`, taken from the project's `package` lines.
- Profile names (from `application-*` files) in `spring.profiles.active` and `spring.profiles.include`.
- Keys inside `${…}`.
- Project class names for `Class` keys, and enum constants (project enums and well-known ones).
- `classpath:` resources under `src/main/resources` for `Resource` keys and the `spring.config.*` location keys.

See [Where keys come from](#where-keys-come-from) for the sources of the key list.

### Catch configuration mistakes

- **Deprecated keys** are struck through, with a warning that names the replacement. ⌥⏎ replaces the key.
- **Unknown keys** get a warning when the parent is a known group of two segments or more (`spring.datasource.urll`). ⌥⏎ can add the key to your `additional-spring-configuration-metadata.json`.
- **Wrong value types** get a warning (`invalid-property-value`): numbers, booleans, durations (`10s`, `PT10S`), data sizes (`10MB`), enums, project classes and list items.
- **Cron expressions** are checked in `@Scheduled(cron = "…")` and in config keys ending with `cron` (`app.cleanup.cron`, `app.report-cron`).

Some keys are never reported as unknown, so third-party and custom keys stay quiet. See [Technical details](#technical-details).

### Check Java and Kotlin code

| Check | What it flags | Setting |
|---|---|---|
| Test constructors | A Spring test class whose only constructor takes parameters Spring won't autowire. JUnit would fail with a `ParameterResolutionException`. | `springBoot.checkTestConstructors` |
| `@EventListener` methods | More than one parameter (error: Spring throws). A plain `String` or primitive return (hint: the value is published as a new event). | `springBoot.checkEventListeners` |
| Early events | A listener for `ApplicationStartingEvent`, `ApplicationEnvironmentPreparedEvent`, `ApplicationContextInitializedEvent` or `ApplicationPreparedEvent`. These fire before the context exists, so the listener never runs (`early-event-listener`). | `springBoot.checkEarlyEvents` |
| `@ConfigurationProperties` | A prefix that isn't lower-case kebab-case, a missing prefix, and misused constructor binding. | `springBoot.checkConfigurationProperties` |
| Application setup | `@SpringBootApplication` in the default package, and a redundant `@EnableAutoConfiguration` or `@ComponentScan` next to it. ⌥⏎ removes the redundant annotation. | `springBoot.checkApplicationSetup` |
| Test lifecycle | Wrong `@DirtiesContext` modes, and `@BeforeTransaction`/`@AfterTransaction` methods that don't return `void`. | `springBoot.checkTestLifecycle` |
| Reactor debug | A hint on `Hooks.onOperatorDebug()` when `reactor-tools` is a dependency (use `ReactorDebugAgent.init()`). | always on |
| JDBC driver (opt-in) | `spring.datasource.url` names a JDBC driver no build file declares. | `springBoot.checkDriverDependency` |

### Check metadata files

`spring-configuration-metadata.json` and `additional-spring-configuration-metadata.json` are checked for shape, types, duplicates, defaults, deprecation, hints and unknown project classes. Turn this off with `springBoot.checkMetadataFiles`.

### Navigate and hover

- Go to Definition from `spring.config.import` and from `Resource` keys with `classpath:`/`file:` values opens the file.
- Go to Definition from a `Class` valued key opens the project class.
- A simple `${key}` in an `application[-profile].properties` or `bootstrap[-profile].properties` file jumps to the same key in that file.
- Hover a key to see its type, default, description, deprecation, and which catalog supplied it.
- Inlay hints show the declared type of a key.
- Hover and inlays never show effective runtime values.

### Find a property's occurrences

Put the caret on a key or `${key}` name in a `.properties` file, then:

- **Spring Boot: Find Property in File…** lists the key and its placeholders in the current file, including unsaved text.
- **Spring Boot: Find Property in Project…** lists them across the project's `application*.properties` and `bootstrap*.properties`, with the profile and file of each.

Pick a row to jump to it. Values and defaults are never shown.

### Rename a property everywhere

On a literal key, press ⌥⏎ and choose **Rename exact static property … with preview…**. The plugin finds the key in configuration files and in Java/Kotlin `@Value` and placeholder strings of the current module. It lists every file, line and column it will change, without showing values. You confirm before anything is written, and the edit is one undo step.

The rename is withheld when it can't be safe. For example, keys that belong to Spring Boot, a dependency, local metadata or a `@ConfigurationProperties` class: renaming the text alone would break their binding.

### Run the app with profiles or arguments

Run **Spring Boot: Run with Profiles/Arguments…**:

1. Pick the build, if the project has more than one.
2. Tick profiles found in `application-<profile>` files and imported files.
3. Type extra profiles and `--key=value` arguments, if any.
4. Check the exact Maven or Gradle command and confirm. It opens in a terminal tab.

Allow `process:mvn` or `process:gradle` first.

To get native Run entries instead, turn on **Offer native Boot Maven/Gradle run profiles** (`springBoot.nativeRunProfiles`). Run then lists **Spring Boot: Maven spring-boot:run (project build)** or **Spring Boot: Gradle bootRun (project build)**. It also lists Maven modules and Gradle subprojects that apply the Boot plugin. Native Run handles start, stop, rerun and logs.

### Read the Run console

In the Run and Debug consoles, Spring Boot log lines get their level coloured: `ERROR` in red and `WARN` in yellow, from the timestamp to the `---` separator. The `APPLICATION FAILED TO START` block and its `***` banners show in red, `Caused by:` lines show the exception in red, and `Started … in … seconds` and `… started on port …` show in green. The colours apply only to Spring Boot's default log format (Boot 2 and 3), so other programs' output is left alone.

### Create a Database source

Open a configuration file with literal `spring.datasource.url`, `username` and `password` values. Run **Spring Boot: Configure Database from Current Source…** and confirm. The plugin creates a native Database source. The password goes straight to the secure credential store and never appears in the confirmation. It doesn't connect or run SQL: open Database to connect.

Supported URLs: PostgreSQL, MySQL/MariaDB, ClickHouse HTTP, Oracle thin service, and SQL Server with `databaseName`. Anything else needs manual setup.

### Read metadata from a dependency JAR

Use this when your project uses a Spring Boot or Spring Cloud version other than the bundled one, or a starter that ships its own metadata.

1. Allow `fs.read:~/.m2/repository` (Maven) or `fs.read:~/.gradle/caches/modules-2/files-2.1` (Gradle).
2. Run **Spring Boot: Read Metadata from Dependency JAR…**. The settings page has a button for it too.
3. Pick a dependency declared in your root `pom.xml` or `build.gradle(.kts)`.
4. Check the project, build file, coordinates and archive, then confirm.

Completion, checks and hover then use that JAR's keys. The snapshot lasts 10 minutes and only for this session. A notification says how many properties were loaded.

### Create Spring Boot files

The New File menu has templates for Java and Kotlin: **Spring Boot Application**, **Spring Boot Test** and **Spring Boot ConfigurationProperties** (each with a **(Kotlin)** variant).

Configuration files also get a Spring Boot file icon.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Spring Boot**; bind keys in Settings › Keymap.

| Command | What it does | Shortcut |
|---|---|---|
| **Spring Boot: Show Configuration Problems** | Checks every configuration and metadata file and lists the problems, errors first. Pick one to open it. | — |
| **Spring Boot: Read Metadata from Dependency JAR…** | Loads configuration metadata from one Maven or Gradle dependency JAR you choose. | — |
| **Spring Boot: Find Property in File…** | Lists the key under the caret and its `${…}` uses in the current file. Also in the context menu of `.properties` editors. | — |
| **Spring Boot: Find Property in Project…** | Lists the key under the caret across the project's `.properties` configuration files. Also in the context menu of `.properties` editors. | — |
| **Spring Boot: Run with Profiles/Arguments…** | Asks for profiles and arguments, shows the Maven or Gradle command, then runs it in a terminal. | — |
| **Spring Boot: Configure Database from Current Source…** | Creates a native Database source from the datasource values in the current file. | — |

## Settings

Settings › Plugins › *Spring Boot Tools*. All are on/off switches.

**Java Source Checks** page:

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Warn when a Spring test class has a constructor with parameters that Spring will not autowire | `springBoot.checkTestConstructors` | on | Test constructor check |
| Check the parameters and return type of @EventListener methods | `springBoot.checkEventListeners` | on | `@EventListener` signature check |
| Warn about @EventListener methods for events published before the context exists | `springBoot.checkEarlyEvents` | on | Early event check |
| Check @ConfigurationProperties prefixes and constructor binding | `springBoot.checkConfigurationProperties` | on | `@ConfigurationProperties` checks |
| Check @SpringBootApplication classes (default package, redundant annotations) | `springBoot.checkApplicationSetup` | on | Application setup checks |
| Check @DirtiesContext modes and @BeforeTransaction/@AfterTransaction signatures | `springBoot.checkTestLifecycle` | on | Test lifecycle checks |

**Configuration and Metadata** page:

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Warn about values that do not match the key type | `springBoot.checkPropertyValues` | on | Value type check |
| Check spring-configuration-metadata.json files | `springBoot.checkMetadataFiles` | on | Metadata file checks |
| Hint when the JDBC driver of spring.datasource.url is missing from the build | `springBoot.checkDriverDependency` | off | JDBC driver hint |
| Offer native Boot Maven/Gradle run profiles | `springBoot.nativeRunProfiles` | off | Adds Spring Boot entries to native Run |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › *Spring Boot Tools*.

| Permission | Used for | Without it |
|---|---|---|
| `events` | Keeping the project's configuration facts current as files change. | Checks that need the whole project (unknown keys, test constructors…) stay quiet, and a notice says so once. |
| `fs.read:~/.m2/repository` | Reading a Maven dependency JAR and cached parent/BOM POMs. | The JAR metadata command says which permission to allow. |
| `fs.read:~/.gradle/caches/modules-2/files-2.1` | Reading a Gradle dependency JAR from the module cache. | A notice asks you to allow it; Gradle dependencies are skipped. |
| `editor.read` | **Find Property in File…**, **Find Property in Project…** and **Configure Database from Current Source…** read the open editor's text and caret. | Those commands say which permission to allow. |
| `database.sources` | **Configure Database from Current Source…** creates a Database source. | That command says which permission to allow. |
| `process:mvn` | Running `mvn` from **Run with Profiles/Arguments…** and native Run. | Maven runs are refused. |
| `process:gradle` | Running `gradle` from **Run with Profiles/Arguments…** and native Run. | Gradle runs are refused. |

## Troubleshooting

General steps are in the [repository guide](../../README.md#using-the-plugins).

| Problem | What to do |
|---|---|
| No unknown-key or test-constructor warnings | These wait until the whole project has been read. Allow `events`. |
| A third-party or custom key isn't flagged as unknown | Expected: unknown groups are never reported. Add the key to `additional-spring-configuration-metadata.json` to document it. |
| Keys from a newer Boot version show as unknown | Run **Spring Boot: Read Metadata from Dependency JAR…** and pick the Boot or Cloud artifact. |
| "No problems found … but the project isn't fully read yet" | Results are partial. If `events` is off, allow it; then run **Spring Boot: Show Configuration Problems** again. |
| JAR command: "Open the Maven or Gradle project first." | Open the project whose root holds `pom.xml` or `build.gradle(.kts)`. |
| JAR command: "Found no dependency with a known version in the root build file…" | Use a literal version, a root property, or the Boot parent/BOM, and build the project once so the parent/BOM is in the local cache. |
| JAR command: "Selected JAR is not in the Gradle module cache." | Build the project once with Gradle so the JAR is downloaded. |
| *Reading Gradle JAR metadata needs the fs.read:~/.gradle/caches/modules-2/files-2.1 permission…* | Allow `fs.read:~/.gradle/caches/modules-2/files-2.1` in Settings › Plugins › Spring Boot Tools. |
| "The build file changed. Run Read Metadata from Dependency JAR… again." | The build file changed during the read. Run the command again. |
| "Find Property works in application*.properties and bootstrap*.properties files…" | Open one of those files (not YAML) and place the caret on a key. |
| "Eight project snapshots are already retained; reload before importing another project." | Reload the plugin in Settings › Plugins, then run the command again. |
| Run command: "No Spring Boot run found. Apply the Spring Boot Maven or Gradle plugin." | Apply the Spring Boot plugin directly in the root `pom.xml` (build plugins) or the root Gradle `plugins` block. |
| The run terminal doesn't start | Allow `process:mvn` or `process:gradle`, and check the tool is on the IDE's `PATH`. |
| Database: "This source file has no explicit spring.datasource.url." | Open the file that declares the datasource, with plain literal values. |
| Database: "URL parameters, embedded credentials and encoded/multiple targets need manual Database setup." | Create the source by hand in Database. |
| Occurrences: "Place the caret on a literal property key or placeholder name." | Move the caret onto the key itself, in a `.properties` file. |
| Occurrences: "Configuration source inventory exceeds 128 files…" | The project has too many configuration files for this command. Use the current-file command. |
| Rename isn't offered, or is refused | The key belongs to Spring Boot, a dependency or a `@ConfigurationProperties` class, or a file can't be read safely. Rename by hand. |

## Limitations

- Keys aren't linked to Java code: no Go to Definition from `${key}` to Java, no Find Usages, no IDE-wide Rename.
- Effective runtime values, active profiles and Actuator data aren't shown.
- Bean references in values aren't resolved.
- Kotlin `@ConfigurationProperties` classes aren't read for keys.
- Not implemented: Flyway/Liquibase, Spring MVC view resolution, live run dashboard data (beans, health, environment), SpEL in `@ConditionalOnExpression`, Kotlin configuration classes, repository query preview, AOT/repository-query inlays.
- Database setup is limited to the command above.
- Quick fixes can't create files: the metadata quick fix needs an existing `additional-spring-configuration-metadata.json`.

## Where keys come from

- **Bundled snapshot:** Spring Boot 3.5.16 (`spring-boot`, `spring-boot-autoconfigure`, `spring-boot-actuator-autoconfigure`) and Spring Cloud 2025.0.3 (Northfields: commons, context, loadbalancer, config, Netflix Eureka, gateway, OpenFeign, Stream and its Kafka binders, circuit breaker Resilience4j, bus, Consul, Zookeeper, Kubernetes, Vault, contract, task, function), in `data/boot-properties.tsv`. Boot's keys win over Cloud's.
- **Your metadata files:** the project's `META-INF/spring-configuration-metadata.json` and `additional-spring-configuration-metadata.json`. The project search honours `.gitignore`, so in practice these are the files under `src/main/resources`, not build output.
- **Your `@ConfigurationProperties` classes** in Java: the prefix plus the dashed name of each field, record component and setter.
- **Selected dependency JARs** (see [Read metadata from a dependency JAR](#read-metadata-from-a-dependency-jar)). They take precedence over the bundled snapshot.

Regenerate the bundled snapshot with `python3 -I scripts/gen-metadata.py 3.5.16 data/boot-properties.tsv 2025.0.3`.

## Technical details

<details>
<summary>Check rules and exceptions</summary>

**Unknown keys** are never reported for: keys under maps (`logging.level.*`, `spring.jpa.properties.*`), list items (`[0]`) of known properties, unknown groups (third-party starters, Spring Cloud modules not in the bundled list, your own keys), and unknown sub-groups of known groups (`spring.datasource.druid.*`, `management.health.consul.*`). The check runs only once the whole project has been read.

**Property values** (`invalid-property-value`, `springBoot.checkPropertyValues`) convert to `Integer`, `Long`, `Short`, `Double`, `Boolean`, `Duration` (`10s`, `PT10S`) or `DataSize` (`10MB`). Enums: any the project declares, or a well-known Spring Boot/JDK one (`Access`, `Show`, `TimeUnit`, `DatabaseInitializationMode`, `WebApplicationType`…), matched the relaxed way (`safe-mode`, `SAFE_MODE`). A `Class` value is flagged only when it names no class in a package the project owns; library classes are never judged. The items of `List<X>`, `Set<X>` and `X[]` keys are checked (comma separated, or the items of a YAML sequence). `${…}`, `#{…}`, maps and other custom types are never checked. Literal values are validated against declared metadata types; environment/profile/import precedence and runtime values are unresolved.

**Cron** follows Spring's six-field syntax (second minute hour day-of-month month day-of-week), macros (`@daily`…), `-`, `?`, ranges, steps, lists, `L`, `L-n`, `LW`, `nW`, `nL`, `n#k`, month and day names. Placeholders (`${…}`) and SpEL (`#{…}`) are skipped.

**Test constructors:** applies to classes with `@SpringBootTest`, `@WebMvcTest`, `@DataJpaTest`, `@ContextConfiguration`… whose only constructor takes parameters without `@Autowired` (on the constructor or a parameter; also `@Qualifier`/`@Value`). Spring autowires them only with `@Autowired`, `@TestConstructor(autowireMode = ALL)`, or `spring.test.constructor.autowire.mode=all` in `spring.properties` or a system property. The check stays quiet until the whole project is read, and while any `spring.properties`, `application*`, build file or `gradle.properties` mentions that key. Unknown configuration stays quiet.

**`@EventListener`:** `void`, event objects, collections and async types are not reported. Runtime event delivery is not evaluated.

**`@ConfigurationProperties`:** a warning for a prefix that is not lower-case kebab-case words (`config-properties-prefix`) and a hint for a missing prefix. Constructor binding: `@ConstructorBinding` on a component-scanned bean (`constructor-binding-component`, warning) or on two constructors (`constructor-binding-ambiguous`), and an info for several constructors with none annotated and no default one (`constructor-binding-missing`). It does not prove effective relaxed-binding owners.

**Application setup** IDs: `default-package-application`, `redundant-enable-auto-configuration`, `redundant-component-scan`. No run configuration is created.

**Test lifecycle** IDs: `@DirtiesContext(methodMode)` on a class, `classMode`/`hierarchyMode` on a method (`dirties-context-mode`); non-`void` `@BeforeTransaction`/`@AfterTransaction` (`transaction-callback-signature`). Tests are not executed.

**JDBC driver hint** is off by default because transitive drivers are invisible: only explicit build declarations are read. No database connection is opened.

**Metadata files** are checked for unknown fields, a malformed `type`, duplicate names, a missing description (hint), a `defaultValue` that does not convert to the `type`, a `deprecation` that is not an object, has unknown fields or a `level` other than `warning`/`error`, a `deprecation.replacement` or hint name that no property defines (info), and a `type` in a project package naming no project class. Library classes are never judged. Declared defaults are static metadata, not effective application values. Needs the host to give JSON files to plugins.

The plugin reads source text only (Java and Kotlin sources for classes and enums).

</details>

<details>
<summary>Which files count as configuration</summary>

Besides `application*` and `bootstrap*`, a `.properties`/`.yml`/`.yaml` file is read as configuration when a standard file points at it with `spring.config.import`, `spring.config.location` or `spring.config.additional-location` (`classpath:`, `file:`, `optional:`; other schemes and directories are skipped), or by `spring.config.name`.

Recognition also follows literal `spring.config.import` chains: import-relative locations resolve against the importing file; fixed `file:`/`classpath:` ones by path tail; `optional:` is handled; `configtree:`, placeholders and wildcards are refused; depth at most 8, at most 64 files, cycle-safe. `<name>-<profile>` variants of imported files are followed for literal profiles. Their keys feed the unknown-key, definition and usage checks. Otherwise only one level of the legacy indirection is followed; transitive hops through non-standard files are followed by command-time discovery only. The project search skips dot directories and gitignored paths.

</details>

<details>
<summary>Navigation, hover and inlay details</summary>

Go to Definition from `Resource` keys skips placeholders and schemes other than `classpath:`/`file:`.

Simple `${key}` / `${key:literal-default}` references in current `application[-profile].properties` and `bootstrap[-profile].properties` text navigate to one exact same-file key token. Hover gives the source line and explains unresolved profiles, imports, relaxed binding, environment overrides and runtime values. Neither declaration values nor defaults are rendered. Duplicate or missing keys suppress the local target. Decoded escaped keys, relaxed alternate spellings, escaped/continued values, SpEL, nested defaults, multi-document properties, YAML and nonstandard imported files are outside this path.

Key hover labels the catalog that supplied the declaration: project-local current/cached metadata; a selected dependency-JAR snapshot (with GAV/scope, archive, entry names and SHA-256 fingerprint, plus an explicit unverified-classpath caveat); or the bundled fallback (labelled as a packaged snapshot with its dataset versions, not the project's resolved version). Unknown keys, placeholders, ambiguous declarations and open property prefixes get no speculative hints.

</details>

<details>
<summary>Commands: rules and limits</summary>

**Show Configuration Problems** runs the same checks as the editor diagnostics, without the opt-in JDBC driver hint. Rows are filtered against the current invoking project owner, and current source identity is rechecked before opening.

**Find Property in File…** lists the one unique exact property spelling and matching placeholder spellings in the current unsaved file (line and UTF-16 column). It reads only the invoking editor: no workspace scan, metadata lookup, network or write. Duplicate or escaped/relaxed spellings, dynamic/nested/escaped placeholder values, multi-document sections and unsupported file contexts stay quiet or refuse. Current text, version, path and project membership are rechecked before opening.

**Find Property in Project…** finds exact raw declaration and placeholder spellings in `application*.properties` and `bootstrap*.properties` files owned by the current project, keeping profile and file provenance. The current editor text is used for its file; other rows describe the saved file. Source identity is rechecked before navigation. Limits: 128 files, 1 MiB total source, 1,024 occurrences. Nested registered projects are excluded. Unreadable, oversized or unsupported inputs fail visibly. These are read-only exact physical-source occurrences, not effective usages.

**Run with Profiles/Arguments…** validates profiles. Besides `application-<profile>` files it offers literal profiles named by `spring.profiles.include`, `spring.profiles.group` and `spring.config.activate.on-profile` (plain names or `a | b` only) or declared in imported files. Only allowlisted `--key=value` arguments are accepted. Maven uses `-Dspring-boot.run.profiles` / `-Dspring-boot.run.arguments`; Gradle uses `bootRun --args`. Nothing runs before confirmation.

**Native run profiles** discovery reads only the registered project root's saved build declarations: a direct Maven build/plugins Boot plugin, or a literal applied Boot plugin plus the Java, Java Library, Application or JVM Kotlin plugin in the root Gradle plugins block. Kotlin `java`, `application` and `kotlin("jvm")` accessors are recognized only in Kotlin builds without a locally shadowing helper. Plugin-management-only, nested, aliased, dynamic, inactive or ambiguous declarations do not establish a profile. Maven modules and Gradle subprojects come from literal root modules/include entries whose own build file applies the Boot plugin; custom project dirs, profile modules and non-literal includes are withheld. Starting a profile runs `mvn --batch-mode --file pom.xml spring-boot:run` or `gradle --console=plain bootRun` in that root with the granted system executable. Project plugins and build logic can execute. Discovery never runs a compiler, process, wrapper or network request. App arguments, profiles and JVM options come from the project build. The adapter does not infer active profiles, edit saved run configurations, supply a wrapper or attach a debugger.

**Configure Database from Current Source…** reads literal current-editor `spring.datasource.url`, username and password values in one properties/YAML document. Explicit/default ports and bracketed IPv6 are supported. Other SQL Server transport/authentication/instance properties are refused. It does not connect, introspect or execute SQL. Duplicate keys, placeholders, encoded/aliased/typed values, embedded credentials, URL parameters and unsupported URL formats require manual setup. If no password is declared, an existing stored credential for the same source is left unchanged. Target or user changes create a separately owned source; password rotation keeps the same target identity.

</details>

<details>
<summary>Dependency JAR metadata: resolution and limits</summary>

Exact paths are derived from supported root `pom.xml` or `build.gradle(.kts)` declarations. The confirmation shows the project, build file (named Maven or Gradle), GAV/scope, version-catalog paths/hashes and selected archive.

**Gradle:** literal `group:artifact[:version]` coordinates (explicit, or managed by the Boot plugin or a platform BOM) from the read-only module cache (`~/.gradle/caches/modules-2/files-2.1/<group>/<artifact>/<version>/<sha1>/`). Ranges, dynamic versions, classifiers and interpolation are refused. When the grant is denied, a notice asks you to allow it.

**Maven:** versions may be literal, exact `${property}` aliases from root POM properties, exact root `dependencyManagement` versions, or managed by the root `spring-boot-starter-parent` or an imported `spring-boot-dependencies` BOM (version literal or a same-POM `${property}`), read from cached BOM POMs under `~/.m2/repository`. Ranges, SNAPSHOT versions and unresolved placeholders are refused. Parent chains deeper than 4 and reading more than 32 imported BOMs are refused. In-project parent POMs are not followed.

Only top-level `META-INF/spring-configuration-metadata.json` and `META-INF/additional-spring-configuration-metadata.json` are read. Identical normalized records are deduplicated; conflicting cross-document records are refused. Project-local metadata wins over selected JAR metadata. Selecting a version-qualified Boot/Cloud artifact removes the bundled fallback for that source scope, and absence findings (unknown keys from an incomplete catalog) are withheld. Multiple snapshots coexist with hashes and version provenance. If a read fails, the previous snapshot stays available until its source changes.

Session-only: at most 16 selected artifacts per project across eight project roots, kept until a source POM or build file changes, 32,768-property aggregate bound. Saving or opening an unchanged POM keeps them.

POM parsing is namespace-aware and bounded: 256 KiB per POM, 4,096 XML elements, nesting 32, 128 direct dependencies, 64 catalog POMs / 2 MiB / 512 offered candidates.

Archives: 8 MiB (host cap 16 MiB), 2,048 entries, 256-byte entry names, 1 MiB central directory, 64 MiB total expansion. Metadata: 512 KiB per entry, 768 KiB combined, 100:1 expansion ratio, 4,096 properties, 1,024 hints, 128 bounded values per hint. Stored/deflate classic ZIP only; ZIP64, encryption, nested/fat JARs, path tricks, duplicate/overlapping entries and inconsistent sizes are refused. Length, UTF-8 and CRC are verified. SHA-256 records source bytes, not publisher signatures.

These are declared catalog candidates, not proof of effective dependency resolution or classpath contents. No classes are loaded, no build runs and nothing is fetched from the network.

</details>

<details>
<summary>Quick fix and rename safety rules</summary>

**Deprecated key fix:** in YAML only when the replacement keeps the key's written parent. It inserts only bounded ordinary dotted ASCII key names; a replacement containing property/YAML syntax stays diagnostic-only. An existing target key (including a relaxed-spelling collision) prevents the edit.

**Unknown key fix** (**Define `key` in additional-spring-configuration-metadata.json**): requires an existing, complete, valid JSON file with unambiguous plain top-level fields, at most 128 KiB source and a 256-byte new key. The key is serialized as a JSON string, so quotes and backslashes stay data.

**Remove redundant annotation fix:** explicit supported owners are required for both annotations, with the same import/FQN, shadow and current-project source checks.

**Rename exact static property … with preview…** inventories host-visible application/profile/bootstrap configurations and conventional Java/Kotlin source roots of the current source module. It follows literal local config imports and `PropertySource`/`TestPropertySource` locations, and updates exact declarations and placeholder/`@Value` spellings. Canonical single-import aliases and Kotlin use-site annotations are supported. Duplicate/relaxed destination collisions, encoded or malformed source, unreadable required local imports and unresolved binding ownership withhold the operation. Nested YAML renames keep the dotted parent. Known framework, dependency metadata, local metadata and `@ConfigurationProperties`-bound keys are withheld. Dynamic/external/environment/`getProperty`/meta-annotation references and non-conventional or host-hidden source roots are outside this static refactor. Limits: 256 files, 128 KiB per file, 4 MiB total, 2,048 edits and the host's exact confirmation size. Resolution itself does not write files; edits are revalidated before they are returned.

</details>

<details>
<summary><code>@ConfigurationProperties</code> extraction</summary>

Keys come from `@ConfigurationProperties(prefix|value = "…")` classes and records in Java sources, one level into classes of the same file. A prefix on a `@Bean` method, or on a class with a superclass, accepts any key below it.

Extraction requires an exact fully qualified annotation or one unique canonical single-type import for `ConfigurationProperties` and `Bean` (both owners for Bean-method prefixes), and exactly one explicit, unescaped string literal in `prefix` or `value`. Concatenation/constant fragments, conflicting or duplicate aliases, escaped strings and text blocks stay quiet. Same-file nested binding follows unique declared enclosing type scopes, plain source and plain non-array field types; imported, qualified, ambiguous or inherited targets stay opaque. Expansion is capped at 512 properties.

This proves only admitted explicit source spelling. Composed annotations, classpath/module identity and effective binding remain unresolved, and this is not semantic rename authority. Automatic host indexing, Find Usages and Rename of configuration keys stay unregistered: existing YAML paths, relaxed keys and Java-derived binding names are not unconditional replaceable anchors.

</details>

<details>
<summary>Caching, bounds and project ownership</summary>

Editor diagnostics use the versioned current-document provider for the Java annotation/cron, metadata JSON and configuration checks. A bounded project cache is warmed during activation and file events. Diagnostics, hover and inlay requests read only that cache and the supplied document text. Unsaved edits replace the current document's cached facts. Missing, conflicting, dynamic or incompletely read declarations suppress absence-based findings.

Request limits: 128 KiB of current text, 512 configuration keys, 500 diagnostics and 200 inlays; oversized inputs stay quiet. The current-file placeholder model is bounded to 32 KiB source, 256 keys, 256 references, 256-byte ASCII names and a cooperative 10 ms budget, and requires a currently owning open project. Token ranges are half-open UTF-16.

Cached source metadata and live facts are filtered against the current open-project roots (including nested projects opened later). Only facts owned by the requesting document's longest matching root are admitted. The same gate filters completion catalogs, type/package facts, imported configuration recognition, profile/resource suggestions, navigation candidates and the metadata file chosen by quick fixes. An ownership-incomplete cached pass cannot establish absence.

Capabilities: required `project.read`; optional `events`, `fs.read:~/.m2/repository`, `fs.read:~/.gradle/caches/modules-2/files-2.1`, `editor.read`, `database.sources`, `process:mvn`, `process:gradle`.

Settings are split over two native pages: **Java Source Checks** (Tests, Application Events, Configuration Classes) and **Configuration and Metadata** (Configuration Files, Static Metadata, Static Build Facts, Project Build Execution). Keys keep their ids, types and defaults, including the driver-dependency opt-in default; existing overrides continue to apply. The configuration page links the JAR metadata command; viewing Settings starts no JAR read, scan, process or network operation. These are inspection switches and static-source import controls: environment, Actuator values, effective profiles and metadata defaults are not editable runtime settings.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
