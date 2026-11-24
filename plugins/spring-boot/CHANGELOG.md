# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-boot-v0.4.0...spring-boot-v0.5.0) (2026-11-24)


### Features

* **spring-web,spring-beans,spring-boot,quarkus:** Search Everywhere tabs and console colours ([2913575](https://github.com/arnaud-ritti/openidea-plugins/commit/2913575c6cb69db741c0e9de4f5cf6b9d5bbe08a))


### Bug Fixes

* **symfony,spring-text:** keep index builds within the per-call CPU budget on large projects ([06744d3](https://github.com/arnaud-ritti/openidea-plugins/commit/06744d38e2528dd3314434980e77f7f353dd6f14))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-boot-v0.3.0...spring-boot-v0.4.0) (2026-11-18)


### Features

* commands and menu entries for toolbar-only actions ([940dcf2](https://github.com/arnaud-ritti/openidea-plugins/commit/940dcf2ae8ebff44ec7b104af38b7559eb86bff9))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* precise permission mapping and command fallbacks ([a46fee8](https://github.com/arnaud-ritti/openidea-plugins/commit/a46fee83335d5cbd91a15ee1739d0f1517daef63))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* **spring-boot:** Find Property only in .properties editors, map denials ([0fb54a0](https://github.com/arnaud-ritti/openidea-plugins/commit/0fb54a00ce2e7de82f006859dbecdc88b4b0b0bd))
* **spring-boot:** flat YAML logging levels, nested import profiles, FQN enum match ([d56c86d](https://github.com/arnaud-ritti/openidea-plugins/commit/d56c86d91909aec1b31f84b20a2540508d8f21e9))
* **spring-boot:** one live index for providers, correct enum checks, JAR import fixes ([15e004c](https://github.com/arnaud-ritti/openidea-plugins/commit/15e004cbfaf2f967c64088fec7be14fad11a09b1))
* **spring-boot:** use spring-text balanced in rename; no simple-name enum fallback for package-qualified types ([47d9a3f](https://github.com/arnaud-ritti/openidea-plugins/commit/47d9a3f8acb54bc7deeddeaa10cfcad23a5ebd24))
* **spring-text:** shared tolerant XML validator, Windows-aware scope and build detection ([c8b343c](https://github.com/arnaud-ritti/openidea-plugins/commit/c8b343ccf5939596fd27c54b7ad6660105d89db0))
* tell users which permission to allow (part 1) ([a9565c7](https://github.com/arnaud-ritti/openidea-plugins/commit/a9565c76d8a6fa0e41a656082d635bb5c62b4556))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-boot-v0.2.0...spring-boot-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* close bounded Devcontainer, Flyway, Boot, Twig, Batch and Runtime residual slices ([e8b6bb5](https://github.com/arnaud-ritti/openidea-plugins/commit/e8b6bb515cd3266490b081421eb889191a8b06a4))
* close queued follow-ups (twig.paths, Boot profile includes, Shell inheritance, XPath prefixes, Symfony time events) ([0bbd769](https://github.com/arnaud-ritti/openidea-plugins/commit/0bbd769278d0587c93690a0c0183e78fc4f48649))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Maven BOM metadata, oasdiff compat rules, Docker ARG scope, form/column checks, workspace chains, JAX-RS checks, view names, wiring registrations ([17b4876](https://github.com/arnaud-ritti/openidea-plugins/commit/17b48765dc99e46aa52915c5a4139d00a66a0ce2))
* **spring:** close follow-up residuals across Spring, JPA, Flyway and Quarkus plugins ([d152eb1](https://github.com/arnaud-ritti/openidea-plugins/commit/d152eb1429e6f06237b3dd6c80c151e25e2010f9))
* **spring:** Gradle cache metadata, WSDL QNames, effective contexts, Security verification fixes, HTTP exchange clients, MVC XML imports ([f371c2f](https://github.com/arnaud-ritti/openidea-plugins/commit/f371c2f5717aa806725da24e646300d8401c4ef6))


### Bug Fixes

* **deps:** build against OpenIDEA SDK v0.1.42 and require plugin API 1.5 ([4ec2afa](https://github.com/arnaud-ritti/openidea-plugins/commit/4ec2afa5da97997d15e0cab94f3cfcadbb89e375))
* verify Liquibase and Spring context rules against sources, tidy leftovers ([be4e938](https://github.com/arnaud-ritti/openidea-plugins/commit/be4e938ec5a803b82f22bdb65055a7e662daa5a2))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-boot-v0.1.0...spring-boot-v0.2.0) (2026-11-07)


### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **spring-boot:** adopt OpenIDEA API 1.2 and add plugin icon ([bff9a54](https://github.com/arnaud-ritti/openidea-plugins/commit/bff9a546c68cb96f9243da0ddb09b9dddad0aa8f))
* **spring-boot:** implement the audit backlog packages ([3b4f24e](https://github.com/arnaud-ritti/openidea-plugins/commit/3b4f24e2792f69900bab193d37645d893fb3126a))
* **spring-boot:** implement the open audit rows of the v2 pass ([8bd2f71](https://github.com/arnaud-ritti/openidea-plugins/commit/8bd2f710fd8d52b3484ce1dd42c04aa7937f5148))


### Bug Fixes

* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **spring-boot:** do not split placeholder defaults into list items ([9e6b3e5](https://github.com/arnaud-ritti/openidea-plugins/commit/9e6b3e57e6598b478ddae50c615352079f56fa36))
* **spring-boot:** generate a plain class in the ConfigurationProperties Kotlin template ([a354686](https://github.com/arnaud-ritti/openidea-plugins/commit/a35468695780f92dd3074606e7606742fa5a6b29))
