use check::{Dialect, check, dialect};
use matcher::{Rule, Sources, ignored_by};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::syntax::{SyntaxProvider, Token};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, FileKind, Invocation, Location, NotificationLevel, PickItem, Position, Project, Range, Severity, StatusItem,
    StatusMenuEntry, TextEdit, host, text,
};
use std::collections::HashSet;

mod actions;
mod unused;
mod check;
mod decorations;
mod git_inventory;
mod file_hints;
mod highlight;
mod matcher;
mod preferences;
mod strict_json;
mod templates {
    include!(concat!(env!("OUT_DIR"), "/templates.rs"));
}
use templates::TEMPLATES;

const IGNORED_STATUS: &str = "ignored";
const USER_TEMPLATE: &str = "template:";
const STARRED_PREFIX: &str = "starred:";

fn range(text: &str, bytes: &std::ops::Range<usize>) -> Range {
    Range::new(text::position(text, bytes.start), text::position(text, bytes.end))
}

fn diagnostics(text: &str, problems: &[check::Problem]) -> Vec<Diagnostic> {
    problems
        .iter()
        .map(|p| {
            let severity = if p.warning { Severity::Warning } else { Severity::Error };
            let mut diagnostic = Diagnostic::new(range(text, &(p.start..p.end)), severity, p.message.clone());
            diagnostic.code = Some(p.code.into());
            diagnostic
        })
        .collect()
}

fn quick_fixes(doc: &Document, line: u32, problems: Vec<check::Problem>) -> Vec<CodeAction> {
    problems
        .into_iter()
        .filter(|p| text::position(&doc.text, p.start).line == line)
        .flat_map(|p| p.fixes)
        .map(|(title, bytes, replacement)| CodeAction {
            title: title.into(),
            kind: Some("quickfix".into()),
            preferred: false,
            edits: vec![(doc.path.clone(), vec![TextEdit { range: range(&doc.text, &bytes), text: replacement }])],
            data: None,
        })
        .collect()
}

fn line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Inserts `addition` (whole lines) after the last line of `content`, with the
/// line ending the file already uses.
fn append(content: &str, addition: &str, blank_line: bool) -> TextEdit {
    let end = text::position(content, content.len());
    let newline = line_ending(content);
    let mut prefix = String::new();
    if !content.is_empty() && !content.ends_with('\n') {
        prefix.push_str(newline);
    }
    if !content.is_empty() && blank_line {
        prefix.push_str(newline);
    }
    let addition = addition.replace("\r\n", "\n").replace('\n', newline);
    TextEdit { range: Range::new(end, end), text: format!("{prefix}{addition}") }
}

/// The open project holding `path`, its root and `path` relative to it.
fn project_of(path: &str) -> Option<(Project, String, String)> {
    let project = host::project_of(path)?;
    let root = project.root().ok()?.trim_end_matches('/').to_owned();
    let relative = path.strip_prefix(&format!("{root}/"))?.to_owned();
    Some((project, root, relative))
}

fn literal_entry(relative: &str) -> Result<String, Error> {
    if relative.is_empty() || relative.chars().any(char::is_control) || relative.split('/').any(|p| matches!(p, "." | ".." | "")) {
        return Err("The selected path cannot be represented as a safe ignore entry.".into());
    }
    let mut result = String::from("/");
    for c in relative.chars() {
        if matches!(c, '\\' | '*' | '?' | '[' | ']' | ' ') {
            result.push('\\')
        }
        result.push(c)
    }
    Ok(result)
}

const ROOT_IGNORE_FILES: [&str; 5] = [".gitignore", ".dockerignore", ".npmignore", ".prettierignore", ".eslintignore"];

/// The focused ignore file and the ignore files at the project root; asks which one when there are several.
fn pick_target(ctx: &Invocation, focused: Option<&str>, unignore: bool) -> Result<Option<String>, Error> {
    let mut candidates: Vec<String> = focused.filter(|path|preferences::can_edit(path)).map(str::to_owned).into_iter().collect();

    if let Some(project) = ctx.project() {
        let root = project.root()?;
        for name in ROOT_IGNORE_FILES {
            let path = format!("{root}/{name}");
            if preferences::can_edit(&path) && !candidates.contains(&path) && project.read_file(&path).is_ok() {
                candidates.push(path);
            }
        }
    }

    match candidates.len() {
        0 => Err("Open the ignore file to add the selected paths to.".into()),
        1 => Ok(candidates.pop()),
        _ => {
            let items: Vec<PickItem> = candidates.iter().map(|path| PickItem::new(path, path.rsplit('/').next().unwrap_or(path))).collect();

            host::pick(if unignore { "Unignore in ignore file" } else { "Add to ignore file" }, &items)
        }
    }
}

