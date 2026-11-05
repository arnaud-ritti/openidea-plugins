---
name: porting-intellij-plugins-to-openidea
description: Use when searching the JetBrains (IntelliJ IDEA) Plugin Marketplace for a plugin to bring to OpenIDEA, or when porting, converting or migrating an IntelliJ plugin (plugin.xml, .jar/.zip, theme, icon pack, language support, actions, inspections, tool window) into an OpenIDEA plugin.
---

# Porting IntelliJ plugins to OpenIDEA

## Overview

IntelliJ plugins are JVM code against the IntelliJ Platform; OpenIDEA plugins are WebAssembly components plus a `plugin.toml`. **Nothing converts mechanically except themes.** Porting = learn *what* the plugin contributes (its `plugin.xml` **and** its source), map each piece to an OpenIDEA contribution or host API, then reimplement it in Rust: from source when the licence allows, clean-room from public behaviour otherwise. Most code plugins port only partly; say so up front.

**REQUIRED SUB-SKILL:** Use writing-openidea-plugins for manifest, SDK, build and `check`.

## 1. Find the plugin (Marketplace API, no auth)

Work in a fresh, empty directory: downloads are untrusted.

```sh
M=https://plugins.jetbrains.com
curl -s "$M/api/searchPlugins?search=rainbow&max=10"   # → .plugins[]: id, xmlId, name, pricingModel, hasSource, tags
curl -s "$M/api/plugins/<id>"                          # → pricingModel, urls.sourceCodeUrl, urls.licenseUrl, vendor.publicName, copyright (not always present)
curl -s "$M/api/plugins/<id>/updates?size=1"           # → [0].file, version, since/until
mkdir -p /tmp/port/<id> && cd /tmp/port/<id>
curl -sL -o plugin.zip "$M/files/<file>"               # redirects to downloads.marketplace.jetbrains.com
```

`/api/search/plugins` requires `&build=IC-<build>` (else HTTP 400). `vendor.name` can be a UUID; the human name is `vendor.publicName`.

## 2. Gate: pick the porting path

Read the licence first: the raw `LICENSE` from the repo, `gh api repos/<owner>/<repo>/license`, or a licence file inside the jar (`licenseUrl` is often an HTML page). "Open licence" = OSI licence allowing derivatives (MIT, Apache-2.0, BSD, GPL/LGPL/MPL with their terms).

| Plugin | Path |
|---|---|
| `FREE`, open licence, public source repo | **Source port:** port from the repo at the tag matching the Marketplace version (else the nearest; note the gap) |
| Open licence, no public repo | **Decompile port:** decompile the jars (`java -jar vineflower.jar lib/x.jar out/`) and port from that; the licence permits it |
| `PAID`, `FREEMIUM`, proprietary, or no licence | **Clean-room port:** reimplement the *behaviour* only, see below |

**Clean-room port.** Features aren't protected; code, text, icons and branding are. Inputs are public material only: the Marketplace description, docs, changelog, screenshots/videos, and what the user observes running the plugin. Do not download, unzip or decompile its jars; do not copy its code, UI strings, icons or assets; do not use its name or say "port of X" (say "inspired by" at most, or nothing). Licence the result yourself. Skip §3 (inventory from public docs instead) and skip the licence-file and author-credit steps below.

Source and decompile ports:
- Ship the original `LICENSE` (and `NOTICE`/`NOTICE.txt` if present; Apache-2.0 requires it) in the package, and use the same licence in `license`.
- Credit the original authors in `authors` and `description`.

All paths: use **your own** id prefix (`<your-publisher>.<name>`); ask the user if unknown. Never use `openidea.*` or the original vendor's name.

## 3. Inventory what it contributes

The package is a `.zip` with `<name>/lib/*.jar`, or a bare `.jar`:

```sh
unzip -q plugin.zip -d x
for j in x/*/lib/*.jar; do unzip -l "$j" | grep '\.xml$'; done                                    # every descriptor
for j in x/*/lib/*.jar; do unzip -l "$j" | grep -q META-INF/plugin.xml && unzip -p "$j" META-INF/plugin.xml; done
ls x/*/lib/                                                                                      # bundled third-party jars
```

Read `plugin.xml`, every other descriptor (`<depends config-file>` targets, v2 `<content>` module XMLs), then the source:
- `grep -rn "registerAction\|ActionManager" src/`: actions registered at runtime (often the headline feature) aren't in `plugin.xml`.
- Read what each `postStartupActivity` / `projectActivity` actually does before mapping it.
- Bundled jars (`commons-text`, `gson`, …) become Rust crates; check each crate's licence too.

## 4. Map each extension

