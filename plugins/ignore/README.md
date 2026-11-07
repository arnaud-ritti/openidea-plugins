# .ignore

Port of [.ignore](https://plugins.jetbrains.com/plugin/7495--ignore)
(`mobi.hsz.idea.gitignore` 4.5.7, [JetBrains/idea-gitignore](https://github.com/JetBrains/idea-gitignore)
commit `860a6855`, Apache-2.0) by Jakub Chrzanowski and JetBrains.

- Problems in ignore files, checked on open and save: duplicate, redundant
  (warning), relative and malformed entries, and invalid `syntax:` values
  (.hgignore). Quick fixes: remove the entry, remove the relative part, use
  glob or regexp syntax.
- Path completion in glob-syntax ignore files (.gitignore, .dockerignore…): files and folders below the
  ignore file's folder, after `/` and `!` too. The IDE hides dot-paths and ignored files from the file
  search, so those are not offered. `syntax:` completion in .hgignore. Ctrl-click on an entry that names one
  file opens it.
- _Add … to .gitignore_ quick fix (and an unignore variant with `!`) in any
  project file, when the project root has a .gitignore.
- New › _Ignore File_: a starter `<name>.ignore` file (the `.ignore` file type). Dot-named files such as `.gitignore` are recognised when opened, not created from the New menu: a New menu entry needs a non-empty extension that is appended to the typed name, so a bare `.dockerignore` can't be produced.
- _Ignore Templates_ tool window: the github/gitignore templates and your
  own (_Create User Template_ saves the focused file). Double-click a template
  to add it to the open ignore file.
- _Ignored_ in the status bar while the focused file is ignored, according to
  `.git/info/exclude` and every `.gitignore` from the root down to the file.
- A warning, once per project, for a Git repository without a .gitignore.
- The `dotignore` language, with highlighting, for ignore files the IDE
  doesn't detect (.chefignore, .p4ignore, .vercelignore…). The IDE's own
  `ignore` language (.gitignore, .dockerignore…) can't take a plugin's
  highlighting.
- Star a template with _Star_ in the Ignore Templates tool window; starred
  templates (kept in storage) are listed in a _Starred_ group.
- Settings › Plugins › .ignore: turn off the warning, the status or the unignore
  variant of the quick fix (`unignoreActions`), and insert templates at the caret.
- _New Ignore File_ (Project view, Tools): pick .gitignore, .dockerignore… and the file is created at the project root (or the selected folder) with `tee`; needs the optional `tee` permission, otherwise the name is copied. _Unignore Selected Paths_ adds `!/path` entries. Both and _Add Selected Paths_ are also in the editor context menu (the current file when nothing is selected). Results show as notifications.
- Adding an entry to a file with CRLF line endings keeps them CRLF.

The redundant-entry check compares patterns. The original matched them against
the files on disk, which plugins can't list once they are ignored. Only entries
that name one path, or a glob inside a literal directory (`build/*.o` under
`build/`), are checked.

`templates/` is [github/gitignore](https://github.com/github/gitignore) at
`53cf2971` (CC0-1.0, see `templates/LICENSE`), embedded in the component by
`build.rs`. `src/lib.rs` registers the component by hand: `register_plugin!`
has no arm for syntax with providers and views.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