fn selected_or_focused(ctx: &Invocation) -> Vec<String> {
    let selected = ctx.selected_paths();
    if !selected.is_empty() {
        return selected;
    }

    ctx.editor().filter(|e| dialect(&e.path()).is_none()).map(|e| e.path()).into_iter().collect()
}

fn add_selected(ctx: &Invocation, unignore: bool) -> Result<(), Error> {
    let selected = selected_or_focused(ctx);
    if selected.is_empty() {
        return Err("Select files or folders in the Project view first.".into());
    }

    let focused = ctx.editor().filter(|e| dialect(&e.path()).is_some());
    let Some(file) = pick_target(ctx, focused.as_ref().map(|e| e.path()).as_deref(), unignore)? else {
        return Ok(());
    };

    let (project, root, _) = project_of(&file).ok_or("The ignore file must belong to the current project.")?;
    if !preferences::can_edit(&file) {return Err("Editor assistance is disabled for this ignore file type.".into());}
    if ctx.project().ok_or(Error::Stale)?.root()?.trim_end_matches('/') != root {
        return Err(Error::Stale);
    }
    if !dialect(&file).is_some_and(|d| d.syntax == check::Syntax::Glob) {
        return Err("Selected-path append supports glob-syntax ignore file types only.".into());
    }
    let editor = focused.filter(|e| e.path() == file);
    let (content, version) = match &editor {
        Some(editor) => {
            let doc = editor.document()?;
            (doc.text, doc.version)
        }
        None => (project.read_file(&file)?, 0),
    };

    let base = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let listed: HashSet<&str> = content.lines().collect();
    let negation = if unignore { "!" } else { "" };
    let mut entries: Vec<String> = Vec::new();
    for path in selected {
        let Some(relative) = path.strip_prefix(base).and_then(|r| r.strip_prefix('/')) else {
            continue;
        };
        let entry = format!("{negation}{}", literal_entry(relative)?);
        if !listed.contains(entry.as_str()) && !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        return Err("The selected paths are already listed or are outside the ignore file's folder.".into());
    }

    let addition: String = entries.iter().map(|e| format!("{e}\n")).collect();
    let edit = append(&content, &addition, false);
    match editor {
        Some(editor) => {
            editor.apply_edits(version, &[edit])?;
        }
        None => {
            project
                .write_file(&file, &format!("{content}{}", edit.text))
                .map_err(|e| ui_kit::denied::map(e, "Adding paths to an ignore file that is not open", "project.write", ".ignore"))?;
            let _ = host::open_location(&file, 0, 0);
        }
    }

    let name = file.rsplit('/').next().unwrap_or(&file);
    host::notify(NotificationLevel::Info, ".ignore", &format!("Added {} to {name}.", entries.len()));
    Ok(())
}

const IGNORE_FILE_NAMES: [&str; 20] = [
    ".gitignore",
    ".dockerignore",
    ".npmignore",
    ".hgignore",
    ".prettierignore",
    ".eslintignore",
    ".stylelintignore",
    ".gcloudignore",
    ".slugignore",
    ".helmignore",
    ".vercelignore",
    ".terraformignore",
    ".yarnignore",
    ".nodemonignore",
    ".jshintignore",
    ".cfignore",
    ".bzrignore",
    ".cvsignore",
    ".p4ignore",
    ".chefignore",
];

fn new_ignore_file(plugin: &mut IgnorePlugin, ctx: &Invocation) -> Result<(), Error> {
    let project = ctx.project().ok_or("Open a project first.")?;
    let selected_dir = match ctx.selected_paths().first() {
        Some(path) if project.stat(path).is_ok_and(|stat| stat.kind == FileKind::Directory) => Some(path.clone()),
        Some(path) => path.rsplit_once('/').map(|(dir, _)| dir.to_owned()),
        None => None,
    };
    if let Some(editor) = ctx.editor().filter(|_| selected_dir.is_none()) {
        if ui_kit::command_file_action::run_titled(plugin, &project, &editor, "Create exact-name ignore file", ".ignore")? {
            return Ok(());
        }
    }

    let names = preferences::creatable_names();
    if names.is_empty() {
        return Err("No enabled Ignore Create actions are available.".into());
    }

    let items: Vec<_> = names.iter().map(|name| PickItem::new(name, name)).collect();
    let Some(name) = host::pick("Ignore file type (exact filename)", &items)? else { return Ok(()) };
    let dir = match selected_dir {
        Some(dir) => dir,
        None => project.root()?,
    };
    let target = format!("{}/{name}", dir.trim_end_matches('/'));
    ui_kit::command_file_action::create_empty(&project, &target, "Create ignore file?", ".ignore")
}

