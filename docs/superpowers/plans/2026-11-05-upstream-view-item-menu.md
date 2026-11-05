# Right-click menu on plugin view items: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A plugin declares commands for the right-click menu of the items of its own `tree` and `list` views; the command runs with the item's key as its argument.

**Architecture:** VS Code's `view/item/context`, adapted to the existing `[[contributes.menus]]` point: a fifth menu id `view.item.context` with a required `view` field. No WIT change (`tree-item` can't gain fields, §10). The host validates the declaration (`ui_points.rs`); the frontend builds the menu from declarations whose `when` holds with two extra keys, `viewItem` (the key) and `viewItemKind` (the key up to its first `:`), and `PluginView` shows it on right-click.

**Tech Stack:** Rust (`ide-host`), TypeScript/React (`src/ui` `Tree`, `showContextMenu`, vitest).

**Spec:** the issue draft below + `docs/superpowers/plans/2026-11-05-upstream-context-menus.md`.

## Issue draft (Feature request form)

**Title:** Plugins: right-click menu on the items of a plugin's tree and list views

**Problem:** A plugin tool window can show a tree (Kubernetes resources, JPA entities, merge requests) but its items have no right-click menu: `view-event` has `select`, `activate`, `expand` and `collapse` only, and the IDE opens a context menu only on editable `Table` rows. Actions on one item (logs, delete, open, copy) have to go in the toolbar and act on "the selected item", or be double-click only.

**Proposed solution:** A new menu id for `[[contributes.menus]]`:
```toml
[[contributes.menus]]
menu = "view.item.context"
view = "resources"                # one of the plugin's own [[contributes.views]]
command = "pod-logs"
when = "viewItemKind == pod"      # viewItem: the item key; viewItemKind: the key up to its first ':'
```
Right-clicking an item of that view lists the matching commands; the command runs with the item's key as its argument (`run-command(id, ctx, Some(key))`). Declarative only: no WIT change.

**Acceptance criteria:**
- [ ] Right-clicking an item of a plugin tree or list view shows the plugin's `view.item.context` commands for that view, titled with the command title.
- [ ] Choosing one runs the command with the item's key as its argument.
- [ ] `when` sees `viewItem` and `viewItemKind`; `viewItemKind == pod` matches the key `pod:default/nginx` and not `svc:default/web`.
- [ ] No declared item for the view (or none matching) → no menu opens.
- [ ] A manifest with `view.item.context` and no `view`, an undeclared `view`, or a `view` on another menu is refused, naming the problem.
- [ ] The menu works from the keyboard (context-menu key / Shift+F10 on the focused item), like other trees.

**Open questions for maintainers:**
1. **Compatibility.** An IDE that doesn't know `view.item.context` refuses the whole manifest (`unknown menu`). Should this ship with an API bump (plugins then require `engines.openidea = "^1.2"`), or should unknown menu ids be skipped with a warning, like unknown contribution types?
2. **Item kinds.** `tree-item` can't gain a `context-value` field in 1.x. Proposed: `viewItemKind` = the key up to its first `:` (a convention plugins opt into). Alternative: `when` gains a prefix operator.

**Alternatives:** Toolbar buttons acting on the selected item (what `aritti.kube-explorer` does now); a `view-event::other` "item-menu" event the plugin answers with a list of actions (needs code at menu-open time, slower, and no `when`).

## Global Constraints

- Menu id exactly `view.item.context`; manifest field `view`; context keys exactly `viewItem`, `viewItemKind`.
- `view` must name one of the same package's `[[contributes.views]]` ids.
- The command argument is the item key, unchanged.
- No new user-visible strings (labels are command titles).
- `PluginView` must not import `uiPoints.tsx` (it imports `PluginView`): the menu comes in as a prop.
- `MENU_IDS` in `ui_points.rs` and the `MenuDecl.menu` union in `uiPoints.tsx` list the same ids.

## Review Focus

1. Same command declared for two views: decl keys must differ (`view.item.context/<view>/<command>`), or the second replaces the first. Test in Task 1.
2. Right-click on empty space in the tree (`node === null`): no menu, no error. Test in Task 3.
3. Item keys containing `:` more than once (`pod:ns:name`): `viewItemKind` = `pod`. Test in Task 2.
4. The command's own `when` (`[[contributes.commands]] when`) is false: `getAction` still returns the action; the menu item must still be filtered like other plugin menu items (`pluginMenuItems` only checks the menu decl's `when`). Keep the same behaviour as `pluginMenuItems`; note it in the PR's review focus.
5. Custom editors render `PluginView` too (`customEditor.tsx:271`) without the prop: no menu there. Covered by the prop being optional; check in `/verify`.

---

