# GitLab

gitlab.com merge requests in OpenIDEA. List them, follow their timeline,
review the changes with comments on diff lines and pending reviews, and
approve, merge, rebase, close or reopen them. The plugin also shows the
current branch's merge request in the status bar and its unresolved threads in
the editor, checks out merge request branches, clones and shares projects,
links to the current lines on GitLab and creates snippets.

## Setup

1. Run **GitLab: Create Access Token** (or click *Create Access Token* in any
   GitLab tool window). It opens gitlab.com's personal access token page with
   the name `OpenIDEA GitLab` and the scopes `api` and `read_user` filled in.
2. Paste the token into Settings › Plugins › GitLab (the `token` secret).

The IDE puts the token in the `Authorization` header itself; the plugin never
reads it. A 401 response means the token was rejected.

The project needs a git remote on gitlab.com (`https://`, `ssh://` or
`git@gitlab.com:`). When several remotes point there, `origin` wins.

The plugin only talks to gitlab.com: the REST API under
`https://gitlab.com/api/v4`, and `https://gitlab.com/api/graphql` for Mark as
Draft / Mark as Ready. Self-hosted servers aren't supported. Every host has to
be listed in `plugin.toml` as `net:<host>`, so another server means changing
`SERVER` / `SERVER_HOST` in `src/api.rs` and rebuilding.

Optional permissions:

| Permission | Used for | Without it |
|---|---|---|
| `clipboard` | Copy Link, the snippet URL | the link is shown in a notification |
| `project.read` | jumping to a thread's line, editor review | a thread's double-click opens its GitLab page |
| `events` | status bar item and editor review | neither is shown |
| `process:git` | Check Out Branch, clone, Share Project | those actions fail |

## Tool windows

- **Merge Requests** (left): the project's merge requests (50 per page, up to 10 pages or 45 s) as
  `!iid title`, with author, branches and draft state. *Filter* cycles through
  the original's quick filters: Open, Include my changes, Need my review,
  Assigned to me (the default) and Closed. Expanding a merge request loads its
  timeline in date order: threads (author, reply count, resolved or not,
  `path:line`) with their replies, plus GitLab's system notes and state,
  label and milestone changes. *Hide Events* / *Show Events* toggles the
  system notes and changes. Selecting an item shows it in Merge Request (a
  selected thread or reply is the one you reply to). Double-clicking a diff
  thread opens its line in the local checkout; anything else opens its GitLab
  page.
- **Merge Request** (right): a form with the comment field, or a reply field
  under the note you reply to when a thread is selected, the *Add to my review*
  checkbox (on by default) that keeps it as a pending review comment, the merge
  options and the read-only description. The status line sums up state, draft,
  branches, author, merge status, pipeline and approvals. Buttons, shown when
  they apply:
  - Comment, Reply or Add to Review;
  - Submit Review (n): publishes the pending comments, and posts the comment
    field's text as a summary if it isn't empty;
  - Resolve Thread / Unresolve Thread;
  - Approve / Revoke Approval;
  - Merge, enabled when you can merge and GitLab reports `mergeable`, with
    *Delete source branch* and *Squash commits* checkboxes;
  - Rebase, when GitLab reports `need_rebase`;
  - Add Me as Reviewer, Mark as Draft / Mark as Ready, Close, Reopen;
  - Check Out Branch, when the merge request isn't merged and isn't the
    current branch;
  - Open on GitLab, Copy Link, Refresh.
- **Merge Request Changes** (bottom): the cumulative diff of the selected
  merge request, one block per file with `+added −removed`, existing threads
  and pending comments under their lines. Click a line number to comment
  (*Add to Review* or *Comment Now*); threads have Reply and Resolve /
  Unresolve. *Viewed* collapses a file and is remembered until the file changes again
  (compared with the last merge request commit touching it, read from the local
  git history; without `process:git` or the commits locally, with the head commit). Clicking a file name opens it locally; clicking the header folds it.
  The diff stays readable without scripts, with every file expanded.
  Binary and very large files have no diff here; open them on GitLab. Toolbar: Refresh,
  Submit Review (n), Open on GitLab.
- **GitLab Projects** (left): the projects you're a member of (100 per page, up to
  10 pages, most recently active first). Double-clicking one runs `git clone` in a
  terminal, into the clone directory. Open the clone with File › Open when it
  finishes.

## Current branch

