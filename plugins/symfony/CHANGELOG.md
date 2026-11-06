# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/symfony-v0.1.0...symfony-v0.2.0) (2026-11-06)


### Features

* **symfony:** add New-menu file templates for common Symfony classes ([49b4e00](https://github.com/arnaud-ritti/openidea-plugins/commit/49b4e00d9dfb5890060584501821bb1647f98ef3))
* **symfony:** add route attribute, parameter, form type and block override code actions ([c8573fd](https://github.com/arnaud-ritti/openidea-plugins/commit/c8573fd0ddb3732a678927ac4b381034f933d025))
* **symfony:** add service config templates, file icons and a status item ([470692a](https://github.com/arnaud-ritti/openidea-plugins/commit/470692a53eb2f3ca384e9917ebf5d5a8811d87ab))
* **symfony:** complete service tag names and add a Twig namespaces setting ([fd97b1f](https://github.com/arnaud-ritti/openidea-plugins/commit/fd97b1f6c02c905fb6f096a90b0d86a666d1d06d))
* **symfony:** complete Twig tags, filters, functions and tests; report unclosed block tags ([05a5cfe](https://github.com/arnaud-ritti/openidea-plugins/commit/05a5cfedb34a0efaa0a5f435dfba7581296da56c))
* **symfony:** list recent profiler requests in a view ([04baac3](https://github.com/arnaud-ritti/openidea-plugins/commit/04baac3b86751b416ab3c1cd263b3f8fc651467d))
* **symfony:** make events optional and add shared PHP text helpers ([f0d7188](https://github.com/arnaud-ritti/openidea-plugins/commit/f0d7188056d01a2b002e33ea1603974c93651a57))
* **symfony:** report command mistakes and gated deprecations in PHP ([88c71e5](https://github.com/arnaud-ritti/openidea-plugins/commit/88c71e58efe9f1658fbe392dfcd982d6d3d59c59))
* **symfony:** report deprecated factory settings and YAML quoting mistakes ([3316876](https://github.com/arnaud-ritti/openidea-plugins/commit/331687618193c41da0aa6f78543f2ce1e1059d2d))
* **symfony:** report deprecated service classes, duplicate parameters and unknown named arguments ([a7207b9](https://github.com/arnaud-ritti/openidea-plugins/commit/a7207b99dd89a754e7aca69dd138561893ed9c58))
* **symfony:** report missing controller methods and deprecated route settings ([783ae9f](https://github.com/arnaud-ritti/openidea-plugins/commit/783ae9f009ccb0967d7031e90cd968ae139a030c))
* **symfony:** report missing routes and assets in Twig templates ([770726d](https://github.com/arnaud-ritti/openidea-plugins/commit/770726de1a37d8de5ea551997f32445ee5cc77ad))
* **symfony:** report removed form type names, missing assets and unknown translation domains in PHP ([ad141df](https://github.com/arnaud-ritti/openidea-plugins/commit/ad141df790d38fbd13ed8fb82fafbfe0e8e5e824))
* **symfony:** report route comparisons, translation domains and syntax mistakes in Twig ([35bc07d](https://github.com/arnaud-ritti/openidea-plugins/commit/35bc07df1ad5ce7e04b2b04603fe8fc0b8af0060))
* **symfony:** report unquoted @ and % YAML values with a quote quick fix ([d29bcd0](https://github.com/arnaud-ritti/openidea-plugins/commit/d29bcd03f416f354823393b27d28ac9bad2b5423))
* **symfony:** report useless and invalid Doctrine mapping details ([ef51b27](https://github.com/arnaud-ritti/openidea-plugins/commit/ef51b27f17f5ff14312742dc0650221f7d0c3b31))
* **symfony:** resolve Twig paths namespaces from twig.yaml ([30f01f7](https://github.com/arnaud-ritti/openidea-plugins/commit/30f01f7642eb52fa782f20bbb302e654ff2ff6ea))
* **symfony:** warn on service ids defined twice in an XML file ([8c356c9](https://github.com/arnaud-ritti/openidea-plugins/commit/8c356c92a1936771b28d0842802a7b159accf2c3))


### Bug Fixes

* activate symfony on its profiler view, spring-messaging on xml, spring-websocket on jsx and tsx ([ea7dcb1](https://github.com/arnaud-ritti/openidea-plugins/commit/ea7dcb1dc71968924df6be933a9d53972c1ac92a))
* **symfony:** avoid a panic on `{%}` in code_regions and cover the Twig names group ([b833873](https://github.com/arnaud-ritti/openidea-plugins/commit/b8338733df87856ad4c53f9b2335230efbd267b5))
* **symfony:** keep the status item while another Symfony project is indexed ([529785b](https://github.com/arnaud-ritti/openidea-plugins/commit/529785b4dbaf781506958ac4d0eb9a68c20eeffd))
* **symfony:** open YAML quotes only at scalar starts, test factory ranges, skip XML comments ([ed7b329](https://github.com/arnaud-ritti/openidea-plugins/commit/ed7b329c5019d25f8897fc08131a6fea6bb6ca71))
* **symfony:** read parent templates only on the extends tag, skip Twig comments, ignore parameter defaults ([43a78d5](https://github.com/arnaud-ritti/openidea-plugins/commit/43a78d504b67db485d6a0ea6072c1b213e192d92))
* **symfony:** stop flagging generated assets, vendor domains, project base controllers and static constructors ([4a1b472](https://github.com/arnaud-ritti/openidea-plugins/commit/4a1b472764990085f51253c662853981e8f86e36))
* **symfony:** survive multibyte carets, cheaper quick fixes, mappedBy and alias edge cases ([6f0cd23](https://github.com/arnaud-ritti/openidea-plugins/commit/6f0cd23317652252514486efb82ea52a6122b105))
