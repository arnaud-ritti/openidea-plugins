//! JPA Assist for OpenIDEA: JPA entities and Spring Data repositories in Java
//! and Kotlin, read from the source text (there is no compiler model to ask).
//!
//! - Entities and repositories are checked when they open and when they are
//!   saved; quick fixes make associations lazy, swap Lombok's `@Data` for
//!   `@Getter`/`@Setter` and drop `final` from entity classes.
//! - In a repository, completion writes derived query methods from the
//!   entity's properties, down to the whole declaration.
//! - On an entity's class line, actions generate `equals`/`hashCode` and
//!   `toString`; commands copy its DDL or a DTO, or an entity for the
//!   `CREATE TABLE` at the caret.
//! - The JPA Structure tool window lists the project's entities.

mod checks;
mod generate;
mod model;
mod queries;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use openidea_plugin::host;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, NotificationLevel, Plugin, Location, Position, Project, Range, register_plugin,
};

use checks::{Finding, inspect_attribute_types, inspect_callbacks, inspect_entity, inspect_repository, missing_id};
use generate::{Dialect, Known, ddl, dto, entity_from_ddl, equals_and_hash_code, inherited, table_name, to_string_method};
use model::{Class, Edit, Kind, Relation, has_word, language_of, mask, parse_class};
use queries::{ItemKind, Prop, complete, parse_repository};

/// Time one provider call may spend reading repositories.
const REPOSITORY_BUDGET: Duration = Duration::from_millis(30);

/// Source files listed per pattern.
const MAX_SOURCES: u32 = 50_000;

fn setting(name: &str) -> bool {
    host::setting_bool(&format!("jpaAssist.{name}")).unwrap_or(true)
}

fn project_of(path: &str) -> Option<Project> {
    host::projects()
        .ok()?
        .into_iter()
        .filter(|p| p.root().is_ok_and(|r| path.starts_with(&r)))
        .max_by_key(|p| p.root().map(|r| r.len()).unwrap_or(0))
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.split('.').next().unwrap_or(name)
}

/// The generated text on the clipboard, with a notification either way.
fn copied(text: String, what: &str) -> Result<Option<String>, Error> {
    match host::copy_to_clipboard(&text) {
        Ok(()) => host::notify(NotificationLevel::Info, "JPA Assist", &format!("{what} copied to the clipboard.")),
        Err(Error::Denied(_)) => host::notify(
            NotificationLevel::Warning,
            "JPA Assist",
            "Allow the clipboard for JPA Assist in Settings › Plugins to copy generated code.",
        ),
        Err(e) => return Err(e),
    }
    Ok(None)
}

fn entities_count(n: usize) -> String {
    if n == 1 { "1 entity".into() } else { format!("{n} entities") }
}

fn action(doc: &Document, title: &str, kind: &str, preferred: bool, edits: &[Edit]) -> CodeAction {
    CodeAction {
        title: title.into(),
        kind: Some(kind.into()),
        preferred,
        edits: vec![(doc.path.clone(), edits.iter().map(|e| e.to_text_edit(&doc.text)).collect())],
        data: None,
    }
}

#[derive(Default)]
pub struct JpaAssist {
    /// Java and Kotlin files per project root, listed once per instance.
    sources: HashMap<String, Vec<String>>,
    /// Classes by simple name; `None`: searched the project, not a JPA class.
    classes: HashMap<String, Option<Class>>,
    /// Names not found next to the file asking (editor providers don't search further).
    missing: HashSet<String>,
    /// Every JPA class of the project is in `classes`.
    scanned: bool,
    /// Structure tree item → where it is declared.
    locations: HashMap<String, (String, Position)>,
    /// Expanded entities of the structure tree.
    expanded: HashSet<String>,
    /// Repositories per project root, read lazily.
    repositories: HashMap<String, RepositoryIndex>,
}

/// Repositories by entity simple name, filled within the provider budget.
#[derive(Default)]
struct RepositoryIndex {
    listed: bool,
    unread: Vec<String>,
    by_entity: HashMap<String, Vec<(String, Location)>>,
}

