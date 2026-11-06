//! The merge request's changes as one diff page (an `html` view body): threads
//! and pending review comments on their lines, new comments on any line,
//! replies, resolving, and files marked as viewed.

use std::collections::BTreeMap;

use openidea_plugin::Error;
use openidea_plugin::host::{self, storage};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent};
use serde_json::Value;

use crate::api::{self, Commit, DiffRefs, Discussion, DraftNote, FileDiff};
use crate::branch::git_ok;
use crate::{CHANGES, DETAILS, FILES, GitLab, LIST, first_line, gitlab_project, safe};

#[derive(Debug, PartialEq)]
pub struct Line {
    /// `+`, `-`, ` ` or `@` (a hunk header).
    pub kind: char,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

fn hunk_start(header: &str, side: char) -> Option<u32> {
    let range = header.split(' ').find_map(|part| part.strip_prefix(side))?;

    range.split(',').next()?.parse().ok()
}

/// A unified diff (GitLab's `diff` field) with each line's old/new number.
pub fn parse_diff(diff: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let (mut old, mut new) = (0, 0);

    for raw in diff.lines() {
        let (kind, text) = raw.split_at(raw.chars().next().map_or(0, char::len_utf8));
        let kind = kind.chars().next().unwrap_or(' ');

        match kind {
            '@' => {
                old = hunk_start(raw, '-').unwrap_or(1);
                new = hunk_start(raw, '+').unwrap_or(1);
                lines.push(Line { kind, old: None, new: None, text: raw.to_owned() });
            }
            '+' => {
                lines.push(Line { kind, old: None, new: Some(new), text: text.to_owned() });
                new += 1;
            }
            '-' => {
                lines.push(Line { kind, old: Some(old), new: None, text: text.to_owned() });
                old += 1;
            }
            '\\' => {}
            _ => {
                lines.push(Line { kind: ' ', old: Some(old), new: Some(new), text: text.to_owned() });
                old += 1;
                new += 1;
            }
        }
    }

    lines
}

pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn number(n: Option<u32>) -> String {
    n.map(|n| n.to_string()).unwrap_or_default()
}

fn viewed_key(project: &str, iid: u64) -> String {
    format!("viewed:{project}:{iid}")
}

/// The newest commit touching each path in `git log --name-only --format=%x01%H`.
fn parse_last_commits(log: &str) -> BTreeMap<String, String> {
    let mut commits = BTreeMap::new();
    let mut sha = "";

    for line in log.lines() {
        match line.strip_prefix('\u{1}') {
            Some(next) => sha = next,
            None if !line.is_empty() => {
                commits.entry(line.to_owned()).or_insert_with(|| sha.to_owned());
            }
            None => {}
        }
    }

    commits
}

/// Last commit of the merge request touching each file, from the local history;
/// empty when the project has no git access or lacks the commits (files then count against the head).
fn last_commits(ctx: &openidea_plugin::Invocation, refs: Option<&DiffRefs>) -> BTreeMap<String, String> {
    let (Some(project), Some(refs)) = (ctx.project(), refs) else {
        return BTreeMap::new();
    };

    if ![&refs.base_sha, &refs.head_sha].into_iter().all(|sha| safe::hex_sha(sha).is_ok()) {
        return BTreeMap::new();
    }

    let range = format!("{}..{}", refs.base_sha, refs.head_sha);
    let args = ["-c", "core.quotepath=off", "log", "--name-only", "--no-renames", "--format=%x01%H", "-n", "1000", &range];

    git_ok(&project, &args).map(|log| parse_last_commits(&log)).unwrap_or_default()
}

/// What a file's viewed mark is compared with: its last commit, else the head.
fn version<'a>(commits: &'a BTreeMap<String, String>, path: &str, head: &'a str) -> &'a str {
    commits.get(path).map(String::as_str).unwrap_or(head)
}

/// Files marked viewed, with the commit they were viewed at.
fn viewed(project: &str, iid: u64) -> BTreeMap<String, String> {
    let saved = storage::get(&viewed_key(project, iid)).ok().flatten().unwrap_or_default();

    serde_json::from_str(&saved).unwrap_or_default()
}

fn thread_html(d: &Discussion, drafts: &[&DraftNote]) -> String {
    let resolved = d.resolvable() && d.resolved();
    let mut html = match resolved {
        true => String::from("<div class=\"thread resolved\"><span class=\"resolved-tag\">Resolved</span>"),
        false => String::from("<div class=\"thread\">"),
    };

    for n in &d.notes {
        html.push_str(&format!("<div class=\"note\"><b>{}</b> <pre>{}</pre></div>", escape(&n.author.username), escape(&n.body)));
    }

    for draft in drafts {
        html.push_str(&format!("<div class=\"note draft\"><b>Pending</b> <pre>{}</pre></div>", escape(&draft.note)));
    }

    let id = escape(&d.id);
    html.push_str(&format!("<div class=\"actions\"><button data-reply=\"{id}\">Reply</button>"));
    if d.resolvable() {
        let label = if resolved { "Unresolve" } else { "Resolve" };
        html.push_str(&format!(" <button data-resolve=\"{id}\" data-resolved=\"{}\">{label}</button>", !resolved));
    }

    html.push_str("</div></div>");
    html
}

pub struct Page<'a> {
    pub iid: u64,
    pub head_sha: &'a str,
    pub last_commits: &'a BTreeMap<String, String>,
    pub diffs: &'a [FileDiff],
    pub threads: &'a [Discussion],
    pub drafts: &'a [DraftNote],
    pub viewed: &'a BTreeMap<String, String>,
    pub anchor: Option<&'a str>,
    /// The short id of a single commit shown read-only: no viewed marks, no new comments.
    pub commit: Option<&'a str>,
}

