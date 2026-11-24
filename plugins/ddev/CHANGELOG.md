# Changelog

## [0.4.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ddev-v0.3.0...ddev-v0.4.0) (2026-11-24)


### Features

* **ddev:** open a menu from the status bar item ([756d555](https://github.com/arnaud-ritti/openidea-plugins/commit/756d5553ac00d5eec78a503a0478064e7763503e))


### Bug Fixes

* **ddev:** offer Start in the status bar menu when the status can't be read ([0ef1087](https://github.com/arnaud-ritti/openidea-plugins/commit/0ef108705d0db59f0ee98d4664344a63b1acc15b))

## [0.3.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ddev-v0.2.0...ddev-v0.3.0) (2026-11-18)


### Features

* **ui-kit:** grouped hub sections, accessible grids, error and permission states ([de9e5ab](https://github.com/arnaud-ritti/openidea-plugins/commit/de9e5abc728029e5875e942bdbf6ae931f193cdf))


### Bug Fixes

* consistent command titles and spacing in messages ([86ecc8d](https://github.com/arnaud-ritti/openidea-plugins/commit/86ecc8d4f27bd0b4ed2eb1b2f7ee68cd5bd76374))
* correct manifest mismatches and unsafe defaults ([d555f8d](https://github.com/arnaud-ritti/openidea-plugins/commit/d555f8d68e191d63e2b0d69e5f52bcdc26561c0d))
* **ddev:** hide the status item outside DDEV projects, confirm sharing ([5722881](https://github.com/arnaud-ritti/openidea-plugins/commit/57228819fa33ed8152e02d41c7b10d569a82ee9a))
* **ddev:** name the project before delete, explain why the runtime is unavailable ([ae8045c](https://github.com/arnaud-ritti/openidea-plugins/commit/ae8045cbed170733af3a9ced09df7dc5cfba454b))
* **ddev:** render services from cache; per-project snapshots ([c317dc4](https://github.com/arnaud-ritti/openidea-plugins/commit/c317dc4f2f4140ef741d82700aa7c5be530f708a))
* **ddev:** surface background refresh failures; per-root failures; one Prefs per render ([c2f3905](https://github.com/arnaud-ritti/openidea-plugins/commit/c2f390551ff718e59e8636b853ba2d48d877d2df))
* **laravel:** register the new UI message keys in the shared catalog ([8e02c5e](https://github.com/arnaud-ritti/openidea-plugins/commit/8e02c5e95861eb4825103eb1895a00cacdee00d4))
* one name per action and plain messages ([5f524dd](https://github.com/arnaud-ritti/openidea-plugins/commit/5f524dd4c3ef2fb1c3e4913cdf56451d3c2e6fbe))
* short setting titles ([3a12e66](https://github.com/arnaud-ritti/openidea-plugins/commit/3a12e6689794c36e9966536ea31838ec6a671018))
* standard permission copy everywhere ([93aa4d7](https://github.com/arnaud-ritti/openidea-plugins/commit/93aa4d74846487a98a4d1c30a2494a9cf96a9b43))
* **symfony:** run Send Request and database probes without blocking, trim console polls ([5ec5053](https://github.com/arnaud-ritti/openidea-plugins/commit/5ec50534a28e4e81d1365846c85a0b7f23e5dce1))
* **ui-kit:** plain action errors, named plugin settings, shallow docks ([064946a](https://github.com/arnaud-ritti/openidea-plugins/commit/064946a3c8091f7ce5fd6e784d9dbaf0cf728279))
* **ui-kit:** spell the row action Copy as cURL in every plugin ([7e54b13](https://github.com/arnaud-ritti/openidea-plugins/commit/7e54b137cdbc637cb23c538933661b45ef337db1))

## [0.2.0](https://github.com/arnaud-ritti/openidea-plugins/compare/ddev-v0.1.0...ddev-v0.2.0) (2026-11-16)


### Features

* adopt plugin API 1.5 (net:ask, extension points, run-target validation, project notifications) ([08db51a](https://github.com/arnaud-ritti/openidea-plugins/commit/08db51a54c99b3b4ae414ab790be2a0783add3c7))
* **ddev:** port DDEV Integration with API 1.4 workflows and parity checks ([575be26](https://github.com/arnaud-ritti/openidea-plugins/commit/575be26f35318a57643c84c1728a10c501c0cdeb))
* **ui:** consolidate plugin panels and native service actions ([16bbb3e](https://github.com/arnaud-ritti/openidea-plugins/commit/16bbb3eba4999c7dbd0b2d1215ee3db3116ae4c3))

## 0.1.0

Initial OpenIDEA release of DDEV integration.

- Services tree, inspect/configuration/help/database/log tabs, DDEV icons.
- Lifecycle commands, container shells, browser links and PHP/Node/Composer tools.
- Automatic status refresh, state-aware actions and a native status/action picker.
- Daily automatic update checks with preferences and optional grants.
- Project PHP-version hints; offline project/global schema completion, nested values,
  hover documentation and YAML/schema diagnostics.
- Source-matched HEAD/RC version comparison, final-error extraction and Start fallback
  when DDEV is installed but project description is unavailable.
- Managed database connection configuration without writing passwords.
- Configuration/help without runtime permissions; persistent service selection.
- Native guided setup, environment diagnostics and reviewed project preferences.
- One-time onboarding and actionable recovery/update notifications.
- Open generated/changed configuration once after Configure, with idle-reclaim persistence.

- Revalidate runtime actions independently of cached UI state, invalidate removed
  configuration, refresh on override/Compose/preference saves, preserve recovery
  actions without describe, and fall back to the web-service URL.
- Republish cached window contexts and avoid cross-project status text; add a
  standalone compiled-Wasm smoke harness with 50 ms editing-call deadlines.

- Complete values immediately after mapping colons, keep diagnostics within the
  configured cap, scope log tabs to project/service, explicitly target their DDEV project and
  reject duplicate owned
  data-source IDs without writing. Extend Unicode/partial-YAML and Wasm coverage.

- Keep failure history, pending Configure follow-ups and unsaved Setup drafts
  independent per project. Recognize background recovery, guard notification
  actions by originating project and discard removed-service selections on reuse.
- Open installation/release/issue links and reveal Services without an open project.

- Use explicit raw PHP/Node execution, initialize all configured projects at
  startup, add Reset Changes, accept inline TOML preferences and apply database
  maintenance immediately after Save even with periodic watching off.

- Restore the invoking project's toolbar selection before dispatch, constrain
  filesystem refreshes to its root, watch parent directory changes and refine
  command availability. Add a host-evaluator manifest-condition smoke harness.

- Update bundled project/global schemas to DDEV v1.25.5, including new hooks,
  Node/agent settings and version/type values; label empty enum completions.
- Add fixture-backed actual-Wasm coverage for all 27 commands and Services/setup
  workflows, and consolidate the final feature/API/verification assessment.

- Retain all valid Compose service identifiers, including leading punctuation,
  and pass shell/log service names as bound option values.

- Republish cached command contexts on action lookup/editor focus after scheduled
  refreshes, and preserve per-project live selection without a storage grant.

- Adopt API 1.4 / SDK v0.1.40: native Database sources/secure credentials and
  connection requests, project DDEV run target with configurable path mapping,
  keyed native terminal exits/environment and window-scoped status.
- Respect host-owned global/project executable selection; keep safe legacy
  Database fallback/migration and user-owned run-target selection.
- Refresh after terminal completion even without watching; open configuration
  only on successful exit, including unchanged content.
- Add readable Services Overview, direct logs/service links, redacted Inspect,
  preference-first Setup and the inherited provider logo on the Services root.

- Preserve native Database connections by skipping identical automatic source/
  credential writes; handle rotation, source removal and partial setup retry.
- Withdraw stale execution targets after external preference edits/runtime
  failures and register again on recovery; omit unavailable Database actions.
- Record the remaining generic-target Composer/cwd and pre-launch validation
  differences.

- Complete three source/integration/editor passes: restore Stop/Restart
  recovery when describe is unavailable; preserve managed source identity after
  project moves; tie Configure follow-up to the exact host operation id.
- Reject overlapping Configure requests with different arguments and ignore
  stale completion events; support Windows/UNC case-insensitive schema paths
  without widening Unix scope.
- Reset Configure follow-up expiry for a new host operation while preserving the
  previous deadline when refocusing the same operation, including after reclaim.
- Suppress repeated runtime recovery alerts throughout one outage even when the
  error reason changes; retain idle-reclaim and older storage-marker compatibility.
- Add an opt-in disposable-container runner for actual PHP/Node/Composer/SQL,
  compiled-Wasm target wrapping and Xdebug DBGp connect-back/source-URI checks.
