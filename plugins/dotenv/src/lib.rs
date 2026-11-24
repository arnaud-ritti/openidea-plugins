//! `.env` files: highlighting, inspections with their quick
//! fixes, completion, go to declaration and find usages of environment
//! variables in `${…}` and in code, keys of Dockerfiles and docker-compose
//! files.

mod code;
mod docker;
mod hover;
mod ownership;
mod semantic;
mod source_update;
mod index;
mod lint;
mod parse;

use index::{Declaration, Index, Usage, env_declarations, is_env_file, nested_usages, range};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::syntax::{Highlight, SyntaxProvider, Token};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, Plugin, Position, Project, Range, Severity, TextEdit, host, text,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// What an activation or a batch of events may spend walking projects (of their 5 s).
/// Reading time of one activation or event batch; an unfinished walk resumes on the next. Every
/// call gets 5×10⁹ fuel, which a few seconds of parsing can use up and trap the plugin.
const SCAN_BUDGET: Duration = Duration::from_secs(1);
const RELATED_MARKER: &str = "icons/dotenv.svg";
/// Markers one gutter button lists at most; the IDE merges a line's same-icon markers into one chooser.
const MAX_LINE_TARGETS: usize = 200;

#[derive(Default)]
pub struct DotEnv {
    /// Each open project's index, by root.
    projects: HashMap<String, Index>,
    /// Changed paths an over-budget batch left unread, finished by the next batch.
    backlog: Vec<(String, bool)>,
    updates: source_update::Updates,
}

/// The open project holding `path`, and its root.
fn project_of(path: &str) -> Option<(String, Project)> {
    host::projects()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| Some((p.root().ok()?, p)))
        .filter(|(root, _)| Path::new(path).starts_with(root))
        .max_by_key(|(root, _)| root.len())
}

/// The line holding byte `offset`: its start and text.
fn line_of(text: &str, offset: usize) -> (usize, &str) {
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    (start, &text[start..end])
}

/// Byte range of the environment variable key at `at`, if the caret is in one.
fn key_at(doc: &Document, at: Position) -> Option<std::ops::Range<usize>> {
    let offset = text::offset(&doc.text, at)?;
    let (start, line) = line_of(&doc.text, offset);
    let key = if doc.language == "dotenv" { code::key_in_nested(line, offset - start) } else { code::key_in_code(line, offset - start) }?;
    Some(start + key.start..start + key.end)
}

/// `$KEY` being typed in a double-quoted value, from the `$`.
fn dollar_at(doc: &Document, at: Position) -> Option<std::ops::Range<usize>> {
    let offset = text::offset(&doc.text, at)?;
    let tokens = parse::lex(&doc.text);
    let quoted = parse::properties(&doc.text, &tokens).into_iter().any(|p| {
        let Some(value) = p.value.filter(|_| p.quote == Some('"')) else {
            return false;
        };
        let closed = value.len() > 1 && doc.text[..value.end].ends_with('"');
        value.start < offset && offset <= value.end && (offset < value.end || !closed)
    });
    if !quoted {
        return None;
    }
    let (start, line) = line_of(&doc.text, offset);
    let dollar = code::dollar_at(line, offset - start)?;
    Some(start + dollar.start..start + dollar.end)
}

/// The key the caret is on where a `.env` file declares it.
fn declared_key_at(doc: &Document, at: Position) -> Option<String> {
    let offset = text::offset(&doc.text, at)?;
    let tokens = parse::lex(&doc.text);
    parse::properties(&doc.text, &tokens).into_iter().find(|p| p.key.start <= offset && offset <= p.key.end).map(|p| p.name(&doc.text))
}

impl DotEnv {
    /// `path`'s project index: just the root's env files until the project is walked.
    fn index(&mut self, path: &str) -> Option<&Index> {
        let (root, project) = project_of(path)?;
        Some(self.projects.entry(root.clone()).or_insert_with(|| Index::quick(&project, &root)))
    }

    /// Walks the open projects not walked yet; a walked index is only updated per file. Too slow for editor requests:
    /// only activation and events (5 s) do it.
    fn index_projects(&mut self) {
        let deadline = Instant::now() + SCAN_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let Ok(root) = project.root() else { continue };
            match self.projects.get_mut(&root) {
                Some(index) if index.walked => {}
                Some(index) if index.walk_in_progress() => index.resume(&project, &root, deadline),
                _ => {
                    let index = Index::full(&project, &root, deadline);
                    self.projects.insert(root, index);
                }
            }
        }
    }

    /// Declarations visible from `path`, with `unsaved` (the editor's text of
    /// `path`) in place of the file on disk.
    fn known(&mut self, path: &str, unsaved: Option<&str>) -> Vec<Declaration> {
        let mut known: Vec<Declaration> = self
            .index(path)
            .map(|index| index.declarations.iter().filter(|d| d.location.path != path).cloned().collect())
            .unwrap_or_default();
        if let Some(text) = unsaved {
            known.extend(env_declarations(path, text));
        }
        known
    }

    /// Where `key` is used, with the `${…}` of `unsaved` in place of `path` on disk.
    fn usages(&mut self, path: &str, unsaved: &str, key: &str) -> Vec<Location> {
        let mut usages: Vec<Usage> =
            self.index(path).map(|index| index.usages.iter().filter(|u| u.location.path != path).cloned().collect()).unwrap_or_default();
        usages.extend(nested_usages(path, unsaved));
        usages.into_iter().filter(|u| u.key == key).map(|u| u.location).collect()
    }

    /// Runs the inspections on `text` and shows their problems in the editor.
    fn inspect(&mut self, path: &str, text: &str) -> Vec<lint::Problem> {
        if text.len() > index::MAX_SOURCE_BYTES {
            return Vec::new();
        }
        let tokens = parse::lex(text);
        let properties = parse::properties(text, &tokens);
        let cached = host::project_of(path).and_then(|project| project.root().ok()).and_then(|root| self.projects.get(&root));
        // Never call known()/index() on a live diagnostic request: their cold
        // path reads files. Unknown cache state must not become proven absence.
        const MAX_CACHED_KEYS: usize = 8192;
        let complete = cached.is_some_and(|index| index.complete && index.declarations.len() <= MAX_CACHED_KEYS);
        let mut known: HashSet<String> = properties.iter().map(|p| p.name(text)).collect();
        if let Some(index) = cached {
            known.extend(index.declarations.iter().take(MAX_CACHED_KEYS).filter(|d| d.location.path != path).map(|d| d.key.clone()));
        }
        let mut problems = lint::check(text, &tokens, &properties, |key| !complete || known.contains(key));
        problems.retain(|problem| host::setting_bool(lint::setting_of(problem.code)).unwrap_or(true));
        problems
    }


}