/// The ignore files that apply to `relative`: `.git/info/exclude`, then the
/// `.gitignore` of the root and of every directory down to the file's.
fn ignore_sources(project: &Project, root: &str, relative: &str) -> Sources {
    let mut sources = Sources::new();
    if let Ok(text) = project.read_file(&format!("{root}/.git/info/exclude")) {
        sources.push((String::new(), ".git/info/exclude".into(), text));
    }

    let mut dirs = vec![String::new()];
    let mut parents: Vec<&str> = relative.split('/').collect();
    parents.pop();
    for part in parents {
        let parent = dirs.last().unwrap();
        dirs.push(if parent.is_empty() { part.to_owned() } else { format!("{parent}/{part}") });
    }

    for dir in dirs {
        let name = if dir.is_empty() { ".gitignore".to_owned() } else { format!("{dir}/.gitignore") };
        if let Ok(text) = project.read_file(&format!("{root}/{name}")) {
            sources.push((dir, name, text));
        }
    }
    sources
}

fn setting(key: &str, default: bool) -> bool {
    preferences::boolean(key, default)
}

/// The status bar says when the focused file is ignored; its menu goes to the rule.
fn show_ignored_status(path: Option<&str>) {
    let verdict = path.filter(|_| setting("ignoredEditing", true)).and_then(|path| {
        let (project, root, relative) = project_of(path)?;
        let sources = ignore_sources(&project, &root, &relative);
        let (file, entry) = ignored_by(&sources, &relative)?;
        Some((file.to_owned(), entry.to_owned()))
    });
    let Some((file, entry)) = verdict else {
        host::remove_status_item(IGNORED_STATUS);
        return;
    };
    let item = StatusItem { text: "Ignored".into(), tooltip: Some(format!("This file is ignored by `{entry}` in {file}.")), command: None };
    let menu = [
        StatusMenuEntry::Header(format!("Ignored by {entry} in {file}")),
        StatusMenuEntry::action("Go to Rule", "open-ignoring-rule").with_description(file),
        StatusMenuEntry::action("Dialect Checks and Preferences", "preferences"),
    ];
    if host::set_status_item_menu(IGNORED_STATUS, &item, &menu).is_err() {
        host::set_status_item(IGNORED_STATUS, &item);
    }
}

/// Opens the ignore file at the rule that ignores the focused file.
fn open_ignoring_rule(ctx: &Invocation) -> Result<(), Error> {
    let path = ctx.editor().map(|editor| editor.path()).ok_or("Open the file to check first.")?;
    let (project, root, relative) = project_of(&path).ok_or("This file is outside the open project.")?;
    let sources = ignore_sources(&project, &root, &relative);
    let Some((file, entry)) = ignored_by(&sources, &relative) else {
        let name = relative.rsplit('/').next().unwrap_or(&relative);
        host::notify(NotificationLevel::Info, ".ignore", &format!("No rule in the project's ignore files ignores {name}."));
        return Ok(());
    };
    let text = sources.iter().find(|(_, name, _)| name == file).map_or("", |(_, _, text)| text.as_str());
    let line = text.lines().enumerate().filter(|(_, line)| line.trim() == entry).last().map_or(0, |(index, _)| index);
    host::open_location(&format!("{root}/{file}"), line as u32, 0)
}

/// Once per project: a Git repository without a root .gitignore.
fn warn_missing_gitignore(project: &Project) {
    let Ok(root) = project.root() else { return };
    let key = format!("missing-gitignore-shown:{root}");
    if !setting("missingGitignore", true) || host::storage::get(&key).ok().flatten().is_some() {
        return;
    }
    if project.read_file(&format!("{root}/.git/HEAD")).is_err() || project.read_file(&format!("{root}/.gitignore")).is_ok() {
        return;
    }

    host::notify(
        NotificationLevel::Warning,
        "No .gitignore file in this Git project",
        "Create one with New › .gitignore File, or Git › Add to .gitignore in the Project view.",
    );
    let _ = host::storage::set(&key, "1");
}

