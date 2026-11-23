# GitLab

GitLab merge requests in OpenIDEA. List them, follow their timeline, review the changes with comments on diff lines and pending reviews, and approve, merge, rebase, close or reopen them. The plugin also shows the current branch's merge request in the status bar and its unresolved threads in the editor. It checks out merge request branches, clones and shares projects, links to the current lines on GitLab and creates snippets. MIT; see `LICENSE`.

## Requirements

- OpenIDEA with plugin API 1.14 or newer.
- A gitlab.com account, or a self-hosted GitLab served over HTTPS.
- A personal access token with the `api` and `read_user` scopes.
- A project with a git remote on that server (`https://`, `ssh://` or `git@gitlab.com:`). When several remotes point there, `origin` wins.
- `git` on the `PATH` for checkout, worktrees, clone and Share Project.
- Optional: `glab` 1.x, authenticated on its own, for **Follow Job Log (glab)** and **GitLab: Check Out Merge Request Source (glab)…**.

## Quick start

1. Install the plugin ([how](../../README.md#install-a-plugin)).
2. Run **GitLab: Create Access Token** (or click **Create Access Token** in the GitLab tool window). It opens GitLab's token page with the name `OpenIDEA GitLab` and the `api` and `read_user` scopes filled in.
3. Paste the token into Settings › Plugins › GitLab (**GitLab personal access token**).
4. Self-hosted GitLab: set **GitLab server** to its HTTPS URL. The IDE asks you to approve the host on first use.
5. Open the **GitLab** tool window (left). Your merge requests appear; select one to see its details.

## Features

### Browse merge requests

The **GitLab** tool window has three tabs: **Merge Requests**, **Details** and **Projects**. Commands select the tab they need.

The **Merge Requests** tab lists the project's merge requests as `!iid title`, with author, branches and draft state.

- **Repository** picks which GitLab repository of the workspace the tab shows. See [Several repositories in one window](#several-repositories-in-one-window).
- **Filter** cycles through the quick filters: Open, Include my changes, Need my review, Assigned to me (the default) and Closed. It can also pick an author, reviewer, label or text filter.
- Expand a merge request to load its timeline in date order: threads (author, reply count, resolved or not, `path:line`) with their replies, plus GitLab's system notes and state, label and milestone changes.
- **Hide Events** / **Show Events** toggles the system notes and changes.
- Select an item to show it in **Details**. A selected thread or reply is the one you reply to.
- Diff threads open their local source line when available. Other threads keep their GitLab links.

### Several repositories in one window

A workspace folder can hold several independent git repositories: the folder's own, plus clones in child folders.

- **Repository** in the **Merge Requests** tab lists the GitLab repositories it finds (the window's own first) and switches the tab and **Details** to the one you pick. The status line shows its `group/name`.
- Without a pick, the window's own repository is shown. When the window's repository has no remote on the GitLab server, the workspace's only GitLab repository is used; with several, the tab asks you to pick one.
- **GitLab: Open on GitLab** and **GitLab: Copy Link** use the repository containing the file, whichever one the tab shows.
- **Check Out Branch**, worktrees and **GitLab: Create Merge Request** run git in the window's own repository only. For a child repository, open its folder in its own window.
- **GitLab: Show Current Branch's Merge Request** switches back to the window's own repository.
- Child folders are found from the project index and the folders git ignores or doesn't track. Up to 32 child repositories are read.

### Review a merge request

**Details** shows the selected merge request. A row of section buttons switches between **Overview**, **Description**, **Changes**, **Commits**, **Files**, **Pipelines**, **Timeline** and **Local Drafts**. The active one has a dot; the others show counts from cached data. The choice is remembered, even when you select another merge request. A failing section shows the error with **Refresh**.

**Overview** is the main form.

- A comment field, or a reply field under the note you reply to.
- **Add to my review** (on by default) keeps the comment as a pending review comment.
- The status line sums up state, draft, branches, author, merge status, pipeline and approvals.
- Buttons appear when they apply:
  - **Comment**, **Reply** or **Add to Review**.
  - **Submit Review (n)** publishes pending comments. Text in the comment field is posted as a summary.
  - **Resolve Thread** / **Unresolve Thread**, **Approve** / **Revoke Approval**.
  - **Merge**, when you can merge and GitLab reports `mergeable`. Options: **Delete source branch** and **Squash commits**. Click twice: the second click reads *Confirm Merge !iid into target* with the chosen options. If the request is ready but you lack the rights, the status line says *You cannot merge this merge request*.
  - **Rebase**, when GitLab reports `need_rebase`.
  - **Edit Labels, People, Milestone…** (same as **GitLab: Edit Merge Request**), **Close** (click twice), **Reopen**.
  - **Check Out Branch**, when the merge request isn't merged and isn't the current branch.
  - **Open on GitLab**, **More…**, **Refresh**.
- **More…** holds the less frequent actions that apply: **React**, **Add Me as Reviewer**, **Re-request Review…**, **Mark as Draft** / **Mark as Ready**, **Open MR Snapshot**, **Open Diff Snapshot**, **Open Existing Worktree** and **Copy Link**.
- Your own comments (not system notes) get **Edit**, **Delete** (click twice) and an award emoji (**More…** › **React**) from eight: `thumbsup`, `thumbsdown`, `heart`, `tada`, `laughing`, `confused`, `rocket`, `eyes`.

**Changes** shows the full diff, one block per file with `+added −removed`. Existing threads and pending comments sit under their lines.

- Click a line number to comment (**Add to Review** or **Comment Now**). Threads have **Reply** and **Resolve** / **Unresolve**.
- **Viewed** collapses a file until it changes again.
- Click a file name to open it locally, or to open it in the IDE's side-by-side diff viewer. An added or deleted file shows as added or deleted, with no empty side. Click the header to fold it.
- Binary and very large files have no diff here; open them on GitLab.

**Commits** lists the merge request's commits. Double-click one to see its diff, read-only, in **Changes**. Commit diffs have no comments and no viewed marks.

**Files** shows the changed files as a tree or a list (**Tree** / **List**). Double-click a file to toggle Viewed, or use **Mark All Viewed** and **Clear Viewed**. It shares the marks of **Changes** and works without page scripts.

**Pipelines** shows the newest 20 pipelines with their jobs by stage. Retry a job or pipeline, cancel a pipeline (click twice), or open it on GitLab. See [job logs](#read-only-snapshots-and-job-logs).

**Timeline** lists discussions and activity with Kind, Author, When, Status and Summary columns, plus search and facets.

- **Refresh Timeline** reads new data from GitLab. Opening and filtering use cached data only.
- **Open** opens the merge request or comment page on GitLab.
- **Reply in Overview** selects the discussion and returns to the reply form. It doesn't post or clear an unsent draft.

**Local Drafts** keeps up to 50 unsent review comments per merge request, saved in the plugin.

- **Add Draft**, **Edit**, **Delete**, **Clear All**.
- **Submit All to Review** adds them to your pending review.
- A draft is bound to the project, merge request and head commit. After a new push it is flagged stale and never submitted. **Rebind to Current Head** fixes that.

The `commitsView` and `pipelineView` settings turn off **Commits** and **Pipelines**. `noteActions` turns off the comment actions.

### Read descriptions and comments

The **Description** and diff comments render CommonMark: tables, task lists, strikethrough and code blocks.

- Raw HTML is shown as text.
- Only HTTP(S) links without embedded credentials are kept.
- Images show their alternative text and are never fetched.
- `@mentions` show as highlighted plain text, never links. A small set of `:emoji:` shortcodes is shown, outside code.
- `$math$`, `$$math$$` and ` ```math ` blocks show as labelled source, not rendered.
- Large inputs are visibly truncated.

### Work with the current branch

When the branch's upstream is on a GitLab remote, the status bar shows `!iid on <branch>` for its open merge request. Forks are skipped. Click it for a menu: **Show Merge Request** selects it in **Details**, **Open Merge Request on GitLab** opens its page, and **Open Pipeline on GitLab** appears when GitLab reported its latest pipeline.

A branch other than the default branch with no open merge request shows **GitLab: Create Merge Request** instead. The lookup runs again on every repository change. It also runs when a file opens, if the commit changed or the last lookup is over 5 minutes old.

With **Show the current branch's unresolved merge request threads in the editor** on, unresolved threads appear in open files. Each shows as a gutter marker and an `Info` problem (source `GitLab`) on its line, new side. They're hidden while the local branch is behind its upstream.

### Apply a review suggestion

When a review thread has a suggestion block (` ```suggestion:-N+M `) on a line of an open file, press ⌥⏎ there and choose **GitLab: Apply suggestion**.

- It replaces the N lines above, the commented line and the M lines below with the suggestion.
- A plain ` ```suggestion ` replaces the commented line. An empty one deletes it.
- It is offered only for unresolved threads shown in the editor.
- It edits the open file only. Nothing is posted, and GitLab doesn't mark the suggestion as applied.
- Check the lines first: the plugin compares them with the cached **Changes** diff only when it has one.

### Check out a merge request

- **Check Out Branch** (Details button) fetches the source branch and checks it out, creating a tracking branch if needed. An existing local branch is fast-forwarded, or left alone if it has diverged. Fork merge requests are fetched from `refs/merge-requests/<iid>/head` into `fork/<author>/<branch>`.
- **GitLab: Open Merge Request in Worktree** fetches the selected merge request (else the current branch's) and runs `git worktree add --detach` in a sibling folder `<repository>-<branch>`. Git refuses a folder that already holds files. A notification shows the path and offers **Open Worktree**.
- **GitLab: Open Existing Merge Request Worktree** lists your local worktrees that match the merge request's head, and opens the one you pick in a new window. It creates nothing.
- **GitLab: Check Out Merge Request Source (glab)…** checks out the source branch with `glab` in a terminal. See [Commands](#commands).
- **GitLab: Rebase Merge Request** asks GitLab to rebase the selected merge request (else the current branch's) onto its target, after you confirm which one. It is the same call as **Rebase** and reports conflicts.

### Create merge requests, snippets and projects

These ask a few questions in a row instead of opening a window.

- **GitLab: Create Merge Request** asks for the source branch (local branches, current first), the target branch (project branches, default first), the title (the last commit subject), a one-line description and draft or not. Dismissing any of the first four prompts creates nothing. If GitLab refuses, a notification offers to open the new-MR page on GitLab. Set assignee, reviewers, labels and milestone afterwards with **GitLab: Edit Merge Request**.
- **GitLab: Create Snippet** uses the focused file or its selection, or the files selected in the Project view. It asks for the title, the visibility (public needs a confirm) and more project files, one path per prompt until an empty answer. Dismissing any prompt creates nothing. The URL is copied and shown.
- **GitLab: Share Project** asks for the name and visibility. When the project has no git repository or no GitLab remote, the tool window offers it as **Share Project on GitLab…**. It creates the project in your namespace, adds it as `origin` (or `gitlab` if `origin` exists) and runs `git push -u` in a terminal. A repository without a commit is initialised and committed after a confirm that lists the files. Files that look secret (`.env*`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`) are refused.

### Clone a project

The **Projects** tab lists the projects you're a member of, most recently active first. Double-click one to run `git clone` in a terminal, into the **Clone directory**. When none is set, the plugin asks for a directory. A notification offers **Open Clone** once the clone has started.

### Link to code

- **GitLab: Open on GitLab** opens the file at the current commit, with the selected lines (`#L3-L5`). It is in the editor, editor tab and Project view context menus.
- **GitLab: Copy Link** copies the same link.

### Read-only snapshots and job logs

**Open MR Snapshot** and **Open Diff Snapshot** (Overview › **More…**; Changes has the diff one) open read-only editor tabs. The MR snapshot holds the title, description and branch details. The diff snapshot holds the unified text diffs. You can't save or comment in these tabs.

- **Reload Cached Snapshot** shows the cached copy again, without contacting GitLab.
- **Refresh Snapshot** re-reads GitLab. It keeps the old snapshot on failure. Retry after a transient failure, or open a new snapshot after a new push.
- **Return to Review** goes back to **Details** without clearing your draft.

**Open Job Trace Snapshot** (on a selected pipeline job) opens the job's log as it is now, read-only. It doesn't follow new output or retry on its own.

**Follow Job Log (glab)** opens a live log tab running `glab ci trace <job-id> --repo <repository-url>`. It needs an approved `glab` 1.x (`process:glab`), signed in on its own. The plugin checks the CLI version and asks you to confirm the repository and job. The tab owns output, restart and cancel. Live log tabs don't run in remote projects.

### Use two accounts

Settings › Plugins › GitLab has two tokens: **GitLab personal access token** and **Secondary GitLab.com personal access token**. **GitLab.com account slot** picks which one to use. Switching clears the old account's cached data. Finish or clear an unsent comment before switching; switching back lets you finish it.

### Edit GitLab CI files

In `.gitlab-ci.yml` and the other CI file names below, problems appear on open and save:

- `needs`, `dependencies`, `extends` or `!reference` naming a job that isn't in the file.
- A job `stage` that isn't in `stages` (or the default stages).
- A job listed twice in one `needs`, `dependencies` or `extends`.
- `$VAR` inside `echo '…'` / `printf '…'` script lines, which the shell doesn't expand.

Cmd+click a job name in `needs`, `dependencies`, `extends` or `!reference` to open the job. Cmd+click a `stage` value to open its entry in `stages`.

Completion offers:

- top-level keywords, and job keywords one level inside a job or `default`;
- the predefined `CI_*` variables after `$` or `${`;
- values for `when`, `stage` (from `stages`), `allow_failure` and `interruptible`;
- job names for `extends`, `needs` and `dependencies` (inline, flow or list items);
- the job, then the key, inside `!reference [`.

The popup opens on letters, not on `$`.

`include: local:` files are read (up to three levels), so their jobs and stages count.

The GitLab icon is shown on `.gitlab-ci.yml`, `.gitlab-ci.yaml`, `*.gitlab-ci.yml`, `*.gitlab-ci.yaml`, `*.gitlab-ci.*.yml` and `*.gitlab-ci.*.yaml`.

## Commands

Find them in Search Everywhere › Actions (⇧⇧) under **GitLab**; bind keys in Settings › Keymap. No shortcuts are set by default.

**Edit Merge Request**, **Re-request Review…**, **Open Existing Merge Request Worktree** and **Check Out Merge Request Source (glab)…** stay disabled until you select a merge request. **Open Cloned or Worktree Folder** stays disabled until a clone or worktree is offered.

| Command | What it does | Menus |
|---|---|---|
| **GitLab: Open on GitLab** | Opens the file at the current commit, with the selected lines (`#L3-L5`). In the Project view, the first selected file or folder. | Editor, editor tab and Project view context |
| **GitLab: Copy Link** | Copies the same link; in the Project view, the selected file or folder's link. | Editor and Project view context |
| **GitLab: Edit Merge Request** | Picks labels, assignees, reviewers, milestone and title from the project's lists. | Details button **Edit Labels, People, Milestone…** |
| **GitLab: Create Snippet** | Creates a snippet from the selection, the file or the selected files. | Editor, editor tab and Project view context |
| **GitLab: Create Merge Request** | Asks for branches, title, description and draft, then creates the merge request. | Tools |
| **GitLab: Share Project** | Creates a GitLab project and pushes this repository to it. Needs a commit, a checked-out branch and no GitLab remote yet. | Tool window button, when there is no GitLab remote |
| **GitLab: Show Current Branch's Merge Request** | Selects the current branch's merge request (also in the status bar item's menu). | — |
| **GitLab: Open Merge Request in Worktree** | Adds a detached worktree for the merge request in a sibling folder. | — |
| **GitLab: Open Existing Merge Request Worktree** | Opens a local worktree that matches the merge request's head, in a new window. Creates nothing. | Details › **More…** |
| **GitLab: Open Cloned or Worktree Folder** | Opens the folder from the last clone or new worktree in a new window. The **Open Clone** / **Open Worktree** notification buttons run it. | — |
| **GitLab: Rebase Merge Request** | Asks GitLab to rebase the merge request onto its target, after you confirm; reports conflicts. | — |
| **GitLab: Create Access Token** | Opens GitLab's token page with name and scopes filled in. | Tool window button |
| **GitLab: Re-request Review…** | Picks an assigned reviewer, confirms, and asks GitLab for a new review. Cancelling sends nothing. | Details › **More…** |
| **GitLab: Check Out Merge Request Source (glab)…** | Checks out the merge request's source branch with `glab` in a terminal. | — |

**GitLab: Check Out Merge Request Source (glab)…** in detail: it needs a clean working tree and saved editors. It asks you to confirm the merge request, source branch and new local branch. Then it opens a terminal running `glab mr checkout <iid> --repo https://gitlab.com/<target> --branch review/mr-<iid>-<head-prefix>` (any glab 1.x; no `--force`). It may update branch tracking and configuration, and prompt about conflicts. This is a source-branch checkout, not an exact-head snapshot. Stop cancels the process, but fetched refs and configuration may remain.

Editor quick fix (⌥⏎): **GitLab: Apply suggestion**.

## Settings

Settings › Plugins › GitLab.

| Setting | Key | Default | What it changes |
|---|---|---|---|
| GitLab personal access token | `token` (secret) | — | The token for gitlab.com or your server (`api`, `read_user`). |
| Secondary GitLab.com personal access token | `secondary-token` (secret) | — | Optional second account. |
| GitLab.com account slot | `accountSlot` | `primary` | Which token to use: `primary` or `secondary`. |
| GitLab server | `server` | empty (`https://gitlab.com`) | HTTPS URL of a self-hosted GitLab. Approved on first use. |
| Show the current branch's unresolved merge request threads in the editor | `editorReview` | `true` | Threads as gutter markers and problems. |
| Edit and delete your own comments and react with emoji in the merge request details | `noteActions` | `true` | Comment actions in **Details**. |
| Show the commits of the selected merge request and each commit's changes | `commitsView` | `true` | The **Commits** section. |
| Show the pipelines and jobs of the selected merge request, with retry and cancel | `pipelineView` | `true` | The **Pipelines** section. |
| Clone and share using SSH | `cloneUsingSsh` | `false` | SSH URL instead of HTTPS. |
| Clone directory | `cloneDirectory` | empty | Where clones go. Empty: next to the current project, or ask. |
| Report needs, dependencies and extends that name a job missing from .gitlab-ci.yml | `ciJobReferences` | `true` | Also covers `!reference`. |
| Report a job stage that is not in stages | `ciStages` | `true` | |
| Report a job listed twice in needs, dependencies or extends | `ciDuplicateJobUsage` | `true` | |
| Report $VARIABLE inside single quotes in script lines | `ciSingleQuotedVariables` | `true` | |

## Permissions

Optional permissions are off until you allow them in Settings › Plugins › GitLab.

| Permission | Used for | Without it |
|---|---|---|
| `clipboard` | **Copy Link**, the snippet URL | The link is shown in a notification that says the permission is off. |
| `project.read` | Jumping to a thread's line, threads in the editor, finding the repositories in child folders | A thread's double-click opens its GitLab page. **Repository** offers only the window's own repository. |
| `events` | Status bar item and threads in the editor | Neither is shown. |
| `process:git` | **Check Out Branch**, worktrees, clone, **Share Project**, Viewed tracking, reading the remotes of child repositories | Those actions fail with a message to allow git. Viewed compares with the head commit. Child repositories aren't found. |
| `process:glab` | **Follow Job Log (glab)**, **Check Out Merge Request Source (glab)…** | Those actions fail. |
| `net:ask` | A self-hosted server set in **GitLab server** | Only gitlab.com works. |

The token is added to requests by the IDE; the plugin never reads it ([how tokens work](../../README.md#tokens-and-secrets)).

## Troubleshooting

| Problem | What to do |
|---|---|
| "Enter a GitLab personal access token…" | Run **GitLab: Create Access Token**, then paste the token in Settings › Plugins › GitLab. |
| "GitLab rejected the access token." (401) | The token is wrong, expired or missing `api` / `read_user`. Make a new one. Check **GitLab.com account slot** points at the token you set. |
| "The repository has no remote on the configured GitLab server…" | Click **Share Project on GitLab…** to publish it, add a remote on your GitLab server, or set **GitLab server** to match your remote. Plain-HTTP servers aren't supported. |
| A self-hosted server isn't used | The value must be an HTTPS URL; an invalid value falls back to gitlab.com. Approve the host when asked (revocable in Settings › Plugins). |
| "Checking out a branch needs the process:git permission. Allow it in Settings › Plugins › GitLab." (or "Adding a worktree needs…") | Allow `process:git`. |
| "… needs GitLab CLI glab 1.x (found …)" | Install glab 1.x, sign in with `glab auth login`, and allow `process:glab`. |
| "The commit list is off" / "The pipeline view is off" | Turn on `commitsView` / `pipelineView` in Settings › Plugins › GitLab. |
| "Select a merge request in the GitLab window." | Select one in the **Merge Requests** tab first. |
| "This workspace has several GitLab repositories…" | Click **Repository** in the **Merge Requests** tab and pick one. |
| "The selected merge request belongs to another workspace repository…" | Open that repository's folder in its own window to check out, add worktrees or create merge requests. |
| "Finish or clear … unsent comment before switching…" | Post or clear the comment in the original project or account first. |
| "The IDE shows pickers only for commands." | Run **GitLab: Edit Merge Request** from Search Everywhere. |
| Clicking a diff line does nothing in **Changes** | The desktop IDE blocks scripts in plugin pages. Comment from **Overview** or on GitLab. Use **Files** for Viewed. |
| "Snapshot … budget exhausted; retry explicitly." | Wait a moment, then **Refresh Snapshot** again. |
| Lists look incomplete | Long lists stop after 10 pages or 35 s. Use **Filter** to narrow them. |

More: [repository troubleshooting](../../README.md#troubleshooting).

## Limitations

- One server at a time (gitlab.com or one HTTPS self-hosted instance), with two account slots. No OAuth login and no account manager.
- Lists stop at 10 pages: 500 merge requests, 1,000 threads per merge request, 1,000 projects, 1,000 diff files.
- No commenting from the editor gutter. Per-commit diffs are read-only.
- Line comments, replies, resolving and Viewed in **Changes** need scripts, which the desktop IDE currently blocks in plugin pages. Comment from **Overview** or on GitLab instead. Viewed also works in **Files**.
- Threads in the editor don't move with unsaved edits.
- No `@mention` completion or attachments in notes. GitLab custom emoji, math and server-side markup extensions are not rendered.
- The prompts have no user, label or milestone pickers, no file chooser and no file tree. Extra snippet files are typed as paths.
- Branch names, refs and paths must use `[A-Za-z0-9._/@+-]`. Project paths over 64 characters can't be cloned.
- Pipelines have no live log in the plugin (use glab) and no manual job play.
- CI files: only the four checks listed. No schema validation. Remote, project, template and component includes are not followed. Keyword lists date from November 2026 and may miss newer names.
- No usage statistics.

## Technical details

<details>
<summary>Limits, caching and safety checks</summary>

### Connection

- REST API under `https://gitlab.com/api/v4` (or `<server>/api/v4`). `https://gitlab.com/api/graphql` is used for Mark as Draft / Mark as Ready.
- The IDE puts the token in the `Authorization` header; the plugin only gets secret placeholders.
- A self-hosted server uses `net:ask`, revocable under Settings › Plugins.
- Lists are read page by page: merge requests 50 per page, projects 100 per page, up to 10 pages each. All the lists of one command or refresh share a 35 s budget; lists after it are partial or empty.

### Input checks

Values that go into a URL or a git command (branch and ref names, remote names, usernames, commit SHAs, repository paths, clone URLs) are checked first and refused when they fail. A branch or path outside `[A-Za-z0-9._/@+-]` can't be pushed or cloned through the plugin. Clones outlast the 60 s call limit and may ask for credentials, so they run in a terminal.

### Ownership and caching

- MR caches belong to the exact repository root, GitLab project path and selected remote. A changed or unavailable owner clears list, details, thread, diff, commit, job, pipeline and branch caches. Equal MR IDs in different projects or worktrees can't reuse an old cache.
- A non-empty unsent comment blocks selecting in another scope, rather than exposing or discarding it.
- Refreshing the review pane also invalidates the list cache. MR data is never broadcast in invalidation messages.
- Switching account slots expires account identity, project lists, selected-MR ownership and snapshot caches, so an old account's controls can't act as the new one.
- Viewed marks are compared with the last merge request commit touching the file, read from local git history. Without `process:git` or the commits locally, the head commit is used.
- Local drafts are stored in plugin storage (up to 50 per merge request, 8 KiB each), bound to project, MR and head commit.

### Suggestions

**Apply suggestion** isn't offered when the cached **Changes** diff shows different text on any line of the replaced range that appears in that diff. Lines outside the diff aren't checked. Without a cached diff it doesn't check at all.

### Markdown

Rendering is bounded CommonMark with shared native tokens. Link clicks must belong to the selected MR snapshot before opening externally.

### Timeline

Built from cached discussions and optional activity data, with selected bounded plain-text body facts. **Refresh Timeline** commits only after rechecking the merge request owner and head. Display is capped at 500 rows and 384 KiB with 4 KiB bodies. An untruncated snapshot is still labelled bounded/historical.

### Snapshots

- Each snapshot is bound to the current GitLab project, root and remote, the selected MR and its full head commit. A fresh owner/head check after loading blocks publishing when those change. Refresh refuses to rebind an old tab to a changed head.
- Snapshot content never grants write or review authority: save and unknown actions are rejected as read-only.
- Content opens are tied to the packaged-view frame that started them, so two windows with the same local request ID can't resolve each other's replies. Frame routing is UI correlation, not an authentication boundary.
- Capped at 384 KiB of UTF-8 text with a visible truncation marker. NUL and control characters are replaced; Unicode boundaries are kept.
- At most four snapshots per plugin instance; the oldest is evicted, and its callbacks report expiration. Reopen from the current MR view.
- API pagination, server limits and binary files can omit diff content, so a diff snapshot doesn't claim to be complete. Refresh is subject to the host's content-action timeout and the request budget.
- Job trace snapshots (API 1.4) use the REST trace route, never a process or browser. MR, pipeline and job membership and owner/head guards are rechecked before fetching. Up to 1 MiB of response is accepted (host transfer cap 8 MiB) and shown within the 384 KiB cap. HTTP reads share the request budget, with a timeout up to 20 s. No polling or retry.
- **Follow Job Log (glab)** verifies MR, pipeline and job membership before opening the tab. glab uses its own credentials and configuration; the plugin doesn't export its token.

### Existing worktrees

- The merge request must be selected in the current project's list. Selection is bound to the GitLab project path, repository root and remote. The project root must equal the repository root; nested project folders are rejected.
- Fresh GitLab reads verify the target project and current head.
- Only `git worktree list --porcelain -z` runs: no fetch, checkout, clone or worktree creation.
- A branch checkout must match the merge request's local branch (including `fork/<author>/` ownership). A detached checkout must have the exact full head commit.
- Locked, prunable, ambiguous and malformed records are excluded or rejected. Lists are bounded to 1 MiB and 256 records.
- Dismissing the picker or cancelling opens nothing. Selection, project, remote, head and worktree registration are rechecked before requesting a new window; the host runs its normal trust flow.
- Worktree paths must be on the UI machine, as `open_project` requires. No local-to-remote path mapping is inferred.

### Re-request Review

Rechecks the current repository, MR, head and reviewer membership before using GitLab's re-review mutation.

### CI files

The reader is a line scanner, not a YAML parser: unusual shapes are skipped. Checks stay quiet when the file has a remote, project, template or component include, a glob, or a local include that can't be read. They also stay quiet for `needs` entries with `project:` or `pipeline:`, and for names built from variables. Keyword and variable lists are typed from the GitLab docs as of November 2026.

</details>

## Development

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
