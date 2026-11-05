//! Properties: inspections, quick fixes, navigation, completion, formatting,
//! a structure view and a resource bundle editor for `.properties` files,
//! ported from JetBrains' Properties plugin (`com.intellij.properties`,
//! intellij-community `idea/263.7307.112`, Apache-2.0). The IDE already
//! highlights, comments and folds these files; this adds what the original
//! plugin computes on top of the PSI.

mod checks;
mod format;
mod props;
mod usages;
mod views;

use checks::{Settings, lint};
use format::{is_sorted, reformat_edits, sorted_text};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, Plugin, Position, Project, Range,
    Severity, TextEdit, TextEditor, host, register_plugin,
};
use props::{
    Prop, append_line, dir, file_name, locale_name, parse, range_of, split_locale, stem, suffix, unescape, whole_lines,
};
use std::collections::{HashMap, HashSet};
use std::ops::Range as Span;
use usages::{KeyIndex, implicitly_used, is_i18n_argument, literal_at, mark_used};
use views::{BUNDLE, BundleFiles, STRUCTURE};

fn project_for(path: &str) -> Result<Project, Error> {
    host::projects()?
        .into_iter()
        .find(|p| p.root().is_ok_and(|root| path.starts_with(&format!("{}/", root.trim_end_matches('/')))))
        .ok_or_else(|| format!("{path} isn't in an open project.").into())
}

/// The other files of the resource bundle: same directory, same base name.
// ponytail: walks the project with find_files on every call; cache per directory if large projects make it slow.
fn bundle_siblings(project: &Project, path: &str) -> Result<Vec<String>, Error> {
    let root = project.root()?;
    let base = split_locale(stem(path)).0;
    let rel_dir = dir(path).strip_prefix(root.trim_end_matches('/')).unwrap_or("").trim_start_matches('/');
    let glob = if rel_dir.is_empty() { format!("{base}*.properties") } else { format!("{rel_dir}/{base}*.properties") };

    Ok(project
        .find_files(&glob, 200)?
        .into_iter()
        .filter(|other| other != path && split_locale(stem(other)).0 == base)
        .collect())
}

/// This file (with `text`) and the other files of its bundle, with their text.
fn bundle_texts(path: &str, text: &str) -> Result<Vec<(String, String)>, Error> {
    let project = project_for(path)?;
    let mut files = vec![(path.to_owned(), text.to_owned())];
    for other in bundle_siblings(&project, path)? {
        files.push((other.clone(), project.read_file(&other)?));
    }
    Ok(files)
}

fn to_edits(text: &str, edits: Vec<(Span<usize>, String)>) -> Vec<TextEdit> {
    edits.into_iter().map(|(span, new)| TextEdit { range: range_of(text, &span), text: new }).collect()
}

fn properties_editor(ctx: &Invocation) -> Result<TextEditor, Error> {
    let editor = ctx.editor().ok_or("No file is open.")?;
    if editor.language() != "properties" {
        return Err("The open file isn't a .properties file.".into());
    }
    Ok(editor)
}

fn keys(text: &str) -> HashSet<String> {
    parse(text).iter().map(|p| p.unescaped_key(text)).collect()
}

/// `IncompletePropertyInspection`: a key missing from other locales of its bundle.
struct Missing {
    key: String,
    span: Span<usize>,
    /// (suffix, absolute path) of each file without the key.
    files: Vec<(String, String)>,
}

fn missing_translations(project: &Project, siblings: &[String], text: &str, props: &[Prop], settings: &Settings) -> Result<Vec<Missing>, Error> {
    let mut others = Vec::new();
    for other in siblings {
        let other_suffix = suffix(other).to_owned();
        if settings.ignored_suffixes.contains(&other_suffix) {
            continue;
        }
        let other_keys = keys(&project.read_file(other)?);
        others.push((other_suffix, other.clone(), other_keys));
    }

    Ok(props
        .iter()
        .filter_map(|p| {
            let key = p.unescaped_key(text);
            let files: Vec<(String, String)> = others
                .iter()
                .filter(|(_, _, other_keys)| !other_keys.contains(&key))
                .map(|(other_suffix, other, _)| (other_suffix.clone(), other.clone()))
                .collect();
            (!files.is_empty()).then(|| Missing { key, span: p.key.clone(), files })
        })
        .collect())
}

