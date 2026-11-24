# Changelog

## [0.4.1](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-runtime-v0.4.0...spring-runtime-v0.4.1) (2026-11-24)


### Bug Fixes

* **symfony,spring-text:** keep index builds within the per-call CPU budget on large projects ([06744d3](https://github.com/arnaud-ritti/openidea-plugins/commit/06744d38e2528dd3314434980e77f7f353dd6f14))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-runtime-v0.3.0...spring-runtime-v0.4.0) (2026-11-18)


### Features

* **spring-runtime:** Open Settings button in the empty view ([f793064](https://github.com/arnaud-ritti/openidea-plugins/commit/f793064bece508243275c387298b77a80df519d7))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* **spring-runtime:** hide views outside JVM build projects, plain auto-refresh wording, no Clear on empty view ([afc78cb](https://github.com/arnaud-ritti/openidea-plugins/commit/afc78cba3839544ca3227c29f471045e1fc1a105))
* **spring-runtime:** mask actuator env secrets in storage, tree and tooltips ([e21569d](https://github.com/arnaud-ritti/openidea-plugins/commit/e21569de1f47ba0b17851117718b4300cdc72286))
* **spring-runtime:** mask compound secret names, rewrite older caches masked, tighten JMX and URL masking ([de76285](https://github.com/arnaud-ritti/openidea-plugins/commit/de76285bc8e8e398369db34439153b77d443f71a))
* **spring-runtime:** mask JMX operation leaves, rank graph from cached index only ([78dd9cc](https://github.com/arnaud-ritti/openidea-plugins/commit/78dd9ccf9982b6e12ba823d5ddb166344def9b64))
* **spring-runtime:** re-mask pre-schema-5 caches with the full parse-time masking ([b9e0960](https://github.com/arnaud-ritti/openidea-plugins/commit/b9e0960cfab1b49d358743762864e3db9219f885))
* **spring-runtime:** stable source navigation and per-project snapshots ([9b13b90](https://github.com/arnaud-ritti/openidea-plugins/commit/9b13b9070aa57f83b5d52bc1794ca9afa3703e61))
* **spring-runtime:** start auto-refresh from the command, confirm Clear ([0dbba6f](https://github.com/arnaud-ritti/openidea-plugins/commit/0dbba6f580d217a90bff5782d9b3c11aea08edc3))
* **spring-runtime:** withhold deep JMX dumps, split attribute at first hash, strict userinfo masking ([3abe91f](https://github.com/arnaud-ritti/openidea-plugins/commit/3abe91f7803bc88013b09b8425c13b398ddc3934))
* **spring-text:** shared tolerant XML validator, Windows-aware scope and build detection ([c8b343c](https://github.com/arnaud-ritti/openidea-plugins/commit/c8b343ccf5939596fd27c54b7ad6660105d89db0))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* tell users which permission to allow (part 1) ([a9565c7](https://github.com/arnaud-ritti/openidea-plugins/commit/a9565c76d8a6fa0e41a656082d635bb5c62b4556))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-runtime-v0.2.0...spring-runtime-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* close bounded Devcontainer, Flyway, Boot, Twig, Batch and Runtime residual slices ([e8b6bb5](https://github.com/arnaud-ritti/openidea-plugins/commit/e8b6bb515cd3266490b081421eb889191a8b06a4))
* custom gutter marker icons for Spring, data and PHP plugins ([1b180e5](https://github.com/arnaud-ritti/openidea-plugins/commit/1b180e583da9bae60f8ba3da6940ca94c4b275f0))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* import-level vulnerability evidence and read-only localhost JMX snapshot ([6de8359](https://github.com/arnaud-ritti/openidea-plugins/commit/6de83599fa9520b7184f1ca1796ea803ed7a8c30))
* SpEL validation, Modulith use-site checks, WSDL pairs, OpenAPI constraints, Actuator source links, persistence units ([a3f36c9](https://github.com/arnaud-ritti/openidea-plugins/commit/a3f36c9fc61bf32bea1c9f7f0bfb94afbcd5f66d))
* **spring:** close follow-up residuals across Spring, JPA, Flyway and Quarkus plugins ([d152eb1](https://github.com/arnaud-ritti/openidea-plugins/commit/d152eb1429e6f06237b3dd6c80c151e25e2010f9))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))
* wire custom gutter marker icons in Spring, data and PHP plugins ([9417a2f](https://github.com/arnaud-ritti/openidea-plugins/commit/9417a2f76bb5fc3699d2e665662457cdd03d8df3))


### Bug Fixes

* **spring-runtime:** drop toolbar buttons duplicating view tabs ([c4ee796](https://github.com/arnaud-ritti/openidea-plugins/commit/c4ee7963418e36f835fba51447cb34a67df51279))
* verification pass against real sources (Yarn berry parser, WS void rule, CRD nulls, Twig falsy namespaces, GitLab draft positions) ([00b86ec](https://github.com/arnaud-ritti/openidea-plugins/commit/00b86ec4f920a18ef709a65a137ff09788186619))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-runtime-v0.1.0...spring-runtime-v0.2.0) (2026-11-07)


### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **spring-runtime:** adopt OpenIDEA API 1.2 and add plugin icon ([9d9f482](https://github.com/arnaud-ritti/openidea-plugins/commit/9d9f482ce71c1dd84dd595b8cfca3c6336b38404))


### Bug Fixes

* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
