# `when` clauses that test project files: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A plugin's `when` clause can say `workspaceContains:<path>`, true while that file or folder exists in the window's project, so menu items and commands only show in projects they apply to.

**Architecture:** No WIT or manifest-format change. `crates/ide-extension/src/when.rs` already tokenizes `workspaceContains:.devcontainer/devcontainer.json` as one key; it gains a check that the path stays in the project. The plugins frontend feature collects these keys from plugin declarations, checks each path with `fsReadDir`, and keeps the context key current on project switch, plugin changes and `fsChanged`.

**Tech Stack:** Rust (`ide-extension`), TypeScript/React (zustand context store, vitest).

**Spec:** the issue draft below + `docs/superpowers/plans/2026-11-05-upstream-context-menus.md`.

## Issue draft (Feature request form)

**Title:** Plugins: `when` clauses that test whether a project contains a file

**Problem:** A plugin's menu items and commands can only be limited by the IDE's context keys (`editorLangId`, `editorFocus`…). A plugin that only makes sense in some projects (Dev Containers: `.devcontainer/devcontainer.json`; Spring Boot: `pom.xml` with Spring; Symfony: `bin/console`) has its commands in every project's menus and in Find Action.

**Proposed solution:** A context key `workspaceContains:<path>` usable in any plugin `when` clause: true while `<path>` (relative to the window's project root, a file or a folder, no glob) exists. For example:
```toml
[[contributes.menus]]
menu = "projectView.context"
command = "start"
when = "workspaceContains:.devcontainer/devcontainer.json"
```
An IDE that doesn't know the key treats it as false, so a plugin using it still installs on older IDEs (its items stay hidden there).

**Acceptance criteria:**
- [ ] A menu item with `when = "workspaceContains:a/b.txt"` shows in a project that has `a/b.txt` and not in one that doesn't.
- [ ] Creating or deleting `a/b.txt` shows or hides the item without reopening the project.
- [ ] Switching the window to another project re-evaluates the key.
- [ ] `workspaceContains:` with an absolute path, a `..` segment or no path is refused when the plugin is installed, with a message naming the clause.
- [ ] Commands' `when` (Find Action) honours the key too.

**Alternatives:** Users ignore the item; the command reports "not a dev container project" when run (what `aritti.devcontainer` does now).

**Open questions for maintainers:** key name (`workspaceContains:` follows VS Code's activation event); globs left out on purpose (one `fsReadDir` per key; globs would need a walk).

## Global Constraints

- Key prefix exactly `workspaceContains:`; path relative to the window's project root, `/`-separated, tokens limited by the existing when grammar (`[\w.\-:/]`).
- No new user-visible strings (no text catalog change).
- Features never import each other: the code lives in `src/features/plugins/`.
- Comments sparse; match surrounding style.

## Review Focus

1. Hidden folders (`.devcontainer/`): `fsReadDir` must list them. Test: a key on `.devcontainer/devcontainer.json` resolves true (Task 2).
2. A window with no project (LightEdit, welcome screen): every key is false, no `fsReadDir` call. Test in Task 2.
3. `fsReadDir` failing (parent folder missing, remote error): key false, no unhandled rejection. Test in Task 2.
4. Gitignored targets: `fsChanged` skips ignored folders (`ExcludedDirsChanged`), so a key on an ignored path updates only on project switch. Documented in README (Task 3), not fixed.
5. A plugin disabled or uninstalled: its keys are no longer refreshed; a stale `true` must not matter because its items are gone. No test needed; check during `/verify`.

---

### Task 1: Validate `workspaceContains:` paths

**Files:**
- Modify: `crates/ide-extension/src/when.rs` (`parse_unary`, `Some(Tok::Id(_))` arm; tests module)

**Interfaces:**
- Produces: `validate_when` refuses `workspaceContains:` keys whose path is empty, starts with `/` or `\`, contains a `..` segment or a `\`. Error text: `workspaceContains: needs a relative path inside the project: <src>`.

- [ ] **Step 1: Write the failing test** in `when.rs` tests:

```rust
#[test]
fn workspace_contains_paths_stay_in_the_project() {
    for ok in ["workspaceContains:pom.xml", "workspaceContains:.devcontainer/devcontainer.json", "!workspaceContains:a/b && editorFocus"] {
        assert!(validate_when(ok).is_ok(), "{ok}");
    }
    for bad in ["workspaceContains:", "workspaceContains:/etc/passwd", "workspaceContains:../x", "workspaceContains:a/../../x"] {
        assert!(validate_when(bad).unwrap_err().contains("workspaceContains"), "{bad}");
    }
}
```

- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-extension workspace_contains` → the `bad` cases return `Ok`.
- [ ] **Step 3: Implement** — keep the key text in `Tok::Id(&str)` (already there); in the `Tok::Id(id)` arm, if `id.strip_prefix("workspaceContains:")` is `Some(path)`, reject per the interface above.
- [ ] **Step 4: Run, expect PASS** — same command; also `cargo nextest run -p ide-extension` (all when tests).
- [ ] **Step 5: Commit** — `feat(plugins): validate workspaceContains: paths in when clauses`

### Task 2: Keep `workspaceContains:` context keys current

**Files:**
- Create: `src/features/plugins/workspaceContains.ts`
- Modify: `src/features/plugins/index.ts:146` (add `startWorkspaceContains()` to `disposers`, next to `startViewUi`)
- Test: `src/features/plugins/__tests__/workspaceContains.test.ts`

**Interfaces:**
- Consumes: `usePlugins.getState().ui` (`{ pluginId, point, key, decl }[]`, `decl.when?: string | null`), `useProject.getState().project?.root`, `commands.fsReadDir(path)` → `FileEntry[]` (`name`), `onIdeEvent("fsChanged", e => …)` (`created`, `removed`: absolute paths), `setContext(key, value)`.
- Produces:
  - `export function workspaceContainsKeys(whens: readonly (string | null | undefined)[]): string[]` — unique keys, matched with `/workspaceContains:[\w.\-:/]+/g`.
  - `export async function refreshWorkspaceContains(root: string | null, keys: readonly string[], list: (dir: string) => Promise<string[]>): Promise<void>` — per key: split the path into parent folder and name, `list(`${root}/${parent}`)` (root itself when no parent), `setContext(key, names.includes(name))`; `root === null` or a rejected `list` → `false`.
  - `export function startWorkspaceContains(): () => void` — refreshes on start, on `usePlugins` `ui` change, on project root change, and on `fsChanged` whose `created`/`removed` contain a path under the root; returns the disposer.

- [ ] **Step 1: Write the failing tests**

```ts
describe("workspaceContains", () => {
  it("collects keys from when clauses", () => {
    expect(workspaceContainsKeys(["workspaceContains:a/b.txt && editorFocus", null, "!workspaceContains:pom.xml", "workspaceContains:a/b.txt"]))
      .toEqual(["workspaceContains:a/b.txt", "workspaceContains:pom.xml"]);
  });

  it("sets each key from its parent folder's listing", async () => {
    const list = vi.fn(async (dir: string) => (dir === "/p/.devcontainer" ? ["devcontainer.json"] : ["src"]));
    await refreshWorkspaceContains("/p", ["workspaceContains:.devcontainer/devcontainer.json", "workspaceContains:pom.xml"], list);
    expect(getContext()["workspaceContains:.devcontainer/devcontainer.json"]).toBe(true);
    expect(getContext()["workspaceContains:pom.xml"]).toBe(false);
    expect(list).toHaveBeenCalledWith("/p");
  });

  it("is false without a project or when the folder can't be read", async () => {
    const list = vi.fn(async () => { throw new Error("missing"); });
    await refreshWorkspaceContains("/p", ["workspaceContains:x/y"], list);
    expect(getContext()["workspaceContains:x/y"]).toBe(false);
    list.mockClear();
    await refreshWorkspaceContains(null, ["workspaceContains:x/y"], list);
    expect(list).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run, expect FAIL** — `pnpm vitest run src/features/plugins/__tests__/workspaceContains.test.ts` → module not found.
- [ ] **Step 3: Implement** `workspaceContains.ts` per the interfaces; `startWorkspaceContains` passes `(dir) => call(commands.fsReadDir(dir)).then((es) => es.map((e) => e.name))`. Wire it in `index.ts`.
- [ ] **Step 4: Run, expect PASS** — same command, then `pnpm vitest run src/features/plugins`.
- [ ] **Step 5: Commit** — `feat(plugins): workspaceContains: context keys for plugin when clauses`

### Task 3: Document, verify in the app, open the PR

**Files:**
- Modify: `plugins/README.md` (the `menus` / `commands` rows of the *Contribution points* table, or the paragraph that explains `when`): one sentence on `workspaceContains:<path>` (relative, file or folder, no glob, ignored folders update on project switch only, false on IDEs that don't know it).

- [ ] **Step 1:** Edit README, run `pnpm format:check`.
- [ ] **Step 2:** Run the checks in the index's step 6 (Rust + frontend parts).
- [ ] **Step 3: `/verify`.** Dev plugin in `fixtures/` project with a `projectView.context` item `when = "workspaceContains:.devcontainer/devcontainer.json"`: screenshot the menu without the file (item absent), create the file from the Project view, screenshot again (item present).
- [ ] **Step 4: Commit** — `docs(plugins): document workspaceContains: in when clauses`
- [ ] **Step 5: PR** — title `feat(plugins): when clauses that test whether the project contains a file`, `Closes #<issue>`, template filled, the two screenshots, the registry PR of `aritti.devcontainer` using it.
