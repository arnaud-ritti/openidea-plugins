# Changelog

## [0.4.1](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-integration-v0.4.0...spring-integration-v0.4.1) (2026-11-24)


### Bug Fixes

* **dotenv,liquibase,symfony,spring-integration,spring-ws,spring-shell:** keep the remaining scans within the per-call CPU budget ([a732240](https://github.com/arnaud-ritti/openidea-plugins/commit/a7322404f1b4285febd6f7d08b329d2059ee3f2f))
* **symfony,spring-text:** keep index builds within the per-call CPU budget on large projects ([06744d3](https://github.com/arnaud-ritti/openidea-plugins/commit/06744d38e2528dd3314434980e77f7f353dd6f14))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-integration-v0.3.0...spring-integration-v0.4.0) (2026-11-18)


### Features

* commands and menu entries for toolbar-only actions ([940dcf2](https://github.com/arnaud-ritti/openidea-plugins/commit/940dcf2ae8ebff44ec7b104af38b7559eb86bff9))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* precise permission mapping and command fallbacks ([a46fee8](https://github.com/arnaud-ritti/openidea-plugins/commit/a46fee83335d5cbd91a15ee1739d0f1517daef63))
* remaining jargon in titles ([4edf7e8](https://github.com/arnaud-ritti/openidea-plugins/commit/4edf7e8ebe03a85b56445d5aebfcc60818d8bfa2))
* **spring-integration:** Batch CLI commands only while running, plain copy ([1091de5](https://github.com/arnaud-ritti/openidea-plugins/commit/1091de5ca7ff1c1ad5cd5f48b8f62a14f2452cfc))
* **spring-integration:** channel gutter icons list every producer or consumer ([760fd4e](https://github.com/arnaud-ritti/openidea-plugins/commit/760fd4e98d804d4c2a097de0637cf6f2cf285844))
* **spring-integration:** cross-file XML navigation, scoped suppression, tolerant XML ([bc810c2](https://github.com/arnaud-ritti/openidea-plugins/commit/bc810c2671682d559cdcfb9fccd6c51b4383e1f6))
* **spring-integration:** gate XML walks on spring_text analysis_valid/generation_valid instead of a 10 ms timer ([3e97648](https://github.com/arnaud-ritti/openidea-plugins/commit/3e9764868266fa294aabe5990d84fd2fcff1bb25))
* **spring-integration:** hide view outside JVM build projects, plain titles and messages ([7348765](https://github.com/arnaud-ritti/openidea-plugins/commit/73487653e206932fcb970df0c7260a711231ff1e))
* **spring-integration:** one shared index, reset-proof contexts cache, shared helpers ([e804d76](https://github.com/arnaud-ritti/openidea-plugins/commit/e804d7671b976062e5cdb3622fabb0c4596217bb))
* **spring-integration:** reject lowercase doctype in batch navigation ([22a20fe](https://github.com/arnaud-ritti/openidea-plugins/commit/22a20fe880dfb9331220894b0e289fcbdf0e6e8e))
* **spring-text:** shared tolerant XML validator, Windows-aware scope and build detection ([c8b343c](https://github.com/arnaud-ritti/openidea-plugins/commit/c8b343ccf5939596fd27c54b7ad6660105d89db0))
* **spring-text:** spring-web diagnostics never discover; applies reads each build file once ([fc2e40d](https://github.com/arnaud-ritti/openidea-plugins/commit/fc2e40d8d645965294a29b9a23c1cdbd59399799))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* tell users which permission to allow (part 2) ([ff1288d](https://github.com/arnaud-ritti/openidea-plugins/commit/ff1288d90688ec61691439f98db08e6a8d8b7a0c))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-integration-v0.2.0...spring-integration-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* close bounded Devcontainer, Flyway, Boot, Twig, Batch and Runtime residual slices ([e8b6bb5](https://github.com/arnaud-ritti/openidea-plugins/commit/e8b6bb515cd3266490b081421eb889191a8b06a4))
* custom gutter marker icons for Spring, data and PHP plugins ([1b180e5](https://github.com/arnaud-ritti/openidea-plugins/commit/1b180e583da9bae60f8ba3da6940ca94c4b275f0))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* SpEL validation, Modulith use-site checks, WSDL pairs, OpenAPI constraints, Actuator source links, persistence units ([a3f36c9](https://github.com/arnaud-ritti/openidea-plugins/commit/a3f36c9fc61bf32bea1c9f7f0bfb94afbcd5f66d))
* **spring:** close another residual round across Data, Security, Initializr, Integration, Shell, JPA and Quarkus ([bfb4579](https://github.com/arnaud-ritti/openidea-plugins/commit/bfb45790361d4fafb2db3e42f5de9bd7094bff14))
* **spring:** endpoint names, GraphQL/messaging hints, effective bean conditions, Batch launch command ([6a71f36](https://github.com/arnaud-ritti/openidea-plugins/commit/6a71f3670ad0c202a34025923ccb9daea81aae32))
* **spring:** Gradle cache metadata, WSDL QNames, effective contexts, Security verification fixes, HTTP exchange clients, MVC XML imports ([f371c2f](https://github.com/arnaud-ritti/openidea-plugins/commit/f371c2f5717aa806725da24e646300d8401c4ef6))
* twig.paths list form, Integration expression attrs and XPath, DefaultWsdl11Definition literals, Kotlin ctor PropertyKey, producer routing keys, Weight/Retry args ([36d07da](https://github.com/arnaud-ritti/openidea-plugins/commit/36d07da51c606c1e619f4fb8f59aa109199087c9))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))
* wire custom gutter marker icons in Spring, data and PHP plugins ([9417a2f](https://github.com/arnaud-ritti/openidea-plugins/commit/9417a2f76bb5fc3699d2e665662457cdd03d8df3))


### Bug Fixes

* verify Liquibase and Spring context rules against sources, tidy leftovers ([be4e938](https://github.com/arnaud-ritti/openidea-plugins/commit/be4e938ec5a803b82f22bdb65055a7e662daa5a2))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-integration-v0.1.0...spring-integration-v0.2.0) (2026-11-07)


### ⚠ BREAKING CHANGES

* **spring-integration:** absorb spring-batch and spring-modulith into one Structure window

### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **spring-batch,spring-modulith,spring-integration:** make the modules embeddable ([5a73ffa](https://github.com/arnaud-ritti/openidea-plugins/commit/5a73ffae537bdb81ca5448e2b03732cad8fd77c6))
* **spring-integration:** absorb spring-batch and spring-modulith into one Structure window ([dce47d3](https://github.com/arnaud-ritti/openidea-plugins/commit/dce47d329dca752ee366e5cdc22b01445b0b5cf7))
* **spring-integration:** add the Structure dispatch layer ([4668ffd](https://github.com/arnaud-ritti/openidea-plugins/commit/4668ffdef8800e9d62e8b0942ab4d579a65909be))
* **spring-integration:** adopt OpenIDEA API 1.2 and add plugin icon ([416b870](https://github.com/arnaud-ritti/openidea-plugins/commit/416b870ed33a56782b03edadad5a6784399a8045))


### Bug Fixes

* compile the Laravel route action, declare storage, test crates in CI ([2d36ba0](https://github.com/arnaud-ritti/openidea-plugins/commit/2d36ba023ff67c685921e38eb138393f72513c9a))
* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **spring-batch,spring-integration,spring-modulith:** prefix view ids, tighten applies markers ([8fb3628](https://github.com/arnaud-ritti/openidea-plugins/commit/8fb3628bce134b3cd67a6aa8ad12c6ecf093a37b))