const SIMPLE_LOGGER_CONFIG: &str = "simplelogger.properties";

/// `SimpleLoggerPropertiesModel`: keys and their defaults.
const SIMPLE_LOGGER_KEYS: [(&str, &str); 10] = [
    ("org.slf4j.simpleLogger.logFile", "System.err"),
    ("org.slf4j.simpleLogger.cacheOutputStream", ""),
    ("org.slf4j.simpleLogger.defaultLogLevel", "info"),
    ("org.slf4j.simpleLogger.showDateTime", "false"),
    ("org.slf4j.simpleLogger.dateTimeFormat", ""),
    ("org.slf4j.simpleLogger.showShortLogName", "false"),
    ("org.slf4j.simpleLogger.showThreadName", "true"),
    ("org.slf4j.simpleLogger.showLogName", "true"),
    ("org.slf4j.simpleLogger.levelInBrackets", "false"),
    ("org.slf4j.simpleLogger.warnLevelString", "WARN"),
];

fn is_source_file(path: &str) -> bool {
    path.ends_with(".java") || path.ends_with(".kt")
}

#[derive(Default)]
pub struct Properties {
    /// Missing translations of the last inspection per file, for their quick fixes.
    missing: HashMap<String, Vec<Missing>>,
    /// Files of the bundle (this one included) at the last inspection, per file.
    locales: HashMap<String, usize>,
    /// Keys the last Find Unused Properties found no mention of, per file.
    unused: HashMap<String, HashSet<String>>,
    /// Keys of the project's `.properties` files, for Java/Kotlin strings.
    index: Option<KeyIndex>,
    bundle: Option<BundleFiles>,
    structure_flat: bool,
    structure_sorted: bool,
    /// Prefix groups of the structure view the user collapsed.
    structure_collapsed: HashSet<String>,
}

impl Properties {
    fn inspect(&mut self, path: &str) -> Result<(), Error> {
        let project = project_for(path)?;
        let text = project.read_file(path)?;
        let props = parse(&text);
        let settings = Settings::load();

        let mut diagnostics: Vec<Diagnostic> = lint(&text, &props, &settings)
            .into_iter()
            .map(|p| {
                let mut d = Diagnostic::new(range_of(&text, &p.span), p.severity, p.message);
                d.code = Some(p.code.into());
                d
            })
            .collect();

        let siblings = bundle_siblings(&project, path)?;
        let missing = missing_translations(&project, &siblings, &text, &props, &settings)?;
        for m in &missing {
            let suffixes: Vec<&str> = m.files.iter().map(|(s, _)| locale_name(s)).collect();
            let message = format!("Property '{}' has no translation in {}.", m.key, suffixes.join(", "));
            let mut d = Diagnostic::new(range_of(&text, &m.span), Severity::Error, message);
            d.code = Some("IncompleteProperty".into());
            diagnostics.push(d);
        }

        if let Some(unused) = self.unused.get(path) {
            for p in props.iter().filter(|p| unused.contains(&p.unescaped_key(&text))) {
                let mut d = Diagnostic::new(range_of(&text, &p.key), Severity::Warning, "Unused property");
                d.code = Some("UnusedProperty".into());
                d.unnecessary = true;
                diagnostics.push(d);
            }
        }

        host::publish_diagnostics(path, &diagnostics)?;
        self.missing.insert(path.to_owned(), missing);
        self.locales.insert(path.to_owned(), siblings.len() + 1);
        Ok(())
    }

