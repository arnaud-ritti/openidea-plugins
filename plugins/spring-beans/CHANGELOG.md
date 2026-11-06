# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/spring-beans-v0.1.0...spring-beans-v0.2.0) (2026-11-06)


### Features

* **spring-beans:** add opt-in unresolved @DependsOn and @ComponentScan checks ([3d598cb](https://github.com/arnaud-ritti/openidea-plugins/commit/3d598cbcfd5326edfbfa9e1350a77d9bb9289bf2))
* **spring-beans:** add proxy, scheduling, duplicate-name, configuration, naming and self-invocation inspections ([552f69b](https://github.com/arnaud-ritti/openidea-plugins/commit/552f69befa056d71fdab808a5cdaf900ce24c71b))
* **spring-beans:** check @Profile expressions and @Async return types ([aa68bd9](https://github.com/arnaud-ritti/openidea-plugins/commit/aa68bd9d96ef565d1a1b47dc2ade7b4ce87f816e))
* **spring-beans:** check bean names and abstract references in XML ([c719d68](https://github.com/arnaud-ritti/openidea-plugins/commit/c719d6835139bd416f8b83cce255537d6220c727))
* **spring-beans:** check cache annotation names, placement and combinations ([2a3b22d](https://github.com/arnaud-ritti/openidea-plugins/commit/2a3b22d3b8752740769bb64ac4b67ba7bbf3c98c))
* **spring-beans:** check XML bean attributes, injected values and scopes ([5e1f8bd](https://github.com/arnaud-ritti/openidea-plugins/commit/5e1f8bd4fa3ef759181f7699b2cd88cc2d4f5735))
* **spring-beans:** flag AspectJ pointcut designators Spring AOP does not support ([49030e0](https://github.com/arnaud-ritti/openidea-plugins/commit/49030e0b8b674de8fef68e7bdc0f833425d68fa5))
* **spring-beans:** follow p: and c: namespace references in XML ([974e5a5](https://github.com/arnaud-ritti/openidea-plugins/commit/974e5a58277200e2f0291627b1cd2334e27475b0))
* **spring-beans:** hint and fix &lt;value&gt; and &lt;ref&gt; children that can be attributes ([6ec21c9](https://github.com/arnaud-ritti/openidea-plugins/commit/6ec21c9b5025f6e136a8cd242ed1b2d0dd6511a5))
* **spring-beans:** merge constructor injection into an existing constructor ([dba5a36](https://github.com/arnaud-ritti/openidea-plugins/commit/dba5a3689096c53942910beaae8070df85a5536a))
* **spring-beans:** report @Autowired on static members and conflicting @Autowired constructors ([47e00fb](https://github.com/arnaud-ritti/openidea-plugins/commit/47e00fbfc66074d6f4084c0d22f35c5af253c00e))
* **spring-beans:** report AOP configuration mistakes in XML ([22a138f](https://github.com/arnaud-ritti/openidea-plugins/commit/22a138f0a5beb65d98e1c3e15ed53ad4c6f2e511))
* **spring-beans:** suggest the kotlin-spring plugin for classes Spring must proxy ([515ce8b](https://github.com/arnaud-ritti/openidea-plugins/commit/515ce8bb067102acd58d4a5e1e8d353ff9fa278f))
* **spring-beans:** type Kotlin @Bean funs by an obvious constructor-call body ([0bc0a43](https://github.com/arnaud-ritti/openidea-plugins/commit/0bc0a433f07204a70f60f5e159b7c02323ec69ee))
* **spring-beans:** XML intentions to use p-namespace, introduce a bean and update the schema ([422e22c](https://github.com/arnaud-ritti/openidea-plugins/commit/422e22cdd1d96f4bd0793e53aab4ec9e560c39f1))


### Bug Fixes

* **spring-beans:** keep @Profile check to errors Spring surely rejects ([0bce378](https://github.com/arnaud-ritti/openidea-plugins/commit/0bce378efd71b8bf46a87a6716b270a09e26f9f3))
* **spring-beans:** make file events optional, skip conditional duplicate beans, publish naming as a hint ([476a941](https://github.com/arnaud-ritti/openidea-plugins/commit/476a941e839f1831e51418973cc86f3ac556c9e5))
* **spring-beans:** merge constructor parameters after the last one, see bare assignments ([cd23ac0](https://github.com/arnaud-ritti/openidea-plugins/commit/cd23ac0c9c5f63e8d61c2bd2d0cfbbe501611989))
* **spring-beans:** put the merged comma before a trailing parameter comment ([3897753](https://github.com/arnaud-ritti/openidea-plugins/commit/38977535945fd77317978ae2921133d08e5fb4b2))
* **spring-beans:** quiet XML name, cache and weaving false positives ([a0c9adb](https://github.com/arnaud-ritti/openidea-plugins/commit/a0c9adb22cccb7b70f036963d18377e8c24df891))
* **spring-beans:** report a missing kotlin-spring plugin only when the build applies Kotlin literally ([036bd16](https://github.com/arnaud-ritti/openidea-plugins/commit/036bd165be9d4b1b11b30419b6210286bfb35388))
* **spring-beans:** say what checkXml and checkAop gate ([ab97e34](https://github.com/arnaud-ritti/openidea-plugins/commit/ab97e3480b3faa8d51f5d0296a31e316d6c47e2e))
* **spring-beans:** scope duplicate bean names to module and source set, name nested beans Outer.Inner, gate unresolved references on a complete index ([eb0b3ba](https://github.com/arnaud-ritti/openidea-plugins/commit/eb0b3ba464bee8dace39eec4db9727f56b23fd3d))
