# Changelog

## [0.4.1](https://github.com/arnaud-ritti/openidea-plugins/compare/dotenv-v0.4.0...dotenv-v0.4.1) (2026-11-24)


### Bug Fixes

* **dotenv,liquibase,symfony,spring-integration,spring-ws,spring-shell:** keep the remaining scans within the per-call CPU budget ([a732240](https://github.com/arnaud-ritti/openidea-plugins/commit/a7322404f1b4285febd6f7d08b329d2059ee3f2f))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/dotenv-v0.3.0...dotenv-v0.4.0) (2026-11-18)


### Features

* **dotenv,spring-osgi:** settings to turn checks off ([63a8525](https://github.com/arnaud-ritti/openidea-plugins/commit/63a852568124c8f5e29de7edd0f5cdae4e788a73))
* **dotenv:** gutter links from env keys to their usages and other env files ([c750d47](https://github.com/arnaud-ritti/openidea-plugins/commit/c750d477b409f940728993d5647ec0c46abca430))


### Bug Fixes

* brand logos for the related-files gutter icons ([10c28d6](https://github.com/arnaud-ritti/openidea-plugins/commit/10c28d68d95fbff873034c530bbc0a29401d9590))
* correct manifest mismatches and unsafe defaults ([d555f8d](https://github.com/arnaud-ritti/openidea-plugins/commit/d555f8d68e191d63e2b0d69e5f52bcdc26561c0d))
* **dotenv:** a truncated inventory stays incomplete after updates ([cf9adf1](https://github.com/arnaud-ritti/openidea-plugins/commit/cf9adf14e09ebb576ba3ab5f6333fb2453d7f928))
* **dotenv:** keep walk bookkeeping consistent with events; queue over-budget event paths ([b1c4c7a](https://github.com/arnaud-ritti/openidea-plugins/commit/b1c4c7a96bf9369f5cd22a7c819648ab78bab2e3))
* **dotenv:** restore completeness when blocking files are fixed, drop facts of a removed folder ([dc0c7d2](https://github.com/arnaud-ritti/openidea-plugins/commit/dc0c7d2e2e9b298940f22ad8a3c65404ab313aa4))
* **dotenv:** resume a deadline-cut walk from its cursor instead of re-walking ([5b2004a](https://github.com/arnaud-ritti/openidea-plugins/commit/5b2004a522ad25b927e14ce3bc5a9a4e6f44bfb2))
* **dotenv:** resume a deadline-cut walk, warn when file watching is denied, never mask empty values ([323c7c9](https://github.com/arnaud-ritti/openidea-plugins/commit/323c7c91285b5eca6d7d65356b57c027f5dc31b8))
* **dotenv:** set hard_incomplete when discovery is truncated ([f948e0b](https://github.com/arnaud-ritti/openidea-plugins/commit/f948e0bd9bc32ab76266bf98ebf3bb0ca13f47e9))
* **dotenv:** show unmasked values in a picker, not a notification ([87bb0c2](https://github.com/arnaud-ritti/openidea-plugins/commit/87bb0c241f9e14f9f533fb43c5e7016fbde7d423))
* **dotenv:** stop re-walking on every save; track external file changes ([c596131](https://github.com/arnaud-ritti/openidea-plugins/commit/c596131a08cabbb18b9e1cc96f106d2721ce6a38))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* prefix the .env and Spring WS command titles ([8b1aeb2](https://github.com/arnaud-ritti/openidea-plugins/commit/8b1aeb2a9dfa66d1352655909397e5b87c2d5c68))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/dotenv-v0.2.0...dotenv-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([08db51a](https://github.com/arnaud-ritti/openidea-plugins/commit/08db51a54c99b3b4ae414ab790be2a0783add3c7))
* close follow-up residuals in dotenv, Properties, Symfony and Twig plugins ([9b7041f](https://github.com/arnaud-ritti/openidea-plugins/commit/9b7041fc978120f694c424b91be6c629863bda89))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Kotlin/XML table mappings, Compose and feature checks, Compose ownership, Helm scope, Twig delimiters and variables ([c3f2239](https://github.com/arnaud-ritti/openidea-plugins/commit/c3f22390f2d6a08a7f4b5f6192343f71b09f7606))
* Maven BOM metadata, oasdiff compat rules, Docker ARG scope, form/column checks, workspace chains, JAX-RS checks, view names, wiring registrations ([17b4876](https://github.com/arnaud-ritti/openidea-plugins/commit/17b48765dc99e46aa52915c5a4139d00a66a0ce2))


### Bug Fixes

* verify Liquibase and Spring context rules against sources, tidy leftovers ([be4e938](https://github.com/arnaud-ritti/openidea-plugins/commit/be4e938ec5a803b82f22bdb65055a7e662daa5a2))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/dotenv-v0.1.0...dotenv-v0.2.0) (2026-11-07)


### Features

* **dotenv:** adopt OpenIDEA API 1.2 and add plugin icon ([7f0cb23](https://github.com/arnaud-ritti/openidea-plugins/commit/7f0cb233a45f7a8e3f36877e75b5ed8b4f7f5e7b))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
