# Changelog

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