/// Ctrl-click on an entry naming one file opens it.
fn entry_target(doc: &Document, at: Position) -> Option<Location> {
    let line = doc.text.lines().nth(at.line as usize)?;
    let rule = Rule::parse(line).filter(|r| r.is_literal() && !r.dir_only)?;
    let dir = doc.path.rsplit_once('/')?.0;
    let path = format!("{dir}/{}", rule.literal_path());
    let (project, _, _) = project_of(&path)?;
    project.read_file(&path).ok()?;
    Some(Location { path, range: Range::default() })
}

/// Entries named from the files below the ignore file's folder. The host
/// skips `.git` and gitignored paths in `find_files`, so those are never offered.
fn path_completions(doc: &Document, offset: usize, line: &str) -> Vec<CompletionItem> {
    let body = line.strip_prefix('!').unwrap_or(line);
    if body.starts_with(|c: char| c.is_whitespace() || c == '#') || body.contains(['*', '?', '[', '{', '\\']) {
        return Vec::new();
    }
    let Some((project, root, relative)) = project_of(&doc.path) else {
        return Vec::new();
    };

    let path = body.strip_prefix('/').unwrap_or(body);
    let (typed_dir, partial) = path.rsplit_once('/').map_or(("", path), |(dir, rest)| (dir, rest));
    if typed_dir.split('/').any(|part| part == "..") {
        return Vec::new();
    }

    let own_dir = relative.rsplit_once('/').map_or("", |(dir, _)| dir);
    let base = [own_dir, typed_dir].into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/");
    let (stem, prefix) =
        if base.is_empty() { (String::new(), format!("{root}/")) } else { (format!("{base}/"), format!("{root}/{base}/")) };
    let mut files = Vec::new();
    for glob in [format!("{stem}{partial}*"), format!("{stem}{partial}*/**")] {
        let Ok(found) = project.find_files(&glob, 500) else {
            return Vec::new();
        };
        files.extend(found);
    }

    let mut names: std::collections::BTreeMap<&str, bool> = std::collections::BTreeMap::new();
    for file in &files {
        let Some(rest) = file.strip_prefix(&prefix) else {
            continue;
        };
        let (name, is_dir) = rest.split_once('/').map_or((rest, false), |(name, _)| (name, true));
        if name.starts_with(partial) {
            *names.entry(name).or_default() |= is_dir;
        }
    }

    let replace = Range::new(text::position(&doc.text, offset - partial.len()), text::position(&doc.text, offset));
    names
        .into_iter()
        .map(|(name, is_dir)| {
            let mut item = CompletionItem::new(
                &if is_dir { format!("{name}/") } else { name.to_owned() },
                if is_dir { CompletionKind::Folder } else { CompletionKind::File },
            );
            item.replace = Some(replace);
            item
        })
        .collect()
}

fn template_label(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('/') {
        Some((group, label)) => (label, Some(group)),
        None => (name, None),
    }
}

fn user_templates() -> Vec<String> {
    let keys = host::storage::keys(USER_TEMPLATE).unwrap_or_default();
    keys.into_iter().filter_map(|k| k.strip_prefix(USER_TEMPLATE).map(str::to_owned)).collect()
}

/// The label and content of `t:<name>` (bundled) or `u:<name>` (the user's).
fn template_content(key: &str) -> Result<(String, String), Error> {
    let missing = || Error::NotFound(format!("template {key}"));
    if let Some(name) = key.strip_prefix("u:") {
        let content = host::storage::get(&format!("{USER_TEMPLATE}{name}"))?.ok_or_else(missing)?;
        return Ok((name.to_owned(), content));
    }
    let name = key.strip_prefix("t:").ok_or_else(missing)?;
    let (_, content) = TEMPLATES.iter().find(|(n, _)| *n == name).ok_or_else(missing)?;
    Ok((template_label(name).0.to_owned(), (*content).to_owned()))
}

fn starred() -> HashSet<String> {
    let keys = host::storage::keys(STARRED_PREFIX).unwrap_or_default();
    keys.into_iter().filter_map(|k| k.strip_prefix(STARRED_PREFIX).map(str::to_owned)).collect()
}