When the branch's upstream is on a gitlab.com remote, the status bar shows
`!iid on <branch>` for its open merge request (forks are skipped, as in the
original). Clicking it selects the merge request in the Merge Request and
Merge Request Changes tool windows. A branch other than the default branch
with no open merge request shows *GitLab: Create Merge Request* instead. The
lookup runs again on every repository change, and when a file opens if the
commit changed or the last lookup is more than 5 minutes old.

With `editorReview` on, the merge request's unresolved threads appear in the
open files as gutter markers and `Info` problems (source `GitLab`) on their
line on the new side. They're hidden while the local branch is behind its
upstream, as in the original.

With a review thread's suggestion block (` ```suggestion:-N+M `) on a line of an
open file, *GitLab: Apply suggestion* appears in the quick fixes there. It
replaces the N lines above, the commented line and the M lines below with the
suggestion (a plain ` ```suggestion ` replaces the commented line; an empty one
deletes it). It's offered for the unresolved threads shown in the editor and
isn't offered when the Changes view's cached diff shows a different text on the
commented line; without that diff it doesn't check, so look at the lines before
applying. It only edits the open file; nothing is posted to GitLab.

## Commands

| Command | Does | Menus |
|---|---|---|
| GitLab: Open on GitLab | opens the file at the current commit on gitlab.com, with the selected lines (`#L3-L5`) | editor and editor tab context |
| GitLab: Copy Link | copies the same link | none |
| GitLab: Create Snippet | creates a private snippet from the selection, or the whole file, named after the file; the URL is copied and shown | editor and editor tab context |
| GitLab: Create Merge Request | opens gitlab.com's new merge request page for the current branch | Tools |
| GitLab: Share Project | creates a private project in your namespace named after the folder, adds it as `origin` (or `gitlab` if `origin` exists) and runs `git push -u` in a terminal; needs a commit, a checked-out branch and no gitlab.com remote yet | none |
| GitLab: Show Current Branch's Merge Request | selects the current branch's merge request (the status bar item runs it) | none |
| GitLab: Create Access Token | opens the token page (see Setup) | none |

Check Out Branch fetches the source branch from the gitlab.com remote and
checks it out, creating a tracking branch if needed. An existing local branch
is fast-forwarded, or left alone if it has diverged. Fork merge requests are
fetched from `refs/merge-requests/<iid>/head` into `fork/<author>/<branch>`.

## Settings

Settings › Plugins › GitLab:

| Key | Default | Effect |
|---|---|---|
| `editorReview` | `true` | show the current branch's unresolved threads in the editor |
| `cloneUsingSsh` | `false` | clone and share with the SSH URL instead of HTTPS |
| `cloneDirectory` | empty | where clones go; empty means next to the current project |

## File icons

The GitLab icon on `.gitlab-ci.yml`, `.gitlab-ci.yaml`, `*.gitlab-ci.yml`,
`*.gitlab-ci.yaml`, `*.gitlab-ci.*.yml` and `*.gitlab-ci.*.yaml`.

## Limits

- gitlab.com only, with one personal access token; no OAuth login and no
  multiple accounts.
- Lists are read page by page up to 10 pages: 500 merge requests, 1,000 threads
  per merge request, 1,000 projects, 1,000 diff files.
- No side-by-side diff in the IDE's diff viewer, no per-commit diffs, and no
  commenting from the editor gutter.
- Line comments, replies, resolving and Viewed in the Changes view need
  scripts, which the desktop IDE currently blocks in plugin pages; comment from
  the Merge Request view or on GitLab instead.
- Threads in the editor don't move with unsaved edits.
- Notes are plain text: no markdown rendering, `@mention` completion or
  attachments.
- Filtering is limited to the five quick filters; no search by author, label
  or text.
- Create Merge Request, Share Project and Create Snippet have no title or
  description fields.
- Share Project doesn't run `git init` or commit files.
- A file counts as viewed until a later merge request commit touches it; that
  needs the commits in the local checkout, else the head commit is used.
- Not ported: worktrees, usage statistics. Applying a suggestion doesn't mark it as applied on GitLab.

See the `aritti.gitlab` section of `LIMITATIONS.md` at the repository root
for the missing APIs behind each limit.

## Credits

Port of the GitLab plugin by JetBrains s.r.o. and contributors
(`org.jetbrains.plugins.gitlab` 263.7307.200, intellij-community tag
`idea/263.7307.200`, `plugins/gitlab`). Apache-2.0, see `LICENSE` and `NOTICE`.

```sh
cargo test
openidea-plugin build
openidea-plugin link
```