    fn copy(&self, ctx: &Invocation, value: bool) -> Result<Option<String>, Error> {
        let editor = properties_editor(ctx)?;
        let text = editor.document()?.text;
        let caret = editor.selections()?.first().map(|s| s.start).unwrap_or_default();
        let at = offset(&text, caret).unwrap_or(text.len());
        let prop = parse(&text).into_iter().find(|p| p.contains(at)).ok_or("No property at the caret.")?;

        let key = prop.unescaped_key(&text);
        if value {
            host::copy_to_clipboard(&unescape(&text[prop.value]))?;
            host::notify(NotificationLevel::Info, "Properties", &format!("Value of '{key}' copied to the clipboard."));
        } else {
            host::copy_to_clipboard(&key)?;
            host::notify(NotificationLevel::Info, "Properties", &format!("Key '{key}' copied to the clipboard."));
        }
        Ok(None)
    }

    /// `UnusedPropertyInspection`, run on demand: keys of the open file that no
    /// other project file mentions.
    // ponytail: text search over at most 20 000 files; a key built at run time ("error." + code) looks unused, as in IntelliJ.
    fn find_unused(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = properties_editor(ctx)?;
        let path = editor.path();
        let text = editor.document()?.text;
        let project = project_for(&path)?;

        let mut unused = if implicitly_used(&path) { HashSet::new() } else { keys(&text) };
        let task = host::begin_task("Searching for unused properties", true);
        let files = project.find_files("**/*", 20_000)?;
        for (n, file) in files.iter().enumerate() {
            if unused.is_empty() {
                break;
            }
            if task.is_cancelled() {
                return Err(Error::Cancelled);
            }
            if file.ends_with(".properties") {
                continue;
            }
            task.report(n as u32, Some(files.len() as u32), Some(file_name(file)));
            if let Ok(source) = project.read_file(file) {
                mark_used(&source, &mut unused);
            }
        }

        let summary = match unused.len() {
            0 => format!("No unused properties in {}.", file_name(&path)),
            1 => format!("1 unused property in {}, marked as a warning in the file.", file_name(&path)),
            n => format!("{n} unused properties in {}, marked as warnings in the file.", file_name(&path)),
        };
        self.unused.insert(path.clone(), unused);
        self.inspect(&path)?;
        host::notify(NotificationLevel::Info, "Properties", &summary);
        Ok(None)
    }

    fn reformat(&self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = properties_editor(ctx)?;
        let doc = editor.document()?;
        let edits = reformat_edits(&doc.text, &Settings::load());
        if edits.is_empty() {
            host::notify(NotificationLevel::Info, "Properties", "The file is already formatted.");
            return Ok(None);
        }
        editor.apply_edits(doc.version, &to_edits(&doc.text, edits))?;
        Ok(None)
    }

    fn key_index(&mut self, path: &str) -> Option<&KeyIndex> {
        if self.index.is_none() {
            self.index = project_for(path).and_then(|p| KeyIndex::build(&p)).ok();
        }
        self.index.as_ref()
    }

    fn property_actions(&self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let text = &doc.text;
        let sel = offset(text, selection.start).unwrap_or(text.len())..offset(text, selection.end).unwrap_or(text.len());
        let touches = |span: &Span<usize>| span.start <= sel.end && sel.start <= span.end;
        let props = parse(text);
        let action = |title: &str, edits: Vec<(String, Vec<TextEdit>)>, data: Option<String>| CodeAction {
            title: title.into(),
            kind: Some("quickfix".into()),
            preferred: false,
            edits,
            data,
        };

        let mut actions: Vec<CodeAction> = lint(text, &props, &Settings::load())
            .into_iter()
            .filter(|p| touches(&p.span))
            .filter_map(|p| p.fix)
            .map(|fix| CodeAction { preferred: true, ..action(fix.title, vec![(doc.path.clone(), to_edits(text, fix.edits))], None) })
            .collect();

        for m in self.missing.get(&doc.path).into_iter().flatten().filter(|m| touches(&m.span)) {
            for (other_suffix, other) in &m.files {
                let title = format!("Add translation for {}", locale_name(other_suffix));
                actions.push(action(&title, Vec::new(), Some(format!("translate\n{other}\n{}", m.key))));
            }
        }

        if let Some(p) = props.iter().find(|p| p.contains(sel.start)) {
            let data = format!("remove\n{}", p.unescaped_key(text));
            let title = match self.locales.get(&doc.path) {
                Some(n) if *n > 1 => format!("Remove property from all {n} locales"),
                _ => "Remove property from all locales".to_owned(),
            };
            actions.push(action(&title, Vec::new(), Some(data)));
        }

        if !is_sorted(text, &props) {
            let sorted = vec![(doc.path.clone(), to_edits(text, vec![(0..text.len(), sorted_text(text, &Settings::load()))]))];
            actions.push(action("Sort properties file", sorted, None));
            actions.push(action("Sort resource bundle files", Vec::new(), Some("sort-bundle".into())));
        }

        actions
    }