| IntelliJ | OpenIDEA |
|---|---|
| `themeProvider` (`.theme.json`), `bundledColorScheme` (`.icls`) | **Automatic:** `pnpm theme:import-intellij X.theme.json X.icls --out <dir> --author "…"` (`scripts/theme/import-intellij-theme.ts` in the openidea repo), or in the IDE: Settings › Plugins › gear › _Import IntelliJ Theme from File…_. Mapping tables: openidea `docs/plugins/themes.md` › Porting a theme |
| `<action>` + `<keyboard-shortcut>` | `[[contributes.commands]]` + `[[contributes.keymaps]]`, logic in `run_command`. Set `category` so they group in Find Action |
| Actions registered at runtime (`ActionManager.registerAction`) | static commands; user-defined variants → a settings string or `host::storage` read by one command |
| `<add-to-group>`, `<group popup="true">`, `<separator>`, `<reference>` | `[[contributes.menus]]` is **flat**: `EditorPopupMenu`→`editor.context`, `ProjectViewPopupMenu`→`projectView.context`, `EditorTabPopupMenu`→`editorTab.context`, `ToolsMenu`→`tools`. Put only the few most-used commands there; nested groups, separators and other groups (`EditMenu`, …) → drop |
| Popup / quick pick / input dialog (`JBPopupFactory`, `Messages.showInputDialog`, `DialogWrapper`) | no popup or input API: a `Form` view whose buttons act on `ctx.editor()`; or fixed commands per common option; or a setting |
| `fileType` + `lang.*` (parser, highlighter, commenter, folding, structureView) | `languages/<id>.toml` + `syntax-provider` (hand lexer; writing-openidea-plugins reference.md §6.1), or a language server via `language-servers`. Existing language ids: openidea repo `languages/` |
| `fileIconProvider`, `iconProvider`, `iconMapper` | `[[contributes.fileIcons]]` or an `iconThemes` JSON (writing-openidea-plugins reference.md §5) |
| `completion.contributor` | provider `completion` |
| `inline.completion.provider` | provider `inline-completion` |
| `localInspectionTool`, `annotator`, `externalAnnotator` | `host::publish_diagnostics` (on `file-opened` / `file-saved` or a command) |
| `intentionAction`, inspection quick fixes | provider `code-actions` (+ `resolve_code_action`) |
| `gotoDeclarationHandler`, `psi.referenceContributor` | provider `definition` |
| `codeInsight.lineMarkerProvider` | provider `line-markers` |
| `toolWindow` (Swing) | `[[contributes.views]]`: `Tree`/`List`/`Form`/`Table`/`Message`, or `Html` / `web` page for custom UI |
| `fileEditorProvider` | `[[contributes.customEditors]]` (`layout = "split"` for editor + preview) |
| `applicationConfigurable` / `projectConfigurable` | `[contributes.settings]` JSON schema + `host::setting*`: scalars only (`boolean`, `string`, `number`, `integer`). List/object settings → `host::storage`, or a multiline string |
| `PersistentStateComponent` state | `host::storage` (1 MiB) |
| `applicationService` / `projectService` without state | plain fields on the plugin struct (rebuilt in `activate`) |
| `statusBarWidgetFactory` | `[[contributes.statusBar]]` / `host::set_status_item` |
| `notificationGroup`, `Notifications.Bus` | `host::notify` |
| `postStartupActivity`, `projectActivity` | depends on what it does: registers actions → static commands, no activation; real startup work → `on-startup-finished` (sparingly; prefer `on-language:`/`on-file:`) |
| listener `AnActionListener` | event `action-performed` (`events.input`) |
| listener `FileEditorManagerListener` | events `file-opened`, `editor-focused` |
| `FileDocumentManagerListener` (save) | event `file-saved` |
| git4idea `GitRepositoryChangeListener` | event `vcs-changed`; `vcs_status`, `vcs_remotes`, `vcs_fetch` |
| `configurationType` / run configurations | `[[contributes.runDetectors]]` |
| `internalFileTemplate`, `fileTemplates/` | `[[contributes.fileTemplates]]` |
| `PasswordSafe` / credential store | `[[contributes.secrets]]` + `{{secret:<name>}}` header |
| `HttpRequests`, OkHttp, etc. | `host::fetch` + `net:<host>` |
| `GeneralCommandLine`, `ProcessHandler` | `Project::run_process` / `host::open_terminal` + `process:<program>` |
| `Task.Backgroundable`, `ProgressIndicator` | `host::begin_task`, `is_cancelled` |
| `Timer`, `Alarm`, scheduled executors | `activation = ["on-schedule:<secs>"]` (≥ 60) |
| `CopyPasteManager.setContents` | `host::copy_to_clipboard` (`clipboard`) |
| PSI element under caret | approximate from document text + `selections()` |
| `resource-bundle` (i18n) | English strings in the manifest |

**No OpenIDEA equivalent (drop and list in the report):** reading the clipboard (paste actions), moving carets or setting selections (`CaretModel`, `SelectionModel`), PSI tree access and refactorings (rename, move, safe delete), writing files outside editor edits or code-action edits, project model / module / SDK APIs, Swing painting of IDE UI, `AWTEventListener` / raw input, live templates, `customizableActionGroupProvider`, `errorHandler`, opening a view from a command, dependencies on other plugins' APIs. Editor-wide behaviour (rainbow brackets, inlay hints, editor decorations) has no API: report it, don't fake it.

Not documented (verify in the openidea repo before relying on it): `when` expressions beyond `editorFocus`; whether `code-actions` receives the caret/range.

## 5. Build and report

1. `openidea-plugin new <your-prefix>-<name>` (or `--template theme|icon-theme|language` for data-only).
2. Fill `plugin.toml` from the table; request only the capabilities the ported parts use.
3. Implement in Rust (from source, decompiled source, or the clean-room behaviour spec); unit-test it with `MockHost`.
4. `openidea-plugin build` (runs `check`) until clean.
5. Report to the user: porting path, source plugin + version (+ source revision) + licence, what was ported, what was dropped and why, capabilities requested.

## Common mistakes

- Promising a full port before reading `plugin.xml` and the source.
- Reading only `plugin.xml`: runtime-registered actions and extra descriptors are missed.
- Decompiling a PAID/FREEMIUM/unlicensed plugin, or copying its strings and icons into a clean-room port.
- Dropping the `NOTICE` file of an Apache-2.0 plugin.
- Hand-mapping theme colours instead of running the import script.
- Asking for `process:`/`net:`/`clipboard` capabilities the ported parts don't use.
