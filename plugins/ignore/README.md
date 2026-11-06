# .ignore

Port of [.ignore](https://plugins.jetbrains.com/plugin/7495--ignore)
(`mobi.hsz.idea.gitignore` 4.5.7, [JetBrains/idea-gitignore](https://github.com/JetBrains/idea-gitignore)
commit `860a6855`, Apache-2.0) by Jakub Chrzanowski and JetBrains.

- Problems in ignore files, checked on open and save: duplicate, redundant
  (warning), relative and malformed entries, and invalid `syntax:` values
  (.hgignore). Quick fixes: remove the entry, remove the relative part, use
  glob or regexp syntax.
- `syntax:` completion in .hgignore. Ctrl-click on an entry that names one
  file opens it.
- _Add … to .gitignore_ quick fix (and an unignore variant with `!`) in any
  project file, when the project root has a .gitignore.
- New › _Ignore File_: a starter ignore file (`<name>.gitignore`).
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
- Settings › Plugins › .ignore: turn off the warning or the status, and insert
  templates at the caret.

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