### Task 1: Accept and validate `view.item.context` declarations

**Files:**
- Modify: `crates/ide-host/src/plugins/ui_points.rs` (module doc comment l.7–10, `MENU_IDS` l.66, `MenuDecl` l.99–105, `menu()` l.107–117)
- Test: `crates/ide-host/tests/plugins.rs` (new test next to `sample_plugin_tool_window_with_a_tree`)

**Interfaces:**
- Produces: `MENU_IDS: [&str; 5]` with `"view.item.context"`; `MenuDecl.view: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`); UI decl key `view.item.context/<view>/<command>` for that menu (unchanged `<menu>/<command>` for others); the decl JSON carries `view`.
- Errors (exact text):
  - `contributes.menus: view.item.context needs \`view\``
  - `contributes.menus: view {v:?} is not declared under [[contributes.views]]`
  - `contributes.menus: \`view\` is only for view.item.context`

- [ ] **Step 1: Write the failing test** (uses the existing `hello_variant`; `hello` declares view `greetings` and command `word-count`):

```rust
#[test]
fn view_item_menus_name_a_declared_view() {
    let menu = |extra: &str| move |m: &str| format!("{m}\n[[contributes.menus]]\ncommand = \"word-count\"\n{extra}\n");
    let f = fixture(Limits::default());
    let tmp = tempfile::tempdir().unwrap();

    let ok = hello_variant(tmp.path(), menu("menu = \"view.item.context\"\nview = \"greetings\"\nwhen = \"viewItemKind == English\""));
    assert_eq!(f.install(&ok).status, PluginStatus::Inactive);
    let item = f.host.ui_items("menus").into_iter().find(|i| i.decl["menu"] == "view.item.context").unwrap();
    assert_eq!((item.key.as_str(), item.decl["view"].as_str()), ("view.item.context/greetings/word-count", Some("greetings")));
    f.host.uninstall("hello").unwrap();

    for (extra, message) in [
        ("menu = \"view.item.context\"", "needs `view`"),
        ("menu = \"view.item.context\"\nview = \"nope\"", "view \"nope\" is not declared"),
        ("menu = \"projectView.context\"\nview = \"greetings\"", "`view` is only for view.item.context"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let err = f.host.install_from_dir(&hello_variant(dir.path(), menu(extra)), &Consent::all()).unwrap_err();
        assert!(err.message.contains(message), "{extra}: {}", err.message);
    }
}
```

- [ ] **Step 2: Run, expect FAIL** — `cargo nextest run -p ide-host --test plugins view_item_menus` → `unknown menu "view.item.context"`.
- [ ] **Step 3: Implement** per the interfaces. Add `fn declared_view(pkg: &Package, id: &str) -> bool` next to `commands()` (same `pkg.manifest.declarations(VIEWS)` lookup on `"id"`).
- [ ] **Step 4: Run, expect PASS** — same command, then `cargo nextest run -p ide-host --test plugins`.
- [ ] **Step 5: Commit** — `feat(plugins): view.item.context menus for plugin view items`

### Task 2: Build the item menu from declarations

**Files:**
- Modify: `src/features/plugins/uiPoints.tsx` (`MenuDecl` l.65–69, new function after `pluginMenuItems` l.81–91)
- Test: `src/features/plugins/__tests__/views.test.tsx` (`describe("plugin menus")`)

**Interfaces:**
- Consumes: `contextItems` (existing map), `getAction`, `pluginActionId`, `evaluateWhen(expr, ctx)`, `getContext()` from `@/platform/context`.
- Produces:
  - `MenuDecl.menu` union gains `"view.item.context"`; `MenuDecl.view?: string | null`.
  - `export function pluginViewItemMenuItems(pluginId: string, view: string, key: string): MenuItem[]` — declarations of `pluginId` with `menu === "view.item.context"` and `view === view` whose `when` holds over `{ ...getContext(), viewItem: key, viewItemKind: key.split(":")[0] }` and whose action exists → `{ kind: "action", action, arg: key }`.
  - `pluginMenuItems` ignores `view.item.context` declarations (no change needed: `CONTEXT_MENUS` has no entry for it; the test pins it).

- [ ] **Step 1: Write the failing test**

