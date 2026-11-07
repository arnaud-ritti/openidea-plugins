# One route list for Spring, Symfony, Laravel and OpenAPI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Routes show as a flat list (per-method icon, path, `[GET] Controller` in grey) with a controller filter, a flat/tree toggle and row actions, in the Spring, Symfony, Laravel and OpenAPI plugins, from one shared crate.

**Architecture:** `crates/ui-kit` gets a `routes` module (data model, list/tree rendering, filter state, curl and `.http` text, two row-action helpers). Each plugin maps its own index into `Route` values and wires its window and `viewItem.context` menu to the module. Symfony and Laravel windows gain sections `Routes` and `Index`.

**Tech Stack:** Rust, `openidea-plugin` SDK v0.1.38 (API 1.2), `MockHost` tests, `openidea-plugin build`/`check`.

**Spec:** `docs/superpowers/specs/2026-11-06-tool-window-consolidation-design.md` (goal 5 and "My calls")

**Order:** needs `crates/ui-kit` and its `sections` module from Task 1 of `2026-11-06-spring-windows.md`. Task 2 here runs before the Spring Endpoints merge (Task 4 of that plan reuses it).

## Global Constraints

- Row text: label = path; grey detail = `[GET] <handler class>` followed by the route name when there is one (`[GET] UserController · app_user_show`). Handler texts: Spring `FuelStationsController`, Symfony `UserController::show`, Laravel `FamilyController@show`, OpenAPI first tag or `(no tag)`.
- Filter key is `Handler.class`. Filter button = `pick_many` (at most 5000 items, keys at most 256 bytes); the active filter shows in the status line (`18 endpoints · controller: AuthController`); a Clear button appears only while a filter is active.
- Cap: 5000 rows, with the status line saying `showing 5000 of N`.
- Row actions: open handler (activate or double-click), Copy as curl, Create HTTP Request File. Base URL by `input`, prefilled `http://localhost:8080` (Spring), `http://localhost:8000` (Symfony, Laravel); the `.http` file goes under `http/` at the project root (next to the spec for OpenAPI) as `<verb>-<path>.http`; an existing file is opened, never overwritten. `process:tee` and `clipboard` stay in `optional`; denied tee falls back to the clipboard.
- Filter, Clear and Flat/Tree buttons show only in the routes section. No table body (no row events), no drop-downs (plain buttons only), no split pane.
- Method icons: SVG files each plugin ships in its own `icons/` and registers as `[[contributes.fileIcons]]` entries the way `plugins/kube-explorer/plugin.toml` does (lines about 227-304); items name them `ext:fileIcon/<plugin id>/route-get`, etc. Colours: GET green, POST orange, PUT blue, DELETE red, PATCH purple, other grey.
- Never edit `version` in `plugin.toml`. Conventional commits, one per task.

## Review Focus

- A project with no routes shows an explicit empty message, not an empty list or an error.
- A filter naming a controller that disappears after a re-index falls back to "no filter" instead of showing nothing.
- A handler without a class (Laravel closure, Symfony invokable) still lists and filters under `Closure` / the class name.
- Path with a quote, space or `{param}` is shell-safe in `curl` and unchanged in `.http`.
- The base URL prompt dismissed: nothing written, nothing copied.
- 5000+ routes: the cap and the status line.

---

### Task 1: The `routes` module in `ui-kit`

**Files:**
- Create: `crates/ui-kit/src/routes.rs`
- Modify: `crates/ui-kit/src/lib.rs` (export)
- Test: unit tests in `routes.rs`