const SOURCES_POINT: &str = "aritti.dotenv.sources";

/// Reads what other plugins contributed to [`SOURCES_POINT`]; unknown roles and path-like names are ignored.
fn contributed_sources() -> Vec<(String, bool)> {
    let contributions = host::extension_contributions(SOURCES_POINT).unwrap_or_default();
    contributions
        .iter()
        .filter_map(|c| serde_json::from_str::<serde_json::Value>(&c.data).ok())
        .filter_map(|v| Some((v["fileName"].as_str()?.to_owned(), v["role"].as_str()?.to_owned())))
        .filter(|(name, _)| !name.contains(['/', '\\']))
        .filter_map(|(name, role)| match role.as_str() {
            "declares" => Some((name, true)),
            "reads" => Some((name, false)),
            _ => None,
        })
        .collect()
}

impl Plugin for DotEnv {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        index::set_extra_sources(contributed_sources());
        let _ = host::subscribe(EventKind::ExtensionsChanged);
        let _ = host::subscribe(EventKind::FileOpened);
        let _ = host::subscribe(EventKind::FileSaved);
        watch_files();
        self.index_projects();
        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "show-value" => self.show_value(ctx).map(|()| None),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.apply_events(events);
    }
}

impl DotEnv {
    fn apply_events(&mut self, events: &[Event]) {
        self.apply_events_before(events, Instant::now() + SCAN_BUDGET);
    }

    fn apply_events_before(&mut self, events: &[Event], deadline: Instant) {
        if events.iter().any(|e| matches!(e, Event::Other { kind, .. } if kind == EventKind::ExtensionsChanged.name())) {
            index::set_extra_sources(contributed_sources());
            watch_files();
            self.projects.clear();
        }
        self.index_projects();
        let mut pending = std::mem::take(&mut self.backlog);
        pending.extend(changed_paths(events));
        let mut pending = pending.into_iter();
        while let Some((path, removed)) = pending.next() {
            if !Index::indexes(&path) {
                if removed {
                    if let Some((root, _)) = project_of(&path) {
                        if let Some(index) = self.projects.get_mut(&root) {
                            index.forget_under(&path);
                        }
                    }
                }
                continue;
            }
            let Some((root, project)) = project_of(&path) else {
                continue;
            };
            let Some(index) = self.projects.get_mut(&root) else {
                continue;
            };
            if Instant::now() >= deadline {
                self.backlog.push((path, removed));
                self.backlog.extend(pending);
                break;
            }
            let read = match removed {
                true => Err(Error::NotFound(path.clone())),
                false => project.read_file(&path),
            };
            index.touched(&path);
            let owners = index.composes_building(&path);
            match read {
                Ok(text) => {
                    index.update(&path, Some(&text));
                    index.add_compose_env_files(&project, &root, &path, &text);
                }
                Err(Error::NotFound(_)) => index.update(&path, None),
                Err(_) => index.mark_incomplete(&path),
            }
            for compose in owners.iter().filter(|c| **c != path) {
                if let Ok(text) = project.read_file(compose) {
                    index.touched(compose);
                    index.update(compose, Some(&text));
                    index.add_compose_env_files(&project, &root, compose, &text);
                }
            }
        }
    }

    /// Shows the real value of the key under the caret, which hover and completion mask.
    /// A dismissable list, not a notification: notifications stay in the history, which would unmask the value.
    fn show_value(&mut self, ctx: &Invocation) -> Result<(), Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let denied = |e: Error| ui_kit::denied::map(e, "Show Value at Caret", "editor.read", ".env files");
        let doc = editor.document().map_err(denied)?;
        let at = editor.selections().map_err(denied)?.first().map(|r| r.start).ok_or("No caret.")?;
        let doc = Document { path: editor.path(), language: editor.language(), version: doc.version, text: doc.text };
        let key = match (doc.language == "dotenv").then(|| declared_key_at(&doc, at)).flatten() {
            Some(key) => key,
            None => key_at(&doc, at).map(|k| doc.text[k].to_owned()).ok_or("Put the caret on an environment variable name.")?,
        };
        let unsaved = (doc.language == "dotenv").then_some(doc.text.as_str());
        let mut found: Vec<Declaration> = self.known(&doc.path, unsaved).into_iter().filter(|d| d.key == key).collect();
        found.dedup_by(|a, b| a.location == b.location && a.value == b.value);
        if found.is_empty() {
            return Err(Error::NotFound(format!("No declaration of {key} found.")));
        }
        let items: Vec<openidea_plugin::PickItem> = found
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let at = format!("{}:{}", index::file_name(&d.location.path), d.location.range.start.line + 1);
                openidea_plugin::PickItem::new(&i.to_string(), &d.value).description(&at)
            })
            .collect();
        let chosen = host::pick(&format!("{key}: pick one to open its declaration"), &items)?;
        if let Some(d) = chosen.and_then(|i| i.parse::<usize>().ok()).and_then(|i| found.get(i)) {
            let start = d.location.range.start;
            host::open_location(&d.location.path, start.line, start.character)?;
        }
        Ok(())
    }
}