impl RepositoryIndex {
    fn add(&mut self, path: &str, text: &str) {
        for found in self.by_entity.values_mut() {
            found.retain(|(_, location)| location.path != path);
        }

        let Some(kotlin) = language_of(path) else { return };
        let Some(repository) = parse_repository(text, kotlin) else { return };

        let at = position(text, repository.name_start);
        let location = Location { path: path.into(), range: Range::new(at, at) };
        self.by_entity.entry(repository.entity).or_default().push((repository.name, location));
    }
}

fn is_repository_file(path: &str) -> bool {
    path.ends_with("Repository.java") || path.ends_with("Repository.kt")
}

impl JpaAssist {
    /// Repositories declaring `entity`, from the index; reads at most one budget's worth of unread files.
    fn repositories_of(&mut self, project: &Project, entity: &str) -> Vec<(String, Location)> {
        let index = self.repositories.entry(project.root().unwrap_or_default()).or_default();

        if !index.listed {
            index.listed = true;
            for pattern in ["**/*Repository.java", "**/*Repository.kt"] {
                index.unread.extend(project.find_files(pattern, MAX_SOURCES).unwrap_or_default());
            }
        }

        let start = Instant::now();
        while start.elapsed() < REPOSITORY_BUDGET {
            let Some(path) = index.unread.pop() else { break };
            if let Ok(text) = project.read_file(&path) {
                index.add(&path, &text);
            }
        }

        let mut found = index.by_entity.get(entity).cloned().unwrap_or_default();
        found.sort_by(|a, b| a.1.path.cmp(&b.1.path));
        found
    }

    fn class(&self, name: &str) -> Option<&Class> {
        self.classes.get(name)?.as_ref()
    }

    fn sources(&mut self, project: &Project) -> Vec<String> {
        let root = project.root().unwrap_or_default();
        self.sources
            .entry(root)
            .or_insert_with(|| {
                let mut files = project.find_files("**/*.java", MAX_SOURCES).unwrap_or_default();
                files.extend(project.find_files("**/*.kt", MAX_SOURCES).unwrap_or_default());
                files
            })
            .clone()
    }

    /// Caches `class`, and where it is for later instances (this one is reclaimed when idle).
    fn remember(&mut self, class: Class) {
        let _ = host::storage::set(&format!("path:{}", class.name), &class.path);
        self.missing.remove(&class.name);
        self.classes.insert(class.name.clone(), Some(class));
    }

    /// Reads class `name` next to `near` or where it was last found, else anywhere in the project when `search`.
    fn load(&mut self, project: &Project, name: &str, near: &str, search: bool) {
        if name.is_empty() || self.classes.contains_key(name) || (!search && self.missing.contains(name)) {
            return;
        }

        let dir = near.rsplit_once('/').map_or("", |(dir, _)| dir);
        let last_seen = host::storage::get(&format!("path:{name}")).ok().flatten();
        let candidates = ["java", "kt"].map(|extension| format!("{dir}/{name}.{extension}"));

        for path in candidates.into_iter().chain(last_seen) {
            if let Ok(text) = project.read_file(&path) {
                self.classes.insert(name.into(), parse_class(&text, &path));
                return;
            }
        }

        if !search {
            self.missing.insert(name.into());
            return;
        }

        let found = self
            .sources(project)
            .into_iter()
            .filter(|path| file_stem(path) == name)
            .find_map(|path| parse_class(&project.read_file(&path).ok()?, &path));

        match found {
            Some(class) => self.remember(class),
            None => {
                self.classes.insert(name.into(), None);
            }
        }
    }