**Interfaces:**
- Produces: `pub struct Handler { pub class: String, pub member: Option<String> }`; `pub struct Route { pub method: String, pub path: String, pub handler: Handler, pub name: Option<String>, pub location: Option<Location> }`.
- Produces: `pub struct RouteList` with `RouteList::new(storage_key: &str, plugin_id: &str) -> RouteList` (loads flat/tree mode and filter from storage), `fn set_routes(&mut self, routes: Vec<Route>)` (drops filter entries whose class no longer exists), `fn render(&self, view: View) -> View` (flat `Body::List` with `ListItem` key `route:<index>` / grouped `Body::Tree`; adds the status line and the Filter…, Clear, Flat/Tree buttons), `fn handle(&mut self, event: &ViewEvent) -> RouteEvent`, `fn route(&self, key: &str) -> Option<&Route>`.
- Produces: `pub enum RouteEvent { Ignored, Changed, Open(Route) }` (`Changed` after a filter/toggle so the caller refreshes).
- Produces: `pub fn curl(method: &str, base: &str, path: &str) -> String` (single-quoted, quotes escaped), `pub fn http_request(method: &str, base: &str, path: &str) -> String` (`### GET /users` then `GET <base><path>`), `pub fn copy_as_curl(route: &Route, default_base: &str) -> Result<(), Error>`, `pub fn create_http_file(route: &Route, default_base: &str) -> Result<(), Error>`.

- [ ] **Step 1: Write failing tests**: `flat_rows_show_path_label_and_grey_method_and_controller`, `rows_carry_one_icon_per_method`, `filter_limits_rows_and_shows_in_the_status_line`, `dismissed_filter_prompt_keeps_the_filter`, `vanished_controller_is_dropped_from_the_filter`, `tree_mode_groups_by_controller`, `more_than_5000_routes_are_capped_with_a_note`, `curl_escapes_quotes_in_paths`, `http_request_keeps_path_parameters`, `base_prompt_dismissed_writes_and_copies_nothing`, `denied_tee_falls_back_to_the_clipboard`, `existing_http_file_is_opened_not_overwritten`.
- [ ] **Step 2: Run** `cargo test routes` in `crates/ui-kit`; expect FAIL.
- [ ] **Step 3: Implement** the module; mode and filter stored under `<storage_key>.mode` and `<storage_key>.filter`.
- [ ] **Step 4: Run** `cargo test` in `crates/ui-kit`; expect pass.
- [ ] **Step 5: Commit** `feat(ui-kit): add the shared route list`.

### Task 2: Spring Endpoints uses it

**Files:**
- Modify: `plugins/spring-web/src/lib.rs` (Endpoints view render/handle, `Copy as curl` and `Create HTTP Request File` commands now in `ui-kit`), `plugins/spring-web/Cargo.toml`, `plugins/spring-web/plugin.toml` (fileIcons, `viewItem.context` entries, `clipboard`/`process:tee` optional), `plugins/spring-web/src/tests.rs` or the in-file tests
- Create: `plugins/spring-web/icons/route-get.svg`, `route-post.svg`, `route-put.svg`, `route-delete.svg`, `route-patch.svg`, `route-other.svg`

**Interfaces:**
- Consumes: Task 1.
- Produces: `fn routes(&self) -> Vec<Route>` mapping the existing endpoint index (method, path, controller class, member, source location).

- [ ] **Step 1: Write failing tests**: `endpoints_view_lists_routes_flat_with_controller_detail` (on the existing sample controllers: 18 rows, `/auth/login` detail `[POST] AuthController`), `controller_filter_narrows_the_endpoints`, `curl_row_action_still_copies_the_old_text` (same output as before the move).
- [ ] **Step 2: Run** `cargo test endpoints` in `plugins/spring-web`; expect FAIL.
- [ ] **Step 3: Implement**: replace the tree rendering with `RouteList`; keep the activate-to-open-handler behaviour via `RouteEvent::Open`; delete the local curl and `.http` generators in favour of `ui_kit::routes`.
- [ ] **Step 4: Run** `cargo test`, `openidea-plugin build`, `openidea-plugin check`; expect pass.
- [ ] **Step 5: Commit** `feat(spring-web): show endpoints as a flat route list with a controller filter`.

### Task 3: Symfony routes section

**Files:**
- Modify: `plugins/symfony/src/lib.rs` (the `symfony` view render/handle near 1339-1366), `plugins/symfony/Cargo.toml`, `plugins/symfony/plugin.toml`, tests
- Create: the six `icons/route-*.svg`