/// Files the events touched, and whether each was removed.
fn changed_paths(events: &[Event]) -> Vec<(String, bool)> {
    let mut paths = Vec::new();
    for event in events {
        match event {
            Event::FileOpened(path) | Event::FileSaved(path) => paths.push((path.clone(), false)),
            Event::FilesChanged(changes) => {
                paths.extend(changes.created.iter().chain(&changes.modified).map(|p| (p.clone(), false)));
                paths.extend(changes.removed.iter().map(|p| (p.clone(), true)));
            }
            _ => {}
        }
    }
    paths
}

/// Watches what the index reads, so edits made outside the editor are seen.
/// External edits to a gitignored .env may not fire; opening or saving it does.
fn watch_files() {
    let mut globs: Vec<String> = ["**/.env", "**/.env.*", "**/*.env", "**/.flaskenv", "**/Dockerfile", "**/Dockerfile.*", "**/*.dockerfile", "**/*.Dockerfile", "**/phpunit.xml*"]
        .into_iter()
        .chain(["docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"])
        .map(|name| if name.starts_with("**/") { name.to_owned() } else { format!("**/{name}") })
        .collect();
    globs.extend(index::CODE.iter().map(|ext| format!("**/*.{ext}")));
    globs.extend(contributed_sources().into_iter().map(|(name, _)| format!("**/{name}")));
    let globs: Vec<&str> = globs.iter().map(String::as_str).collect();
    if host::subscribe_paths("files-changed", &globs).is_err() && !WATCH_WARNED.replace(true) {
        host::notify(
            openidea_plugin::NotificationLevel::Warning,
            ".env files",
            "External file changes can't be tracked. Allow events for .env files in Settings › Plugins.",
        );
    }
}

thread_local! {
    static WATCH_WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl SyntaxProvider for DotEnv {
    fn highlights(&mut self, _language: &str, text: &str) -> Vec<Token> {
        parse::lex(text)
            .into_iter()
            .filter_map(|t| {
                let kind = match t.kind {
                    parse::Kind::Key | parse::Kind::NestedKey => Highlight::Keyword,
                    parse::Kind::Value | parse::Kind::Quote => Highlight::String,
                    parse::Kind::Comment | parse::Kind::Export => Highlight::Comment,
                    parse::Kind::Separator => Highlight::Operator,
                    parse::Kind::NestedStart | parse::Kind::NestedEnd => Highlight::Punctuation,
                    parse::Kind::Space | parse::Kind::Bad => return None,
                };
                Some(Token { range: t.range, kind })
            })
            .collect()
    }
}

impl Providers for DotEnv {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let in_env_file = doc.language == "dotenv";
        if !in_env_file && !host::setting_bool("completion").unwrap_or(true) {
            return Vec::new();
        }
        // `"$DB` in a `.env` value completes to `${DB_HOST}`: the editor has no
        // popup on `$` (the original's typed handler), but one on the letters after it.
        let (key, dollar) = match key_at(doc, at) {
            Some(key) => (key, false),
            None if in_env_file => match dollar_at(doc, at) {
                Some(key) => (key, true),
                None => return Vec::new(),
            },
            None => return Vec::new(),
        };

        let replace = range(&doc.text, key);
        let show_values = host::setting_bool("showValues").unwrap_or(true);
        let mut known = self.known(&doc.path, in_env_file.then_some(doc.text.as_str()));
        known.sort_by_key(|d| !d.primary);
        let mut seen = HashSet::new();

        known
            .into_iter()
            .filter(|d| seen.insert(d.key.clone()))
            .map(|d| {
                let mut item = match dollar {
                    true => {
                        let mut item = CompletionItem::new(&format!("${{{}}}", d.key), CompletionKind::Variable);
                        item.filter_text = Some(format!("${}", d.key));
                        item
                    }
                    false => CompletionItem::new(&d.key, CompletionKind::Variable),
                };
                if show_values && !d.value.is_empty() {
                    item.detail = Some(format!("= {}", index::shown_value(&d.key, &d.value)));
                }
                item.replace = Some(replace.clone());
                item
            })
            .collect()
    }

    /// From a usage to the declarations; from a declaration in a `.env` file
    /// to the usages (the original's Ctrl+Click on a key).
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if doc.language == "dotenv" {
            if let Some(key) = declared_key_at(doc, at) {
                return self.usages(&doc.path, &doc.text, &key);
            }
        }
        let Some(key) = key_at(doc, at) else {
            return Vec::new();
        };

        let key = &doc.text[key];
        let unsaved = (doc.language == "dotenv").then_some(doc.text.as_str());
        let matching: Vec<Declaration> = self.known(&doc.path, unsaved).into_iter().filter(|d| d.key == key).collect();
        let primary = matching.iter().any(|d| d.primary);

        matching.into_iter().filter(|d| d.primary == primary).map(|d| d.location).collect()
    }

    /// On each key of a `.env` file, one marker per usage and per declaration in another env file, from the
    /// cached index only: the IDE merges them into one button (a chooser with several, a jump with one).
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if doc.language != "dotenv" || doc.text.len() > index::MAX_SOURCE_BYTES {
            return Vec::new();
        }
        let Some(index) = project_of(&doc.path).and_then(|(root, _)| self.projects.get(&root)) else {
            return Vec::new();
        };
        let tokens = parse::lex(&doc.text);
        let keys: Vec<(String, u32)> =
            parse::properties(&doc.text, &tokens).into_iter().map(|p| (p.name(&doc.text), text::position(&doc.text, p.key.start).line)).collect();
        let mut targets: HashMap<&str, Vec<Location>> = keys.iter().map(|(key, _)| (key.as_str(), Vec::new())).collect();
        let unsaved = nested_usages(&doc.path, &doc.text);
        for usage in index.usages.iter().filter(|u| u.location.path != doc.path).chain(&unsaved) {
            if let Some(found) = targets.get_mut(usage.key.as_str()) {
                found.push(usage.location.clone());
            }
        }
        for declaration in index.declarations.iter().filter(|d| d.location.path != doc.path && is_env_file(&d.location.path)) {
            if let Some(found) = targets.get_mut(declaration.key.as_str()) {
                found.push(declaration.location.clone());
            }
        }
        keys.iter().flat_map(|(key, line)| related_markers(*line, targets[key.as_str()].clone())).collect()
    }

    /// Quick fixes use the same findings as versioned diagnostics.
    // ponytail: with no language server for .env, an empty answer comes back as `unavailable` and the
    // editor stops asking for 15 s; the host should treat a plugin's answer as available (upstream fix).
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let actions: Vec<CodeAction> = self
            .inspect(&doc.path, &doc.text)
            .into_iter()
            .filter_map(|problem| {
                let fix = problem.fix?;
                let at = range(&doc.text, problem.range);
                let touches = at.start.line <= selection.end.line && at.end.line >= selection.start.line;
                touches.then(|| CodeAction {
                    title: fix.title.into(),
                    kind: Some("quickfix".into()),
                    preferred: true,
                    edits: vec![(doc.path.clone(), vec![TextEdit { range: range(&doc.text, fix.range), text: fix.text }])],
                    data: None,
                })
            })
            .collect();

        actions
    }
}

