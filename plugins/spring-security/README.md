# Spring Security Tools

Spring Security support for Java projects in OpenIDEA:

- **Spring Security 6 migration**:
  - a class extending `WebSecurityConfigurerAdapter` (removed: declare a `SecurityFilterChain` `@Bean`);
  - `@EnableGlobalMethodSecurity` (deprecated) → quick fix to `@EnableMethodSecurity`, dropping `prePostEnabled = true` and fixing the import;
  - `.antMatchers(`/`.mvcMatchers(` (removed) → quick fix to `requestMatchers`; `.regexMatchers(` is flagged without a fix;
  - `.authorizeRequests(` (deprecated) → quick fix to `authorizeHttpRequests`.
- **`ROLE_` prefix**: `hasRole('ROLE_ADMIN')`/`hasAnyRole(…)` in `@PreAuthorize`/`@PostAuthorize`/`@PreFilter`/`@PostFilter` and `.hasRole("ROLE_X")`/`.hasAnyRole(…)` calls. Spring adds the prefix itself; the quick fix removes it.
- **Plain-text passwords**: `NoOpPasswordEncoder.getInstance()` (warning), `{noop}` passwords passed to `.password(…)` (info).
- **Completion** in the four method security annotations: `hasRole('…')`, `hasAnyRole(…)`, `hasAuthority('…')`, `hasAnyAuthority(…)`, `hasPermission(…, '…')`, `isAuthenticated()`, `isAnonymous()`, `isFullyAuthenticated()`, `isRememberMe()`, `permitAll`, `denyAll`, `principal`, `authentication`, `returnObject` (`@PostAuthorize`/`@PostFilter`), `filterObject` (`@PreFilter`/`@PostFilter`); inside `hasRole('…')`/`hasAnyRole('…')`, the role names the project already uses (`hasRole`/`hasAnyRole`, `@Secured`, `@RolesAllowed`, `ROLE_` stripped).

Text-based: Java and Kotlin sources (the same migration, `ROLE_` and password checks, including the Kotlin DSL's `authorizeRequests { … }`; completion stays Java-only) and Spring XML files. Calls match by method name; the matcher, `authorizeRequests` and `hasRole`/`hasAnyRole` call checks only run in files that import from `org.springframework.security`.

- **Security XML namespace** (`http://www.springframework.org/schema/security`, any prefix): `<intercept-url access>` expressions (unbalanced quotes/parentheses, `hasRole()` without argument, unknown `has…`/`is…` function names, redundant `ROLE_`), duplicate `pattern`s within one `<http>`, a `/**` catch-all placed before more specific patterns, `use-expressions="false"` and `<global-method-security>` (deprecated; use `<method-security>`). `unknown-security-function` is a warning only and can be a false positive for a custom `SecurityExpressionRoot` (methods such as `isOwner(#id)`); there is nothing to configure, ignore it. Plain role lists are not checked when `use-expressions="false"`.

Not ported: SpEL language support beyond completion and these checks (highlighting, navigation), OAuth2/SAML configuration assistance, quick fixes for the XML checks.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