    /// Class `name` and those its superclass and associations lead to, `depth` levels down.
    fn load_graph(&mut self, project: &Project, name: &str, near: &str, depth: u32, search: bool) {
        self.load(project, name, near, search);

        let Some(class) = self.class(name) else { return };
        if depth == 0 {
            return;
        }

        let path = class.path.clone();
        let mut next: Vec<String> =
            class.attributes.iter().filter(|a| a.relation != Relation::None).map(|a| a.target.clone()).collect();
        next.extend(class.superclass.clone());

        for name in next {
            self.load_graph(project, &name, &path, depth - 1, search);
        }
    }

    fn props(&self, name: &str) -> Option<Vec<Prop>> {
        let class = self.class(name)?;
        let known = |n: &str| self.class(n);

        Some(
            inherited(class, &known)
                .into_iter()
                .map(|a| Prop {
                    name: a.name.clone(),
                    ty: a.ty.clone(),
                    fqcn: (a.relation == Relation::None).then(|| class.qualified(&a.ty)).flatten(),
                    target: (a.relation != Relation::None).then(|| a.target.clone()),
                })
                .collect(),
        )
    }

    /// Whether `class` and every superclass above it are project classes and none declares an id.
    fn lacks_id(&mut self, project: &Project, class: &Class) -> bool {
        let mut current = class.clone();

        for _ in 0..16 {
            if current.has_id {
                return false;
            }

            let Some(parent) = current.superclass.clone() else { return true };
            self.load(project, &parent, &current.path, true);
            let Some(next) = self.class(&parent) else { return false };
            current = next.clone();
        }

        false
    }

    fn entity_rule_findings(&mut self, project: &Project, path: &str, text: &str, kotlin: bool) -> Vec<Finding> {
        let mut findings = Vec::new();

        if setting("checkEntityId")
            && let Some(class) = parse_class(text, path)
            && class.kind == Kind::Entity
            && self.lacks_id(project, &class)
        {
            findings.extend(missing_id(text));
        }

        if setting("checkAttributeTypes") {
            findings.extend(inspect_attribute_types(text, kotlin, &|n| self.class(n).is_some()));
        }

        if setting("checkListenerSignatures") {
            findings.extend(inspect_callbacks(text, kotlin));
        }

        findings
    }

    fn findings(&mut self, project: &Project, path: &str, text: &str, kotlin: bool) -> Vec<Finding> {
        if let Some(class) = parse_class(text, path) {
            self.remember(class);
            if self.scanned {
                host::refresh_view("structure");
            }
            let mut findings = inspect_entity(text, kotlin);
            findings.extend(self.entity_rule_findings(project, path, text, kotlin));
            return findings;
        }

        if setting("checkListenerSignatures") {
            let callbacks = inspect_callbacks(text, kotlin);
            if !callbacks.is_empty() {
                return callbacks;
            }
        }

        let Some(repo) = parse_repository(text, kotlin) else { return Vec::new() };
        self.load_graph(project, &repo.entity, path, 2, true);

        let known = |n: &str| self.class(n);
        let entity_id = self
            .class(&repo.entity)
            .and_then(|c| inherited(c, &known).into_iter().find(|a| a.id).map(|a| a.ty.clone()));
        inspect_repository(&repo, entity_id.as_deref(), &|n| self.props(n))
    }

    /// Reads every JPA class of the project.
    fn scan(&mut self, project: &Project) -> Result<(), Error> {
        let task = host::begin_task("Finding JPA entities", true);
        let files = self.sources(project);

        for (i, path) in files.iter().enumerate() {
            if task.is_cancelled() {
                return Err(Error::Cancelled);
            }

            if i % 100 == 0 {
                task.report(i as u32, Some(files.len() as u32), None);
            }

            let Ok(text) = project.read_file(path) else { continue };
            if !["@Entity", "@MappedSuperclass", "@Embeddable"].iter().any(|m| text.contains(m)) {
                continue;
            }

            if let Some(class) = parse_class(&text, path) {
                self.remember(class);
            }
        }

        self.scanned = true;
        Ok(())
    }

    fn entities(&self) -> Vec<&Class> {
        let mut entities: Vec<&Class> = self.classes.values().flatten().filter(|c| c.kind == Kind::Entity).collect();
        entities.sort_by(|a, b| a.name.cmp(&b.name));
        entities
    }

