use check::{Dialect, check, dialect};
use matcher::{Rule, Sources, ignored_by};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::syntax::{SyntaxProvider, Token};
use openidea_plugin::views::{Body, Button, Field, FieldValue, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, Position, Project, Range, Severity,
    StatusItem, TextEdit, host, text,
};
use std::collections::HashSet;

mod check;
mod highlight;
mod matcher;
mod templates {
    include!(concat!(env!("OUT_DIR"), "/templates.rs"));
}
use templates::TEMPLATES;

const IGNORED_STATUS: &str = "ignored";
const USER_TEMPLATE: &str = "template:";

fn range(text: &str, bytes: &std::ops::Range<usize>) -> Range {
    Range::new(text::position(text, bytes.start), text::position(text, bytes.end))
}

fn diagnostics(text: &str, dialect: Dialect) -> Vec<Diagnostic> {
    check(text, dialect)
        .into_iter()
        .map(|p| {
            let severity = if p.warning { Severity::Warning } else { Severity::Error };
            let mut diagnostic = Diagnostic::new(range(text, &(p.start..p.end)), severity, p.message);
            diagnostic.code = Some(p.code.into());
            diagnostic
        })
        .collect()
}

fn quick_fixes(doc: &Document, line: u32, dialect: Dialect) -> Vec<CodeAction> {
    check(&doc.text, dialect)
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

/// Inserts `addition` (whole lines) after the last line of `content`.
fn append(content: &str, addition: &str, blank_line: bool) -> TextEdit {
    let end = text::position(content, content.len());
    let mut prefix = String::new();
    if !content.is_empty() && !content.ends_with('\n') {
        prefix.push('\n');
    }
    if !content.is_empty() && blank_line {
        prefix.push('\n');
    }
    TextEdit { range: Range::new(end, end), text: format!("{prefix}{addition}") }
}

/// The open project holding `path`, its root and `path` relative to it.
fn project_of(path: &str) -> Option<(Project, String, String)> {
    host::projects().ok()?.into_iter().find_map(|p| {
        let root = p.root().ok()?.trim_end_matches('/').to_owned();
        let relative = path.strip_prefix(&root)?.strip_prefix('/')?.to_owned();
        Some((p, root, relative))
    })
}

/// Add to .gitignore (and its unignore twin) for a file of a project whose
/// root has a .gitignore that doesn't list it yet.
fn gitignore_actions(path: &str) -> Vec<CodeAction> {
    let Some((project, root, relative)) = project_of(path) else { return Vec::new() };
    let gitignore = format!("{root}/.gitignore");
    let Ok(content) = project.read_file(&gitignore) else { return Vec::new() };
    let listed: HashSet<&str> = content.lines().map(str::trim).collect();
    let entry = format!("/{}", relative.replace('[', "\\[").replace(']', "\\]"));

    [(entry.clone(), ""), (format!("!{entry}"), " (unignore)")]
        .into_iter()
        .filter(|(line, _)| !listed.contains(line.as_str()))
        .map(|(line, note)| CodeAction {
            title: format!("Add {line} to .gitignore{note}"),
            kind: Some("source".into()),
            preferred: false,
            edits: vec![(gitignore.clone(), vec![append(&content, &format!("{line}\n"), false)])],
            data: None,
        })
        .collect()
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
    host::setting_bool(key).unwrap_or(default)
}

/// The status bar says when the focused file is ignored (the original plugin's
/// editor banner).
fn show_ignored_status(path: Option<&str>) {
    let verdict = path.filter(|_| setting("ignoredEditing", true)).and_then(|path| {
        let (project, root, relative) = project_of(path)?;
        let sources = ignore_sources(&project, &root, &relative);
        let (file, entry) = ignored_by(&sources, &relative)?;
        Some(format!("This file is ignored by `{entry}` in {file}."))
    });

    match verdict {
        Some(tooltip) => host::set_status_item(
            IGNORED_STATUS,
            &StatusItem { text: "Ignored".into(), tooltip: Some(tooltip), command: None },
        ),
        None => host::remove_status_item(IGNORED_STATUS),
    }
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

fn template_tree(user: &[String], expanded: &HashSet<String>) -> Vec<TreeItem> {
    let group = |key: &str, label: &str| TreeItem::new(key, label).icon("folder").expandable(expanded.contains(key));
    let mut items = vec![
        group("g:user", "User templates"),
        group("g:", "Languages and frameworks"),
        group("g:Global", "Global templates (OS, editors, tools)"),
        group("g:community", "Community templates"),
    ];
    items.extend(user.iter().map(|name| TreeItem::new(&format!("u:{name}"), name).icon("star").parent("g:user")));
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
        items.push(TreeItem::new(&format!("t:{name}"), label).icon("file-ignored").parent(&format!("g:{dir}")));
    }
    items
}

#[derive(Default)]
struct IgnorePlugin {
    /// The name typed for a new user template, while its form is shown.
    naming: Option<String>,
    /// The user template waiting for its deletion to be confirmed.
    deleting: Option<String>,
    selected: Option<String>,
    expanded: HashSet<String>,
}

impl IgnorePlugin {
    fn selected_user_template(&self) -> Option<&str> {
        self.selected.as_deref().and_then(|k| k.strip_prefix("u:"))
    }

    fn add_template(&self, key: &str, ctx: &Invocation) -> Result<(), Error> {
        let (label, content) = template_content(key)?;
        let editor = ctx.editor().ok_or("Open an ignore file to add the template to.")?;
        if dialect(&editor.path()).is_none() {
            return Err(format!("{} is not an ignore file.", editor.path()).into());
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
                TextEdit { range: Range::new(line_start, line_start), text: format!("{block}\n") }
            }
            None => append(&doc.text, &block, true),
        };
        editor.apply_edits(doc.version, &[edit])?;
        Ok(())
    }

    fn save_user_template(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let name = self.naming.as_deref().unwrap_or("").trim().to_owned();
        if name.is_empty() {
            return Err("Enter a template name.".into());
        }
        let editor = ctx.editor().ok_or("Open the file to save as a template.")?;
        let key = format!("{USER_TEMPLATE}{name}");
        if host::storage::get(&key)?.is_some() {
            return Err(format!("A user template named {name} already exists.").into());
        }
        host::storage::set(&key, &editor.document()?.text)?;
        self.naming = None;
        self.expanded.insert("g:user".into());
        host::notify(NotificationLevel::Info, "User template created", &format!("Saved user template {name}."));
        Ok(())
    }
}

