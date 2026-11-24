# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/liquibase-v0.4.0...liquibase-v0.5.0) (2026-11-24)


### Features

* **flyway,liquibase,ignore,tasks:** status bar menus, Search Everywhere tabs and console colours ([65df94d](https://github.com/arnaud-ritti/openidea-plugins/commit/65df94db07ab2b86d602b16384b102dd1ef6830b))


### Bug Fixes

* **dotenv,liquibase,symfony,spring-integration,spring-ws,spring-shell:** keep the remaining scans within the per-call CPU budget ([a732240](https://github.com/arnaud-ritti/openidea-plugins/commit/a7322404f1b4285febd6f7d08b329d2059ee3f2f))
* **openapi,liquibase,laravel:** unique search ids, cheaper hover actions, no unconfirmed migrate in menu ([ed0334a](https://github.com/arnaud-ritti/openidea-plugins/commit/ed0334a6919ae21fcadc8c95ee892ede7447ee6c))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/liquibase-v0.3.0...liquibase-v0.4.0) (2026-11-18)


### Features

* **liquibase:** show pending changesets and mark changelog files ([b565c30](https://github.com/arnaud-ritti/openidea-plugins/commit/b565c30fb92bd32f37022c4d2b5c765b546a0479))
* **symfony,ignore,liquibase:** make New … commands create the file ([797a5b2](https://github.com/arnaud-ritti/openidea-plugins/commit/797a5b2208f68b17560909f10cfd76dd268dda5f))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* correct manifest mismatches and unsafe defaults ([d555f8d](https://github.com/arnaud-ritti/openidea-plugins/commit/d555f8d68e191d63e2b0d69e5f52bcdc26561c0d))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* **liquibase:** bound the events-denied fallback warm to the open file ([e1f7e7f](https://github.com/arnaud-ritti/openidea-plugins/commit/e1f7e7f9846540c8320944e262b11b73281bd397))
* **liquibase:** includeAll lines list their changelogs, Go to Definition shows every candidate ([8dc59da](https://github.com/arnaud-ritti/openidea-plugins/commit/8dc59da6e99981ebab81a1678424fb412ad323cb))
* **liquibase:** lead run confirmations with the effect, target and command ([f4e23be](https://github.com/arnaud-ritti/openidea-plugins/commit/f4e23be37ba369ce0125590e88fd2ced89d09117))
* **liquibase:** one deadline per event batch, forget unreached changelogs, warm files opened before activation ([73d188d](https://github.com/arnaud-ritti/openidea-plugins/commit/73d188d0f32ec557b58563701789aa198590060d))
* **liquibase:** plain confirmations, first changelog, shorter toolbars ([624609b](https://github.com/arnaud-ritti/openidea-plugins/commit/624609b9349c2589bc29fb1aa62f4cd0ef84fa8a))
* **liquibase:** ship include and property checks; faster positions ([befee62](https://github.com/arnaud-ritti/openidea-plugins/commit/befee6269c55079e93509d8296be4d7187da1d7d))
* **liquibase:** warm the open file when events are denied; bound warm budget ([4f8f2e1](https://github.com/arnaud-ritti/openidea-plugins/commit/4f8f2e131aca09092376896795dbcbb1866df598))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* precise permission mapping and command fallbacks ([a46fee8](https://github.com/arnaud-ritti/openidea-plugins/commit/a46fee83335d5cbd91a15ee1739d0f1517daef63))
* **properties,liquibase:** drop dead settings path and test JSON completion ([3a0f8c5](https://github.com/arnaud-ritti/openidea-plugins/commit/3a0f8c5f6a865954e349e7d39774da1a3f29bdbb))
* regenerate UI config and finish message spacing ([6562555](https://github.com/arnaud-ritti/openidea-plugins/commit/6562555a205b3452c0f2fb00c20c672947cb7dd4))
* **spring-text:** share one XML walker, match drive letters case-insensitively ([61d354d](https://github.com/arnaud-ritti/openidea-plugins/commit/61d354d7122df0117b830a25290e76ceaa59d04f))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/liquibase-v0.2.0...liquibase-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* CRD pruning checks, Qute template warnings, Liquibase structure checks, JPA XML/inheritance checks, Symfony service/route references ([1c9a57d](https://github.com/arnaud-ritti/openidea-plugins/commit/1c9a57d89b6d3fd795f3de02ac076926f8fe385a))
* custom gutter marker icons for Spring, data and PHP plugins ([1b180e5](https://github.com/arnaud-ritti/openidea-plugins/commit/1b180e583da9bae60f8ba3da6940ca94c4b275f0))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))
* wire custom gutter marker icons in Spring, data and PHP plugins ([9417a2f](https://github.com/arnaud-ritti/openidea-plugins/commit/9417a2f76bb5fc3699d2e665662457cdd03d8df3))


### Bug Fixes

* verify Liquibase and Spring context rules against sources, tidy leftovers ([be4e938](https://github.com/arnaud-ritti/openidea-plugins/commit/be4e938ec5a803b82f22bdb65055a7e662daa5a2))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/liquibase-v0.1.0...liquibase-v0.2.0) (2026-11-07)


### Features

* **liquibase:** adopt OpenIDEA API 1.2 and add plugin icon ([605f9ca](https://github.com/arnaud-ritti/openidea-plugins/commit/605f9ca96c585c390518fa65efeee174b230f146))
* **liquibase:** implement the audit backlog packages ([a138368](https://github.com/arnaud-ritti/openidea-plugins/commit/a13836811ab5d57d1b56ef138f3782e496792cd6))
* **liquibase:** implement the open audit rows of the v2 pass ([2256807](https://github.com/arnaud-ritti/openidea-plugins/commit/225680725d18bce7642210dbde236d719d8ae8a7))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))


### Bug Fixes

* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