**Interfaces:**
- Consumes: Task 1, `ui_kit::sections::Switcher` (Spring plan Task 1).
- Produces: sections `routes` (flat list) and `index` (today's tree, unchanged); `fn routes(&self, index: &Index) -> Vec<Route>` from the route index (`Class::method`, route name in the detail).

- [ ] **Step 1: Write failing tests**: `symfony_window_has_routes_and_index_sections`, `routes_section_lists_controller_method_and_route_name`, `filter_and_toggle_buttons_show_only_in_the_routes_section`, `non_symfony_project_still_shows_the_message`.
- [ ] **Step 2: Run** `cargo test routes_section` in `plugins/symfony`; expect FAIL.
- [ ] **Step 3: Implement** the sections and wire the row actions (`viewItem.context` entries with `view = "symfony"`).
- [ ] **Step 4: Run** `cargo test`, `build`, `check`; expect pass.
- [ ] **Step 5: Commit** `feat(symfony): add a flat Routes section to the Symfony window`.

### Task 4: Laravel routes section

**Files:** the same shape as Task 3 in `plugins/laravel` (`src/lib.rs` near 1273-1299, `Cargo.toml`, `plugin.toml`, tests, six icons).

**Interfaces:**
- Consumes: Task 1, `Switcher`.
- Produces: sections `routes` and `index`; `fn routes(&self, index: &Index) -> Vec<Route>` (`Controller@method`, route name, a closure route listed under handler class `Closure`).

- [ ] **Step 1: Write failing tests**: `laravel_window_has_routes_and_index_sections`, `closure_routes_list_under_closure`, `routes_show_names_and_controller_methods`, `filter_by_controller`.
- [ ] **Step 2-4:** as Task 3 for Laravel (`cargo test routes_section`, implement, full `cargo test`/`build`/`check`).
- [ ] **Step 5: Commit** `feat(laravel): add a flat Routes section to the Laravel window`.

### Task 5: OpenAPI endpoints (last; drop if it slows the rest)

**Files:** `plugins/openapi/src/lib.rs` (the `endpoints` view), `Cargo.toml`, `plugin.toml`, tests, six icons.

**Interfaces:**
- Consumes: Task 1.
- Produces: `fn routes(&self, spec: &Spec) -> Vec<Route>` (path and method from the spec; `Handler.class` = first tag or `(no tag)`; `member` = `operationId`; `location` = the operation in the spec file).

- [ ] **Step 1: Write failing tests**: `endpoints_list_operations_flat_with_their_tag`, `untagged_operations_group_under_no_tag`, `open_goes_to_the_operation_in_the_spec`.
- [ ] **Step 2-4:** run, implement, full `cargo test`/`build`/`check`.
- [ ] **Step 5: Commit** `feat(openapi): show endpoints as the shared route list`.

### Task 6: Method icons, docs and real IDE check

**Files:** the READMEs, `ICONS.md`, `LIMITATIONS.md`, `PARITY.md`, the plugins' `change-notes`.

- [ ] **Step 1: Draw** the six 16x16 method SVGs once (GET green, POST orange, PUT blue, DELETE red, PATCH purple, other grey; rounded square with the verb's initial or arrow glyph, under 1 KiB each) and copy them into each plugin's `icons/`.
- [ ] **Step 2: Docs**: describe the flat list, filter, toggle and row actions; the limits (grey text follows the path; no split request panel; no gutter icon; those go to the upstream requests).
- [ ] **Step 3: Verify in OpenIDEA 0.1.38**: `ma-station-essence` shows 18 flat rows with method icons; the controller filter narrows to one controller and the status line says so; Clear restores; Flat/Tree toggles; Copy as curl and Create HTTP Request File work (grant `process:tee` first). In `baby-bob`, the Laravel Routes section lists the project's routes and the filter works; in a Symfony project the same.
- [ ] **Step 4: Commit** `docs: describe the shared route list`.
