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

Text-based, Java only. Calls match by method name; the matcher, `authorizeRequests` and `hasRole`/`hasAnyRole` call checks only run in files that import from `org.springframework.security`.

Not ported: the XML security namespace, SpEL language support beyond completion (highlighting, validation, navigation), OAuth2/SAML configuration assistance, the Kotlin DSL.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