fn summary(iid: u64, files: usize) -> String {
    match files {
        0 => format!("!{iid} · No changes"),
        1 => format!("!{iid} · 1 file changed · click a line number to comment"),
        n => format!("!{iid} · {n} files changed · click a line number to comment"),
    }
}

fn commit_summary(iid: u64, commit: &str, files: usize) -> String {
    let files = match files {
        1 => "1 file changed".to_owned(),
        n => format!("{n} files changed"),
    };

    format!("Commit {} of !{iid} · {files} · read-only", escape(commit))
}

pub fn render_html(page: &Page) -> String {
    let read_only = if page.commit.is_some() { " data-read-only" } else { "" };
    let mut html = format!("<style>{THEME_DEFAULTS}{CONTROLS}{STYLE}</style><body data-anchor=\"{}\"{read_only}>", escape(page.anchor.unwrap_or("")));
    let summary = match page.commit {
        Some(commit) => commit_summary(page.iid, commit, page.diffs.len()),
        None => summary(page.iid, page.diffs.len()),
    };
    html.push_str(&format!("<p class=\"summary\">{summary}</p><p id=\"error\" class=\"error\" role=\"alert\" hidden></p>"));
    if page.commit.is_none() {
        html.push_str(NO_SCRIPTS);
    }

    for (index, file) in page.diffs.iter().enumerate() {
        let is_viewed = page.commit.is_none() && page.viewed.get(&file.new_path).is_some_and(|sha| sha == version(page.last_commits, &file.new_path, page.head_sha));
        let lines = parse_diff(&file.diff);
        let added = lines.iter().filter(|l| l.kind == '+').count();
        let removed = lines.iter().filter(|l| l.kind == '-').count();
        let name = match file.renamed_file {
            true => format!("{} → {}", file.old_path, file.new_path),
            false => file.new_path.clone(),
        };
        let status = match (file.new_file, file.deleted_file) {
            (true, _) => " (new)",
            (_, true) => " (deleted)",
            _ => "",
        };

        html.push_str(&format!(
            "<div class=\"file{}\" id=\"f{index}\" data-old-path=\"{}\" data-new-path=\"{}\">\
             <div class=\"header\" tabindex=\"0\">{}<a href=\"#\" class=\"open\" data-line=\"1\">{}</a>{status} <span class=\"stat\">+{added} −{removed}</span></div><table>",
            if is_viewed { " collapsed" } else { "" },
            escape(&file.old_path),
            escape(&file.new_path),
            match (page.commit, is_viewed) {
                (Some(_), _) => "",
                (None, true) => "<label><input type=\"checkbox\" class=\"viewed\" checked> Viewed</label> ",
                (None, false) => "<label><input type=\"checkbox\" class=\"viewed\"> Viewed</label> ",
            },
            escape(&name),
        ));

        if file.diff.is_empty() {
            html.push_str("<tr><td colspan=\"3\" class=\"empty\">No diff to show (binary or too large). Open the file on GitLab.</td></tr>");
        }

        for line in &lines {
            let class = match line.kind {
                '+' => "add",
                '-' => "del",
                '@' => "hunk",
                _ => "ctx",
            };
            let row_id = format!("f{index}-{}-{}", number(line.old), number(line.new));
            let focusable = if line.kind == '@' { "" } else { " tabindex=\"0\"" };
            html.push_str(&format!(
                "<tr class=\"{class}\" id=\"{row_id}\" data-old=\"{}\" data-new=\"{}\"><td class=\"ln\"{focusable}>{}</td><td class=\"ln\"{focusable}>{}</td><td class=\"code\">{}</td></tr>",
                number(line.old),
                number(line.new),
                number(line.old),
                number(line.new),
                escape(&line.text),
            ));

            if line.kind == '@' {
                continue;
            }

            let at = |p: &api::NotePosition| p.is_at(&file.old_path, &file.new_path, line.old, line.new);
            for d in page.threads.iter().filter(|d| d.notes[0].position.as_ref().is_some_and(at)) {
                let replies: Vec<&DraftNote> = page.drafts.iter().filter(|n| n.discussion_id.as_deref() == Some(&d.id)).collect();
                html.push_str(&format!("<tr class=\"comments\"><td colspan=\"3\">{}</td></tr>", thread_html(d, &replies)));
            }

            for draft in page.drafts.iter().filter(|n| n.discussion_id.is_none() && n.position.as_ref().is_some_and(at)) {
                html.push_str(&format!(
                    "<tr class=\"comments\"><td colspan=\"3\"><div class=\"thread\"><div class=\"note draft\"><b>Pending</b> <pre>{}</pre></div></div></td></tr>",
                    escape(&draft.note)
                ));
            }
        }

        html.push_str("</table></div>");
    }

    html.push_str(&format!("<script>{SCRIPT}</script></body>"));
    html
}

