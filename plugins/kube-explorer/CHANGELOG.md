# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/kube-explorer-v0.4.0...kube-explorer-v0.5.0) (2026-11-24)


### Features

* **openapi,ideolog,kube-explorer,gitlab:** search, status menus, console colours, clipboard ([6116c36](https://github.com/arnaud-ritti/openidea-plugins/commit/6116c36b3e2cdd76a3aaac09a4882b48b8495baa))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/kube-explorer-v0.3.0...kube-explorer-v0.4.0) (2026-11-18)


### Features

* **kube-explorer:** context in the status bar, live diff before apply, reusable shells ([b7bd51f](https://github.com/arnaud-ritti/openidea-plugins/commit/b7bd51f504d4e19ae6ed381f545603e6f44ce214))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* **devcontainer,kube-explorer:** real command names in messages, one category ([64e5776](https://github.com/arnaud-ritti/openidea-plugins/commit/64e577666b4cc78bba4b30048d4cad98d9501e68))
* explain what to select when a row command runs from Search Everywhere ([47728bd](https://github.com/arnaud-ritti/openidea-plugins/commit/47728bdd035532879ddc1b6ea60357ca7b769c84))
* **kube-explorer:** confirm live replace and manifest delete, explain kubectl failures ([24edfea](https://github.com/arnaud-ritti/openidea-plugins/commit/24edfeae1f70bd71d42b2b9d8304b1f7bf6e47b9))
* **kube-explorer:** diff the live objects against a server dry run with noise removed ([f084c62](https://github.com/arnaud-ritti/openidea-plugins/commit/f084c62b9e7a8e670b1d8023a2f6c351d7f589fa))
* **kube-explorer:** keep the indexer as cache warmer, bound import placeholders, one eviction cache ([0659274](https://github.com/arnaud-ritti/openidea-plugins/commit/0659274bc1a97613a690fcf90744d2cfc26e587a))
* **kube-explorer:** label and selector gutter icons list every matching resource ([fe62182](https://github.com/arnaud-ritti/openidea-plugins/commit/fe62182ee84c1bb0e2d823f0df8222663609bf7b))
* **kube-explorer:** name context and namespace before delete and apply, separate no-context state ([00d1fc8](https://github.com/arnaud-ritti/openidea-plugins/commit/00d1fc8b05d61ab2c206b5bf37d94e2fae2a81e9))
* **kube-explorer:** never write live Secret data into manifests on reload ([cb84afe](https://github.com/arnaud-ritti/openidea-plugins/commit/cb84afe5962d7d9b87cbddfe541e64a5d3d5a09c))
* **kube-explorer:** reload past forbidden kinds, refuse fetched Secrets, reject duplicate data keys ([ad2968e](https://github.com/arnaud-ritti/openidea-plugins/commit/ad2968e6beac8f427bbdcab9a180f04516596022))
* **kube-explorer:** resilient watch, use and delete imported kubeconfigs, readme and change notes ([ceb731a](https://github.com/arnaud-ritti/openidea-plugins/commit/ceb731aff94f6be2163c12ca14382b44904ed8ae))
* **kube-explorer:** single diagnostics producer, scoped cache invalidation, cached kustomize facts ([3f267f7](https://github.com/arnaud-ritti/openidea-plugins/commit/3f267f7649bfe5e70d12d0fb546c3a1151b571d2))
* **kube-explorer:** stat-checked label cache, line table, helm ranges, one-step port forward ([3cf00d0](https://github.com/arnaud-ritti/openidea-plugins/commit/3cf00d08dadd043b138ae7ce1c4c88c667c8fa38))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* remaining jargon in titles ([4edf7e8](https://github.com/arnaud-ritti/openidea-plugins/commit/4edf7e8ebe03a85b56445d5aebfcc60818d8bfa2))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/kube-explorer-v0.2.0...kube-explorer-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([08db51a](https://github.com/arnaud-ritti/openidea-plugins/commit/08db51a54c99b3b4ae414ab790be2a0783add3c7))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([6f70311](https://github.com/arnaud-ritti/openidea-plugins/commit/6f70311ffc9ec945f0f1d9dcec4c77eec1801cb3))
* close bounded Dependency Audit, GitLab, Ignore, Kubernetes and OpenAPI residual slices ([88748a2](https://github.com/arnaud-ritti/openidea-plugins/commit/88748a26fac2cebe07889ba68263889b4573fde0))
* close follow-up residuals in Dependency Audit, GitLab, OpenAPI and Kubernetes plugins ([15e91cc](https://github.com/arnaud-ritti/openidea-plugins/commit/15e91cc2ac526df01a84fbec13afa7e661baad0e))
* close six small non-Laravel parity residuals ([7fabc9c](https://github.com/arnaud-ritti/openidea-plugins/commit/7fabc9ca01e86306c5596c64c04df72a59caa6c7))
* CRD pruning checks, Qute template warnings, Liquibase structure checks, JPA XML/inheritance checks, Symfony service/route references ([1c9a57d](https://github.com/arnaud-ritti/openidea-plugins/commit/1c9a57d89b6d3fd795f3de02ac076926f8fe385a))
* finish small residuals (audit rows, escaped scalar rename, gateway arity) ([3e5c69f](https://github.com/arnaud-ritti/openidea-plugins/commit/3e5c69f197def48cc05526766fc88421536de5d5))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Kotlin/XML table mappings, Compose and feature checks, Compose ownership, Helm scope, Twig delimiters and variables ([c3f2239](https://github.com/arnaud-ritti/openidea-plugins/commit/c3f22390f2d6a08a7f4b5f6192343f71b09f7606))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))


### Bug Fixes

* verification pass against real sources (Yarn berry parser, WS void rule, CRD nulls, Twig falsy namespaces, GitLab draft positions) ([00b86ec](https://github.com/arnaud-ritti/openidea-plugins/commit/00b86ec4f920a18ef709a65a137ff09788186619))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/kube-explorer-v0.1.0...kube-explorer-v0.2.0) (2026-11-07)


### Features

* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
