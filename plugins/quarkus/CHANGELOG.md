# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/quarkus-v0.4.0...quarkus-v0.5.0) (2026-11-24)


### Features

* **spring-web,spring-beans,spring-boot,quarkus:** Search Everywhere tabs and console colours ([2913575](https://github.com/arnaud-ritti/openidea-plugins/commit/2913575c6cb69db741c0e9de4f5cf6b9d5bbe08a))


### Bug Fixes

* **symfony,spring-text:** keep index builds within the per-call CPU budget on large projects ([06744d3](https://github.com/arnaud-ritti/openidea-plugins/commit/06744d38e2528dd3314434980e77f7f353dd6f14))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/quarkus-v0.3.0...quarkus-v0.4.0) (2026-11-18)


### Features

* **quarkus:** create the starter directly when project.write is allowed ([5df90f3](https://github.com/arnaud-ritti/openidea-plugins/commit/5df90f3c1761a0e4503f9d658c42135c7e16b82e))
* **quarkus:** gutter links between config keys and @ConfigProperty injections ([53bd171](https://github.com/arnaud-ritti/openidea-plugins/commit/53bd17181005751ae49d288938ed7210f7390388))


### Bug Fixes

* brand logos for the related-files gutter icons ([10c28d6](https://github.com/arnaud-ritti/openidea-plugins/commit/10c28d68d95fbff873034c530bbc0a29401d9590))
* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* explain what to select when a row command runs from Search Everywhere ([47728bd](https://github.com/arnaud-ritti/openidea-plugins/commit/47728bdd035532879ddc1b6ea60357ca7b769c84))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* **quarkus:** activate only for Quarkus projects ([fd3a4fb](https://github.com/arnaud-ritti/openidea-plugins/commit/fd3a4fbe44cf50cb876d9a84a540563d0daabba2))
* **quarkus:** one-confirm starter, palette path for lens details, plain copy ([e37ebdf](https://github.com/arnaud-ritti/openidea-plugins/commit/e37ebdf5ed8697d678c4b1984dd78bda3275e56d))
* **quarkus:** plain wording in JAR metadata messages ([12a2ca4](https://github.com/arnaud-ritti/openidea-plugins/commit/12a2ca4c47f17eaa6593cf05c35b1a610b31ab09))
* **quarkus:** state the gitignored .env limit precisely, test late Quarkus dependency ([eb891e1](https://github.com/arnaud-ritti/openidea-plugins/commit/eb891e18be4a1f873de5f029b04381c62d8edb7b))
* remaining jargon in titles ([4edf7e8](https://github.com/arnaud-ritti/openidea-plugins/commit/4edf7e8ebe03a85b56445d5aebfcc60818d8bfa2))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* **spring-text:** share one XML walker, match drive letters case-insensitively ([61d354d](https://github.com/arnaud-ritti/openidea-plugins/commit/61d354d7122df0117b830a25290e76ceaa59d04f))
* tell users which permission to allow (part 1) ([a9565c7](https://github.com/arnaud-ritti/openidea-plugins/commit/a9565c76d8a6fa0e41a656082d635bb5c62b4556))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/quarkus-v0.2.0...quarkus-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* close round 5 gaps (Quarkus saved sources, Shell rename, Symfony go-to links, lockfile parsers, Initializr Maven) ([39404e6](https://github.com/arnaud-ritti/openidea-plugins/commit/39404e6b3b539c78586060bf4bedd249ba95e9a7))
* CRD pruning checks, Qute template warnings, Liquibase structure checks, JPA XML/inheritance checks, Symfony service/route references ([1c9a57d](https://github.com/arnaud-ritti/openidea-plugins/commit/1c9a57d89b6d3fd795f3de02ac076926f8fe385a))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Maven BOM metadata, oasdiff compat rules, Docker ARG scope, form/column checks, workspace chains, JAX-RS checks, view names, wiring registrations ([17b4876](https://github.com/arnaud-ritti/openidea-plugins/commit/17b48765dc99e46aa52915c5a4139d00a66a0ce2))
* **spring:** close another residual round across Data, Security, Initializr, Integration, Shell, JPA and Quarkus ([bfb4579](https://github.com/arnaud-ritti/openidea-plugins/commit/bfb45790361d4fafb2db3e42f5de9bd7094bff14))
* **spring:** close follow-up residuals across Spring, JPA, Flyway and Quarkus plugins ([d152eb1](https://github.com/arnaud-ritti/openidea-plugins/commit/d152eb1429e6f06237b3dd6c80c151e25e2010f9))


### Bug Fixes

* **deps:** build against OpenIDEA SDK v0.1.42 and require plugin API 1.5 ([4ec2afa](https://github.com/arnaud-ritti/openidea-plugins/commit/4ec2afa5da97997d15e0cab94f3cfcadbb89e375))
* verification pass against real sources (Yarn berry parser, WS void rule, CRD nulls, Twig falsy namespaces, GitLab draft positions) ([00b86ec](https://github.com/arnaud-ritti/openidea-plugins/commit/00b86ec4f920a18ef709a65a137ff09788186619))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/quarkus-v0.1.0...quarkus-v0.2.0) (2026-11-07)


### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **quarkus:** adopt OpenIDEA API 1.2 and add plugin icon ([0ad8886](https://github.com/arnaud-ritti/openidea-plugins/commit/0ad888687c65d73eb9e3af752d817420885aa2df))
* **quarkus:** implement the audit backlog packages ([75861af](https://github.com/arnaud-ritti/openidea-plugins/commit/75861af8f164d05414ba6cfcdd6621e41f4f0c1d))
* **quarkus:** implement the open audit rows of the v2 pass ([fb74202](https://github.com/arnaud-ritti/openidea-plugins/commit/fb74202b12af5ae7cfdcfff09fc17ad93cbece79))
