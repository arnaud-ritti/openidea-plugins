# Spring Initializr Launcher

Starts Spring Boot projects from OpenIDEA through a Spring Initializr server:

- Spring Initializr tool window (right): server URL (default `https://start.spring.io`), build (Maven, Gradle - Groovy, Gradle - Kotlin), language, Spring Boot version (blank for the server's default), group, artifact, name and package name (both follow the artifact until you change them; the package is `group.artifact` lower-cased without `-`), description, packaging, Java version and dependencies (comma-separated ids such as `web,data-jpa`). The form is kept in the plugin's storage, and Reset restores the defaults.
- Generate Project opens `<server>/starter.zip?…` in the browser, which downloads the project zip. Explore in Browser opens the server's page (`<server>/#!…`) with the same options. Blank options are left out. The status line says what's missing (an http(s) server URL, a group or an artifact).
- Generate with Spring Boot CLI opens an IDE terminal running `spring init` with the form's build, type, language, Boot and Java versions, packaging, group, artifact, name, description, package name and dependencies, plus `--extract` into a directory named after the artifact (in the terminal's working directory). It needs the optional `process:spring` capability and the Spring Boot CLI on the PATH; without the capability, a notification says so and the browser download runs instead. Group, artifact, name, versions and package name are checked against letters, digits, `.`, `_` and `-`, and the button reports a refusal otherwise. Values reach `spring init` as separate program arguments, never through a shell line, so none can inject a command. The plugin cannot see the CLI's result or open the generated project: OpenIDEA has no open-project API, so open the directory yourself.
- The Spring Initializr: Open start.spring.io command opens the site.

- Pick Spring Boot Version (Project section) lists the server's Boot versions in a picker and fills the form.
- The Dependencies section (toolbar button "Dependencies (N)" next to "Project") browses the server's catalogue as a tree. The first node, Selected (N), lists the chosen ids (name and description from the catalogue, the raw id when unknown); below it come the catalogue categories, collapsed, with a check on chosen dependencies. Double-click or Enter on a dependency adds or removes it; the chosen ids stay in the form's `dependencies` field, which you can still edit in the Project section. Search... filters by name and description (case-insensitive, categories open, "N matches" in the status line) until Clear search; Clear all asks first; Refresh reloads the catalogue. Dependencies whose `compatibilityRange` excludes the current Spring Boot version (the form's version, or the server's default when blank) are hidden; a chosen one stays under Selected marked "(not compatible with <version>)". The Generate buttons work in both sections.
- The catalogue comes from `https://start.spring.io/metadata/client`, needs the optional `net:start.spring.io` capability and is fetched once on first opening of the section. For any other server, or without the capability, the tree shows a message and Selected keeps working; type the ids in the Project section.
- Spring Initializr: Add Spring Boot Starters (Tools menu) picks starters and inserts them into the open `pom.xml` or `build.gradle(.kts)`; it needs `editor.read` and `editor.write` and the picker's network capability.

Generating the project is done by the browser or the CLI, not by the plugin; only the Dependencies section, Pick Spring Boot Version and Add Starters make requests, and only to start.spring.io.

Not ported: creating or importing the project inside the IDE (OpenIDEA has no new-project API, so unzip the project and open it yourself); the dependency list and Boot version picker for servers other than start.spring.io (the manifest allows one host).

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
