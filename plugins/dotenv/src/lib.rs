//! `.env` files: highlighting, the original inspections with their quick
//! fixes, completion, go to declaration and find usages of environment
//! variables in `${…}` and in code, keys of Dockerfiles and docker-compose
//! files. A port of JetBrains' ".env files" plugin (`ru.adelf.idea.dotenv`, MIT).

mod code;
mod docker;
mod index;
mod lint;
mod parse;

use index::{Declaration, Index, Usage, env_declarations, is_env_file, nested_usages, range};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::syntax::{Highlight, SyntaxProvider, Token};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, Location, Plugin, Position, Project, Range, Severity,
    TextEdit, host, text,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// What an activation or a batch of events may spend walking projects (of their 5 s).
const SCAN_BUDGET: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct DotEnv {
    /// Each open project's index, by root.
    projects: HashMap<String, Index>,
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
    let key = if doc.language == "dotenv" {
        code::key_in_nested(line, offset - start)
    } else {
        code::key_in_code(line, offset - start)
    }?;
    Some(start + key.start..start + key.end)
}

/// `$KEY` being typed in a double-quoted value, from the `$`.
fn dollar_at(doc: &Document, at: Position) -> Option<std::ops::Range<usize>> {
    let offset = text::offset(&doc.text, at)?;
    let tokens = parse::lex(&doc.text);
    let quoted = parse::properties(&doc.text, &tokens).into_iter().any(|p| {
        let Some(value) = p.value.filter(|_| p.quote == Some('"')) else { return false };
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
    parse::properties(&doc.text, &tokens)
        .into_iter()
        .find(|p| p.key.start <= offset && offset <= p.key.end)
        .map(|p| p.name(&doc.text))
}

impl DotEnv {
    /// `path`'s project index: just the root's env files until the project is walked.
    fn index(&mut self, path: &str) -> Option<&Index> {
        let (root, project) = project_of(path)?;
        Some(self.projects.entry(root.clone()).or_insert_with(|| Index::quick(&project, &root)))
    }

    /// Walks the open projects not walked yet. Too slow for editor requests:
    /// only activation and events (5 s) do it.
    fn index_projects(&mut self) {
        let deadline = Instant::now() + SCAN_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let Ok(root) = project.root() else { continue };
            if !self.projects.get(&root).is_some_and(|index| index.complete) {
                let index = Index::full(&project, &root, deadline);
                self.projects.insert(root, index);
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
        let mut usages: Vec<Usage> = self
            .index(path)
            .map(|index| index.usages.iter().filter(|u| u.location.path != path).cloned().collect())
            .unwrap_or_default();
        usages.extend(nested_usages(path, unsaved));
        usages.into_iter().filter(|u| u.key == key).map(|u| u.location).collect()
    }

    /// Runs the inspections on `text` and shows their problems in the editor.
    fn inspect(&mut self, path: &str, text: &str) -> Vec<lint::Problem> {
        let known = self.known(path, Some(text));
        let tokens = parse::lex(text);
        let properties = parse::properties(text, &tokens);
        let problems = lint::check(text, &tokens, &properties, |key| known.iter().any(|d| d.key == key));
        let diagnostics: Vec<Diagnostic> = problems
            .iter()
            .map(|p| {
                let severity = match p.level {
                    lint::Level::Error => Severity::Error,
                    lint::Level::Warning => Severity::Warning,
                };
                let mut diagnostic = Diagnostic::new(range(text, p.range.clone()), severity, p.message.clone());
                diagnostic.code = Some(p.code.into());
                diagnostic
            })
            .collect();
        let _ = host::publish_diagnostics(path, &diagnostics);
        problems
    }

    /// The rename refactoring the original plugin has, after the fact: the key
    /// on `line` was edited (same value, same number of keys as the saved
    /// file, another name), so offer to update every usage of the old key.
    fn rename_usages(&mut self, doc: &Document, line: u32) -> Option<CodeAction> {
        let tokens = parse::lex(&doc.text);
        let properties = parse::properties(&doc.text, &tokens);
        let renamed = properties.iter().find(|p| range(&doc.text, p.key.clone()).start.line == line)?;
        let new = renamed.name(&doc.text);
        let index = self.index(&doc.path)?;
        let saved = index.declarations.iter().find(|d| d.location.path == doc.path && d.location.range.start.line == line)?;
        let old = saved.key.clone();
        let saved_keys = index.declarations.iter().filter(|d| d.location.path == doc.path).count();
        if old == new
            || saved.value != renamed.short_value()
            || saved_keys != properties.len()
            || properties.iter().any(|p| p.name(&doc.text) == old)
        {
            return None;
        }

        let mut edits: BTreeMap<String, Vec<TextEdit>> = BTreeMap::new();
        let elsewhere = index.usages.iter().filter(|u| u.location.path != doc.path).cloned();
        for usage in elsewhere.chain(nested_usages(&doc.path, &doc.text)).filter(|u| u.key == old) {
            edits.entry(usage.location.path).or_default().push(TextEdit { range: usage.location.range, text: new.clone() });
        }
        if edits.is_empty() {
            return None;
        }

        Some(CodeAction {
            title: format!("Rename usages of {old} to {new}"),
            kind: Some("refactor.rewrite".into()),
            preferred: false,
            edits: edits.into_iter().collect(),
            data: None,
        })
    }
}

impl Plugin for DotEnv {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        let _ = host::subscribe(EventKind::FileOpened);
        let _ = host::subscribe(EventKind::FileSaved);
        self.index_projects();
        Ok(())
    }

    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        self.index_projects();
        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            if !Index::indexes(path) {
                continue;
            }
            let Some((root, project)) = project_of(path) else { continue };
            let text = project.read_file(path).ok();
            if let Some(index) = self.projects.get_mut(&root) {
                index.update(path, text.as_deref());
                if let Some(text) = &text {
                    index.add_compose_env_files(&project, path, text);
                }
            }
            if let Some(text) = text.filter(|_| is_env_file(path)) {
                self.inspect(path, &text);
            }
        }
    }
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
                    item.detail = Some(format!("= {}", d.value));
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
        let Some(key) = key_at(doc, at) else { return Vec::new() };

        let key = &doc.text[key];
        let unsaved = (doc.language == "dotenv").then_some(doc.text.as_str());
        let matching: Vec<Declaration> = self.known(&doc.path, unsaved).into_iter().filter(|d| d.key == key).collect();
        let primary = matching.iter().any(|d| d.primary);

        matching.into_iter().filter(|d| d.primary == primary).map(|d| d.location).collect()
    }

    /// Also publishes the problems: the editor asks for code actions when it
    /// opens the file and as it changes, so they follow typing between saves.
    // ponytail: with no language server for .env, an empty answer comes back as `unavailable` and the
    // editor stops asking for 15 s; the host should treat a plugin's answer as available (upstream fix).
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let mut actions: Vec<CodeAction> = self
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
        actions.extend(self.rename_usages(doc, selection.start.line));
        actions
    }
}

