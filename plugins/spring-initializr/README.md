# Spring Initializr Launcher

Starts Spring Boot projects from OpenIDEA through a Spring Initializr server:

- Spring Initializr tool window (right): server URL (default `https://start.spring.io`), build (Maven, Gradle - Groovy, Gradle - Kotlin), language, Spring Boot version (blank for the server's default), group, artifact, name and package name (both follow the artifact until you change them; the package is `group.artifact` lower-cased without `-`), description, packaging, Java version and dependencies (comma-separated ids such as `web,data-jpa`). The form is kept in the plugin's storage, and Reset restores the defaults.
- Generate Project opens `<server>/starter.zip?…` in the browser, which downloads the project zip. Explore in Browser opens the server's page (`<server>/#!…`) with the same options. Blank options are left out. The status line says what's missing (an http(s) server URL, a group or an artifact).
- The Spring Initializr: Open start.spring.io command opens the site.

The plugin makes no network requests itself: the browser talks to the server.

Not ported: creating or importing the project inside the IDE (OpenIDEA has no new-project API, so unzip the project and open it yourself), fetching the server's dependency catalogue and Boot versions for pick lists.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
