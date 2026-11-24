# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ideolog-v0.4.0...ideolog-v0.5.0) (2026-11-24)


### Features

* **openapi,ideolog,kube-explorer,gitlab:** search, status menus, console colours, clipboard ([6116c36](https://github.com/arnaud-ritti/openidea-plugins/commit/6116c36b3e2cdd76a3aaac09a4882b48b8495baa))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ideolog-v0.3.0...ideolog-v0.4.0) (2026-11-18)


### Features

* **ideolog:** info, debug and trace counts in the Log Levels panel, level filter in the snapshot ([dab4613](https://github.com/arnaud-ritti/openidea-plugins/commit/dab46135410cfc6954e005ec3cb0e7ef3ebc3cb2))
* **ideolog:** level panel beside log editors, previous error/warning/exception navigation ([8c5d0cc](https://github.com/arnaud-ritti/openidea-plugins/commit/8c5d0cca74749cb9d127e8a5090f8d8d5e3cd1c5))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* correct manifest mismatches and unsafe defaults ([d555f8d](https://github.com/arnaud-ritti/openidea-plugins/commit/d555f8d68e191d63e2b0d69e5f52bcdc26561c0d))
* **ideolog:** bound single huge events, keep partial folds, narrow event invalidation ([884503f](https://github.com/arnaud-ritti/openidea-plugins/commit/884503f290fd4ff6d31eca37e700a1cc07cc34e7))
* **ideolog:** colour whole logs and apply pattern changes immediately ([ed2ce2d](https://github.com/arnaud-ritti/openidea-plugins/commit/ed2ce2d00ddf608ead59cb6d8e25ab03865911dd))
* **ideolog:** plain live tail and JVM copy, language-scoped commands, one tab name ([be005c5](https://github.com/arnaud-ritti/openidea-plugins/commit/be005c5ceda933c1694e440488335f49d56db939))
* **ideolog:** scope log commands to log files, explain highlight limits, confirm bulk removal ([07699af](https://github.com/arnaud-ritti/openidea-plugins/commit/07699af23712bcfadc7fb09d4e2a2ce20944f8eb))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* precise permission mapping and command fallbacks ([a46fee8](https://github.com/arnaud-ritti/openidea-plugins/commit/a46fee83335d5cbd91a15ee1739d0f1517daef63))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* tell users which permission to allow (part 2) ([ff1288d](https://github.com/arnaud-ritti/openidea-plugins/commit/ff1288d90688ec61691439f98db08e6a8d8b7a0c))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ideolog-v0.2.0...ideolog-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* **ideolog:** update hub UI and move live tail into the hub ([7c8af35](https://github.com/arnaud-ritti/openidea-plugins/commit/7c8af35eaedab4eb7950df237cca3ac0a3d4377c))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* **ui-kit:** update hub web view and shared sync script ([eaef9a1](https://github.com/arnaud-ritti/openidea-plugins/commit/eaef9a1f375f2d2653425df4a121802dc79a228d))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))


### Bug Fixes

* **deps:** build against OpenIDEA SDK v0.1.42 and require plugin API 1.5 ([4ec2afa](https://github.com/arnaud-ritti/openidea-plugins/commit/4ec2afa5da97997d15e0cab94f3cfcadbb89e375))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ideolog-v0.1.0...ideolog-v0.2.0) (2026-11-07)


### Features

* **ideolog:** adopt OpenIDEA API 1.2 and add plugin icon ([71614e4](https://github.com/arnaud-ritti/openidea-plugins/commit/71614e40aec5adbb468f14059dd7f9c9ae29ced8))
* **ideolog:** implement the audit backlog packages ([56b2ba2](https://github.com/arnaud-ritti/openidea-plugins/commit/56b2ba2d61f4f1b4a3ad793791bce5cb5589e230))
* **ideolog:** implement the open audit rows of the v2 pass ([51344e7](https://github.com/arnaud-ritti/openidea-plugins/commit/51344e766af042433cae9d21d016733fcaebe88b))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
