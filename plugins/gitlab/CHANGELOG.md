# Changelog

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/gitlab-v0.2.0...gitlab-v0.3.0) (2026-11-16)


### Features

* adopt API 1.4 and extend scoped plugin workflows ([5a8a92b](https://github.com/arnaud-ritti/openidea-plugins/commit/5a8a92b6a6268552c45d1e67b8ae40866a25daee))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([08db51a](https://github.com/arnaud-ritti/openidea-plugins/commit/08db51a54c99b3b4ae414ab790be2a0783add3c7))
* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([6f70311](https://github.com/arnaud-ritti/openidea-plugins/commit/6f70311ffc9ec945f0f1d9dcec4c77eec1801cb3))
* close bounded Dependency Audit, GitLab, Ignore, Kubernetes and OpenAPI residual slices ([88748a2](https://github.com/arnaud-ritti/openidea-plugins/commit/88748a26fac2cebe07889ba68263889b4573fde0))
* close follow-up residuals in Dependency Audit, GitLab, OpenAPI and Kubernetes plugins ([15e91cc](https://github.com/arnaud-ritti/openidea-plugins/commit/15e91cc2ac526df01a84fbec13afa7e661baad0e))
* **gitlab:** add sections to the Merge Request window and move Changes into it ([94fbb85](https://github.com/arnaud-ritti/openidea-plugins/commit/94fbb85c2764c47a8a1aeb49d502780d61d32942))
* **gitlab:** adopt OpenIDEA API 1.2 and add plugin icon ([38146e2](https://github.com/arnaud-ritti/openidea-plugins/commit/38146e28e978ea2aa1fcbfc943e5bfd917c17906))
* **gitlab:** browse the commits of a merge request ([83458e6](https://github.com/arnaud-ritti/openidea-plugins/commit/83458e6f889845c079919579941f74b89d28b801))
* **gitlab:** create merge requests from a form view ([04791b6](https://github.com/arnaud-ritti/openidea-plugins/commit/04791b68babb23851614946a35c5b7a3b0efe612))
* **gitlab:** create merge requests with prompts instead of a form window ([bab6c87](https://github.com/arnaud-ritti/openidea-plugins/commit/bab6c8742eb7df029e580d904eaf22f7291ca8a3))
* **gitlab:** create snippets and share projects with prompts ([50b7c26](https://github.com/arnaud-ritti/openidea-plugins/commit/50b7c263e98842793b2c95adf076246a0070ba1c))
* **gitlab:** create snippets from a form view ([207ae2d](https://github.com/arnaud-ritti/openidea-plugins/commit/207ae2dda75d5af6a016569842bed685fc5a6cdf))
* **gitlab:** edit and delete notes and react with emoji ([e61f5f2](https://github.com/arnaud-ritti/openidea-plugins/commit/e61f5f25e0ac212a91424498948e759167061672))
* **gitlab:** fold commits, files and pipelines into the Merge Request window ([5d68302](https://github.com/arnaud-ritti/openidea-plugins/commit/5d6830265d836bf862173e86c35b7c968ca135f9))
* **gitlab:** give the three windows distinct icons ([d263396](https://github.com/arnaud-ritti/openidea-plugins/commit/d263396854476f6c7c1eb1d1ab804da9eb5cf57c))
* **gitlab:** implement the audit backlog packages ([cc05073](https://github.com/arnaud-ritti/openidea-plugins/commit/cc050739b2138082704263b9e24df3a869f77b63))
* **gitlab:** implement the open audit rows of the v2 pass ([d2172db](https://github.com/arnaud-ritti/openidea-plugins/commit/d2172db0dfec6bc09fc401b5b9f5477d3549e27a))
* **gitlab:** pipeline and job details with retry and cancel ([74200d8](https://github.com/arnaud-ritti/openidea-plugins/commit/74200d87789762c045ecae064c9bd84ef5599577))
* **gitlab:** self-hosted HTTPS servers, native side-by-side diff, open after clone ([db9add4](https://github.com/arnaud-ritti/openidea-plugins/commit/db9add49e2a3543a0adffe7c05beb415dfb3c769)), closes [#11](https://github.com/arnaud-ritti/openidea-plugins/issues/11)
* **gitlab:** share projects from a form with init and initial commit ([18615bb](https://github.com/arnaud-ritti/openidea-plugins/commit/18615bb0d2f6af904d84f83d9b1d3024e8fcb7ed))
* implement plugin parity workflows and scoped refactors ([d0e1efc](https://github.com/arnaud-ritti/openidea-plugins/commit/d0e1efcd3f7017bb4ebfba8b50901d8affe0e8ba))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))


### Bug Fixes

* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **gitlab:** bind share commit, merge and close confirms to what was shown ([de7917d](https://github.com/arnaud-ritti/openidea-plugins/commit/de7917d03b4ec3b98f3c2457df1b9c1271b9f446))
* **gitlab:** bind the public snippet confirm to the content it was shown for ([5718d22](https://github.com/arnaud-ritti/openidea-plugins/commit/5718d226eb140a16accbcef6170bbaf69986cb6d))
* **gitlab:** commit selection opens the Changes section, drop stale on-view ([cc4147d](https://github.com/arnaud-ritti/openidea-plugins/commit/cc4147d5a8ba3e097cc5468b687715e168a55fec))
* **gitlab:** ignore a leftover merge error during a rebase, clear stale CI problems when the file cannot be read ([cc63b96](https://github.com/arnaud-ritti/openidea-plugins/commit/cc63b96e8a68f0324f904214c9b482f6558978c9))
* **gitlab:** keep section buttons when a section fails to render ([fdf69d8](https://github.com/arnaud-ritti/openidea-plugins/commit/fdf69d8145b2d3953924d46da6d2db6ea1420ff6))
* **gitlab:** keep the change-notes line continuation ([2e9bb2a](https://github.com/arnaud-ritti/openidea-plugins/commit/2e9bb2adda4984d38ae4e0a895edbf2f8d9db654))
* **gitlab:** validate values that reach git arguments and REST paths ([e86d3cd](https://github.com/arnaud-ritti/openidea-plugins/commit/e86d3cd4e3c9b2c3ec4129d55f9dcf5c485e7042))
* verification pass against real sources (Yarn berry parser, WS void rule, CRD nulls, Twig falsy namespaces, GitLab draft positions) ([00b86ec](https://github.com/arnaud-ritti/openidea-plugins/commit/00b86ec4f920a18ef709a65a137ff09788186619))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/gitlab-v0.1.0...gitlab-v0.2.0) (2026-11-07)


### Features

* **gitlab:** add sections to the Merge Request window and move Changes into it ([94fbb85](https://github.com/arnaud-ritti/openidea-plugins/commit/94fbb85c2764c47a8a1aeb49d502780d61d32942))
* **gitlab:** adopt OpenIDEA API 1.2 and add plugin icon ([38146e2](https://github.com/arnaud-ritti/openidea-plugins/commit/38146e28e978ea2aa1fcbfc943e5bfd917c17906))
* **gitlab:** create merge requests with prompts instead of a form window ([bab6c87](https://github.com/arnaud-ritti/openidea-plugins/commit/bab6c8742eb7df029e580d904eaf22f7291ca8a3))
* **gitlab:** create snippets and share projects with prompts ([50b7c26](https://github.com/arnaud-ritti/openidea-plugins/commit/50b7c263e98842793b2c95adf076246a0070ba1c))
* **gitlab:** fold commits, files and pipelines into the Merge Request window ([5d68302](https://github.com/arnaud-ritti/openidea-plugins/commit/5d6830265d836bf862173e86c35b7c968ca135f9))
* **gitlab:** give the three windows distinct icons ([d263396](https://github.com/arnaud-ritti/openidea-plugins/commit/d263396854476f6c7c1eb1d1ab804da9eb5cf57c))
* **gitlab:** implement the audit backlog packages ([cc05073](https://github.com/arnaud-ritti/openidea-plugins/commit/cc050739b2138082704263b9e24df3a869f77b63))
* **gitlab:** implement the open audit rows of the v2 pass ([d2172db](https://github.com/arnaud-ritti/openidea-plugins/commit/d2172db0dfec6bc09fc401b5b9f5477d3549e27a))
* OpenIDEA API 1.2 audit rows, window consolidation, route list, icons ([#5](https://github.com/arnaud-ritti/openidea-plugins/issues/5)) ([26a4dd9](https://github.com/arnaud-ritti/openidea-plugins/commit/26a4dd95b57cc4be7fb61065e33a8c6d56deecea))


### Bug Fixes

* declare the package icon in 24 plugin manifests ([770a781](https://github.com/arnaud-ritti/openidea-plugins/commit/770a781805b051c263c839479e8064cf317c30b9))
* **gitlab:** commit selection opens the Changes section, drop stale on-view ([cc4147d](https://github.com/arnaud-ritti/openidea-plugins/commit/cc4147d5a8ba3e097cc5468b687715e168a55fec))
* **gitlab:** keep section buttons when a section fails to render ([fdf69d8](https://github.com/arnaud-ritti/openidea-plugins/commit/fdf69d8145b2d3953924d46da6d2db6ea1420ff6))
* **gitlab:** keep the change-notes line continuation ([2e9bb2a](https://github.com/arnaud-ritti/openidea-plugins/commit/2e9bb2adda4984d38ae4e0a895edbf2f8d9db654))