/// Defaults for the IDE's theme variables, which the frame's script would set; the desktop IDE blocks
/// that script (LIMITATIONS.md). System colours follow light/dark; the IDE's values win when the script runs.
const THEME_DEFAULTS: &str = ":root{--openidea-bg:Canvas;--openidea-bg-elevated:color-mix(in srgb,CanvasText 6%,Canvas);--openidea-bg-hover:color-mix(in srgb,CanvasText 12%,Canvas);--openidea-bg-selected:color-mix(in srgb,Highlight 35%,Canvas);--openidea-bg-input:Field;--openidea-fg:CanvasText;--openidea-fg-strong:CanvasText;--openidea-fg-muted:GrayText;--openidea-border:color-mix(in srgb,CanvasText 25%,Canvas);--openidea-focus-ring:Highlight;--openidea-link:LinkText;--openidea-accent:Highlight;--openidea-fg-on-accent:HighlightText;--openidea-danger:color-mix(in srgb,#db5c5c 75%,CanvasText);--openidea-warning:color-mix(in srgb,#d6ae58 75%,CanvasText);--openidea-success:color-mix(in srgb,#5fad65 75%,CanvasText);--openidea-info:color-mix(in srgb,#548af7 75%,CanvasText);--openidea-font-ui:system-ui,sans-serif;--openidea-font-mono:ui-monospace,Menlo,monospace;--openidea-font-size:13px}";

