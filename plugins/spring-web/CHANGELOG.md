# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-web-v0.4.0...spring-web-v0.5.0) (2026-11-24)


### Features

* **spring-web,spring-beans,spring-boot,quarkus:** Search Everywhere tabs and console colours ([2913575](https://github.com/arnaud-ritti/openidea-plugins/commit/2913575c6cb69db741c0e9de4f5cf6b9d5bbe08a))


### Bug Fixes

* **spring-web:** fill the Spring Endpoints window over several calls on large projects ([#23](https://github.com/arnaud-ritti/openidea-plugins/issues/23)) ([8271607](https://github.com/arnaud-ritti/openidea-plugins/commit/827160790ac4338992408f53f3995bc245e0812a)), closes [#21](https://github.com/arnaud-ritti/openidea-plugins/issues/21)
* **symfony,spring-text:** keep index builds within the per-call CPU budget on large projects ([06744d3](https://github.com/arnaud-ritti/openidea-plugins/commit/06744d38e2528dd3314434980e77f7f353dd6f14))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-web-v0.3.0...spring-web-v0.4.0) (2026-11-18)


### Features

* **spring-web:** resolve Feign navigation by service identity ([#16](https://github.com/arnaud-ritti/openidea-plugins/issues/16)) ([3d4ae08](https://github.com/arnaud-ritti/openidea-plugins/commit/3d4ae08511e1a179e2be6f06a03d02d39ff9b121))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* correct manifest mismatches and unsafe defaults ([d555f8d](https://github.com/arnaud-ritti/openidea-plugins/commit/d555f8d68e191d63e2b0d69e5f52bcdc26561c0d))
* explain what to select when a row command runs from Search Everywhere ([47728bd](https://github.com/arnaud-ritti/openidea-plugins/commit/47728bdd035532879ddc1b6ea60357ca7b769c84))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* remaining jargon in titles ([4edf7e8](https://github.com/arnaud-ritti/openidea-plugins/commit/4edf7e8ebe03a85b56445d5aebfcc60818d8bfa2))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* **spring-text:** shared tolerant XML validator, Windows-aware scope and build detection ([c8b343c](https://github.com/arnaud-ritti/openidea-plugins/commit/c8b343ccf5939596fd27c54b7ad6660105d89db0))
* **spring-text:** spring-web diagnostics never discover; applies reads each build file once ([fc2e40d](https://github.com/arnaud-ritti/openidea-plugins/commit/fc2e40d8d645965294a29b9a23c1cdbd59399799))
* **spring-web:** gutter icons list every route, template and module target ([d787eff](https://github.com/arnaud-ritti/openidea-plugins/commit/d787eff531fa45290cbc6befa054c6d0607fceb9))
* **spring-web:** hide views outside JVM build projects, endpoint picker for palette commands, consistent titles ([1d6f436](https://github.com/arnaud-ritti/openidea-plugins/commit/1d6f4367200c93e34a666024c1cfe29fdbe94a81))
* **spring-web:** one HTTP icon per request mapping ([abedc56](https://github.com/arnaud-ritti/openidea-plugins/commit/abedc56fe4c9c9754c9dadf5f0a7239022207f44))
* **spring-web:** partial endpoint indexes, shared provider budget, per-path event updates ([231f5e2](https://github.com/arnaud-ritti/openidea-plugins/commit/231f5e2909af39b1f66e9f830307251910b4580c))
* **spring-web:** plainer GraphQL subscription flow and empty states ([c0ab281](https://github.com/arnaud-ritti/openidea-plugins/commit/c0ab28197fb83374f8d6fb79a2bdf4d36ffa76b5))
* **spring-web:** share coverage notes, build detection and bracket check from spring-text ([ed34a86](https://github.com/arnaud-ritti/openidea-plugins/commit/ed34a8635d0e25037415efbe1ba267b106b4d69e))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* tell users which permission to allow (part 1) ([a9565c7](https://github.com/arnaud-ritti/openidea-plugins/commit/a9565c76d8a6fa0e41a656082d635bb5c62b4556))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-web-v0.2.0...spring-web-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* custom gutter marker icons for Spring, data and PHP plugins ([1b180e5](https://github.com/arnaud-ritti/openidea-plugins/commit/1b180e583da9bae60f8ba3da6940ca94c4b275f0))
* finish small residuals (audit rows, escaped scalar rename, gateway arity) ([3e5c69f](https://github.com/arnaud-ritti/openidea-plugins/commit/3e5c69f197def48cc05526766fc88421536de5d5))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Maven BOM metadata, oasdiff compat rules, Docker ARG scope, form/column checks, workspace chains, JAX-RS checks, view names, wiring registrations ([17b4876](https://github.com/arnaud-ritti/openidea-plugins/commit/17b48765dc99e46aa52915c5a4139d00a66a0ce2))
* SpEL validation, Modulith use-site checks, WSDL pairs, OpenAPI constraints, Actuator source links, persistence units ([a3f36c9](https://github.com/arnaud-ritti/openidea-plugins/commit/a3f36c9fc61bf32bea1c9f7f0bfb94afbcd5f66d))
* **spring-web:** close bounded MVC, Cloud, GraphQL, messaging and WebSocket residuals ([08ee674](https://github.com/arnaud-ritti/openidea-plugins/commit/08ee674851d91d75fd274c70cc623dc99fa06b07))
* **spring:** close follow-up residuals across Spring, JPA, Flyway and Quarkus plugins ([d152eb1](https://github.com/arnaud-ritti/openidea-plugins/commit/d152eb1429e6f06237b3dd6c80c151e25e2010f9))
* **spring:** endpoint names, GraphQL/messaging hints, effective bean conditions, Batch launch command ([6a71f36](https://github.com/arnaud-ritti/openidea-plugins/commit/6a71f3670ad0c202a34025923ccb9daea81aae32))
* **spring:** Gradle cache metadata, WSDL QNames, effective contexts, Security verification fixes, HTTP exchange clients, MVC XML imports ([f371c2f](https://github.com/arnaud-ritti/openidea-plugins/commit/f371c2f5717aa806725da24e646300d8401c4ef6))
* twig.paths list form, Integration expression attrs and XPath, DefaultWsdl11Definition literals, Kotlin ctor PropertyKey, producer routing keys, Weight/Retry args ([36d07da](https://github.com/arnaud-ritti/openidea-plugins/commit/36d07da51c606c1e619f4fb8f59aa109199087c9))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))
* wire custom gutter marker icons in Spring, data and PHP plugins ([9417a2f](https://github.com/arnaud-ritti/openidea-plugins/commit/9417a2f76bb5fc3699d2e665662457cdd03d8df3))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-web-v0.1.0...spring-web-v0.2.0) (2026-11-07)


### ⚠ BREAKING CHANGES

* **spring-web:** absorb websocket, messaging, graphql, webflow and cloud into one Endpoints window

### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **spring-web:** absorb websocket, messaging, graphql, webflow and cloud into one Endpoints window ([752d895](https://github.com/arnaud-ritti/openidea-plugins/commit/752d8958618e610dcc1fefe198f6e33a14e51d0c))
* **spring-web:** adopt OpenIDEA API 1.2 and add plugin icon ([cdb62d7](https://github.com/arnaud-ritti/openidea-plugins/commit/cdb62d77e58ef606725b9187f88e237b78597044))
* **spring-web:** implement the audit backlog packages ([c029566](https://github.com/arnaud-ritti/openidea-plugins/commit/c029566c75efc1c7009c55c7cbc85b312ed0e7f5))
* **spring-web:** implement the open audit rows of the v2 pass ([36b837a](https://github.com/arnaud-ritti/openidea-plugins/commit/36b837a6f468d1e436f853b1cd572b0dedc6468d))
* **spring-web:** show endpoints as a flat route list with a controller filter ([1c1913b](https://github.com/arnaud-ritti/openidea-plugins/commit/1c1913bf4aa0b1fa4d1b9a7257b2b8fa26fe0879))


### Bug Fixes

* compile the Laravel route action, declare storage, test crates in CI ([2d36ba0](https://github.com/arnaud-ritti/openidea-plugins/commit/2d36ba023ff67c685921e38eb138393f72513c9a))
* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **spring-web:** keep exception details out of the generated ControllerAdvice response ([9f5393e](https://github.com/arnaud-ritti/openidea-plugins/commit/9f5393eae3c7d51fc98c6cd21022ee4712375988))
* **spring-web:** scope the ListItem import to the tests ([2c5853f](https://github.com/arnaud-ritti/openidea-plugins/commit/2c5853f59594ab57fda23b08e1ab5d3e266ffcf7))