    /// `AddMissingTranslationFix`: appends the property, with this file's value, to the other locale.
    fn add_translation(doc: &Document, target: &str, key: &str) -> Result<Vec<(String, Vec<TextEdit>)>, Error> {
        let text = &doc.text;
        let prop = parse(text)
            .into_iter()
            .find(|p| p.unescaped_key(text) == key)
            .ok_or_else(|| format!("'{key}' is no longer in {}.", file_name(&doc.path)))?;

        let target_text = project_for(target)?.read_file(target)?;
        let line = format!("{}{}{}", &text[prop.key.clone()], Settings::load().delimiter_text(), &text[prop.value.clone()]);
        Ok(vec![(target.to_owned(), to_edits(&target_text, vec![append_line(&target_text, &line)]))])
    }

    /// `RemovePropertyFromBundleLocalFix` / safe delete: the key goes from every locale.
    fn remove_everywhere(doc: &Document, key: &str) -> Result<Vec<(String, Vec<TextEdit>)>, Error> {
        Ok(bundle_texts(&doc.path, &doc.text)?
            .into_iter()
            .filter_map(|(path, text)| {
                let spans: Vec<(Span<usize>, String)> = parse(&text)
                    .iter()
                    .filter(|p| p.unescaped_key(&text) == key)
                    .map(|p| (whole_lines(&text, p), String::new()))
                    .collect();
                (!spans.is_empty()).then(|| (path, to_edits(&text, spans)))
            })
            .collect())
    }

    /// `PropertiesSorterQuickFix` for the bundle: every unsorted file, and this one.
    fn sort_bundle(doc: &Document) -> Result<Vec<(String, Vec<TextEdit>)>, Error> {
        let settings = Settings::load();
        Ok(bundle_texts(&doc.path, &doc.text)?
            .into_iter()
            .filter(|(path, text)| *path == doc.path || !is_sorted(text, &parse(text)))
            .map(|(path, text)| {
                let edits = to_edits(&text, vec![(0..text.len(), sorted_text(&text, &settings))]);
                (path, edits)
            })
            .collect())
    }

    fn logger_keys(text: &str, key: Span<usize>, with_delimiter: bool) -> Vec<CompletionItem> {
        let delimiter = if with_delimiter { Settings::load().delimiter_text() } else { String::new() };

        SIMPLE_LOGGER_KEYS
            .iter()
            .map(|(name, default)| {
                let mut item = CompletionItem::new(name, CompletionKind::Property);
                item.insert_text = format!("{name}{delimiter}");
                item.replace = Some(range_of(text, &key));
                item.detail = (!default.is_empty()).then(|| format!("={default}"));
                item
            })
            .collect()
    }
}

impl Plugin for Properties {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "copy-key" => self.copy(ctx, false),
            "copy-value" => self.copy(ctx, true),
            "reformat" => self.reformat(ctx),
            "find-unused" => self.find_unused(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        let mut inspect: Vec<&str> = Vec::new();
        let mut saved = false;
        let mut refresh = false;
        let mut source_opened = None;

        for event in events {
            match event {
                Event::FileOpened(p) if p.ends_with(".properties") => inspect.push(p),
                Event::FileSaved(p) if p.ends_with(".properties") => {
                    inspect.push(p);
                    saved = true;
                }
                Event::FileOpened(p) if is_source_file(p) => source_opened = Some(p.as_str()),
                Event::EditorFocused(_) => refresh = true,
                _ => {}
            }
        }
        inspect.sort();
        inspect.dedup();

        if saved {
            self.index = None;
            self.bundle = None;
            host::refresh_view(BUNDLE);
        }

        for path in &inspect {
            if let Err(e) = self.inspect(path) {
                host::log(LogLevel::Warn, &format!("inspecting {path}: {e}"));
            }
        }

        if refresh || !inspect.is_empty() {
            host::refresh_view(STRUCTURE);
        }

        // Built here, where there is time, so Java/Kotlin completion finds it ready.
        if let Some(path) = source_opened {
            self.key_index(path);
        }
    }
}