const STYLE: &str = r#"
body { margin: 0; padding: 8px; }
.summary { color: var(--openidea-fg-muted); margin: 0 0 8px; }
.file { border: 1px solid var(--openidea-border); border-radius: 4px; margin-bottom: 12px; }
.header { padding: 4px 8px; background: var(--openidea-bg-elevated); border-bottom: 1px solid var(--openidea-border); cursor: pointer; }
.header a { color: var(--openidea-link); }
.stat { color: var(--openidea-fg-muted); float: right; }
.js .collapsed table, .js .nojs { display: none; }
.nojs { color: var(--openidea-warning); margin: 0 0 8px; }
table { border-collapse: collapse; width: 100%; font-family: var(--openidea-font-mono); }
td { padding: 0 6px; vertical-align: top; }
.ln { width: 1%; color: var(--openidea-fg-muted); text-align: right; user-select: none; }
tr:not(.hunk) .ln { cursor: pointer; }
tr:not(.hunk) .ln:hover { color: var(--openidea-accent); }
.ln:focus-visible, .header:focus-visible { outline: 2px solid var(--openidea-focus-ring); outline-offset: -2px; }
.code { white-space: pre-wrap; word-break: break-all; }
.add { background: color-mix(in srgb, var(--openidea-success) 15%, transparent); }
.del { background: color-mix(in srgb, var(--openidea-danger) 15%, transparent); }
.hunk { color: var(--openidea-info); background: var(--openidea-bg-elevated); }
.comments td { font-family: var(--openidea-font-ui); padding: 4px 8px; }
.thread { border: 1px solid var(--openidea-border); border-radius: 4px; padding: 4px 8px; background: var(--openidea-bg); }
.note pre { white-space: pre-wrap; font-family: var(--openidea-font-ui); margin: 2px 0 6px; }
.draft b { color: var(--openidea-warning); }
.resolved { opacity: 0.7; }
.resolved-tag { color: var(--openidea-success); font-weight: 600; }
.empty { color: var(--openidea-fg-muted); padding: 6px; }
"#;

/// Buttons, text areas and errors, shared by the HTML pages.
pub const CONTROLS: &str = r#"
body { font-family: var(--openidea-font-ui); font-size: var(--openidea-font-size); color: var(--openidea-fg); background: var(--openidea-bg); }
textarea { width: 100%; min-height: 4em; box-sizing: border-box; font: inherit; background: var(--openidea-bg-input); color: var(--openidea-fg); border: 1px solid var(--openidea-border); border-radius: 4px; padding: 4px; }
button { margin: 2px 4px 2px 0; padding: 2px 10px; font: inherit; background: var(--openidea-bg-input); color: var(--openidea-fg); border: 1px solid var(--openidea-border); border-radius: 4px; cursor: pointer; }
button:hover { background: var(--openidea-bg-hover); }
button.primary { background: var(--openidea-accent); color: var(--openidea-fg-on-accent); border-color: var(--openidea-accent); }
button.primary:hover { background: var(--openidea-accent); filter: brightness(1.1); }
button:disabled { opacity: 0.5; cursor: default; }
textarea:focus-visible, button:focus-visible, input:focus-visible { outline: 2px solid var(--openidea-focus-ring); outline-offset: 1px; }
.error { color: var(--openidea-danger); margin: 4px 0; }
"#;

/// Shown while the page's script hasn't run: some IDE builds block scripts in plugin pages.
const NO_SCRIPTS: &str = "<p class=\"nojs\" role=\"note\">Commenting on lines doesn't work in this panel: this version of the IDE blocks scripts in plugin pages. \
                          Use Comment in the Merge Request view, or open the merge request on GitLab.</p>";

const SCRIPT: &str = r#"
document.documentElement.classList.add("js");
const send = (message) => openidea.postMessage(JSON.stringify(message));
const fileOf = (el) => el.closest('.file');
const anchorOf = (tr) => { while (tr && !tr.id) tr = tr.previousElementSibling; return tr ? tr.id : ''; };
const lineOf = (tr) => ({ old: tr.dataset.old ? +tr.dataset.old : null, new: tr.dataset.new ? +tr.dataset.new : null });

function editor(afterRow, onSubmit) {
  document.querySelectorAll('tr.editor').forEach((r) => r.remove());
  const row = document.createElement('tr');
  row.className = 'editor comments';
  row.innerHTML = '<td colspan="3"><textarea aria-label="Comment"></textarea><div class="error" role="alert" hidden></div>'
    + '<button class="primary" data-draft="1">Add to Review</button><button data-draft="0">Comment Now</button><button data-cancel>Cancel</button></td>';
  afterRow.after(row);
  const text = row.querySelector('textarea');
  text.focus();
  row.addEventListener('click', (e) => {
    if (e.target.dataset.cancel !== undefined) { row.remove(); return; }
    if (e.target.dataset.draft === undefined || !text.value.trim()) return;
    row.querySelectorAll('button').forEach((b) => { b.disabled = true; });
    onSubmit(text.value, e.target.dataset.draft === '1', anchorOf(row.previousElementSibling));
  });
}

