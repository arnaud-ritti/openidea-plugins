# Quarkus Tools

Ported from Red Hat's Quarkus Tools for IntelliJ (EPL-2.0), `com.redhat.devtools.intellij.quarkus` 2.6.1. Completion and checks for Quarkus configuration in OpenIDEA, plus Dev Mode run configurations:

- Key completion in `application*.properties`, `application*.yaml|yml` and `META-INF/microprofile-config.properties`: full dotted keys in `.properties`, the next segment under the current parent in YAML. Each item shows the property's type, and its documentation gives the description, the default and any deprecation.
- Value completion: enum values, `true`/`false` and log levels.
- Profiles: `%dev.`, `%test.` and `%prod.` are offered at the start of a key. Profiled keys (`%dev.quarkus.http.port`, and YAML under `"%dev":`) complete and are checked as their base key.
- Project keys: `@ConfigProperty(name, defaultValue)` on fields and parameters, and `@ConfigMapping(prefix)` interfaces (`@WithName`, `@WithParentName`, `@WithDefault`, `namingStrategy`, `Optional<T>`, nested interfaces of the same file).
- Warnings: deprecated keys, and unknown keys inside a known group of two or more segments (`quarkus.http.portt`). Quoted map keys (`quarkus.log.category."io.acme".level`) and keys of extensions that aren't bundled are never flagged. Unknown-key warnings appear only after the whole project index has been read.
- Quarkus: Open code.quarkus.io (Find Action) opens the project generator in the browser.
- Dev Mode run configurations: `mvn quarkus:dev`, `gradle quarkusDev` and `quarkus dev`.

The plugin acts only in Quarkus modules, found by an `io.quarkus` mention in the nearest `pom.xml`, `build.gradle` or `build.gradle.kts` up to the project root. The answer is cached per directory, so after adding `io.quarkus` to a build file, reload the plugin. Gradle builds that declare the Quarkus plugin only in `gradle/libs.versions.toml` (`alias(libs.plugins.quarkus)`) aren't detected.

Go to Declaration works between a key in `application*.properties|yaml|yml` or `microprofile-config.properties` and the Java code that declares it, both ways: the literal of `@ConfigProperty(name = "…")` and the accessor of a `@ConfigMapping` member (prefix, `@WithName`, `@WithParentName`, nested interfaces of the same file), with a gutter marker on both sides. Keys match as written without the profile (no relaxed names); a key below a map member (`server.labels.a`) and `@ConfigProperty` names built by concatenation are not linked. Properties files are indexed only in Quarkus modules. Qute templates have no language support: a plugin language for `*.qute.html` would replace the built-in HTML highlighting of those files and `.html` itself cannot be claimed.

Where keys come from:

- Quarkus 3.40.1's own configuration metadata for about 170 of the most-used `io.quarkus` extensions (86 of them contribute their own keys: 2,797 properties), bundled in `data/quarkus-properties.tsv` (Apache-2.0, see `NOTICE`). Regenerate it with `python3 -I scripts/gen-metadata.py 3.40.1 data/quarkus-properties.tsv [extra-artifact-id…]`; the extension list is `EXTENSIONS` in the script, ids without metadata are reported and skipped.
- The MicroProfile Config keys of the original plugin's static metadata.
- `@ConfigProperty` and `@ConfigMapping` declarations in the project's Java sources.

The plugin reads source text only. Extensions outside the bundled set and the metadata inside dependency jars are unknown to it.

Dev Mode run configurations:

- They need `mvn`, `gradle` or `quarkus` on `PATH`, and each is offered only once the matching `process:` capability is granted.
- A detector can only match marker files by name, so they show in every Maven or Gradle project once granted, Quarkus or not.
- `./mvnw` and `./gradlew` can't be run (a `process:` program must be on `PATH` or absolute).

Checks (each group has a setting): duplicate keys, lines without a separator and empty values (`quarkus.checkSyntax`); values that are not a number, enum constant, log level, duration or memory size, never booleans (`quarkus.checkValues`); `${...}` expressions that look like a typo of a key defined in the same `application.properties|yaml|yml` (never in profile files, never for names that could come from other files, the environment or `-D`; `quarkus.checkExpressions`). An empty value is only a hint, for a property whose metadata documents a default (Quarkus treats it as unset); other empty values are not reported.

Named instances: a key like `quarkus.datasource."users".jdbc.url` is completed (type, default, documentation, values) and checked as `quarkus.datasource.jdbc.url`, for the roots the metadata lists as named (datasource, hibernate-orm, flyway, liquibase, mongodb, redis, oidc, oidc-client, mailer and a few more). Typing the name after the quote and a dot completes the rest of the key; the name itself is never suggested. Map entries such as `quarkus.log.category."io.acme".level` are still not checked.

Java sources (text only, no type information): `@ConfigProperty(name = "` and `@ConfigMapping(prefix = "` complete keys and prefixes from the bundled metadata and the project. `quarkus.checkAnnotations` (on) reports an `@RestClient` injection whose interface, found in the project with `@Path`, lacks `@RegisterRestClient`, and `@Scheduled` with a `cron` of other than 6 or 7 fields (quiet when `quarkus.scheduler.cron-type` is set anywhere), a malformed `every`, both or neither of `cron` and `every`; values with `{...}` or `${...}` are skipped. `quarkus.checkUnassigned` (off) flags a `@ConfigProperty(name = "x")` with no `defaultValue`, not `Optional`, whose key no `application*` file, `microprofile-config.properties` or `.env` sets (profiles ignored; quiet for `quarkus.*` keys, bundled keys with a default and until the whole project is read; system properties, `-D`, other config sources and `@ConfigMapping` members are invisible to it). Quick fixes: Add defaultValue (by the field's type) and Add the key to the nearest existing `application.properties`.

Required keys (off by default, `quarkus.checkRequired`): an Info note on a key whose sibling is set while a key the metadata lists as required (no default, not `Optional`) is missing from every `application*` file and `.env`, for example `quarkus.tls.key-store.jks.path` next to `...jks.password`. Only 13 such keys exist in the bundled metadata, since most mandatory settings of Quarkus extensions are `Optional` in the model; values from system properties, other config sources or `-D` are invisible to the check, which is why it is opt-in.

Not ported: Java-side MicroProfile/Quarkus language server features (hover, go to definition, code lens, inlay hints, JAX-RS symbols and the remaining Java diagnostics), Qute support, the in-IDE project wizard (use code.quarkus.io), debugger auto-attach to Dev Mode and the run dashboard, and keys from extensions outside the bundled set. See `LIMITATIONS.md`.

Source code: this plugin's source repository, directory `plugins/quarkus`.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