fn template_tree(user: &[String], expanded: &HashSet<String>, starred: &HashSet<String>) -> Vec<TreeItem> {
    let group = |key: &str, label: &str| TreeItem::new(key, label).icon("folder").expandable(expanded.contains(key));
    let mut items = vec![
        group("g:starred", "Starred"),
        group("g:user", "User templates"),
        group("g:", "Languages and frameworks"),
        group("g:Global", "Global templates (OS, editors, tools)"),
        group("g:community", "Community templates"),
    ];
    items.extend(user.iter().map(|name| TreeItem::new(&format!("u:{name}"), name).icon(if starred.contains(&format!("u:{name}")) {"star"} else {"file-ignored"}).parent("g:user")));
    let icon = |key: &str| {
        if starred.contains(key) { "star" } else { "file-ignored" }
    };
    // An expandable item without children shows as loading forever.
    if user.is_empty() {
        items.push(TreeItem::new("g:user:none", "No user templates. Use Create User Template to add one.").icon("info").parent("g:user"));
    }

    let mut groups: HashSet<&str> = ["", "Global", "community"].into();
    for (name, _) in TEMPLATES {
        let (label, dir) = template_label(name);
        let dir = dir.unwrap_or("");
        if groups.insert(dir) {
            let (sub, parent) = template_label(dir);
            items.push(group(&format!("g:{dir}"), sub).parent(&format!("g:{}", parent.unwrap_or(""))));
        }
        let key = format!("t:{name}");
        items.push(TreeItem::new(&key, label).icon(icon(&key)).parent(&format!("g:{dir}")));
    }

    let mut listed: Vec<(String, String)> = Vec::new();
    for key in starred {
        let label = match key.strip_prefix("u:") {
            Some(name) => user.iter().any(|u| u == name).then_some(name),
            None => key.strip_prefix("t:").filter(|n| TEMPLATES.iter().any(|(t, _)| t == n)).map(|n| template_label(n).0),
        };
        if let Some(label) = label {
            listed.push((key.clone(), label.to_owned()));
        }
    }
    listed.sort();
    if listed.is_empty() {
        items.retain(|i| i.key != "g:starred");
    }
    items.extend(listed.iter().map(|(key, label)| TreeItem::new(&format!("s:{key}"), label).icon("star").parent("g:starred")));
    items
}

