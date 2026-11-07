# Spring Security Tools

Spring Security support for Java projects in OpenIDEA:

- **Spring Security 6 migration**:
  - a class extending `WebSecurityConfigurerAdapter` (removed: declare a `SecurityFilterChain` `@Bean`);
  - `@EnableGlobalMethodSecurity` (deprecated) → quick fix to `@EnableMethodSecurity`, dropping `prePostEnabled = true` and fixing the import;
  - `.antMatchers(`/`.mvcMatchers(` (removed) → quick fix to `requestMatchers`; `.regexMatchers(` is flagged without a fix;
  - `.authorizeRequests(` (deprecated) → quick fix to `authorizeHttpRequests`.
- **`ROLE_` prefix**: `hasRole('ROLE_ADMIN')`/`hasAnyRole(…)` in `@PreAuthorize`/`@PostAuthorize`/`@PreFilter`/`@PostFilter` and `.hasRole("ROLE_X")`/`.hasAnyRole(…)` calls. Spring adds the prefix itself; the quick fix removes it.
- **Plain-text passwords**: `NoOpPasswordEncoder.getInstance()` (warning), `{noop}` passwords passed to `.password(…)` (info).
- **Matcher order** (Java and Kotlin DSL, setting `springSecurity.checkMatchers`): a rule after `anyRequest()` in the same `authorizeHttpRequests` block (Spring Security rejects it at startup), and a narrower `requestMatchers` pattern shadowed by an earlier broader one (`/**` or `/x/**` before `/x/y`, same or unrestricted HTTP method; regex and dynamic patterns are skipped).
- **Debug logging** (setting `springSecurity.checkDebug`): `@EnableWebSecurity(debug = true)`, `web.debug(true)` in a `WebSecurityCustomizer` and `<debug/>` in the security XML namespace.
- **Completion** in the four method security annotations: `hasRole('…')`, `hasAnyRole(…)`, `hasAuthority('…')`, `hasAnyAuthority(…)`, `hasPermission(…, '…')`, `isAuthenticated()`, `isAnonymous()`, `isFullyAuthenticated()`, `isRememberMe()`, `permitAll`, `denyAll`, `principal`, `authentication`, `returnObject` (`@PostAuthorize`/`@PostFilter`), `filterObject` (`@PreFilter`/`@PostFilter`); inside `hasRole('…')`/`hasAnyRole('…')`, the role names the project already uses (`hasRole`/`hasAnyRole`, `@Secured`, `@RolesAllowed`, `ROLE_` stripped).

Text-based: Java and Kotlin sources (the same migration, `ROLE_` and password checks, including the Kotlin DSL's `authorizeRequests { … }`; completion stays Java-only) and Spring XML files. Calls match by method name; the matcher, `authorizeRequests` and `hasRole`/`hasAnyRole` call checks only run in files that import from `org.springframework.security`.

- **Security XML namespace** (`http://www.springframework.org/schema/security`, any prefix): `<intercept-url access>` expressions (unbalanced quotes/parentheses, `hasRole()` without argument, unknown `has…`/`is…` function names, redundant `ROLE_`), duplicate `pattern`s within one `<http>`, a `/**` catch-all placed before more specific patterns (not reported when the catch-all has a `method` or `servlet-path`, or `<http>` sets a regex `request-matcher` or a `request-matcher-ref`), `use-expressions="false"` and `<global-method-security>` (deprecated; use `<method-security>`). `unknown-security-function` is a warning only and can be a false positive for a custom `SecurityExpressionRoot` (methods such as `isOwner(#id)`); there is nothing to configure, ignore it. Plain role lists are not checked when `use-expressions="false"`.

- **Bean references** (Info, setting `springSecurity.checkBeanReferences`): `@WithUserDetails(userDetailsServiceBeanName = "…")`, `@bean.method()` in `@PreAuthorize`/`@PostAuthorize`/`@PreFilter`/`@PostFilter` expressions, and the `ref` attributes of the security XML namespace (`user-service-ref`, `authentication-manager-ref`, `entry-point-ref`, handler refs, `<password-encoder ref>`, `<jdbc-user-service data-source-ref>`) that no bean of the project answers. Beans are `@Component`-family classes, `@Bean` methods, repository interfaces and XML bean ids; beans of libraries and auto-configuration are invisible, so it stays quiet until the whole project was read and for a few well-known names.
- **More security XML checks**: required attributes (`<intercept-url pattern|request-matcher-ref>`, `<user name>`, `<password-encoder ref|hash>`, `<jdbc-user-service data-source-ref>`) and values of `create-session`, `request-matcher`, `requires-channel`, `method`, `session-fixation-protection` and `<global-method-security mode>`, from a hard-coded table.
- **Wider XML tables**: `true`/`false` values of `<http>` (`auto-config`, `once-per-request`, `disable-url-rewriting`, `servlet-api-provision`, `jaas-api-provision`), `<method-security>`, `<csrf>`, `<headers>`, `<anonymous>`, `<logout>`, `<form-login>`, `<concurrency-control>`, `<authentication-manager>`; `enabled`/`disabled` annotation switches of `<global-method-security>`; required `ref` of `<custom-filter>`; and bean references of `<cors>`, `<csrf>`, `<remember-me>`, `<concurrency-control>`, `<session-management>` and more `<http>` attributes. Written by hand from the schema, only for elements and attributes known completely; other elements are not checked.
- **Self-invocation** (opt-in, `springSecurity.checkSelfInvocation`, default off): a call from a method of a Spring bean class to another method of the same class that carries `@PreAuthorize`, `@PostAuthorize`, `@PreFilter` or `@PostFilter` skips the security proxy. Wrong with AspectJ mode, hence off by default. Java only.
- **Matcher navigation**: Go to Definition on a pattern of `requestMatchers(…)`, `securityMatcher(…)` (an `HttpMethod.X` argument narrows the verb) and on `<intercept-url pattern>` lists the controller mappings the Ant pattern covers. Mappings are those with literal paths in `@Controller`/`@RestController` classes.

Not ported: SpEL language support beyond completion and these checks (highlighting, navigation), OAuth2/SAML configuration assistance, quick fixes for the XML checks.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
