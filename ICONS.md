# Icons

Plugin icons (`plugins/<name>/icon.svg`) show the official full-colour logo of the technology a plugin supports on a white tile. Where several plugins share one logo, a small dark badge names the feature (all Spring plugins use the Spring leaf, as JetBrains does). Tool window icons (`icon` in `[[contributes.views]]`) use the same coloured logos at 16 px, except Symfony, whose black logo would vanish on a dark theme and stays a `#6C707E` glyph the IDE tints.

- **Sources:** logos come from [Iconify](https://iconify.design) sets `logos` (CC0 1.0: Quarkus, Spring, Kubernetes, GitLab, Laravel, Symfony, Docker, Git, GraphQL, OpenAPI), `devicon` (MIT: Flyway, Liquibase), `thesvg-color` (MIT: Spring Boot, Spring Security, dotenv) and `vscode-icons` (MIT: Twig). Single-colour glyphs in `#6C707E` come from [Simple Icons](https://simpleicons.org) 16.36.0 (CC0 1.0).
- **Trademarks:** the logos belong to their owners (Broadcom/VMware Spring, Linux Foundation Kubernetes, GitLab, Laravel, SensioLabs Symfony and Twig, Red Hat Quarkus, Redgate Flyway, Liquibase, OpenAPI Initiative, Docker, Git, GraphQL Foundation, dotenv). They identify the supported technology and don't imply endorsement.
- **Drawn here:** the tiles, the badges, and the pictograms of `jpa-assist` (database + JPA badge), `ideolog` (log file), `properties` (key/value rows) and `dependency-audit` (shield), and `tasks` (checklist), which have no official logo.
- **Blade:** no standalone Blade logo exists in these sets, so the icon is the Laravel logo with a `{ }` badge, drawn here.
- Icons are not taken from the JetBrains plugins that some plugins were ported or inspired from.

## Tool window icons after the window consolidation

- **GitLab:** `projects.svg`, `merge-requests.svg` and `merge-request.svg` are the GitLab fox at 75% with a small bottom-right glyph (list, two arrows, info), so the three windows differ in the stripe.
- **Spring:** `spring-web/icons/endpoints.svg` (globe), `spring-integration/icons/structure.svg` (branching boxes) and `spring-beans/icons/beans.svg` (bean) are the Spring leaf at 75% with a corner glyph, drawn here. Runtime and Initializr keep the plain leaf and the Spring Boot icon. Not yet looked at in the IDE.
- **Route methods:** `crates/ui-kit/icons/route-{get,post,put,delete,patch,other}.svg` (green, orange, blue, red, purple, grey rounded squares, under 1 KiB) are copied into `spring-web/icons/` and `laravel/icons/` and registered as `fileIcons` for the route list rows.
