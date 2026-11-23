# .ignore

Support for ignore files: `.gitignore`, `.dockerignore`, `.npmignore`, `.hgignore` and many more. It checks entries, completes paths, adds github/gitignore templates and tells you when a file is ignored. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- Optional: `git` on the `PATH` for **Ignore: Scan Git for Ignored Files…**.
- Optional: `python3` on the `PATH` for **Ignore: Find Unused Entries…**.
- No accounts or network access.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open a project and its `.gitignore`. Problems are underlined; press ⌥⏎ on one for a fix.
3. Open the **Ignore Templates** tool window on the right. Double-click a template to add it to the open ignore file.
4. In any other project file, press ⌥⏎ and pick **Add … to .gitignore**.

## Features

### Spot mistakes in ignore files

Checks run when you open or save an ignore file.

- Duplicate entries.
- Redundant entries, as a warning. Patterns are compared with each other, not with files on disk.
- Relative and malformed entries.
- Invalid `syntax:` values in `.hgignore`.

Quick fixes on ⌥⏎: remove the entry, remove the relative part, or switch to glob or regexp syntax.

### Complete and follow paths

- Path completion in glob-syntax files (`.gitignore`, `.dockerignore`…). It offers files and folders below the ignore file's folder, also after `/` and `!`.
- `syntax:` completion in `.hgignore`.
- Ctrl-click an entry that names one file to open it.

### Ignore or unignore a file from anywhere

- In any project file, ⌥⏎ offers **Add … to .gitignore** and an unignore variant with `!`. The project root needs a `.gitignore`.
- In the Project view or editor, right-click and choose **Add Selected Paths to the Open Ignore File** or **Unignore Selected Paths in the Open Ignore File**. With several ignore files you pick one. An ignore file that isn't open is written directly and then opened (needs `project.write`).
- Each ⌥⏎ change is shown before it is applied and can be undone. CRLF line endings stay CRLF.
- Turn off the unignore variant with **Offer Add to .gitignore (unignore) in the quick-fix menu**.

### Create an ignore file

- **New › .gitignore File** and 19 more exact names (`.dockerignore`, `.npmignore`, `.hgignore`…). The Git template has a starter list. Others start with a comment; `.hgignore` starts in glob mode.
- ⌥⏎ › **Create exact-name ignore file…** in any project editor. Pick a name and a folder (empty means the project root), then confirm.
- ⌥⏎ › **Create ignore file from templates…** combines up to 16 templates into one new file.
- **Ignore: New Ignore File…** (Project view, Tools menu) creates the file directly. From a project editor it runs **Create exact-name ignore file…**; from a folder or file selected in the Project view, or without an editor, it creates an empty file of the type you pick in that folder (or at the project root). You confirm first, and the file opens. It is not a single undo step.

### Use templates

