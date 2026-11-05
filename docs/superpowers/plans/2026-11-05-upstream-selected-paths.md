# `invocation.selected-paths`: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A plugin command run from the Project view or an editor tab menu can read every selected file, not only the first one it gets as its argument.

**Architecture:** Plugin API 1.2.0. A new method on the host-implemented `invocation` resource, `selected-paths: func() -> list<string>` (allowed: host interfaces may gain functions, §10 rule 2). The frontend menu passes `{ path, paths }` to the command's action; `runPluginCommand` sends `paths` to `plugin_run_command`, which fills `Invocation.selected_paths`; `wit_host.rs` returns it. The argument stays `paths[0]`, so 1.0/1.1 plugins see no change.

**Tech Stack:** WIT, Rust (`ide-plugin`, `ide-host`, `src-tauri`, SDK `openidea-plugin`), TypeScript (vitest), tauri-specta.

**Spec:** the issue draft below + `docs/superpowers/plans/2026-11-05-upstream-context-menus.md`.

## Issue draft (Feature request form)

**Title:** Plugins: let a command read every file selected in the Project view

**Problem:** A plugin command in `projectView.context` or `editorTab.context` gets the right-clicked file as its argument (`paths[0]`, `src/features/plugins/uiPoints.tsx:89`), and nothing else. With several files selected, the plugin can't act on all of them (create a snippet from the selected files, run a check on a selection, add files to a commit).

**Proposed solution:** In API 1.2, a new method on the invocation resource:
```wit
/// Files or folders the command was run on from a Project view or editor tab
/// menu, as absolute paths (the first one is also the command's argument);
/// empty otherwise.
selected-paths: func() -> list<string>;
```
No capability: the user picked these files, as with `text-editor.path`. The argument stays the first path, so existing plugins are unaffected.

**Acceptance criteria:**
- [ ] Selecting three files in the Project view and running a plugin's context menu command gives the plugin those three absolute paths, in selection order.
- [ ] The command's argument is still the first path.
- [ ] From Find Action, a shortcut or the editor menu, `selected-paths` is empty.
- [ ] It works the same in a remote project (`openidea-server`).
- [ ] A 1.1 plugin keeps loading and running unchanged.

**Open questions for maintainers:** API 1.2 is the first bump since 1.1: fine to carry it here, or batch with other 1.2 additions (`ui.set-context`, proposed separately)?

**Alternatives:** Pass the paths joined in the argument string (breaks plugins that expect one path); a separate `selection-changed` event (noisy, and racy with the menu).

## Global Constraints

- WIT package, `ENGINE_VERSION`, `API_VERSION` all become 1.2 (`1.2.0` / `"1.2.0"` / `"1.2"`); `sdk/rust/openidea-plugin/wit/` stays byte-identical to `wit/`.
- `tests/api/released/` stays at 1.1.0 (it is updated in the release commit, not here).
- At most `MAX_FIND` (10 000, `wit_host.rs`) paths are returned.
- Paths are absolute, as the menu got them (remote paths for remote windows).
- `pnpm gen:bindings` after the Tauri command change; commit `src/bindings.ts`.

## Review Focus

1. A remote window: `remote_host.rs`'s `plugin_run_command` handler must read `paths` too, or remote plugins always see `[]`. Test in Task 3 (`remote_plugins` test or the dispatcher test that covers `plugin_run_command`).
2. Every `Invocation { … }` literal (events, views, providers in `mod.rs:1385`, `views.rs`, tests) must default `selected_paths` to empty, not copy a stale selection. Compiler finds them; use `..Default::default()` only where a literal lists every field today.
3. `executeAction("plugin.x.cmd", "str")` from another feature still works (string argument, no paths). Test in Task 4.
4. Editor menu (`editor.context`) passes no paths (it runs with the focused editor). Test in Task 4.
5. A 1.1 component (the shipped official plugins before rebuild) links against the 1.2 host. Covered by `api_compat.rs`'s simulated-host tests; run them.

---

### Task 1: Bump the plugin API to 1.2.0

Skip if `upstream/main` already has `package openidea:plugin@1.2.0;` (e.g. `ui.set-context` landed first).