// A failed save leaves the page as it is (the plugin doesn't redraw it), so the typed text stays.
openidea.onMessage((data) => {
  const message = typeof data === 'string' ? JSON.parse(data) : data;
  if (message.type !== 'error') return;
  const target = document.querySelector('tr.editor .error') || document.getElementById('error');
  target.textContent = message.message;
  target.hidden = false;
  document.querySelectorAll('button:disabled').forEach((b) => { b.disabled = false; });
});

document.addEventListener('keydown', (e) => {
  if (e.key !== 'Enter' && e.key !== ' ') return;
  const t = e.target;
  if (!t.classList.contains('ln') && !t.classList.contains('header')) return;
  e.preventDefault();
  t.click();
});

document.addEventListener('click', (e) => {
  const t = e.target;
  if (t.classList.contains('ln')) {
    const tr = t.closest('tr');
    if (tr.classList.contains('hunk') || document.body.dataset.readOnly !== undefined) return;
    const file = fileOf(tr);
    editor(tr, (body, draft, anchor) => send({ type: 'comment', oldPath: file.dataset.oldPath, newPath: file.dataset.newPath, ...lineOf(tr), body, draft, anchor }));
  } else if (t.dataset.reply) {
    editor(t.closest('tr'), (body, draft, anchor) => send({ type: 'reply', discussion: t.dataset.reply, body, draft, anchor }));
  } else if (t.dataset.resolve) {
    t.disabled = true;
    send({ type: 'resolve', discussion: t.dataset.resolve, resolved: t.dataset.resolved === 'true', anchor: anchorOf(t.closest('tr')) });
  } else if (t.classList.contains('open')) {
    e.preventDefault();
    const file = fileOf(t);
    send({ type: 'open', path: file.dataset.newPath, line: +t.dataset.line });
  } else if (t.classList.contains('viewed')) {
    const file = fileOf(t);
    file.classList.toggle('collapsed', t.checked);
    send({ type: 'viewed', path: file.dataset.newPath, viewed: t.checked });
  } else if (t.closest('.header') && !t.closest('label')) {
    fileOf(t).classList.toggle('collapsed');
  }
});

const anchor = document.body.dataset.anchor && document.getElementById(document.body.dataset.anchor);
if (anchor) anchor.scrollIntoView({ block: 'center' });
"#;

fn set_viewed(project: &str, iid: u64, path: &str, sha: Option<String>) -> Result<(), Error> {
    let mut marks = viewed(project, iid);
    match sha {
        Some(sha) => marks.insert(path.to_owned(), sha),
        None => marks.remove(path),
    };
    let json = serde_json::to_string(&marks).map_err(|e| Error::Failed(e.to_string()))?;

    storage::set(&viewed_key(project, iid), &json)
}

const LAYOUT_KEY: &str = "files-layout";

fn files_as_list() -> bool {
    storage::get(LAYOUT_KEY).ok().flatten().as_deref() == Some("list")
}

/// A list of full paths, or the directories as parents of their files.
fn file_items(files: &[FileDiff], is_viewed: impl Fn(&str) -> bool, as_list: bool) -> Vec<TreeItem> {
    let mut items: Vec<TreeItem> = Vec::new();
    let mut dirs: BTreeMap<String, ()> = BTreeMap::new();

    for file in files {
        let lines = parse_diff(&file.diff);
        let detail = format!("+{} -{}", lines.iter().filter(|l| l.kind == '+').count(), lines.iter().filter(|l| l.kind == '-').count());
        let path = file.new_path.as_str();
        let (parent, label) = match (as_list, path.rsplit_once('/')) {
            (false, Some((dir, name))) => (Some(dir), name),
            _ => (None, path),
        };

        if let Some(dir) = parent {
            let mut prefix = String::new();
            let mut grand: Option<String> = None;
            for segment in dir.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(segment);
                if dirs.insert(prefix.clone(), ()).is_none() {
                    let mut item = TreeItem::new(&format!("dir:{prefix}"), segment).expandable(true);
                    if let Some(grand) = &grand {
                        item = item.parent(&format!("dir:{grand}"));
                    }
                    items.push(item);
                }
                grand = Some(prefix.clone());
            }
        }

        let mut item = TreeItem::new(&format!("file:{path}"), label).detail(&detail);
        if let Some(dir) = parent {
            item = item.parent(&format!("dir:{dir}"));
        }
        if is_viewed(path) {
            item = item.icon("check");
        }
        items.push(item);
    }

    items
}