/// One marker per target on `line`, at most [`MAX_LINE_TARGETS`]; the tooltip says when some were left out.
fn related_markers(line: u32, targets: Vec<Location>) -> Vec<LineMarker> {
    let targets: Vec<Location> = targets.into_iter().filter(|target| target.range.start != target.range.end).collect();
    let tooltip = match targets.len() > MAX_LINE_TARGETS {
        true => format!("Usages and other env files (first {MAX_LINE_TARGETS} of {})", targets.len()),
        false => "Usages and other env files".to_owned(),
    };
    targets
        .into_iter()
        .take(MAX_LINE_TARGETS)
        .map(|target| LineMarker { line, icon: Some(RELATED_MARKER.into()), tooltip: tooltip.clone(), target: Some(target) })
        .collect()
}

// API 1.3 registers the optional exports on the same component.
openidea_plugin::register_plugin!(DotEnv, providers, syntax, diagnostics, hover, file_actions, semantic_tokens);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn project() -> MockHost {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "DB_HOST=localhost\nAPI_URL=\"http://x\"\n");
        host.add_file(".env.example", "DB_HOST=example\nONLY_EXAMPLE=1\n");
        host.add_file("app.py", "host = os.environ['DB_HOST']\n");
        host
    }

    fn walked() -> DotEnv {
        let mut plugin = DotEnv::default();
        plugin.index_projects();
        plugin
    }

    #[test]
    fn disabled_lowercase_key_check_reports_nothing() {
        let host = project();
        let env = doc("/p/.env", "dotenv", "my_key=1\n");
        let lowercase = |plugin: &mut DotEnv| {
            openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(plugin, &env).iter().filter(|d| d.code.as_deref() == Some("DotEnvLowercaseKeyInspection")).count()
        };
        assert_eq!(lowercase(&mut walked()), 1);

        host.set_setting("checkKeyNames", "false");

        assert_eq!(lowercase(&mut walked()), 0);
    }

    #[test]
    fn contributed_file_names_are_indexed_and_follow_changes() {
        let host = project();
        host.add_file("vars.custom", "FROM_PLUGIN=1\n");
        host.add_file("run.custom2", "getenv('FROM_PLUGIN');\n");
        host.set_extension_contributions(
            SOURCES_POINT,
            vec![
                ("acme.x", r#"{"fileName":"vars.custom","role":"declares"}"#),
                ("acme.x", r#"{"fileName":"run.custom2","role":"reads"}"#),
                ("acme.x", r#"{"fileName":"../evil","role":"declares"}"#),
                ("acme.x", r#"{"fileName":"odd","role":"nope"}"#),
            ],
        );
        let mut plugin = DotEnv::default();
        plugin.activate(&Context::default()).ok();
        assert_eq!(contributed_sources(), [("vars.custom".to_owned(), true), ("run.custom2".to_owned(), false)]);
        assert!(index::is_env_file("/p/vars.custom"));
        assert!(index::Index::indexes("/p/run.custom2"));

        host.set_extension_contributions(SOURCES_POINT, vec![]);
        plugin.apply_events(&[Event::Other { kind: "extensions-changed".into(), data: "[]".into() }]);
        assert!(!index::is_env_file("/p/vars.custom"));
    }

    #[test]
    fn completes_keys_in_code_with_the_primary_values() {
        let _host = project();
        let php = doc("/p/index.php", "php", "<?php getenv('DB');");
        let items = DotEnv::default().completion(&php, Position::new(0, 16), None);
        let labels: Vec<_> = items.iter().map(|i| (i.label.as_str(), i.detail.as_deref())).collect();
        assert_eq!(labels, [("DB_HOST", Some("= localhost")), ("API_URL", Some("= http://x")), ("ONLY_EXAMPLE", Some("= 1"))]);
        assert_eq!(items[0].replace, Some(Range::new(Position::new(0, 14), Position::new(0, 16))));
        assert!(DotEnv::default().completion(&php, Position::new(0, 3), None).is_empty());
    }

    #[test]
    fn completes_a_dollar_in_a_quoted_value() {
        let _host = project();
        let env = doc("/p/.env", "dotenv", "URL=\"http://$DB\"\nRAW=$DB\n");
        let items = DotEnv::default().completion(&env, Position::new(0, 15), None);
        let db = items.iter().find(|i| i.label == "${DB_HOST}").unwrap();
        assert_eq!(db.filter_text.as_deref(), Some("$DB_HOST"));
        assert_eq!(db.replace, Some(Range::new(Position::new(0, 12), Position::new(0, 15))));
        assert!(DotEnv::default().completion(&env, Position::new(1, 7), None).is_empty(), "not outside quotes");
    }

    #[test]
    fn goes_to_the_dotenv_declaration_first() {
        let _host = project();
        let py = doc("/p/app.py", "python", "os.environ['DB_HOST']\nos.getenv('ONLY_EXAMPLE')");
        let targets = DotEnv::default().definition(&py, Position::new(0, 14));
        assert_eq!(targets, [Location { path: "/p/.env".into(), range: Range::new(Position::new(0, 0), Position::new(0, 7)) }]);
        let targets = DotEnv::default().definition(&py, Position::new(1, 12));
        assert_eq!(targets[0].path, "/p/.env.example");
    }

    #[test]
    fn goes_from_a_declaration_to_its_usages() {
        let _host = project();
        let env = doc("/p/.env", "dotenv", "DB_HOST=localhost\nURL=\"${DB_HOST}\"\n");
        let targets: Vec<_> = walked().definition(&env, Position::new(0, 3)).into_iter().map(|l| (l.path, l.range.start)).collect();
        assert_eq!(targets, [("/p/app.py".into(), Position::new(0, 19)), ("/p/.env".into(), Position::new(1, 7))]);
    }

    #[test]
    fn key_lines_get_one_marker_per_usage_and_other_env_file() {
        let _host = project();
        let env = doc("/p/.env", "dotenv", "DB_HOST=localhost\nURL=\"${DB_HOST}\"\nNOWHERE=1\n");
        assert!(DotEnv::default().line_markers(&env).is_empty(), "a cold cache gives no marker");

        let markers = walked().line_markers(&env);
        let targets: Vec<_> = markers.iter().map(|m| (m.line, m.target.clone().unwrap())).collect();
        let at = |path: &str, line, start, end| Location { path: path.into(), range: Range::new(Position::new(line, start), Position::new(line, end)) };
        assert_eq!(
            targets,
            [(0, at("/p/app.py", 0, 19, 26)), (0, at("/p/.env", 1, 7, 14)), (0, at("/p/.env.example", 0, 0, 7))],
            "URL and NOWHERE have no usage or other declaration"
        );
        assert!(markers.iter().all(|m| m.icon.as_deref() == Some(RELATED_MARKER)));
        assert!(include_str!("../plugin.toml").contains(&format!("icon = \"{RELATED_MARKER}\"")));
        assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join(RELATED_MARKER).is_file());
    }

    #[test]
    fn a_line_lists_at_most_200_targets_and_says_so() {
        let many = (0..250).map(|n| Location { path: format!("/p/{n}.py"), range: Range::new(Position::new(0, 0), Position::new(0, 1)) }).collect();
        let markers = related_markers(3, many);
        assert_eq!(markers.len(), 200);
        assert_eq!(markers[0].tooltip, "Usages and other env files (first 200 of 250)");
        let empty = Location { path: "/p/x.py".into(), range: Range::new(Position::new(0, 1), Position::new(0, 1)) };
        assert!(related_markers(0, vec![empty]).is_empty());
    }

    #[test]
    fn does_not_offer_unversioned_project_wide_rename_from_a_changed_line() {
        let _host=project();let env=doc("/p/.env","dotenv","DATABASE_HOST=localhost\nAPI_URL=\"${DB_HOST}\"\n");
        let actions=walked().code_actions(&env,Range::new(Position::new(0,5),Position::new(0,5)));
        assert!(actions.iter().all(|a| !a.title.starts_with("Rename")));
    }

    #[test]
    fn saved_files_update_the_index() {
        let host = project();
        let mut plugin = walked();
        host.add_file("worker.py", "os.getenv('API_URL')\n");
        plugin.handle_events(&[Event::FileSaved("/p/worker.py".into())], &host.invocation());
        let env = doc("/p/.env", "dotenv", "API_URL=x\n");
        assert_eq!(plugin.definition(&env, Position::new(0, 1))[0].path, "/p/worker.py");
    }

    #[test]
    fn gitignore_present_no_rewalk_on_save() {
        let host = project();
        host.add_file(".gitignore", "vendor/\nnode_modules/\n");
        let mut plugin = walked();
        assert!(plugin.projects["/p"].walked && plugin.projects["/p"].complete);
        host.add_file("late.py", "os.getenv('LATE_KEY')\n");
        plugin.apply_events(&[Event::FileSaved("/p/app.py".into())]);
        assert!(plugin.projects["/p"].usages.iter().all(|u| u.key != "LATE_KEY"), "no re-walk found the unevented file");
    }

    #[test]
    fn gitignore_present_keeps_undefined_variable_check() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let host = project();
        host.add_file(".gitignore", "vendor/\n");
        let mut plugin = walked();
        let env = doc("/p/.env", "dotenv", "B=\"${NOPE}\"\n");
        assert!(plugin.diagnostics(&env).iter().any(|p| p.code.as_deref() == Some("UndefinedNestedVariableInspection")));
    }

    #[test]
    fn removing_a_folder_drops_its_facts() {
        let host = project();
        host.add_file("sub/.env", "SUB_KEY=1\n");
        let mut plugin = walked();
        assert!(plugin.projects["/p"].declarations.iter().any(|d| d.key == "SUB_KEY"));
        plugin.apply_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { removed: vec!["/p/sub".into()], ..Default::default() })]);
        assert!(plugin.projects["/p"].declarations.iter().all(|d| d.key != "SUB_KEY"));
        assert!(plugin.projects["/p"].declarations.iter().any(|d| d.key == "API_URL"));
    }

    #[test]
    fn external_delete_drops_declarations() {
        let host = project();
        let mut plugin = walked();
        assert!(plugin.projects["/p"].declarations.iter().any(|d| d.key == "API_URL"));
        plugin.apply_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { removed: vec!["/p/.env".into()], ..Default::default() })]);
        assert!(plugin.projects["/p"].declarations.iter().all(|d| d.key != "API_URL"));
        host.add_file("worker.py", "os.getenv('W')\n");
        plugin.apply_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { created: vec!["/p/worker.py".into()], ..Default::default() })]);
        assert!(plugin.projects["/p"].usages.iter().any(|u| u.key == "W"));
    }

    #[test]
    fn activation_watches_env_compose_dockerfile_and_code_paths() {
        let host = project();
        DotEnv::default().activate(&Context::default()).ok();
        let globs = &host.path_subscriptions()["files-changed"];
        for expected in ["**/.env", "**/.env.*", "**/*.env", "**/Dockerfile", "**/compose.yaml", "**/*.py"] {
            assert!(globs.iter().any(|g| g == expected), "{expected}");
        }
    }

    #[test]
    fn unfinished_walk_is_continued_by_the_next_event_batch() {
        let host = project();
        let mut plugin = DotEnv::default();
        let (root, p) = project_of("/p/.env").unwrap();
        plugin.projects.insert(root.clone(), Index::full(&p, &root, Instant::now() - Duration::from_secs(1)));
        assert!(!plugin.projects["/p"].walked);
        host.add_file("late.py", "os.getenv('LATE_KEY')\n");
        plugin.apply_events(&[Event::FileSaved("/p/app.py".into())]);
        assert!(plugin.projects["/p"].walked);
        assert!(plugin.projects["/p"].usages.iter().any(|u| u.key == "LATE_KEY"));
    }

    #[test]
    fn denied_events_notify_once_that_changes_cannot_be_tracked() {
        let host = project();
        let host = host.with_capabilities(&["project.read"]);
        watch_files();
        watch_files();
        assert_eq!(host.notifications().len(), 1);
    }

    #[test]
    fn empty_secret_like_values_are_not_masked() {
        let host = project();
        host.add_file(".env", "DB_PASSWORD=\n");
        let php = doc("/p/index.php", "php", "<?php getenv('DB');");
        let items = DotEnv::default().completion(&php, Position::new(0, 16), None);
        assert!(items.iter().all(|i| i.detail.as_deref() != Some("= ••••")));
        assert!(!index::is_secret_like("DB_PASSWORD", ""));
    }

    #[test]
    fn owner_compose_reread_during_a_walk_is_not_added_twice() {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "X=1\n");
        host.add_file("compose.yaml", "services:\n  web:\n    build:\n      context: .\n      args:\n        A: ${X}\n    image: ${X}\n");
        host.add_file("Dockerfile", "FROM php\nARG A\n");
        let mut plugin = DotEnv::default();
        let (root, project) = project_of("/p/.env").unwrap();
        let mut index = Index::begin(&project, &root, Instant::now() + Duration::from_secs(60));
        index.update("/p/compose.yaml", Some(&project.read_file("/p/compose.yaml").unwrap()));
        plugin.projects.insert(root, index);
        plugin.apply_events(&[Event::FileSaved("/p/Dockerfile".into())]);
        assert!(plugin.projects["/p"].walked);
        let image_usages = plugin.projects["/p"].usages.iter().filter(|u| u.key == "X" && u.location.path == "/p/compose.yaml" && u.location.range.start.line == 6).count();
        assert_eq!(image_usages, 1);
    }

    #[test]
    fn over_budget_events_finish_on_the_next_batch_without_a_relist() {
        let host = project();
        let mut plugin = walked();
        host.add_file("unevented.py", "os.getenv('UNEVENTED')\n");
        host.add_file("worker.py", "os.getenv('W')\n");
        plugin.apply_events_before(&[Event::FileSaved("/p/worker.py".into())], Instant::now() - Duration::from_secs(1));
        assert!(plugin.projects["/p"].walked, "no re-walk is forced");
        assert!(plugin.projects["/p"].usages.iter().all(|u| u.key != "W"));
        plugin.apply_events(&[]);
        assert!(plugin.projects["/p"].usages.iter().any(|u| u.key == "W"));
        assert!(plugin.projects["/p"].usages.iter().all(|u| u.key != "UNEVENTED"), "never re-listed");
        assert!(plugin.backlog.is_empty());
    }

    #[test]
    fn secret_like_values_masked_in_completion() {
        let host = project();
        host.add_file(".env", "DB_HOST=localhost\nDB_PASSWORD=hunter2\nAPP_KEY=base64:x\n");
        let php = doc("/p/index.php", "php", "<?php getenv('DB');");
        let items = DotEnv::default().completion(&php, Position::new(0, 16), None);
        let detail = |label: &str| items.iter().find(|i| i.label == label).and_then(|i| i.detail.clone());
        assert_eq!(detail("DB_HOST").as_deref(), Some("= localhost"));
        assert_eq!(detail("DB_PASSWORD").as_deref(), Some("= ••••"));
        assert!(items.iter().all(|i| !i.detail.as_deref().unwrap_or("").contains("hunter2")));
    }

    #[test]
    fn show_value_command_reveals_the_value_under_the_caret() {
        let host = project();
        host.add_file(".env", "DB_PASSWORD=hunter2\n");
        host.open_editor("/p/app.py", "python", "os.environ['DB_PASSWORD']", &[(0, 15, 0, 15)]);
        let mut plugin = DotEnv::default();
        host.answer_next_prompt(Some(&["0"]));
        plugin.run_command("show-value", &host.invocation(), None).unwrap();
        assert!(host.notifications().is_empty());
        assert_eq!(host.opened_locations(), vec![("/p/.env".to_owned(), 0, 0)]);
    }

    #[test]
    fn show_value_without_editor_read_names_the_permission() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.set_project_root(Some("/p"));
        host.add_file(".env", "DB_PASSWORD=hunter2\n");
        host.open_editor("/p/app.py", "python", "os.environ['DB_PASSWORD']", &[(0, 15, 0, 15)]);
        let err = DotEnv::default().run_command("show-value", &host.invocation(), None).unwrap_err();
        assert!(matches!(err, Error::Invalid(m) if m.contains("editor.read permission") && m.contains(".env files")));
    }

    #[test]
    fn no_provider_panics_at_any_offset() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        use openidea_plugin::hover::HoverProvider;
        let _host = project();
        let mut plugin = walked();
        let seeds = ["{{ x }}{#}", "café", "<?php\nuse $😀", "é::class", "<!-- Paramètres --><a/>", "A=\"${DB_HOST}😀\"\nB='é'\n", "getenv('é😀');"];
        for seed in seeds {
            for (language, path) in [("dotenv", "/p/.env"), ("php", "/p/a.php"), ("yaml", "/p/compose.yaml")] {
                let document = doc(path, language, seed);
                plugin.diagnostics(&document);
                plugin.highlights(language, seed);
                openidea_plugin::semantic_tokens::SemanticTokens::semantic_tokens(&mut plugin, &document);
                for (offset, _) in seed.char_indices().chain([(seed.len(), ' ')]) {
                    let at = text::position(seed, offset);
                    plugin.completion(&document, at, None);
                    plugin.definition(&document, at);
                    plugin.hover(&document, at);
                    plugin.code_actions(&document, Range::new(at, at));
                    openidea_plugin::file_actions::FileActions::file_actions(&mut plugin, &document, Range::new(at, at));
                }
            }
        }
    }

    fn compose_project() -> MockHost {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "WEB_PORT=80\nUNUSED=1\nAPP_KEY=x\nDB_PASSWORD=y\nCOMPOSE_PROJECT_NAME=p\n");
        host.add_file("compose.yaml", "services:\n  web:\n    ports: [\"${WEB_PORT}:80\"]\n    image: ${IMAGE}\n    command: ${TAG:-x} ${MISSING}\n    environment:\n      - DB_PASSWORD\n");
        host.add_file("app.php", "<?php env('APP_KEY');");
        host
    }

    #[test]
    fn saving_a_build_dockerfile_refreshes_its_arg_links() {
        let host = MockHost::install();
        host.set_project_root(Some("/p"));
        host.add_file(".env", "PHP_VERSION=8\n");
        host.add_file("compose.yaml", "services:\n  web:\n    build:\n      context: .\n      args:\n        PHP: ${PHP_VERSION}\n");
        let mut plugin = walked();
        assert!(plugin.projects["/p"].arg_links.is_empty());
        host.add_file("Dockerfile", "FROM php\nARG PHP\n");
        plugin.apply_events(&[Event::FileSaved("/p/Dockerfile".into())]);
        assert_eq!(plugin.projects["/p"].arg_links.len(), 1);
        host.add_file("Dockerfile", "FROM php\n");
        plugin.apply_events(&[Event::FileSaved("/p/Dockerfile".into())]);
        assert!(plugin.projects["/p"].arg_links.is_empty());
    }

    #[test]
    fn compose_interpolations_link_to_dotenv_keys_with_info_hints_only() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let _host = compose_project();
        let mut plugin = walked();
        let text = host::project_of("/p/compose.yaml").unwrap().read_file("/p/compose.yaml").unwrap();
        let compose = doc("/p/compose.yaml", "yaml", &text);
        let hints: Vec<_> = plugin.diagnostics(&compose).into_iter().map(|d| (d.severity, d.message)).collect();
        assert_eq!(hints.len(), 2);
        assert!(hints.iter().all(|(s, _)| *s == Severity::Info));
        assert!(hints[0].1.contains("IMAGE") && hints[1].1.contains("MISSING"));

        let env = doc("/p/.env", "dotenv", "WEB_PORT=80\nUNUSED=1\nAPP_KEY=x\nDB_PASSWORD=y\nCOMPOSE_PROJECT_NAME=p\n");
        let unused: Vec<_> = plugin.diagnostics(&env).into_iter().map(|d| (d.severity, d.message)).collect();
        assert_eq!(unused.len(), 1);
        assert!(unused[0].0 == Severity::Info && unused[0].1.contains("UNUSED"));
        let usages: Vec<_> = plugin.definition(&env, Position::new(0, 2)).into_iter().map(|l| l.path).collect();
        assert_eq!(usages, ["/p/compose.yaml"]);

        plugin.projects.get_mut("/p").unwrap().complete = false;
        assert!(plugin.diagnostics(&compose).is_empty() && plugin.diagnostics(&env).is_empty());
    }

    #[test]
    fn env_file_targets_are_not_reported_unused() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let host = compose_project();
        host.add_file("compose.yaml", "services:\n  web:\n    env_file: .env\n");
        let env = doc("/p/.env", "dotenv", "UNUSED=1\n");
        assert!(walked().diagnostics(&env).is_empty());
    }

    #[test]
    fn publishes_problems_and_offers_their_fixes() {
        let _host = project();
        let env = doc("/p/.env", "dotenv", "A = 1\nB=\"${DB_HOST}${NOPE}\"\n");
        let actions = DotEnv::default().code_actions(&env, Range::new(Position::new(0, 0), Position::new(0, 0)));
        let titles: Vec<_> = actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Remove spaces around '='"]);
        let problems = openidea_plugin::diagnostics::DiagnosticsProvider::diagnostics(&mut walked(), &env);
        let codes: Vec<_> = problems.iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(codes, ["DotEnvSpaceAroundSeparatorInspection", "UndefinedNestedVariableInspection"]);
    }

    #[test]
    fn diagnostics_are_cache_only_and_absence_requires_a_complete_owned_cache() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let host = project();
        let env = doc("/p/.env", "dotenv", "A = 1\nB=\"${DB_HOST}${NOPE}\"\n");
        let mut cold = DotEnv::default();
        let problems = cold.diagnostics(&env);
        assert_eq!(problems.len(), 1, "local syntax still checked, absence unknown");
        assert_eq!(problems[0].code.as_deref(), Some("DotEnvSpaceAroundSeparatorInspection"));
        assert!(cold.projects.is_empty(), "diagnostics must not lazily probe root files");
        let mut warm = walked();
        assert!(warm.diagnostics(&env).iter().any(|p| p.code.as_deref() == Some("UndefinedNestedVariableInspection")));
        warm.projects.get_mut("/p").unwrap().complete = false;
        assert!(warm.diagnostics(&env).iter().all(|p| p.code.as_deref() != Some("UndefinedNestedVariableInspection")));
        let _denied = host.with_capabilities(&[]);
        assert_eq!(cold.diagnostics(&env).len(), 1);
        assert!(cold.projects.is_empty());
    }

    #[test]
    fn diagnostics_overlay_unsaved_declarations_and_bound_documents() {
        use openidea_plugin::diagnostics::DiagnosticsProvider;
        let _host = project();
        let mut plugin = walked();
        let env = doc("/p/.env", "dotenv", "LOCAL=1\nB=\"${LOCAL}\"\n");
        assert!(plugin.diagnostics(&env).is_empty());
        let other = doc("/other/.env", "dotenv", "B=\"${NOPE}\"\n");
        assert!(plugin.diagnostics(&other).is_empty(), "foreign project cannot prove absence");
        let huge = doc("/p/.env", "dotenv", &"#".repeat(index::MAX_SOURCE_BYTES + 1));
        assert!(plugin.diagnostics(&huge).is_empty());
    }

    #[test]
    fn highlights_keys_values_and_comments() {
        let kinds: Vec<_> = DotEnv::default().highlights("dotenv", "A=1 # c").into_iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [Highlight::Keyword, Highlight::Operator, Highlight::String, Highlight::Comment]);
    }
}