**Files:**
- Modify: `wit/*.wit` (9 files, `package openidea:plugin@1.1.0;` → `@1.2.0`), same in `sdk/rust/openidea-plugin/wit/*.wit`
- Modify: `crates/ide-extension/src/manifest.rs:35` (`ENGINE_VERSION = "1.2.0"`), `crates/ide-plugin/src/runtime.rs:36` (`API_VERSION = "1.2"`)
- Modify: `crates/ide-plugin/tests/fixtures/misbehave/src/lib.rs:8-9` (`@1.1.0` → `@1.2.0`)
- Modify: `plugins/README.md:40` (`openidea:plugin@1.2.0`), §10 "Released versions are frozen" sentence unchanged (1.2 isn't released)

- [ ] **Step 1:** Make the edits.
- [ ] **Step 2: Run** `cargo nextest run -p ide-plugin --test api_compat` and `cargo test --manifest-path sdk/rust/openidea-plugin/Cargo.toml --test api_frozen` → PASS (versions agree, SDK copy identical, 1.1 snapshot still a subset).
- [ ] **Step 3: Commit** — `feat(plugins): plugin API 1.2.0`

### Task 2: `selected-paths` in WIT, host and SDK

**Files:**
- Modify: `wit/workspace.wit` (`resource invocation`, after `editor`), copy to `sdk/rust/openidea-plugin/wit/workspace.wit`
- Modify: `crates/ide-plugin/src/types.rs:27` (`Invocation.selected_paths: Vec<PathBuf>`)
- Modify: `crates/ide-plugin/src/wit_host.rs` (the `invocation::HostInvocation` impl for `StoreData`)
- Modify: `sdk/rust/openidea-plugin/src/host.rs` (wasm `imp::invocation_selected_paths`, public `Invocation::selected_paths`), `sdk/rust/openidea-plugin/src/mock.rs` (state field, `invocation_selected_paths`, `MockHost::select_paths`)
- Modify: `crates/ide-plugin/tests/fixtures/jobs/plugin.toml` (command `selection`), `crates/ide-plugin/tests/fixtures/jobs/src/lib.rs` (`"selection" => Ok(Some(ctx.selected_paths().join("\n")))`)
- Test: `crates/ide-host/tests/plugins.rs`, `sdk/rust/openidea-plugin/src/mock.rs` tests

**Interfaces:**
- Produces:
  - WIT: `selected-paths: func() -> list<string>;` with the doc comment from the issue.
  - Rust host: `Invocation { client, workspace_root, editor, selected_paths: Vec<PathBuf> }` (derives `Default`).
  - SDK: `impl Invocation<'_> { pub fn selected_paths(&self) -> Vec<String> }`; mock: `MockHost::select_paths(&self, paths: &[&str])`, read back by `invocation().selected_paths()`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/ide-host/tests/plugins.rs
#[test]
fn commands_see_the_paths_they_were_run_on() {
    let f = fixture(Limits::default());
    f.install(&sample("jobs"));
    let (a, b) = (f.project.join("src/a.txt"), f.project.join("src/b.txt"));
    let inv = Invocation { selected_paths: vec![a.clone(), b.clone()], ..f.inv() };
    let out = f.host.run_command("jobs", "selection", Some(&a.to_string_lossy()), inv).unwrap();
    assert_eq!(out.as_deref(), Some(format!("{}\n{}", a.display(), b.display()).as_str()));
    assert_eq!(f.run("jobs", "selection", None).unwrap().as_deref(), Some(""));
}
```

```rust
// sdk mock.rs tests
#[test]
fn invocations_carry_selected_paths() {
    let host = MockHost::new();
    assert!(host.invocation().selected_paths().is_empty());
    host.select_paths(&["/p/a", "/p/b"]);
    assert_eq!(host.invocation().selected_paths(), ["/p/a", "/p/b"]);
}
```

(`sample("jobs")` resolves `crates/ide-plugin/tests/fixtures/jobs` like `sample("hello")`; adjust if the jobs tests use another helper. Use the constructor the other mock tests use if it isn't `MockHost::new()`.)

- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-host --test plugins commands_see_the_paths` and `cargo test --manifest-path sdk/rust/openidea-plugin/Cargo.toml invocations_carry` → compile errors (`selected_paths` unknown).
- [ ] **Step 3: Implement** the interfaces; fix every `Invocation { … }` literal the compiler reports (`src-tauri/src/commands/plugin.rs:328`, `remote_host.rs:370`, `mod.rs:1385`, `views.rs`, tests) with `selected_paths: Vec::new()` or `..Default::default()`. `wit_host.rs` returns at most `MAX_FIND` paths, `to_string_lossy`.
- [ ] **Step 4: Rebuild components** — `plugins/build.sh` (fixtures + official plugins; every component's WIT changed), then `plugins/build.sh --check`.
- [ ] **Step 5: Run, expect PASS** — the two commands above, then `cargo test --manifest-path sdk/rust/openidea-plugin/Cargo.toml` (`wit_coverage` sees the new wrapper) and `cargo nextest run -p ide-plugin -p ide-host`.
- [ ] **Step 6: Commit** — `feat(plugins): invocation.selected-paths`

### Task 3: Pass the selection through `plugin_run_command` (desktop and remote)

**Files:**
- Modify: `src-tauri/src/commands/plugin.rs:321-334` (`paths: Option<Vec<PathBuf>>` → `selected_paths: paths.unwrap_or_default()`)
- Modify: `crates/ide-host/src/plugins/remote_host.rs:362-375` (`let paths: Option<Vec<PathBuf>> = c.arg("paths")?;`)
- Modify: `src/bindings.ts` (regenerated)
- Test: the remote plugin test that calls `plugin_run_command` (`crates/ide-host/tests/remote_plugins.rs`; add `paths` to its call and assert with the `selection` command)

**Interfaces:**
- Produces: Tauri `plugin_run_command(pluginId, commandId, arg, editor, paths)`; bindings `pluginRunCommand(pluginId: string, commandId: string, arg: string | null, editor: PluginEditor | null, paths: string[] | null)`; remote dispatcher argument name `paths`.

- [ ] **Step 1: Write the failing test** in `remote_plugins.rs`: run `jobs`/`selection` over the remote dispatcher with `"paths": ["<root>/src/a.txt"]`, assert the result is that path.
- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-host --test remote_plugins` → empty result.
- [ ] **Step 3: Implement**; run `pnpm gen:bindings`.
- [ ] **Step 4: Run, expect PASS** — same command; `cargo check -p openidea` (src-tauri crate name as in its `Cargo.toml`).
- [ ] **Step 5: Commit** — `feat(plugins): send the menu's selection with plugin commands`

### Task 4: Menu items pass `{ path, paths }`

**Files:**
- Modify: `src/features/plugins/uiPoints.tsx:81-91` (`pluginMenuItems`)
- Modify: `src/features/plugins/points.tsx:150-152` (command action `run`)
- Modify: `src/features/plugins/store.ts:395-397` (`runPluginCommand`)
- Modify: `src/features/plugins/backend.ts:51,153,482` (`PluginsBackend.runCommand` signature, desktop and memory backends)
- Test: `src/features/plugins/__tests__/views.test.tsx` (`describe("plugin menus")`), `src/features/plugins/__tests__/plugins.test.tsx` (command run)

**Interfaces:**
- Consumes: bindings from Task 3.
- Produces:
  - `pluginMenuItems` → `{ kind: "action", action, arg: { path: paths[0], paths: [...paths] } }` for `projectView`/`editorTab`; `arg: undefined` for `editor` (unchanged).
  - `export function commandArgs(arg: unknown): [string | null, string[]]` in `points.tsx`: a string → `[arg, []]`; `{ path: string, paths: string[] }` → `[path, paths]`; anything else → `[null, []]`.
  - `runPluginCommand(pluginId: string, commandId: string, arg: string | null = null, paths: string[] = [])`.
  - `PluginsBackend.runCommand(pluginId, commandId, arg, editor, paths: string[])`.

- [ ] **Step 1: Write the failing tests** — update the existing expectation in `"projects context menu entries for their command's action"`:

```ts
expect(pluginMenuItems("projectView", ["/p/a.java", "/p/b.java"])).toEqual([
  { kind: "action", action: "plugin.p.check", arg: { path: "/p/a.java", paths: ["/p/a.java", "/p/b.java"] } },
]);
```

and add:

```ts
it("unpacks command arguments", () => {
  expect(commandArgs("x")).toEqual(["x", []]);
  expect(commandArgs({ path: "/a", paths: ["/a", "/b"] })).toEqual(["/a", ["/a", "/b"]]);
  expect(commandArgs(undefined)).toEqual([null, []]);
  expect(commandArgs({ path: 3 })).toEqual([null, []]);
});
```

plus, in `plugins.test.tsx`, running `plugin.p.check` with `{ path, paths }` reaches the memory backend's `runCommand` with `arg === "/p/a.java"` and `paths` equal to both paths.

- [ ] **Step 2: Run, expect FAIL** — `pnpm vitest run src/features/plugins/__tests__/views.test.tsx src/features/plugins/__tests__/plugins.test.tsx`.
- [ ] **Step 3: Implement** per the interfaces.
- [ ] **Step 4: Run, expect PASS** — same command, then `pnpm vitest run src/features/plugins` and `pnpm typecheck`.
- [ ] **Step 5: Commit** — `feat(plugins): pass the Project view selection to plugin commands`

### Task 5: Document, verify in the app, open the PR

**Files:**
- Modify: `plugins/README.md` API table (the `Invocation` / `TextEditor` rows near l.300): `Invocation::selected_paths()` → `invocation.selected-paths`, no capability, "files a Project view or editor tab menu ran the command on; empty otherwise". Note the 1.2 addition where §10 lists versions if the 1.1 PR did.

- [ ] **Step 1:** Edit README; `pnpm format:check`.
- [ ] **Step 2:** Run the checks in the index's step 6 (Rust, SDK, `plugins/build.sh --check`, frontend).
- [ ] **Step 3: `/verify`.** The `jobs` fixture as a dev plugin with `[[contributes.menus]] menu = "projectView.context" command = "selection"`: select three files, run it, screenshot the notification/result listing three paths. Non-visual otherwise: no before/after needed, say so.
- [ ] **Step 4: Commit** — `docs(plugins): document invocation.selected-paths`
- [ ] **Step 5: PR** — title `feat(plugins): let commands read every file selected in the Project view`, `Closes #<issue>`, template; review focus: WIT at 1.2.0, remote dispatcher, `commandArgs`. Link the `aritti.gitlab` registry PR (Create Snippet from selected files, `engines.openidea = "^1.2"`).