    /// The entity open in the editor, with the classes it refers to loaded.
    fn current_entity(&mut self, ctx: &Invocation) -> Result<Class, Error> {
        let editor = ctx.editor().ok_or("Open an entity class first.")?;
        let path = editor.path();
        let class = parse_class(&editor.document()?.text, &path)
            .filter(|c| c.kind == Kind::Entity)
            .ok_or("The open file isn't a JPA entity.")?;

        self.remember(class.clone());
        if let Some(project) = ctx.project() {
            self.load_graph(&project, &class.name, &path, 2, true);
        }

        Ok(class)
    }
}

impl Plugin for JpaAssist {
    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        let dialect = Dialect::from_setting(host::setting_string("ddlDialect").as_deref());

        match id {
            "copy-ddl" => {
                let class = self.current_entity(ctx)?;
                let known = |n: &str| self.class(n);
                copied(ddl(&[&class], &known, dialect), &format!("DDL of {}", class.name))
            }
            "copy-ddl-all" => {
                let project = ctx.project().ok_or("Open a project first.")?;
                self.scan(&project)?;
                let entities = self.entities();
                if entities.is_empty() {
                    host::notify(NotificationLevel::Info, "JPA Assist", "No JPA entities found in this project.");
                    return Ok(None);
                }

                let known = |n: &str| self.class(n);
                copied(ddl(&entities, &known, dialect), &format!("DDL of {}", entities_count(entities.len())))
            }
            "copy-dto" => {
                let class = self.current_entity(ctx)?;
                let known = |n: &str| self.class(n);
                copied(dto(&class, &known), &format!("{}Dto", class.name))
            }
            "copy-entity" => {
                let editor = ctx.editor().ok_or("Open a SQL file first.")?;
                let doc = editor.document()?;
                let caret = editor.selections()?.first().map_or(Position::new(0, 0), |s| s.start);
                let at = offset(&doc.text, caret).unwrap_or(doc.text.len());
                let kotlin = host::setting_string("entityLanguage").as_deref() == Some("kotlin");
                let entity = entity_from_ddl(&doc.text, at, kotlin).ok_or("Put the caret in a CREATE TABLE statement.")?;
                copied(entity, "Entity")
            }
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for event in events {
            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            let Some(kotlin) = language_of(path) else { continue };
            let Some(project) = project_of(path) else { continue };
            let Ok(text) = project.read_file(path) else { continue };

            if is_repository_file(path)
                && let Some(index) = self.repositories.get_mut(&project.root().unwrap_or_default())
            {
                index.add(path, &text);
            }

            let diagnostics: Vec<Diagnostic> =
                self.findings(&project, path, &text, kotlin).iter().map(|f| f.diagnostic(&text)).collect();
            let _ = host::publish_diagnostics(path, &diagnostics);
        }
    }
}

impl Providers for JpaAssist {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(kotlin) = language_of(&doc.path) else { return Vec::new() };
        let Some(repo) = parse_repository(&doc.text, kotlin) else { return Vec::new() };
        let Some(at) = offset(&doc.text, at) else { return Vec::new() };

        if let Some(project) = project_of(&doc.path) {
            self.load_graph(&project, &repo.entity, &doc.path, 2, false);
        }

        complete(&doc.text, at, &repo, &|n| self.props(n), kotlin)
            .into_iter()
            .map(|item| {
                let kind = match item.kind {
                    ItemKind::Keyword => CompletionKind::Keyword,
                    ItemKind::Property => CompletionKind::Property,
                    ItemKind::Operator => CompletionKind::Operator,
                    ItemKind::Method => CompletionKind::Method,
                };

                let mut completion = CompletionItem::new(&item.label, kind);
                completion.insert_text = item.insert;
                completion.detail = item.detail;
                completion.additional_edits = item.imports.iter().map(|e| e.to_text_edit(&doc.text)).collect();
                completion
            })
            .collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(class) = parse_class(&doc.text, &doc.path) else { return Vec::new() };
        if class.kind != Kind::Entity {
            return Vec::new();
        }
        let Some(project) = project_of(&doc.path) else { return Vec::new() };