impl openidea_plugin::Plugin for IgnorePlugin {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        let Some(project) = ctx.project() else { return };
        let mut checked = HashSet::new();

        for event in events {
            match event {
                Event::FileOpened(path) | Event::FileSaved(path) => {
                    if matches!(event, Event::FileOpened(_)) {
                        warn_missing_gitignore(&project);
                    }
                    let Some(dialect) = dialect(path).filter(|_| checked.insert(path)) else { continue };
                    let Ok(text) = project.read_file(path) else { continue };
                    if let Err(e) = host::publish_diagnostics(path, &diagnostics(&text, dialect)) {
                        host::log(LogLevel::Warn, &format!("{path}: {e}"));
                    }
                }
                Event::EditorFocused(path) => show_ignored_status(path.as_deref()),
                _ => {}
            }
        }
    }
}

impl Providers for IgnorePlugin {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !dialect(&doc.path).is_some_and(|d| d.switchable) {
            return Vec::new();
        }
        let Some(offset) = text::offset(&doc.text, at) else { return Vec::new() };
        let line = &doc.text[doc.text[..offset].rfind('\n').map_or(0, |i| i + 1)..offset];
        let Some(value) = line.trim_start().strip_prefix("syntax:") else { return Vec::new() };
        if !value.trim_start().chars().all(|c| c.is_ascii_alphabetic()) {
            return Vec::new();
        }

