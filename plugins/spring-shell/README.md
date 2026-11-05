# Spring Shell Tools

Spring Shell support for Java projects in OpenIDEA, for both annotation models:

- **Command index**: `@ShellMethod` methods (named by `key`, else by the method name in kebab-case: `addUser` → `add-user`) and `@Command` methods (the class `command` words followed by the method `command` words, or by the method name in kebab-case when the method has none).
- **Availability checks**: a `@ShellMethodAvailability` name that matches no command (of the class, else of the project) is an error, and so is an availability method (annotated, or named `<command method>Availability`) that doesn't return `Availability`. A `@CommandAvailability(provider = "x")` with no bean `x` in the project is a warning.
- **Duplicate commands**: a command name declared twice in the project is an error at each declaration.
- **Completion**: command names and `*` in `@ShellMethodAvailability("…")`, existing group names in `group = "…"` / `@ShellCommandGroup("…")`, `AvailabilityProvider` beans in `@CommandAvailability(provider = "…")`.
- **Navigation**: Go to Definition from an availability name to the command method and from a provider name to its bean; a gutter arrow on a command method opens its availability method or provider.

Matching is text-based. `@Command` only counts in files importing from `org.springframework.shell` (picocli has an annotation of the same name). Beans come from stereotype classes and `@Bean` methods of the project; providers are `@Bean` methods returning `AvailabilityProvider` or classes implementing it. Checks that need the whole project wait until the project has been read once.

Not ported: shell script files, an interactive command list, option value completion, `alias` names in the duplicate check, Kotlin.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