```ts
it("projects view item menu entries with the item key as argument", () => {
  const off = registerAction({ id: "plugin.k.logs", title: "Logs", run: () => {} });
  syncUiContributions(
    [{ pluginId: "k", point: "menus", key: "view.item.context/res/logs",
       decl: { menu: "view.item.context", view: "res", command: "logs", when: "viewItemKind == pod" } }],
    [{ id: "k", name: "K", status: "active", enabled: true } as unknown as Parameters<typeof syncUiContributions>[1][number]],
  );
  expect(pluginViewItemMenuItems("k", "res", "pod:default:nginx")).toEqual([
    { kind: "action", action: "plugin.k.logs", arg: "pod:default:nginx" },
  ]);
  expect(pluginViewItemMenuItems("k", "res", "svc:default/web")).toEqual([]);
  expect(pluginViewItemMenuItems("k", "other", "pod:x")).toEqual([]);
  expect(pluginViewItemMenuItems("other", "res", "pod:x")).toEqual([]);
  expect(pluginMenuItems("projectView", ["/p/a"])).toEqual([]);
  off();
});
```

- [ ] **Step 2: Run, expect FAIL** — `pnpm vitest run src/features/plugins/__tests__/views.test.tsx -t "view item menu"` → `pluginViewItemMenuItems` is not exported.
- [ ] **Step 3: Implement** per the interfaces.
- [ ] **Step 4: Run, expect PASS** — same command.
- [ ] **Step 5: Commit** — `feat(plugins): build the right-click menu of plugin view items`

### Task 3: Show the menu on right-click in `PluginView`

**Files:**
- Modify: `src/features/plugins/views/PluginView.tsx` (`PluginViewProps`, `Body`/`BodyProps`, `TreeBody` l.208–229, the `list` case l.152–171)
- Modify: `src/features/plugins/uiPoints.tsx:304` (pass `itemMenu`)
- Test: `src/features/plugins/__tests__/views.test.tsx` (`describe("PluginView")`)

**Interfaces:**
- Consumes: `pluginViewItemMenuItems` (Task 2); `Tree`'s `onContextMenu?: (node: T | null, selection: Set<string>, e: MouseEvent) => void` (`src/ui/Tree.tsx:109`); `showContextMenu(e, items)` from `@/ui`.
- Produces: `PluginViewProps.itemMenu?: (key: string) => MenuItem[]`, forwarded through `Body` to `TreeBody` and the list. On right-click of an item: `items = itemMenu(key)`; open `showContextMenu(e, items)` only when `items.length > 0`; `node === null` → nothing. The list's `<li>` gets `onContextMenu` the same way. `uiPoints.tsx:304` passes `itemMenu={(key) => pluginViewItemMenuItems(pluginId, decl.id, key)}`.

- [ ] **Step 1: Write the failing test** (reuse the tree `View` and memory backend setup of `"renders a tree and sends expand / activate"`):

```ts
it("opens the item menu on right-click", async () => {
  const itemMenu = vi.fn((key: string) => (key === "ns" ? [{ label: "Describe", run: () => {} }] : []));
  render(<PluginView pluginId="jobs" view="jobs" itemMenu={itemMenu} />);
  await userEvent.pointer({ keys: "[MouseRight]", target: await screen.findByText("default") });
  expect(itemMenu).toHaveBeenCalledWith("ns");
  expect(await screen.findByText("Describe")).toBeTruthy();
});
```

- [ ] **Step 2: Run, expect FAIL** — `pnpm vitest run src/features/plugins/__tests__/views.test.tsx -t "item menu on right-click"` → `itemMenu` never called.
- [ ] **Step 3: Implement** per the interfaces.
- [ ] **Step 4: Run, expect PASS** — same command, then `pnpm vitest run src/features/plugins`.
- [ ] **Step 5: Commit** — `feat(plugins): right-click menu on plugin tree and list items`

### Task 4: Document, verify in the app, open the PR

**Files:**
- Modify: `plugins/README.md` (`menus` row of the *Contribution points* table l.139: add `view.item.context` + `view`; one paragraph: argument = item key, `viewItem` / `viewItemKind`).
- Modify: `crates/ide-plugin/tests/fixtures/jobs/plugin.toml`: declare `[[contributes.menus]] menu = "view.item.context" view = "jobs" command = "follow"` so the app has a live example (the `follow` command exists). Rebuild not needed: manifest only; run `plugins/build.sh --check` to confirm.

- [ ] **Step 1:** Edit README and the fixture; `pnpm format:check`; `plugins/build.sh --check`.
- [ ] **Step 2:** Run the checks in the index's step 6 (Rust + frontend).
- [ ] **Step 3: `/verify`.** Install `aritti.kube-explorer` built against this branch (or the `jobs` fixture as a dev plugin): screenshot right-click on an item before (no menu, on `upstream/main`) and after (menu with the command). Check Shift+F10 on a focused item; check a custom editor shows no menu.
- [ ] **Step 4: Commit** — `docs(plugins): document view.item.context menus`
- [ ] **Step 5: PR** — title `feat(plugins): right-click menu on plugin view items`, `Closes #<issue>`, before/after screenshots, review focus: the compatibility answer from the issue, `viewItemKind` convention.
