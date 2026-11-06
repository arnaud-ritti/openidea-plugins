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

The IDE itself provides Go to Definition and Find Usages between keys and Java code.

Where keys come from:

- Quarkus 3.40.1's own configuration metadata for about 170 of the most-used `io.quarkus` extensions (86 of them contribute their own keys: 2,797 properties), bundled in `data/quarkus-properties.tsv` (Apache-2.0, see `NOTICE`). Regenerate it with `python3 -I scripts/gen-metadata.py 3.40.1 data/quarkus-properties.tsv [extra-artifact-id…]`; the extension list is `EXTENSIONS` in the script, ids without metadata are reported and skipped.
- The MicroProfile Config keys of the original plugin's static metadata.
- `@ConfigProperty` and `@ConfigMapping` declarations in the project's Java sources.

The plugin reads source text only. Extensions outside the bundled set and the metadata inside dependency jars are unknown to it.

Dev Mode run configurations:

- They need `mvn`, `gradle` or `quarkus` on `PATH`, and each is offered only once the matching `process:` capability is granted.
- A detector can only match marker files by name, so they show in every Maven or Gradle project once granted, Quarkus or not.
- `./mvnw` and `./gradlew` can't be run (a `process:` program must be on `PATH` or absolute).

Checks (each group has a setting): duplicate keys, lines without a separator and empty values (`quarkus.checkSyntax`); values that are not a number, boolean, enum constant, duration or memory size (`quarkus.checkValues`); `${...}` expressions naming nothing defined in the file, the project, the environment or the system properties (`quarkus.checkExpressions`).

Not ported: the project-wide missing-required-property check (the bundled metadata does not say which properties are required), Java-side MicroProfile/Quarkus language server features (hover, go to definition, code lens, inlay hints, JAX-RS symbols, Java diagnostics and quick fixes), Qute support, the in-IDE project wizard (use code.quarkus.io), debugger auto-attach to Dev Mode and the run dashboard, named-map entries in completion (`quarkus.datasource."name".*`), and keys from extensions outside the bundled set. See `LIMITATIONS.md`.

Source code: this plugin's source repository, directory `plugins/quarkus`.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
