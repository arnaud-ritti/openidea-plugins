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

mod build;
mod checks;
mod generate;
mod model;
mod queries;
mod wizard;
mod xml;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use openidea_plugin::host;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, TreeItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Context, Diagnostic, Error, Event, EventKind, Invocation, NotificationLevel, PickItem, Plugin, Location, Position, Project, Range, TextEdit, register_plugin,
};

use checks::{CALLBACKS, Finding, inspect_attribute_signatures, inspect_attribute_types, lacks_default_constructor, listener_without_constructor, inspect_class_signature, is_empty_listener, listener_references, unused_listener, inspect_callbacks, inspect_entity, inspect_kotlin_build, inspect_name_references, inspect_query_parameters, name_references, named_query_references, unknown_named_query, declared_named_queries, inspect_kotlin_entity, inspect_repository, missing_id};
use build::{BUILD_FILES, Build, analyse, is_build_file};
use generate::{Dialect, Known, callback_import, callback_method, ddl, dto, dto_fields, entity_from_ddl, equals_and_hash_code, inherited, table_name, to_string_method};
use model::{Class, Edit, Kind, Relation, annotations, capitalize, decapitalize, snake_case, has_word, import_edit, language_of, mask, parse_class, words};
use wizard::{AttributeSpec, EntitySpec, attribute_edits, entity_source, inverse_spec};
use xml::Lookup;
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

const ENTITY_KEY_PREFIX: &str = "entity:";

fn choose(title: &str, items: &[PickItem]) -> Result<Option<String>, Error> {
    match host::pick(title, items) {
        Err(Error::Invalid(_)) => Ok(items.first().map(|i| i.key.clone())),
        answer => answer,
    }
}

fn choose_many(title: &str, items: &[PickItem]) -> Result<Option<Vec<String>>, Error> {
    match host::pick_many(title, items) {
        Err(Error::Invalid(_)) => Ok(Some(items.iter().map(|i| i.key.clone()).collect())),
        answer => answer,
    }
}

fn choose_dialect() -> Result<Option<Dialect>, Error> {
    let configured = host::setting_string("ddlDialect");
    let mut items = vec![PickItem::new("postgresql", "PostgreSQL"), PickItem::new("mysql", "MySQL")];
    if Dialect::from_setting(configured.as_deref()) == Dialect::MySql {
        items.reverse();
    }
    items[0].description = Some("default".into());

    Ok(choose("SQL dialect", &items)?.map(|key| Dialect::from_setting(Some(&key))))
}

