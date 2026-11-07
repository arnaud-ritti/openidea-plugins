//! Declarative views in place of the original's Swing UI: the structure view
//! (`PropertiesStructureViewModel`, keys grouped by dotted prefix) as a tool
//! window, and the resource bundle editor (`ResourceBundleEditor`, keys ×
//! locales) as a custom editor.

use crate::checks::Settings;
use crate::props::{append_line, escape_key, escape_value, file_name, locale_name, parse, suffix, unescape, whole_lines};
use crate::{Properties, bundle_siblings, project_for, to_edits};
use openidea_plugin::text::position;
use openidea_plugin::views::{Body, Button, Table, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{Error, EventKind, Invocation, NotificationLevel, host};
use std::collections::{HashMap, HashSet};
use std::ops::Range as Span;

pub const STRUCTURE: &str = "structure";
pub const BUNDLE: &str = "bundle";

struct Entry {
    /// Earlier properties of the file with the same key.
    occurrence: usize,
    key: String,
    value: String,
}

fn with_parent(item: TreeItem, parent: Option<&str>) -> TreeItem {
    match parent {
        Some(parent) => item.parent(parent),
        None => item,
    }
}

fn leaf(entry: &Entry, label: &str, parent: Option<&str>) -> TreeItem {
    let item = TreeItem::new(&format!("p:{}:{}", entry.occurrence, entry.key), label).icon("property").detail(&entry.value);
    with_parent(item, parent)
}

/// `PropertiesPrefixGroup`: keys sharing a dotted prefix with at least one
/// other key go under a node for that prefix, recursively.
fn group_by_prefix(entries: &[&Entry], consumed: usize, parent: Option<&str>, collapsed: &HashSet<String>, out: &mut Vec<TreeItem>) {
    let mut buckets: Vec<(&str, Vec<&Entry>)> = Vec::new();
    for &entry in entries {
        let segment = entry.key[consumed..].split('.').next().unwrap_or_default();
        match buckets.iter_mut().find(|(s, _)| *s == segment) {
            Some((_, members)) => members.push(entry),
            None => buckets.push((segment, vec![entry])),
        }
    }

    for (segment, members) in buckets {
        let end = consumed + segment.len();
        let (deeper, here): (Vec<&Entry>, Vec<&Entry>) = members.iter().copied().partition(|e| e.key.len() > end);
        if deeper.len() < 2 {
            out.extend(members.iter().map(|e| leaf(e, &e.key[consumed..], parent)));
            continue;
        }

        let group = format!("g:{}", &deeper[0].key[..end]);
        out.extend(here.iter().map(|e| leaf(e, &e.key[consumed..], parent)));
        out.push(with_parent(TreeItem::new(&group, segment).icon("package").expandable(!collapsed.contains(&group)), parent));
        group_by_prefix(&deeper, end + 1, Some(&group), collapsed, out);
    }
}

/// The other files of a bundle, read once per editor (until a save or Reload).
pub struct BundleFiles {
    pub path: String,
    pub others: Vec<(String, String)>,
}

/// The bundle editor's table: a row per key of any locale, a column per file
/// (this one first).
pub struct Grid {
    pub columns: Vec<String>,
    pub files: Vec<String>,
    pub keys: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub missing: usize,
    pub gaps: Vec<usize>,
}

pub fn grid(path: &str, text: &str, others: &[(String, String)]) -> Grid {
    let files: Vec<(&str, &str)> =
        std::iter::once((path, text)).chain(others.iter().map(|(p, t)| (p.as_str(), t.as_str()))).collect();

    let mut keys: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut values: Vec<HashMap<String, String>> = Vec::new();
    for (_, file_text) in &files {
        let mut file_values = HashMap::new();
        for p in parse(file_text) {
            let key = p.unescaped_key(file_text);
            if seen.insert(key.clone()) {
                keys.push(key.clone());
            }
            file_values.entry(key).or_insert_with(|| unescape(&file_text[p.value.clone()]));
        }
        values.push(file_values);
    }

    let columns = std::iter::once("Key".to_owned())
        .chain(files.iter().enumerate().map(|(i, (file, _))| {
            let locale = locale_name(suffix(file));
            if i == 0 { format!("{locale} (this file)") } else { locale.to_string() }
        }))
        .collect();

    let rows = keys
        .iter()
        .map(|key| std::iter::once(key.clone()).chain(values.iter().map(|v| v.get(key).cloned().unwrap_or_default())).collect())
        .collect();

    let gaps: Vec<usize> = keys.iter().map(|key| values.iter().filter(|v| !v.contains_key(key)).count()).collect();
    let missing = gaps.iter().sum();

    Grid { columns, files: files.iter().map(|(p, _)| (*p).to_owned()).collect(), keys, rows, missing, gaps }
}

impl Grid {
    fn keep_incomplete(mut self) -> Self {
        let keep: Vec<bool> = self.gaps.iter().map(|g| *g > 0).collect();
        let mut flags = keep.iter();
        self.keys.retain(|_| *flags.next().unwrap_or(&false));
        let mut flags = keep.iter();
        self.rows.retain(|_| *flags.next().unwrap_or(&false));
        self.gaps.retain(|g| *g > 0);
        self
    }
}

/// What a table edit does to this file: cells of the key and of this file's
/// column, inserted and deleted rows. Other locales are only shown.
pub fn bundle_edits(text: &str, grid: &Grid, event: &ViewEvent, settings: &Settings) -> Result<Vec<(Span<usize>, String)>, Error> {
    let props = parse(text);
    let declared = |key: &str| props.iter().filter(|p| p.unescaped_key(text) == key).collect::<Vec<_>>();
    let line = |key: &str, value: &str| format!("{}{}{}", escape_key(key), settings.delimiter_text(), escape_value(value));

    match event {
        ViewEvent::Cell { row, column, value } => {
            let key = grid.keys.get(*row as usize).ok_or("No such row.")?;
            match column {
                0 => {
                    if value.is_empty() {
                        return Err("A key can't be empty.".into());
                    }
                    let found = declared(key);
                    if found.is_empty() {
                        return Err(format!("'{key}' isn't in this file.").into());
                    }
                    Ok(found.iter().map(|p| (p.key.clone(), escape_key(value))).collect())
                }
                1 => Ok(vec![match declared(key).first() {
                    Some(p) if p.sep.is_some() => (p.value.clone(), escape_value(value)),
                    Some(p) => (p.key.end..p.value.end, format!("{}{}", settings.delimiter_text(), escape_value(value))),
                    None => append_line(text, &line(key, value)),
                }]),
                column => {
                    let other = grid.files.get(*column as usize - 1).ok_or("No such column.")?;
                    Err(format!("Open {} to edit its translations.", file_name(other)).into())
                }
            }
        }
        ViewEvent::InsertRow(before) => {
            let mut key = "new.property".to_owned();
            for n in 2.. {
                if !grid.keys.contains(&key) {
                    break;
                }
                key = format!("new.property.{n}");
            }
            let at = grid.keys.get(*before as usize).and_then(|k| declared(k).first().map(|p| whole_lines(text, p).start));
            Ok(vec![match at {
                Some(at) => (at..at, format!("{}\n", line(&key, ""))),
                None => append_line(text, &line(&key, "")),
            }])
        }
        ViewEvent::DeleteRows(rows) => {
            let keys: HashSet<&String> = rows.iter().filter_map(|r| grid.keys.get(*r as usize)).collect();
            Ok(props.iter().filter(|p| keys.contains(&p.unescaped_key(text))).map(|p| (whole_lines(text, p), String::new())).collect())
        }
        _ => Ok(Vec::new()),
    }
}

impl Properties {
    fn bundle_files(&mut self, path: &str) -> Result<&BundleFiles, Error> {
        if self.bundle.as_ref().is_none_or(|b| b.path != path) {
            let project = project_for(path)?;
            let mut others: Vec<(String, String)> = bundle_siblings(&project, path)?
                .into_iter()
                .filter_map(|other| project.read_file(&other).ok().map(|text| (other, text)))
                .collect();
            others.sort_by(|a, b| suffix(&a.0).cmp(suffix(&b.0)));
            self.bundle = Some(BundleFiles { path: path.to_owned(), others });
        }
        self.bundle.as_ref().ok_or_else(|| "no bundle".into())
    }

    fn bundle_grid(&mut self, path: &str, text: &str) -> Result<Grid, Error> {
        let grid = grid(path, text, &self.bundle_files(path)?.others);
        Ok(if self.bundle_incomplete_only { grid.keep_incomplete() } else { grid })
    }

    fn render_structure(&mut self, ctx: &Invocation) -> Result<View, Error> {
        let _ = host::subscribe(EventKind::EditorFocused);

        let group = if self.structure_flat { "Group by Prefix" } else { "Show Flat" };
        let sort = if self.structure_sorted { "Keep File Order" } else { "Sort Alphabetically" };
        let grouped = !self.structure_flat;
        let toolbar = |view: View| {
            view.button(Button::new("group", group).icon("package"))
                .button(Button::new("sort", sort).icon("sort"))
                .button(Button::new("expand-all", "Expand All").icon("expand-all").enabled(grouped))
                .button(Button::new("collapse-all", "Collapse All").icon("collapse-all").enabled(grouped))
                .button(Button::new("refresh", "Refresh").icon("refresh"))
        };

        let Some(editor) = ctx.editor().filter(|e| e.language() == "properties") else {
            return Ok(toolbar(View::message("Open a .properties file to see its properties.")));
        };

        let text = editor.document()?.text;
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut entries: Vec<Entry> = parse(&text)
            .iter()
            .map(|p| {
                let key = p.unescaped_key(&text);
                let occurrence = seen.entry(key.clone()).or_default();
                *occurrence += 1;
                Entry {
                    occurrence: *occurrence - 1,
                    key,
                    value: unescape(&text[p.value.clone()]).replace(['\n', '\r'], " ").chars().take(80).collect(),
                }
            })
            .collect();
        if entries.is_empty() {
            return Ok(toolbar(View::message("No properties in this file.")));
        }
        if self.structure_sorted {
            entries.sort_by_key(|e| e.key.to_lowercase());
        }

        let mut items = Vec::new();
        if self.structure_flat {
            items.extend(entries.iter().map(|e| leaf(e, &e.key, None)));
        } else {
            group_by_prefix(&entries.iter().collect::<Vec<_>>(), 0, None, &self.structure_collapsed, &mut items);
        }

        let count = match entries.len() {
            1 => "1 property".to_owned(),
            n => format!("{n} properties"),
        };
        let status = format!("{} · {count}", file_name(&editor.path()));
        Ok(toolbar(View::new(Body::Tree(items))).status(&status))
    }

    fn handle_structure(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Button(id) => {
                match id.as_str() {
                    "group" => self.structure_flat = !self.structure_flat,
                    "sort" => self.structure_sorted = !self.structure_sorted,
                    "expand-all" => self.structure_collapsed.clear(),
                    "collapse-all" => {
                        self.structure_collapsed.clear();
                        if let Body::Tree(items) = self.render_structure(ctx)?.body {
                            self.structure_collapsed = items.into_iter().map(|i| i.key).filter(|k| k.starts_with("g:")).collect();
                        }
                    }
                    _ => {}
                }
                host::refresh_view(STRUCTURE);
            }
            ViewEvent::Expand(key) => {
                self.structure_collapsed.remove(&key);
                host::refresh_view(STRUCTURE);
            }
            ViewEvent::Collapse(key) => {
                self.structure_collapsed.insert(key);
                host::refresh_view(STRUCTURE);
            }
            ViewEvent::Activate(key) => {
                let Some((occurrence, key)) = key.strip_prefix("p:").and_then(|k| k.split_once(':')) else { return Ok(()) };
                let Ok(occurrence) = occurrence.parse::<usize>() else { return Ok(()) };
                let editor = ctx.editor().ok_or("No file is open.")?;
                let text = editor.document()?.text;
                let p = parse(&text)
                    .into_iter()
                    .filter(|p| p.unescaped_key(&text) == key)
                    .nth(occurrence)
                    .ok_or("The file has changed. Refresh the view.")?;
                let at = position(&text, p.key.start);
                host::open_location(&editor.path(), at.line, at.character)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn render_bundle(&mut self, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let editor = ctx.editor().ok_or("The Resource Bundle editor needs an open file.")?;
        let path = editor.path();
        let text = editor.document()?.text;
        let grid = self.bundle_grid(&path, &text)?;

        let total = grid.keys.len() as u32;
        let shown = Viewport::range(viewport, total, 200);
        let table = Table {
            columns: grid.columns,
            total_rows: total,
            first_row: shown.start,
            rows: grid.rows[shown.start as usize..shown.end as usize].to_vec(),
            editable: true,
        };

        let filter = if self.bundle_incomplete_only { "Show All Keys" } else { "Show Incomplete Only" };
        let mut status = format!("{total} keys · {} locales · {} missing translations", grid.files.len(), grid.missing);
        if grid.files.len() > 1 {
            status.push_str(" · Editing another locale's cell writes its file");
        }
        Ok(View::new(Body::Table(table))
            .button(Button::new("reload", "Reload Locales").icon("refresh"))
            .button(Button::new("incomplete", filter).icon("filter"))
            .status(&status))
    }

    fn write_locale(&mut self, grid: &Grid, event: &ViewEvent, row: u32, column: u32) -> Result<(), Error> {
        if !host::setting_bool("bundle.writeOtherLocales").unwrap_or(true) {
            return self.open_locale(grid, row, column);
        }
        let ViewEvent::Cell { value, .. } = event else { return Ok(()) };
        let other = grid.files.get(column as usize - 1).ok_or("No such column.")?.clone();
        let project = project_for(&other)?;
        let text = project.read_file(&other)?;
        let edit_event = ViewEvent::Cell { row, column: 1, value: value.clone() };
        let mut edits = bundle_edits(&text, grid, &edit_event, &Settings::load())?;
        edits.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut updated = text;
        for (span, replacement) in edits {
            updated.replace_range(span, &replacement);
        }

        let written = host::run_process("tee", &[other.clone()], Some(&updated))
            .and_then(|out| if out.status == Some(0) { Ok(()) } else { Err(Error::Invalid(out.stderr)) });
        match written {
            Ok(()) => {
                self.bundle = None;
                host::refresh_view(BUNDLE);
                Ok(())
            }
            Err(Error::Denied(_)) => self.open_locale(grid, row, column),
            Err(e) => Err(e),
        }
    }

    fn open_locale(&self, grid: &Grid, row: u32, column: u32) -> Result<(), Error> {
        let key = grid.keys.get(row as usize).ok_or("No such row.")?;
        let other = grid.files.get(column as usize - 1).ok_or("No such column.")?;
        let text = project_for(other)?.read_file(other)?;
        let line = parse(&text)
            .iter()
            .find(|p| p.unescaped_key(&text) == *key)
            .map_or_else(|| text.lines().count() as u32, |p| position(&text, p.key.start).line);
        host::open_location(other, line, 0)
    }

    fn handle_bundle(&mut self, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if matches!(&event, ViewEvent::Button(id) if id == "reload") {
            self.bundle = None;
            host::refresh_view(BUNDLE);
            return Ok(());
        }

        if matches!(&event, ViewEvent::Button(id) if id == "incomplete") {
            self.bundle_incomplete_only = !self.bundle_incomplete_only;
            host::refresh_view(BUNDLE);
            return Ok(());
        }

        let editor = ctx.editor().ok_or("The Resource Bundle editor needs an open file.")?;
        let path = editor.path();
        let doc = editor.document()?;
        let grid = self.bundle_grid(&path, &doc.text)?;

        if let ViewEvent::Cell { row, column, .. } = &event {
            if *column >= 2 {
                return self.write_locale(&grid, &event, *row, *column);
            }
        }

        let edits = bundle_edits(&doc.text, &grid, &event, &Settings::load())?;
        if !edits.is_empty() {
            editor.apply_edits(doc.version, &to_edits(&doc.text, edits))?;
        }

        if let ViewEvent::DeleteRows(rows) = &event {
            let others = &self.bundle_files(&path)?.others;
            let elsewhere = rows
                .iter()
                .filter_map(|r| grid.keys.get(*r as usize))
                .any(|key| others.iter().any(|(_, text)| parse(text).iter().any(|p| p.unescaped_key(text) == *key)));
            if elsewhere {
                host::notify(
                    NotificationLevel::Info,
                    "Properties",
                    "Removed from this file only. To delete it everywhere, use Remove property from all locales.",
                );
            }
        }
        Ok(())
    }
}

impl Views for Properties {
    fn render(&mut self, view: &str, viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        match view {
            STRUCTURE => self.render_structure(ctx),
            BUNDLE => self.render_bundle(viewport, ctx),
            _ => Err(Error::NotFound(view.into())),
        }
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        match view {
            STRUCTURE => self.handle_structure(event, ctx),
            BUNDLE => self.handle_bundle(event, ctx),
            _ => Err(Error::NotFound(view.into())),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    #[test]
    fn groups_keys_by_prefix() {
        let host = MockHost::install();
        host.open_editor(
            "/project/m.properties",
            "properties",
            "app.title=T\napp.name=N\napp.menu.file=F\napp.menu.edit=E\nother=O\n",
            &[],
        );

        let view = Properties::default().render(STRUCTURE, None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("a tree") };
        let shape: Vec<(String, Option<String>)> = items.iter().map(|i| (i.label.clone(), i.parent.clone())).collect();
        let g = |s: &str| Some(s.to_owned());

        assert_eq!(
            shape,
            [
                ("app".into(), None),
                ("title".into(), g("g:app")),
                ("name".into(), g("g:app")),
                ("menu".into(), g("g:app")),
                ("file".into(), g("g:app.menu")),
                ("edit".into(), g("g:app.menu")),
                ("other".into(), None),
            ]
        );
    }

    #[test]
    fn opens_the_activated_property() {
        let host = MockHost::install();
        host.open_editor("/project/m.properties", "properties", "a=1\nb=2\n", &[]);

        Properties::default().handle(STRUCTURE, ViewEvent::Activate("p:0:b".into()), &host.invocation()).unwrap();
        host.open_editor("/project/m.properties", "properties", "b=2\na=1\na=3\n", &[]);
        Properties::default().handle(STRUCTURE, ViewEvent::Activate("p:1:a".into()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/m.properties".to_owned(), 1, 0), ("/project/m.properties".to_owned(), 2, 0)]);
    }

    #[test]
    fn collapses_and_expands_prefix_groups() {
        let host = MockHost::install();
        host.open_editor("/project/m.properties", "properties", "app.a=1\napp.b=2\napp.menu.x=3\napp.menu.y=4\n", &[]);
        let mut plugin = Properties::default();
        let ctx = host.invocation();
        let expanded = |plugin: &mut Properties| -> Vec<(String, bool)> {
            let Body::Tree(items) = plugin.render(STRUCTURE, None, &ctx).unwrap().body else { panic!("a tree") };
            items.into_iter().filter(|i| i.expandable).map(|i| (i.key, i.expanded)).collect()
        };
        let state = |app, menu| vec![("g:app".to_owned(), app), ("g:app.menu".to_owned(), menu)];
        assert_eq!(expanded(&mut plugin), state(true, true));

        plugin.handle(STRUCTURE, ViewEvent::Collapse("g:app.menu".into()), &ctx).unwrap();
        assert_eq!(expanded(&mut plugin), state(true, false));
        plugin.handle(STRUCTURE, ViewEvent::Expand("g:app.menu".into()), &ctx).unwrap();
        assert_eq!(expanded(&mut plugin), state(true, true));
        plugin.handle(STRUCTURE, ViewEvent::Button("collapse-all".into()), &ctx).unwrap();
        assert_eq!(expanded(&mut plugin), state(false, false));
        plugin.handle(STRUCTURE, ViewEvent::Button("expand-all".into()), &ctx).unwrap();
        assert_eq!(expanded(&mut plugin), state(true, true));
        assert_eq!(host.refreshed_views().len(), 4);
    }

    #[test]
    fn structure_status_and_empty_file() {
        let host = MockHost::install();
        host.open_editor("/project/m.properties", "properties", "a=1\n", &[]);
        let view = Properties::default().render(STRUCTURE, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("m.properties · 1 property"));

        host.open_editor("/project/m.properties", "properties", "# nothing yet\n", &[]);
        let view = Properties::default().render(STRUCTURE, None, &host.invocation()).unwrap();
        assert_eq!(view.body, Body::Message("No properties in this file.".into()));
        assert!(!view.toolbar.is_empty());
    }

    #[test]
    fn shows_and_edits_the_bundle() {
        let host = MockHost::install();
        host.add_file("/project/messages_fr.properties", "hello=Bonjour\nextra=En plus\n");
        host.open_editor("/project/messages.properties", "properties", "hello=Hello\nbye=Bye\n", &[]);
        let mut plugin = Properties::default();

        let view = plugin.render(BUNDLE, None, &host.invocation()).unwrap();
        let Body::Table(table) = view.body else { panic!("a table") };
        assert_eq!(table.columns, ["Key", "default (this file)", "fr"]);
        assert_eq!(table.rows[2], ["extra", "", "En plus"]);
        assert_eq!(
            view.status.as_deref(),
            Some("3 keys · 2 locales · 2 missing translations · Editing another locale's cell writes its file")
        );

        let cell = |row, column, value: &str| ViewEvent::Cell { row, column, value: value.into() };
        plugin.handle(BUNDLE, cell(1, 1, "Goodbye"), &host.invocation()).unwrap();
        plugin.handle(BUNDLE, cell(2, 1, "Extra"), &host.invocation()).unwrap();
        plugin.handle(BUNDLE, cell(0, 0, "greeting"), &host.invocation()).unwrap();
        assert_eq!(host.editor_text(), "greeting=Hello\nbye=Goodbye\nextra=Extra\n");

        plugin.handle(BUNDLE, cell(2, 2, "x"), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/messages_fr.properties".to_owned(), 1, 0)], "tee is not granted here");

        plugin.handle(BUNDLE, ViewEvent::InsertRow(1), &host.invocation()).unwrap();
        plugin.handle(BUNDLE, ViewEvent::DeleteRows(vec![0, 3]), &host.invocation()).unwrap();
        assert_eq!(host.editor_text(), "new.property=\nbye=Goodbye\n");
        assert_eq!(host.notifications().len(), 1, "extra stays in fr");

        plugin.handle(BUNDLE, ViewEvent::DeleteRows(vec![1]), &host.invocation()).unwrap();
        assert_eq!(host.editor_text(), "new.property=\n");
        assert_eq!(host.notifications().len(), 1, "bye was only in this file");
    }

    #[test]
    fn filters_the_bundle_to_incomplete_keys() {
        let host = MockHost::install();
        host.add_file("/project/messages_fr.properties", "hello=Bonjour\n");
        host.open_editor("/project/messages.properties", "properties", "hello=Hello\nbye=Bye\n", &[]);
        let mut plugin = Properties::default();
        let ctx = host.invocation();

        plugin.handle(BUNDLE, ViewEvent::Button("incomplete".into()), &ctx).unwrap();
        let Body::Table(table) = plugin.render(BUNDLE, None, &ctx).unwrap().body else { panic!("a table") };
        assert_eq!(table.rows, [["bye", "Bye", ""]]);

        plugin.handle(BUNDLE, ViewEvent::Cell { row: 0, column: 1, value: "Ciao".into() }, &ctx).unwrap();
        assert_eq!(host.editor_text(), "hello=Hello\nbye=Ciao\n");
    }

    #[test]
    fn deleting_a_row_kept_by_other_locales_says_so() {
        let host = MockHost::install();
        host.add_file("/project/messages_fr.properties", "hello=Bonjour\n");
        host.open_editor("/project/messages.properties", "properties", "hello=Hello\n", &[]);
        let mut plugin = Properties::default();

        plugin.handle(BUNDLE, ViewEvent::DeleteRows(vec![0]), &host.invocation()).unwrap();
        assert_eq!(host.editor_text(), "");
        assert_eq!(
            host.notifications()[0].body,
            "Removed from this file only. To delete it everywhere, use Remove property from all locales."
        );
    }

    fn tee_host() -> (MockHost, std::rc::Rc<std::cell::RefCell<String>>) {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "editor.write", "process:tee"]);
        let written = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        let sink = written.clone();
        host.on_process("tee", move |_, stdin| {
            *sink.borrow_mut() = stdin.unwrap_or_default().to_owned();
            Ok(openidea_plugin::ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
        });
        host.add_file("/project/messages_fr.properties", "hello=Bonjour\n");
        host.open_editor("/project/messages.properties", "properties", "hello=Hello\nbye=Bye\n", &[]);
        (host, written)
    }

    #[test]
    fn editing_another_locale_cell_writes_that_file_with_tee() {
        let (host, written) = tee_host();
        let mut plugin = Properties::default();
        let cell = |row, value: &str| ViewEvent::Cell { row, column: 2, value: value.into() };

        plugin.handle(BUNDLE, cell(1, "Au revoir"), &host.invocation()).unwrap();
        assert_eq!(*written.borrow(), "hello=Bonjour\nbye=Au revoir\n");
        assert_eq!(host.process_runs()[0].args, ["/project/messages_fr.properties"]);
        assert!(host.opened_locations().is_empty());

        plugin.handle(BUNDLE, cell(0, "Salut"), &host.invocation()).unwrap();
        assert_eq!(*written.borrow(), "hello=Salut\n");
    }

    #[test]
    fn the_setting_keeps_other_locales_read_only() {
        let (host, written) = tee_host();
        host.set_setting("bundle.writeOtherLocales", "false");
        Properties::default().handle(BUNDLE, ViewEvent::Cell { row: 1, column: 2, value: "x".into() }, &host.invocation()).unwrap();
        assert!(written.borrow().is_empty());
        assert_eq!(host.opened_locations().len(), 1);
    }
}
