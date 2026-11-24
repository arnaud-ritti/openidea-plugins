# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/laravel-v0.4.0...laravel-v0.5.0) (2026-11-24)


### Features

* **laravel,symfony:** Search Everywhere tabs, status bar menus and console colours ([7c4e5cd](https://github.com/arnaud-ritti/openidea-plugins/commit/7c4e5cd772f6ab4ab8598006f472f26883ac36c6))


### Bug Fixes

* **laravel:** translate the Laravel Routes search tab title ([1ac8868](https://github.com/arnaud-ritti/openidea-plugins/commit/1ac88680a38677b8890cc99136738a10bb535e80))
* **openapi,liquibase,laravel:** unique search ids, cheaper hover actions, no unconfirmed migrate in menu ([ed0334a](https://github.com/arnaud-ritti/openidea-plugins/commit/ed0334a6919ae21fcadc8c95ee892ede7447ee6c))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/laravel-v0.3.0...laravel-v0.4.0) (2026-11-18)


### Features

* **laravel:** controller methods open the native route chooser instead of a Route actions dialog ([80d9bbc](https://github.com/arnaud-ritti/openidea-plugins/commit/80d9bbcab64348023e129e2917642c5ccb92d625))
* **laravel:** fold Schema into the Database tab and show tables as a tree ([8aeb68f](https://github.com/arnaud-ritti/openidea-plugins/commit/8aeb68f66b90436fc44643d2fd58ee83ee874243))
* **symfony:** Database tab with Doctrine schema, compare, rows and migrations ([ceda891](https://github.com/arnaud-ritti/openidea-plugins/commit/ceda89170aea90a2742ac7f9caf0b71010b9a82d))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* explain what to select when a row command runs from Search Everywhere ([47728bd](https://github.com/arnaud-ritti/openidea-plugins/commit/47728bdd035532879ddc1b6ea60357ca7b769c84))
* **laravel,symfony:** show each tool window only in its own kind of project ([42a2797](https://github.com/arnaud-ritti/openidea-plugins/commit/42a2797267dcf49b73155cef3c4b853fca0d95f0))
* **laravel:** cap each indexing step at 150 ms ([1b86467](https://github.com/arnaud-ritti/openidea-plugins/commit/1b86467f1ca5c3523731f7f11c63f98ddf9395da))
* **laravel:** commands finish their own edits, plain names and messages ([9c60bb6](https://github.com/arnaud-ritti/openidea-plugins/commit/9c60bb65db2742da7fbc779017f149c5d12a40dd))
* **laravel:** confirm destructive database commands, route picker from the palette, hide commands outside Laravel projects ([cafe3f2](https://github.com/arnaud-ritti/openidea-plugins/commit/cafe3f23c5f41f0c19b2ff61663463849789333e))
* **laravel:** controller route icons stay within the editor's time budget ([4bda806](https://github.com/arnaud-ritti/openidea-plugins/commit/4bda806f942bcf8f0abeca740ba88b56673d5a31))
* **laravel:** keep editor providers out of index setup and mask probe errors ([5934298](https://github.com/arnaud-ritti/openidea-plugins/commit/5934298ec10ae71c5f402094d4650d0b138fcf89))
* **laravel:** make Docker targets and long PHP probes work on a real daemon ([6924168](https://github.com/arnaud-ritti/openidea-plugins/commit/69241688bfbd5865c23d26576582fe8370e27e33))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* **laravel:** send request input over stdin and keep provider indexing in budget ([54b096a](https://github.com/arnaud-ritti/openidea-plugins/commit/54b096af26199e68c1e10d33d451b5cd168785c9))
* **laravel:** translate the renamed Docker, editor hint and cURL titles ([cf22b24](https://github.com/arnaud-ritti/openidea-plugins/commit/cf22b240b8f7e7d282c4345da49c3a97c90bc70d))
* **laravel:** translate the short setting titles ([a469b52](https://github.com/arnaud-ritti/openidea-plugins/commit/a469b523e85097f5cdd9f98ecaa47bd7008d69a2))
* **laravel:** use shortcuts no other plugin takes ([87a9510](https://github.com/arnaud-ritti/openidea-plugins/commit/87a9510b59c988c60f67407da0356e3ec025339e))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* remaining jargon in titles ([4edf7e8](https://github.com/arnaud-ritti/openidea-plugins/commit/4edf7e8ebe03a85b56445d5aebfcc60818d8bfa2))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* standard permission copy for the HTTP request file fallback ([1c7c3bf](https://github.com/arnaud-ritti/openidea-plugins/commit/1c7c3bf8a2cf93ba179a01feaff81972d79bb137))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* **symfony:** stop overdue Send Request and database probes, close the rename racy-mtime gap ([43898a2](https://github.com/arnaud-ritti/openidea-plugins/commit/43898a225be5e7af0f58fbb3eadc4cfedf674e12))
* tell users which permission to allow (part 2) ([ff1288d](https://github.com/arnaud-ritti/openidea-plugins/commit/ff1288d90688ec61691439f98db08e6a8d8b7a0c))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/laravel-v0.2.0...laravel-v0.3.0) (2026-11-16)


### Features

* add laravel database workspace, schema tools and enum where inspection ([c1589d2](https://github.com/arnaud-ritti/openidea-plugins/commit/c1589d25ce6a954d1799a3d77aaf36a0b5328166))
* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* **laravel:** add a flat Routes section to the Laravel window ([fef2331](https://github.com/arnaud-ritti/openidea-plugins/commit/fef23310bfd512a209ccca7dd3f639460d579780))
* **laravel:** add class directive, bulk rule and controller, Rule object and facade import rewrites ([386f89c](https://github.com/arnaud-ritti/openidea-plugins/commit/386f89c25bd729e886b578b4ee8b983ed5a647a0))
* **laravel:** add model class gutter and refresh hub UI ([693c787](https://github.com/arnaud-ritti/openidea-plugins/commit/693c7875d4d7e938cb6ca92b6318831435204a93))
* **laravel:** adopt OpenIDEA API 1.2 and add plugin icon ([ee9e836](https://github.com/arnaud-ritti/openidea-plugins/commit/ee9e8363a8e05e28dfa47e5bf28d14aa0fa8d3db))
* **laravel:** bounded resumable indexing and parity implementation ([c4a69ff](https://github.com/arnaud-ritti/openidea-plugins/commit/c4a69ff01ce3dca30fad90292d06f4e24c005702))
* **laravel:** complete Blade directives with argument and closing snippets ([06411e7](https://github.com/arnaud-ritti/openidea-plugins/commit/06411e72191584ecf065dc955194373db63bb2ab))
* **laravel:** implement the audit backlog packages ([297f36e](https://github.com/arnaud-ritti/openidea-plugins/commit/297f36e0711101507e7f367b2725e9fbb7674903))
* **laravel:** implement the open audit rows of the v2 pass ([1a991f0](https://github.com/arnaud-ritti/openidea-plugins/commit/1a991f0b5ed8791b268e2a2b9c84ff82d66ef612))
* **laravel:** read view namespaces from ide-blade.json ([6304cb6](https://github.com/arnaud-ritti/openidea-plugins/commit/6304cb652903c992c585495240fff9d916fc9dc9))
* **laravel:** report env() outside config files and more orderBy date columns ([7afb15b](https://github.com/arnaud-ritti/openidea-plugins/commit/7afb15be6de398b668ae831cf48db3e377df640d))
* **laravel:** security and upgrade inspections, Pest/Pint/PHPStan/Socialite completion, snippets, feature test generator ([dd96a5e](https://github.com/arnaud-ritti/openidea-plugins/commit/dd96a5eb445233f38e50577aeb699d60bcbacdcd))
* **laravel:** track config coverage per array instead of per file ([4cf73c5](https://github.com/arnaud-ritti/openidea-plugins/commit/4cf73c5d9f104f9386ac60cd16f1925011b83b67))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **symfony,laravel:** config and env tables with masked values ([6a390bf](https://github.com/arnaud-ritti/openidea-plugins/commit/6a390bfc8eacf7205267bedf399bcbc2a84050b9))
* **ui-kit:** update hub web view and shared sync script ([eaef9a1](https://github.com/arnaud-ritti/openidea-plugins/commit/eaef9a1f375f2d2653425df4a121802dc79a228d))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))
* wire custom gutter marker icons in Spring, data and PHP plugins ([9417a2f](https://github.com/arnaud-ritti/openidea-plugins/commit/9417a2f76bb5fc3699d2e665662457cdd03d8df3))


### Bug Fixes

* compile the Laravel route action, declare storage, test crates in CI ([2d36ba0](https://github.com/arnaud-ritti/openidea-plugins/commit/2d36ba023ff67c685921e38eb138393f72513c9a))
* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **laravel:** add the empty branch to the forelse snippet and tighten the snippet tests ([1521824](https://github.com/arnaud-ritti/openidea-plugins/commit/1521824d66d715cdb8e628f30d2ead62542d4850))
* **laravel:** advance the index from the hub's tick envelope ([a008269](https://github.com/arnaud-ritti/openidea-plugins/commit/a00826904148e137c8358089480fc6a4394d0785))
* **laravel:** keep namespaced env functions quiet and end heredocs only at the exact label ([be1424e](https://github.com/arnaud-ritti/openidea-plugins/commit/be1424eaf3dcad6e3edaff62565f487c91f39927))
* **laravel:** map Context to its facade, match declared class names by word, gate bulk controller scan on the caret ([d51538b](https://github.com/arnaud-ritti/openidea-plugins/commit/d51538bd6b08807465f05ed25596481082ff7442))
* **laravel:** no panic on UTF-8 in the env check, relative quiet directories, sturdier ide-blade.json ([88bd4da](https://github.com/arnaud-ritti/openidea-plugins/commit/88bd4da452be061f4e9be1ab79eff31f9523ff48))
* **laravel:** skip strings and heredocs in the env() check and report nested calls once ([c076dc9](https://github.com/arnaud-ritti/openidea-plugins/commit/c076dc9fe2fef07120e8d6d959e7227bff502397))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/laravel-v0.1.0...laravel-v0.2.0) (2026-11-07)


### Features

* **laravel:** add a flat Routes section to the Laravel window ([fef2331](https://github.com/arnaud-ritti/openidea-plugins/commit/fef23310bfd512a209ccca7dd3f639460d579780))
* **laravel:** adopt OpenIDEA API 1.2 and add plugin icon ([ee9e836](https://github.com/arnaud-ritti/openidea-plugins/commit/ee9e8363a8e05e28dfa47e5bf28d14aa0fa8d3db))
* **laravel:** implement the audit backlog packages ([297f36e](https://github.com/arnaud-ritti/openidea-plugins/commit/297f36e0711101507e7f367b2725e9fbb7674903))
* **laravel:** implement the open audit rows of the v2 pass ([1a991f0](https://github.com/arnaud-ritti/openidea-plugins/commit/1a991f0b5ed8791b268e2a2b9c84ff82d66ef612))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))


### Bug Fixes

* compile the Laravel route action, declare storage, test crates in CI ([2d36ba0](https://github.com/arnaud-ritti/openidea-plugins/commit/2d36ba023ff67c685921e38eb138393f72513c9a))
* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