/// Model-free invalidation; each mounted page requests its own current model.
fn refresh_templates() {
    host::refresh_view("templates");
    let _ = host::post_view_message("templates", r#"{"channel":"ui-kit-refresh"}"#);
}

#[derive(Default)]
struct IgnorePlugin {
    file_hints: file_hints::State,
    preferences: preferences::State,
    show_preferences: bool,
    inventory: git_inventory::State,
    unused: unused::State,
    directories: decorations::Directories,
    cached_checks: Vec<(String,String,Vec<check::Problem>)>,
    selected: Option<String>,
    expanded: HashSet<String>,
}

impl IgnorePlugin {
    /// The problems of `text`, checked once per path and text (diagnostics and quick fixes share them).
    fn problems_for(&mut self, path: &str, text: &str, dialect: check::Dialect) -> Vec<check::Problem> {
        if text.len() > 128 << 10 {
            return Vec::new();
        }

        if let Some((_, _, items)) = self.cached_checks.iter().find(|(p, t, _)| p == path && t == text) {
            return items.clone();
        }

        let items = check(text, dialect);
        self.cached_checks.retain(|(p, _, _)| p != path);
        if self.cached_checks.len() >= 8 {
            self.cached_checks.remove(0);
        }
        self.cached_checks.push((path.to_owned(), text.to_owned(), items.clone()));
        items
    }

    fn checks_for(&mut self, path: &str, text: &str, dialect: check::Dialect) -> Vec<Diagnostic> {
        let problems = self.problems_for(path, text, dialect);
        diagnostics(text, &problems)
    }

    fn selected_user_template(&self) -> Option<&str> {
        self.selected_template().and_then(|k| k.strip_prefix("u:"))
    }

    fn selected_template(&self) -> Option<&str> {
        let key = self.selected.as_deref()?;
        let key = key.strip_prefix("s:").unwrap_or(key);
        (key.starts_with("t:") || key.starts_with("u:")).then_some(key)
    }

    fn add_template(&self, key: &str, ctx: &Invocation) -> Result<(), Error> {
        let (label, content) = template_content(key)?;
        let editor = ctx.editor().ok_or("Open an ignore file to add the template to.")?;
        if !preferences::can_edit(&editor.path()) {return Err("Editor assistance is disabled for this ignore file type.".into());}
        if !dialect(&editor.path()).is_some_and(|d| d.syntax == check::Syntax::Glob) {
            return Err("Template insertion supports glob-syntax ignore file types only.".into());
        }

        let (_, root, _) = project_of(&editor.path()).ok_or("Open an ignore file inside the current project.")?;
        if ctx.project().ok_or(Error::Stale)?.root()?.trim_end_matches('/') != root {
            return Err(Error::Stale);
        }
        let doc = editor.document()?;
        let mut block = format!("### {label} template\n{content}");
        if !block.ends_with('\n') {
            block.push('\n');
        }

        let caret = setting("insertAtCursor", false).then(|| editor.selections().ok()?.first().copied()).flatten();
        let edit = match caret {
            Some(caret) => {
                let line_start = Position::new(caret.start.line, 0);
                let newline = line_ending(&doc.text);
                TextEdit {
                    range: Range::new(line_start, line_start),
                    text: format!("{block}{newline}").replace("\r\n", "\n").replace('\n', newline),
                }
            }
            None => append(&doc.text, &block, true),
        };
        editor.apply_edits(doc.version, &[edit])?;
        Ok(())
    }

    fn create_user_template(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let editor = ctx.editor().ok_or("Open the file to save as a template.")?;
        let before = editor.document()?;
        let path = editor.path();
        let default_name = path.rsplit(['/', '\\']).next().unwrap_or("");
        let Some(name) = host::input("Create User Template", Some(default_name))? else {return Ok(());};
        let name = name.trim();
        if name.is_empty() {return Err("Enter a template name.".into());}
        let key = format!("{USER_TEMPLATE}{name}");
        if host::storage::get(&key)?.is_some() {return Err(format!("A user template named {name} already exists.").into());}
        let current = editor.document()?;
        if editor.path() != path || current.version != before.version || current.text != before.text
            || ctx.editor().is_none_or(|current|current.path() != path) {return Err(Error::Stale);}
        host::storage::set(&key, &before.text)?;
        self.expanded.insert("g:user".into());
        host::notify(NotificationLevel::Info, "User template created", &format!("Saved user template {name}."));
        Ok(())
    }
}

impl openidea_plugin::Plugin for IgnorePlugin {
    fn activate(&mut self, _ctx: &openidea_plugin::Context) -> Result<(), Error> {
        self.file_hints.clear()?;
        let settings = host::subscribe(openidea_plugin::EventKind::SettingsChanged);
        let vcs = host::subscribe(openidea_plugin::EventKind::VcsChanged);
        let files = host::subscribe_paths("files-changed", &["**/*"]);
        self.unused.configure(files.is_ok());
        self.directories.configure(files.is_ok());
        self.file_hints.configure(settings.is_ok() && vcs.is_ok() && files.is_ok());
        Ok(())
    }

    fn deactivate(&mut self) {self.unused.configure(false);self.directories.configure(false); self.file_hints.configure(false); let _ = self.file_hints.clear();}

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "find-unused-complete" => self.unused.capture(ctx),
            "publish-git-sample-hints" => self.file_hints.publish(&self.inventory, ctx),
            "clear-git-sample-hints" => self.file_hints.clear().map(|_| None),
            "git-inventory-start" => { self.file_hints.clear()?; self.inventory.start(ctx) },
            "git-inventory-clear" => { self.file_hints.clear()?; self.inventory.clear(ctx) },
            "git-inventory-review" => self.inventory.review(ctx),
            "preferences" => {
                self.file_hints.clear()?;
                self.show_preferences = true;
                refresh_templates();
                host::reveal_view(preferences::VIEW)?;
                Ok(None)
            }
            "add-selected" => add_selected(ctx, false).map(|()| None),
            "unignore-selected" => add_selected(ctx, true).map(|()| None),
            "new-file" => new_ignore_file(self, ctx).map(|()| None),
            "open-ignoring-rule" => open_ignoring_rule(ctx).map(|()| None),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.unused.invalidate(events);
        self.file_hints.changed(events, ctx);
        self.directories.events(events);
        for event in events {
            match event {
                Event::VcsChanged(root) => self.inventory.invalidate(root),
                Event::FileSaved(path) => self.inventory.invalidate(path),
                Event::FilesChanged(batch) => {
                    for path in batch.created.iter().chain(&batch.modified).chain(&batch.removed) {
                        self.inventory.invalidate(path);
                    }
                }
                _ => {}
            }
        }
        let Some(project) = ctx.project() else { return };
        let mut checked = HashSet::new();

        for event in events {
            match event {
                Event::FileOpened(path) | Event::FileSaved(path) => {
                    if matches!(event, Event::FileOpened(_)) {
                        warn_missing_gitignore(&project);
                    }
                    let Some(dialect) = dialect(path).filter(|_| checked.insert(path)) else {
                        continue;
                    };
                    let Ok(text) = project.read_file(path) else {
                        continue;
                    };
                    self.directories.admit(path, &text, ctx);
                    if !preferences::checks(path) {
                        continue;
                    }
                    self.problems_for(path, &text, dialect);
                }
                Event::EditorFocused(path) => show_ignored_status(path.as_deref()),
                _ => {}
            }
        }
    }
}