        let repositories = self.repositories_of(&project, &class.name);
        let Some((_, first)) = repositories.first() else { return Vec::new() };
        let names: Vec<&str> = repositories.iter().map(|(name, _)| name.as_str()).collect();

        vec![LineMarker {
            line: class.pos.line,
            icon: Some("navigate".into()),
            tooltip: format!("Repositories: {}", names.join(", ")),
            target: Some(first.clone()),
        }]
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(kotlin) = language_of(&doc.path) else { return Vec::new() };
        let text = &doc.text;
        let touches = |start: usize, end: usize| {
            position(text, start).line <= selection.end.line && position(text, end).line >= selection.start.line
        };

        let mut actions: Vec<CodeAction> = inspect_entity(text, kotlin)
            .into_iter()
            .filter(|f| touches(f.start, f.end))
            .filter_map(|f| {
                let (title, edits) = f.fix?;
                Some(action(doc, title, "quickfix", true, &edits))
            })
            .collect();

        let Some(class) = parse_class(text, &doc.path).filter(|c| c.kind == Kind::Entity) else { return actions };
        if !(selection.start.line..=selection.end.line).contains(&class.pos.line) {
            return actions;
        }

        let masked = mask(text);
        let known = |n: &str| self.class(n);
        let known: Known = &known;

        if !has_word(&masked, "equals") {
            actions.extend(equals_and_hash_code(&class, text, known).map(|e| action(doc, "Generate equals() and hashCode()", "source", false, &[e])));
        }

        if !has_word(&masked, "toString") {
            actions.extend(to_string_method(&class, known).map(|e| action(doc, "Generate toString()", "source", false, &[e])));
        }

        actions
    }
}

impl Views for JpaAssist {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else { return Ok(View::message("Open a project to see its JPA entities.")) };

        if !self.scanned {
            self.scan(&project)?;
        }

        let mut items = Vec::new();
        let mut locations = HashMap::new();
        let entities = self.entities();

        for entity in &entities {
            let key = entity.path.clone();
            let item = TreeItem::new(&key, &entity.name).icon("db-table").detail(&table_name(entity));
            items.push(if entity.attributes.is_empty() { item } else { item.expandable(self.expanded.contains(&key)) });
            locations.insert(key.clone(), (entity.path.clone(), entity.pos));

            for a in &entity.attributes {
                let icon = match a.relation {
                    _ if a.id => "db-column-key",
                    r if r.to_one() => "db-column-fk",
                    r if r.to_many() => "db-columns",
                    _ => "db-column",
                };
                let attribute_key = format!("{key}#{}", a.name);
                items.push(TreeItem::new(&attribute_key, &a.name).parent(&key).icon(icon).detail(&a.ty));
                locations.insert(attribute_key, (entity.path.clone(), a.pos));
            }
        }

        let status = entities_count(entities.len());
        self.locations = locations;

        if items.is_empty() {
            return Ok(View::message("No JPA entities found in this project.").button(Button::new("refresh", "Refresh").icon("refresh")));
        }

        Ok(View::new(Body::Tree(items)).button(Button::new("refresh", "Refresh").icon("refresh")).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
        match event {
            ViewEvent::Button(id) if id == "refresh" => {
                self.sources.clear();
                self.classes.clear();
                self.missing.clear();
                self.scanned = false;
                host::refresh_view(view);
            }
            ViewEvent::Activate(key) => {
                if let Some((path, pos)) = self.locations.get(&key) {
                    host::open_location(path, pos.line, pos.character)?;
                }
            }
            ViewEvent::Expand(key) => {
                self.expanded.insert(key);
                host::refresh_view(view);
            }
            ViewEvent::Collapse(key) => {
                self.expanded.remove(&key);
                host::refresh_view(view);
            }
            _ => {}
        }

        Ok(())
    }
}

