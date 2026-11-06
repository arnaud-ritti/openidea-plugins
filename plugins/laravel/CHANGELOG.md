# Changelog

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/laravel-v0.1.0...laravel-v0.2.0) (2026-11-06)


### Features

* **laravel:** add class directive, bulk rule and controller, Rule object and facade import rewrites ([386f89c](https://github.com/arnaud-ritti/openidea-plugins/commit/386f89c25bd729e886b578b4ee8b983ed5a647a0))
* **laravel:** add the intention that turns a rules array into a pipe string ([e676e05](https://github.com/arnaud-ritti/openidea-plugins/commit/e676e050f5badd43da1b62082266f3dc31a2b460))
* **laravel:** artisan run configurations, db seed and fresh migration ([3882ec5](https://github.com/arnaud-ritti/openidea-plugins/commit/3882ec5bc999f8dc64e6b16fd6f6a8182d856cca))
* **laravel:** complete and open Ziggy route names in JavaScript, TypeScript and Vue ([f717638](https://github.com/arnaud-ritti/openidea-plugins/commit/f717638b294912566f5637e88808246197f7e7e8))
* **laravel:** complete Blade directives after @ ([0129d61](https://github.com/arnaud-ritti/openidea-plugins/commit/0129d61071677029ed1d25621205225608e25e05))
* **laravel:** complete Blade directives with argument and closing snippets ([06411e7](https://github.com/arnaud-ritti/openidea-plugins/commit/06411e72191584ecf065dc955194373db63bb2ab))
* **laravel:** completion, navigation, problems and tool window for Laravel ([9b8f3fe](https://github.com/arnaud-ritti/openidea-plugins/commit/9b8f3fe7559e17c0ce8024e1bb5057f223ac0a31))
* **laravel:** intentions for validation rules, controller strings and Blade echoes ([ff6f258](https://github.com/arnaud-ritti/openidea-plugins/commit/ff6f258baca6278b6bfacff160c890e91cb3ac46))
* **laravel:** New-menu file templates for common Laravel classes ([7d45cc0](https://github.com/arnaud-ritti/openidea-plugins/commit/7d45cc0e530e888f7e7322e2dd8ac71517334217))
* **laravel:** read view namespaces from ide-blade.json ([6304cb6](https://github.com/arnaud-ritti/openidea-plugins/commit/6304cb652903c992c585495240fff9d916fc9dc9))
* **laravel:** report env() outside config files and more orderBy date columns ([7afb15b](https://github.com/arnaud-ritti/openidea-plugins/commit/7afb15be6de398b668ae831cf48db3e377df640d))
* **laravel:** report unguarded mass assignment, unscheduled prunable models and orderBy created_at ([bd9f6aa](https://github.com/arnaud-ritti/openidea-plugins/commit/bd9f6aac050f3d190c65981287ea6baa1d3b2cd2))
* **laravel:** report unknown components, Livewire, Inertia pages and factories ([b015041](https://github.com/arnaud-ritti/openidea-plugins/commit/b0150419a30c6e988f7461a7f183f33abddf2e3d))


### Bug Fixes

* **laravel:** add the empty branch to the forelse snippet and tighten the snippet tests ([1521824](https://github.com/arnaud-ritti/openidea-plugins/commit/1521824d66d715cdb8e628f30d2ead62542d4850))
* **laravel:** confirm migrate-fresh per project ([7a6df0a](https://github.com/arnaud-ritti/openidea-plugins/commit/7a6df0aacfa522f7fe9e602218b3c6f4a4959721))
* **laravel:** do not report guarded mass assignment in seeders and factories ([ecb9ddb](https://github.com/arnaud-ritti/openidea-plugins/commit/ecb9ddb872215e47258a279f1adac627a9b07775))
* **laravel:** end backtick strings at the line break in script route scan ([c9de3a9](https://github.com/arnaud-ritti/openidea-plugins/commit/c9de3a9a65b438d7a1caedb0256208631aebaf3f))
* **laravel:** find the rules argument of controller validate calls ([94d5aeb](https://github.com/arnaud-ritti/openidea-plugins/commit/94d5aeb68c2f89495ee24e8edd015cce383482e6))
* **laravel:** ignore vendor classes that share a project model name ([b1f3fe1](https://github.com/arnaud-ritti/openidea-plugins/commit/b1f3fe1ba05fa552f6180f79187d577aa23243cc))
* **laravel:** keep namespaced env functions quiet and end heredocs only at the exact label ([be1424e](https://github.com/arnaud-ritti/openidea-plugins/commit/be1424eaf3dcad6e3edaff62565f487c91f39927))
* **laravel:** keep unknown-name checks quiet for vendor components, aliasless registrations and partial indexes ([b075b5f](https://github.com/arnaud-ritti/openidea-plugins/commit/b075b5fa94a5ad61f343c9cd7ae935afeec71f76))
* **laravel:** map Context to its facade, match declared class names by word, gate bulk controller scan on the caret ([d51538b](https://github.com/arnaud-ritti/openidea-plugins/commit/d51538bd6b08807465f05ed25596481082ff7442))
* **laravel:** no panic on UTF-8 in the env check, relative quiet directories, sturdier ide-blade.json ([88bd4da](https://github.com/arnaud-ritti/openidea-plugins/commit/88bd4da452be061f4e9be1ab79eff31f9523ff48))
* **laravel:** offer the rules-to-array intention only on the rules array's own values ([6295443](https://github.com/arnaud-ritti/openidea-plugins/commit/62954436aa6f71061b3e1495ed368f74b557ffed))
* **laravel:** resolve the model file from the imported class name ([dcd6552](https://github.com/arnaud-ritti/openidea-plugins/commit/dcd6552e62d5e4257131b4bc1ac3689dbcb67055))
* **laravel:** send artisan test results to the test tree ([c532356](https://github.com/arnaud-ritti/openidea-plugins/commit/c53235676335819efc4d5379f987c37466351839))
* **laravel:** skip strings and heredocs in the env() check and report nested calls once ([c076dc9](https://github.com/arnaud-ritti/openidea-plugins/commit/c076dc9fe2fef07120e8d6d959e7227bff502397))
* **laravel:** tell Livewire and controller validate calls apart by their first argument ([964ba35](https://github.com/arnaud-ritti/openidea-plugins/commit/964ba353d1a5ef72a09724cdf2564504a421b4bb))
* **laravel:** track comments with a quote-aware scan in script route completion ([acf0332](https://github.com/arnaud-ritti/openidea-plugins/commit/acf03328c44f4a0f1430320afd1adb86bb787661))