impl openidea_plugin::diagnostics::DiagnosticsProvider for DotEnv {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if doc.text.len() > index::MAX_SOURCE_BYTES {
            return Vec::new();
        }
        if index::is_compose_file(&doc.path) {
            return self.undefined_compose_variables(doc);
        }
        if !is_env_file(&doc.path) {
            return Vec::new();
        }
        let unused = self.unused_compose_keys(doc);
        self.inspect(&doc.path, &doc.text)
            .into_iter()
            .map(|p| {
                let severity = match p.level {
                    lint::Level::Error => Severity::Error,
                    lint::Level::Warning => Severity::Warning,
                };
                Diagnostic::new(range(&doc.text, p.range), severity, p.message).code(p.code)
            })
            .chain(unused)
            .collect()
    }
}

/// Cache-only Info hints about Compose interpolation. Compose also reads the
/// shell and `--env-file`, so these are never warnings and stay quiet unless
/// the cache is complete and holds the project-directory `.env`.
impl DotEnv {
    fn cached_index(&self, path: &str) -> Option<&Index> {
        let root = host::project_of(path).and_then(|project| project.root().ok())?;
        self.projects.get(&root).filter(|index| index.complete)
    }

    fn undefined_compose_variables(&self, doc: &Document) -> Vec<Diagnostic> {
        let Some(index) = self.cached_index(&doc.path) else {
            return Vec::new();
        };
        let env_path = format!("{}/.env", index::folder_of(&doc.path));
        let defined: HashSet<&str> = index.declarations.iter().filter(|d| d.location.path == env_path).map(|d| d.key.as_str()).collect();
        if defined.is_empty() {
            return Vec::new();
        }
        docker::compose_interpolations(&doc.text)
            .into_iter()
            .filter(|i| !i.has_operator && !defined.contains(i.name.as_str()) && !i.name.starts_with("COMPOSE_"))
            .map(|i| {
                let message = format!("`{}` is not set in the project-directory .env; Compose may still read it from the shell or --env-file.", i.name);
                Diagnostic::new(range(&doc.text, i.range), Severity::Info, message).code("ComposeUndefinedVariable")
            })
            .collect()
    }

    fn unused_compose_keys(&self, doc: &Document) -> Vec<Diagnostic> {
        if index::file_name(&doc.path) != ".env" {
            return Vec::new();
        }
        let Some(index) = self.cached_index(&doc.path) else {
            return Vec::new();
        };
        if !index.compose_folders.contains(index::folder_of(&doc.path)) || index.env_file_paths.contains(&doc.path) {
            return Vec::new();
        }
        let tokens = parse::lex(&doc.text);
        parse::properties(&doc.text, &tokens)
            .into_iter()
            .filter(|p| {
                let key = p.name(&doc.text);
                !key.starts_with("COMPOSE_")
                    && !key.starts_with("DOCKER_")
                    && !index.usages.iter().any(|u| u.key == key)
                    && !index.declarations.iter().any(|d| d.key == key && !is_env_file(&d.location.path))
            })
            .map(|p| {
                let message = format!("`{}` is not referenced by Compose interpolation, code or container environment.", p.name(&doc.text));
                Diagnostic::new(range(&doc.text, p.key), Severity::Info, message).code("ComposeUnusedKey")
            })
            .collect()
    }
}
