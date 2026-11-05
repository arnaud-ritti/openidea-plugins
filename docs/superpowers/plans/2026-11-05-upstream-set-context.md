# `ui.set-context`: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plugin code sets its own context keys (`plugin.<id>.<key>`) so its menu items and commands show only when its state says so ("this file is an entity", "a cluster is connected").

**Architecture:** Plugin API 1.2.0 (shares the bump with `selected-paths`; see that plan's Task 1). New host import `ui.set-context(key, value)`. The host validates it, keeps the keys per plugin and window (like status items in `api.rs`), emits `PluginEvent::ContextChanged`, and answers `plugin_context_keys` for windows that (re)load. The frontend writes them into the existing `useContextKeys` store, where every `when` clause already looks. Keys are dropped when the plugin stops (`clear_plugin`), not on idle reclaim.

**Tech Stack:** WIT, Rust (`ide-plugin`, `ide-host`, `src-tauri`, SDK), TypeScript (vitest), tauri-specta.

**Spec:** the issue draft below + `docs/superpowers/plans/2026-11-05-upstream-context-menus.md`.

## Issue draft (Feature request form)

**Title:** Plugins: let plugin code set context keys for its `when` clauses

**Problem:** `when` clauses of plugin menus and commands can only test the IDE's own keys (`editorLangId`, `editorPath`, `editorFocus`…; `src/platform/context.ts`). A plugin can limit "Copy DDL" to Java files, but not to Java files that are JPA entities; it can't hide "Disconnect" while no cluster is connected. Its items are always listed and the commands report "doesn't apply here" when run.

**Proposed solution:** In API 1.2, a host function:
```wit
/// Sets the context key `plugin.<plugin id>.<key>` that `when` clauses test,
/// in the window the current command or event came from (every window when
/// there is none). `value` is a JSON scalar (`true`, `3`, `"text"`); none
/// removes the key. Keys: 1–64 characters of `A-Za-z0-9._-`; at most 100 per
/// plugin. The keys go away when the plugin stops.
set-context: func(key: string, value: option<string>) -> result<_, error>;
```
Example: on `editor-focused`, `set-context("isEntity", Some("true"))`, and `when = "plugin.aritti.jpa-assist.isEntity"` on the menu item. No capability (like status items).

**Acceptance criteria:**
- [ ] After a plugin sets `isEntity` to `true` from an `editor-focused` event, its menu item with `when = "plugin.<id>.isEntity"` shows in that window's editor menu, and hides after it sets `false` or removes it.
- [ ] A key set with no invoking window applies to every window; one set from a window's event applies to that window only.
- [ ] Keys survive a window reload and the plugin's idle reclaim; they're removed when the plugin is disabled, uninstalled or fails.
- [ ] A plugin can't set another plugin's or an IDE key: the host always prefixes `plugin.<id>.`.
- [ ] Invalid keys, non-scalar or invalid JSON values, and a 101st key return `invalid` without changing anything.
- [ ] Works in remote projects.

**Open questions for maintainers:** scope per window vs global only; the 100-key cap; JSON text vs a typed WIT variant (JSON matches `settings.get`).

**Alternatives:** Commands that check and report (current workaround); a `when` key per file computed by a provider (needs a new export: not allowed in 1.x without a new world).

## Global Constraints

- Frontend key = `plugin.${pluginId}.${key}`; the plugin never chooses the prefix.
- Key regex `^[A-Za-z0-9._-]{1,64}$`; value a JSON scalar (bool, number, string ≤ 1024 bytes); `MAX_CONTEXT_KEYS = 100` per plugin (counted across windows).
- Window scope rule identical to `show-notification` (`self.client()`; none → every window).
- Removal on `clear_plugin` (`api.rs:224`), not on idle reclaim.
- WIT 1.2.0 (bump from the `selected-paths` plan, Task 1, if not on `upstream/main` yet).
- `pnpm gen:bindings`; commit `src/bindings.ts`.

## Review Focus

1. A window-scoped key and a global key with the same name: the window's value wins in that window; removing the window's key falls back to the global value. Test in Task 2.
2. Window closed: its keys must not pile up. Drop them when the host learns the window is gone, or accept the leak with a `ponytail:` note naming the cap (100 per plugin bounds it). Decide in Task 2, test whichever.
3. `value = Some("null")` or `Some("[1]")`: `invalid`, key unchanged. Test in Task 2.
4. Remote windows: `forwarded()` in `remote_host.rs` must include `ContextChanged`, and the remote dispatcher must serve `plugin_context_keys`. Test in Task 3.
5. Plugin reloaded (dev plugin rebuild): `clear_plugin` runs, keys go; the plugin sets them again on its next event. Check in `/verify`.

---

### Task 1: Bump to 1.2.0 (if needed)

Same as `2026-11-05-upstream-selected-paths.md` Task 1. Skip when `upstream/main` already has `@1.2.0`.

### Task 2: `set-context` in WIT, host and SDK

**Files:**
- Modify: `wit/host.wit` (`interface ui`, after `remove-status-item`), copy to `sdk/rust/openidea-plugin/wit/host.wit`
- Modify: `crates/ide-plugin/src/wit_host.rs` (`impl ui::Host for StoreData`: validation, `self.host.set_context(...)`)
- Modify: `crates/ide-plugin/src/runtime.rs:58` (`HostApi::set_context`)
- Modify: `crates/ide-host/src/plugins/api.rs` (`contexts` map in the shared state next to `status`, `set_context`, `context_keys(client)`, removal in `clear_plugin`)
- Modify: `crates/ide-host/src/plugins/types.rs:163` (`PluginEvent::ContextChanged`, `PluginContextKey`)
- Modify: `crates/ide-host/src/plugins/mod.rs:1251` (`PluginHost::context_keys(client)`, next to `status_items`)
- Modify: SDK `src/host.rs` (wasm + public `host::set_context`), `src/mock.rs` (`MockHost::context()` getter)
- Modify: `crates/ide-plugin/tests/fixtures/jobs/plugin.toml` + `src/lib.rs`: command `context` that calls `host::set_context` with the argument split as `key=value` (`key=` removes)
- Test: `crates/ide-host/tests/plugins.rs`, SDK mock tests

**Interfaces:**
- Produces:
  - WIT: `set-context: func(key: string, value: option<string>) -> result<_, error>;`
  - `HostApi::set_context(&self, client: Option<&str>, key: &str, value: Option<String>) -> Result<(), HostError>` (value already validated JSON text).
  - `PluginEvent::ContextChanged { plugin_id: String, key: String, value: Option<String> }` (`#[serde(rename_all = "camelCase")]`), emitted to the scope's client (or `None`).
  - `pub struct PluginContextKey { pub plugin_id: String, pub key: String, pub value: String }` (serde camelCase, specta `Type`).
  - `PluginHost::context_keys(&self, client: Option<&str>) -> Vec<PluginContextKey>`: global keys overlaid with that window's.
  - SDK: `pub fn set_context(key: &str, value: Option<&str>) -> Result<(), Error>`; `MockHost::context(&self) -> BTreeMap<String, String>`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn plugins_set_their_own_context_keys() {
    let f = fixture(Limits::default());
    f.install(&sample("jobs"));
    f.take_events();
    f.run("jobs", "context", Some("connected=true")).unwrap();
    let changed: Vec<_> = f.take_events().into_iter().filter_map(|(client, e)| match e {
        PluginEvent::ContextChanged { plugin_id, key, value } => Some((client, plugin_id, key, value)),
        _ => None,
    }).collect();
    assert_eq!(changed, [(Some("main".into()), "jobs".into(), "connected".into(), Some("true".into()))]);
    assert_eq!(f.host.context_keys(Some("main")).len(), 1);
    assert!(f.host.context_keys(Some("other")).is_empty(), "window-scoped");

    for bad in ["bad key=true", "x=null", "x=[1]", "x={"] {
        assert!(f.run("jobs", "context", Some(bad)).unwrap_err().contains("invalid"), "{bad}");
    }

    f.run("jobs", "context", Some("connected=")).unwrap();
    assert!(f.host.context_keys(Some("main")).is_empty());

    f.run("jobs", "context", Some("connected=true")).unwrap();
    f.host.set_enabled("jobs", false).unwrap();
    assert!(f.host.context_keys(Some("main")).is_empty(), "cleared when the plugin stops");
}
```

Add one more test for the cap (101st key → `invalid`) and the window/global overlay (Review Focus 1), and an SDK mock test: `host::set_context("a", Some("1"))` → `host.context()["a"] == "1"`; `None` removes it.

- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-host --test plugins plugins_set_their_own_context_keys` → compile errors.
- [ ] **Step 3: Implement** per the interfaces. Validation lives in `wit_host.rs` (key regex, `serde_json::from_str::<serde_json::Value>` must be `Bool | Number | String`, string ≤ 1024 bytes) and returns `wit::Error::Invalid(..)`; the cap lives in `api.rs` (it owns the map) and returns `HostError::Invalid`.
- [ ] **Step 4: Rebuild** — `plugins/build.sh`, then `plugins/build.sh --check`.
- [ ] **Step 5: Run, expect PASS** — the test, then `cargo nextest run -p ide-plugin -p ide-host` and `cargo test --manifest-path sdk/rust/openidea-plugin/Cargo.toml`.
- [ ] **Step 6: Commit** — `feat(plugins): ui.set-context for plugin when clauses`

### Task 3: Serve the keys to windows (desktop and remote)

**Files:**
- Modify: `src-tauri/src/commands/plugin.rs` (new `plugin_context_keys(window, state) -> Vec<PluginContextKey>` next to `plugin_status_items` l.363), `src-tauri/src/commands/mod.rs:473` (register it)
- Modify: `crates/ide-host/src/plugins/remote_host.rs` (`forwarded()` l.105–120 adds `ContextChanged`; dispatcher `plugin_context_keys` next to `plugin_status_items` l.392, with `c.window`)
- Modify: `src/bindings.ts` (regenerated)
- Test: `crates/ide-host/tests/remote_plugins.rs` (key set by a remote window's command reaches that window and `plugin_context_keys`)

**Interfaces:**
- Produces: bindings `pluginContextKeys(): Promise<PluginContextKey[]>`; `PluginEvent` union gains `{ type: "contextChanged"; pluginId: string; key: string; value: string | null }`.

- [ ] **Step 1: Write the failing remote test.**
- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-host --test remote_plugins`.
- [ ] **Step 3: Implement**; `pnpm gen:bindings`.
- [ ] **Step 4: Run, expect PASS** — same command.
- [ ] **Step 5: Commit** — `feat(plugins): send plugin context keys to windows`

### Task 4: Apply the keys in the frontend

**Files:**
- Modify: `src/features/plugins/runtimeUi.tsx` (`handleRuntimeEvent` l.240: `case "contextChanged"`; `refreshContextKeys()` next to `refreshStatusItems()` l.136 and called where it is)
- Modify: `src/features/plugins/backend.ts` (`PluginsBackend.contextKeys()`, desktop + memory backends)
- Test: `src/features/plugins/__tests__/plugins.test.tsx`

**Interfaces:**
- Consumes: `setContext(key, value)` from `@/platform/context`.
- Produces: `export function applyPluginContext(pluginId: string, key: string, value: string | null): void` in `runtimeUi.tsx` — `setContext(`plugin.${pluginId}.${key}`, value === null ? undefined : JSON.parse(value))`.

- [ ] **Step 1: Write the failing test**

```ts
it("applies plugin context keys", () => {
  expect(handleRuntimeEvent({ type: "contextChanged", pluginId: "p", key: "isEntity", value: "true" })).toBe(true);
  expect(evaluateWhen("plugin.p.isEntity")).toBe(true);
  handleRuntimeEvent({ type: "contextChanged", pluginId: "p", key: "kind", value: "\"pod\"" });
  expect(evaluateWhen("plugin.p.kind == pod")).toBe(true);
  handleRuntimeEvent({ type: "contextChanged", pluginId: "p", key: "isEntity", value: null });
  expect(evaluateWhen("plugin.p.isEntity")).toBe(false);
});
```

- [ ] **Step 2: Run, expect FAIL** — `pnpm vitest run src/features/plugins/__tests__/plugins.test.tsx -t "context keys"`.
- [ ] **Step 3: Implement** per the interfaces; `refreshContextKeys()` applies every key from `pluginsBackend().contextKeys()`.
- [ ] **Step 4: Run, expect PASS** — same command, then `pnpm vitest run src/features/plugins`, `pnpm typecheck`.
- [ ] **Step 5: Commit** — `feat(plugins): apply plugin context keys to when clauses`

### Task 5: Document, verify in the app, open the PR

**Files:**
- Modify: `plugins/README.md` API table (row next to `host::set_status_item`): `host::set_context(key, value)` → `ui.set-context`, no capability, scope, prefix, limits, cleared when the plugin stops; one line in the `menus`/`commands` `when` docs pointing to `plugin.<id>.<key>`.

- [ ] **Step 1:** Edit README; `pnpm format:check`.
- [ ] **Step 2:** Run the checks in the index's step 6.
- [ ] **Step 3: `/verify`.** `aritti.jpa-assist` built against this branch: an `editor.context` item with `when = "plugin.aritti.jpa-assist.isEntity"`; screenshot the editor menu in an entity file (item shown) and in a plain Java class (item hidden). Reload the window: the item still follows the key.
- [ ] **Step 4: Commit** — `docs(plugins): document ui.set-context`
- [ ] **Step 5: PR** — title `feat(plugins): let plugin code set context keys for when clauses`, `Closes #<issue>`, screenshots, review focus: scope rules, cap, clear on stop vs idle reclaim, remote forwarding. Link the `aritti.jpa-assist` registry PR.