register_plugin!(JpaAssist, providers, views);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const ORDER: &str = "package shop;

import jakarta.persistence.*;

@Entity
public class Order {
    @Id
    private Long id;

    private String number;

    @ManyToOne
    private Customer customer;
}
";

    const CUSTOMER: &str = "package shop;\n\n@Entity\npublic class Customer {\n    @Id\n    private Long id;\n    private String name;\n}\n";

    const REPOSITORY: &str = "package shop.data;

import org.springframework.data.jpa.repository.JpaRepository;

public interface OrderRepository extends JpaRepository<Order, Integer> {
    List<Order> findByCustomerName(String name);
    List<Order> findByColour(String colour);
}
";

    fn host() -> MockHost {
        let host = MockHost::install().with_capabilities(&["events", "project.read", "editor.read", "storage", "clipboard"]);
        host.add_file("src/shop/Order.java", ORDER);
        host.add_file("src/shop/Customer.java", CUSTOMER);
        host.add_file("src/shop/data/OrderRepository.java", REPOSITORY);
        host.add_file("src/notes.txt", "@Entity");
        host
    }

    #[test]
    fn publishes_entity_and_repository_problems() {
        let host = host();
        let mut plugin = JpaAssist::default();
        plugin.handle_events(
            &[
                Event::FileOpened("/project/src/shop/Order.java".into()),
                Event::FileOpened("/project/src/shop/data/OrderRepository.java".into()),
                Event::FileSaved("/project/src/notes.txt".into()),
            ],
            &host.invocation(),
        );

        let diagnostics = host.diagnostics();
        assert_eq!(diagnostics["/project/src/shop/Order.java"].len(), 1, "eager to-one");
        let repository: Vec<&str> =
            diagnostics["/project/src/shop/data/OrderRepository.java"].iter().map(|d| d.message.as_str()).collect();
        assert_eq!(repository, ["The id of Order is Long, not Integer.", "Order has no property 'colour' (at 'Colour')."]);
        assert!(!diagnostics.contains_key("/project/src/notes.txt"));
    }

    fn problems(host: &MockHost, path: &str, text: &str) -> Vec<String> {
        host.add_file(path, text);
        let mut plugin = JpaAssist::default();
        plugin.handle_events(&[Event::FileOpened(format!("/project/{path}"))], &host.invocation());
        host.diagnostics().get(&format!("/project/{path}")).map_or(Vec::new(), |d| {
            d.iter().filter_map(|d| d.code.clone()).collect()
        })
    }

    #[test]
    fn mapped_superclass_id_is_inherited() {
        let host = host();
        host.add_file("src/shop/Base.java", "package shop;\n@MappedSuperclass\nclass Base {\n  @Id Long id;\n}\n");
        host.add_file("src/shop/Bare.java", "package shop;\n@MappedSuperclass\nclass Bare {\n  String x;\n}\n");

        let child = |name: &str, parent: &str| format!("package shop;\n@Entity\nclass {name} extends {parent} {{}}\n");
        assert!(problems(&host, "src/shop/A.java", &child("A", "Base")).is_empty());
        assert_eq!(problems(&host, "src/shop/B.java", &child("B", "Bare")), ["entity-missing-id"]);
        assert!(problems(&host, "src/shop/C.java", &child("C", "AbstractPersistable")).is_empty(), "library superclass");
        assert_eq!(problems(&host, "src/shop/D.java", "@Entity\nclass D {}\n"), ["entity-missing-id"]);
        assert!(problems(&host, "src/shop/D.java", "@Entity\nclass D { @Id Long id; }\n").is_empty(), "fixed file");
    }

    #[test]
    fn rule_settings_turn_each_group_off() {
        let host = host();
        host.set_setting("jpaAssist.checkEntityId", "false");
        host.set_setting("jpaAssist.checkAttributeTypes", "false");
        host.set_setting("jpaAssist.checkListenerSignatures", "false");
        let text = "@Entity\nclass E {\n  @Version String v;\n  @PrePersist void p(Object o) {}\n}\n";
        assert!(problems(&host, "src/shop/E.java", text).is_empty());
    }

    #[test]
    fn reports_attribute_types_and_listener_signatures() {
        let host = host();
        let text = "@Entity\nclass E {\n  @Id Long id;\n  @Version String v;\n  @PrePersist void p(Object o) {}\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", text), ["attribute-type", "entity-listener-signature"]);
        let listener = "import jakarta.persistence.*;\nclass L {\n  @PrePersist void p() {}\n}\n";
        assert_eq!(problems(&host, "src/shop/L.java", listener), ["entity-listener-signature"]);
        let fixed = "import jakarta.persistence.*;\nclass L {\n  @PrePersist void p(Object o) {}\n}\n";
        assert!(problems(&host, "src/shop/L.java", fixed).is_empty());
    }

    #[test]
    fn completes_after_the_repository_was_opened() {
        let host = host();
        let mut plugin = JpaAssist::default();
        plugin.handle_events(&[Event::FileOpened("/project/src/shop/data/OrderRepository.java".into())], &host.invocation());

        let text = REPOSITORY.replace("    List<Order> findByColour(String colour);\n", "    findByCustomerN\n");
        let doc = Document { path: "/project/src/shop/data/OrderRepository.java".into(), language: "java".into(), version: 1, text };
        let items = plugin.completion(&doc, Position::new(6, 19), None);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"findByCustomerName"), "{labels:?}");

        let mut after_reclaim = JpaAssist::default();
        let items = after_reclaim.completion(&doc, Position::new(6, 19), None);
        assert!(items.iter().any(|i| i.label == "findByCustomerName"), "found again through storage");
    }

    #[test]
    fn offers_fixes_and_generators_on_the_class_line() {
        let _host = host();
        let mut plugin = JpaAssist::default();
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: ORDER.into() };

        let class_line = Position::new(5, 15);
        let titles: Vec<String> = plugin.code_actions(&doc, Range::new(class_line, class_line)).into_iter().map(|a| a.title).collect();
        assert_eq!(titles, ["Generate equals() and hashCode()", "Generate toString()"]);

        let association = Position::new(11, 6);
        let titles: Vec<String> = plugin.code_actions(&doc, Range::new(association, association)).into_iter().map(|a| a.title).collect();
        assert_eq!(titles, ["Fetch lazily"]);
    }

    #[test]
    fn copies_ddl_dto_and_entities() {
        let host = host();
        let mut plugin = JpaAssist::default();
        host.open_editor("/project/src/shop/Order.java", "java", ORDER, &[(0, 0, 0, 0)]);

        plugin.run_command("copy-ddl", &host.invocation(), None).unwrap();
        let sql = host.clipboard().unwrap();
        assert!(sql.contains("CREATE TABLE order\n"), "{sql}");
        assert!(sql.contains("REFERENCES customer (id)"), "{sql}");

        plugin.run_command("copy-dto", &host.invocation(), None).unwrap();
        assert!(host.clipboard().unwrap().contains("public record OrderDto("));

        plugin.run_command("copy-ddl-all", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications().last().unwrap().body, "DDL of 2 entities copied to the clipboard.");

        host.open_editor("/project/db.sql", "sql", "create table tag (id bigint primary key, label text);", &[(0, 20, 0, 20)]);
        plugin.run_command("copy-entity", &host.invocation(), None).unwrap();
        assert!(host.clipboard().unwrap().contains("public class Tag {"));
    }

    #[test]
    fn entity_marker_targets_repository() {
        let _host = host();
        let mut plugin = JpaAssist::default();
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: ORDER.into() };

        let markers = plugin.line_markers(&doc);

        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 5);
        assert_eq!(markers[0].icon.as_deref(), Some("navigate"));
        assert_eq!(markers[0].tooltip, "Repositories: OrderRepository");
        assert_eq!(markers[0].target.as_ref().unwrap().path, "/project/src/shop/data/OrderRepository.java");

        let customer = Document { path: "/project/src/shop/Customer.java".into(), language: "java".into(), version: 1, text: "@Entity\nclass Customer {}".into() };
        assert!(plugin.line_markers(&customer).is_empty());
    }

    #[test]
    fn entity_marker_does_not_rescan_repositories_per_request() {
        let host = host();
        let mut plugin = JpaAssist::default();
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: ORDER.into() };
        assert_eq!(plugin.line_markers(&doc).len(), 1);

        host.add_file("src/shop/data/OrderRepository.java", "interface OrderRepository {}");
        host.add_file("src/shop/data/OtherRepository.java", REPOSITORY);
        assert_eq!(plugin.line_markers(&doc)[0].tooltip, "Repositories: OrderRepository", "served from the index");

        plugin.handle_events(&[Event::FileSaved("/project/src/shop/data/OrderRepository.java".into())], &host.invocation());
        assert!(plugin.line_markers(&doc).is_empty(), "a saved repository is read again");
    }

    #[test]
    fn menu_entries_have_when_clauses() {
        let value = |block: &str, key: &str| {
            block.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(" = \"")?.strip_suffix('"')).unwrap_or_default().to_owned()
        };
        let editor_menus: Vec<(String, String)> = include_str!("../plugin.toml")
            .split("[[contributes.menus]]")
            .skip(1)
            .map(|block| block.split("\n[").next().unwrap_or(block))
            .filter(|block| value(block, "menu") == "editor.context")
            .map(|block| (value(block, "command"), value(block, "when")))
            .collect();

        assert_eq!(
            editor_menus,
            [
                ("copy-ddl".to_owned(), "editorLangId == java || editorLangId == kotlin".to_owned()),
                ("copy-dto".to_owned(), "editorLangId == java || editorLangId == kotlin".to_owned()),
                ("copy-entity".to_owned(), "editorLangId == sql".to_owned()),
            ]
        );
    }

    #[test]
    fn structure_view_lists_entities_and_navigates() {
        let host = host();
        let mut plugin = JpaAssist::default();
        let view = plugin.render("structure", None, &host.invocation()).unwrap();
        let Body::Tree(items) = view.body else { panic!("a tree") };
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["Customer", "id", "name", "Order", "id", "number", "customer"]);

        assert_eq!(view.status.as_deref(), Some("2 entities"));

        plugin.handle("structure", ViewEvent::Activate("/project/src/shop/Order.java#customer".into()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/shop/Order.java".to_owned(), 12, 21)]);
    }

    #[test]
    fn structure_view_expands_entities_on_request() {
        let host = host();
        let mut plugin = JpaAssist::default();
        let order = "/project/src/shop/Order.java";
        let expanded = |plugin: &mut JpaAssist| {
            let Body::Tree(items) = plugin.render("structure", None, &host.invocation()).unwrap().body else { panic!() };
            let item = items.into_iter().find(|i| i.key == order).unwrap();
            assert!(item.expandable);
            item.expanded
        };
        assert!(!expanded(&mut plugin));

        plugin.handle("structure", ViewEvent::Expand(order.into()), &host.invocation()).unwrap();
        assert!(expanded(&mut plugin));
        plugin.handle("structure", ViewEvent::Collapse(order.into()), &host.invocation()).unwrap();
        assert!(!expanded(&mut plugin));
        assert_eq!(host.refreshed_views(), ["structure", "structure"]);
    }

    #[test]
    fn asks_for_the_clipboard_when_denied() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "storage"]);
        host.open_editor("/project/src/shop/Order.java", "java", ORDER, &[(0, 0, 0, 0)]);
        let result = JpaAssist::default().run_command("copy-dto", &host.invocation(), None).unwrap();
        assert_eq!(result, None);
        assert_eq!(
            host.notifications()[0].body,
            "Allow the clipboard for JPA Assist in Settings › Plugins to copy generated code."
        );
    }
}
