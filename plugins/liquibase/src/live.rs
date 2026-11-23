//! Current-source checks and positive cached facts; never a complete runtime model.
use crate::{
    changelog::{self, Changelog, Format, Include, Target},
    checks,
};
use openidea_plugin::hover::Hover;
use openidea_plugin::providers::{Document, LineMarker};
use openidea_plugin::text::{offset, position};
use openidea_plugin::{Diagnostic, Location, Position, Range, host};
use openidea_plugin::Project;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::time::{Duration, Instant};
const MAX_TEXT: usize = 256 * 1024;
const TTL: Duration = Duration::from_secs(120);
const FALLBACK_BUDGET: Duration = Duration::from_millis(10);
const MAX_RESOLVED: usize = 16;
const MAX_LINE_TARGETS: usize = 200;
struct Fact {
    at: Instant,
    bytes: usize,
    log: Rc<Changelog>,
    /// Editor version and text hash the parse belongs to; `None` for a parse of the file on disk.
    source: Option<(u64, u64)>,
}
type Spec = (String, bool, bool);
fn spec(include: &Include) -> Spec {
    (include.path.clone(), include.relative, include.dir)
}
struct Properties {
    parameters: Option<BTreeSet<String>>,
    declared: Option<BTreeSet<String>>,
}
/// What the project said when a changelog was last opened, saved or touched on disk; diagnostics never ask the project again.
struct Resolution {
    at: Instant,
    source: Option<(u64, u64)>,
    targets: BTreeMap<Spec, Target>,
    properties: Option<Properties>,
}
#[derive(Default)]
pub struct State {
    roots: BTreeMap<String, BTreeMap<String, Fact>>,
    resolved: BTreeMap<String, Resolution>,
    /// Set when the host refused the event subscriptions; diagnostics then warm the open document themselves.
    pub events_denied: bool,
    /// Whether the console rules are set: from the first changelog seen on.
    pub console_rules: bool,
}
fn text_hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
pub(crate) fn parse(path: &str, text: &str) -> Option<Changelog> {
    if text.len() > MAX_TEXT
        || text.match_indices("${").take(1025).count() > 1024
        || (path.ends_with(".xml") && text.bytes().filter(|b| *b == b'<').take(8193).count() > 8192)
    {
        return None;
    }
    let log = changelog::parse(path, text)?;
    if log.change_sets.len() + log.includes.len() + log.changes.len() + log.properties.len() > 4096 {
        return None;
    }
    if log.format == Format::Xml {
        let elements = spring_text::xml::elements(text);
        if elements.iter().flat_map(|e| &e.attrs).any(|a| {
            (a.name == "xmlns" || a.name.starts_with("xmlns:"))
                && a.name != "xmlns:xsi"
                && a.value != "http://www.liquibase.org/xml/ns/dbchangelog"
                && a.value != "http://www.liquibase.org/xml/ns/dbchangelog-ext"
        }) {
            return None;
        }
        if !elements.is_empty()
            && spring_text::xml::namespace(&elements, 0).is_some_and(|n| n != "http://www.liquibase.org/xml/ns/dbchangelog")
        {
            return None;
        }
        if elements.iter().enumerate().any(|(i, e)| e.name.contains(':') && spring_text::xml::namespace(&elements, i).is_none()) { return None; }
        if elements.iter().enumerate().any(|(i, e)| {
            matches!(e.local(), "changeSet" | "property" | "include" | "includeAll")
                && spring_text::xml::namespace(&elements, i).is_some_and(|n| n != "http://www.liquibase.org/xml/ns/dbchangelog")
        }) {
            return None;
        }
    }
    Some(log)
}
fn range(text: &str, a: usize, b: usize) -> Option<Range> {
    (a < b && b <= text.len() && text.is_char_boundary(a) && text.is_char_boundary(b))
        .then(|| Range::new(position(text, a), position(text, b)))
}
fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().take(2048) {
        if c.is_control() {
            out.push(' ');
        } else {
            if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '!' | '(' | ')' | '|') {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}
fn owned(root: &str, path: &str) -> bool {
    root.starts_with('/') && root.len() <= 4096 && path.len() <= 4096
        && !root.chars().chain(path.chars()).any(char::is_control)
        && !root.contains('\\') && !path.contains('\\')
        && !root.split('/').chain(path.split('/')).any(|part| matches!(part, "." | ".."))
        && path.strip_prefix(root.trim_end_matches('/')).is_some_and(|tail| tail.starts_with('/'))
        && host::project_of(path).and_then(|project| project.root().ok()).as_deref() == Some(root)
}
impl State {
    pub fn invalidate(&mut self, path: &str) {
        for files in self.roots.values_mut() {
            files.remove(path);
        }
    }
    pub fn update(&mut self, root: &str, path: &str, text: &str) {
        self.invalidate(path);
        let Some(log) = parse(path, text) else {
            return;
        };
        self.store(root, path, text.len(), Rc::new(log), None);
    }
    fn store(&mut self, root: &str, path: &str, bytes: usize, log: Rc<Changelog>, source: Option<(u64, u64)>) {
        if !owned(root, path) {
            return;
        }
        if !std::mem::replace(&mut self.console_rules, true) {
            crate::console::apply();
        }
        if !self.roots.contains_key(root) && self.roots.len() >= 8 {
            self.roots.clear();
        }
        let files = self.roots.entry(root.into()).or_default();
        files.retain(|_, f| f.at.elapsed() < TTL);
        if files.len() >= 128 || files.values().map(|f| f.bytes).sum::<usize>() + bytes > 4 * 1024 * 1024 {
            files.clear();
        }
        files.insert(path.into(), Fact { at: Instant::now(), bytes, log, source });
    }
    /// Asks the project what `text`'s includes point at and which properties it can supply. Only events and commands call this.
    /// `project_wide` allows the project listings behind property facts; without it those stay unknown.
    /// Whatever `budget` does not reach stays unknown, which the checks treat as quiet.
    pub fn warm(&mut self, project: &Project, root: &str, path: &str, text: &str, budget: Duration, source: Option<(u64, u64)>, project_wide: bool) {
        let deadline = Instant::now() + budget;
        self.resolved.remove(path);
        if !owned(root, path) {
            return;
        }
        let Some(log) = parse(path, text) else {
            return;
        };
        let mut targets = BTreeMap::new();
        for include in log.includes.iter().chain(&log.sql_files).take(512) {
            if Instant::now() > deadline {
                break;
            }
            targets.entry(spec(include)).or_insert_with(|| crate::target_until(project, root, path, include, Some(deadline)));
        }
        let has_unknown_placeholder = checks::placeholders(text, log.format)
            .into_iter()
            .any(|(_, _, name)| !name.starts_with("database.") && !log.properties.iter().any(|p| p == name));
        let properties = (project_wide && has_unknown_placeholder).then(|| Properties {
            parameters: crate::project_parameters(project, root, path, deadline),
            declared: crate::project_properties(project, root, path, &log, deadline),
        });
        if self.resolved.len() >= MAX_RESOLVED {
            let oldest = self.resolved.iter().min_by_key(|(_, r)| r.at).map(|(p, _)| p.clone());
            if let Some(oldest) = oldest {
                self.resolved.remove(&oldest);
            }
        }
        self.resolved.insert(path.into(), Resolution { at: Instant::now(), source, targets, properties });
    }
    fn warm_current(&mut self, doc: &Document, root: Option<&str>) {
        let source = Some((doc.version, text_hash(&doc.text)));
        if self.resolved.get(&doc.path).is_some_and(|r| r.source == source) {
            return;
        }
        let Some(root) = root else {
            return;
        };
        let Some(project) = host::project_of(&doc.path) else {
            return;
        };
        self.warm(&project, root, &doc.path, &doc.text, FALLBACK_BUDGET, source, false);
    }
    pub fn forget(&mut self, path: &str) {
        self.invalidate(path);
        self.resolved.remove(path);
    }
    pub fn warmed_paths(&self) -> Vec<String> {
        self.resolved.keys().cloned().collect()
    }
    fn current(&mut self, doc: &Document) -> Option<(Option<String>, Rc<Changelog>)> {
        if !matches!(doc.language.as_str(), "xml" | "yaml" | "json" | "sql") {
            return None;
        }
        let root = host::project_of(&doc.path).and_then(|p| p.root().ok());
        let source = (doc.version, text_hash(&doc.text));
        let cached = root
            .as_ref()
            .and_then(|root| self.roots.get(root))
            .and_then(|files| files.get(&doc.path))
            .filter(|fact| fact.source == Some(source) && fact.at.elapsed() < TTL);
        if let Some(fact) = cached {
            return Some((root, fact.log.clone()));
        }
        self.invalidate(&doc.path);
        let log = Rc::new(parse(&doc.path, &doc.text)?);
        if let Some(root) = &root {
            self.store(root, &doc.path, doc.text.len(), log.clone(), Some(source));
        }
        Some((root, log))
    }
    fn target(&self, root: &str, path: &str, include: &Include) -> Option<String> {
        if include.dir || !include.relative || include.path.contains("${") || include.path.starts_with('/') || include.path.contains(':') {
            return None;
        }
        let files = self.roots.get(root)?;
        let mut found =
            changelog::candidates(path, root, include).into_iter().filter(|p| files.get(p).is_some_and(|f| f.at.elapsed() < TTL) && owned(root, p));
        let first = found.next()?;
        if found.next().is_some() { None } else { Some(first) }
    }
    pub fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        let Some((root, log)) = self.current(doc) else {
            return vec![];
        };
        if self.events_denied || !self.resolved.contains_key(&doc.path) {
            self.warm_current(doc, root.as_deref());
        }
        let resolution = self.resolved.get(&doc.path);
        let target = |include: &Include| resolution.and_then(|r| r.targets.get(&spec(include))).cloned().unwrap_or(Target::Unknown);
        let defined = |name: &str| {
            let Some(Properties { parameters: Some(parameters), declared }) = resolution.and_then(|r| r.properties.as_ref()) else {
                return true;
            };
            parameters.contains(name) || declared.as_ref().is_none_or(|declared| declared.contains(name))
        };
        let check_properties = host::setting_bool("checkProperties").unwrap_or(true);
        let defined: Option<&dyn Fn(&str) -> bool> = check_properties.then_some(&defined);
        let mut problems = checks::problems(&doc.path, &doc.text, &log, &target, defined);
        problems.truncate(256);
        problems
    }
    pub fn markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some((Some(root), log)) = self.current(doc) else {
            return vec![];
        };
        let mut markers = Vec::new();
        for include in log.includes.iter().take(256) {
            let targets = if include.dir {
                self.directory_targets(&root, &doc.path, include)
            } else {
                self.target(&root, &doc.path, include).into_iter().collect()
            };
            let total = targets.len();
            let line = changelog::line_of(&doc.text, include.at);
            // Same icon on one line: the IDE merges the markers into one button with a target chooser.
            markers.extend(targets.into_iter().take(MAX_LINE_TARGETS).map(|path| {
                let name = path.rsplit('/').next().unwrap_or(&path);
                let tooltip = if total > MAX_LINE_TARGETS {
                    format!("Open cached source {name} (first {MAX_LINE_TARGETS} of {total} shown)")
                } else {
                    format!("Open cached source {name}")
                };
                LineMarker {
                    line,
                    icon: Some(crate::INCLUDE_MARKER.into()),
                    tooltip,
                    target: Some(Location { path, range: Range::new(Position::new(0, 0), Position::new(0, 0)) }),
                }
            }));
        }
        markers
    }
    /// Cached changelogs under a relative `includeAll` directory, recursively and in Liquibase's alphabetical order.
    fn directory_targets(&self, root: &str, path: &str, include: &Include) -> Vec<String> {
        if !include.relative || include.path.contains("${") || include.path.starts_with('/') || include.path.contains(':') {
            return Vec::new();
        }
        let (Some(files), Some(dir)) = (self.roots.get(root), changelog::candidates(path, root, include).into_iter().next()) else {
            return Vec::new();
        };
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        files
            .iter()
            .filter(|(file, fact)| file.starts_with(&prefix) && file.as_str() != path && fact.at.elapsed() < TTL && owned(root, file))
            .map(|(file, _)| file.clone())
            .collect()
    }
    fn declarations(&self, root: &str, path: &str, name: &str, seen: &mut BTreeSet<String>, found: &mut Vec<String>, budget: &mut usize) {
        if !owned(root, path) || seen.len() >= 32 || !seen.insert(path.into()) {
            return;
        }
        let Some(fact) = self.roots.get(root).and_then(|f| f.get(path)).filter(|f| f.at.elapsed() < TTL) else {
            return;
        };
        for property in &fact.log.properties {
            if property == name {
                found.push(path.into());
            }
        }
        for include in &fact.log.includes {
            if *budget == 0 { found.clear(); return; }
            *budget -= 1;
            if let Some(next) = self.target(root, path, include) {
                self.declarations(root, &next, name, seen, found, budget);
                if *budget == 0 { found.clear(); return; }
            }
        }
    }
    pub fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        let (root, log) = self.current(doc)?;
        let at = offset(&doc.text, at)?;
        for include in &log.includes {
            if include.at <= at && at < include.end {
                let target = root.as_ref().and_then(|r| self.target(r, &doc.path, include));
                let status = target
                    .map(|p| format!("Cached source: {}", escape(&p)))
                    .unwrap_or_else(|| "Target not established by the current source cache; no absence or filesystem claim.".into());
                return Some(Hover::new(format!("**Liquibase {}**: {}\n\n{}\n\nrelativeToChangelogFile: {} · optional: {}",if include.dir {"includeAll"} else {"include"},escape(&include.path),status,include.relative,include.optional)).range(range(&doc.text,include.at,include.end)?));
            }
        }
        for set in &log.change_sets {
            if set.at <= at && at < set.end {
                return Some(Hover::new(format!("**Liquibase changeset**\n\nID: {}\n\nAuthor: {}\n\nSource: {}",escape(set.id.as_deref().unwrap_or("(missing)")),escape(set.author.as_deref().unwrap_or("(missing)")),escape(&doc.path))).range(range(&doc.text,set.at,set.end)?));
            }
        }
        for (from, to, name) in checks::placeholders(&doc.text, log.format) {
            if from <= at && at < to {
                let mut found = vec![];
                if let Some(root) = &root {
                    self.declarations(root, &doc.path, name, &mut BTreeSet::new(), &mut found, &mut 128);
                } else {
                    for n in &log.properties {
                        if n == name {
                            found.push(doc.path.clone());
                        }
                    }
                }
                let [owner] = found.as_slice() else {
                    return None;
                };
                return Some(Hover::new(format!("**Liquibase property**: {}\n\nDeclared in cached source: {}",escape(name),escape(owner))).range(range(&doc.text,from,to)?));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Liquibase;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::{Event, FilesChanged, Plugin};
    const XML: &str = "<databaseChangeLog xmlns='http://www.liquibase.org/xml/ns/dbchangelog'><!-- 😀 --><changeSet id='1' author='a'/><changeSet id='1' author='a'/><include file='child.xml' relativeToChangelogFile='true'/></databaseChangeLog>";
    fn doc(path: &str, text: &str) -> Document {
        Document {
            path: path.into(),
            language: if path.ends_with("xml") {
                "xml"
            } else if path.ends_with("sql") {
                "sql"
            } else if path.ends_with("json") {
                "json"
            } else {
                "yaml"
            }
            .into(),
            version: 1,
            text: text.into(),
        }
    }
    #[test]
    fn unsaved_provider_reports_and_clears_without_project_permission_or_manual_results() {
        for caps in [vec![], vec!["project.read", "events", "editor.read"]] {
            let h = MockHost::install().with_capabilities(&caps);
            h.add_file("db/a.xml", "<databaseChangeLog/>");
            h.open_editor("/project/db/a.xml", "xml", XML, &[]);
            let mut p = Liquibase::default();
            h.refresh_diagnostics(&mut p);
            assert!(h.provider_diagnostics()["/project/db/a.xml"].iter().any(|d| d.code.as_deref() == Some("duplicate-changeset")));
            assert!(h.diagnostics().is_empty());
            h.open_editor("/project/db/a.xml", "xml", "<databaseChangeLog/>", &[]);
            h.refresh_diagnostics(&mut p);
            assert!(h.provider_diagnostics().values().all(Vec::is_empty));
            assert!(h.diagnostics().is_empty());
        }
    }
    #[test]
    fn pure_rules_cover_all_existing_formats() {
        let _h = MockHost::install();
        let mut s = State::default();
        for (path, text, code) in [
            ("/project/db/a.sql", "--changeset a:1\nselect 1;", "missing-header"),
            (
                "/project/db/a.yaml",
                "databaseChangeLog:\n  - changeSet:\n      id: 1\n      changes:\n        - nonexistent: {}\n",
                "incomplete-changeset",
            ),
            ("/project/db/a.json", r#"{"databaseChangeLog":[{"changeSet":{"id":"1"}}]}"#, "incomplete-changeset"),
            ("/project/db/a.xml", "<databaseChangeLog><changeSet author='a'/></databaseChangeLog>", "incomplete-changeset"),
        ] {
            assert!(s.diagnostics(&doc(path, text)).iter().any(|d| d.code.as_deref() == Some(code)), "{path}");
        }
    }
    #[test]
    fn unknown_include_property_and_foreign_xml_context_are_quiet() {
        let _h = MockHost::install();
        let mut s = State::default();
        let text = "<databaseChangeLog><include file='unknown.xml'/><changeSet id='${library.id}' author='a'/></databaseChangeLog>";
        s.warm(&host::project_of("/project/db/a.xml").unwrap(), "/project", "/project/db/a.xml", text, Duration::ZERO, None, false);
        assert!(s.diagnostics(&doc("/project/db/a.xml", text)).is_empty(), "facts the warm did not reach stay quiet");
        assert!(
            s.diagnostics(&doc("/project/db/a.xml", "<databaseChangeLog xmlns='urn:foreign'><changeSet/></databaseChangeLog>")).is_empty()
        );
    }
    #[test]
    fn hover_ranges_are_utf16_and_report_only_literal_source_facts() {
        let _h = MockHost::install();
        let mut s = State::default();
        let d = doc("/project/db/a.xml", XML);
        let at = XML.find("changeSet").unwrap();
        let h = s.hover(&d, position(XML, at + 2)).unwrap();
        assert_eq!(h.range, range(XML, at, at + 9));
        assert!(h.markdown.contains("ID: 1") && !h.markdown.contains("not resolved"));
        let at = XML.find("child.xml").unwrap();
        let h = s.hover(&d, position(XML, at + 2)).unwrap();
        assert_eq!(h.range, range(XML, at, at + 9));
        assert!(h.markdown.contains("Target not established"));
    }
    #[test]
    fn cached_include_navigation_is_positive_root_owned_and_invalidated_by_events() {
        let h = MockHost::install().with_capabilities(&["project.read", "events"]);
        let mut p = Liquibase::default();
        let d = doc("/project/db/a.xml", XML);
        assert!(p.live.markers(&d).is_empty());
        p.live.update("/another", "/another/db/child.xml", "<databaseChangeLog/>");
        assert!(p.live.markers(&d).is_empty());
        p.live.update("/project", "/project/db/child.xml", "<databaseChangeLog/>");
        assert_eq!(p.live.markers(&d).len(), 1);
        p.handle_events(
            &[Event::FilesChanged(FilesChanged { modified: vec!["/project/db/child.xml".into()], ..Default::default() })],
            &h.invocation(),
        );
        assert!(p.live.markers(&d).is_empty());
        assert!(h.diagnostics().is_empty());
    }
    #[test]
    fn include_all_marks_each_cached_changelog_of_its_directory_on_one_line() {
        let _h = MockHost::install();
        let mut s = State::default();
        let text = "<databaseChangeLog>\n<includeAll path='changes/' relativeToChangelogFile='true'/>\n</databaseChangeLog>";
        let d = doc("/project/db/master.xml", text);
        s.update("/project", "/project/db/changes/001.xml", "<databaseChangeLog/>");
        let single = s.markers(&d);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].tooltip, "Open cached source 001.xml");
        s.update("/project", "/project/db/changes/sub/002.xml", "<databaseChangeLog/>");
        s.update("/project", "/project/db/other/003.xml", "<databaseChangeLog/>");
        s.update("/project", "/project/db/changes-old/004.xml", "<databaseChangeLog/>");
        let markers = s.markers(&d);
        let paths: Vec<_> = markers.iter().map(|m| m.target.as_ref().unwrap().path.as_str()).collect();
        assert_eq!(paths, ["/project/db/changes/001.xml", "/project/db/changes/sub/002.xml"]);
        assert!(markers.iter().all(|m| m.line == 1 && m.icon.as_deref() == Some(crate::INCLUDE_MARKER)));
        let absolute = doc("/project/db/master.xml", &text.replace(" relativeToChangelogFile='true'", ""));
        assert!(s.markers(&absolute).is_empty());
    }
    #[test]
    fn current_reuses_the_parse_for_the_same_version_and_text() {
        let _h = MockHost::install();
        let mut s = State::default();
        let d = doc("/project/db/a.xml", XML);
        let first = s.current(&d).unwrap().1;
        assert!(Rc::ptr_eq(&first, &s.current(&d).unwrap().1));
        let mut edited = doc("/project/db/a.xml", XML);
        edited.version = 2;
        assert!(!Rc::ptr_eq(&first, &s.current(&edited).unwrap().1));
    }
    #[test]
    fn property_hover_uses_only_reachable_cached_declarations_and_never_values() {
        let _h = MockHost::install();
        let mut s = State::default();
        let text = "<databaseChangeLog><include file='child.xml' relativeToChangelogFile='true'/><changeSet id='${name}' author='a'/></databaseChangeLog>";
        let d = doc("/project/db/a.xml", text);
        let at = position(text, text.find("${name}").unwrap() + 3);
        assert!(s.hover(&d, at).is_none());
        let property = "<databaseChangeLog><property name='name' value='SECRET'/></databaseChangeLog>";
        s.update("/project", "/project/other.xml", property);
        assert!(s.hover(&d, at).is_none());
        s.update("/project", "/project/db/child.xml", property);
        let hover = s.hover(&d, at).unwrap();
        assert!(hover.markdown.contains("/project/db/child.xml") && !hover.markdown.contains("SECRET"));
        s.update(
            "/project",
            "/project/db/child.xml",
            "<databaseChangeLog><property name='name'/><property name='name'/></databaseChangeLog>",
        );
        assert!(s.hover(&d, at).is_none());
        s.update("/project", "/project/db/child.xml", "<databaseChangeLog/>");
        assert!(s.hover(&d, at).is_none());
    }
    #[test]
    fn separately_owned_nested_sources_never_supply_outer_include_links_or_property_provenance() {
        for opened_after_warm in [false, true] {
            let host = MockHost::install();
            let mut state = State::default();
            let nested = "/project/db/nested";
            if !opened_after_warm { host.add_project(nested); }
            state.update("/project", "/project/db/nested/child.xml", "<databaseChangeLog><property name='name' value='PRIVATE'/></databaseChangeLog>");
            if opened_after_warm { host.add_project(nested); }
            let text = "<databaseChangeLog><include file='nested/child.xml' relativeToChangelogFile='true'/><changeSet id='${name}' author='a'/></databaseChangeLog>";
            let source = doc("/project/db/a.xml", text);
            assert!(state.markers(&source).is_empty(), "a nested project cannot supply an outer-root navigation target");
            assert!(state.hover(&source, position(text, text.find("${name}").unwrap() + 3)).is_none());
            let include = state.hover(&source, position(text, text.find("nested/child.xml").unwrap() + 2)).unwrap();
            assert!(include.markdown.contains("Target not established"));
        }
    }
    #[test]
    fn invalid_ownership_and_exhausted_traversal_never_publish_positive_property_provenance() {
        let host = MockHost::install();
        let mut state = State::default();
        state.update("/project", "/project/db/../child.xml", "<databaseChangeLog/>");
        state.update("/project", "/project-other/child.xml", "<databaseChangeLog/>");
        assert!(state.roots.is_empty());
        state.update("/project", "/project/db/child.xml", "<databaseChangeLog><property name='name'/></databaseChangeLog>");
        let text = format!("<databaseChangeLog>{}<changeSet id='${{name}}' author='a'/></databaseChangeLog>", "<include file='child.xml' relativeToChangelogFile='true'/>".repeat(129));
        let source = doc("/project/db/a.xml", &text);
        assert!(state.hover(&source, position(&text, text.find("${name}").unwrap() + 3)).is_none());
        let _host = host.with_capabilities(&[]);
        state.update("/project", "/project/db/new.xml", "<databaseChangeLog/>");
        assert!(!state.roots["/project"].contains_key("/project/db/new.xml"));
    }
    #[test]
    fn limits_expiry_and_source_removal_drop_positive_facts() {
        let _h = MockHost::install();
        let mut s = State::default();
        s.update("/project", "/project/db/child.xml", "<databaseChangeLog/>");
        s.roots.get_mut("/project").unwrap().get_mut("/project/db/child.xml").unwrap().at = Instant::now() - TTL;
        assert!(s.markers(&doc("/project/db/a.xml", XML)).is_empty());
        s.update("/project", "/project/db/child.xml", "<databaseChangeLog/>");
        s.update("/project", "/project/db/child.xml", "unrelated");
        assert!(s.markers(&doc("/project/db/a.xml", XML)).is_empty());
        assert!(s.diagnostics(&doc("/project/db/a.xml", &"x".repeat(MAX_TEXT + 1))).is_empty());
        _h.add_project("/p");
        for n in 0..129 {
            s.update("/p", &format!("/p/{n}.xml"), "<databaseChangeLog/>");
        }
        assert!(s.roots["/p"].len() <= 128);
    }
    #[test]
    fn include_classpath_candidates_never_imply_runtime_owner_and_delimiter_limits_are_quiet() {
        let _h = MockHost::install();
        let mut state = State::default();
        state.update("/project", "/project/child.xml", "<databaseChangeLog/>");
        let d = doc("/project/a.xml", "<databaseChangeLog><include file='child.xml'/></databaseChangeLog>");
        assert!(state.markers(&d).is_empty());
        assert!(
            state.diagnostics(&doc("/project/a.xml", &format!("<databaseChangeLog>{}</databaseChangeLog>", "${".repeat(1025)))).is_empty()
        );
        state.warm(&host::project_of("/project/a.xml").unwrap(), "/project", "/project/a.xml", XML, Duration::ZERO, None, false);
        let problems = state.diagnostics(&doc("/project/a.xml", XML));
        for diagnostic in problems {
            let a = offset(XML, diagnostic.range.start).unwrap();
            let b = offset(XML, diagnostic.range.end).unwrap();
            assert!(a < b && &XML[a..b] == "changeSet");
        }
    }
}