impl Providers for IgnorePlugin {
    fn line_markers(&mut self, doc: &Document) -> Vec<openidea_plugin::providers::LineMarker> {
        if !preferences::enabled(&doc.path) {return Vec::new();}
        let mut markers=decorations::markers(doc);
        markers.extend(self.directories.markers(doc));
        markers.sort_by_key(|m|m.line);
        markers.truncate(256);
        markers
    }
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !preferences::enabled(&doc.path) {return Vec::new();}
        let Some(dialect) = dialect(&doc.path) else {
            return Vec::new();
        };
        let Some(offset) = text::offset(&doc.text, at) else {
            return Vec::new();
        };
        let line = &doc.text[doc.text[..offset].rfind('\n').map_or(0, |i| i + 1)..offset];

        if dialect.switchable {
            let Some(value) = line.trim_start().strip_prefix("syntax:") else {
                return Vec::new();
            };
            if !value.trim_start().chars().all(|c| c.is_ascii_alphabetic()) {
                return Vec::new();
            }
            return ["glob", "regexp", "rootglob"].map(|s| CompletionItem::new(s, CompletionKind::Keyword)).into();
        }

        if dialect.syntax != check::Syntax::Glob {
            return Vec::new();
        }
        path_completions(doc, offset, line)
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if !preferences::checks(&doc.path) {
            return Vec::new();
        }
        let mut actions=match dialect(&doc.path).filter(|_| doc.text.len()<=128<<10) {
            Some(dialect) => {
                let problems = self.problems_for(&doc.path, &doc.text, dialect);
                quick_fixes(doc, selection.start.line, problems)
            }
            None => Vec::new(),
        };
        actions.extend(self.unused.actions(doc,selection));actions
    }
    fn resolve_code_action(&mut self,doc:&Document,action:CodeAction)->Result<CodeAction,Error> {
        if !preferences::checks(&doc.path){return Err(Error::Stale);}
        if action.data.as_deref().is_some_and(|data|data.starts_with("[\"unused-snapshot\"")){self.unused.resolve(doc,action)}else{Ok(action)}
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if !preferences::enabled(&doc.path) {return Vec::new();}
        entry_target(doc, at).into_iter().collect()
    }
}

impl SyntaxProvider for IgnorePlugin {
    fn highlights(&mut self, language: &str, text: &str) -> Vec<Token> {
        if language!="dotignore" || !setting("packageSyntax",true) {return Vec::new();}
        highlight::highlights(text)
    }
}