impl GitLab {
    fn ensure_diffs(&mut self, project: &str, iid: u64) -> Result<(), Error> {
        if self.diffs.as_ref().map(|(i, _)| *i) != Some(iid) {
            let _task = host::begin_task("Loading changes", false);
            self.diffs = Some((iid, api::Project(project).diffs(iid)?));
        }

        Ok(())
    }

    pub(crate) fn render_files(&mut self, ctx: &openidea_plugin::Invocation) -> Result<View, Error> {
        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the Merge Requests tool window."));
        };

        let project = gitlab_project(ctx)?.path;
        self.ensure_details(&project, iid)?;
        self.ensure_diffs(&project, iid)?;
        let details = self.details.as_ref().ok_or("The merge request is not loaded; refresh it.")?;
        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let files = self.diffs.as_ref().map(|(_, d)| d.as_slice()).unwrap_or_default();
        if files.is_empty() {
            return Ok(View::message(&format!("!{iid} has no changed files.")).button(refresh));
        }

        let head = details.mr.diff_refs.as_ref().map(|r| r.head_sha.as_str()).unwrap_or("");
        let commits = last_commits(ctx, details.mr.diff_refs.as_ref());
        let marks = viewed(&project, iid);
        let is_viewed = |path: &str| marks.get(path).is_some_and(|sha| sha == version(&commits, path, head));
        let done = files.iter().filter(|f| is_viewed(&f.new_path)).count();
        let as_list = files_as_list();
        let layout = match as_list {
            true => Button::new("layout", "Tree").icon("module"),
            false => Button::new("layout", "List").icon("module"),
        };