impl Providers for Properties {
    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        if doc.language != "properties" {
            return Vec::new();
        }
        self.property_actions(doc, selection)
    }

    fn resolve_code_action(&mut self, doc: &Document, mut action: CodeAction) -> Result<CodeAction, Error> {
        let data = action.data.clone().ok_or("nothing to resolve")?;
        let parts: Vec<&str> = data.splitn(3, '\n').collect();

        action.edits = match parts.as_slice() {
            ["translate", target, key] => Self::add_translation(doc, target, key)?,
            ["remove", key] => Self::remove_everywhere(doc, key)?,
            ["sort-bundle"] => Self::sort_bundle(doc)?,
            _ => return Err(Error::Invalid(format!("unknown action {data:?}"))),
        };
        Ok(action)
    }

    /// `GotoPropertyParentDeclarationHandler` in `.properties` files (the same
    /// key in the parent locale: `messages_fr_CA` → `messages_fr` → `messages`);
    /// the declarations of a key in a Java or Kotlin string.
    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        let text = &doc.text;
        let Some(at) = offset(text, at) else { return Vec::new() };

        if doc.language != "properties" {
            let Some(literal) = literal_at(text, at) else { return Vec::new() };
            let key = &text[literal];
            let Some(index) = self.key_index(&doc.path) else { return Vec::new() };
            return index.keys.get(key).into_iter().flatten().map(|d| d.location.clone()).collect();
        }

        let Some(prop) = parse(text).into_iter().find(|p| p.contains(at)) else { return Vec::new() };
        let key = prop.unescaped_key(text);
        let Ok(project) = project_for(&doc.path) else { return Vec::new() };

        let (base, mut locale) = split_locale(stem(&doc.path));
        while !locale.is_empty() {
            locale = locale.rfind('_').map_or("", |i| &locale[..i]);
            let parent_stem = if locale.is_empty() { base.to_owned() } else { format!("{base}_{locale}") };
            let parent = format!("{}/{parent_stem}.properties", dir(&doc.path));
            let Ok(parent_text) = project.read_file(&parent) else { continue };

            if let Some(p) = parse(&parent_text).into_iter().find(|p| p.unescaped_key(&parent_text) == key) {
                return vec![Location { path: parent, range: range_of(&parent_text, &p.key) }];
            }
        }

        Vec::new()
    }

    /// `SimpleLoggerPropertiesCompletionContributor` in `simplelogger.properties`;
    /// `PropertiesCompletionContributor` in the key argument of Java/Kotlin i18n calls.
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let text = &doc.text;
        let Some(at) = offset(text, at) else { return Vec::new() };

        if doc.language != "properties" {
            let Some(literal) = literal_at(text, at).filter(|l| is_i18n_argument(text, l)) else { return Vec::new() };
            let Some(index) = self.key_index(&doc.path) else { return Vec::new() };
            return index
                .keys
                .keys()
                .map(|key| {
                    let mut item = CompletionItem::new(key, CompletionKind::Property);
                    item.replace = Some(range_of(text, &literal));
                    item.detail = index.value(key).map(|v| format!("={v}"));
                    item
                })
                .collect();
        }

        if file_name(&doc.path) != SIMPLE_LOGGER_CONFIG {
            return Vec::new();
        }

        let Some(prop) = parse(text).into_iter().find(|p| p.contains(at)) else {
            return Self::logger_keys(text, at..at, true);
        };
        if at > prop.key.end {
            return Vec::new();
        }

        Self::logger_keys(text, prop.key.clone(), prop.sep.is_none() && prop.value.is_empty())
    }
}