impl Views for IgnorePlugin {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            if self.show_preferences {
                return self.preferences_render().map(|view|view.button(Button::new("preferences-back","Back to Templates").icon("arrow-left")));
            }
            let user = user_templates();
            let user_selected = self.selected_user_template().is_some();
            let starred = starred();
            let selected = self.selected_template();
            let star_label = if selected.is_some_and(|k| starred.contains(k)) { "Unstar" } else { "Star" };
            let items=template_tree(&user, &self.expanded, &starred);
            let body=if ui_kit::web::is_rendering() {
                let can_insert=ctx.editor().is_some_and(|editor| {
                    let path=editor.path();
                    preferences::can_edit(&path) && dialect(&path).is_some_and(|dialect|dialect.syntax == check::Syntax::Glob)
                        && project_of(&path).is_some_and(|(_,root,_)|ctx.project().and_then(|project|project.root().ok()).is_some_and(|current|current.trim_end_matches('/') == root))
                });
                Body::Other {kind:"tree".into(),data:serde_json::json!({"items":items.iter().map(|item| {
                    let key=item.key.strip_prefix("s:").unwrap_or(&item.key);
                    let template=key.starts_with("t:") || key.starts_with("u:");
                    serde_json::json!({"key":item.key,"parent":item.parent,"label":item.label,"icon":item.icon,"detail":item.detail,
                        "expandable":item.expandable,"expanded":item.expanded,"activationLabel":template.then_some("Add to Ignore File"),
                        "activationIcon":template.then_some("add"),"activationEnabled":if template {Some(can_insert)} else {None}})
                }).collect::<Vec<_>>()}).to_string()}
            } else {Body::Tree(items)};
            Ok(View::new(body)
                .button(Button::new("new", "Create User Template").icon("plus").enabled(ctx.editor().is_some()))
                .button(Button::new("delete", "Delete User Template").icon("trash").enabled(user_selected))
                .button(Button::new("star", star_label).icon("star").enabled(selected.is_some()))
                .button(Button::new("preferences", "Preferences").icon("settings"))
                .status(&format!("Double-click a template to add it to the open ignore file. {}", self.inventory.summary(ctx))))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, ctx, include_str!("ui_config.json"))? {
            return Ok(());
        }

        if self.show_preferences {
            if matches!(&event,ViewEvent::Button(id) if id == "preferences-back") {
                self.show_preferences=false;
            } else {
                self.file_hints.clear()?;
                let close=self.preferences.closes(&event);
                self.preferences_event(event,ctx)?;
                if close {self.show_preferences=false;}
            }
            refresh_templates();
            return Ok(());
        }
        // Global preferences are available without a project, but anonymous
        // packaged template actions must not gain mutation authority from that.
        if ui_kit::web::is_rendering() && ctx.project().and_then(|p|p.root().ok()).filter(|root|!root.is_empty()).is_none()
            && !matches!(&event,ViewEvent::Button(id) if id == "preferences")
            && !matches!(&event,ViewEvent::Select(_) | ViewEvent::Expand(_) | ViewEvent::Collapse(_)) {
            return Err(Error::Denied("Open an identifiable project before using template actions.".into()));
        }
        match event {
            ViewEvent::Activate(key) if !key.starts_with("g:") => {
                return self.add_template(key.strip_prefix("s:").unwrap_or(&key), ctx);
            }
            ViewEvent::Select(key) => {
                self.selected = Some(key);
                host::refresh_view(view);
            }
            ViewEvent::Expand(key) => {
                self.expanded.insert(key);
                host::refresh_view(view);
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
                host::refresh_view(view);
            }
            ViewEvent::Button(button) => {
                match button.as_str() {
                    "preferences" => {self.file_hints.clear()?;self.show_preferences=true;refresh_templates();}
                    "new" => self.create_user_template(ctx)?,
                    "star" => {
                        let key = self.selected_template().ok_or("Select a template to star.")?;
                        let storage_key = format!("{STARRED_PREFIX}{key}");
                        if host::storage::get(&storage_key)?.is_some() {
                            host::storage::delete(&storage_key)?;
                        } else {
                            host::storage::set(&storage_key, "1")?;
                        }
                    }
                    "delete" => {
                        let name = self.selected_user_template().ok_or("Select a user template to delete.")?.to_owned();
                        let key=format!("{USER_TEMPLATE}{name}");
                        let content=host::storage::get(&key)?.ok_or_else(||Error::NotFound(format!("user template {name}")))?;
                        let selected=self.selected.clone();
                        if !ui_kit::prompts::confirm_exact("Delete User Template",&format!("Delete user template {name}?\n\nThis removes the saved template and its favorite marker. Existing ignore files are unchanged."))? {
                            return Ok(());
                        }
                        if self.selected != selected || host::storage::get(&key)?.as_ref() != Some(&content) {
                            return Err(Error::Stale);
                        }
                        host::storage::delete(&key)?;
                        host::storage::delete(&format!("{STARRED_PREFIX}u:{name}"))?;
                        self.selected = None;
                        host::notify(NotificationLevel::Info, "User template deleted", &format!("Deleted user template {name}."));
                    }
                    _ => {}
                }
                host::refresh_view(view);
            }
            _ => {}
        }
        Ok(())
    }
}

impl openidea_plugin::diagnostics::DiagnosticsProvider for IgnorePlugin {
    fn diagnostics(&mut self,doc:&Document)->Vec<Diagnostic> {
        if doc.text.len()>128<<10||!preferences::checks(&doc.path){return Vec::new();}
        let Some(dialect)=dialect(&doc.path) else{return Vec::new();};
        let mut diagnostics=self.checks_for(&doc.path,&doc.text,dialect);
        diagnostics.extend(self.unused.diagnostics(doc));diagnostics
    }
}

// API 1.3 registers the optional exports on the same component.
openidea_plugin::register_plugin!(IgnorePlugin, providers, views, syntax, semantic_tokens, file_actions, diagnostics);

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
