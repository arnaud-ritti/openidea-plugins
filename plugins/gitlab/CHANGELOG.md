# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/gitlab-v0.1.0...gitlab-v0.2.0) (2026-11-06)


### Features

* **gitlab:** add worktree and update-branch commands for merge requests ([378fe16](https://github.com/arnaud-ritti/openidea-plugins/commit/378fe1679aa4f24917fcddfebd562b78dec41d19))
* **gitlab:** apply review suggestion blocks as a quick fix ([7a59ee2](https://github.com/arnaud-ritti/openidea-plugins/commit/7a59ee2147d2f72fcd513046a2312268c0370dc3))
* **gitlab:** browse the commits of a merge request ([83458e6](https://github.com/arnaud-ritti/openidea-plugins/commit/83458e6f889845c079919579941f74b89d28b801))
* **gitlab:** check .gitlab-ci.yml jobs, stages and script variables, and open jobs from needs/extends ([5d47168](https://github.com/arnaud-ritti/openidea-plugins/commit/5d4716821d79f6de94ec2a6e972a7f2f6f3d4788))
* **gitlab:** create merge requests from a form view ([04791b6](https://github.com/arnaud-ritti/openidea-plugins/commit/04791b68babb23851614946a35c5b7a3b0efe612))
* **gitlab:** create snippets from a form view ([207ae2d](https://github.com/arnaud-ritti/openidea-plugins/commit/207ae2dda75d5af6a016569842bed685fc5a6cdf))
* **gitlab:** edit and delete notes and react with emoji ([e61f5f2](https://github.com/arnaud-ritti/openidea-plugins/commit/e61f5f25e0ac212a91424498948e759167061672))
* **gitlab:** page merge requests, threads and projects past the first page ([088877f](https://github.com/arnaud-ritti/openidea-plugins/commit/088877fe6ebbd8b4627d81f5104f14c077813f64))
* **gitlab:** pipeline and job details with retry and cancel ([74200d8](https://github.com/arnaud-ritti/openidea-plugins/commit/74200d87789762c045ecae064c9bd84ef5599577))
* **gitlab:** share projects from a form with init and initial commit ([18615bb](https://github.com/arnaud-ritti/openidea-plugins/commit/18615bb0d2f6af904d84f83d9b1d3024e8fcb7ed))
* **gitlab:** viewed files stay viewed until a later commit touches them ([eeb70f5](https://github.com/arnaud-ritti/openidea-plugins/commit/eeb70f5346583a41b24cc8ee40441e2126a49b82))


### Bug Fixes

* **gitlab:** 35 s paging budget, suggestion guard over the whole range ([890e2a5](https://github.com/arnaud-ritti/openidea-plugins/commit/890e2a5afbb4762b3ef35c67d9ec40f6d37d3758))
* **gitlab:** bind share commit, merge and close confirms to what was shown ([de7917d](https://github.com/arnaud-ritti/openidea-plugins/commit/de7917d03b4ec3b98f3c2457df1b9c1271b9f446))
* **gitlab:** bind the public snippet confirm to the content it was shown for ([5718d22](https://github.com/arnaud-ritti/openidea-plugins/commit/5718d226eb140a16accbcef6170bbaf69986cb6d))
* **gitlab:** ignore a leftover merge error during a rebase, clear stale CI problems when the file cannot be read ([cc63b96](https://github.com/arnaud-ritti/openidea-plugins/commit/cc63b96e8a68f0324f904214c9b482f6558978c9))
* **gitlab:** one 35 s paging budget per task ([063bb82](https://github.com/arnaud-ritti/openidea-plugins/commit/063bb82e29e79f55d75db689429554f6a16bffa8))
* **gitlab:** only pass hex merge request shas to git log ([340166c](https://github.com/arnaud-ritti/openidea-plugins/commit/340166c3731e18f21544b6850b6d6a8dfd4f28ee))
* **gitlab:** paging time budget, CRLF suggestions, unquoted git paths, diff guard ([9912840](https://github.com/arnaud-ritti/openidea-plugins/commit/99128409b7fde1706d1f1bad9c0cc8ed79076366))
* **gitlab:** stay quiet on unreadable stages and count every top-level key as a defined job ([910ba46](https://github.com/arnaud-ritti/openidea-plugins/commit/910ba46b72d61137e575e7129aaed6214c58774a))
* **gitlab:** validate values that reach git arguments and REST paths ([e86d3cd](https://github.com/arnaud-ritti/openidea-plugins/commit/e86d3cd4e3c9b2c3ec4129d55f9dcf5c485e7042))
