# Changelog

## [0.5.0](https://github.com/arnaud-ritti/openidea-plugins/compare/openapi-v0.4.0...openapi-v0.5.0) (2026-11-24)


### Features

* **openapi,ideolog,kube-explorer,gitlab:** search, status menus, console colours, clipboard ([6116c36](https://github.com/arnaud-ritti/openidea-plugins/commit/6116c36b3e2cdd76a3aaac09a4882b48b8495baa))


### Bug Fixes

* **openapi,liquibase,laravel:** unique search ids, cheaper hover actions, no unconfirmed migrate in menu ([ed0334a](https://github.com/arnaud-ritti/openidea-plugins/commit/ed0334a6919ae21fcadc8c95ee892ede7447ee6c))

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/openapi-v0.3.0...openapi-v0.4.0) (2026-11-18)


### Features

* **openapi:** gutter links to every $ref of an operation and to a component's usages ([f3572b5](https://github.com/arnaud-ritti/openidea-plugins/commit/f3572b541dbd8b6a0de21aa89b6491727c3c5a67))
* **openapi:** operations panel beside spec editors ([e58517d](https://github.com/arnaud-ritti/openidea-plugins/commit/e58517d29b8d58db57c336864a46661357292a74))
* **openapi:** Try it out for YAML specs and a Try button in the preview ([8391499](https://github.com/arnaud-ritti/openidea-plugins/commit/8391499ccab9ebb88df62eb1002f7887d2c476e2))
* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* brand logos for the related-files gutter icons ([10c28d6](https://github.com/arnaud-ritti/openidea-plugins/commit/10c28d68d95fbff873034c530bbc0a29401d9590))
* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* **openapi:** close private-host bypasses and hidden-dir fallback ([42cc5fd](https://github.com/arnaud-ritti/openidea-plugins/commit/42cc5fd722f4662693fd5c38f7e49668b6d3ddc2))
* **openapi:** default request base URL to the spec server, remember the preview renderer ([fb30260](https://github.com/arnaud-ritti/openidea-plugins/commit/fb302601ef7848d8729d423cb4944db7f6c0e0e4))
* **openapi:** keep the rename confirm under the host's size limit ([0a86b79](https://github.com/arnaud-ritti/openidea-plugins/commit/0a86b799c740fb2e617563c921c62bb426df525a))
* **openapi:** name the DNS-rebinding ceiling of the host guard ([ff8df2a](https://github.com/arnaud-ritti/openidea-plugins/commit/ff8df2ad7d6942ef0dc825cbbb50eea63271ced1))
* **openapi:** palette endpoint picker, spec-only commands, codegen guidance ([ddab4d8](https://github.com/arnaud-ritti/openidea-plugins/commit/ddab4d85ca2a010304818cc2ce7d18ad22fba537))
* **openapi:** plain wording in Try it out, rename and codegen messages ([ad8d00e](https://github.com/arnaud-ritti/openidea-plugins/commit/ad8d00e754c6c8686916e462276369ca24c4e8cc))
* **openapi:** skip non-spec files, ignore staging dirs, refuse private hosts ([b97cf90](https://github.com/arnaud-ritti/openidea-plugins/commit/b97cf90592e5b385cd027ede8d4d1ee4d2fceb67))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* **spring-text:** share one XML walker, match drive letters case-insensitively ([61d354d](https://github.com/arnaud-ritti/openidea-plugins/commit/61d354d7122df0117b830a25290e76ceaa59d04f))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* standard permission copy for the HTTP request file fallback ([1c7c3bf](https://github.com/arnaud-ritti/openidea-plugins/commit/1c7c3bf8a2cf93ba179a01feaff81972d79bb137))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* tell users which permission to allow (part 1) ([a9565c7](https://github.com/arnaud-ritti/openidea-plugins/commit/a9565c76d8a6fa0e41a656082d635bb5c62b4556))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/openapi-v0.2.0...openapi-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([08db51a](https://github.com/arnaud-ritti/openidea-plugins/commit/08db51a54c99b3b4ae414ab790be2a0783add3c7))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([6f70311](https://github.com/arnaud-ritti/openidea-plugins/commit/6f70311ffc9ec945f0f1d9dcec4c77eec1801cb3))
* close another residual round across Dependency Audit, OpenAPI, Symfony and Twig ([2b8dfe8](https://github.com/arnaud-ritti/openidea-plugins/commit/2b8dfe818985ee6378430960ab13eea92a733dd9))
* close bounded Dependency Audit, GitLab, Ignore, Kubernetes and OpenAPI residual slices ([88748a2](https://github.com/arnaud-ritti/openidea-plugins/commit/88748a26fac2cebe07889ba68263889b4573fde0))
* close follow-up residuals in Dependency Audit, GitLab, OpenAPI and Kubernetes plugins ([15e91cc](https://github.com/arnaud-ritti/openidea-plugins/commit/15e91cc2ac526df01a84fbec13afa7e661baad0e))
* close six small non-Laravel parity residuals ([7fabc9c](https://github.com/arnaud-ritti/openidea-plugins/commit/7fabc9ca01e86306c5596c64c04df72a59caa6c7))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* Maven BOM metadata, oasdiff compat rules, Docker ARG scope, form/column checks, workspace chains, JAX-RS checks, view names, wiring registrations ([17b4876](https://github.com/arnaud-ritti/openidea-plugins/commit/17b48765dc99e46aa52915c5a4139d00a66a0ce2))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/openapi-v0.1.0...openapi-v0.2.0) (2026-11-07)


### Features

* **openapi:** adopt OpenIDEA API 1.2 and add plugin icon ([9f47dc1](https://github.com/arnaud-ritti/openidea-plugins/commit/9f47dc19769bdd88bfb0d233e84657c59db2c57f))
* **openapi:** implement the audit backlog packages ([39dad49](https://github.com/arnaud-ritti/openidea-plugins/commit/39dad492f269cb7b3637eac52d8015ae610015d4))
* **openapi:** implement the open audit rows of the v2 pass ([b7de84b](https://github.com/arnaud-ritti/openidea-plugins/commit/b7de84b62b564e7f52a7e00e6620932b4a63e560))
* **openapi:** show endpoints as the shared route list ([865dfbb](https://github.com/arnaud-ritti/openidea-plugins/commit/865dfbb325fd5f049943f929da8b2e22f3396c36))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))


### Bug Fixes

* compile the Laravel route action, declare storage, test crates in CI ([2d36ba0](https://github.com/arnaud-ritti/openidea-plugins/commit/2d36ba023ff67c685921e38eb138393f72513c9a))
* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **openapi:** write the HTTP request file beside the spec, cover row actions and flow-style tags ([897d5b3](https://github.com/arnaud-ritti/openidea-plugins/commit/897d5b356f040de8d08cbf8f8daa1d32a423481a))
