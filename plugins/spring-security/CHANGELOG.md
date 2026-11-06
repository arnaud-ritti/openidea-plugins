# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-security-v0.1.0...spring-security-v0.2.0) (2026-11-06)


### Features

* **spring-security:** check Kotlin sources and the Kotlin DSL ([ce83734](https://github.com/arnaud-ritti/openidea-plugins/commit/ce837349ec889579415ade836a4159bbb6d271bf))
* **spring-security:** check the security XML namespace ([19a92d2](https://github.com/arnaud-ritti/openidea-plugins/commit/19a92d220daf7d9f757f5b83834ab277e58439f5))
* **spring-security:** flag matchers after anyRequest, shadowed matcher patterns and enabled security debug ([d2d1f95](https://github.com/arnaud-ritti/openidea-plugins/commit/d2d1f95741b5c83baca2963f8eb9b86936abd7cd))
* **spring-security:** flag path patterns that PathPatternParser rejects ([62b3ef4](https://github.com/arnaud-ritti/openidea-plugins/commit/62b3ef470d72ed0e71e0a9010867781023f52095))


### Bug Fixes

* **spring-security:** do not claim PathPatternParser is the Spring Security 6 default ([6976517](https://github.com/arnaud-ritti/openidea-plugins/commit/6976517341fe165aff9f8e99ab8c960c9d01bd39))
* **spring-security:** drop needless mut ([d84ea33](https://github.com/arnaud-ritti/openidea-plugins/commit/d84ea337d21739295d218bf44689ba9886294a77))
* **spring-security:** ignore Kotlin comments and strings, key duplicates on servlet-path, skip regex matchers ([fe836fd](https://github.com/arnaud-ritti/openidea-plugins/commit/fe836fd72df3e9bda9eec8d8c7c8278dde051622))
* **spring-security:** skip Kotlin char literals when masking comments and strings ([af910ed](https://github.com/arnaud-ritti/openidea-plugins/commit/af910ed76089a51009252ade332d8a59f7c028ed))
