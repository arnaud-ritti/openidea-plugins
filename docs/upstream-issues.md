# Upstream issues filed on l0ng-ai/openidea (2026-11-07)

Plugin API gaps found while building the plugins in this repository, each filed as its own issue on https://github.com/l0ng-ai/openidea. They cover what the audit ([audit-gaps-2026-11-06/audit-v2.md](audit-gaps-2026-11-06/audit-v2.md)) classes B: blocked by a missing host API. Existing related issues are named inside each body (#464, #466, #1570, #1612, #1618, #1622, #1768, #1769, #1822 to #1826, #1936).

| # | Issue | Title |
|---|---|---|
| 01 | [#1943](https://github.com/l0ng-ai/openidea/issues/1943) | Plugins: let a plugin provide references and rename (Find Usages, Rename Symbol) |
| 02 | [#1944](https://github.com/l0ng-ai/openidea/issues/1944) | Plugins: let a plugin query the language service and symbol index of built-in languages |
| 03 | [#1945](https://github.com/l0ng-ai/openidea/issues/1945) | Plugins: let a code action ask the user, or run a command, before it applies edits |
| 04 | [#1946](https://github.com/l0ng-ai/openidea/issues/1946) | Plugins: let host::input take a multi-line value |
| 05 | [#1947](https://github.com/l0ng-ai/openidea/issues/1947) | Plugins: create private temporary files and directories (owner-only) for a plugin |
| 06 | [#1948](https://github.com/l0ng-ai/openidea/issues/1948) | Plugins: an event for files created, changed and deleted on disk |
| 07 | [#1949](https://github.com/l0ng-ai/openidea/issues/1949) | Plugins: group the views of several plugins in one tool window and show a view only when it applies |
| 08 | [#1950](https://github.com/l0ng-ai/openidea/issues/1950) | Plugins: a view with a list and a detail pane |
| 09 | [#1951](https://github.com/l0ng-ai/openidea/issues/1951) | Plugins: gutter line markers with a custom icon and a click command |
| 10 | [#1952](https://github.com/l0ng-ai/openidea/issues/1952) | Plugins: a diagnostics provider that refreshes while the user types |
| 11 | [#1953](https://github.com/l0ng-ai/openidea/issues/1953) | Plugins: view toolbar controls, read-only form fields and table row events |
| 12 | [#1954](https://github.com/l0ng-ai/openidea/issues/1954) | Plugins: file templates with a default file name and package variables |
| 13 | [#1955](https://github.com/l0ng-ai/openidea/issues/1955) | Plugins: notifications with action buttons |
| 14 | [#1956](https://github.com/l0ng-ai/openidea/issues/1956) | Plugins: let a plugin language embed another language (language injection) |
| 15 | [#1957](https://github.com/l0ng-ai/openidea/issues/1957) | Plugins: report a missing file as NotFound and add exists and stat |
| 16 | [#1958](https://github.com/l0ng-ai/openidea/issues/1958) | Plugins: filter events by path and give editor providers the project |
| 17 | [#1959](https://github.com/l0ng-ai/openidea/issues/1959) | Plugins: let a plugin open a project and ask the user for a directory |
| 18 | [#1960](https://github.com/l0ng-ai/openidea/issues/1960) | Plugins: contribute routes to the built-in HTTP client (completion, Open in HTTP Client, Search Everywhere) |
| 19 | [#1961](https://github.com/l0ng-ai/openidea/issues/1961) | Plugins: a per-plugin cache directory or more than 1 MiB of storage |
| 20 | [#1962](https://github.com/l0ng-ai/openidea/issues/1962) | SDK: register_plugin! arms that combine syntax with providers, views and indexing |

The full text of each issue follows, as filed.

## [#1943](https://github.com/l0ng-ai/openidea/issues/1943) Plugins: let a plugin provide references and rename (Find Usages, Rename Symbol)

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

Plugin providers are `completion`, `inline-completion`, `code-actions`, `definition` and `line-markers`. There is no `references` or `rename` provider, so a plugin can navigate to a symbol it owns but cannot list its usages or rename it.

This blocks the core refactoring loop of every framework plugin: renaming a Symfony route, a Spring bean name, a Laravel route name, a translation key, a Twig block or a Flyway placeholder across the files that use it. The document indexer (1.2) answers Go to Declaration and gutter markers from the keys a plugin publishes, but it has no usages list and no rename.

#### Proposed solution

Two new provider kinds, declared in `[[contributes.providers]]` like the others:

```wit
references: func(doc: document, at: position, include-declaration: bool) -> list<location>;
rename: func(doc: document, at: position, new-name: string) -> result<list<file-edit>, error>;
prepare-rename: func(doc: document, at: position) -> option<range>;
```

- The host merges the result with the language server's own references, as it does for `definition` today.
- `rename` returns text edits across files (the same `file-edit` as code actions) so the host applies them as one undoable workspace edit.
- A plugin whose document indexer already holds `definitions` and `references` for a key can answer from that index, so an alternative is to let the indexer's graph serve Find Usages and Rename directly.

#### Acceptance criteria

- [ ] Shift+F12 / Find Usages on a plugin-owned symbol lists the plugin's references next to the language server's
- [ ] Rename on a plugin-owned symbol shows a preview and applies all edits as one undoable change
- [ ] A symbol the plugin doesn't own falls through to the language server unchanged

#### Alternatives and workarounds

Today a plugin can only offer a code action per usage, or tell the user to search for the old name. The Symfony, Laravel, Spring and Flyway plugins all list this as their largest gap.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1944](https://github.com/l0ng-ai/openidea/issues/1944) Plugins: let a plugin query the language service and symbol index of built-in languages

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

A provider gets `document { path, language, version, text }` and nothing else. A plugin cannot ask what the type of an expression is, which members a class has, where a symbol is defined, or which classes exist in the project. Plugin features that depend on a resolved type are therefore guesses based on text patterns.

Blocked in our plugins: container `get('id')` return types and form types (Symfony), Twig variable members (`{{ post.| }}`), Eloquent relation and column completion beyond the model file, Spring type-based bean injection checks, JPQL checks against entity fields in libraries, and every inspection that needs a resolved PHP or Java type.

#### Proposed solution

Read-only host functions that forward to the language service already running for the file:

```wit
interface language-service {
    definition: func(path: string, at: position) -> result<list<location>, error>;
    hover-type: func(path: string, at: position) -> result<option<string>, error>;
    members-of: func(fqcn: string) -> result<list<member>, error>;
    workspace-symbols: func(query: string, limit: u32) -> result<list<symbol-info>, error>;
}
```

Gated by a `language-service` capability; calls count against the plugin's budget and return `unavailable` when no server runs for the language. A smaller first step is `members-of` and `workspace-symbols` over the host's own index (`crates/ide-index`).

#### Acceptance criteria

- [ ] A plugin can resolve the type name under the caret in a PHP and a Java file
- [ ] A plugin can list the public members of a class by fully qualified name
- [ ] Calls fail with a clear error when no language server is running, instead of hanging

#### Alternatives and workarounds

Text patterns: a call is recognised by its method name (`->generate(`), not by the class it is called on, so unrelated objects get framework completions.

#### Additional context

See also #464 (versioned language-service capabilities for IDE features), which is the internal counterpart.

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1945](https://github.com/l0ng-ai/openidea/issues/1945) Plugins: let a code action ask the user, or run a command, before it applies edits

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`pick`, `input` and `confirm` work only inside a command or event of a window (#1768) and return `invalid` in editor providers, which includes `resolve-code-action`. A quick fix therefore cannot ask which file to add a key to, which service to inject, or confirm a destructive rewrite.

Concretely: the Symfony "add translation key" fix has to offer one action per candidate YAML file because it cannot show a chooser; "Create service" or "Add inverse attribute" cannot ask for a name.

#### Proposed solution

Let a code action declare a follow-up instead of edits:

```wit
record code-action {
    // …existing fields…
    /// Run this plugin command (with `argument`) when the action is chosen,
    /// in the window of the editor; the command may then prompt.
    command: option<command-ref>,
}
```

With `edits` empty and `command` set, choosing the action runs the command with a normal command invocation (so prompts work, the 5 s budget applies, and it can edit the focused editor).

#### Acceptance criteria

- [ ] A code action with a `command` runs it in the editor's window when chosen
- [ ] `pick`, `input` and `confirm` work inside that command
- [ ] A code action with edits keeps working as today

#### Alternatives and workarounds

One code action per possible answer, which does not scale and clutters the menu.

#### Additional context

Related: #1768 (prompts in commands, done) and #1769 (file operations in code actions).

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1946](https://github.com/l0ng-ai/openidea/issues/1946) Plugins: let host::input take a multi-line value

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`host::input(prompt, value)` is one line. Commands that ask for text with line breaks cannot use it: a merge request description, a commit message, a snippet body, a note. The GitLab plugin asks for a one-line description and then offers to open GitLab for longer text.

#### Proposed solution

Add a variant or a flag so the prompt shows a text area:

```wit
input-multiline: func(prompt: string, value: option<string>) -> result<option<string>, error>;
```

Same rules as `input` (command or event of a window, `none` on dismiss, no capability), with Cmd/Ctrl+Enter to confirm.

#### Acceptance criteria

- [ ] A plugin command can ask for a multi-line text and receive it with its line breaks
- [ ] Dismissing returns `none` and changes nothing

#### Alternatives and workarounds

A `Form` view with a `Multiline` field works but needs a tool window just to ask one question, which is what the consolidation of our windows is trying to remove.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1947](https://github.com/l0ng-ai/openidea/issues/1947) Plugins: create private temporary files and directories (owner-only) for a plugin

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

A plugin that wants to open generated content in a real editor has no safe place to write it. Today it can only run an approved program: `tee /tmp/…` creates the file with the process umask, normally mode `0644`, readable by every local user, and nothing deletes it. This was the blocker in the registry review of `aritti.kube-explorer 0.1.0` (l0ng-ai/openidea-plugins#18): the YAML of a Kubernetes Secret was written to `/tmp` world-readable.

The plugin API gives no way to create the file privately: no umask, no chmod, no mkdir, no per-plugin directory.

#### Proposed solution

A host function that creates the file privately and returns its path:

```wit
/// Creates an empty file or directory readable only by the current user in a
/// per-plugin temporary directory; the host deletes it when the plugin stops
/// or after `ttl` seconds. Requires `temp-files`.
create-temp-file: func(name-hint: string, content: string, ttl: option<u32>) -> result<string, error>;
create-temp-dir: func(name-hint: string) -> result<string, error>;
```

The file is created `0600` (directory `0700`) from the start, never changed afterwards, so there is no exposure window. The returned path can then be opened with `open_location` and written back by the IDE's save.

#### Acceptance criteria

- [ ] The file is created with mode 0600 and the directory 0700 on macOS and Linux (owner-only ACL on Windows)
- [ ] Files are removed when the plugin stops or when the TTL elapses
- [ ] The path can be passed to `open_location` and to a process argument

#### Alternatives and workarounds

Keep the data out of files: the kube-explorer plugin now opens YAML in a panel instead (setting `yamlInEditor` turns the temp file back on), and loses the editor's schema completion and validation.

#### Additional context

Related: #1824 (write a file inside the project) covers files the user keeps; this asks for files the plugin does not want others to read.

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1948](https://github.com/l0ng-ai/openidea/issues/1948) Plugins: an event for files created, changed and deleted on disk

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

The events are `file-opened`, `file-saved`, `editor-focused`, `vcs-changed` (repository root only), `action-performed` and `scheduled`. A plugin that keeps an index cannot learn that a file changed unless the IDE opened or saved it: `git checkout`, `composer install`, `cache:warmup`, a code generator or another tool leaves its index stale until the plugin rebuilds everything.

Our Symfony, Laravel and Spring plugins rebuild their whole index on activation and on saves under a few directories, reading for at most 3 s and marking the index partial.

#### Proposed solution

A batched event with the paths, debounced by the host:

```wit
record files-changed { created: list<string>, modified: list<string>, removed: list<string> }
// variant event { …, files-changed(files-changed) }
```

Subscribed with `subscribe("files-changed")`. The host already has this signal for its own indexer (`fsChanged`); paths ignored by `.gitignore` could be excluded unless the plugin asks for them.

#### Acceptance criteria

- [ ] Creating, editing and deleting a file with an external tool delivers one batched event within a second
- [ ] A burst (git checkout) arrives as a single event with all paths
- [ ] The event needs the `events` capability and only covers the open project

#### Alternatives and workarounds

Re-indexing on activation and on the saves of watched directories.

#### Additional context

See also #1612 (`workspaceContains:` when clauses), which tests existence but doesn't notify.

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1949](https://github.com/l0ng-ai/openidea/issues/1949) Plugins: group the views of several plugins in one tool window and show a view only when it applies

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

Every plugin view is its own tool window, `plugin.<plugin id>.<view id>`, drawn by its plugin; a view can't live in another plugin's window, plugins can't call each other, and a view has only `id`, `title`, `icon`, `location` and `web`: no group and no condition. So the stripe grows with each plugin and a view is visible even in a project it does not apply to (a Symfony window in a Laravel project shows "This project is not a Symfony application").

Our 36 plugins add 37 tool windows; the GitLab plugin alone had 10 and Spring 15, all with the same icon. We consolidated them by merging plugins, which removes the user's ability to install them separately.

#### Proposed solution

Two optional manifest fields on `[[contributes.views]]`:

```toml
[[contributes.views]]
id = "endpoints"
title = "Endpoints"
group = "spring"          # views of any plugin with the same group share one tool window, as tabs
when = "workspaceContains:pom.xml || workspaceContains:build.gradle"
```

- `group` places the views in one tool window (tab per view, ordered by `order`); the group's icon comes from the first plugin that declares it.
- `when` uses the same context-key language as menus (`workspaceContains:`, `plugin.<id>.<key>` set with `set_context`), and a view whose condition is false does not appear in the stripe.

#### Acceptance criteria

- [ ] Two plugins declaring the same `group` show their views as tabs of one tool window
- [ ] A view with a false `when` has no stripe icon and no tool window
- [ ] The `when` re-evaluates when `set_context` or the files change

#### Alternatives and workarounds

Merge plugins that should share a window (what we did for GitLab and Spring), or have each plugin show an explanatory empty state.

#### Additional context

Related: #1618 (menu on view items) and #1622 (`set_context`), both done, which this builds on.

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1950](https://github.com/l0ng-ai/openidea/issues/1950) Plugins: a view with a list and a detail pane

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

A view has one body: `message`, `tree`, `list`, `form`, `table` or `html`. A master/detail layout, like the JetBrains Endpoints window (a list on the left and an HTTP client with the request and its response on the right), cannot be built: the plugin must replace the whole body when an item is selected, or open another window.

#### Proposed solution

A `split` body with two children and a direction, and selection events scoped to the first child that re-render the second:

```wit
variant body { …, split(split) }
record split { direction: orientation, first: body, second: body, ratio: option<f32> }
```

The `second` body is requested again after each `select` event of the first, with the selected key.

#### Acceptance criteria

- [ ] A plugin can render a list with a detail pane that follows the selection
- [ ] The divider can be dragged and its position is remembered
- [ ] A body that is not a split behaves as today

#### Alternatives and workarounds

Selecting a row opens a second tool window, or the detail replaces the list with a Back button (what the Kube Explorer and the Spring plugins do).

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1951](https://github.com/l0ng-ai/openidea/issues/1951) Plugins: gutter line markers with a custom icon and a click command

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`line-marker` has `icon` from a fixed list (`navigate`, `implemented`, `implementing`, `run`), a single `target: option<location>` and a tooltip. A marker can't carry the plugin's own icon, and clicking it can only navigate: it can't run a command or pass the line to one.

Effects: our Spring marker "URL: GET /path" has no target and no icon of its own; the GitLab plugin can't start a review thread from the gutter; Symfony markers lose the "overriding" icon when published through the document indexer; Kube Explorer can't show a resource marker with a Kubernetes icon.

#### Proposed solution

```wit
record line-marker {
    // …existing fields…
    icon: option<string>,        // allow "ext:fileIcon/<plugin id>/<id>" like tree items do
    command: option<command-ref>,// run with the line as argument when clicked
    targets: list<location>,     // several targets open the chooser the indexer markers already use
}
```

#### Acceptance criteria

- [ ] A plugin marker can use an SVG icon shipped in its package, light and dark
- [ ] Clicking a marker with a `command` runs it with the line and path
- [ ] A marker with several targets opens the chooser

#### Alternatives and workarounds

A line marker with a tooltip only, and a separate code action on the same line.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1952](https://github.com/l0ng-ai/openidea/issues/1952) Plugins: a diagnostics provider that refreshes while the user types

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`publish_diagnostics` can only be called from commands and event handlers, and the events fire on open, save and focus. Problems of a plugin are therefore stale while the user edits and only refresh on save. There is no `document-changed` event and no diagnostics provider on the editing path.

#### Proposed solution

A provider kind `diagnostics`, called per document version after a debounce (about 300 ms), under the editor-provider budget:

```wit
diagnostics: func(doc: document) -> list<diagnostic>;
```

Or a debounced `document-changed` event with the path and version, so a plugin can publish from its own scan.

#### Acceptance criteria

- [ ] Typing in a file refreshes the plugin's problems within a second without saving
- [ ] Results for an older version are dropped
- [ ] Declared only for the languages in the provider's `languages`

#### Alternatives and workarounds

Publishing on open and save, which is what every plugin of ours does.

#### Additional context

Related: #466 (diagnostics, code actions and document events for the WASM plugin API), which predates API 1.x.

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1953](https://github.com/l0ng-ai/openidea/issues/1953) Plugins: view toolbar controls, read-only form fields and table row events

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

View toolbars hold plain buttons only. A `Form` field is `text`, `multiline`, `checkbox` or `choice`: there is no read-only field or label, so showing data (a merge request title and author) in a form looks editable and edits are lost. A `Table` has no row selection or activation event, so a table can't be a list a user acts on.

Effects: filters are buttons that open a picker instead of a search field or drop-down; the Spring and Laravel route lists are lists rather than tables because rows can't be clicked; the GitLab view cycles quick filters with one button.

#### Proposed solution

- Toolbar items: `search` (text field reporting `field` events), `choice` (drop-down), `toggle`.
- `field-value::label(string)` and a `readonly` flag on `text` and `multiline`.
- `table` events `select(row)`, `activate(row)` and a context menu like tree items (#1618).

#### Acceptance criteria

- [ ] A view toolbar can show a search field and a drop-down that send events
- [ ] A form can show a read-only value
- [ ] A table row can be selected and activated, and have a context menu

#### Alternatives and workarounds

Buttons that open `pick` prompts for filters, Html bodies for read-only text.

#### Additional context

Related: #1570 (clickable column headers for plugin tables).

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1954](https://github.com/l0ng-ai/openidea/issues/1954) Plugins: file templates with a default file name and package variables

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`[[contributes.fileTemplates]]` takes `id`, `name`, `extension` and `path`; the user types the file name and the extension is appended. Two limits:

1. A template can't carry a default name, so dot-files such as `.dockerignore` or `.gitattributes` can't be created from the New menu (the extension would be appended to an empty name).
2. Plugin templates get no package style: `$PACKAGE$` is empty and its line is dropped, so a Java or Kotlin class template can't write its `package` statement, and a template can't prompt for extra variables (the entity a repository is for).

#### Proposed solution

```toml
[[contributes.fileTemplates]]
id = "dockerignore"
name = ".dockerignore"
file-name = ".dockerignore"        # default name, no prompt for the name
package-style = "jvm"             # fills $PACKAGE$ from the target folder
variables = [{ name = "ENTITY", prompt = "Entity class" }]
```

#### Acceptance criteria

- [ ] A template with `file-name` creates that file without asking for a name
- [ ] `package-style = "jvm"` fills `$PACKAGE$` from the folder under a source root
- [ ] Declared variables are asked in one dialog before the file is created

#### Alternatives and workarounds

Templates without a package line (the caret lands on the type); a command that asks and writes with `tee`.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1955](https://github.com/l0ng-ai/openidea/issues/1955) Plugins: notifications with action buttons

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`show-notification(level, title, body)` shows text only. A plugin can't offer "Create merge request", "Open the diff" or "Undo" on the notification, so after an action the plugin has to say in text which window to open.

#### Proposed solution

An optional list of actions, each a label and a plugin command:

```wit
show-notification-with-actions: func(level: notification-level, title: string, body: string, actions: list<tuple<string, command-ref>>);
```

Choosing an action runs the command in the notification's window.

#### Acceptance criteria

- [ ] A notification can carry up to three action buttons
- [ ] A button runs the plugin command in the right window and dismisses the notification

#### Alternatives and workarounds

Naming the window in the text, or `confirm` prompts.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1956](https://github.com/l0ng-ai/openidea/issues/1956) Plugins: let a plugin language embed another language (language injection)

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

A plugin language is a single flat grammar. Template languages embed others: Twig and Blade contain HTML, CSS, JavaScript and PHP; Qute contains HTML; YAML values contain expressions. Our Twig and Blade plugins ship their own approximate lexers and have no HTML/PHP completion, highlighting or formatting inside templates.

#### Proposed solution

A way for a language contribution to declare injection ranges, either in the syntax provider's result (`injections: list<injection { range, language }>`) or with TextMate-style begin/end patterns in the manifest. The host then runs the embedded language's own providers on the range, with offsets mapped back.

#### Acceptance criteria

- [ ] A plugin syntax result can mark a range as HTML or PHP
- [ ] Completion, hover and highlighting for the embedded language work inside the range
- [ ] Edits and folding stay in host coordinates

#### Alternatives and workarounds

Approximate lexers inside the plugin, which can't match a full HTML or PHP grammar.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1957](https://github.com/l0ng-ai/openidea/issues/1957) Plugins: report a missing file as NotFound and add exists and stat

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`Project::read_file` on a missing path returns `Error::Invalid("<path>: no such file")` in the host (`sandbox::resolve_in_root` canonicalizes first), while the SDK's `MockHost` returns `Error::NotFound`. Tests pass and the IDE behaves differently. There is also no way to test existence cheaply: a plugin reads the whole file, or lists with `find-files`, which skips hidden and ignored paths.

#### Proposed solution

- Return `not-found` for a missing file in `read-file`, as the mock does.
- Add `exists: func(path: string) -> result<bool, error>` and `stat` (kind, size, modified time) to `project`.

#### Acceptance criteria

- [ ] `read-file` of a missing path returns `not-found` in the host and in the mock
- [ ] `exists` answers true for files and directories, false for missing, without reading content
- [ ] Paths outside the project are refused as today

#### Alternatives and workarounds

Treating both `NotFound` and `Invalid` as absent (what our plugins do), at the cost of masking a real bad path.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1958](https://github.com/l0ng-ai/openidea/issues/1958) Plugins: filter events by path and give editor providers the project

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`subscribe(kind)` takes only the event kind, so a plugin that only cares about `config/*.yaml` is woken for every file opened or saved in any project and filters by path itself. Editor providers receive a `document`, not an invocation, so a provider that needs the project (to read a parent template, to resolve a quick fix) calls `host::projects()` and matches roots on every call, inside the 50 ms budget.

#### Proposed solution

- `subscribe(kind, glob)` or `activation = ["on-event:file-saved:config/**"]`.
- Pass `project: option<project>` (or its root) in the provider calls.

#### Acceptance criteria

- [ ] A plugin subscribed with a glob is not woken for other paths
- [ ] Provider calls carry the project the document belongs to

#### Alternatives and workarounds

Filtering inside the plugin and calling `host::projects()` per call.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1959](https://github.com/l0ng-ai/openidea/issues/1959) Plugins: let a plugin open a project and ask the user for a directory

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

A plugin can't open a project in a window and can't ask for a folder. The GitLab plugin can clone a repository and create a merge request worktree but then has to tell the user to File > Open it by hand, and the clone target is a setting instead of a chooser.

#### Proposed solution

```wit
pick-directory: func(title: string, initial: option<string>) -> result<option<string>, error>;
open-project: func(path: string, new-window: bool) -> result<_, error>;
```

`open-project` behind a capability with the trust prompt the IDE already uses for projects.

#### Acceptance criteria

- [ ] A command can ask for a directory and receives its absolute path or `none`
- [ ] A plugin can open a folder as a project in the current or a new window, after the usual trust check

#### Alternatives and workarounds

A `cloneDirectory` setting and a notification telling the user to open the folder.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1960](https://github.com/l0ng-ai/openidea/issues/1960) Plugins: contribute routes to the built-in HTTP client (completion, Open in HTTP Client, Search Everywhere)

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

OpenIDEA's HTTP client has a Spring Web feature: a gutter icon "Open in HTTP Client" on each mapping, a generated-requests scratch, URL completion in `.http` files and an Endpoints entry in Search Everywhere. It comes from an internal endpoint provider (`registerEndpointProvider`, `src/features/http-client/spring/routeIndex.ts`) that plugins can't reach.

Symfony, Laravel, OpenAPI and other frameworks already index their routes in plugins, so they could feed the same features, but today they can only write their own `.http` files with `process:tee` and show their own tool windows.

#### Proposed solution

Let a plugin publish endpoints to the host's provider:

```wit
record endpoint { method: string, path: string, handler: option<location>, name: option<string>, body-type: option<string> }
publish-endpoints: func(endpoints: list<endpoint>) -> result<_, error>;
```

The host then offers the gutter icon on `handler`, completion, scratch generation and the Search Everywhere entries for them.

#### Acceptance criteria

- [ ] A plugin's routes appear in Search Everywhere's Endpoints section and in `.http` URL completion
- [ ] The handler gets the "Open in HTTP Client" gutter action
- [ ] Endpoints disappear when the plugin stops

#### Alternatives and workarounds

Per-plugin windows and `.http` files written with `tee` (our Spring, Symfony, Laravel route lists).

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1961](https://github.com/l0ng-ai/openidea/issues/1961) Plugins: a per-plugin cache directory or more than 1 MiB of storage

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`host::storage` is 1 MiB per plugin, and an instance is reclaimed after 5 minutes idle, so a plugin that indexes a project (Symfony services and routes, Spring beans, Laravel models) cannot persist its index and rebuilds it from scratch on every activation, within the 5 s activation budget and the 3 s read cap we use.

#### Proposed solution

A per-plugin cache directory, cleared with the plugin's data, readable and writable by the plugin only, with a larger quota (for example 64 MiB) behind a `cache` capability; or raise the `storage` quota to that size.

#### Acceptance criteria

- [ ] A plugin can store and read back a multi-megabyte index after the instance is reclaimed
- [ ] The quota is documented and the error when it is exceeded is clear
- [ ] Uninstalling the plugin removes the cache

#### Alternatives and workarounds

Rebuilding on every activation and marking the index partial when it can't finish.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.


## [#1962](https://github.com/l0ng-ai/openidea/issues/1962) SDK: register_plugin! arms that combine syntax with providers, views and indexing

#### Before you start

- [x] I searched [existing issues](https://github.com/l0ng-ai/openidea/issues?q=is%3Aissue) and this hasn't been requested yet.

#### Problem

`register_plugin!` has arms for `providers`, `views`, `providers, views`, `syntax` and (1.2) `indexing` combinations, but none that combines `syntax` with the others. A plugin that owns a language and also needs providers, a view or an indexer has to expand the macro by hand with the internal `__plugin_exports!`, `__providers_exports!`, `__views_exports!` and `__export_*_world!` macros (we do this in the ignore, ideolog, twig, symfony and laravel plugins).

#### Proposed solution

Add arms for the missing combinations, or make the macro accept a list: `register_plugin!(Plugin, providers, views, indexing, syntax)`.

#### Acceptance criteria

- [ ] `register_plugin!(P, providers, syntax)` compiles and exports both worlds
- [ ] The list form accepts any subset in any order
- [ ] `openidea-plugin check` still accepts the component

#### Alternatives and workarounds

Hand-written registration with the `__*` macros.

#### Additional context

Found while building 36 plugins against plugin API 1.2 (OpenIDEA 0.1.38): https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/LIMITATIONS.md lists each plugin's limits, and https://github.com/arnaud-ritti/openidea-plugins/blob/2ab5f0a/docs/audit-gaps-2026-11-06/audit-v2.md ranks the missing features by cause.
