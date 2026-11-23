# Quarkus Tools

Quarkus support in OpenIDEA: completion, checks and navigation for configuration files, Java sources and Qute templates, plus Dev Mode run configurations. It covers `application*.properties`, `application*.yaml|yml` and `META-INF/microprofile-config.properties`. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- A Maven or Gradle project that declares `io.quarkus` in its `pom.xml`, `build.gradle` or `build.gradle.kts`. The plugin stays quiet elsewhere.
- For Dev Mode run configurations: `mvn`, `gradle` or the `quarkus` CLI on your `PATH`.
- Optional: the [Qute/JDT adapter](adapters/README.md) for typed Qute language server features (Java, Python and the tools it lists).

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Allow the optional **events** permission in Settings › Plugins › *Quarkus Tools*. It enables unknown-key and cross-file checks.
3. Open `src/main/resources/application.properties` in a Quarkus module.
4. Type `quarkus.http.` and pick a key from the completion list. Its type, default and description show in the documentation popup.
5. To start the app, allow `process:mvn` (or `process:gradle`, `process:quarkus`) and run **Quarkus Dev Mode (mvn)** from the run configurations.

## Features

### Complete configuration keys and values

- Keys: full dotted keys in `.properties`; the next segment under the current parent in YAML.
- Each item shows the property's type. Its documentation gives the description, the default and any deprecation.
- Values: enum values, `true`/`false` and log levels.
- Profiles: `%dev.`, `%test.` and `%prod.` are offered at the start of a key. Profiled keys (`%dev.quarkus.http.port`, or YAML under `"%dev":`) are completed and checked like their base key.
- Named instances: `quarkus.datasource."users".jdbc.url` is completed and checked like `quarkus.datasource.jdbc.url`. This works for the roots the metadata marks as named: datasource, hibernate-orm, flyway, liquibase, mongodb, redis, oidc, oidc-client, mailer and a few more.
- Your own keys: from `@ConfigProperty(name, defaultValue)` on fields and parameters, and `@ConfigMapping(prefix)` interfaces (`@WithName`, `@WithParentName`, `@WithDefault`, `namingStrategy`, `Optional<T>`, nested interfaces in the same file).
- In Java, `@ConfigProperty(name = "` and `@ConfigMapping(prefix = "` complete keys and prefixes.

Keys come from four places:

- Quarkus 3.40.1 metadata for about 170 popular `io.quarkus` extensions, bundled with the plugin.
- MicroProfile Config metadata.
- `@ConfigProperty` and `@ConfigMapping` declarations in your Java sources.
- A dependency JAR you choose (see [Read keys from a dependency JAR](#read-keys-from-a-dependency-jar)).

### Hover and inlay hints

- Hover a config key, or a literal `@ConfigProperty` name in Java, to see its type, description, default and deprecation or replacement.
- The hover says where the definition came from: your sources, the selected dependency JAR (with its coordinates, scope and SHA-256 fingerprint), or the bundled metadata (with its dataset versions).
- Config files show each key's declared type as an inlay. Configured or runtime values are never shown.
- Java files show declared `@ConfigProperty` defaults and `@ConfigMapping` defaults and converters.

### Check configuration files

Each group can be turned off in [Settings](#settings).

- **Syntax**: duplicate keys, lines without a separator, empty values. An empty value is only a hint, and only when the property has a documented default (Quarkus treats it as unset).
- **Values**: values that aren't a valid number, enum constant, log level, duration or memory size. Booleans are never flagged.
- **Expressions**: `${...}` expressions that look like a typo of a key in the same file. Never in profile files, and never for names that could come from other files, the environment or `-D`.
- **Required keys** (off by default): an info note when a sibling key is set but a required key (no default, not `Optional`) is missing from every `application*` file and `.env`.
- Deprecated keys always get a warning.
- Unknown keys inside a known group get a warning, for example `quarkus.http.portt`. This appears only after the whole project has been read.

### Check Java sources

- **Annotations** (on by default):
  - an `@RestClient` injection whose interface has `@Path` but no `@RegisterRestClient`;
  - `@Scheduled` with a `cron` that doesn't have 6 or 7 fields, a malformed `every`, or neither;
  - `${key}` or `{key}` references in `cron`, `every`, `timeZone` or `delayed`, with no default, when no config file sets the key.
- **Unassigned keys** (off by default): a `@ConfigProperty(name = "x")` with no `defaultValue`, not `Optional`, whose key no config file or `.env` sets.
- **Health and metrics** (on by default):
  - a class with `@Liveness`, `@Readiness` or `@Wellness` that doesn't implement `HealthCheck`;
  - a top-level `HealthCheck` class with none of these annotations;
  - a `@Gauge` in a bean whose scope isn't `@ApplicationScoped`;
  - a `@Gauge` method that returns a string, boolean or void.
- **JAX-RS resources**:
  - a `@PathParam` or `@RestPath` name that matches no `{x}` variable in the class or method `@Path` (warning);
  - a path variable that no parameter binds (info);
  - more than one HTTP method annotation on a method (warning);
  - `@GET` with an unannotated entity parameter (info);
  - two resource methods with the same path, verb and overlapping `@Consumes`/`@Produces` (warning);
  - duplicate `@Scheduled` identity values (warning).
- **Qute templates**: a `@Location`, injected `Template` field or `@CheckedTemplate` method or record whose template file doesn't exist.

### Fix problems with ⌥⏎

- **Add @RegisterRestClient to …** on the interface an `@RestClient` injection names.
- **Let … implement HealthCheck**.
- **Annotate … with @Liveness** or **@Readiness**.
- **Use @ApplicationScoped** for a `@Gauge` bean.
- **Add defaultValue**, based on the field's type.
- **Add `key` to application.properties**, in the nearest existing file.

### Navigate

- **Go to Definition** links a config key with its `@ConfigProperty(name = "…")` injection, both ways. When several files match, all of them are listed.
- A gutter icon on `@ConfigProperty(name = "…")` opens the key in the module's `application*.properties|yaml` files, profile files such as `application-dev.properties` included. On `@ConfigMapping(prefix = "…")` it lists every key below the prefix. On a config key it opens the `@ConfigProperty` injections of that key. With several targets the icon shows a list to pick from; with one it jumps straight there.
- **Go to Definition** on `@Location("pages/home.html")` of a `Template` field opens that template.
- Java code lenses and document symbols show JAX-RS mappings and scheduler triggers. Click a lens to see the full effective path, built from `quarkus.http.root-path`, `@ApplicationPath` or `quarkus.rest.path`, and the class and method `@Path`.

### Work with Qute templates

- Include tags jump to the included template, and include paths complete.
- Section snippets complete in HTML and plain-text templates.
- An info note flags a static include whose template doesn't exist.
- Injected `Template` fields and `@CheckedTemplate` methods link to their template.
- `{name}` completes from checked-method parameters, record components, `.data("name", ..)` chains and `@TemplateData` / `@TemplateExtension` namespaces.
- **Create empty Qute template** (⌥⏎ on a `Template` field with `@Location("….html")` or `.txt`): previews the target path, then creates an empty file. Undo removes it.
- Files ending in `.qute.html` open in **Qute HTML** mode.
- For typed Qute features, set up the optional [Qute/JDT adapter](adapters/README.md). It runs the official Qute language server and Eclipse JDT LS.

### Run Dev Mode

Three run configurations are offered:

- **Quarkus Dev Mode (mvn)**: `mvn quarkus:dev`, in projects with a `pom.xml`.
- **Quarkus Dev Mode (gradle)**: `gradle quarkusDev`, in projects with `build.gradle` or `build.gradle.kts`.
- **Quarkus Dev Mode (quarkus CLI)**: `quarkus dev`, in either.

Each needs its program on `PATH` and its `process:` permission allowed.

In the Run and Debug consoles, Quarkus log lines get their level coloured: `ERROR` in red and `WARN` in yellow, from the timestamp to the category. `Caused by:` lines show the exception in red, and the `(powered by Quarkus …) started in …s` line shows in green. The colours apply only to Quarkus' default log format, so other programs' output is left alone.

### Create a new project

- **Quarkus: Open code.quarkus.io** opens the official project generator in your browser.
- Or build a small offline Maven + Qute starter:
  1. Open a Java or XML file of the parent project.
  2. Run **Quarkus: Create Maven/Qute Starter…**.
  3. Enter the groupId (default `org.acme`), folder/artifactId (default `qute-starter`), version (default `1.0.0-SNAPSHOT`) and the Quarkus platform version (default `3.40.1`, the version of the bundled metadata).
  4. Check the folder and the four files in the confirmation and confirm. The files are created and the new `pom.xml` opens. Existing files are never overwritten.
  5. Without the optional `project.write` permission, nothing is written: within five minutes, press ⌥⏎ in the same file and choose **Create Quarkus Starter qute-starter** instead.
  6. Click **Open Starter** in the notification, or run **Quarkus: Open Starter…**, to open the new folder as a project.

The starter has `pom.xml`, `src/main/resources/application.properties`, `src/main/resources/templates/hello.txt` and a README. It has no Java sources, REST endpoint or tests.

### Read keys from a dependency JAR

1. Allow the optional `fs.read:~/.m2/repository` permission.
2. Run **Quarkus: Load Config Keys from a Dependency JAR…**.
3. Pick a dependency from the root `pom.xml` and review the details.

The JAR's keys then feed completion, hover, checks and inlays for ten minutes. Your own source declarations win over the JAR; the JAR wins over the bundled metadata.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Quarkus**; bind keys in Settings › Keymap.

| Command | What it does | Shortcut |
|---|---|---|
| **Quarkus: Open code.quarkus.io** | Opens the official project generator in the browser. | None |
| **Quarkus: Create Maven/Qute Starter…** | Asks for coordinates, confirms the folder and its four files, then creates them. Without `project.write`, offers ⌥⏎ › **Create Quarkus Starter …** instead. Needs a Java or XML editor. | None |
| **Quarkus: Open Starter…** | Checks the created starter and opens it as a project. Available once a starter was created. | None |
| **Quarkus: Load Config Keys from a Dependency JAR…** | Loads config keys from a dependency JAR in your local Maven cache. | None |
| **Quarkus: Show Endpoint or Schedule Details…** | Shows the declaration behind a JAX-RS or scheduler code lens. From Search Everywhere, pick one of the current Java file. | None |

Run configurations: **Quarkus Dev Mode (mvn)**, **Quarkus Dev Mode (gradle)**, **Quarkus Dev Mode (quarkus CLI)**.

## Settings

Settings › Plugins › Quarkus Tools.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Warn about duplicate keys, lines without a separator and empty values | `quarkus.checkSyntax` | on | Syntax checks in config files. |
| Warn about values that do not fit the property's type (number, boolean, enum, duration, memory size) | `quarkus.checkValues` | on | Value checks in config files. |
| Check property names in expressions (same file) | `quarkus.checkExpressions` | on | Expression typo checks. |
| Note required keys (no default, not Optional) that no application file or .env sets while a sibling key is set | `quarkus.checkRequired` | off | Required-key notes. |
| Warn about @ConfigProperty keys without a default that no application file or .env sets | `quarkus.checkUnassigned` | off | Unassigned-key checks in Java. |
| Check health checks (@Liveness/@Readiness without HealthCheck and the reverse) and @Gauge beans | `quarkus.checkHealthMetrics` | on | Health and metrics checks. |
| Check @RestClient injections against @RegisterRestClient and @Scheduled cron and every expressions | `quarkus.checkAnnotations` | on | REST client and scheduler checks. |

## Permissions

The plugin always reads project files (`project.read`). Optional permissions are off until you allow them in Settings › Plugins › *Quarkus Tools*.

| Permission | Used for | Without it |
|---|---|---|
| `events` | Reading the project in the background: unknown-key, unassigned-key, required-key and REST-client checks; template inventories. | Local syntax, value and annotation checks still work. Cross-file checks stay quiet. |
| `fs.read:~/.m2/repository` | Reading a dependency JAR from your local Maven cache. | **Quarkus: Load Config Keys from a Dependency JAR…** fails. |
| `editor.read` | Reading the current file for the starter and code-lens commands. | **Quarkus: Create Maven/Qute Starter…** and **Quarkus: Show Endpoint or Schedule Details…** can't run. |
| `project.write` | Creating the starter files from **Quarkus: Create Maven/Qute Starter…**. | The command asks you to create the files with ⌥⏎ › **Create Quarkus Starter …** instead. |
| `process:mvn`, `process:gradle`, `process:quarkus` | Running Dev Mode. | The matching run configuration isn't offered. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| No completion or checks at all | The module must declare `io.quarkus` in its build file (a dependency, plugin or catalogue alias, not just a comment). Edits to the build file can take up to 30 seconds to be noticed. |
| Unknown-key warnings never appear | Allow `events`. They show only after the whole project has been read. Keys of extensions that aren't bundled are never flagged. |
| A key from an extension is missing | The bundled metadata covers about 170 extensions. Use **Quarkus: Load Config Keys from a Dependency JAR…** for others. |
| *Open the Maven project first.* | Open a file of the Maven project whose root `pom.xml` lists the dependency. |
| *The root pom.xml lists no JAR dependency with written-out coordinates.* | Only direct dependencies with written-out coordinates in the root `pom.xml` are listed. BOM-managed, property-based and Gradle dependencies aren't. |
| *No supported Quarkus config model in the selected JAR.* | The JAR has no `META-INF/quarkus-config-doc/quarkus-config-model.json`. Older layouts aren't supported. |
| The JAR read is denied | Allow `fs.read:~/.m2/repository` in Settings › Plugins › *Quarkus Tools*. |
| *Selected JAR must be a file of at most 8 MiB.* | The JAR is too large, or not in your Maven cache yet. Build the project once so Maven downloads it. |
| *Open a Java file or pom.xml of the project that should contain the starter first.* | Open a Java or XML file of the parent project, then run the starter command again. |
| *Use plain Maven coordinates, a lowercase folder name and a Quarkus 3 version.* | Use a dotted groupId, a lowercase folder name and a platform version starting with `3.`. |
| *Creating the starter needs the project.write permission.* | Allow `project.write` in Settings › Plugins › *Quarkus Tools*, or press ⌥⏎ in the same file within five minutes and choose **Create Quarkus Starter …**. |
| *… already exists or is open with unsaved changes.* | The starter never overwrites files. Choose another artifactId, or save or close the named file. |
| *Couldn't create … Already created: …* | Writing stopped midway. Remove the listed files, then run the command again. |
| **Create Quarkus Starter …** isn't offered | The plan expires after five minutes. Run **Quarkus: Create Maven/Qute Starter…** again from the same file. |
| *No starter created yet.* | Create the starter before running **Quarkus: Open Starter…**. |
| *This file has no Quarkus REST endpoint or @Scheduled method.* | Open the Java file with the endpoint or scheduled method, then run **Quarkus: Show Endpoint or Schedule Details…** again. |
| Dev Mode run configuration is missing | Allow `process:mvn` / `process:gradle` / `process:quarkus` and make sure the program is on `PATH`. |
| Dev Mode shows in a non-Quarkus project | Expected: run detectors match `pom.xml` or `build.gradle` by name only. |

## Limitations

- `./mvnw` and `./gradlew` can't be used for Dev Mode. The program must be on `PATH` or absolute.
- Map entries such as `quarkus.log.category."io.acme".level` aren't checked.
- Checks are text-based. There is no compiler, classpath or type resolution.
- No converters, runtime config sources, effective injected values or Java converter evaluation.
- No full dependency metadata; keys from extensions outside the bundled and selected set are unknown.
- Only part of the MicroProfile/Quarkus language-server feature set: no code lens beyond literal mappings, broader Java definition or diagnostics, or JAX-RS symbols.
- No in-IDE project wizard (use code.quarkus.io), debugger auto-attach to Dev Mode or run dashboard.
- No scoped usages or Rename. Keys with a `%profile.` prefix and `@ConfigMapping` members get no gutter icon.
- `@ConfigMapping` navigation is limited to same-file groups.
- Nonstandard, generated or test layouts aren't supported.
- No complete Qute type/member resolution and no template Rename.
- Prefixed Maven element layouts and Gradle beyond literal declarations aren't supported.
- No effective classpath, inherited profiles or runtime models.

## Technical details

<details>
<summary>Module detection, limits, caching and safety checks</summary>

**Module detection.** The plugin acts only in Quarkus modules, found through literal `io.quarkus` build declarations in the nearest `pom.xml`, `build.gradle` or `build.gradle.kts` up to the project root. A mere mention in a description, comment or unrelated string is quiet. Conventional Gradle catalogue aliases (`alias(libs.plugins.quarkus)`) are detected from exact `io.quarkus` plugin IDs; hyphen/underscore accessors are normalised; malformed TOML, ambiguous normalised aliases and custom catalogue declarations remain unknown. Module proof is cached for at most 30 seconds. Cache-only editor requests never refresh it, so a missed event can leave an up-to-30-second stale window before navigation and metadata go quiet until lifecycle discovery proves the module again. Build invalidation is processed first. Explicit build deletions record bounded in-memory tombstones (at most 1,024 tombstones and 64 directory proofs; overflow suppresses proofs until restart). Cached paths are rechecked against the current deepest host project owner, so a nested project's declarations cannot become outer-project facts.

**Bundled metadata.** Quarkus 3.40.1's own configuration metadata for about 170 of the most-used `io.quarkus` extensions (86 contribute keys: 2,797 properties), bundled in `data/quarkus-properties.tsv`. Regenerate with `python3 -I scripts/gen-metadata.py 3.40.1 data/quarkus-properties.tsv [extra-artifact-id…]`.

**Config checks.** Quoted map keys and keys of extensions that aren't bundled are never flagged as unknown. Unknown keys are flagged only inside a known group of two or more segments. Only 13 required keys (no default, not `Optional`) exist in the bundled metadata; values from system properties, other config sources or `-D` are invisible to the required-key check. Inputs over 128 KiB or config documents over 512 keys suppress diagnostics and inlays; results are capped at 500 diagnostics and 200 inlays. Unknown, dynamic and conflicting declarations produce no hover.

**Java source facts.** Text only, no type resolution. Java config facts need a literal framework-qualified annotation spelling or one exact matching import (or known same-package spelling) with no same-file shadow. `ConfigMapping` modifiers and naming strategies need their literal SmallRye namespace/enum owner. Mapping groups use unambiguous same-file lexical owners. Scalar metadata accepts exact ordinary unescaped string literals; compound names, escapes and text blocks stay unknown and mark the source model incomplete, so it cannot authorise absence claims. `Optional<T>` is unwrapped only with a literal `java.util.Optional` owner.

**Annotation checks.** The `@RestClient` interface must be found in the project with `@Path`. `@Scheduled` cron checks are quiet when `quarkus.scheduler.cron-type` is set anywhere. `every` is checked against the documented `Duration.parse` prefix rules (a bare number or `30ms` is invalid). A nonempty `cron` takes precedence over `every`. Values with `{...}` or `${...}` are skipped. The `${key}` reference note is controlled by `quarkus.checkUnassigned`.

**Unassigned keys.** Profiles are ignored. Quiet for `quarkus.*` keys, bundled keys with a default, and until the whole project is read. Sources checked: `application*` files, `microprofile-config.properties` and `.env`. System properties, `-D`, other config sources and `@ConfigMapping` members are invisible to it.

**JAX-RS checks.** Literal source only; cross-file checks run only in inventory-complete modules. Path variables may be `{x}` or `{x:regex}`. Dynamic values stay quiet.

**Qute template checks.** A warning appears only when the fresh source inventory proves no template file exists. Lookup honours default suffixes (`qute.html`, `qute.txt`, `html`, `txt`) and a literal `quarkus.qute.suffixes`. Quarkus has no template-root property; `template-path-exclude`, profiled, dynamic or YAML values stay quiet.

**Quick fixes.** **Add @RegisterRestClient** adds the import, writes the annotation fully qualified at the declaration, preserves imports and rechecks the unique fresh target. **Add key to application.properties** re-reads bounded target keys and refuses unsupported spellings or existing keys.

**Config key ↔ `@ConfigProperty` navigation.** Links a plain config leaf key with its one exact literal Java `@ConfigProperty(name = "…")` injection name. Annotation ownership needs the explicit MicroProfile import with no conflicting import or local type; dynamic, escaped, concatenated, generic-shadowed and unknown names stay quiet. Parent YAML nodes, profiles, named instances, derived `@ConfigMapping` member associations and placeholders are excluded. No anchors are registered for the host's automatic Rename. Source facts live 120 seconds. Bounds: 128 KiB input, 64 Java types, 128 bracket nesting, 512 declarations per file, 512 cached files and 4,096 candidate records per lookup. Every exact match in the module is returned, so a key injected in several places lists all of them. Gutter icons use the same cached facts: one marker per target on the key's line, at most 200 per line, and nothing while the cache is cold. A `@ConfigMapping` prefix links to cached keys that start with `prefix.`, never to derived member names. The gutter icon is the Quarkus logo from `icon.svg` with the white tile removed; its dark-theme variant draws the logo's near-black facets in white.

**`@Location` navigation.** Works on the exact unescaped `@Location` value of a non-static, source-proved Quarkus `Template` field. Only explicit portable ASCII `.html`/`.txt` paths below conventional `src/main/resources/templates` are linked: no suffix guessing, fragments, custom roots, implicit field naming or CheckedTemplate lookup. At most 256 template paths and 16 module inventories are warmed by events; inventories expire after 30 seconds. Provider requests perform no file discovery or reads.

**Code lenses.** Lenses and document symbols expose exact literal JAX-RS mappings and scheduler triggers. The lens action opens revalidated read-only declaration evidence. A single literal `@ApplicationPath` replaces `quarkus.rest.path`. Quiet when ambiguous or non-literal.

**Qute support.** Works within fresh, known, conventional Qute template modules. Static includes navigate to uniquely owned cached template files. Section snippets don't replace the host language. Missing-include notes acknowledge custom runtime locators. Ambiguous variants, overloads, dynamic paths and custom naming strategies stay unknown.

**Create empty Qute template.** Offered on a Java field with an explicitly imported or fully qualified `io.quarkus.qute.Template` type and an `io.quarkus.qute.Location` annotation containing one unescaped explicit relative `.html` or `.txt` string. Only conventional module `src/main/resources/templates` targets. Static fields, ambiguous or shadowed names, dynamic expressions and extensionless names stay quiet. The preview shows the exact target and content (empty, zero bytes), rechecks source, project, build text, target collision and parents, and returns one host-managed non-overwriting Create with Undo.

**Qute/JDT adapter.** The [adapter setup](adapters/README.md) supplies a broker for the official Qute Language Server and Eclipse JDT LS with the official Qute/MicroProfile JDT extensions: hash-pinned profiles, isolated per-session Java workspaces and exact native project settings. Only the exact `.qute.html` suffix is contributed automatically (HTML mode and grammar); generic HTML and Java are not reassigned. The optional JDT configuration overlay also adds saved and unsaved `application.yaml|yml`, `.env` and `config/application.properties|yaml` files as sources (bounded, inside the project root, no symlink escapes). Unsaved buffers override saved files. YAML is flattened with dotted keys, `[n]` list indexes, quoted keys and `%profile` top-level keys; anchors, flow, block scalars, tabs and multi-document files are refused.

**Dev Mode detectors.** A detector can only match marker files by name, so once the `process:` permission is granted the run configurations show in every Maven or Gradle project, Quarkus or not.

**Offline starter.** The artifactId must be a portable lowercase folder name; the Quarkus platform version is not fetched or resolved by the plugin. The confirmation names the folder and lists four files: `pom.xml`, `src/main/resources/application.properties`, `src/main/resources/templates/hello.txt` and a scope/provenance README. With `project.write`, the command checks every destination with `Project::exists` before writing any file, then writes them one by one; without it, the ⌥⏎ step returns four atomic non-overwriting Create operations without asking again. Existing destinations, open unsaved targets, changed source/version/owner and expired or replayed plans are refused. It only creates a vacant direct child of the existing project: no merge, source edit, wrapper, build, process or HTTP request. **Quarkus: Open Starter…** checks all four files and uses the ordinary project trust/open flow, with no build or automatic language-server settings. The scaffold has the platform BOM, the Quarkus Maven plugin, a direct `quarkus-qute` dependency and a literal text template. Version availability, JDK compatibility, dependency resolution and remote registry workflows are not verified.

**Dependency JAR models.** The command lists supported literal direct dependencies of the current project's root `pom.xml`, reviews the exact project/POM, GAV, declared scope and standard Maven-cache archive, then reads bytes. No Maven/Gradle/Java execution, cache scan, network or extraction. Only the modern `META-INF/quarkus-config-doc/quarkus-config-model.json` layout is supported: plain property names with declared types, scalar defaults, deprecation replacements and static enum hints; optional javadoc descriptions joined by exact `sourceType.sourceElementName`. `quarkus-extension.properties` deployment coordinates, `quarkus-config-roots.list` and older javadoc layouts are deliberately not converted into a catalogue. Conflicting names and unsupported structures fail without installing a partial replacement. The snapshot is retained ten minutes per project (up to eight exact project roots, session-only, invalidated by matching POM events). Maven scope is narrow: root direct literal GAVs, JAR type, compile/runtime/provided/test scope, no classifier; BOMs, property resolution, Gradle and transitives are gaps. POMs must be complete supported XML (256 KiB, 4,096 elements, nesting 32, 128 dependencies) with coherent namespaces. Archives are classic ZIP bounded at 8 MiB, 2,048 entries, 512 KiB per metadata entry and 100:1 expansion; encryption, ZIP64 and path tricks are refused.

**How it works.** External edits to a gitignored `.env` (the usual case) are picked up when you open or save it in the IDE; tracked `.env` files are watched. Current-text diagnostics, metadata hover and type inlays use API 1.3 on the current unsaved document. Activation and optional file events warm module, source/config and `.env` facts outside editor callbacks. Diagnostics, hover and inlays don't discover files, read `.env` or contact a server. Unknown-key, unassigned/required-key and REST-client absence checks need `events` and complete, usable caches; local value/syntax/annotation checks work on established bundled/current-text facts without it. Oversized cached source/config files mark absence facts incomplete; a denied or cold context stays quiet. Capabilities: required `project.read`; optional `events`, `fs.read:~/.m2/repository`, `editor.read`, `project.write`. Metadata and annotation parsing stay textual rather than compiler or classpath resolution.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
