# Spring Initializr Launcher

Starts Spring Boot projects from OpenIDEA through a Spring Initializr server:

- Spring Initializr tool window (right): server URL (default `https://start.spring.io`), build (Maven, Gradle - Groovy, Gradle - Kotlin), language, Spring Boot version (blank for the server's default), group, artifact, name and package name (both follow the artifact until you change them; the package is `group.artifact` lower-cased without `-`), description, packaging, Java version and dependencies (comma-separated ids such as `web,data-jpa`). The form is kept in the plugin's storage, and Reset restores the defaults.
- Generate Project opens `<server>/starter.zip?…` in the browser, which downloads the project zip. Explore in Browser opens the server's page (`<server>/#!…`) with the same options. Blank options are left out. The status line says what's missing (an http(s) server URL, a group or an artifact).
- Generate with Spring Boot CLI opens an IDE terminal running `spring init` with the form's build, type, language, Boot and Java versions, packaging, group, artifact, name, description, package name and dependencies, plus `--extract` into a directory named after the artifact (in the terminal's working directory). It needs the optional `process:spring` capability and the Spring Boot CLI on the PATH; without the capability, a notification says so and the browser download runs instead. Group, artifact, name, versions and package name are checked against letters, digits, `.`, `_` and `-`, and the button reports a refusal otherwise. Values reach `spring init` as separate program arguments, never through a shell line, so none can inject a command. The plugin cannot see the CLI's result or open the generated project: OpenIDEA has no open-project API, so open the directory yourself.
- The Spring Initializr: Open start.spring.io command opens the site.

- Pick Spring Boot Version and Pick Dependencies buttons list the server's Boot versions and dependencies in a picker and fill the form. They fetch `https://start.spring.io/metadata/client` and need the optional `net:start.spring.io` capability; for any other server URL they warn and you type the values.
- Spring Initializr: Add Spring Boot Starters (Tools menu) picks starters and inserts them into the open `pom.xml` or `build.gradle(.kts)`; it needs `editor.read` and `editor.write` and the picker's network capability.

Generating the project is done by the browser or the CLI, not by the plugin; only the pick lists make requests, and only to start.spring.io.

Not ported: creating or importing the project inside the IDE (OpenIDEA has no new-project API, so unzip the project and open it yourself); pick lists for servers other than start.spring.io (the manifest allows one host).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