register_plugin!(Properties, providers, views);

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    fn doc(path: &str, language: &str, text: &str) -> Document {
        Document { path: path.into(), language: language.into(), version: 1, text: text.into() }
    }

    fn caret(line: u32, character: u32) -> Range {
        Range::new(Position::new(line, character), Position::new(line, character))
    }

    fn titles(actions: &[CodeAction]) -> Vec<&str> {
        actions.iter().map(|a| a.title.as_str()).collect()
    }

    #[test]
    fn quick_fixes_edit_the_document() {
        let _host = MockHost::install();
        let doc = doc("/project/m.properties", "properties", "a=1\na=2\nb:3\n");

        let actions = Properties::default().code_actions(&doc, caret(1, 0));
        assert_eq!(titles(&actions), ["Remove property", "Remove property from all locales"]);

        let delimiter = Properties::default().code_actions(&doc, caret(2, 0)).remove(0);
        assert_eq!(delimiter.edits[0].1[0].text, "=");
        assert_eq!(delimiter.edits[0].1[0].range, Range::new(Position::new(2, 1), Position::new(2, 2)));
    }

    #[test]
    fn reports_and_adds_missing_translations() {
        let host = MockHost::install();
        host.add_file("/project/i18n/messages.properties", "hello=Hello\nbye=Bye\n");
        host.add_file("/project/i18n/messages_fr.properties", "hello=Bonjour");
        host.add_file("/project/i18n/messages_de.properties", "hello=Hallo\nbye=Tschüss\n");
        host.add_file("/project/i18n/other.properties", "");

        let mut plugin = Properties::default();
        let path = "/project/i18n/messages.properties";
        plugin.handle_events(&[Event::FileOpened(path.into())], &host.invocation());

        let diagnostics = &host.diagnostics()[path];
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "Property 'bye' has no translation in fr.");

        let doc = doc(path, "properties", "hello=Hello\nbye=Bye\n");
        let action = plugin.code_actions(&doc, caret(1, 1)).remove(0);
        assert_eq!(action.title, "Add translation for fr");

        let resolved = plugin.resolve_code_action(&doc, action).unwrap();
        let (target, edits) = &resolved.edits[0];
        assert_eq!(target, "/project/i18n/messages_fr.properties");
        assert_eq!(edits[0].text, "\nbye=Bye\n");
        assert_eq!(edits[0].range.start, Position::new(0, 13));
    }

    #[test]
    fn removes_a_key_from_every_locale() {
        let host = MockHost::install();
        host.add_file("/project/messages_fr.properties", "a=1\nb=2\n");
        host.add_file("/project/messages.properties", "b=x\na=y\n");
        let doc = doc("/project/messages.properties", "properties", "b=x\na=y\n");

        let mut plugin = Properties::default();
        plugin.handle_events(&[Event::FileOpened(doc.path.clone())], &host.invocation());
        let action = plugin.code_actions(&doc, caret(0, 0)).into_iter().find(|a| a.title.contains("all")).unwrap();
        assert_eq!(action.title, "Remove property from all 2 locales");
        let resolved = plugin.resolve_code_action(&doc, action).unwrap();

        let files: Vec<&str> = resolved.edits.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(files, ["/project/messages.properties", "/project/messages_fr.properties"]);
        assert_eq!(resolved.edits[1].1[0].range, Range::new(Position::new(1, 0), Position::new(2, 0)));
    }

    #[test]
    fn sorts_the_file_and_the_bundle() {
        let host = MockHost::install();
        host.add_file("/project/messages_fr.properties", "z=1\na=2\n");
        host.add_file("/project/messages_de.properties", "a=1\nz=2\n");
        let doc = doc("/project/messages.properties", "properties", "b=x\na=y\n");

        let mut plugin = Properties::default();
        let actions = plugin.code_actions(&doc, caret(0, 0));
        let sort = actions.iter().find(|a| a.title == "Sort properties file").unwrap();
        assert_eq!(sort.edits[0].1[0].text, "a=y\nb=x\n");

        let bundle = actions.into_iter().find(|a| a.title == "Sort resource bundle files").unwrap();
        let resolved = plugin.resolve_code_action(&doc, bundle).unwrap();
        let files: Vec<&str> = resolved.edits.iter().map(|(path, _)| path.as_str()).collect();
        assert_eq!(files, ["/project/messages.properties", "/project/messages_fr.properties"]);
    }

    #[test]
    fn goes_to_the_parent_locale() {
        let host = MockHost::install();
        host.add_file("/project/messages.properties", "# base\ngreeting=Hi\n");
        host.add_file("/project/messages_fr.properties", "other=x\n");
        let doc = doc("/project/messages_fr_CA.properties", "properties", "greeting=Allo\n");

        let targets = Properties::default().definition(&doc, Position::new(0, 3));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].path, "/project/messages.properties");
        assert_eq!(targets[0].range.start, Position::new(1, 0));
    }

    #[test]
    fn completes_simple_logger_keys() {
        let _host = MockHost::install();
        let doc = doc("/project/src/main/resources/simplelogger.properties", "properties", "org.slf4j.simpleLogger.def");

        let items = Properties::default().completion(&doc, Position::new(0, 26), None);
        let level = items.iter().find(|i| i.label == "org.slf4j.simpleLogger.defaultLogLevel").unwrap();
        assert_eq!(level.insert_text, "org.slf4j.simpleLogger.defaultLogLevel=");
        assert_eq!(level.detail.as_deref(), Some("=info"));
    }

    #[test]
    fn completes_and_resolves_keys_in_java_strings() {
        let host = MockHost::install();
        host.add_file("/project/src/messages.properties", "app.title=Title\n");
        host.add_file("/project/src/messages_fr.properties", "app.title=Titre\n");
        let mut plugin = Properties::default();

        let java = doc("/project/src/App.java", "java", "label(bundle.getString(\"app.ti\"));\n");
        let items = plugin.completion(&java, Position::new(0, 30), None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail.as_deref(), Some("=Title"));
        assert_eq!(items[0].replace, Some(Range::new(Position::new(0, 24), Position::new(0, 30))));

        let java = doc("/project/src/App.java", "java", "x = \"app.title\";\n");
        assert_eq!(plugin.definition(&java, Position::new(0, 7)).len(), 2);
    }

    #[test]
    fn finds_unused_properties() {
        let host = MockHost::install();
        host.add_file("/project/src/App.java", "bundle.getString(\"used.key\");");
        host.add_file("/project/src/other.properties", "unused.key=mentioned here only\n");
        let text = "used.key=1\nunused.key=2\n";
        host.add_file("/project/src/messages.properties", text);
        host.open_editor("/project/src/messages.properties", "properties", text, &[]);

        Properties::default().run_command("find-unused", &host.invocation(), None).unwrap();

        let diagnostics = host.diagnostics();
        let unused: Vec<&Diagnostic> = diagnostics["/project/src/messages.properties"].iter().filter(|d| d.unnecessary).collect();
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].range.start, Position::new(1, 0));
        assert_eq!(host.notifications()[0].body, "1 unused property in messages.properties, marked as a warning in the file.");
    }

    #[test]
    fn reformats_the_open_file() {
        let host = MockHost::install();
        host.open_editor("/project/a.properties", "properties", "  a = 1\n\n\nb:2\n", &[]);
        let mut plugin = Properties::default();
        plugin.run_command("reformat", &host.invocation(), None).unwrap();
        assert_eq!(host.editor_text(), "a=1\nb:2\n");
        assert!(host.notifications().is_empty(), "the edit shows it");

        plugin.run_command("reformat", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications()[0].body, "The file is already formatted.");
    }

    #[test]
    fn copies_key_and_value() {
        let host = MockHost::install();
        host.open_editor("/project/a.properties", "properties", "x=1\nmy\\ key = caf\\u00e9\n", &[(1, 2, 1, 2)]);
        let mut plugin = Properties::default();

        plugin.run_command("copy-key", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("my key"));

        plugin.run_command("copy-value", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("café"));

        let bodies: Vec<String> = host.notifications().into_iter().map(|n| n.body).collect();
        assert_eq!(bodies, ["Key 'my key' copied to the clipboard.", "Value of 'my key' copied to the clipboard."]);
    }
}