- The **Ignore Templates** tool window lists the [github/gitignore](https://github.com/github/gitignore) templates and your own.
- Double-click a template to add it to the open ignore file.
- **Star** a template to list it in a **Starred** group.
- **Create User Template** saves the focused file as a template. **Delete User Template** asks first.
- ⌥⏎ › **Append ignore template…** adds up to 16 templates to a glob-syntax file. Recently chosen templates are listed first.
- Templates go at the end of the file. Turn on **Insert templates at the caret instead of at the end of the file** to change that.

### Know when a file is ignored

- **Ignored** shows in the status bar while the focused file is ignored. The plugin reads `.git/info/exclude` and every `.gitignore` from the root down to the file.
- Click **Ignored** to see the rule and the file it comes from. **Go to Rule** opens that file at the rule. **Ignore: Go to Rule Ignoring This File** does the same from the command palette.
- A warning appears once per project when a Git repository has no `.gitignore`.

### Highlighting and folder markers

- Ignore files the IDE doesn't detect (`.chefignore`, `.p4ignore`, `.vercelignore`…) get the `dotignore` language with highlighting.
- Entries ending in `/` get a folder icon in the gutter. Negated folder entries get a re-inclusion tooltip.
- After you open or save an ignore file, literal paths that match real project folders also get the folder icon for 30 seconds.

### Choose what runs for each file type

**Ignore: Dialect Checks and Preferences** (Tools menu) opens a form that works without a project. Pick one of 42 file names and set:

- **Editor assistance**: checks, quick fixes, completion, navigation, markers, colouring and edits for that file type.
- **Allow Create action**: whether ⌥⏎ create actions offer that name.
- **Checks**: turn checks off while keeping the rest.

The four workflow flags (warning, status, caret insertion, unignore) take **Inherit**, **On** or **Off**. **Inherit** uses the value from Settings. Press **Save** to keep changes; **Cancel** or **Reload** discards them. The same form opens from the gear in the **Ignore Templates** window.

### Find unused entries (optional)

**Ignore: Find Unused Entries…** scans the folder of the focused `.gitignore` with `python3`. Entries that match no file get a hint and a reviewed removal fix. Results reflect the moment of the scan; run it again to refresh.

### Sample what Git ignores (optional)

These commands run `git ls-files` once, after showing exactly what will run.

- **Ignore: Scan Git for Ignored Files…** collects a sample of ignored paths.
- **Ignore: Show Last Git Scan of Ignored Files** shows the sample and how many paths each root `.gitignore` rule matched.
- **Ignore: Mark Ignored Files from Last Git Scan** puts an **I** badge on up to 16 sampled files.
- **Ignore: Clear Ignored-File Marks** removes the badges.
- **Ignore: Clear Last Git Scan of Ignored Files** forgets the sample.

Results are always partial. They are a sample, never a full list of ignored files.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Ignore**; bind keys in Settings › Keymap.

| Command | What it does | Shortcut |
|---|---|---|
| **Ignore: Add Selected Paths to the Open Ignore File** | Adds the paths selected in the Project view to the open ignore file, or to a root ignore file you pick. Also in the Project view and editor context menus. | none |
| **Ignore: Unignore Selected Paths in the Open Ignore File** | Adds `!` entries for the selected paths. Also in the Project view and editor context menus. | none |
| **Ignore: New Ignore File…** | Creates an ignore file directly after you confirm (not a single undo step): the editor's **Create exact-name ignore file…**, or an empty file of the picked type in the selected folder or at the project root. Also in the Project view menu and Tools. | none |
| **Ignore: Go to Rule Ignoring This File** | Opens the ignore file at the rule that ignores the focused file. Also in the **Ignored** status bar menu. | none |
| **Ignore: Dialect Checks and Preferences** | Opens the per-file-type preferences form. Also in Tools. | none |
| **Ignore: Scan Git for Ignored Files…** | Shows the Git command, then collects a sample of ignored paths. | none |
| **Ignore: Show Last Git Scan of Ignored Files** | Shows the cached sample and rule match counts. | none |
| **Ignore: Clear Last Git Scan of Ignored Files** | Forgets this project's cached sample. | none |
| **Ignore: Mark Ignored Files from Last Git Scan** | Badges up to 16 sampled files with **I**. | none |
| **Ignore: Clear Ignored-File Marks** | Removes those badges. | none |
| **Ignore: Find Unused Entries…** | Scans the `.gitignore` folder and marks entries that match nothing. | none |

## Settings

Settings › Plugins › .ignore. Each setting is used when **Ignore: Dialect Checks and Preferences** says **Inherit** for it.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Warn about a Git project without a .gitignore file | `missingGitignore` | on | The once-per-project warning. |
| Show Ignored in the status bar when editing an ignored file | `ignoredEditing` | on | The **Ignored** status. |
| Insert templates at the caret instead of at the end of the file | `insertAtCursor` | off | Where templates go. |
| Offer Add to .gitignore (unignore) in the quick-fix menu | `unignoreActions` | on | The `!` variant of the quick fix. |
| Colour additional ignore files (shared) | `packageSyntax` | on | Highlighting for `dotignore` files. The IDE's own grammar is unchanged. |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › .ignore.

| Permission | Used for | Without it |
|---|---|---|
| `process:git` | Running `git ls-files` for the Git observation commands. | Those commands are refused. |
| `vcs.read` | Finding the repository root and HEAD for the Git observation commands. | Those commands are refused. |
| `process:python3` | Running the helper for **Ignore: Find Unused Entries…**. | That command is refused. |
| `project.write` | **Ignore: New Ignore File…** writing the new file; **Add/Unignore Selected Paths** writing an ignore file that isn't open. | That command says which permission to allow. New and ⌥⏎ still create files; adding to the focused ignore file still works. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| "Open the ignore file to add the selected paths to." | Open the target ignore file in the editor, then run the command again. |
| "Select files or folders in the Project view first." | Select the paths to add, or run the command from the editor of the file to add. |
| "The selected paths are already listed or are outside the ignore file's folder." | Select paths below the ignore file's folder that aren't listed yet. |
| "Selected-path append supports glob-syntax ignore file types only." | Use a glob-syntax file. `.hgignore` in regexp mode isn't supported. |
| "Editor assistance is disabled for this ignore file type." | Turn **Editor assistance** back on in **Ignore: Dialect Checks and Preferences**. |
| "No enabled Ignore Create actions are available." | Turn on **Allow Create action** for at least one type in the preferences form. |
| "A selected template header is already present; review the file before appending again." | The template is already in the file. Remove it first if you want it again. |
| **Add … to .gitignore** isn't offered | The project root needs a `.gitignore`, and the entry must not be listed already. |
| A hidden or ignored file isn't completed | The IDE's file search hides dot-paths and ignored files. Type the path by hand. |
| "Enable checks for this ignore file before collecting unused-entry hints." | Turn **Checks** on for `.gitignore` in the preferences form. |
| "Complete unused-entry checks require delivered filesystem events; reload Ignore after granting events." | Reload the plugin in Settings › Plugins. |
| "Complete inventory refused an error, bound or concurrent change; no absence conclusions." | The folder is too large, changed during the scan, or has unsupported names. See the limits below. |
| "No Git scan for this project yet. Run Ignore: Scan Git for Ignored Files… first." | Run **Ignore: Scan Git for Ignored Files…** first. |
| "Eight project inventories are retained. Clear an unneeded project's cache first." | Run **Ignore: Clear Last Git Scan of Ignored Files** in another project. |
| "Git inventory supports a concrete Unix project/repository root of at most 512 bytes." | The project must be the repository root on macOS or Linux. Worktrees, submodules and Windows roots aren't supported. |
| "Stored Ignore preferences are malformed…; preserved without changes." | The stored form data is damaged. The plugin won't overwrite it, and create/append actions are refused until it's fixed. |

## Limitations

- Built-in `ignore` files (`.gitignore`…) aren't coloured yet. The IDE renderer has no style for these token kinds.
- Git observations are always a partial sample, never a complete list or your full Git policy.
- Templates can't be inserted into regexp files, including `.hgignore`.
- Folder markers need the entry to end in `/`.
- No visual generator dialog like other IDEs' ignore support.
- Unignoring a file may also need its parent folder unignored. The plugin doesn't add those rules.

## Technical details

<details>
<summary>Matching rules, limits, caching and safety checks</summary>

### Matching

- Only entries that name one path, or a glob inside a literal directory (`build/*.o` under `build/`), are checked for redundancy. Ignored files can't be listed, so nothing is compared with the disk.
- The lexical matcher follows Git's whitespace rules. Leading spaces and escaped trailing spaces are part of the pattern; unescaped trailing spaces are ignored. Duplicate and redundancy checks use the same spelling policy for glob dialects.
- A child negation can't restore a file beneath an excluded directory (every parent must first be included, as in Git). Nested ignore files can't override an excluded ancestor.

### Add, unignore and create actions

- **Add … to .gitignore** and its unignore variant are non-overwriting, undoable file actions. Literal paths escape wildcard, bracket, backslash and space characters. Newline, control and traversal paths are refused.
- The target's existing LF/CRLF and current unsaved text are preserved. The source path, version and project are bound, and target content is rechecked after confirmation. Each buffer or addition is limited to 256 KiB. No program or clipboard permission is required.
- **Create exact-name ignore file…** offers the 20 supported names and an existing project-relative directory. You confirm the exact destination and initial content, and the new file opens. Existing files or buffers, outside paths, traversal and invalid directories are refused.
- **Create ignore file from templates…** is an atomic intention combining up to 16 bundled or user templates into one new glob-dialect file. The exact combined content and destination are reviewed first. Dismissal, duplicate or unknown selections, existing files or buffers and changed source context return no operation. Regexp dialects (including `.hgignore`) are excluded from template workflows.
- **Append ignore template…** confirms the exact insertion. Existing template headers are refused instead of silently duplicating a block. Template pickers list the up-to-16 most recently chosen templates first (stored locally, bounds-checked).
- **Ignore: New Ignore File…** applies the same confirmed creation from a command, writing through `project.write`; it refuses existing files. **Add Selected Paths…** and **Unignore Selected Paths…** edit the focused ignore file through the version-checked editor; another ignore file is written through `project.write`, which the IDE refuses while that file has unsaved changes.
- Native New uses the `fileName` field for exact dotfile names rather than an appended extension.
- Native-intention creation and append hold no filesystem lock after resolution. Final application, encoding/BOM preservation, conflict detection and undo are owned by the host.

### Ignore Templates tool window

- **Create User Template** uses a native input prefilled with the focused file name. Empty or duplicate names are rejected, and the document, version and editor identity are rechecked before saving.
- Starred templates are kept in storage.
- Deleting a user template uses a native confirmation and rechecks the selected template and its stored content first. Cancel changes nothing.
- A right-aligned **Preferences** gear opens the preferences draft in the same window, with Back to Templates.

### Highlighting

The IDE's own `ignore` language receives honest semantic tokens: comments (headers and sections use Documentation), glob operators, character classes, escaped literals, Mercurial regexp entries, and known syntax/include directives, with Comment/Operator/Regexp/String/Keyword kinds. Paths are never mislabelled just to obtain a colour. Visible colouring of the built-in language is blocked by the pinned host renderer, which maps no CSS class for these token kinds. The backend contract and the `dotignore` lexical fallback remain in place.

### Preferences form

- One exact-file-name selector covers 42 shipped and built-in file names. Each type has independent **Editor assistance**, **Allow Create action** and **Checks** controls. Switching types keeps the current draft; stale controls from the previously selected type are refused.
- Disabling assistance or checks clears tracked manual diagnostics on Save (up to 256 retained paths). Other results update on the next editor or provider request.
- Controls are exact-file-name. Unlisted or custom file names keep their defaults.
- Turning creation off doesn't disable editing an existing file. Turning assistance off doesn't forbid independently enabled creation. The Create action supports all 42 catalog names, including additional `dotignore` files. Static native New-menu templates stay registered and aren't hidden. Create choices are filtered by type and checked again during resolution and after confirmation.
- A fifth shared colouring switch controls lexical token output for the `dotignore` language family. Per-file lexical disablement is unavailable because the syntax request carries language and text but no file name; the switch never guesses from the active editor or equal text. Filename-aware semantic tokens can be suppressed independently.
- Invalid values block saving. One bounded 16 KiB write compares both preference keys plus all five scalar baselines.
- Controls use a separate version-2 storage key. Valid legacy v1 data is read only when v2 is absent and copied into a draft without rewriting it. Intervening changes to either key refuse a pending Save. Malformed v2 data is preserved and blocks draft and save rather than falling back to legacy. Duplicate decoded JSON object keys (including escaped duplicates) are refused and preserved. Creation and append paths refuse unavailable preference evidence.

### Folder markers

- Markers describe source syntax. They don't assert that a directory exists or navigate to a guessed path. Light and dark icon variants are packaged.
- Blank, comment, empty/root-only, regexp, escaped/bracketed or overlong patterns are quiet. Mercurial mode switches are tracked, and invalid switches suppress further markers until a known switch. Includes aren't traversed. No filesystem, inventory, process, network or project discovery runs from the decoration providers.
- After open or save, up to 64 safe literal paths beneath the file's folder are checked against project directories. Matching lines can show the folder icon for up to 30 seconds using the exact cached source.
- These cached observations survive irrelevant focus, status and other-file events and duplicate save→filesystem-watcher echoes, only after bounded current-source, known-directory and innermost-owner proof. Matching source or target changes, unavailable proof, expired TTL, disabled support, oversized event batches and unknown future events discard the sample. Watcher echoes keep the original 30-second observation time.
- No filesystem reads happen while rendering. Wildcards, regex, escaped paths and foreign project owners are excluded. Directory admission uses host-confined stat results (the host may follow links within its root, so physical path identity isn't established). This is a positive historical match, not complete directory discovery.
- Limits: 128 KiB per document, the first 4,096 lines, 1,024 bytes per directory pattern, 256 markers and 4,096 semantic tokens. All spans are nonempty, sorted UTF-16 ranges. Unsaved source replaces output immediately. The providers don't modify checks or preferences or infer ignored-file coverage.

### Git observations

- An explicit opt-in short background collection task. It verifies the project's native repository root and normal `.git` directory. It then previews the exact project/CWD, HEAD, Git argv, added environment and limits before starting one owned managed child with stdin closed. A source/repository check after confirmation refuses a changed scope.
- Unix project/repository roots up to 512 bytes are supported. Worktrees, submodules, bare repositories, subdirectory projects and Windows roots aren't. No ancestor or default working directory is used.
- The read-only request is `git --no-pager --no-optional-locks -C ROOT --git-dir=ROOT/.git --work-tree=ROOT -c core.fsmonitor=false -c core.untrackedCache=false -c core.excludesFile=/dev/null ls-files --others --ignored --exclude-standard --full-name -z --`.
- Explicit safe Git directory/worktree/index/common-directory/object/config/trace environment values are additive. An inherited name collision is refused rather than silently replaced. System and global configuration and global excludes are disabled: this observes repository ignores, not your complete global Git ignore policy. No shell, alias, external helper, fsmonitor, pager, Git write command or network operation is requested. You still grant execution of the host-resolved Git program.
- The managed child belongs to the command's native progress task. Cancellation is checked between bounded reads. Cancellation, deadline, errors and source/project changes kill and drop the child. At most 40 collection rounds use one native wait of up to 100 ms plus at most three nonblocking polls per round, with a four-second cooperative deadline. Source, API or dispatch delays can extend wall time; this isn't a hard OS execution timeout. No durable watcher, automatic retry, restart or scheduled collection is installed. Provider requests and cached rendering never start, wait on or poll a process.
- The process transport keeps only the last 1 MiB per stream without a loss flag and can't prove complete output. So each poll discards its first NUL fragment and trailing unterminated fragment; only intact interior records become observations. A single short ignored path can yield no samples, and the first path in every poll is omitted. Results are at most **Partial**, never an ignored-file total, and never feed absence diagnostics or native file status.
- Combined output is limited to 512 KiB, 2,048 unique paths, 64 KiB per stream/poll, 1,024 bytes per path, and UTF-8 without replacement, control or path-traversal forms. On a bound, valid earlier samples can be kept as Partial or Stale. Native repository, HEAD, index, root-rule and exclude changes are checked during collection, and file/VCS events mark matching cached roots stale.
- The review shows status, at most 256 observed paths and up to 128 root-`.gitignore` literal-rule sample counts. Counts use the lexical matcher, not proof of which rule caused Git's decision. Zero means **unknown**, never an unused-rule warning. Git reads disk ignore files, so unsaved editor rules aren't applied.
- Successful-exit Partial snapshots keep their capture scope. The review adds a historical-provenance row and rechecks that scope even when a file/VCS event was missed, retiring a changed or unavailable proof to Stale while keeping positive samples. It starts no Git child and collects no new paths; rendering and summary are cache-only.
- States are Not scanned, Running, Partial and Stale. Caches are session-only, up to eight exact project roots, stale after 120 seconds. Reload or reclaim forgets them. Clearing affects only that root's transient cache, not preferences or files.
- Inventory commands need one current open normalized Unix root. Anonymous, root-directory, trailing/double-slash, backslash or dot-segment roots are refused. No review can turn Partial into Complete, generate an absence warning, collect paths or publish effective ignored-file decorations.
- Sample hints use a muted I badge and a historical tooltip, from a successful Partial capture younger than 120 seconds that passes current scope checks. No new Git child starts. Clearing hints, and observed source/project/preferences/lifecycle events, withdraw the owned set (the host supplies no automatic decoration expiry).

### Unused entries from a complete directory snapshot

- The command reviews and runs the optional `python3` helper (`adapters/complete_inventory.py`) in isolated mode (`-I -S`). It scans the focused `.gitignore` directory twice, including hidden and ignored paths and Git metadata.
- It follows no symlinks. It refuses errors, unsupported file-system names or kinds, concurrent changes and limits: 4,096 entries, 512 directories, depth 24, 8 seconds, 1 MiB result.
- The helper writes its final complete result atomically into plugin-private storage, so managed stdout loss can't turn a partial scan into a complete inventory. It imports no project application and changes no project file.
- For admitted complete inventories, unmatched entries appear as native snapshot hints with an exact reviewed, undoable removal action.
- ASCII glob matching conservatively ignores case to avoid false absence across case-sensitive and case-insensitive file systems. Non-ASCII literal and glob patterns are evaluated by exact bytes. Misses are trusted only when no inventory path is non-ASCII, and non-ASCII patterns with `?` or `[` stay withheld. Unsupported pattern forms are counted and withheld.
- This checks whether names match any existing path, not whether a rule is Git's winning rule or whether future generated files will need it.
- Providers use only source-, version- and owner-bound cached results, never file-system scans or processes. Eligibility lasts 120 seconds, and observed source or file-system changes withdraw it.

### Packaging

`templates/` is [github/gitignore](https://github.com/github/gitignore) at `53cf2971` (CC0-1.0, see `templates/LICENSE`), embedded in the component by `build.rs`. The plugin registers syntax, providers, views and file actions through the plugin API. Required capabilities: `project.read`, `editor.read`, `editor.write`, `events`, `storage`. Optional: `process:git`, `process:python3`, `vcs.read`, `project.write`. Missing optional grants disable the matching opt-in commands.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