        ["glob", "regexp", "rootglob"].map(|s| CompletionItem::new(s, CompletionKind::Keyword)).into()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        match dialect(&doc.path) {
            Some(dialect) => quick_fixes(doc, selection.start.line, dialect),
            None => gitignore_actions(&doc.path),
        }
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        entry_target(doc, at).into_iter().collect()
    }
}

impl SyntaxProvider for IgnorePlugin {
    fn highlights(&mut self, _language: &str, text: &str) -> Vec<Token> {
        highlight::highlights(text)
    }
}

impl Views for IgnorePlugin {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if let Some(name) = &self.naming {
            return Ok(View::new(Body::Form(vec![Field::new("name", "Template name", FieldValue::Text(name.clone()))]))
                .button(Button::new("save", "Create"))
                .button(Button::new("cancel", "Cancel"))
                .status("Saves the focused file as a user template."));
        }

        if let Some(name) = &self.deleting {
            return Ok(View::message(&format!("Delete user template {name}?"))
                .button(Button::new("confirm-delete", "Delete").icon("trash"))
                .button(Button::new("cancel", "Cancel")));
        }

        let user = user_templates();
        let user_selected = self.selected_user_template().is_some();
        Ok(View::new(Body::Tree(template_tree(&user, &self.expanded)))
            .button(Button::new("new", "Create User Template").icon("plus").enabled(ctx.editor().is_some()))
            .button(Button::new("delete", "Delete User Template").icon("trash").enabled(user_selected))
            .status("Double-click a template to add it to the open ignore file."))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Activate(key) if !key.starts_with("g:") => return self.add_template(&key, ctx),
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
            ViewEvent::Field(_, value) => self.naming = Some(value),
            ViewEvent::Button(button) => {
                match button.as_str() {
                    "new" => {
                        let name = ctx.editor().map(|e| e.path().rsplit('/').next().unwrap_or("").to_owned());
                        self.naming = Some(name.unwrap_or_default());
                    }
                    "save" => self.save_user_template(ctx)?,
                    "cancel" => {
                        self.naming = None;
                        self.deleting = None;
                    }
                    "delete" => {
                        let name = self.selected_user_template().ok_or("Select a user template to delete.")?;
                        self.deleting = Some(name.to_owned());
                    }
                    "confirm-delete" => {
                        let Some(name) = self.deleting.take() else { return Ok(()) };
                        host::storage::delete(&format!("{USER_TEMPLATE}{name}"))?;
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

// `register_plugin!` has no arm for syntax with providers and views; these are
// its expansions put together (each export is its own WIT world, merged into
// one component like the macro does for two of them).
#[cfg(not(target_arch = "wasm32"))]
const _: fn() -> IgnorePlugin = <IgnorePlugin as Default>::default;
#[cfg(target_arch = "wasm32")]
const _: () = {
    use openidea_plugin::syntax_bindings::exports::openidea::plugin::syntax_provider as wit;

    struct Component;
    openidea_plugin::__plugin_exports!(IgnorePlugin, Component, openidea_plugin::bindings);
    openidea_plugin::__providers_exports!(IgnorePlugin, Component);
    openidea_plugin::__views_exports!(IgnorePlugin, Component);

    impl wit::Guest for Component {
        fn highlights(language: String, text: String) -> Vec<wit::Token> {
            openidea_plugin::__rt::highlights::<IgnorePlugin>(&language, &text)
        }

        fn outline(language: String, text: String) -> Vec<wit::Symbol> {
            openidea_plugin::__rt::outline::<IgnorePlugin>(&language, &text)
        }

        fn folding(language: String, text: String) -> Vec<wit::Fold> {
            openidea_plugin::__rt::folding::<IgnorePlugin>(&language, &text)
        }
    }

    openidea_plugin::bindings::__export_plugin_world!(Component with_types_in openidea_plugin::bindings);
    openidea_plugin::syntax_bindings::__export_syntax_world!(Component with_types_in openidea_plugin::syntax_bindings);
};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
