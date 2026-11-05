# Tasks

Port of the IntelliJ **Issue Trackers** plugin (`com.intellij.tasks` 263.7307.108, intellij-community tag
`idea/263.7307.112`: `plugins/tasks`, `platform/tasks-platform-impl`, the GitHub tracker from `plugins/github`
and `GitTaskHandler` from `plugins/git4idea`). Apache-2.0, © JetBrains s.r.o. (see `LICENSE`, `NOTICE`).

- **Tasks** tool window: create, edit, switch and close tasks, see the time spent on each, post it, and update issues.
  IntelliJ's dialogs become the New, Open and Close forms, which remember their choices.
- **Git** (`process:git`): each new task gets a branch (`branchNameFormat`), checked out when you switch to it.
  Changes are shelved as stashes. Closing a task can merge its branch into the base branch and delete it. The commit
  message can also go to `commit.template`.
- **Context** (`events`): switching back to a task reopens the last 10 files opened while it was active.
- Status bar: the active task. Click it to copy its commit message (`commitMessageFormat`, default `{id} {summary}`).
- Commands: Close Active Task (⌥⇧W, with the Close form's last choices), Open Active Task in Browser (⌥⇧B),
  Copy Commit Message, Update Issues.
- Trackers: GitHub, gitlab.com, codeberg.org, YouTrack Cloud, Jira Cloud and Trello. Each one is used once its token
  is set in Settings › Plugins › Tasks, and issues are updated every 20 minutes. Closing a task can resolve GitHub,
  GitLab, Codeberg, YouTrack and Jira issues; work time can be posted to YouTrack and Jira.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