// `register_plugin!` has no arm for syntax and providers together; this is
// its `syntax` arm plus the providers export, through the same SDK macros.
#[cfg(not(target_arch = "wasm32"))]
const _: fn() -> DotEnv = <DotEnv as Default>::default;
#[cfg(target_arch = "wasm32")]
const _: () = {
    use openidea_plugin::syntax_bindings::exports::openidea::plugin::syntax_provider as wit;

    struct Component;
    openidea_plugin::__plugin_exports!(DotEnv, Component, openidea_plugin::bindings);
    openidea_plugin::__providers_exports!(DotEnv, Component);

    impl wit::Guest for Component {
        fn highlights(language: String, text: String) -> Vec<wit::Token> {
            openidea_plugin::__rt::highlights::<DotEnv>(&language, &text)
        }

        fn outline(language: String, text: String) -> Vec<wit::Symbol> {
            openidea_plugin::__rt::outline::<DotEnv>(&language, &text)
        }

        fn folding(language: String, text: String) -> Vec<wit::Fold> {
            openidea_plugin::__rt::folding::<DotEnv>(&language, &text)
        }
    }

    openidea_plugin::bindings::__export_plugin_world!(Component with_types_in openidea_plugin::bindings);
    openidea_plugin::syntax_bindings::__export_syntax_world!(Component with_types_in openidea_plugin::syntax_bindings);
};

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
    fn completes_keys_in_code_with_the_primary_values() {
        let _host = project();
        let php = doc("/p/index.php", "php", "<?php getenv('DB');");
        let items = DotEnv::default().completion(&php, Position::new(0, 16), None);
        let labels: Vec<_> = items.iter().map(|i| (i.label.as_str(), i.detail.as_deref())).collect();
        assert_eq!(
            labels,
            [("DB_HOST", Some("= localhost")), ("API_URL", Some("= http://x")), ("ONLY_EXAMPLE", Some("= 1"))]
        );
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
        let targets: Vec<_> =
            walked().definition(&env, Position::new(0, 3)).into_iter().map(|l| (l.path, l.range.start)).collect();
        assert_eq!(targets, [("/p/app.py".into(), Position::new(0, 19)), ("/p/.env".into(), Position::new(1, 7))]);
    }

    #[test]
    fn offers_to_rename_the_usages_of_an_edited_key() {
        let _host = project();
        let env = doc("/p/.env", "dotenv", "DATABASE_HOST=localhost\nAPI_URL=\"${DB_HOST}\"\n");
        let actions = walked().code_actions(&env, Range::new(Position::new(0, 5), Position::new(0, 5)));
        let rename = actions.iter().find(|a| a.title == "Rename usages of DB_HOST to DATABASE_HOST").unwrap();
        let deleted = doc("/p/.env", "dotenv", "API_URL=\"http://x\"\n");
        let actions = walked().code_actions(&deleted, Range::new(Position::new(0, 1), Position::new(0, 1)));
        assert!(actions.iter().all(|a| !a.title.starts_with("Rename")), "a deleted line is no rename");
        let files: Vec<_> = rename.edits.iter().map(|(path, edits)| (path.as_str(), edits.len())).collect();
        assert_eq!(files, [("/p/.env", 1), ("/p/app.py", 1)]);
        assert_eq!(rename.edits[1].1[0].range, Range::new(Position::new(0, 19), Position::new(0, 26)));
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
    fn publishes_problems_and_offers_their_fixes() {
        let host = project();
        let env = doc("/p/.env", "dotenv", "A = 1\nB=\"${DB_HOST}${NOPE}\"\n");
        let actions = DotEnv::default().code_actions(&env, Range::new(Position::new(0, 0), Position::new(0, 0)));
        let titles: Vec<_> = actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Remove spaces around '='"]);
        let problems = &host.diagnostics()["/p/.env"];
        let codes: Vec<_> = problems.iter().map(|d| d.code.as_deref().unwrap()).collect();
        assert_eq!(codes, ["DotEnvSpaceAroundSeparatorInspection", "UndefinedNestedVariableInspection"]);
    }

    #[test]
    fn highlights_keys_values_and_comments() {
        let kinds: Vec<_> = DotEnv::default().highlights("dotenv", "A=1 # c").into_iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [Highlight::Keyword, Highlight::Operator, Highlight::String, Highlight::Comment]);
    }
}