fn choose_entity_language() -> Result<Option<bool>, Error> {
    let mut items = vec![PickItem::new("java", "Java"), PickItem::new("kotlin", "Kotlin")];
    if host::setting_string("entityLanguage").as_deref() == Some("kotlin") {
        items.reverse();
    }
    items[0].description = Some("default".into());

    Ok(choose("Entity language", &items)?.map(|key| key == "kotlin"))
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn package_of(path: &str) -> Option<String> {
    let dir = format!("{}/", path.rsplit_once('/')?.0);
    let (at, marker) = ["/java/", "/kotlin/"].into_iter().filter_map(|m| dir.rfind(m).map(|i| (i, m))).max()?;
    let package = dir[at + marker.len()..].trim_end_matches('/').replace('/', ".");

    (!package.is_empty()).then_some(package)
}

fn ask_identifier(prompt: &str) -> Result<Option<String>, Error> {
    let Some(answer) = host::input(prompt, None)? else { return Ok(None) };
    let answer = answer.trim();
    if !is_identifier(answer) {
        return Err(format!("\"{answer}\" isn't a valid name.").into());
    }

    Ok(Some(answer.to_owned()))
}

fn ask_basic_type() -> Result<Option<(String, bool)>, Error> {
    let names = ["String", "Long", "Integer", "BigDecimal", "LocalDate", "LocalDateTime", "UUID", "Boolean"];
    let mut items: Vec<PickItem> = names.iter().map(|n| PickItem::new(n, n)).collect();
    items.push(PickItem::new("enum", "Enum"));
    items.push(PickItem::new("other", "Other type"));

    let Some(key) = choose("Attribute type", &items)? else { return Ok(None) };
    if key == "enum" || key == "other" {
        let Some(ty) = ask_identifier(if key == "enum" { "Enum type" } else { "Type" })? else { return Ok(None) };
        return Ok(Some((ty, key == "enum")));
    }

    Ok(Some((key, false)))
}

fn deliver_edits(editor: &openidea_plugin::TextEditor, version: u64, edits: &[TextEdit], what: &str) -> Result<(), Error> {
    match editor.apply_edits(version, edits) {
        Ok(_) => host::notify(NotificationLevel::Info, "JPA Assist", &format!("{what} added.")),
        Err(Error::Denied(_)) => host::notify(
            NotificationLevel::Warning,
            "JPA Assist",
            "Allow editor changes for JPA Assist in Settings › Plugins to add code.",
        ),
        Err(e) => return Err(e),
    }

    Ok(())
}

fn new_entity(ctx: &Invocation) -> Result<Option<String>, Error> {
    let Some(name) = ask_identifier("Entity class name")? else { return Ok(None) };
    let name = capitalize(&name);
    let Some(table) = host::input("Table name", Some(&snake_case(&name)))? else { return Ok(None) };

    let ids = [("none", "Assigned"), ("IDENTITY", "IDENTITY"), ("SEQUENCE", "SEQUENCE"), ("AUTO", "AUTO"), ("UUID", "UUID")];
    let items: Vec<PickItem> = ids.iter().map(|(k, l)| PickItem::new(k, l)).collect();
    let Some(id) = choose("ID strategy", &items)? else { return Ok(None) };

    let extras = [("auditing", "Auditing fields (createdAt, updatedAt)"), ("version", "Version field"), ("equals", "equals() and hashCode()")];
    let items: Vec<PickItem> = extras.iter().map(|(k, l)| PickItem::new(k, l)).collect();
    let Some(extras) = host::pick_many("Extras", &items).or_else(|e| match e {
        Error::Invalid(_) => Ok(Some(Vec::new())),
        e => Err(e),
    })?
    else {
        return Ok(None);
    };

    let editor = ctx.editor();
    let path = editor.as_ref().map(|e| e.path());
    let kotlin = path
        .as_deref()
        .and_then(language_of)
        .unwrap_or_else(|| host::setting_string("entityLanguage").as_deref() == Some("kotlin"));
    let table = table.trim();
    let source = entity_source(&EntitySpec {
        table: if table.is_empty() { snake_case(&name) } else { table.to_owned() },
        package: path.as_deref().and_then(package_of),
        kotlin,
        id,
        auditing: extras.iter().any(|e| e == "auditing"),
        version: extras.iter().any(|e| e == "version"),
        equals: extras.iter().any(|e| e == "equals"),
        name: name.clone(),
    });

    copied(source.clone(), &name)?;

    let Some(editor) = editor else { return Ok(None) };
    let doc = editor.document()?;
    if doc.text.trim().is_empty() {
        let edit = Edit { start: 0, end: doc.text.len(), text: source };
        deliver_edits(&editor, doc.version, &[edit.to_text_edit(&doc.text)], &name)?;
    }

    Ok(None)
}

fn new_attribute(ctx: &Invocation) -> Result<Option<String>, Error> {
    let editor = ctx.editor().ok_or("Open an entity class first.")?;
    let path = editor.path();
    let entity = |text: &str| parse_class(text, &path).filter(|c| c.kind == Kind::Entity).ok_or("The open file isn't a JPA entity.");
    entity(&editor.document()?.text)?;

    let Some(name) = ask_identifier("Attribute name")? else { return Ok(None) };
    let name = decapitalize(&name);

    let relations = [("none", "None (basic attribute)"), ("ManyToOne", "ManyToOne"), ("OneToMany", "OneToMany"), ("OneToOne", "OneToOne"), ("ManyToMany", "ManyToMany")];
    let items: Vec<PickItem> = relations.iter().map(|(k, l)| PickItem::new(k, l)).collect();
    let Some(kind) = choose("Association", &items)? else { return Ok(None) };
    let relation = match kind.as_str() {
        "ManyToOne" => Relation::ManyToOne,
        "OneToMany" => Relation::OneToMany,
        "OneToOne" => Relation::OneToOne,
        "ManyToMany" => Relation::ManyToMany,
        _ => Relation::None,
    };

    let (ty, enumerated) = if relation == Relation::None {
        let Some(chosen) = ask_basic_type()? else { return Ok(None) };
        chosen
    } else {
        let Some(target) = ask_identifier("Target entity")? else { return Ok(None) };
        (capitalize(&target), false)
    };

    let mut options = vec![PickItem::new("cascade", "Cascade all")];
    if !relation.to_many() {
        options.insert(0, PickItem::new("notNull", "Not null"));
    }
    if relation == Relation::None {
        options.remove(options.len() - 1);
        options.push(PickItem::new("unique", "Unique"));
    } else {
        let label = if relation.to_one() { "Lazy fetch" } else { "Eager fetch" };
        options.push(PickItem::new("fetch", label));
    }
    let Some(chosen) = choose_many("Options", &options)? else { return Ok(None) };
    let has = |key: &str| chosen.iter().any(|c| c == key);

    let doc = editor.document()?;
    let class = entity(&doc.text)?;
    if class.attributes.iter().any(|a| a.name == name) {
        return Err(format!("{} already has an attribute named {name}.", class.name).into());
    }

    let spec = AttributeSpec {
        name: name.clone(),
        ty,
        relation,
        enumerated,
        not_null: has("notNull"),
        unique: has("unique"),
        cascade: has("cascade"),
        fetch: has("fetch"),
        mapped_by: None,
        imports: Vec::new(),
    };
    let edits = attribute_edits(&class, &doc.text, &mask(&doc.text), &spec).ok_or("The class has no body to add an attribute to.")?;
    let edits: Vec<TextEdit> = edits.iter().map(|e| e.to_text_edit(&doc.text)).collect();

    deliver_edits(&editor, doc.version, &edits, &format!("Attribute {name}"))?;

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
    /// Names of `@NamedQuery` and friends across the project; `None` once computed means an XML mapping or a non-literal name hides some.
    named_queries: Option<Option<HashSet<String>>>,
    /// Build files around a Kotlin entity, by the entity's path; cleared when a build file is saved.
    builds: HashMap<String, Build>,
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

    /// The build files from the directory of `path` up to the project root.
    fn build_of(&mut self, project: &Project, path: &str) -> &Build {
        self.builds.entry(path.to_owned()).or_insert_with(|| {
            let root = project.root().unwrap_or_default();
            let mut files = Vec::new();
            let mut dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);

            while dir.starts_with(&root) {
                for name in BUILD_FILES {
                    let candidate = format!("{dir}/{name}");
                    if let Ok(text) = project.read_file(&candidate) {
                        files.push((candidate, text));
                    }
                }

                let Some((parent, _)) = dir.rsplit_once('/') else { break };
                if dir == root {
                    break;
                }
                dir = parent;
            }

            analyse(&files)
        })
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

    /// Listeners named in `@EntityListeners` that are found once in the project and declare no callback.
    fn listener_findings(&mut self, project: &Project, text: &str) -> Vec<Finding> {
        let references = listener_references(text);
        if references.is_empty() {
            return Vec::new();
        }

        let files = self.sources(project);
        let mut findings = Vec::new();

        for (name, start, end) in references {
            let mut found = files.iter().filter(|path| file_stem(path) == name);
            let (Some(path), None) = (found.next(), found.next()) else { continue };
            let Some(kotlin) = language_of(path) else { continue };
            let Ok(source) = project.read_file(path) else { continue };

            if is_empty_listener(&source, &name, kotlin) {
                findings.push(unused_listener(start, end, &name));
            } else if lacks_default_constructor(&source, &name, kotlin) {
                findings.push(listener_without_constructor(start, end, &name));
            }
        }

        findings
    }

    /// The names of every named query of the project, `None` when mapping files or constants may declare others.
    fn named_queries(&mut self, project: &Project) -> Option<&HashSet<String>> {
        if self.named_queries.is_none() {
            let mut names = HashSet::new();
            let mut complete = ["**/*orm.xml", "**/jpa-named-queries.properties"]
                .iter()
                .all(|pattern| project.find_files(pattern, 1).unwrap_or_default().is_empty());
            complete &= project
                .find_files("**/persistence.xml", 20)
                .unwrap_or_default()
                .iter()
                .all(|p| project.read_file(p).is_ok_and(|t| !t.contains("mapping-file")));

            for path in self.sources(project) {
                let Some(kotlin) = language_of(&path) else { continue };
                let Ok(text) = project.read_file(&path) else { continue };
                if !text.contains("NamedQuery") && !text.contains("NamedNativeQuery") && !text.contains("NamedStoredProcedureQuery") {
                    continue;
                }

                match declared_named_queries(&text, kotlin) {
                    Some(found) => names.extend(found),
                    None => complete = false,
                }
            }

            self.named_queries = Some(complete.then_some(names));
        }

        self.named_queries.as_ref()?.as_ref()
    }

    fn check_xml(&mut self, project: &Project, path: &str, text: &str) {
        let persistence = text.contains("<persistence") && text.contains("persistence-unit");
        if !(persistence || text.contains("<entity-mappings")) || !setting("checkPersistenceXml") {
            return;
        }

        let files = self.sources(project);
        let orm_present = !project.find_files("**/*orm.xml", 1).unwrap_or_default().is_empty();
        let findings = xml::inspect(text, orm_present, &|fqcn| xml_lookup(project, &files, fqcn));
        let diagnostics: Vec<Diagnostic> = findings.iter().map(|f| f.diagnostic(text)).collect();
        let _ = host::publish_diagnostics(path, &diagnostics);
    }

    fn name_findings(&mut self, project: &Project, path: &str, text: &str, kotlin: bool) -> Vec<Finding> {
        let Some(class) = parse_class(text, path) else { return Vec::new() };
        let references = name_references(text, kotlin, &class);
        if references.is_empty() {
            return Vec::new();
        }

        let files = self.sources(project);
        let mut stems: HashMap<&str, usize> = HashMap::new();
        for file in &files {
            *stems.entry(file_stem(file)).or_default() += 1;
        }

        let mut wanted: Vec<String> = references.iter().map(|r| r.owner.clone()).collect();
        wanted.sort();
        wanted.dedup();
        for name in &wanted {
            let mut current = name.clone();
            for _ in 0..8 {
                self.load(project, &current, path, true);
                let Some(parent) = self.class(&current).and_then(|c| c.superclass.clone()) else { break };
                current = parent;
            }
        }

        let names = |owner: &str| -> Option<Vec<String>> {
            let mut chain = vec![owner.to_owned()];
            let mut attributes = Vec::new();

            while let Some(name) = chain.pop() {
                if stems.get(name.as_str()).copied().unwrap_or(0) > 1 || chain.len() > 8 {
                    return None;
                }

                let Some(Some(class)) = self.classes.get(&name) else {
                    return (stems.contains_key(name.as_str()) && !attributes.is_empty()).then_some(attributes);
                };

                attributes.extend(class.attributes.iter().map(|a| a.name.clone()));
                chain.extend(class.superclass.clone());
            }

            Some(attributes)
        };

        inspect_name_references(&references, &names)
    }

    fn query_findings(&mut self, project: &Project, text: &str, kotlin: bool) -> Vec<Finding> {
        let mut findings = inspect_query_parameters(text, kotlin);
        let references = named_query_references(text, kotlin);
        if references.is_empty() {
            return findings;
        }

        let Some(known) = self.named_queries(project) else { return findings };
        findings.extend(references.iter().filter(|(name, ..)| !known.contains(name)).map(|(name, start, end)| unknown_named_query(name, *start, *end)));

        findings
    }

    fn findings(&mut self, project: &Project, path: &str, text: &str, kotlin: bool) -> Vec<Finding> {
        let mut findings = self.findings_core(project, path, text, kotlin);

        if setting("checkMappingNames") {
            findings.extend(self.name_findings(project, path, text, kotlin));
        }

        if setting("checkQueryReferences") {
            findings.extend(self.query_findings(project, text, kotlin));
        }

        findings
    }

    fn findings_core(&mut self, project: &Project, path: &str, text: &str, kotlin: bool) -> Vec<Finding> {
        let signature = if setting("checkClassSignature") { inspect_class_signature(text, kotlin) } else { Vec::new() };

        if let Some(class) = parse_class(text, path) {
            self.remember(class);
            if self.scanned {
                host::refresh_view("structure");
            }
            let mut findings = inspect_entity(text, kotlin);
            if kotlin {
                findings.extend(inspect_kotlin_entity(text));
            }
            if kotlin && setting("checkKotlinEntities") {
                let build = self.build_of(project, path);
                findings.extend(inspect_kotlin_build(text, build));
            }
            if !setting("checkKotlinEntities") {
                findings.retain(|f| !matches!(f.code, "jpa-data-class" | "jpa-val-attribute" | "jpa-immutable-collection" | "jpa-kotlin-plugin-missing" | "jpa-redundant-constructor" | "jpa-redundant-open"));
            }
            findings.extend(signature);
            if setting("checkAttributeSignatures") {
                findings.extend(inspect_attribute_signatures(text, kotlin));
            }
            findings.extend(self.entity_rule_findings(project, path, text, kotlin));
            if setting("checkListenerUsage") {
                findings.extend(self.listener_findings(project, text));
            }
            return findings;
        }

        let mut early = signature;
        if setting("checkListenerSignatures") {
            early.extend(inspect_callbacks(text, kotlin));
        }
        if !early.is_empty() {
            return early;
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

    /// The entity right-clicked in JPA Structure, else the one open in the editor.
    fn entity_of(&mut self, ctx: &Invocation, item: Option<&str>) -> Result<Class, Error> {
        let Some(path) = item.and_then(|key| key.strip_prefix(ENTITY_KEY_PREFIX)) else {
            return self.current_entity(ctx);
        };
        let project = ctx.project().ok_or("Open a project first.")?;
        self.scan(&project)?;

        self.entities().into_iter().find(|c| c.path == path).cloned().ok_or_else(|| "That entity is no longer in the project.".into())
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

fn xml_lookup(project: &Project, files: &[String], fqcn: &str) -> Lookup {
    if fqcn.contains('$') {
        return Lookup::Found { annotated: true };
    }

    let relative = fqcn.replace('.', "/");
    let found = files.iter().find(|p| [".java", ".kt"].iter().any(|e| p.ends_with(&format!("/{relative}{e}"))));
    let Some(path) = found else { return Lookup::NotInProject };

    let text = project.read_file(path).unwrap_or_default();
    Lookup::Found { annotated: ["@Entity", "@MappedSuperclass", "@Embeddable", "@Converter"].iter().any(|m| text.contains(m)) }
}

fn publish_context(path: Option<&str>) {
    let text = path.and_then(|p| project_of(p)?.read_file(p).ok()).unwrap_or_default();
    let is_entity = path.is_some_and(|p| parse_class(&text, p).is_some_and(|c| c.kind == Kind::Entity));
    let has_create_table = path.is_some_and(|p| p.ends_with(".sql")) && text.to_lowercase().contains("create table");

    let _ = host::set_context("isEntity", is_entity);
    let _ = host::set_context("hasCreateTable", has_create_table);
}

impl Plugin for JpaAssist {
    fn activate(&mut self, _ctx: &Context) -> Result<(), Error> {
        let _ = host::subscribe(EventKind::EditorFocused);

        Ok(())
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        match id {
            "copy-ddl" => {
                let class = self.entity_of(ctx, arg)?;
                let Some(dialect) = choose_dialect()? else { return Ok(None) };
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

                let Some(dialect) = choose_dialect()? else { return Ok(None) };

                let known = |n: &str| self.class(n);
                copied(ddl(&entities, &known, dialect), &format!("DDL of {}", entities_count(entities.len())))
            }
            "copy-dto" => {
                let class = self.entity_of(ctx, arg)?;
                let known = |n: &str| self.class(n);
                let items: Vec<PickItem> =
                    dto_fields(&class, &known).iter().map(|(name, ty)| PickItem::new(name, name).description(ty)).collect();
                let Some(chosen) = choose_many("Fields of the DTO", &items)? else { return Ok(None) };
                if chosen.is_empty() {
                    host::notify(NotificationLevel::Info, "JPA Assist", "No field chosen, nothing copied.");
                    return Ok(None);
                }

                copied(dto(&class, &known, Some(&chosen)), &format!("{}Dto", class.name))
            }
            "copy-entity" => {
                let editor = ctx.editor().ok_or("Open a SQL file first.")?;
                let doc = editor.document()?;
                let caret = editor.selections()?.first().map_or(Position::new(0, 0), |s| s.start);
                let at = offset(&doc.text, caret).unwrap_or(doc.text.len());
                let Some(kotlin) = choose_entity_language()? else { return Ok(None) };
                let entity = entity_from_ddl(&doc.text, at, kotlin).ok_or("Put the caret in a CREATE TABLE statement.")?;
                copied(entity, "Entity")
            }
            "new-entity" => new_entity(ctx),
            "new-attribute" => new_attribute(ctx),
            _ => Err(Error::NotFound(format!("command {id}"))),
        }
    }

    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        for event in events {
            match event {
                Event::EditorFocused(path) => {
                    publish_context(path.as_deref());
                    continue;
                }
                Event::FileOpened(path) | Event::FileSaved(path) => {
                    if is_build_file(path) {
                        self.builds.clear();
                    }
                    publish_context(Some(path))
                }
                _ => continue,
            }

            let (Event::FileOpened(path) | Event::FileSaved(path)) = event else { continue };
            let Some(project) = project_of(path) else { continue };
            if path.ends_with(".xml") {
                if let Ok(text) = project.read_file(path) {
                    self.check_xml(&project, path, &text);
                }
                continue;
            }

            let Some(kotlin) = language_of(path) else { continue };
            let Ok(text) = project.read_file(path) else { continue };

            if let Some(Some(known)) = self.named_queries.as_mut()
                && let Some(declared) = declared_named_queries(&text, kotlin)
            {
                known.extend(declared);
            }

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

impl JpaAssist {
    /// "Add inverse attribute" for the owning-side association whose declaration (annotations included) the selection touches.
    fn inverse_actions(&mut self, doc: &Document, selection: Range, class: &Class) -> Vec<CodeAction> {
        let Some(project) = project_of(&doc.path) else { return Vec::new() };
        let lines: Vec<&str> = doc.text.lines().collect();
        let mut actions = Vec::new();

        for attribute in class.attributes.iter().filter(|a| matches!(a.relation, Relation::ManyToOne | Relation::OneToOne | Relation::ManyToMany) && !a.mapped_by) {
            let declared = attribute.pos.line;
            let reaches = selection.start.line <= declared
                && selection.end.line >= selection.start.line
                && declared - selection.start.line <= 4
                && (selection.start.line..declared).all(|l| lines.get(l as usize).is_some_and(|line| line.trim_start().starts_with('@')))
                && selection.end.line <= declared;
            if !reaches {
                continue;
            }

            self.load(&project, &attribute.target, &doc.path, true);
            let Some(target_path) = self.class(&attribute.target).map(|c| c.path.clone()).filter(|p| *p != doc.path) else { continue };
            let Ok(target_text) = project.read_file(&target_path) else { continue };
            let Some(target) = parse_class(&target_text, &target_path).filter(|c| c.kind == Kind::Entity) else { continue };
            let Some(spec) = inverse_spec(class, attribute, &target) else { continue };
            let Some(edits) = attribute_edits(&target, &target_text, &mask(&target_text), &spec) else { continue };

            actions.push(CodeAction {
                title: format!("Add inverse attribute '{}' to {}", spec.name, target.name),
                kind: Some("refactor".into()),
                preferred: false,
                edits: vec![(target_path, edits.iter().map(|e| e.to_text_edit(&target_text)).collect())],
                data: None,
            });
        }

        actions
    }

    /// Fully qualified class names of the project's sources for the text typed inside a `<class>` element.
    fn class_element_completion(&mut self, doc: &Document, at: Position) -> Vec<CompletionItem> {
        let Some(caret) = offset(&doc.text, at) else { return Vec::new() };
        let before = &doc.text[..caret];
        let Some(tag) = before.rfind('<') else { return Vec::new() };
        let Some(typed) = before[tag..].strip_prefix("<class>") else { return Vec::new() };
        let Some(project) = project_of(&doc.path) else { return Vec::new() };

        let typed = typed.trim();
        let mut names: Vec<String> = self
            .sources(&project)
            .iter()
            .filter_map(|path| {
                let (_, after_root) = path.split_once("/java/").or_else(|| path.split_once("/kotlin/"))?;
                let name = after_root.rsplit_once('.')?.0.replace('/', ".");
                name.starts_with(typed).then_some(name)
            })
            .collect();

        names.sort();
        names.truncate(100);
        names.into_iter().map(|n| CompletionItem::new(&n, CompletionKind::Class)).collect()
    }
}

impl Providers for JpaAssist {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if doc.path.ends_with(".xml") {
            return self.class_element_completion(doc, at);
        }

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

        if kotlin && setting("checkKotlinEntities") && let Some(build) = self.builds.get(&doc.path) {
            for finding in inspect_kotlin_build(text, build).into_iter().filter(|f| touches(f.start, f.end)) {
                if let Some((title, edits)) = finding.fix {
                    actions.push(action(doc, title, "quickfix", true, &edits));
                    continue;
                }

                if let Some(fix) = build.fix.as_ref().filter(|_| finding.code == "jpa-kotlin-plugin-missing") {
                    let file = fix.path.rsplit('/').next().unwrap_or(&fix.path);
                    actions.push(CodeAction {
                        title: format!("Add the kotlin-jpa plugin to {file}"),
                        kind: Some("quickfix".into()),
                        preferred: false,
                        edits: vec![(fix.path.clone(), vec![fix.edit.to_text_edit(&fix.text)])],
                        data: None,
                    });
                }
            }
        }

        let Some(class) = parse_class(text, &doc.path).filter(|c| c.kind == Kind::Entity) else { return actions };
        actions.extend(self.inverse_actions(doc, selection, &class));

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

        let declared: Vec<&str> = annotations(&masked).iter().map(|a| a.name).collect();

        let declares_method = |name: &str| words(&masked, name).any(|i| masked[i + name.len()..].trim_start().starts_with('('));
        let crlf = text.contains("\r\n");

        for callback in CALLBACKS.iter().filter(|c| !declared.contains(c)) {
            if declares_method(&decapitalize(callback)) {
                continue;
            }
            let Some(method) = callback_method(&class, callback) else { continue };
            let mut edits: Vec<Edit> = import_edit(text, &[&callback_import(&class, callback)], kotlin).into_iter().collect();
            edits.push(method);
            if crlf {
                edits.iter_mut().for_each(|e| e.text = e.text.replace('\n', "\r\n"));
            }
            actions.push(action(doc, &format!("Add @{callback} callback"), "source", false, &edits));
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
            let key = format!("{ENTITY_KEY_PREFIX}{}", entity.path);
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
                let attribute_key = format!("attribute:{}#{}", entity.path, a.name);
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

    fn kotlin_codes(text: &str) -> Vec<String> {
        problems(&host(), "src/shop/K.kt", text)
    }

    #[test]
    fn val_attribute_in_an_entity_warns_on_the_name() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  val name: String = \"\"\n}\n";
        let found = checks::inspect_kotlin_entity(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "jpa-val-attribute");
        assert_eq!(&text[found[0].start..found[0].end], "val");
    }

    #[test]
    fn val_in_constructor_and_in_body_both_warn() {
        let text = "@Entity\nclass K(@Id var id: Long?, val a: String) {\n  val b: String = \"\"\n}\n";
        assert_eq!(kotlin_codes(text), ["jpa-val-attribute", "jpa-val-attribute"]);
    }

    #[test]
    fn val_id_is_quiet() {
        assert!(kotlin_codes("@Entity\nclass K(@Id val id: Long)\n").is_empty());
        assert!(kotlin_codes("@Entity\nclass K(@EmbeddedId val key: Key)\n").is_empty());
        assert!(kotlin_codes("@Entity\nclass K {\n  @Id\n  val id: Long = 0\n}\n").is_empty());
    }

    #[test]
    fn val_transient_is_quiet() {
        assert!(kotlin_codes("@Entity\nclass K(@Id var id: Long?, @Transient val t: String)\n").is_empty());
    }

    #[test]
    fn immutable_hibernate_entity_is_quiet() {
        assert!(kotlin_codes("@Entity\n@Immutable\nclass K(@Id val id: Long, val a: String)\n").is_empty());
    }

    #[test]
    fn val_in_embeddable_is_quiet() {
        assert!(kotlin_codes("@Embeddable\nclass K(val a: String)\n").is_empty());
        assert!(kotlin_codes("@MappedSuperclass\nclass K(val a: String)\n").is_empty());
    }

    #[test]
    fn var_attribute_is_quiet() {
        assert!(kotlin_codes("@Entity\nclass K(@Id var id: Long?, var a: String)\n").is_empty());
    }

    #[test]
    fn java_entity_is_ignored() {
        let host = host();
        assert!(problems(&host, "src/shop/J.java", "@Entity\nclass J {\n  @Id Long id;\n  val a = 1;\n}\n").is_empty());
    }

    #[test]
    fn val_attribute_is_quiet_for_library_or_dynamic_name() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  val a: Int\n    get() = 1\n  val b by lazy { 1 }\n  val c: Int get() = 2\n  fun f() { val x = 1 }\n  companion object { val Y = 1 }\n}\n";
        assert!(kotlin_codes(text).is_empty(), "{:?}", kotlin_codes(text));
        assert!(kotlin_codes("class Plain(val a: String)\n").is_empty(), "not an entity");
        assert!(checks::inspect_kotlin_entity("@Entity\nclass K(val").is_empty(), "unfinished");
        assert!(checks::inspect_kotlin_entity("@Entity\nclass K(@Id var i: Long) {\n  val").is_empty(), "unfinished body");
    }

    #[test]
    fn immutable_list_on_one_to_many_warns() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  @OneToMany(mappedBy = \"k\")\n  var items: List<Item> = listOf()\n}\n";
        let found = checks::inspect_kotlin_entity(text);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code, "jpa-immutable-collection");
        assert_eq!(&text[found[0].start..found[0].end], "List");
    }

    #[test]
    fn mutable_list_on_one_to_many_is_quiet() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  @OneToMany var a: MutableList<Item> = mutableListOf()\n  @OneToMany var b: MutableSet<Item>? = null\n}\n";
        assert!(kotlin_codes(text).is_empty());
    }

    #[test]
    fn arraylist_on_many_to_many_is_quiet() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  @ManyToMany var a: ArrayList<Item> = ArrayList()\n  @ManyToMany var b: HashSet<Item> = HashSet()\n  @ManyToMany var c: LinkedHashSet<Item> = LinkedHashSet()\n}\n";
        assert!(kotlin_codes(text).is_empty());
    }

    #[test]
    fn immutable_list_without_a_relation_annotation_is_quiet() {
        assert!(kotlin_codes("@Entity\nclass K(@Id var id: Long?) {\n  var tags: List<String> = listOf()\n}\n").is_empty());
    }

    #[test]
    fn element_collection_set_warns() {
        let text = "@Entity\nclass K(@Id var id: Long?) {\n  @ElementCollection var tags: Set<String> = setOf()\n}\n";
        assert_eq!(kotlin_codes(text), ["jpa-immutable-collection"]);
    }

    const DATA: &str = "@Entity\ndata class K(@Id var id: Long?) {\n}\n";

    #[test]
    fn data_class_entity_warns_without_the_three_overrides() {
        assert_eq!(kotlin_codes(DATA), ["jpa-data-class"]);
    }

    #[test]
    fn data_class_entity_with_equals_hashcode_and_tostring_is_quiet() {
        let text = "@Entity\ndata class K(@Id var id: Long?) {\n  override fun equals(other: Any?) = other is K\n  override fun hashCode() = 1\n  override fun toString() = \"K\"\n}\n";
        assert!(kotlin_codes(text).is_empty());
    }

    #[test]
    fn data_class_with_only_equals_still_warns() {
        let text = "@Entity\ndata class K(@Id var id: Long?) {\n  override fun equals(other: Any?) = other is K\n}\n";
        assert_eq!(kotlin_codes(text), ["jpa-data-class"]);
    }

    #[test]
    fn kotlin_checks_can_be_switched_off() {
        let host = host();
        host.set_setting("jpaAssist.checkKotlinEntities", "false");
        let text = "@Entity\ndata class K(@Id var id: Long?, val a: String) {\n  @OneToMany var l: List<X> = listOf()\n}\n";
        assert!(problems(&host, "src/shop/K.kt", text).is_empty());
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = host();
        assert_eq!(problems(&host, "src/shop/K.kt", "@Entity\nclass K(@Id var id: Long?, val a: String)\n"), ["jpa-val-attribute"]);
        assert!(problems(&host, "src/shop/K.kt", "@Entity\nclass K(@Id var id: Long?, var a: String)\n").is_empty());
    }

    fn first_range(host: &MockHost, path: &str) -> (u32, u32) {
        let d = &host.diagnostics()[&format!("/project/{path}")][0];
        (d.range.start.line, d.range.start.character)
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let host = host();
        let text = "@Entity\nclass K(@Id var id: Long?, /* \u{1F600} */ val a: String)\n";
        assert_eq!(problems(&host, "src/shop/K.kt", text), ["jpa-val-attribute"]);
        assert_eq!(first_range(&host, "src/shop/K.kt"), (1, 36));
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let host = host();
        let text = "@Entity\r\nclass K(\r\n  @Id var id: Long?,\r\n  val a: String\r\n)\r\n";
        assert_eq!(problems(&host, "src/shop/K.kt", text), ["jpa-val-attribute"]);
        assert_eq!(first_range(&host, "src/shop/K.kt"), (3, 2));
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
        let titles: Vec<String> = plugin
            .code_actions(&doc, Range::new(class_line, class_line))
            .into_iter()
            .map(|a| a.title)
            .filter(|t| !t.starts_with("Add @"))
            .collect();
        assert_eq!(titles, ["Generate equals() and hashCode()", "Generate toString()"]);

        let association = Position::new(11, 6);
        let titles: Vec<String> = plugin.code_actions(&doc, Range::new(association, association)).into_iter().map(|a| a.title).collect();
        assert_eq!(titles, ["Fetch lazily", "Add inverse attribute 'orders' to Customer"]);
    }

    #[test]
    fn copies_ddl_dto_and_entities() {
        let host = host();
        let mut plugin = JpaAssist::default();
        host.open_editor("/project/src/shop/Order.java", "java", ORDER, &[(0, 0, 0, 0)]);

        host.answer_next_prompt(Some(&["postgresql"]));
        plugin.run_command("copy-ddl", &host.invocation(), None).unwrap();
        let sql = host.clipboard().unwrap();
        assert!(sql.contains("CREATE TABLE order\n"), "{sql}");
        assert!(sql.contains("REFERENCES customer (id)"), "{sql}");

        host.answer_next_prompt(Some(&["number", "customerId"]));
        plugin.run_command("copy-dto", &host.invocation(), None).unwrap();
        let dto = host.clipboard().unwrap();
        assert!(dto.contains("public record OrderDto("), "{dto}");
        assert!(dto.contains("String number") && dto.contains("customerId") && !dto.contains("Long id"), "{dto}");

        host.answer_next_prompt(Some(&["postgresql"]));
        plugin.run_command("copy-ddl-all", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications().last().unwrap().body, "DDL of 2 entities copied to the clipboard.");

        host.open_editor("/project/db.sql", "sql", "create table tag (id bigint primary key, label text);", &[(0, 20, 0, 20)]);
        host.answer_next_prompt(Some(&["java"]));
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

    const EMPTY_JAVA: &str = "/project/src/main/java/shop/Tag.java";

    fn wizard_host(path: &str, language: &str, text: &str) -> MockHost {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "editor.write", "storage", "clipboard"]);
        host.open_editor(path, language, text, &[(0, 0, 0, 0)]);
        host
    }

    fn answers(host: &MockHost, all: &[&[&str]]) {
        all.iter().for_each(|a| host.answer_next_prompt(Some(a)));
    }

    #[test]
    fn new_entity_goes_to_the_clipboard_and_the_empty_file() {
        let host = wizard_host(EMPTY_JAVA, "java", "");
        answers(&host, &[&["Tag"], &["tags"], &["IDENTITY"], &["version", "equals"]]);
        JpaAssist::default().run_command("new-entity", &host.invocation(), None).unwrap();

        let source = host.clipboard().unwrap();
        assert_eq!(host.editor_text(), source);
        for expected in [
            "package shop;",
            "import jakarta.persistence.Version;",
            "@Table(name = \"tags\")",
            "public class Tag {",
            "@GeneratedValue(strategy = GenerationType.IDENTITY)",
            "private Long id;",
            "public Long getId()",
            "private Long version;",
            "public boolean equals(Object o)",
        ] {
            assert!(source.contains(expected), "{expected}\n{source}");
        }
        assert!(!source.contains("AuditingEntityListener"));
        assert!(parse_class(&source, "Tag.java").is_some_and(|c| c.kind == Kind::Entity));
    }

    #[test]
    fn new_entity_in_kotlin_uses_vars_and_the_uuid_type() {
        let host = wizard_host("/project/src/main/kotlin/shop/Tag.kt", "kotlin", "\n");
        answers(&host, &[&["tag"], &["tag"], &["UUID"], &["auditing", "equals"]]);
        JpaAssist::default().run_command("new-entity", &host.invocation(), None).unwrap();

        let source = host.clipboard().unwrap();
        for expected in [
            "package shop\n",
            "import java.time.Instant\n",
            "@EntityListeners(AuditingEntityListener::class)",
            "@GeneratedValue(strategy = GenerationType.UUID)",
            "var id: UUID? = null",
            "var createdAt: Instant? = null",
            "override fun hashCode()",
        ] {
            assert!(source.contains(expected), "{expected}\n{source}");
        }
        assert!(!source.contains(';') && !source.contains("private "), "{source}");
        assert_eq!(host.editor_text(), source);
    }

    #[test]
    fn new_entity_keeps_a_file_that_has_content_and_follows_the_setting() {
        let host = wizard_host("/project/src/main/java/shop/Tag.java", "java", "class Tag {}");
        answers(&host, &[&["Tag"], &["tag"], &["none"], &[]]);
        JpaAssist::default().run_command("new-entity", &host.invocation(), None).unwrap();

        assert_eq!(host.editor_text(), "class Tag {}");
        assert!(!host.clipboard().unwrap().contains("@GeneratedValue"));
    }

    #[test]
    fn dismissing_any_new_entity_prompt_changes_nothing() {
        let steps: [&[&str]; 4] = [&["Tag"], &["tags"], &["UUID"], &["version"]];
        for answered in 0..steps.len() {
            let host = wizard_host(EMPTY_JAVA, "java", "");
            answers(&host, &steps[..answered]);
            JpaAssist::default().run_command("new-entity", &host.invocation(), None).unwrap();

            assert_eq!(host.clipboard(), None, "dismissed at prompt {answered}");
            assert_eq!(host.editor_text(), "");
        }
    }

    #[test]
    fn new_entity_rejects_a_bad_class_name() {
        let host = wizard_host(EMPTY_JAVA, "java", "");
        answers(&host, &[&["1 Tag"]]);
        assert!(JpaAssist::default().run_command("new-entity", &host.invocation(), None).is_err());
    }

    #[test]
    fn new_attribute_adds_a_typed_field_with_its_import() {
        let host = wizard_host("/project/src/shop/Order.java", "java", ORDER);
        answers(&host, &[&["total"], &["none"], &["BigDecimal"], &["notNull"]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();

        let text = host.editor_text();
        assert!(text.contains("import java.math.BigDecimal;\n"), "{text}");
        assert!(text.contains("    @Column(nullable = false)\n    private BigDecimal total;\n}"), "{text}");
        assert!(!text.contains("getTotal"), "no accessors in a class without getters");
        assert!(parse_class(&text, "Order.java").unwrap().attributes.iter().any(|a| a.name == "total"));
    }

    #[test]
    fn new_attribute_adds_a_lazy_cascading_collection() {
        let host = wizard_host("/project/src/shop/Order.java", "java", ORDER);
        answers(&host, &[&["lines"], &["OneToMany"], &["OrderLine"], &["cascade"]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();

        let text = host.editor_text();
        assert!(text.contains("@OneToMany(cascade = CascadeType.ALL)\n    private List<OrderLine> lines = new ArrayList<>();"), "{text}");
        assert!(text.contains("import java.util.ArrayList;") && text.contains("import java.util.List;"), "{text}");
    }

    #[test]
    fn new_attribute_in_kotlin_is_a_var() {
        let kotlin = "package shop\n\nimport jakarta.persistence.*\n\n@Entity\nclass Order {\n    @Id\n    var id: Long? = null\n}\n";
        let host = wizard_host("/project/src/shop/Order.kt", "kotlin", kotlin);
        answers(&host, &[&["customer"], &["ManyToOne"], &["Customer"], &["notNull", "fetch"]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();
        answers(&host, &[&["count"], &["none"], &["Integer"], &[]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();

        let text = host.editor_text();
        assert!(text.contains("@ManyToOne(fetch = FetchType.LAZY, optional = false)\n    var customer: Customer? = null"), "{text}");
        assert!(text.contains("    var count: Int? = null\n}"), "{text}");
        assert!(!text.contains(';'));
    }

    #[test]
    fn new_attribute_marks_enums_as_string_enumerated() {
        let host = wizard_host("/project/src/shop/Order.java", "java", ORDER);
        answers(&host, &[&["status"], &["none"], &["enum"], &["OrderStatus"], &[]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();

        assert!(host.editor_text().contains("@Enumerated(EnumType.STRING)\n    private OrderStatus status;"));
    }

    #[test]
    fn new_attribute_dismissed_or_outside_an_entity_edits_nothing() {
        let steps: [&[&str]; 4] = [&["total"], &["none"], &["Long"], &["unique"]];
        for answered in 0..steps.len() {
            let host = wizard_host("/project/src/shop/Order.java", "java", ORDER);
            answers(&host, &steps[..answered]);
            JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();
            assert_eq!(host.editor_text(), ORDER, "dismissed at prompt {answered}");
        }

        let host = wizard_host("/project/src/notes.java", "java", "class Plain {}");
        assert!(JpaAssist::default().run_command("new-attribute", &host.invocation(), None).is_err());

        let host = wizard_host("/project/src/shop/Order.java", "java", ORDER);
        answers(&host, &[&["number"], &["none"], &["String"], &[]]);
        assert!(JpaAssist::default().run_command("new-attribute", &host.invocation(), None).is_err());
        assert_eq!(host.editor_text(), ORDER);
    }

    #[test]
    fn new_attribute_without_editor_write_shows_a_notice() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "storage"]);
        host.open_editor("/project/src/shop/Order.java", "java", ORDER, &[(0, 0, 0, 0)]);
        answers(&host, &[&["total"], &["none"], &["Long"], &[]]);
        JpaAssist::default().run_command("new-attribute", &host.invocation(), None).unwrap();

        assert_eq!(host.editor_text(), ORDER);
        assert!(host.notifications()[0].body.contains("Allow editor changes"));
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
                ("new-attribute".to_owned(), "(editorLangId == java || editorLangId == kotlin) && plugin.aritti.jpa-assist.isEntity".to_owned()),
                ("copy-ddl".to_owned(), "(editorLangId == java || editorLangId == kotlin) && plugin.aritti.jpa-assist.isEntity".to_owned()),
                ("copy-dto".to_owned(), "(editorLangId == java || editorLangId == kotlin) && plugin.aritti.jpa-assist.isEntity".to_owned()),
                ("copy-entity".to_owned(), "editorLangId == sql && plugin.aritti.jpa-assist.hasCreateTable".to_owned()),
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

        plugin.handle("structure", ViewEvent::Activate("attribute:/project/src/shop/Order.java#customer".into()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/src/shop/Order.java".to_owned(), 12, 21)]);
    }

    #[test]
    fn structure_view_expands_entities_on_request() {
        let host = host();
        let mut plugin = JpaAssist::default();
        let order = "entity:/project/src/shop/Order.java";
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
        host.answer_next_prompt(Some(&["id"]));
        let result = JpaAssist::default().run_command("copy-dto", &host.invocation(), None).unwrap();
        assert_eq!(result, None);
        assert_eq!(
            host.notifications()[0].body,
            "Allow the clipboard for JPA Assist in Settings › Plugins to copy generated code."
        );
    }

    fn context_of(host: &MockHost) -> (bool, bool) {
        let keys = host.context_keys();
        (keys["isEntity"] == true.into(), keys["hasCreateTable"] == true.into())
    }

    #[test]
    fn context_keys_follow_the_file_in_focus() {
        let host = host();
        host.add_file("src/db.sql", "create table tag (id bigint);");
        let mut plugin = JpaAssist::default();
        let focus = |plugin: &mut JpaAssist, path: Option<&str>| {
            plugin.handle_events(&[Event::EditorFocused(path.map(str::to_owned))], &host.invocation());
            context_of(&host)
        };

        assert_eq!(focus(&mut plugin, Some("/project/src/shop/Order.java")), (true, false));
        assert_eq!(focus(&mut plugin, Some("/project/src/notes.txt")), (false, false));
        assert_eq!(focus(&mut plugin, Some("/project/src/db.sql")), (false, true));
        assert_eq!(focus(&mut plugin, None), (false, false));

        plugin.handle_events(&[Event::FileOpened("/project/src/shop/Customer.java".into())], &host.invocation());
        assert_eq!(context_of(&host), (true, false));
    }

    #[test]
    fn right_clicking_an_entity_copies_its_ddl_and_chosen_dto_fields() {
        let host = host();
        let mut plugin = JpaAssist::default();
        let key = "entity:/project/src/shop/Customer.java";

        host.answer_next_prompt(Some(&["mysql"]));
        plugin.run_command("copy-ddl", &host.invocation(), Some(key)).unwrap();
        assert!(host.clipboard().unwrap().contains("CREATE TABLE customer"));

        host.answer_next_prompt(Some(&["name"]));
        plugin.run_command("copy-dto", &host.invocation(), Some(key)).unwrap();
        let dto = host.clipboard().unwrap();
        assert!(dto.contains("String name") && !dto.contains("Long id"), "{dto}");
    }

    #[test]
    fn dismissing_a_prompt_copies_nothing() {
        let host = host();
        let mut plugin = JpaAssist::default();
        host.open_editor("/project/src/shop/Order.java", "java", ORDER, &[(0, 0, 0, 0)]);

        for command in ["copy-ddl", "copy-dto"] {
            plugin.run_command(command, &host.invocation(), None).unwrap();
        }
        assert_eq!(host.clipboard(), None);
    }

    #[test]
    fn the_dialect_comes_from_the_prompt() {
        let host = host();

        assert_eq!(choose_dialect().unwrap(), None);
        host.answer_next_prompt(Some(&["mysql"]));
        assert_eq!(choose_dialect().unwrap(), Some(Dialect::MySql));
    }

    fn java_codes(text: &str) -> Vec<String> {
        problems(&host(), "src/shop/J.java", text)
    }

    #[test]
    fn inner_entity_class_warns() {
        let text = "class Outer {\n  @Entity\n  class In {\n    @Id Long id;\n  }\n}\n";
        assert_eq!(java_codes(text), ["jpa-entity-nested"]);
        let found = checks::inspect_class_signature(text, false);
        assert_eq!(&text[found[0].start..found[0].end], "class");
    }

    #[test]
    fn static_nested_entity_is_quiet() {
        assert!(java_codes("class Outer {\n  @Entity\n  public static class In {\n    @Id Long id;\n  }\n}\n").is_empty());
        assert!(java_codes("class Outer {\n  @Entity static class In {\n    @Id Long id;\n  }\n}\n").is_empty());
    }

    #[test]
    fn kotlin_nested_entity_is_quiet_and_inner_warns() {
        let nested = "class Outer {\n  @Entity\n  class In(@Id var id: Long?)\n}\n";
        assert!(kotlin_codes(nested).is_empty());
        let inner = "class Outer {\n  @Entity\n  inner class In(@Id var id: Long?)\n}\n";
        assert_eq!(kotlin_codes(inner), ["jpa-entity-nested"]);
    }

    #[test]
    fn entity_with_embeddable_warns_on_the_second_annotation() {
        let text = "@Entity\n@Embeddable\nclass E {\n  @Id Long id;\n}\n";
        assert_eq!(java_codes(text), ["jpa-entity-multiple-roles"]);
        let found = checks::inspect_class_signature(text, false);
        assert_eq!(&text[found[0].start..found[0].end], "@Embeddable");

        let reversed = "@Embeddable\n@Entity\nclass E {\n  @Id Long id;\n}\n";
        let found = checks::inspect_class_signature(reversed, false);
        assert_eq!(&reversed[found[0].start..found[0].end], "@Entity");
    }

    #[test]
    fn entity_with_mapped_superclass_warns() {
        assert_eq!(java_codes("@Entity\n@MappedSuperclass\nclass E {\n  @Id Long id;\n}\n"), ["jpa-entity-multiple-roles"]);
    }

    #[test]
    fn entity_on_interface_warns() {
        let text = "@Entity\ninterface I {}\n";
        assert_eq!(java_codes(text), ["jpa-entity-not-a-class"]);
        let found = checks::inspect_class_signature(text, false);
        assert_eq!(&text[found[0].start..found[0].end], "interface");
        assert_eq!(checks::inspect_class_signature("@Entity\n@interface A {}\n", false).len(), 1);
        assert_eq!(checks::inspect_class_signature("@Entity\nannotation class A\n", true).len(), 1);
    }

    #[test]
    fn entity_on_enum_warns() {
        assert_eq!(java_codes("@Entity\nenum E { A }\n\nclass Later {}\n"), ["jpa-entity-not-a-class"]);
        assert_eq!(kotlin_codes("@Entity\nenum class E { A }\n"), ["jpa-entity-not-a-class"]);
    }

    #[test]
    fn entity_signature_is_quiet_for_library_or_dynamic_name() {
        assert!(java_codes("@Entity\nclass E {\n  @Id Long id;\n}\n").is_empty());
        assert!(java_codes("@Entity\nclass E {\n  @Id Long id;\n}\n\nclass Later {}\n").is_empty());
        assert!(java_codes("class Entity {}\nclass Embeddable {}\n").is_empty());
        assert!(java_codes("@Entity\n@EntityListeners(A.class)\nclass E {\n  @Id Long id;\n}\n").is_empty());
        for unfinished in ["@Entity", "@Entity\nclass", "@Entity\nenum", "class A {\n @Entity\n", "@Entity @Embeddable"] {
            checks::inspect_class_signature(unfinished, false);
            checks::listener_references(unfinished);
        }
    }

    #[test]
    fn kotlin_class_literal_in_an_annotation_is_not_the_class() {
        let text = "@Entity\n@EntityListeners(AuditListener::class)\nclass K(@Id var id: Long?, val name: String)\n";
        let found = checks::inspect_kotlin_entity(text);
        assert_eq!(found.len(), 1);
        assert_eq!(&text[found[0].start..found[0].end], "val");
        assert!(checks::inspect_class_signature(text, true).is_empty());
    }

    const LISTENED: &str = "@Entity\n@EntityListeners(AuditListener.class)\nclass E {\n  @Id Long id;\n}\n";

    fn with_listener(listener: &str) -> MockHost {
        let host = host();
        host.add_file("src/shop/AuditListener.java", listener);
        host
    }

    #[test]
    fn listener_without_callbacks_warns_on_the_class_reference() {
        let host = with_listener("class AuditListener {\n  void touch() {}\n}\n");
        assert_eq!(problems(&host, "src/shop/E.java", LISTENED), ["jpa-listener-without-callbacks"]);
        assert_eq!(first_range(&host, "src/shop/E.java"), (1, 17));
        let found = checks::listener_references(LISTENED);
        assert_eq!(&LISTENED[found[0].1..found[0].2], "AuditListener");
    }

    #[test]
    fn listener_with_a_callback_is_quiet() {
        let host = with_listener("class AuditListener {\n  @PrePersist void touch(Object o) {}\n}\n");
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
    }

    #[test]
    fn unresolved_listener_is_quiet() {
        assert!(problems(&host(), "src/shop/E.java", LISTENED).is_empty());
    }

    #[test]
    fn listener_found_in_two_files_is_quiet() {
        let host = with_listener("class AuditListener {}\n");
        host.add_file("src/other/AuditListener.java", "class AuditListener {}\n");
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
    }

    #[test]
    fn listener_with_a_superclass_is_quiet() {
        let host = with_listener("class AuditListener extends BaseListener {}\n");
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
    }

    #[test]
    fn listener_in_kotlin_entity_is_read() {
        let host = host();
        host.add_file("src/shop/AuditListener.kt", "class AuditListener {\n  fun touch() {}\n}\n");
        let text = "@Entity\n@EntityListeners(AuditListener::class, value = [Other::class])\nclass K(@Id var id: Long?)\n";
        assert_eq!(problems(&host, "src/shop/K.kt", text), ["jpa-listener-without-callbacks"]);

        host.add_file("src/shop/AuditListener.kt", "class AuditListener : Base() {\n}\n");
        assert!(problems(&host, "src/shop/K.kt", text).is_empty());
    }

    #[test]
    fn fixing_the_listener_clears_the_warning_on_the_entity_after_reopen() {
        let host = with_listener("class AuditListener {}\n");
        assert_eq!(problems(&host, "src/shop/E.java", LISTENED), ["jpa-listener-without-callbacks"]);
        host.add_file("src/shop/AuditListener.java", "class AuditListener {\n  @PostLoad void loaded(Object o) {}\n}\n");
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
    }

    fn actions_at(path: &str, text: &str, line: u32) -> Vec<CodeAction> {
        let _host = host();
        let doc = Document { path: format!("/project/{path}"), language: "java".into(), version: 1, text: text.into() };
        let at = Position::new(line, 6);
        let all = JpaAssist::default().code_actions(&doc, Range::new(at, at));
        all.into_iter().filter(|a| a.title.starts_with("Add @")).collect()
    }

    const BARE: &str = "@Entity\nclass E {\n  @Id Long id;\n}\n";

    #[test]
    fn class_line_offers_one_action_per_missing_callback() {
        let titles: Vec<String> = actions_at("src/shop/E.java", BARE, 1).into_iter().map(|a| a.title).collect();
        assert_eq!(
            titles,
            [
                "Add @PrePersist callback",
                "Add @PostPersist callback",
                "Add @PreUpdate callback",
                "Add @PostUpdate callback",
                "Add @PreRemove callback",
                "Add @PostRemove callback",
                "Add @PostLoad callback"
            ]
        );

        let first = &actions_at("src/shop/E.java", BARE, 1)[0];
        assert_eq!(first.kind.as_deref(), Some("source"));
        let method = first.edits[0].1.last().unwrap();
        assert_eq!(method.text, "\n    @PrePersist void prePersist() {}\n");
        assert_eq!(method.range.start, Position::new(3, 0));
    }

    #[test]
    fn declared_callbacks_are_not_offered_again() {
        let text = "@Entity\nclass E {\n  @Id Long id;\n  @PrePersist void p() {}\n  @PostLoad void l() {}\n}\n";
        let titles: Vec<String> = actions_at("src/shop/E.java", text, 1).into_iter().map(|a| a.title).collect();
        assert_eq!(titles.len(), 5);
        assert!(!titles.iter().any(|t| t.contains("PrePersist") || t.contains("PostLoad")));
    }

    #[test]
    fn callback_action_is_skipped_when_its_method_name_is_taken() {
        let text = "@Entity\nclass E {\n  @Id Long id;\n  void prePersist() {}\n  void postLoad (String s) {}\n  String preUpdateNote;\n}\n";
        let titles: Vec<String> = actions_at("src/shop/E.java", text, 1).into_iter().map(|a| a.title).collect();
        assert_eq!(titles.len(), 5, "{titles:?}");
        assert!(!titles.iter().any(|t| t.contains("PrePersist") || t.contains("PostLoad")), "{titles:?}");
        assert!(titles.iter().any(|t| t.contains("PreUpdate")), "a field of a similar name does not count");
    }

    #[test]
    fn callback_action_keeps_crlf_line_endings() {
        let text = BARE.replace('\n', "\r\n");
        let action = &actions_at("src/shop/E.java", &text, 1)[0];
        for edit in &action.edits[0].1 {
            assert!(!edit.text.replace("\r\n", "").contains('\n'), "{:?}", edit.text);
        }
        assert_eq!(action.edits[0].1.last().unwrap().text, "\r\n    @PrePersist void prePersist() {}\r\n");
    }

    #[test]
    fn callback_action_adds_the_import_once() {
        let actions = actions_at("src/shop/E.java", BARE, 1);
        let imports: Vec<&str> = actions[0].edits[0].1.iter().map(|e| e.text.as_str()).filter(|t| t.contains("import")).collect();
        assert_eq!(imports, ["import jakarta.persistence.PrePersist;\n\n"]);

        let imported = "import jakarta.persistence.PrePersist;\n@Entity\nclass E {\n  @Id Long id;\n}\n";
        let actions = actions_at("src/shop/E.java", imported, 2);
        assert_eq!(actions[0].edits[0].1.len(), 1, "PrePersist is imported already");
        assert_eq!(actions[1].edits[0].1.len(), 2, "import and method for another callback");
        let wildcard = "import jakarta.persistence.*;\n@Entity\nclass E {\n  @Id Long id;\n}\n";
        assert!(actions_at("src/shop/E.java", wildcard, 2).iter().all(|a| a.edits[0].1.len() == 1));
    }

    #[test]
    fn callback_action_edits_only_the_open_file() {
        for action in actions_at("src/shop/E.java", BARE, 1) {
            assert_eq!(action.edits.len(), 1);
            assert_eq!(action.edits[0].0, "/project/src/shop/E.java");
        }
    }

    #[test]
    fn callback_action_uses_javax_when_the_file_does_too() {
        let text = "import javax.persistence.Entity;\nimport javax.persistence.Id;\n@Entity\nclass E {\n  @Id Long id;\n}\n";
        let actions = actions_at("src/shop/E.java", text, 3);
        let inserted: String = actions[0].edits[0].1.iter().map(|e| e.text.clone()).collect();
        assert!(inserted.contains("import javax.persistence.PrePersist;"), "{inserted}");
        assert!(!inserted.contains("jakarta"));
    }

    #[test]
    fn callback_action_is_quiet_off_the_class_line_and_for_non_entities() {
        assert!(actions_at("src/shop/E.java", BARE, 2).is_empty());
        assert!(actions_at("src/shop/P.java", "class P {\n}\n", 0).is_empty());
        assert!(actions_at("src/shop/P.java", "@Embeddable\nclass P {\n}\n", 1).is_empty());
        assert!(actions_at("src/shop/P.java", "@Entity\nclass P", 1).is_empty(), "unfinished body");
    }

    #[test]
    fn callback_action_for_kotlin_uses_fun() {
        let _host = host();
        let text = "@Entity\nclass K(@Id var id: Long?) {\n}\n";
        let doc = Document { path: "/project/src/shop/K.kt".into(), language: "kotlin".into(), version: 1, text: text.into() };
        let at = Position::new(1, 6);
        let actions = JpaAssist::default().code_actions(&doc, Range::new(at, at));
        let action = actions.iter().find(|a| a.title == "Add @PostLoad callback").unwrap();
        let inserted: String = action.edits[0].1.iter().map(|e| e.text.clone()).collect();
        assert!(inserted.contains("@PostLoad fun postLoad() {}"), "{inserted}");
        assert!(inserted.contains("import jakarta.persistence.PostLoad\n"), "{inserted}");
    }

    #[test]
    fn class_signature_group_can_be_switched_off() {
        let host = host();
        host.set_setting("jpaAssist.checkClassSignature", "false");
        assert!(problems(&host, "src/shop/J.java", "@Entity\n@Embeddable\nclass E {\n  @Id Long id;\n}\n").is_empty());
        assert!(problems(&host, "src/shop/I.java", "@Entity\ninterface I {}\n").is_empty());
    }

    #[test]
    fn listener_usage_group_can_be_switched_off() {
        let host = with_listener("class AuditListener {}\n");
        host.set_setting("jpaAssist.checkListenerUsage", "false");
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
    }

    #[test]
    fn signature_and_listener_ranges_use_utf16_columns_with_multibyte_text() {
        let host = host();
        assert_eq!(problems(&host, "src/shop/I.java", "@Entity\n/* \u{1F600} */ interface I {}\n"), ["jpa-entity-not-a-class"]);
        assert_eq!(first_range(&host, "src/shop/I.java"), (1, 9));

        let host = with_listener("class AuditListener {}\n");
        let text = "@Entity\n@EntityListeners(/* \u{1F600} */ AuditListener.class)\nclass E {\n  @Id Long id;\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", text), ["jpa-listener-without-callbacks"]);
        assert_eq!(first_range(&host, "src/shop/E.java"), (1, 26));
    }

    #[test]
    fn signature_findings_have_correct_lines_in_crlf_files() {
        let host = host();
        let text = "@Entity\r\n@Embeddable\r\nclass E {\r\n  @Id Long id;\r\n}\r\n";
        assert_eq!(problems(&host, "src/shop/E.java", text), ["jpa-entity-multiple-roles"]);
        assert_eq!(first_range(&host, "src/shop/E.java"), (1, 0));
    }

    #[test]
    fn missing_file_is_treated_as_absent() {
        let host = host();
        let mut plugin = JpaAssist::default();
        plugin.handle_events(&[Event::FileOpened("/project/src/shop/Gone.java".into())], &host.invocation());
        assert!(!host.diagnostics().contains_key("/project/src/shop/Gone.java"));
        assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty(), "listener file not in the project");
    }


    #[test]
    fn two_associations_on_one_attribute_warn_on_the_second() {
        let host = host();
        let text = "@Entity\nclass E {\n  @Id Long id;\n  @OneToOne(fetch = FetchType.LAZY) @ManyToOne(fetch = FetchType.LAZY) Customer c;\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", text), ["jpa-attribute-multiple-relations"]);
        assert_eq!(first_range(&host, "src/shop/E.java"), (3, 36));
        let ok = "@Entity\nclass E {\n  @Id Long id;\n  @ManyToOne(fetch = FetchType.LAZY) Customer c;\n  @OneToMany List<Customer> d;\n}\n";
        assert!(problems(&host, "src/shop/E.java", ok).is_empty());
    }

    #[test]
    fn association_on_a_wrong_type_warns_and_other_types_are_quiet() {
        let host = host();
        let bad = "@Entity\nclass E {\n  @Id Long id;\n  @OneToOne Map<String, Customer> m;\n  @ManyToOne(fetch = FetchType.LAZY) long n;\n  @OneToMany(mappedBy = \"a\") String s;\n  @ManyToOne(fetch = FetchType.LAZY) List<A> l;\n}\n";
        assert_eq!(
            problems(&host, "src/shop/E.java", bad),
            ["jpa-eager-to-one", "jpa-relationship-type", "jpa-relationship-type", "jpa-relationship-type", "jpa-relationship-type"]
        );
        let fine = "@Entity\nclass E {\n  @Id Long id;\n  @OneToMany(mappedBy = \"a\") Map<String, Customer> m;\n  @ManyToOne(fetch = FetchType.LAZY) Vendor.Customer c;\n  @ElementCollection Set<String> tags;\n}\n";
        assert!(problems(&host, "src/shop/E.java", fine).is_empty());
        assert_eq!(problems(&host, "src/shop/K.kt", "@Entity\nclass K(@Id var id: Long?, @ManyToOne(fetch = FetchType.LAZY) var o: MutableList<A>)\n"), ["jpa-relationship-type"]);
    }

    #[test]
    fn embedded_id_with_id_or_id_class_warns() {
        let host = host();
        let both = "@Entity\nclass E {\n  @Id Long a;\n  @EmbeddedId Key k;\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", both), ["jpa-multiple-id-kinds"]);
        let with_class = "@Entity\n@IdClass(Key.class)\nclass E {\n  @EmbeddedId Key k;\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", with_class), ["jpa-multiple-id-kinds"]);
        assert!(problems(&host, "src/shop/E.java", "@Entity\nclass E {\n  @EmbeddedId Key k;\n}\n").is_empty());
    }

    #[test]
    fn getter_annotations_with_field_id_are_flagged_unless_access_is_declared() {
        let host = host();
        let mixed = "@Entity\nclass E {\n  @Id Long id;\n  String n;\n  @Column(name = \"x\")\n  public String getN() { return n; }\n  @Transient public int getZ() { return 1; }\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", mixed), ["jpa-mixed-access"]);
        assert_eq!(first_range(&host, "src/shop/E.java"), (4, 2));
        let access = mixed.replace("@Entity", "@Entity\n@Access(AccessType.FIELD)");
        assert!(problems(&host, "src/shop/E.java", &access).is_empty());
        let on_getter = "@Entity\nclass E {\n  @Column Long f;\n  @Id public Long getId() { return 1L; }\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", on_getter), ["jpa-mixed-access"]);
        let consistent = "@Entity\nclass E {\n  @Id public Long getId() { return 1L; }\n  @Column public String getN() { return n; }\n}\n";
        assert!(problems(&host, "src/shop/E.java", consistent).is_empty());
    }

    #[test]
    fn attribute_signature_group_can_be_switched_off() {
        let host = host();
        host.set_setting("jpaAssist.checkAttributeSignatures", "false");
        let text = "@Entity\nclass E {\n  @Id Long a;\n  @EmbeddedId Key k;\n}\n";
        assert!(problems(&host, "src/shop/E.java", text).is_empty());
    }

    #[test]
    fn listener_without_a_public_default_constructor_is_info() {
        let host = with_listener("class AuditListener {\n  AuditListener(Clock c) {}\n  @PrePersist void t(Object o) {}\n}\n");
        assert_eq!(problems(&host, "src/shop/E.java", LISTENED), ["jpa-listener-constructor"]);
        assert_eq!(host.diagnostics()["/project/src/shop/E.java"][0].severity, openidea_plugin::Severity::Info);

        let private = "class AuditListener {\n  private AuditListener() {}\n  @PrePersist void t(Object o) {}\n}\n";
        host.add_file("src/shop/AuditListener.java", private);
        assert_eq!(problems(&host, "src/shop/E.java", LISTENED), ["jpa-listener-constructor"]);

        for ok in [
            "class AuditListener {\n  public AuditListener() {}\n  AuditListener(int x) {}\n  @PrePersist void t(Object o) {}\n}\n",
            "class AuditListener {\n  @PrePersist void t(Object o) { new Other(1); }\n}\n",
        ] {
            host.add_file("src/shop/AuditListener.java", ok);
            assert!(problems(&host, "src/shop/E.java", LISTENED).is_empty());
        }
    }

    #[test]
    fn kotlin_listener_constructor_needs_defaults_or_a_secondary_one() {
        let host = host();
        let entity = "@Entity\n@EntityListeners(AuditListener::class)\nclass K(@Id var id: Long?)\n";
        let listener = |header: &str| format!("class AuditListener{header} {{\n  @PrePersist fun t(o: Any) {{}}\n}}\n");

        host.add_file("src/shop/AuditListener.kt", &listener("(val clock: Clock)"));
        assert_eq!(problems(&host, "src/shop/K.kt", entity), ["jpa-listener-constructor"]);
        for header in ["", "()", "(val clock: Clock = Clock())", " private constructor()"] {
            host.add_file("src/shop/AuditListener.kt", &listener(header));
            assert!(problems(&host, "src/shop/K.kt", entity).is_empty(), "{header}");
        }
    }

    const KOTLIN_PLAIN: &str = "@Entity\nclass K(\n  @Id var id: Long?,\n  var name: String,\n)\n";

    fn kotlin_host(build: &str) -> MockHost {
        let host = host();
        host.add_file("build.gradle.kts", build);
        host
    }

    const KOTLIN_BUILD: &str = "plugins {\n    kotlin(\"jvm\") version \"2.0.21\"\n}\n";

    #[test]
    fn kotlin_entity_without_the_jpa_plugin_gets_an_info_and_a_build_file_fix() {
        let host = kotlin_host(KOTLIN_BUILD);
        assert_eq!(problems(&host, "src/shop/K.kt", KOTLIN_PLAIN), ["jpa-kotlin-plugin-missing"]);
        assert_eq!(host.diagnostics()["/project/src/shop/K.kt"][0].severity, openidea_plugin::Severity::Info);

        let mut plugin = JpaAssist::default();
        plugin.handle_events(&[Event::FileOpened("/project/src/shop/K.kt".into())], &host.invocation());
        let doc = Document { path: "/project/src/shop/K.kt".into(), language: "kotlin".into(), version: 1, text: KOTLIN_PLAIN.into() };
        let at = Position::new(0, 1);
        let fix = plugin.code_actions(&doc, Range::new(at, at)).into_iter().find(|a| a.title.starts_with("Add the kotlin-jpa")).unwrap();
        assert_eq!(fix.title, "Add the kotlin-jpa plugin to build.gradle.kts");
        assert_eq!(fix.edits[0].0, "/project/build.gradle.kts");
        assert_eq!(fix.edits[0].1[0].text, "\n    kotlin(\"plugin.jpa\") version \"2.0.21\"");
    }

    #[test]
    fn plugin_missing_is_quiet_with_defaults_a_no_arg_constructor_or_an_unknown_build() {
        let host = kotlin_host(KOTLIN_BUILD);
        assert!(problems(&host, "src/shop/K.kt", "@Entity\nclass K(@Id var id: Long? = null, var name: String = \"\")\n").is_empty());
        assert!(problems(&host, "src/shop/K.kt", "@Entity\nclass K(@Id var id: Long?) {\n  constructor() : this(null)\n}\n").is_empty());
        host.add_file("build.gradle.kts", "plugins {\n    alias(libs.plugins.kotlin.jvm)\n}\n");
        assert!(problems(&host, "src/shop/K.kt", KOTLIN_PLAIN).is_empty());
        let java_only = MockHost::install().with_capabilities(&["events", "project.read", "editor.read", "storage"]);
        assert!(problems(&java_only, "src/shop/K.kt", KOTLIN_PLAIN).is_empty(), "no build file");
    }

    #[test]
    fn module_build_file_wins_over_the_root_one_and_a_root_plugin_counts() {
        let host = kotlin_host("plugins {\n    kotlin(\"plugin.jpa\") version \"2.0.21\"\n}\n");
        host.add_file("src/build.gradle.kts", KOTLIN_BUILD);
        assert!(problems(&host, "src/shop/K.kt", KOTLIN_PLAIN).is_empty());
    }

    const WITH_JPA: &str = "plugins {\n    kotlin(\"jvm\") version \"2.0.21\"\n    kotlin(\"plugin.jpa\") version \"2.0.21\"\n}\n";

    #[test]
    fn redundant_no_arg_constructor_is_flagged_only_when_the_plugin_is_named() {
        let host = kotlin_host(WITH_JPA);
        let text = "@Entity\nclass K(@Id var id: Long?) {\n    constructor() {}\n\n    var name: String = \"\"\n}\n";
        assert_eq!(problems(&host, "src/shop/K.kt", text), ["jpa-redundant-constructor"]);

        let delegating = "@Entity\nclass K(@Id var id: Long?) {\n    constructor() : this(null)\n}\n";
        assert!(problems(&host, "src/shop/K.kt", delegating).is_empty());
        let plain = kotlin_host(KOTLIN_BUILD);
        assert!(problems(&plain, "src/shop/K.kt", text).is_empty());
    }

    #[test]
    fn redundant_constructor_fix_removes_its_line() {
        let host = kotlin_host(WITH_JPA);
        let text = "@Entity\nclass K(@Id var id: Long?) {\n    constructor() {}\n    var name: String = \"\"\n}\n";
        problems(&host, "src/shop/K.kt", text);
        let mut plugin = JpaAssist::default();
        plugin.handle_events(&[Event::FileOpened("/project/src/shop/K.kt".into())], &host.invocation());
        let doc = Document { path: "/project/src/shop/K.kt".into(), language: "kotlin".into(), version: 1, text: text.into() };
        let at = Position::new(2, 6);
        let fix = plugin.code_actions(&doc, Range::new(at, at)).into_iter().find(|a| a.title == "Remove constructor").unwrap();
        let edit = &fix.edits[0].1[0];
        assert_eq!((edit.range.start, edit.range.end, edit.text.as_str()), (Position::new(2, 0), Position::new(3, 0), ""));
    }

    #[test]
    fn open_entity_is_redundant_only_with_an_allopen_entity_configuration() {
        let text = "@Entity\nopen class K(@Id var id: Long? = null)\n";
        let host = kotlin_host("plugins { kotlin(\"plugin.jpa\") }\n");
        assert!(problems(&host, "src/shop/K.kt", text).is_empty());
        host.add_file("build.gradle.kts", "allOpen {\n  annotation(\"jakarta.persistence.Entity\")\n}\n");
        assert_eq!(problems(&host, "src/shop/K.kt", text), ["jpa-redundant-open"]);
    }

    #[test]
    fn saving_a_build_file_refreshes_the_detection() {
        let host = kotlin_host(KOTLIN_BUILD);
        host.add_file("src/shop/K.kt", KOTLIN_PLAIN);
        let mut plugin = JpaAssist::default();
        let kotlin = "/project/src/shop/K.kt";
        plugin.handle_events(&[Event::FileOpened(kotlin.into())], &host.invocation());
        assert_eq!(host.diagnostics()[kotlin].len(), 1);

        host.add_file("build.gradle.kts", WITH_JPA);
        plugin.handle_events(&[Event::FileSaved("/project/build.gradle.kts".into()), Event::FileSaved(kotlin.into())], &host.invocation());
        assert!(host.diagnostics().get(kotlin).is_none_or(|d| d.is_empty()));
    }

    const ADDRESS: &str = "package shop;\n@Embeddable\nclass Address {\n  String street;\n  String city;\n}\n";

    fn name_host() -> MockHost {
        let host = host();
        host.add_file("src/shop/Address.java", ADDRESS);
        host.add_file("src/shop/Base.java", "package shop;\n@MappedSuperclass\nclass Base {\n  @Id Long id;\n  String created;\n}\n");
        host
    }

    fn message_of(host: &MockHost, path: &str) -> Vec<String> {
        host.diagnostics().get(&format!("/project/{path}")).map_or(Vec::new(), |d| d.iter().map(|d| d.message.clone()).collect())
    }

    #[test]
    fn attribute_override_names_are_checked_against_the_embeddable_and_the_mapped_superclass() {
        let host = name_host();
        let text = "package shop;\n@Entity\n@AttributeOverride(name = \"created\", column = @Column(name = \"c\"))\nclass E extends Base {\n  @Embedded\n  @AttributeOverrides({@AttributeOverride(name = \"street\", column = @Column(name = \"s\")), @AttributeOverride(name = \"zip\", column = @Column(name = \"z\"))})\n  Address a;\n}\n";
        assert_eq!(problems(&host, "src/shop/E.java", text), ["jpa-unknown-attribute-override"]);
        assert_eq!(message_of(&host, "src/shop/E.java"), ["Address has no attribute 'zip' (named in @AttributeOverride)."]);

        let bad_class_level = text.replace("name = \"created\"", "name = \"made\"");
        assert_eq!(problems(&host, "src/shop/E.java", &bad_class_level).len(), 2);
    }

    #[test]
    fn name_checks_are_quiet_for_unknown_duplicate_or_library_targets() {
        let host = name_host();
        let unknown = "@Entity\nclass E {\n  @Id Long id;\n  @Embedded @AttributeOverride(name = \"zip\", column = @Column(name = \"z\")) Vendor v;\n}\n";
        assert!(problems(&host, "src/shop/E.java", unknown).is_empty());

        let library_parent = "@Entity\n@AttributeOverride(name = \"zip\", column = @Column(name = \"z\"))\nclass E extends AbstractPersistable {}\n";
        assert!(problems(&host, "src/shop/E.java", library_parent).is_empty());

        host.add_file("src/other/Address.java", "package other;\n@Embeddable\nclass Address {\n  String zip;\n}\n");
        let text = "@Entity\nclass E {\n  @Id Long id;\n  @Embedded @AttributeOverride(name = \"zip\", column = @Column(name = \"z\")) Address a;\n}\n";
        assert!(problems(&host, "src/shop/E.java", text).is_empty(), "two Address classes");

        assert!(problems(&host, "src/shop/E.java", &text.replace("\"zip\"", "SOME_CONSTANT")).is_empty(), "not a literal");
    }

    #[test]
    fn maps_id_and_map_key_are_read_against_the_embedded_id_and_the_element_type() {
        let host = name_host();
        host.add_file("src/shop/Key.java", "package shop;\n@Embeddable\nclass Key {\n  Long orderId;\n}\n");
        let maps_id = "package shop;\n@Entity\nclass E {\n  @EmbeddedId Key key;\n  @MapsId(\"orderId\") @ManyToOne(fetch = FetchType.LAZY) Order order;\n  @MapsId(\"missing\") @ManyToOne(fetch = FetchType.LAZY) Order other;\n}\n";
        assert_eq!(message_of_after(&host, "src/shop/E.java", maps_id), ["Key has no attribute 'missing' (named in @MapsId)."]);

        let map_key = "package shop;\n@Entity\nclass F {\n  @Id Long id;\n  @OneToMany(mappedBy = \"f\") @MapKey(name = \"number\") Map<String, Order> ok;\n  @OneToMany(mappedBy = \"g\") @MapKey(name = \"nope\") Map<String, Order> bad;\n}\n";
        host.add_file("src/shop/Order.java", "package shop;\n@Entity\nclass Order {\n  @Id Long id;\n  String number;\n}\n");
        assert_eq!(message_of_after(&host, "src/shop/F.java", map_key), ["Order has no attribute 'nope' (named in @MapKey)."]);
    }

    fn message_of_after(host: &MockHost, path: &str, text: &str) -> Vec<String> {
        problems(host, path, text);
        message_of(host, path)
    }

    #[test]
    fn entity_graph_nodes_name_attributes_but_subgraph_nodes_are_left_alone() {
        let host = name_host();
        let text = "package shop;\n@Entity\n@NamedEntityGraph(name = \"g\", attributeNodes = {@NamedAttributeNode(\"created\"), @NamedAttributeNode(value = \"lines\", subgraph = \"s\"), @NamedAttributeNode(\"ghost\")},\n  subgraphs = @NamedSubgraph(name = \"s\", attributeNodes = @NamedAttributeNode(\"anything\")))\nclass E extends Base {\n  @OneToMany(mappedBy = \"e\") List<Line> lines;\n}\n";
        assert_eq!(message_of_after(&host, "src/shop/E.java", text), ["E has no attribute 'ghost' (named in @NamedAttributeNode)."]);
    }

    #[test]
    fn mapping_name_group_can_be_switched_off() {
        let host = name_host();
        host.set_setting("jpaAssist.checkMappingNames", "false");
        let text = "@Entity\nclass E {\n  @Id Long id;\n  @Embedded @AttributeOverride(name = \"zip\", column = @Column(name = \"z\")) Address a;\n}\n";
        assert!(problems(&host, "src/shop/E.java", text).is_empty());
    }

    const QUERIES: &str = "package shop.data;\ninterface R extends JpaRepository<Order, Long> {\n  @Query(\"select o from Order o where o.number = :number and o.x = :other and d = '10:30' and e = a::text\")\n  List<Order> a(@Param(\"number\") String n, String another);\n  @Query(\"select o from Order o where o.number = :n\")\n  List<Order> b(@Param(\"number\") String n);\n  @Query(\"select o from Order o where o.number = :number\")\n  List<Order> c(String number);\n  @Query(\"x = :#{#p.id} and y = :z\" + \" more\")\n  List<Order> d(String p);\n}\n";

    #[test]
    fn query_parameters_must_be_parameters_of_the_method() {
        let host = host();
        let messages = message_of_after(&host, "src/shop/data/R.java", QUERIES);
        assert_eq!(
            messages,
            [
                "The method has no parameter named 'other'. Name the parameter or annotate it with @Param(\"other\").",
                "The method has no parameter named 'n'. Name the parameter or annotate it with @Param(\"n\")."
            ]
        );
        assert_eq!(first_range(&host, "src/shop/data/R.java").0, 2);
    }

    #[test]
    fn kotlin_query_parameters_and_templates() {
        let host = host();
        let text = "interface R : JpaRepository<Order, Long> {\n  @Query(\"select o from Order o where o.number = :number\")\n  fun a(@Param(\"num\") number: String): List<Order>\n  @Query(\"x = :n\")\n  fun b(n: String): List<Order>\n  @Query(\"x = :n and $t\")\n  fun c(): List<Order>\n}\n";
        assert_eq!(message_of_after(&host, "src/shop/data/R.kt", text), ["The method has no parameter named 'number'. Name the parameter or annotate it with @Param(\"number\")."]);
    }

    #[test]
    fn named_queries_are_looked_up_across_the_project() {
        let host = host();
        host.add_file("src/shop/Order.java", "package shop;\n@Entity\n@NamedQuery(name = \"Order.open\", query = \"select o from Order o\")\nclass Order {\n  @Id Long id;\n}\n");
        let text = "package shop.data;\ninterface R extends JpaRepository<Order, Long> {\n  @Query(name = \"Order.open\")\n  List<Order> a();\n  @Query(name = \"Order.gone\")\n  List<Order> b();\n}\n";
        assert_eq!(message_of_after(&host, "src/shop/data/R.java", text), ["No @NamedQuery or @NamedNativeQuery named 'Order.gone' in the project."]);

        let call = "class S {\n  void f(EntityManager em) { em.createNamedQuery(\"Order.open\"); em.createNamedQuery(\"nope\", Order.class); }\n}\n";
        assert_eq!(message_of_after(&host, "src/shop/S.java", call), ["No @NamedQuery or @NamedNativeQuery named 'nope' in the project."]);
    }

    #[test]
    fn named_queries_are_not_judged_when_xml_mappings_may_declare_them() {
        let host = host();
        host.add_file("src/main/resources/META-INF/orm.xml", "<entity-mappings/>");
        let text = "class S {\n  void f(EntityManager em) { em.createNamedQuery(\"nope\"); }\n}\n";
        assert!(problems(&host, "src/shop/S.java", text).is_empty());
    }

    #[test]
    fn query_reference_group_can_be_switched_off() {
        let host = host();
        host.set_setting("jpaAssist.checkQueryReferences", "false");
        assert!(problems(&host, "src/shop/data/R.java", QUERIES).is_empty());
    }

    const PERSISTENCE: &str = "<persistence version=\"3.0\">\n  <persistence-unit name=\"main\" transaction-type=\"XA\">\n    <class>shop.Order</class>\n    <class>shop.Gone</class>\n    <class>shop.Plain</class>\n  </persistence-unit>\n</persistence>\n";

    #[test]
    fn persistence_xml_is_checked_when_it_opens() {
        let host = host();
        host.add_file("src/shop/Plain.java", "package shop;\nclass Plain {}\n");
        let mut codes = problems(&host, "src/main/resources/META-INF/persistence.xml", PERSISTENCE);
        codes.sort();
        assert_eq!(codes, ["jpa-xml-class-not-persistent", "jpa-xml-class-unknown", "jpa-xml-value"]);
    }

    #[test]
    fn mapping_files_make_unannotated_classes_acceptable() {
        let host = host();
        host.add_file("src/shop/Plain.java", "package shop;\nclass Plain {}\n");
        host.add_file("src/main/resources/META-INF/orm.xml", "<entity-mappings><entity class=\"shop.Plain\"/></entity-mappings>");
        let codes = problems(&host, "src/main/resources/META-INF/persistence.xml", PERSISTENCE);
        assert!(!codes.contains(&"jpa-xml-class-not-persistent".to_owned()), "{codes:?}");
        assert!(problems(&host, "src/main/resources/META-INF/orm.xml", "<entity-mappings><entity class=\"shop.Plain\"/></entity-mappings>").is_empty());
    }

    #[test]
    fn other_xml_files_and_the_switch_are_ignored() {
        let host = host();
        assert!(problems(&host, "src/pom.xml", "<project><class>nope.Missing</class></project>").is_empty());
        host.set_setting("jpaAssist.checkPersistenceXml", "false");
        assert!(problems(&host, "src/main/resources/META-INF/persistence.xml", PERSISTENCE).is_empty());
    }

    #[test]
    fn class_element_completes_class_names_from_the_sources() {
        let host = host();
        let doc = |text: &str| Document { path: "/project/persistence.xml".into(), language: "xml".into(), version: 1, text: text.into() };
        host.add_file("src/main/java/shop/Order.java", ORDER);
        host.add_file("src/main/java/shop/Customer.java", CUSTOMER);
        let text = "<persistence-unit>\n  <class>shop.O</class>";
        let at = Position::new(1, "  <class>shop.O".len() as u32);
        let items = JpaAssist::default().completion(&doc(text), at, None);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["shop.Order"]);

        let outside = JpaAssist::default().completion(&doc("<persistence-unit>\n  <provider>shop.O"), Position::new(1, 17), None);
        assert!(outside.is_empty());
    }

    fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
        let mut out = text.to_owned();
        let mut sorted: Vec<&TextEdit> = edits.iter().collect();
        sorted.sort_by_key(|e| std::cmp::Reverse((e.range.start.line, e.range.start.character)));
        for e in sorted {
            let start = offset(&out, e.range.start).unwrap();
            let end = offset(&out, e.range.end).unwrap();
            out.replace_range(start..end, &e.text);
        }
        out
    }

    fn inverse_at(host: &MockHost, line: u32, selected: &str) -> Vec<CodeAction> {
        let _ = host;
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: selected.into() };
        let at = Position::new(line, 4);
        JpaAssist::default().code_actions(&doc, Range::new(at, at)).into_iter().filter(|a| a.title.starts_with("Add inverse")).collect()
    }

    #[test]
    fn many_to_one_offers_a_one_to_many_on_the_target_file() {
        let host = host();
        for line in [11, 12] {
            let actions = inverse_at(&host, line, ORDER);
            assert_eq!(actions.len(), 1, "line {line}");
            assert_eq!(actions[0].title, "Add inverse attribute 'orders' to Customer");
            let (path, edits) = &actions[0].edits[0];
            assert_eq!(path, "/project/src/shop/Customer.java");
            let result = apply_edits(CUSTOMER, edits);
            assert!(result.contains("import java.util.List;"), "{result}");
            assert!(result.contains("    @OneToMany(mappedBy = \"customer\")\n    private List<Order> orders = new ArrayList<>();\n}"), "{result}");
            assert!(result.contains("import jakarta.persistence.OneToMany;"));
        }
    }

    #[test]
    fn inverse_is_not_offered_off_the_declaration_or_when_one_exists() {
        let host = host();
        assert!(inverse_at(&host, 7, ORDER).is_empty());
        assert!(inverse_at(&host, 13, ORDER).is_empty());

        host.add_file("src/shop/Customer.java", "package shop;\n@Entity\npublic class Customer {\n    @Id Long id;\n    @OneToMany(mappedBy = \"customer\") List<Order> orders;\n}\n");
        assert!(inverse_at(&host, 12, ORDER).is_empty());

        host.add_file("src/shop/Customer.java", "package shop;\n@Entity\npublic class Customer {\n    @Id Long id;\n    String orders;\n}\n");
        assert!(inverse_at(&host, 12, ORDER).is_empty(), "name taken");

        host.add_file("src/shop/Customer.java", "package shop;\npublic class Customer {}\n");
        assert!(inverse_at(&host, 12, ORDER).is_empty(), "not an entity");

        let missing = ORDER.replace("Customer customer", "Stranger customer");
        assert!(inverse_at(&host, 12, &missing).is_empty(), "target file missing");
    }

    #[test]
    fn one_to_one_and_cross_package_targets_get_a_mapped_by_and_an_import() {
        let host = host();
        host.add_file("src/billing/Invoice.java", "package billing;\n\nimport jakarta.persistence.*;\n\n@Entity\npublic class Invoice {\n    @Id Long id;\n}\n");
        let text = "package shop;\n\nimport billing.Invoice;\n\n@Entity\npublic class Order {\n    @Id Long id;\n    @OneToOne\n    private Invoice invoice;\n}\n";
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: text.into() };
        let at = Position::new(8, 4);
        let found = JpaAssist::default().code_actions(&doc, Range::new(at, at));
        let action = found.iter().find(|a| a.title.starts_with("Add inverse")).unwrap();
        assert_eq!(action.title, "Add inverse attribute 'order' to Invoice");
        let invoice = host.invocation().project().unwrap().read_file("/project/src/billing/Invoice.java").unwrap();
        let result = apply_edits(&invoice, &action.edits[0].1);
        assert!(result.contains("import shop.Order;"), "{result}");
        assert!(result.contains("@OneToOne(mappedBy = \"invoice\")\n    private Order order;"), "{result}");
    }

    #[test]
    fn kotlin_targets_get_a_mutable_list() {
        let host = MockHost::install().with_capabilities(&["events", "project.read", "editor.read", "storage"]);
        host.add_file("src/shop/Order.java", ORDER);
        host.add_file("src/shop/Customer.kt", "package shop\n\nimport jakarta.persistence.*\n\n@Entity\nclass Customer {\n    @Id\n    var id: Long? = null\n}\n");
        let doc = Document { path: "/project/src/shop/Order.java".into(), language: "java".into(), version: 1, text: ORDER.into() };
        let at = Position::new(12, 4);
        let found = JpaAssist::default().code_actions(&doc, Range::new(at, at));
        let action = found.iter().find(|a| a.title.starts_with("Add inverse")).unwrap();
        assert_eq!(action.edits[0].0, "/project/src/shop/Customer.kt");
        let customer = host.invocation().project().unwrap().read_file("/project/src/shop/Customer.kt").unwrap();
        let result = apply_edits(&customer, &action.edits[0].1);
        assert!(result.contains("@OneToMany(mappedBy = \"customer\")\n    var orders: MutableList<Order> = mutableListOf()"), "{result}");
    }
}
