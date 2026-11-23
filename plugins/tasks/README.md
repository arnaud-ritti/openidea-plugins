# Tasks

Work on one task at a time. Create local tasks or pick issues from your tracker. With Git, each task gets its own branch and shelved changes, and switching back reopens its files. The plugin also tracks time per task and fills in commit messages. MIT; see `LICENSE`.

Supported trackers: GitHub, GitLab (gitlab.com), Codeberg, YouTrack (Cloud or self-hosted), Jira Cloud, Trello, Lighthouse Cloud, FogBugz Cloud, self-hosted Redmine and MantisBT, and other REST trackers you describe yourself.

## Requirements

- OpenIDEA with plugin API 1.14 or later (`engines.openidea = "^1.14"`).
- Optional: Git, for a branch per task, shelving and merge on close.
- Optional: an account and token for each tracker you want to use.
- Only for FogBugz, Redmine, MantisBT and generic trackers: Python 3 and a private credential profile file. Generic trackers with a `java-regex` parser also need Java 17+.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Open the **Tasks** tool window (left).
3. Click **New Task…** to create a local task, or set up a tracker (below) and click **More…** › **Search Issues…**.
4. Pick the task and click **Open Task…**. With `process:git` allowed, the form offers a branch for it.
5. When you're done, press ⌥⇧W (**Tasks: Close Active Task**).

To connect a tracker, for example GitHub:

1. Set the **GitHub token** secret in Settings › Plugins › Tasks ([about secrets](../../README.md#tokens-and-secrets)).
2. Allow `secrets` and `net:api.github.com`.
3. Leave **GitHub repository** empty to use the project's github.com remote, or enter `owner/name`.

## Features

### Manage tasks in the tool window

The **Tasks** window shows open tasks, closed tasks and tracker issues as trees with counts. It also shows the active task and its elapsed time. The toolbar has:

- **New Task…**: create a local task.
- **Open Task…** / **Switch Task** / **Reopen Task**: activate the selected task. With nothing selected, a searchable picker opens.
- **Close Active Task…**: review and close the active task, whatever is selected.
- **Task Actions…**: show the description, edit, switch, close, open in browser or post work time for the selected task.
- **More…**: **Search Issues…** (search a tracker), **Update Issues** (refresh issues now), and **Save Context…**, **Load Context…** and **Remove Saved Context…** (a named set of open files).

Open and Close forms remember your last choices. Task descriptions open in read-only tabs. A successful close shows a **Task closed** notification.

When the Issues group is empty, its row says why: no tracker is set up, network access isn't allowed, the update failed, or there are no open issues.

### Switch from the status bar or Search Everywhere

- The status bar shows the active task. Click it for a menu: **Copy Commit Message**, **Open in Browser** (issues), **Close Task…**, the other open tasks to switch to in one click (up to 20, then **All N Open Tasks…**), **New Task…** and **Open Task…**.
- Press Shift twice and open the **Tasks** tab to find an open task by id, summary or branch. Choosing it switches to it.

### Branch per task (Git)

With `process:git` allowed:

- Each new task gets a branch, named with **Branch name format**. It's checked out when you switch to the task.
- Your changes are shelved as stashes when you switch away (**Shelve changes when switching tasks**).
- Closing a task can merge its branch into the base branch, then delete the branch. The close form names both: **Merge branch … into …, then delete …**.
- The commit message can also go to Git's `commit.template` (setting **Write the active task's commit message to git's commit.template…**).

If Git fails, you get a warning and the task switch still happens.

### Reopen a task's files

When you switch back to a task, the last 10 files you opened while it was active reopen (needs `events`).

Named contexts save up to ten open editors of the current window, plus the active editor's selection. Use **Tasks: Save Context**, **Tasks: Load Context** and **Tasks: Remove Saved Context**, or **Contexts…** in the window. Saving needs `editor.read`.

### Commit messages

- **Tasks: Copy Commit Message**, or **Copy Commit Message** in the status bar menu, copies the active task's commit message.
- **Tasks: Apply Active Task to Commit Message** fills the Commit message field. It's also above the Commit message field. You review the old and new text first. It doesn't stage or commit anything.

The message uses **Commit message format** (default `{id} {summary}`).

### Work with tracker issues

- Issues update every 20 minutes (open issues only). Turn this off with **Update issues every 20 minutes**.
- Closing a task can resolve the issue on GitHub, GitLab, Codeberg, YouTrack, Jira, Lighthouse, FogBugz, Redmine, MantisBT and generic trackers that define a close action. If the tracker's network permission is denied, the task still closes and a warning says which permission to allow.
- Work time can be posted to YouTrack and Jira (**Task Actions…**).
- Read more repositories with **Tasks: Add Repository** (GitHub, GitLab or Codeberg). **Tasks: Remove Added Repository** removes one.
- **Tasks: Open Active Task in Browser** (⌥⇧B) opens the active issue.

### Search issues

**Tasks: Search Issues** asks for a query, then which server or repository to search if you have several.

- The picker offers **Next page (more results may exist)**, **Previous page (session cache)**, **Retry this page from server**, **New search query** and **Choose another server / repository**.
- Each page has up to 50 issues, and up to five pages per query. Refine the query for more.
- Pick an issue to open it as a task. Cancel keeps your active task.
- In Jira, start the query with `jql:` to send your own JQL. Otherwise the text becomes `text ~ "…"`.
- Turn on **Include closed issues in Search Issues** to find closed GitHub, GitLab, Codeberg and Lighthouse issues. Picking one creates a local task; it doesn't reopen the issue.

### Use issue IDs in your code

After a search, hover an issue ID in any file to see its summary, tracker and link. Right-click it for:

- **Tasks: Open Issue at Caret in Browser**.
- **Tasks: Start Task for Issue at Caret (No Branch)**: makes it the active task without touching Git.

**Tasks: Start Task with Branch for Issue at Caret…** opens the branch options form for that issue instead (needs `process:git`).

### Set up each tracker

| Tracker | What to set in Settings › Plugins › Tasks | Permission |
|---|---|---|
| GitHub | **GitHub token** secret; optionally **GitHub repository**, **GitHub: only issues assigned to me**, **GitHub: extra search qualifiers** | `net:api.github.com` |
| GitLab | **GitLab token** secret; optionally **GitLab project** | `net:gitlab.com` |
| Codeberg | **Codeberg token** secret; optionally **Codeberg repository** | `net:codeberg.org` |
| YouTrack Cloud | **YouTrack token** secret, **YouTrack host**; optionally **YouTrack search query**, **YouTrack state to set when closing a task** | `net:*.youtrack.cloud` |
| YouTrack self-hosted | Same, with your host name | `net:ask` (the IDE asks at first use) |
| Jira Cloud | **Jira credentials** secret, **Jira Cloud host**; optionally **Jira JQL query** | `net:*.atlassian.net` |
| Trello | **Trello API key** and **Trello token** secrets | `net:api.trello.com` |
| Lighthouse Cloud | **Lighthouse API Token** secret, **Lighthouse Cloud tenant host**, **Lighthouse project ID** | `net:*.lighthouseapp.com` |
| FogBugz Cloud | **FogBugz exact tenant host** and the profile path. See [profile setup](adapters/README.md). | `process:python3` |
| Redmine, MantisBT | **Redmine, MantisBT and generic trackers**. See [setup and server contracts](adapters/SELF_HOSTED.md). | `process:python3` |
| Other REST trackers | A `generic` entry in the same profiles list. See [adapters/GENERIC.md](adapters/GENERIC.md). | `process:python3` |

All token-based trackers also need `secrets`.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **Tasks**; bind keys in Settings › Keymap.

| Command | What it does | Shortcut |
|---|---|---|
| **Tasks: New Task…** | Opens the new task form in the Tasks tool window. | ⌥⇧K |
| **Tasks: Open Task…** | Picks an open task or cached issue and opens it. | ⌥⇧N |
| **Tasks: Switch Task** | Picks another task and switches to it. | |
| **Tasks: Close Active Task** | Opens the close form with your last choices. Also in **Tools**. | ⌥⇧W |
| **Tasks: Open Active Task in Browser** | Opens the active issue's page. Also in **Tools**. | ⌥⇧B |
| **Tasks: Copy Commit Message** | Copies the active task's commit message. Also in the status bar menu. | |
| **Tasks: Apply Active Task to Commit Message** | Replaces the Commit message with the active task's, after review. Also above the Commit message field. | |
| **Tasks: Update Issues** | Refreshes issues from all trackers now. | |
| **Tasks: Search Issues** | Searches one tracker and opens the picked issue. | |
| **Tasks: Save Context** | Saves the open files as a named context. | |
| **Tasks: Load Context** | Reopens a saved context. | |
| **Tasks: Remove Saved Context** | Deletes a saved context. | |
| **Tasks: Add Repository** | Adds another GitHub, GitLab or Codeberg repository. | |
| **Tasks: Remove Added Repository** | Removes an added repository. | |
| **Tasks: Open Issue at Caret in Browser** | Opens the issue whose ID is under the caret. Also in the editor context menu. Needs a focused editor. | |
| **Tasks: Start Task for Issue at Caret (No Branch)** | Activates that issue as a task without Git, after you confirm. Also in the editor context menu. Needs a focused editor. | |
| **Tasks: Start Task with Branch for Issue at Caret…** | Opens the branch options for that issue. Needs a focused editor. | |
| **Tasks: Reset Tasks for This Project…** | Deletes this project's saved tasks, time tracking, contexts and repository overrides, after a confirmation that counts them. For corrupt or unwanted data. | |

## Settings

Settings › Plugins › Tasks. The common ones:

| Setting | Key | Default | What it changes |
|---|---|---|---|
| Commit message format ({id} and {summary} are replaced; local tasks have no ID) | `commitMessageFormat` | `{id} {summary}` | `{id}` and `{summary}` are replaced. Local tasks have no ID. |
| Branch name format | `branchNameFormat` | `{id}` | Same placeholders. An empty name falls back to the first two words of the summary. |
| Pre-fill the commit message from the active task | `fillCommitTemplate` | off | Pre-fills the commit field. A template you set yourself is left alone. |
| Update issues every 20 minutes | `updateIssuesAutomatically` | on | Background issue refresh. |
| Include closed issues in Search Issues | `searchClosedIssues` | off | Adds closed GitHub, GitLab, Codeberg and Lighthouse issues to searches. |

Tracker settings:

| Setting | Key | Default |
|---|---|---|
| GitHub repository as owner/name (leave empty to use the github.com remote) | `githubRepository` | empty |
| GitHub: only issues assigned to me | `githubAssignedOnly` | off |
| GitHub: extra search qualifiers (label:bug, milestone:v2…) | `githubQuery` | empty |
| GitLab project as group/name (empty: the gitlab.com remote, or all your issues if there is none) | `gitlabProject` | empty |
| Codeberg repository as owner/name (leave empty to use the codeberg.org remote) | `forgejoRepository` | empty |
| YouTrack host, such as acme.youtrack.cloud (a self-hosted host is approved on first use) | `youtrackHost` | empty |
| YouTrack search query | `youtrackQuery` | `Assignee: me sort by: updated #Unresolved` |
| YouTrack state to set when closing a task | `youtrackCloseState` | `Fixed` |
| Jira Cloud host, such as acme.atlassian.net | `jiraHost` | empty |
| Jira JQL query | `jiraQuery` | `assignee = currentUser() and resolution = Unresolved order by updated` |
| Lighthouse Cloud tenant host, such as acme.lighthouseapp.com | `lighthouseHost` | empty |
| Lighthouse project ID (positive numeric ID) | `lighthouseProject` | empty |
| FogBugz exact tenant host, such as acme.fogbugz.com | `fogbugzHost` | empty |
| FogBugz credential file (readable only by you) | `fogbugzProfile` | empty |
| Redmine, MantisBT and generic trackers | `selfHostedProfiles` | none |

Secrets (set in the same page): **GitHub token**, **GitLab token**, **Codeberg token**, **YouTrack token**, **Jira credentials**, **Trello API key**, **Trello token**, **Lighthouse API Token**. Each one's description says which scopes it needs.

## Permissions

`project.read` and `storage` are required. Optional permissions are off until you allow them in Settings › Plugins › Tasks.

| Permission | Used for | Without it |
|---|---|---|
| `secrets` | Sending tracker tokens. The plugin never sees or logs them. | Token-based trackers can't be used. |
| `net:api.github.com`, `net:gitlab.com`, `net:codeberg.org`, `net:*.youtrack.cloud`, `net:*.atlassian.net`, `net:api.trello.com`, `net:*.lighthouseapp.com` | Fetching and updating issues on that tracker. Only trackers you set up are contacted. | That tracker is unavailable. The others still work. |
| `net:ask` | A self-hosted YouTrack host, approved in a dialog at first use. | Self-hosted YouTrack is unavailable. Background refreshes report it as denied. |
| `process:git` | Branch per task, shelving, merge on close, commit template. | Tasks work without Git. |
| `process:python3` | FogBugz, Redmine, MantisBT and generic tracker transports. | Those trackers are unavailable. |
| `events` | Recording the files of each task. | Switching back doesn't reopen files. |
| `editor.read` | Saving named contexts; the caret issue commands. | Those features are off. |
| `clipboard` | **Tasks: Copy Commit Message**, also in the status bar menu. | Copying fails. |
| `vcs.read` | Reading tracker remotes and the current commit draft. | Those features are off. |
| `vcs.write` | **Tasks: Apply Active Task to Commit Message**. | That command is off. |

## Troubleshooting

See also the [repository guide](../../README.md#troubleshooting).

| Problem | What to do |
|---|---|
| "No issue tracker set up: add a tracker token in Settings › Plugins › Tasks" | Set a tracker's token and host, and allow its `net:` permission. |
| "Issues couldn't be loaded: network access is not allowed (Settings › Plugins › Tasks)" | Allow the tracker's `net:` permission, then run **Update Issues**. |
| "Issues couldn't be loaded: run Update Issues to see why" | Run **Update Issues** and fix what the message says. |
| "Add a tracker token in Settings › Plugins › Tasks. Trackers that use a credential profile are searched with Search Issues." | Set the token secret and allow `secrets`. FogBugz, Redmine, MantisBT and custom REST trackers don't update automatically; use **Tasks: Search Issues**. |
| "The issue state was not updated: allow network access to … in Settings › Plugins › Tasks." | The task closed locally. Allow the tracker's `net:` permission and update the issue in the tracker. |
| "No github.com remote. Set the repository in Settings › Plugins › Tasks." | Fill **GitHub repository** (or the Codeberg equivalent). |
| "Set the … host in Settings › Plugins › Tasks." | Fill the tracker's host setting. |
| "No supported tracker/server is configured and authorized…" | Check token, host and repository, and the tracker's `net:` permission. |
| "Some trackers couldn't be updated" | The message lists each failure. The other trackers updated. |
| No branch is created for a task | Allow `process:git`. Without it, tasks switch without Git. |
| "Switched task, but some steps failed" / "Closed …, but some steps failed" | Git or the tracker failed. Fix what the message says; the task itself changed. |
| "Grant process:git and reload Tasks to review branch options." | Allow `process:git`, then Settings › Plugins › **Reload**. |
| "No unambiguous cached issue at the caret. Run Tasks: Search Issues first." | Run a search that returns that issue, then try again within 2 minutes. |
| "The server has more matches than the 50-result window; refine the query." | Narrow the search. |
| "FogBugz credential bridge failed…" | Check the profile file (owned by you, mode 0600), `process:python3` and the network. |
| "FogBugz bridge timed out; check remote issue state before retrying a close." | Look at the issue in FogBugz before you try again. |
| "Include changes from this task project only before applying its commit message." | Include only files from this project in the commit. |
| "This window has no open files owned by the current project to save." | Open some project files first. |
| "Task state: … Existing storage was not changed." | Stored data is unreadable. Run **Tasks: Reset Tasks for This Project…** if you want to start fresh. |

## Limitations

- No changelists, automatic context-on-commit, or full time-tracking UI.
- Contexts don't restore tab/split order, folds, unsaved buffer contents, bookmarks or breakpoints.
- Elapsed time is wall-clock time while a task is active, not focused coding time.
- Search returns at most five pages of 50 issues per query.
- Apply to Commit Message sets only the message. No automatic close-on-commit, merge choices or changelist commits.
- There's no transaction across Git and task storage.
- Issue data is a cached snapshot, not a live status.

## Technical details

<details>
<summary>Storage, search bounds, tracker contracts and safety checks</summary>

### Tool window

- The panel is native, with no packaged web frame. Native text fields commit on blur/Enter. It follows the standard Tasks & Contexts flow but isn't a pixel-for-pixel reproduction.
- Open works on the selected row; the active task disables Open. Close Active always reviews the active task, regardless of selection. Forms show the reviewed task as a read-only value above their branch/issue choices.
- The status bar is window-scoped: each view reports its own root's active task.

### Contexts

- Named contexts capture up to ten open project editors in the invoking window (API 1.4). Foreign/nested-project editors are excluded. More than ten owned tabs refuse rather than truncate.
- The active editor's first selection is restored only while its coordinates still fit the file.
- Legacy named contexts and automatic task-switch history are file-only and reopen at line/column zero.
- Reopening never closes, rewrites or silently saves an editor. All recorded paths are checked against the current owner before opening any, and the result says how many opened. Changed context state during a picker, or a nested project created under recorded paths, refuses reopening.

### Commit message

- The Apply review shows current/replacement text, task, project and draft revision. Cancel keeps the draft. Changed task text or commit revision refuses the write without retrying.
- Needs included changes all within the task project. Refuses empty drafts and mixed-project selection.
- The host may save included dirty files when reading the draft, as its Commit workflow does.

### Trackers

- Descriptions are kept as plain text (Jira Cloud descriptions are flattened from Atlassian Document Format). Formatting marks, links and embedded resources aren't executed or fetched. Bounded to 32 KiB with truncation markers. Hover shows a bounded escaped excerpt.
- Changing `searchClosedIssues` invalidates the active search ownership signature, so cached pages aren't reused. YouTrack and Jira keep their configured query semantics.

### Lighthouse Cloud

- `lighthouseHost` (e.g. `acme.lighthouseapp.com`), a positive numeric `lighthouseProject` and the `lighthouse-token` secret.
- The token is sent only in `X-LighthouseToken`. Every request stays bound to that tenant/project. IDs are `LH-<tenant>-<project>-<number>`.
- Close Task can set the documented `resolved` state after rechecking tenant/project and exact API/web paths.
- Responses capped at 1 MiB / 50 tickets. Malformed, foreign-project and incomplete XML pages, including DTDs, are refused. No external XML entities are resolved; raw error bodies aren't logged.

### FogBugz Cloud

- `fogbugzHost` is the exact tenant (such as `acme.fogbugz.com`). `fogbugzProfile` is the absolute path of an externally provisioned private JSON profile: an owned regular 0600 file, no symlinks.
- The token stays inside the helper. Native settings/storage get only an opaque profile fingerprint, issue number and latest-event identity.
- Each explicit search reviews tenant/profile/query and admits at most 50 validated results. The XML API has no offset, so refine larger queries.
- Startup and periodic refresh never launch the helper.
- Close offers a remote state update only for a validated closable issue. The helper checks permissions and event identity, submits optimistic `ixBugEvent` and verifies the closed state. TLS verification is mandatory; every redirect is refused. A timed-out mutation requires checking the remote issue before retrying.

### Self-hosted Redmine and MantisBT

- Up to 16 nonsecret `selfHostedProfiles` descriptors (`family`, exact HTTPS `baseUrl`, absolute `profilePath`). The external owned 0600 profile holds the API key and optional close-status ID. No API key enters native settings/storage.
- Search offers those servers with documented filter parameters and up to five 50-result pages. Redmine uses server status metadata and offset/limit. Mantis needs a project and reads its status/workflow metadata. Neither invents free-text or closed-state semantics when metadata is missing.
- Close reviews the exact installation/project/issue/target status and stored profile/revision. Redmine checks allowed closed statuses before PUT and verifies the result. Mantis verifies a strong ETag and uses `If-Match` for PATCH. Both recheck credentials and revision, refuse foreign identities, verify the closed state, and never retry failed/uncertain writes.
- Installations under different URL prefixes keep distinct task identities.

### Search

- GitHub, gitlab.com and Codeberg search one repository at a time (GitLab also has an all-visible-issues selection). YouTrack/Jira use a configured Cloud tenant. Trello uses the configured account. Unsupported or missing Cloud hosts are excluded.
- Repository paths, returned issue hosts and issue API endpoints are validated before a result can be selected. GitLab also needs response `references.full`, numeric project ID and IID to agree with the repository. Trello needs a 24-hex card ID plus an 8-character shortLink matching its URL; links use `https://trello.com/c/<shortLink>`.
- Empty results and request errors have separate states. Retry is explicit: no automatic retry or rate-limit loop.
- Picking a hit keeps the Open Task branch review, or activates directly when Git is off. An existing task with the same ID is reused only when tracker and issue API identity match.
- Paging: GitHub/GitLab/Codeberg use `page`; YouTrack `$skip`/`$top`; Trello zero-based `cards_page`. Next is offered after a full page, so the next page may be empty. Jira enhanced-JQL follows its opaque `nextPageToken`, including short/empty pages with a continuation. The five-page limit is a deliberate partial-results bound.
- `jql:` rules: at most 256 bytes, no control characters, balanced quotes/parentheses, no trailing `and`/`or`/`not`, at most one final `ORDER BY field [ASC|DESC], …`. Jira judges field and operator names. Jira errors show a bounded constant reason, without raw bodies or continuation tokens.
- The Trello member-card refresh listing is separate; only explicit searches use these pages.
- Previous pages come from a memory cache keyed by project, server/filter configuration, repository, query and page. Labelled, 120-second expiry, at most 20 pages / 1000 issues. Retry invalidates that query's pages; a new invocation clears the cache. Nothing is persisted.
- The host exposes secret presence, not credential identity. A same-name credential rotation while a picker is open can't be detected; retry or restart the search after changing accounts.
- Project/configuration membership is checked after each request and prompt, including right before accepting a hit.
- Limits: queries 256 bytes, 16 selections, 20 picker actions per invocation, 1 MiB tracker responses. Timeouts/cancellation are the host's job.

### Cached issue references

- Hover uses only validated in-memory search pages and the document's verified project. No storage/file reads, source searches, git commands, tracker requests or credential lookups on hover.
- Missing/conflicting mappings, embedded Unicode identifiers, unknown IDs, expired results and same-ID/different-server collisions stay quiet. Only the last explicit search is used; local tasks and background refresh results aren't. These facts aren't serialized with tasks. Snapshots never claim current authorization or live status.
- The caret commands need one caret or one exact ID selection, current source text/version, the same project, an unexpired unambiguous hit and unchanged configuration. Source documents up to 512 KiB; issue token ASCII up to 128 bytes.
- Local activation previews root, issue, tracker, identity and scope, rechecks ownership after confirmation, refuses conflicting task identities, and updates local task/time tracking only. It bypasses Git, remote state and context reopening. Opening uses the already-validated HTTPS issue page with no new request.
- The branch review binds source text/version, project, issue identity, search expiry/configuration and branch-name format; Open Task rechecks them. Preparing or cancelling changes nothing and runs no Git. Branch creation can be turned off and shelving chosen. Existing branches are switched to; otherwise Git creates from HEAD.

### Stored task state

- Task lists, active task/clock, choices, repository overrides and named contexts persist under the exact project-root key with a schema/owner/revision envelope.
- Malformed, foreign-owner or unsupported records never fall back to a default. Valid legacy records are validated (including file paths) and upgraded on the next successful write.
- Empty task arrays, unknown identities, duplicate IDs, foreign/traversal paths and oversized data fail without replacing stored bytes. Storage denial doesn't create volatile replacement state.
- Different project roots keep separate counters and data.
- Limits: 2 MiB serialized; 512 tasks, 1000 cached issues, 64 named contexts, ten files per task/context; bounded names/descriptions/overrides. A rejected oversized write keeps the last valid record.
- Writes recheck the original raw record and advance a checked revision, so stale writers fail instead of retrying. The SDK has no atomic compare-and-set, so a final read/write race remains.
- Reset names the project and counts its tasks, contexts and repository overrides (or says the saved data can't be read), rechecks the stored data, then replaces only this project's record with a fresh default. It changes no Git, tracker, source files or other project's key.
- Tree keys, field keys and button IDs carry opaque project/snapshot/form identity. Only an exact local write confirmation advances a callback binding, so stale callbacks can't submit another project's form.
- UI drafts and selection reset across projects; only submitted state persists.
- File-open events record only exact owning-project paths (longest-root check for nested projects).
- Active clock fields survive restart as elapsed wall-clock time, including inactive time.

</details>

## Development

    cargo test
    openidea-plugin build
    openidea-plugin link