        Ok(View::new(Body::Tree(file_items(files, is_viewed, as_list)))
            .button(layout)
            .button(Button::new("mark-all", "Mark All Viewed").icon("check"))
            .button(Button::new("clear", "Clear Viewed").icon("undo"))
            .button(refresh)
            .status(&format!("!{iid} · {done} of {} files viewed · double-click a file to toggle", files.len())))
    }

    pub(crate) fn handle_files(&mut self, event: ViewEvent, ctx: &openidea_plugin::Invocation) -> Result<(), Error> {
        let Some(iid) = Self::selected() else { return Ok(()) };
        let project = gitlab_project(ctx)?.path;
        self.ensure_details(&project, iid)?;
        let head = self.details.as_ref().and_then(|d| d.mr.diff_refs.clone());
        let commits = || last_commits(ctx, head.as_ref());
        let head_sha = head.as_ref().map(|r| r.head_sha.clone()).unwrap_or_default();
        let listed: Vec<String> = self.diffs.as_ref().filter(|(i, _)| *i == iid).map(|(_, d)| d.iter().map(|f| f.new_path.clone()).collect()).unwrap_or_default();

        match event {
            ViewEvent::Activate(key) => {
                let Some(path) = key.strip_prefix("file:").filter(|p| listed.iter().any(|l| l == p)) else { return Ok(()) };
                let sha = version(&commits(), path, &head_sha).to_owned();
                let on = viewed(&project, iid).get(path) != Some(&sha);
                set_viewed(&project, iid, path, on.then_some(sha))?;
            }
            ViewEvent::Button(id) if id == "layout" => {
                storage::set(LAYOUT_KEY, if files_as_list() { "tree" } else { "list" })?;
            }
            ViewEvent::Button(id) if id == "mark-all" => {
                let commits = commits();
                let marks: BTreeMap<String, String> = listed.iter().map(|p| (p.clone(), version(&commits, p, &head_sha).to_owned())).collect();
                storage::set(&viewed_key(&project, iid), &serde_json::to_string(&marks).map_err(|e| Error::Failed(e.to_string()))?)?;
            }
            ViewEvent::Button(id) if id == "clear" => storage::set(&viewed_key(&project, iid), "{}")?,
            ViewEvent::Button(id) if id == "refresh" => {
                self.diffs = None;
                self.reload_merge_request(iid);

                return Ok(());
            }
            _ => return Ok(()),
        }

        host::refresh_view(FILES);
        host::refresh_view(CHANGES);

        Ok(())
    }

    pub(crate) fn render_changes(&mut self, ctx: &openidea_plugin::Invocation) -> Result<View, Error> {
        let Some(iid) = Self::selected() else {
            return Ok(View::message("Select a merge request in the Merge Requests tool window."));
        };

        let project = gitlab_project(ctx)?.path;
        self.ensure_details(&project, iid)?;
        self.ensure_diffs(&project, iid)?;

        if let Some(commit) = self.active_commit().cloned() {
            return self.render_commit_changes(&project, iid, &commit);
        }

        let details = self.details.as_ref().ok_or("The merge request is not loaded; refresh it.")?;
        let threads: Vec<Discussion> = self.threads.get(&iid).into_iter().flatten().filter(|d| !d.is_system()).cloned().collect();
        let head_sha = details.mr.diff_refs.as_ref().map(|r| r.head_sha.as_str()).unwrap_or("");
        let viewed = viewed(&project, iid);
        let last_commits = last_commits(ctx, details.mr.diff_refs.as_ref());
        let page = Page {
            iid,
            head_sha,
            last_commits: &last_commits,
            diffs: self.diffs.as_ref().map(|(_, d)| d.as_slice()).unwrap_or_default(),
            threads: &threads,
            drafts: &details.drafts,
            viewed: &viewed,
            anchor: self.anchor.as_deref(),
            commit: None,
        };

        let mut view = View::new(Body::Html(render_html(&page))).button(Button::new("refresh", "Refresh").icon("refresh"));
        if !details.drafts.is_empty() {
            view = view.button(Button::new("submit", &format!("Submit Review ({})", details.drafts.len())).icon("send"));
        }

        view = view.button(Button::new("browser", "Open on GitLab").icon("external-link"));
        Ok(match &self.status {
            Some(status) => view.status(status),
            None => view,
        })
    }

    /// One commit's diff, read-only: its threads need the merge request's diff refs, so none are shown.
    fn render_commit_changes(&mut self, project: &str, iid: u64, commit: &Commit) -> Result<View, Error> {
        if self.commit_diff.as_ref().map(|(sha, _)| sha) != Some(&commit.id) {
            let _task = host::begin_task("Loading commit", false);
            self.commit_diff = Some((commit.id.clone(), api::Project(project).commit_diff(&commit.id)?));
        }

        let none = BTreeMap::new();
        let page = Page {
            iid,
            head_sha: "",
            last_commits: &none,
            diffs: self.commit_diff.as_ref().map(|(_, d)| d.as_slice()).unwrap_or_default(),
            threads: &[],
            drafts: &[],
            viewed: &none,
            anchor: None,
            commit: Some(&commit.short_id),
        };

        Ok(View::new(Body::Html(render_html(&page)))
            .button(Button::new("refresh", "Refresh").icon("refresh"))
            .button(Button::new("all-changes", "All Changes").icon("arrow-left"))
            .button(Button::new("browser", "Open on GitLab").icon("external-link"))
            .status(&format!("Commit {} {} · read-only", commit.short_id, first_line(&commit.title))))
    }

    pub(crate) fn handle_changes_message(&mut self, message: &str, ctx: &openidea_plugin::Invocation) -> Result<(), Error> {
        let message: Value = serde_json::from_str(message).map_err(|e| Error::Invalid(e.to_string()))?;
        if self.active_commit().is_some() && message["type"] != "open" {
            let error = serde_json::json!({ "type": "error", "message": "A commit's changes are read-only. Click All Changes to comment." });
            return host::post_view_message(CHANGES, &error.to_string());
        }

        let repo = gitlab_project(ctx)?;
        let (status, project) = (repo.status, repo.path);
        if let Some(iid) = Self::selected() {
            self.ensure_details(&project, iid)?;
        }

        let details = self.details.as_ref().ok_or("Select a merge request first.")?;
        let iid = details.mr.iid;
        let text = |key: &str| message[key].as_str().unwrap_or("").to_owned();
        let line = |key: &str| message[key].as_u64().map(|n| n as u32);

        match message["type"].as_str().unwrap_or("") {
            "open" => {
                let path = format!("{}/{}", status.root, text("path"));
                return host::open_location(&path, line("line").unwrap_or(1).saturating_sub(1), 0);
            }
            "viewed" => {
                let head = details.mr.diff_refs.as_ref().map(|r| r.head_sha.clone()).unwrap_or_default();
                let sha = (message["viewed"].as_bool() == Some(true)).then(|| {
                    let commits = last_commits(ctx, details.mr.diff_refs.as_ref());
                    version(&commits, &text("path"), &head).to_owned()
                });
                set_viewed(&project, iid, &text("path"), sha)?;

                return Ok(host::refresh_view(FILES));
            }
            _ => {}
        }

        let api = api::Project(&project);
        let draft = message["draft"].as_bool() == Some(true);
        let _task = host::begin_task("Updating merge request", false);
        let result = match message["type"].as_str().unwrap_or("") {
            "comment" => {
                let refs = details.mr.diff_refs.as_ref().ok_or("GitLab sent no diff refs for this merge request.")?;
                let position = api::diff_position(refs, &text("oldPath"), &text("newPath"), line("old"), line("new"));
                match draft {
                    true => api.create_draft(iid, &text("body"), Some(position), None),
                    false => api.discuss(iid, &text("body"), Some(position)),
                }
            }
            "reply" => match draft {
                true => api.create_draft(iid, &text("body"), None, Some(&text("discussion"))),
                false => api.reply(iid, &text("discussion"), &text("body")),
            },
            "resolve" => api.resolve(iid, &text("discussion"), message["resolved"].as_bool() == Some(true)),
            other => Err(Error::Invalid(format!("message {other}"))),
        };

        if let Err(e) = result {
            // Redrawing would reload the page and lose the open editor's text.
            return host::post_view_message(CHANGES, &serde_json::json!({ "type": "error", "message": e.to_string() }).to_string());
        }

        self.status = Some(match (draft, message["type"].as_str(), message["resolved"].as_bool()) {
            (true, ..) => "Added to your review.",
            (false, Some("resolve"), Some(true)) => "Thread resolved.",
            (false, Some("resolve"), _) => "Thread unresolved.",
            (false, ..) => "Comment posted.",
        }
        .into());
        self.anchor = Some(text("anchor")).filter(|a| !a.is_empty());
        self.reload_merge_request(iid);

        Ok(())
    }

    /// Forgets the loaded merge request and redraws everything that shows it.
    pub(crate) fn reload_merge_request(&mut self, iid: u64) {
        self.details = None;
        self.threads.remove(&iid);
        self.events.remove(&iid);
        host::refresh_view(DETAILS);
        host::refresh_view(CHANGES);
        host::refresh_view(LIST);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_diff_lines() {
        let lines = parse_diff("@@ -3,2 +3,3 @@ fn x\n a\n-b\n+c\n+d\n\\ No newline at end of file\n");

        let numbers: Vec<(char, Option<u32>, Option<u32>)> = lines.iter().map(|l| (l.kind, l.old, l.new)).collect();
        assert_eq!(numbers, [('@', None, None), (' ', Some(3), Some(3)), ('-', Some(4), None), ('+', None, Some(4)), ('+', None, Some(5))]);
        assert_eq!(lines[2].text, "b");
    }

    #[test]
    fn summarises_the_file_count() {
        assert_eq!(summary(3, 0), "!3 · No changes");
        assert_eq!(summary(3, 1), "!3 · 1 file changed · click a line number to comment");
        assert_eq!(summary(3, 2), "!3 · 2 files changed · click a line number to comment");
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape("<a href=\"x\">&</a>"), "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;");
    }
}
