//! Bounded source snapshots; no discovery or file reads in editor providers.
use super::*;
use openidea_plugin::diagnostics::DiagnosticsProvider;
use openidea_plugin::hover::{Hover, HoverProvider};
use openidea_plugin::inlay_hints::{InlayHint, InlayHints};
use openidea_plugin::{Event, EventKind};
use spring_text::index::DIAGNOSTICS_WARM;
use spring_text::scope::{current_owner, scope_path};
use spring_text::text::balanced;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// ponytail: once per instance, re-shown after idle reclaim
static EVENTS_NOTICE: AtomicBool = AtomicBool::new(false);
const MAX_TEXT: usize = 128 << 10;
#[derive(Clone)]
pub(super) struct Source {
    identities: Vec<String>,
    file: JavaFile,
    classes: Vec<Class>,
    complete: bool,
}
fn source(path: &str, text: &str) -> Option<Source> {
    if text.len() > MAX_TEXT || !balanced(text, is_kotlin(path)) {
        return Some(Source { identities: Vec::new(), file: JavaFile::default(), classes: Vec::new(), complete: false });
    }
    let file = parse_source(path, text);
    if file.types.len() > 256 || file.types.iter().map(|t| t.members.len()).sum::<usize>() > 2048 {
        return Some(Source { identities: Vec::new(), file: JavaFile::default(), classes: Vec::new(), complete: false });
    }
    let mut parsed = classes(path, text).unwrap_or_default();
    parsed.retain(|c| file.types.iter().any(|t| t.name == c.name && t.outer.is_none()));
    let qualified_names: std::collections::BTreeSet<String> = file
        .types
        .iter()
        .flat_map(|t| {
            t.fields()
                .filter_map(|f| f.ty.as_ref())
                .chain(t.components.iter().map(|p| &p.ty))
                .chain(t.methods().filter_map(|m| m.ty.as_ref()))
        })
        .flat_map(|ty| std::iter::once(ty).chain(ty.args.iter()))
        .filter(|ty| ty.text.split('<').next().is_some_and(|written| written.contains('.')))
        .map(|ty| ty.name.clone())
        .collect();
    let aliasing = is_kotlin(path)
        && (java::tokens(text).iter().any(|t| t.s == "typealias")
            || text.lines().any(|l| l.trim_start().starts_with("import ") && l.contains(" as ")));
    for c in &mut parsed {
        let qualify = |name: &str| {
            let imports: Vec<_> = file.imports.iter().filter(|i| !i.is_static && i.path.rsplit('.').next() == Some(name)).collect();
            if aliasing {
                "\0unknown".into()
            } else if imports.len() == 1 {
                format!("@type:{}", imports[0].path)
            } else if imports.len() > 1 || aliasing {
                "\0unknown".into()
            } else if file.package.is_empty() {
                format!("@type:{name}")
            } else {
                format!("@type:{}.{name}", file.package)
            }
        };
        let declaration = file.types.iter().find(|t| t.name == c.name);
        let generic = declaration.is_some_and(|t| java::tokens(&text[t.at + t.name.len()..t.end]).first().is_some_and(|t| t.s == "<"));
        // Unwrapping is only justified for a source-owned standard container. A
        // same-spelled custom List/Optional must not manufacture element fields.
        let uncertain_elements: std::collections::BTreeSet<_> = declaration
            .into_iter()
            .flat_map(|t| {
                t.fields()
                    .filter_map(|f| f.ty.as_ref())
                    .chain(t.components.iter().map(|p| &p.ty))
                    .chain(t.methods().filter_map(|m| m.ty.as_ref()))
            })
            .filter(|ty| !ty.args.is_empty())
            .filter(|ty| {
                let approved = match ty.name.as_str() {
                    "Iterable" => "java.lang.Iterable".to_owned(),
                    name if CONTAINERS.contains(&name) => format!("java.util.{name}"),
                    _ => return true,
                };
                !file.imports.iter().any(|i| !i.is_static && i.path == approved) || file.types.iter().any(|t| t.name == ty.name)
            })
            .map(element)
            .collect();
        for prop in &mut c.props {
            prop.target = Some(if generic || qualified_names.contains(&prop.ty) || uncertain_elements.contains(&prop.ty) {
                "\0unknown".into()
            } else {
                qualify(&prop.ty)
            });
            if qualified_names.contains(&prop.ty) {
                prop.ty = "\0unknown".into();
            } else if let Some(import) = file.imports.iter().find(|i| !i.is_static && i.path.rsplit('.').next() == Some(prop.ty.as_str())) {
                prop.ty = import.path.clone();
            }
        }
        if let Some(parent) = &mut c.parent {
            *parent =
                if generic || declaration.is_some_and(|t| t.extends.first().is_some_and(|e| e.text.contains('.') || !e.args.is_empty())) {
                    "\0unknown".into()
                } else {
                    qualify(parent)
                };
        }
    }
    let identities = parsed
        .iter()
        .map(|c| if file.package.is_empty() { format!("@type:{}", c.name) } else { format!("@type:{}.{}", file.package, c.name) })
        .collect();
    Some(Source { identities, file, classes: parsed, complete: true })
}
type FileKey = (String, u64, u64);
type FactsKey = (u64, u64, String, Vec<String>);

pub(crate) struct Live {
    files: Indexes<Source>,
    pub(super) watching: bool,
    file: Option<(FileKey, Rc<JavaFile>)>,
    facts: Option<(FactsKey, Rc<Facts>)>,
}
impl Default for Live {
    fn default() -> Self {
        Self { files: Indexes::new(SOURCE_GLOBS, source), watching: false, file: None, facts: None }
    }
}
pub(super) fn completion_supported(doc: &Document) -> bool {
    doc.text.len() <= MAX_TEXT
        && matches!((doc.language.as_str(), doc.path.rsplit('.').next()), ("java", Some("java")) | ("kotlin", Some("kt")))
}
impl DataTools {
    pub(super) fn repository_file(&mut self, doc: &Document) -> Rc<JavaFile> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        doc.text.hash(&mut hasher);
        let key = (doc.path.clone(), doc.version, hasher.finish());
        if let Some((cached, file)) = &self.live.file
            && *cached == key
        {
            return file.clone();
        }
        let file = Rc::new(repository_file(doc));
        self.live.file = Some((key, file.clone()));
        file
    }
    pub(super) fn model_for<'a>(&'a mut self, doc: &Document, file: &JavaFile, warm: bool) -> Option<Model<'a>> {
        let live = &mut self.live;
        let index = if warm {
            live.files.for_cached_document_warm(&doc.path, &doc.text, DIAGNOSTICS_WARM)?
        } else {
            live.files.for_cached_document(&doc.path, &doc.text)?
        };
        Some(model(index, &mut live.facts, file, live.watching, &doc.path))
    }
    pub(crate) fn cached_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if !supported(doc) {
            return Vec::new();
        }
        let file = self.repository_file(doc);
        let repos = repositories(&file);
        let Some(model) = self.model_for(doc, &file, false) else {
            return Vec::new();
        };
        let mut markers = Vec::new();
        let mut derived_markers = Vec::new();
        for (ti, domain) in repos {
            let t = &file.types[ti];
            let line = range(&doc.text, t.at, t.at).start.line;
            if let Some((path, c)) = model.class(&domain) {
                let target = Some(Location { path: path.into(), range: c.range });
                markers.push(LineMarker { line, icon: Some(DOMAIN_MARKER.into()), tooltip: format!("Domain type: {domain}"), target });
            }
            let custom = format!("{}Impl", t.name);
            if let Some((path, c)) = model.class(&custom) {
                let target = Some(Location { path: path.into(), range: c.range });
                markers.push(LineMarker {
                    line,
                    icon: Some(IMPLEMENTATION_MARKER.into()),
                    tooltip: format!("Custom implementation: {custom}"),
                    target,
                });
            }
            for m in t.methods().filter(|m| derived_method(m)) {
                let Some(parts) = derived(&m.name).filter(|p| !p.is_empty()) else {
                    continue;
                };
                let line = range(&doc.text, m.at, m.at).start.line;
                let tooltip = format!(
                    "Derived query: {} {domain} where {}",
                    m.name[..parts[0].from.min(m.name.len())].trim_end_matches("By"),
                    describe(&m.name, &parts)
                );
                derived_markers.push(LineMarker { line, icon: Some(QUERY_MARKER.into()), tooltip, target: None });
            }
        }
        markers.extend(derived_markers);
        markers
    }

    pub(crate) fn activate_live(&mut self) {
        self.live.watching = host::subscribe_paths("files-changed", SOURCE_GLOBS).is_ok();
        if matches!(host::subscribe(EventKind::FileSaved), Err(Error::Denied(_))) && !EVENTS_NOTICE.swap(true, Ordering::Relaxed) {
            host::notify(
                NotificationLevel::Info,
                "Spring Data Tools",
                "Live checking needs the events permission. Allow it in Settings › Plugins › Spring Data Tools.",
            );
        }
        let deadline = Instant::now() + spring_text::index::COMMAND_BUDGET;
        for project in host::projects().unwrap_or_default() {
            let budget = deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(250));
            if budget.is_zero() {
                break;
            }
            self.live.files.get(&project, budget);
        }
    }
    pub(crate) fn live_events(&mut self, events: &[Event]) {
        self.live.files.on_events(events, Instant::now() + spring_text::index::COMMAND_BUDGET);
    }
}
fn supported(doc: &Document) -> bool {
    doc.text.len() <= MAX_TEXT
        && balanced(&doc.text, is_kotlin(&doc.path))
        && matches!((doc.language.as_str(), doc.path.rsplit('.').next()), ("java", Some("java")) | ("kotlin", Some("kt")))
}
const BASES: &[(&str, &str)] = &[
    ("Repository", "org.springframework.data.repository.Repository"),
    ("CrudRepository", "org.springframework.data.repository.CrudRepository"),
    ("ListCrudRepository", "org.springframework.data.repository.ListCrudRepository"),
    ("PagingAndSortingRepository", "org.springframework.data.repository.PagingAndSortingRepository"),
    ("JpaRepository", "org.springframework.data.jpa.repository.JpaRepository"),
    ("MongoRepository", "org.springframework.data.mongodb.repository.MongoRepository"),
    ("ReactiveCrudRepository", "org.springframework.data.repository.reactive.ReactiveCrudRepository"),
];
fn owned(file: &JavaFile, t: &TypeDecl) -> bool {
    t.extends.iter().any(|base| {
        BASES.iter().any(|(name, qualified)| {
            base.name == *name
                && base.text.split('<').next().is_some_and(|written| written.trim() == *name)
                && base.args.first().is_some_and(|domain| !domain.text.contains(['.', '<', '?', '*']))
                && !file.types.iter().any(|t| t.name == *name)
                && file.imports.iter().any(|i| !i.is_static && i.path == *qualified)
                && !file.imports.iter().any(|i| !i.is_static && i.path.rsplit('.').next() == Some(*name) && i.path != *qualified)
        })
    })
}
/// Only source-written class literals of an owned annotation establish a repository.
/// Semantic annotation aliases, constants and competing base substitutions remain unknown.
fn owned_definition(file: &JavaFile, ty: &TypeDecl, doc: &Document) -> bool {
    const NAME: &str = "RepositoryDefinition";
    const FQN: &str = "org.springframework.data.repository.RepositoryDefinition";
    let annotations: Vec<_> = ty.annotations.iter().filter(|a| a.name == NAME).collect();
    let [annotation] = annotations.as_slice() else { return false; };
    if !ty.extends.is_empty() || annotation.args.len() != 2 {
        return false;
    }
    let written = &doc.text[annotation.at + 1..annotation.name_at + annotation.name.len()];
    let namespace = if written == FQN {
        true
    } else if written == NAME {
        let imports: Vec<_> = file.imports.iter().filter(|i| !i.is_static && i.path.rsplit('.').next() == Some(NAME)).collect();
        imports.len() == 1 && imports[0].path == FQN && !file.types.iter().any(|t| t.name == NAME)
    } else { false };
    if !namespace { return false; }
    ["domainClass", "idClass"].iter().all(|name| {
        let args: Vec<_> = annotation.args.iter().filter(|a| a.name == *name).collect();
        let [arg] = args.as_slice() else { return false; };
        let value = arg.text.trim();
        let Some(class) = value.strip_suffix(if is_kotlin(&doc.path) { "::class" } else { ".class" }) else { return false; };
        let mut chars = class.chars();
        chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !matches!(class, "class" | "this" | "super" | "void" | "true" | "false" | "null")
    })
}
fn annotation_owned(file: &JavaFile, doc: &Document, annotation: &java::Annotation) -> bool {
    let name = annotation.name.as_str();
    let allowed: &[&str] = match name {
        "Query" => &[
            "org.springframework.data.jpa.repository.Query",
            "org.springframework.data.mongodb.repository.Query",
            "org.springframework.data.jdbc.repository.query.Query",
            "org.springframework.data.r2dbc.repository.Query",
        ],
        "Modifying" => &[
            "org.springframework.data.jpa.repository.Modifying",
            "org.springframework.data.jdbc.repository.query.Modifying",
            "org.springframework.data.r2dbc.repository.Modifying",
        ],
        "Param" => &["org.springframework.data.repository.query.Param"],
        "NativeQuery" => &["org.springframework.data.jpa.repository.NativeQuery"],
        "Sequence" => &["org.springframework.data.relational.core.mapping.Sequence"],
        "MappedCollection" => &["org.springframework.data.relational.core.mapping.MappedCollection"],
        _ => &[],
    };
    let written: String = java::tokens(&doc.text[annotation.at + 1..annotation.name_at + annotation.name.len()])
        .iter().map(|token| token.s).collect();
    if written.contains('.') {
        // A foreign qualified spelling cannot borrow an unrelated Spring import.
        return allowed.contains(&written.as_str()) && !file.types.iter().any(|ty| ty.name == "org");
    }
    if written != name || file.types.iter().any(|t| t.name == name) {
        return false;
    }
    let imports: Vec<_> = file.imports.iter()
        .filter(|i| !i.is_static && i.path.rsplit('.').next() == Some(name)).collect();
    imports.len() == 1 && allowed.contains(&imports[0].path.as_str())
}
pub(super) fn repository_file(doc: &Document) -> JavaFile {
    let mut file = parse_source(&doc.path, &doc.text);
    let aliased = is_kotlin(&doc.path)
        && (java::tokens(&doc.text).iter().any(|t| t.s == "typealias")
            || doc.text.lines().any(|l| l.trim_start().starts_with("import ") && l.contains(" as ")));
    let valid: Vec<_> = file
        .types
        .iter()
        .map(|t| {
            !aliased
                && !java::tokens(&doc.text[t.at + t.name.len()..t.end]).first().is_some_and(|token| token.s == "<")
                && if t.annotations.iter().any(|a|a.name == "RepositoryDefinition") { owned_definition(&file,t,doc) } else { owned(&file,t) }
        })
        .collect();
    let special_ok = |name: &str| {
        if !SPECIAL_PARAMS.contains(&name) {
            return true;
        }
        if file.types.iter().any(|t| t.name == name) {
            return false;
        }
        let qualified = if name == "Class" { "java.lang.Class".to_owned() } else { format!("org.springframework.data.domain.{name}") };
        let imported: Vec<_> = file.imports.iter().filter(|i| !i.is_static && i.path.rsplit('.').next() == Some(name)).collect();
        imported.iter().all(|i| i.path == qualified)
            && (name == "Class"
                || !imported.is_empty()
                || file.imports.iter().any(|i| !i.is_static && i.path == "org.springframework.data.domain.*"))
    };
    let valid_methods: Vec<Vec<bool>> =
        file.types
            .iter()
            .map(|t| {
                t.members
                    .iter()
                    .map(|m| {
                        m.params.iter().all(|p| special_ok(&p.ty.name))
                            && m.annotations.iter().filter(|a| a.name == "Query").all(|a| {
                                a.arg("value").is_some_and(|arg| {
                                    let tokens = java::tokens(&arg.text);
                                    tokens.len() == 1 && arg.strings.len() == 1
                                })
                            })
                            && m.annotations.iter().chain(m.params.iter().flat_map(|p| p.annotations.iter())).all(|a| {
                                !matches!(a.name.as_str(), "Query" | "NativeQuery" | "Modifying" | "Param") || annotation_owned(&file, doc, a)
                            })
                    })
                    .collect()
            })
            .collect();
    // JDBC source checks must inspect the annotation's own identity too; an
    // unrelated import does not establish a field/record-component annotation.
    let unknown_jdbc: std::collections::BTreeSet<usize> = file.types.iter()
        .flat_map(|ty| ty.members.iter().flat_map(|member| member.annotations.iter())
            .chain(ty.components.iter().flat_map(|component| component.annotations.iter())))
        .filter(|a| matches!(a.name.as_str(), "Sequence" | "MappedCollection") && !annotation_owned(&file, doc, a))
        .map(|a| a.at).collect();
    for ((t, valid), methods) in file.types.iter_mut().zip(valid).zip(valid_methods) {
        for member in &mut t.members { member.annotations.retain(|a| !unknown_jdbc.contains(&a.at)); }
        for component in &mut t.components { component.annotations.retain(|a| !unknown_jdbc.contains(&a.at)); }
        let mut n = 0;
        t.members.retain(|_| {
            let keep = methods[n];
            n += 1;
            keep
        });
        if t.kind == "interface" && !valid {
            t.extends.clear();
            t.annotations.retain(|a| a.name != "RepositoryDefinition");
        }
    }
    file
}
fn current_roots() -> Option<Vec<String>> {
    let projects = host::projects().ok()?;
    if projects.len() > 64 {
        return None;
    }
    projects.into_iter().map(|project| scope_path(&project.root().ok()?)).collect()
}
const MAX_CLASSES: usize = 5000;
/// What a whole project's classes say about identities that cannot be told apart.
#[derive(Default)]
pub(super) struct Facts {
    /// Duplicated identities, their simple names and duplicated entity names.
    pub(super) ambiguous: BTreeSet<String>,
    ownership_complete: bool,
    sources_complete: bool,
    capped: bool,
}
fn facts(index: &Index<Source>, roots: &[String], root: &str) -> Facts {
    let mut identities: BTreeMap<&str, usize> = BTreeMap::new();
    let mut entities: BTreeMap<&str, usize> = BTreeMap::new();
    let mut found = Facts { ownership_complete: true, sources_complete: true, ..Facts::default() };
    for (path, source) in index.iter() {
        found.sources_complete &= source.complete;
        if current_owner(path, roots).as_deref() != Some(root) {
            found.ownership_complete = false;
            continue;
        }
        for (identity, class) in source.identities.iter().zip(&source.classes) {
            *identities.entry(identity).or_default() += 1;
            if let Some(entity) = &class.entity {
                *entities.entry(entity).or_default() += 1;
            }
        }
    }
    found.capped = identities.len() > MAX_CLASSES;
    for (identity, _) in identities.into_iter().filter(|(_, count)| *count > 1) {
        let name = identity.strip_prefix("@type:").unwrap_or(identity);
        found.ambiguous.insert(identity.to_owned());
        found.ambiguous.insert(name.rsplit('.').next().unwrap_or(name).to_owned());
    }
    found.ambiguous.extend(entities.into_iter().filter(|(_, count)| *count > 1).map(|(name, _)| name.to_owned()));
    found
}
fn model<'a>(
    index: &'a Index<Source>,
    cache: &mut Option<(FactsKey, Rc<Facts>)>,
    file: &JavaFile,
    watching: bool,
    current_path: &str,
) -> Model<'a> {
    let Some(roots) = current_roots() else {
        return Model::unknown();
    };
    let Some(root) = current_owner(current_path, &roots) else {
        return Model::unknown();
    };
    let key = (index.id(), index.generation(), root.clone(), roots.clone());
    let facts = match cache {
        Some((cached, facts)) if *cached == key => facts.clone(),
        _ => {
            let facts = Rc::new(facts(index, &roots, &root));
            *cache = Some((key, facts.clone()));
            facts
        }
    };
    if facts.capped {
        return Model::unknown();
    }
    let mut by_name = BTreeMap::new();
    let mut entity_candidates: BTreeMap<&str, &Class> = BTreeMap::new();
    for (path, source) in index.iter() {
        if current_owner(path, &roots).as_deref() != Some(root.as_str()) {
            continue;
        }
        for (identity, class) in source.identities.iter().zip(&source.classes) {
            if facts.ambiguous.contains(identity) {
                continue;
            }
            by_name.insert(identity.as_str(), vec![(path, class)]);
            if let Some(entity) = &class.entity
                && !facts.ambiguous.contains(entity)
            {
                entity_candidates.insert(entity, class);
            }
            let imported: Vec<_> =
                file.imports.iter().filter(|i| !i.is_static && i.path.rsplit('.').next() == Some(class.name.as_str())).collect();
            let visible = if imported.is_empty() {
                source.file.package == file.package
            } else {
                imported.len() == 1 && imported[0].path == identity.strip_prefix("@type:").unwrap_or(identity)
            };
            if visible {
                by_name.insert(class.name.as_str(), vec![(path, class)]);
            }
        }
    }
    let complete = watching && facts.ownership_complete && facts.sources_complete && index.fresh();
    Model { by_name, entities: entity_candidates, complete, facts }
}
impl DiagnosticsProvider for DataTools {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if !supported(doc) {
            return Vec::new();
        }
        let file = self.repository_file(doc);
        let repos = repositories(&file);
        let ids: Vec<_> = repos.iter().map(|(i, _)| *i).collect();
        let mut found: Vec<_> = checks::check(&file, &ids, checks::Groups::from_settings())
            .iter()
            .filter(|f| match f.code {
                "page-without-pageable" => {
                    file.imports.iter().any(|i| i.path == "org.springframework.data.domain.Page")
                        && !file.types.iter().any(|t| t.name == "Page")
                }
                "jdbc-empty-sequence-name" => {
                    file.imports.iter().any(|i| i.path == "org.springframework.data.relational.core.mapping.Sequence")
                }
                "mapped-collection-key-column" => {
                    file.imports.iter().any(|i| i.path == "org.springframework.data.relational.core.mapping.MappedCollection")
                }
                _ => true,
            })
            .map(|f| f.diagnostic(&doc.text))
            .collect();
        if let Some(model) = self.model_for(doc, &file, true) {
            found.extend(problems(&model, &doc.text, &file).into_iter().filter(|d| {
                model.complete
                    || !matches!(
                        d.code.as_deref(),
                        Some(
                            "unknown-property"
                                | "mongo-unknown-field"
                                | "projection-unknown-property"
                                | "projection-property-type"
                                | "entity-graph-unknown-attribute"
                                | "entity-graph-not-association"
                        )
                    )
            }));
        }
        found.truncate(500);
        found
    }
}
struct Hint {
    from: usize,
    to: usize,
    description: String,
    parameters: Vec<(usize, String)>,
    parameter_tooltip: &'static str,
}
fn explicit_query_hint(file: &JavaFile, doc: &Document, method: &java::Member) -> Option<Hint> {
    const QUERY: &str = "org.springframework.data.jpa.repository.Query";
    if method.body.is_some() || method.modifiers.iter().any(|m|matches!(m.as_str(),"static"|"default")) { return None; }
    let queries: Vec<_> = method.annotations.iter().filter(|a| a.name == "Query").collect();
    let [query] = queries.as_slice() else { return None; };
    if !annotation_owned(file, doc, query) { return None; }
    let written: String = java::tokens(&doc.text[query.at+1..query.name_at+query.name.len()]).iter().map(|t|t.s).collect();
    if written != QUERY && !(written == "Query" && file.imports.iter().any(|i| !i.is_static && i.path == QUERY)) { return None; }
    if query.arg("nativeQuery").is_some_and(|a| a.text.trim() != "false") { return None; }
    let argument=query.arg("value")?;
    let [literal]=argument.strings.as_slice() else { return None; };
    if java::tokens(&argument.text).len()!=1 || literal.value.len()>4096 || literal.value.len()!=literal.end-literal.at { return None; }
    let names:std::collections::BTreeSet<_>=named_parameters(&literal.value).into_iter().map(|(_,name)|name).collect();
    let mut declared=BTreeMap::new();
    for param in &method.params {
        let aliases:Vec<_>=param.annotations.iter().filter(|a|a.name=="Param").collect();
        if aliases.is_empty() { continue; }
        let [alias]=aliases.as_slice() else { return None; };
        if !annotation_owned(file,doc,alias) { continue; }
        let argument=alias.arg("value")?;
        let [literal]=argument.strings.as_slice() else { return None; };
        if java::tokens(&argument.text).len()!=1 || literal.value.len()>64 || literal.value.len()!=literal.end-literal.at
            || !literal.value.bytes().next().is_some_and(|b|b.is_ascii_alphabetic()||b==b'_')
            || !literal.value.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_') { return None; }
        if declared.insert(literal.value.as_str(),param).is_some() { return None; }
    }
    let parameters=declared.iter().filter(|(name,_)|names.contains(**name)).map(|(name,param)|
        (param.at+param.name.len(),format!("query parameter: :{name}"))).collect::<Vec<_>>();
    Some(Hint {from:query.at,to:query.end,description:format!("Declared JPA query with {} named parameter(s), {} of them bound by an explicit @Param.",names.len(),parameters.len()),parameters,parameter_tooltip:"Explicit literal @Param alias also present in the declared query text. Source facts only; runtime binding is unverified."})
}
fn hover_text(value: &str) -> String {
    let mut text=String::new();
    for ch in value.chars().take(512) {
        if ch.is_control() {text.push(' ');} else {
            if "\\`*_{}[]<>#|!".contains(ch) {text.push('\\');}
            text.push(ch);
        }
    }
    if value.chars().count()>512 {text.push('…');}
    text
}
fn hints(model: &Model, file: &JavaFile, doc: &Document) -> Vec<Hint> {
    let mut out = Vec::new();
    if !host::setting_bool("springData.checkQueryTypes").unwrap_or(true) {
        return out;
    }
    for (ti, domain) in repositories(file) {
        let Some((path,class))=model.class(&domain) else { continue; };
        let repository=&file.types[ti];
        if repository.name.len()<=128 && domain.len()<=128 {
            out.push(Hint {from:repository.at,to:repository.at+repository.name.len(),
                description:format!("**Spring Data repository** {}\n\nManages {}, declared in {}:{}{}.",hover_text(&repository.name),hover_text(&domain),hover_text(path),class.range.start.line.saturating_add(1),if model.complete {""} else {" (the project index is incomplete, so this may be out of date)"}),parameters:Vec::new(),parameter_tooltip:""});
        }
        if !model.complete { continue; }
        for m in file.types[ti].methods().filter(|m| m.name.len() <= 256) {
            if let Some(hint) = explicit_query_hint(file, doc, m) { out.push(hint); }
            if !derived_method(m) { continue; }
            let Some(parts) = derived(&m.name).filter(|p| !p.is_empty()) else {
                continue;
            };
            if !parts.iter().all(|p| matches!(resolve(model, &domain, &m.name, p.from, p.to), Resolved::Found(_))) {
                continue;
            }
            let mut parameters = Vec::new();
            let mut params = m.params.iter().filter(|p| !SPECIAL_PARAMS.contains(&p.ty.name.as_str()));
            let mut valid = true;
            for p in &parts {
                let Some(count) = p.args else {
                    valid = false;
                    break;
                };
                for _ in 0..count {
                    let Some(param) = params.next() else {
                        valid = false;
                        break;
                    };
                    if count == 1 {
                        parameters.push((param.at + param.name.len(), format!("criterion: {}", &m.name[p.from..p.to])));
                    }
                }
            }
            if params.next().is_some() {
                valid = false;
            }
            out.push(Hint {from:m.at,to:m.at+m.name.len(),description:format!("Derived query on `{domain}`: {}.",describe(&m.name,&parts)),parameters:if valid {parameters}else{Vec::new()},parameter_tooltip:"Derived criterion bound by source argument order; special Pageable/Sort parameters excluded. Not an SQL binding."});
        }
    }
    out
}
impl HoverProvider for DataTools {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<Hover> {
        if !supported(doc) {
            return None;
        }
        let off = offset(&doc.text, at)?;
        let file = self.repository_file(doc);
        let model = self.model_for(doc, &file, false)?;
        let hint = hints(&model, &file, doc).into_iter().find(|h| h.from <= off && off < h.to)?;
        Some(Hover::new(hint.description).range(range(&doc.text, hint.from, hint.to)))
    }
}
impl InlayHints for DataTools {
    fn inlay_hints(&mut self, doc: &Document, requested: Range) -> Vec<InlayHint> {
        if !supported(doc) {
            return Vec::new();
        }
        let file = self.repository_file(doc);
        let Some(model) = self.model_for(doc, &file, false) else {
            return Vec::new();
        };
        hints(&model, &file, doc)
            .into_iter()
            .flat_map(|h| h.parameters.into_iter().map(move |(at,label)|(at,label,h.parameter_tooltip)))
            .filter_map(|(at, label, tooltip)| {
                let pos = openidea_plugin::text::position(&doc.text, at);
                if pos < requested.start || pos > requested.end {
                    return None;
                }
                Some(InlayHint::new(pos, label).padding_left().tooltip(tooltip))
            })
            .take(200)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    const ENTITY: &str = "package app; class Person {String name; int age;}";
    const IMPORT: &str = "import org.springframework.data.jpa.repository.JpaRepository;";
    fn doc(body: &str) -> Document {
        Document {
            path: "/project/PersonRepository.java".into(),
            language: "java".into(),
            version: 9,
            text: format!("package app; {IMPORT} interface PersonRepository extends JpaRepository<Person,Long> {{ {body} }}"),
        }
    }
    fn setup() -> (MockHost, DataTools) {
        let host = MockHost::install();
        host.add_file("Person.java", ENTITY);
        let mut tools = DataTools::default();
        tools.activate_live();
        (host, tools)
    }
    fn all() -> Range {
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX))
    }
    fn names(tools: &mut DataTools, d: &Document) -> Vec<String> {
        let at = d.text.find("findBy").unwrap() + "findBy".len();
        tools.completion(d, openidea_plugin::text::position(&d.text, at), None).into_iter().map(|c| c.label).collect()
    }
    #[test]
    fn live_checks_notice_shown_once() {
        let host = MockHost::install().with_capabilities(&["project.read"]);
        DataTools::default().activate_live();
        DataTools::default().activate_live();
        let notices: Vec<_> = host.notifications().into_iter().filter(|n| n.body.contains("needs the events permission")).collect();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].body.contains("Settings › Plugins › Spring Data Tools"));
    }
    #[test]
    fn newly_opened_nested_project_cannot_supply_outer_cached_positive_facts() {
        let host=MockHost::install();
        host.add_file("nested/Person.java", ENTITY);
        let mut tools=DataTools::default();
        tools.activate_live();
        let d=doc("Person findByName(String value);");
        let at=openidea_plugin::text::position(&d.text,d.text.find("findByName").unwrap()+7);
        assert!(tools.hover(&d,at).is_some());
        assert!(!tools.inlay_hints(&d,all()).is_empty());
        host.add_project("/project/nested");
        assert!(tools.hover(&d,at).is_none());
        assert!(tools.inlay_hints(&d,all()).is_empty());
        assert!(names(&mut tools,&d).is_empty());
        assert!(tools.diagnostics(&doc("Person findByNmae(String value);")).is_empty());
    }
    #[test]
    fn ownership_filter_retains_outer_positive_navigation_and_rejects_unknown_scope() {
        let host=MockHost::install();
        host.add_file("Person.java",ENTITY);
        host.add_file("nested/Other.java","package other; class Other {String value;}");
        let mut tools=DataTools::default(); tools.activate_live();
        host.add_project("/project/nested");
        let d=doc("Person findByName(String value);");
        assert!(names(&mut tools,&d).contains(&"findByName".into()));
        let at=openidea_plugin::text::position(&d.text,d.text.find("findByName").unwrap()+7);
        let targets=tools.definition(&d,at);
        assert_eq!(targets.len(),1); assert_eq!(targets[0].path,"/project/Person.java");
        assert!(tools.cached_markers(&d).iter().all(|m|m.target.as_ref().is_none_or(|target|!target.path.starts_with("/project/nested/"))));
        for path in ["/project/../outside/R.java","/project//R.java","/project/nested/R.java"] {
            let changed=Document {path:path.into(),..d.clone()};
            assert!(names(&mut tools,&changed).is_empty(),"{path}");
        }
        for i in 0..65 { host.add_project(&format!("/other-{i}")); }
        assert!(names(&mut tools,&d).is_empty(),"bounded root inventory must not invent ownership");
        assert!(tools.hover(&d,at).is_none());
        assert!(host.process_runs().is_empty() && host.requests().is_empty());
    }
    fn named_query_doc() -> Document {
        let mut d=doc("/* 😀 */ @Query(\"select p from Person p where p.name=:name or p.name=:name and p.age=:compilerOnly\") Person lookup(@Param(\"name\") String input, int compilerOnly);");
        d.text=d.text.replace(IMPORT,&format!("{IMPORT} import org.springframework.data.jpa.repository.Query; import org.springframework.data.repository.query.Param;"));
        d
    }
    #[test]
    fn explicit_jpa_query_hints_only_name_unique_literal_param_aliases() {
        let (_host,mut tools)=setup(); let d=named_query_doc();
        let hints=tools.inlay_hints(&d,all()); assert_eq!(hints.len(),1);
        assert_eq!(hints[0].label,"query parameter: :name");
        assert_eq!(hints[0].position,openidea_plugin::text::position(&d.text,d.text.find("String input").unwrap()+"String input".len()));
        let at=openidea_plugin::text::position(&d.text,d.text.find("@Query").unwrap()+1);
        let hover=tools.hover(&d,at).unwrap();
        assert!(hover.markdown.contains("2 named parameter(s), 1 of them bound"));
        let end=hover.range.unwrap().end; assert!(tools.hover(&d,end).is_none());
        assert!(tools.inlay_hints(&d,Range::new(Position::new(0,0),at)).is_empty());
        let changed=Document {text:d.text.replace("@Param(\"name\")","@Param(\"other\")"),version:d.version+1,..d.clone()};
        assert!(tools.inlay_hints(&changed,all()).is_empty());
        let qualified=Document {text:d.text.replace("@Query","@org.springframework.data.jpa.repository.Query").replace("@Param","@org.springframework.data.repository.query.Param"),..d};
        assert_eq!(tools.inlay_hints(&qualified,all()).len(),1);
    }
    #[test]
    fn named_query_hints_withhold_ambiguous_dynamic_native_or_foreign_bindings() {
        let (_host,mut tools)=setup(); let d=named_query_doc();
        for text in [
            d.text.replace("@Query(","@other.Query("),
            d.text.replace("@Param(","@other.Param("),
            d.text.replace("int compilerOnly","@Param(\"name\") int compilerOnly"),
            d.text.replace("@Param(\"name\")","@Param(NAME)"),
            d.text.replace("@Param(\"name\")","@Param(\"na\" + \"me\")"),
            d.text.replace("@Query(\"","@Query(nativeQuery=true,value=\""),
            d.text.replace("repository.Query;","repository.Query; import org.springframework.data.mongodb.repository.Query;"),
            d.text.replace("@Query(\"select","@Query(BASE + \"select"),
        ] { let invalid=Document {text,..d.clone()}; assert!(tools.inlay_hints(&invalid,all()).is_empty(),"{}",invalid.text); }
    }
    #[test]
    fn named_query_hint_model_and_feature_gates_remain_cache_only() {
        let (host,mut tools)=setup(); let d=named_query_doc();
        host.add_file("Person.java","package app; class Person {String changed;}");
        assert_eq!(tools.inlay_hints(&d,all()).len(),1,"saved source is not re-read by hint");
        let mut cold=DataTools::default(); assert!(cold.inlay_hints(&d,all()).is_empty());
        host.set_setting("springData.checkQueryTypes","false"); assert!(tools.inlay_hints(&d,all()).is_empty());
        host.set_setting("springData.checkQueryTypes","true");
        let huge=Document {text:d.text.replace("select p",&format!("select {} p"," ".repeat(4096))),..d.clone()};
        assert!(tools.inlay_hints(&huge,all()).is_empty());
        host.set_project_root(Some("/other")); assert!(tools.inlay_hints(&d,all()).is_empty());
        assert!(host.process_runs().is_empty() && host.requests().is_empty());
    }
    #[test]
    fn repository_header_hover_uses_positive_current_owner_domain_facts() {
        let (host,mut tools)=setup();
        let mut d=doc("Person findByName(String value);");
        d.text=d.text.replace("interface PersonRepository","/* 😀 */ interface PersonRepository");
        let at=openidea_plugin::text::position(&d.text,d.text.find("PersonRepository").unwrap());
        let hover=tools.hover(&d,at).unwrap();
        assert!(hover.markdown.contains("Spring Data repository") && hover.markdown.contains("Manages Person") && hover.markdown.contains("/project/Person.java:1"));
        assert_eq!(hover.range.unwrap(),range(&d.text,d.text.find("PersonRepository").unwrap(),d.text.find("PersonRepository").unwrap()+"PersonRepository".len()));
        host.add_file("Person.java","package app; class Person {String replaced;}");
        assert!(tools.hover(&d,at).is_some(),"cached positive fact does not re-read source");
        let unknown=Document {text:d.text.replace("JpaRepository<Person,Long>","JpaRepository<Unknown,Long>"),..d.clone()};
        assert!(tools.hover(&unknown,at).is_none());
        let mut cold=DataTools::default(); assert!(cold.hover(&d,at).is_none());
        host.add_project("/project/nested");
        let nested=Document {path:"/project/nested/R.java".into(),..d};
        assert!(tools.hover(&nested,at).is_none());
        assert!(host.process_runs().is_empty() && host.requests().is_empty());
    }
    #[test]
    fn repository_header_hover_labels_partial_context_and_escapes_bounded_source_path() {
        let host=MockHost::install();
        let path="domain/[unsafe](javascript:bad).java";
        host.add_file(path,ENTITY);
        host.add_file("nested/Other.java","package other; class Other {String value;}");
        let mut tools=DataTools::default(); tools.activate_live(); host.add_project("/project/nested");
        let d=doc("Person findByName(String value);");
        let at=openidea_plugin::text::position(&d.text,d.text.find("PersonRepository").unwrap());
        let hover=tools.hover(&d,at).unwrap();
        assert!(hover.markdown.contains("incomplete") && hover.markdown.contains("\\[unsafe\\]"));
        assert!(hover.markdown.len()<1400);
        assert!(tools.hover(&d,hover.range.unwrap().end).is_none());
        host.set_setting("springData.checkQueryTypes","false"); assert!(tools.hover(&d,at).is_none());
    }
    #[test]
    fn completion_never_discovers_and_uses_exact_imported_owner() {
        let host = MockHost::install();
        host.add_file("Person.java", ENTITY);
        host.add_file("other/Person.java", "package other; class Person {String foreign;}");
        let mut tools = DataTools::default();
        let d = doc("Person findBy(String value);");
        assert!(names(&mut tools, &d).is_empty());
        tools.activate_live();
        assert!(names(&mut tools, &d).contains(&"findByName".into()));
        assert!(!names(&mut tools, &d).contains(&"findByForeign".into()));
        host.add_file("Late.java", "package app; class Late {String discovered;}");
        let late = Document { text: d.text.replace("JpaRepository<Person,", "JpaRepository<Late,"), ..d.clone() };
        assert!(names(&mut tools, &late).is_empty());
        let foreign = Document { text: d.text.replace(IMPORT, &format!("{IMPORT} import other.Person;")), ..d };
        assert!(names(&mut tools, &foreign).contains(&"findByForeign".into()));
        assert!(!names(&mut tools, &foreign).contains(&"findByName".into()));
    }
    #[test]
    fn nested_completion_round_trips_and_uses_domain_imports() {
        let host = MockHost::install();
        host.add_file("Person.java", "package app; import domain.Address; class Person {Address address; String addressZipCode; String qCode; Q q;} class Q {String code;}");
        host.add_file("Address.java", "package domain; class Address {String zipCode;}");
        let mut tools = DataTools::default();
        tools.activate_live();
        let d = doc("Person findBy(String value);");
        let labels = names(&mut tools, &d);
        for expected in ["findByAddressZipCode", "findByAddress_ZipCode", "findByQCode", "findByQ_Code"] {
            assert!(labels.contains(&expected.to_owned()), "{expected}: {labels:?}");
        }
        assert!(!labels.iter().any(|s| s.contains("@type:")));
    }
    #[test]
    fn longest_known_head_commits_and_unknown_nested_types_stay_quiet() {
        let host = MockHost::install();
        host.add_file("Person.java", "package app; class Person {Address address; AddressZip addressZip; Library library;} class Address {String zipCode;} class AddressZip {String other;}");
        let mut tools = DataTools::default();
        tools.activate_live();
        let labels = names(&mut tools, &doc("Person findBy(String value);"));
        assert!(labels.contains(&"findByAddress_ZipCode".into()));
        assert!(!labels.contains(&"findByAddressZipCode".into()));
        assert!(!labels.contains(&"findByLibraryGuessed".into()));
        let d = doc("Person findByAddressZipCode(String value);");
        let diagnostics = tools.diagnostics(&d);
        assert!(diagnostics.iter().any(|d| d.message.contains("AddressZip") && d.message.contains("code")));
    }
    #[test]
    fn custom_container_aliases_and_unsaved_domain_never_guess_nested_fields() {
        let host = MockHost::install();
        host.add_file(
            "Person.java",
            "package app; import custom.List; class Person {List<Address> addresses;} class Address {String street;}",
        );
        let mut tools = DataTools::default();
        tools.activate_live();
        let d = doc("Person findBy(String value);");
        assert!(!names(&mut tools, &d).contains(&"findByAddressesStreet".into()));
        let entity = Document {
            path: "/project/Person.java".into(),
            language: "java".into(),
            version: 10,
            text: "package app; class Person {String renamed;}".into(),
        };
        tools.diagnostics(&entity);
        let labels = names(&mut tools, &d);
        assert!(labels.contains(&"findByRenamed".into()));
        assert!(!labels.contains(&"findByAddresses".into()));
        let generic = Document { text: d.text.replace("PersonRepository extends", "PersonRepository<T> extends"), ..d };
        assert!(names(&mut tools, &generic).is_empty());
    }
    #[test]
    fn provider_context_and_unicode_ranges_are_conservative() {
        let (_host, mut tools) = setup();
        for body in ["// findBy", "String value = \"findBy\";", "default void f() { findBy(); }"] {
            assert!(names(&mut tools, &doc(body)).is_empty(), "{body}");
        }
        let d = doc("/* 😀 */ Person findByName(String value);");
        let byte = d.text.find("Name(").unwrap();
        let links = tools.definition(&d, openidea_plugin::text::position(&d.text, byte));
        assert_eq!(links.len(), 1);
        assert!(links[0].path.ends_with("Person.java"));
    }
    #[test]
    fn unsaved_query_typo_and_fixed_version_replace_provider_results() {
        let (host, mut tools) = setup();
        let mut d = doc("Person findByNmae(String value);");
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        assert!(host.provider_diagnostics()[&d.path].iter().any(|d| d.code.as_deref() == Some("unknown-property")));
        d.version += 1;
        d.text = d.text.replace("Nmae", "Name");
        host.open_editor(&d.path, &d.language, &d.text, &[]);
        host.refresh_diagnostics(&mut tools);
        assert!(host.provider_diagnostics().get(&d.path).is_none_or(Vec::is_empty));
        tools.line_markers(&d);
        assert!(host.diagnostics().is_empty());
    }
    #[test]
    fn hover_and_criterion_inlays_use_known_source_and_utf16_ranges() {
        let (_host, mut tools) = setup();
        let mut d = doc("Person findByName(String value);");
        d.text = d.text.replace("Person findByName", "/* 😀 */ Person findByName");
        let at = openidea_plugin::text::position(&d.text, d.text.find("findByName").unwrap());
        let hover = tools.hover(&d, at).unwrap();
        assert_eq!(hover.range.unwrap().start, at);
        let hints = tools.inlay_hints(&d, all());
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].label, "criterion: Name");
        assert_eq!(hints[0].position, openidea_plugin::text::position(&d.text, d.text.find("value").unwrap() + 5));
        assert!(tools.inlay_hints(&d, Range::new(Position::new(0, 0), at)).is_empty());
    }
    #[test]
    fn cold_duplicate_unimported_and_library_owners_are_quiet() {
        let host = MockHost::install();
        let mut tools = DataTools::default();
        let d = doc("Person findByNmae(String value);");
        assert!(tools.diagnostics(&d).is_empty());
        assert!(tools.inlay_hints(&doc("Person findByName(String value);"), all()).is_empty());
        host.add_file("Person.java", ENTITY);
        host.add_file("other/Person.java", "package app; class Person {String other;}");
        tools.activate_live();
        assert!(tools.diagnostics(&d).is_empty());
        assert!(tools.inlay_hints(&doc("Person findByName(String value);"), all()).is_empty());
        for import in ["", "import other.JpaRepository;"] {
            let mut wrong = d.clone();
            wrong.text = wrong.text.replace(IMPORT, import);
            assert!(tools.diagnostics(&wrong).is_empty());
        }
    }
    #[test]
    fn current_entity_edit_and_malformed_or_oversized_facts_remain_conservative() {
        let (host, mut tools) = setup();
        let mut d = doc("Person findByName(String value);");
        d.text.push_str(" class Person {String renamed;}");
        assert!(tools.diagnostics(&d).is_empty(), "duplicate current document identity is not merged");
        let mut huge = d.clone();
        huge.text = " ".repeat(MAX_TEXT + 1);
        assert!(tools.diagnostics(&huge).is_empty());
        host.add_file("Broken.java", "class Broken {");
        tools.live_events(&[Event::FileSaved("/project/Broken.java".into())]);
        assert!(tools.diagnostics(&doc("Person findByNmae(String value);")).is_empty());
        let mut broken = doc("Person findByNmae(String value);");
        broken.text.pop();
        assert!(tools.diagnostics(&broken).is_empty());
    }
    #[test]
    fn unknown_library_annotations_do_not_become_spring_queries() {
        let (_host, mut tools) = setup();
        let mut d = doc("@Query(\"select x from Person x where x.name=:missing\") Person find();");
        d.text = d.text.replace(IMPORT, &format!("{IMPORT} import other.Query;"));
        assert!(tools.diagnostics(&d).is_empty());
    }
    #[test]
    fn foreign_qualified_or_ambiguous_query_annotations_do_not_borrow_spring_imports() {
        let (_host, mut tools) = setup();
        for extra in [
            "import org.springframework.data.jpa.repository.Query;",
            "import org.springframework.data.jpa.repository.Query; import org.springframework.data.mongodb.repository.Query;",
        ] {
            let mut d = doc("@other.Query(\"select p from Person p where p.name=:missing\") Person find();");
            d.text = d.text.replace(IMPORT, &format!("{IMPORT} {extra}"));
            assert!(tools.diagnostics(&d).is_empty(), "{}", d.text);
        }
        let mut ambiguous = doc("@Query(\"select p from Person p where p.name=:missing\") Person find();");
        ambiguous.text = ambiguous.text.replace(IMPORT, &format!("{IMPORT} import org.springframework.data.jpa.repository.Query; import org.springframework.data.mongodb.repository.Query;"));
        assert!(tools.diagnostics(&ambiguous).is_empty());
    }
    #[test]
    fn query_annotation_identity_is_current_and_independent_for_each_annotation() {
        let (_host, mut tools) = setup();
        for (name, namespace, method) in [
            ("Query", "org.springframework.data.jpa.repository.Query", "@Query(\"select p from Person p where p.name=:missing\") Person find();"),
            ("Modifying", "org.springframework.data.jpa.repository.Modifying", "@Modifying Person find();"),
            ("Param", "org.springframework.data.repository.query.Param", "Person findByName(@Param(\"x\") String value);"),
            ("NativeQuery", "org.springframework.data.jpa.repository.NativeQuery", "@NativeQuery(\"select * from person\") Person find();"),
        ] {
            let mut d = doc(method);
            d.text = d.text.replace(IMPORT, &format!("{IMPORT} import {namespace};"));
            assert_eq!(repository_file(&d).types[0].methods().count(), 1, "{name}");
            let foreign = Document { text:d.text.replace(&format!("@{name}"), &format!("@other.{name}")), version:d.version+1, ..d.clone() };
            assert_eq!(repository_file(&foreign).types[0].methods().count(), 0, "{name}");
            assert!(tools.diagnostics(&foreign).is_empty());
            assert_eq!(repository_file(&d).types[0].methods().count(), 1, "unsaved correction {name}");
        }
        let d = doc("/* 😀 */ @org.springframework.data.jpa.repository.Query(\"select p from Person p where p.name=:missing\") Person find();");
        let findings = tools.diagnostics(&d);
        let finding = findings.iter().find(|f|f.code.as_deref()==Some("unbound-query-parameter")).unwrap();
        let start = offset(&d.text,finding.range.start).unwrap();
        assert!(d.text[start..].starts_with(":missing"));
        let shadowed = Document { text:format!("{} class org {{}}",d.text), ..d };
        assert!(tools.diagnostics(&shadowed).is_empty());
    }
    #[test]
    fn foreign_jdbc_field_annotations_cannot_borrow_spring_imports() {
        let (_host, mut tools) = setup();
        for (name, property) in [
            ("Sequence", "@Sequence(sequence=\"\") Long id;"),
            ("MappedCollection", "@MappedCollection(idColumn=\"P\") java.util.List<Person> items;"),
        ] {
            let text = format!("package app; import org.springframework.data.relational.core.mapping.{name}; class Data {{ {property} }}");
            let d = Document {path:"/project/Data.java".into(),language:"java".into(),version:1,text};
            assert_eq!(tools.diagnostics(&d).len(),1,"positive {name}");
            let foreign = Document {text:d.text.replace(&format!("@{name}"),&format!("@other.{name}")),version:2,..d.clone()};
            assert!(tools.diagnostics(&foreign).is_empty(),"foreign {name}");
            assert_eq!(tools.diagnostics(&d).len(),1,"current correction {name}");
        }
    }
    #[test]
    fn jdbc_component_annotation_ownership_handles_import_ambiguity_and_unsaved_shapes() {
        let (_host, mut tools) = setup();
        for (path,language,body) in [
            ("/project/Record.java","java","record Data(@Sequence(sequence=\"\") Long id) {}"),
            ("/project/Data.kt","kotlin","class Data(@Sequence(sequence=\"\") val id: Long)"),
        ] {
            let terminator = if language == "java" { ";" } else { "" };
            let d=Document {path:path.into(),language:language.into(),version:1,text:format!("package app{terminator}\nimport org.springframework.data.relational.core.mapping.Sequence{terminator}\n{body}")};
            assert_eq!(tools.diagnostics(&d).len(),1,"{path}");
            for text in [
                d.text.replace("@Sequence", "@other.Sequence"),
                d.text.replace("class Data", "class Sequence {} class Data").replace("record Data", "class Sequence {} record Data"),
                d.text.replace(body, &format!("import other.Sequence{terminator}\n{body}")),
            ] {
                let unknown=Document {text,version:2,..d.clone()};
                assert!(tools.diagnostics(&unknown).is_empty(),"{}",unknown.text);
            }
        }
    }
    #[test]
    fn named_query_binding_and_local_groups_use_verified_annotations() {
        let (host, mut tools) = setup();
        let mut d = doc("@Query(\"select p from Person p where p.name=:missing\") Person find();");
        d.text = d.text.replace(IMPORT, &format!("{IMPORT} import org.springframework.data.jpa.repository.Query;"));
        assert!(tools.diagnostics(&d).iter().any(|d| d.code.as_deref() == Some("unbound-query-parameter")));
        let mut page = doc("Page<Person> findByName(String value);");
        page.text = page.text.replace(IMPORT, &format!("{IMPORT} import org.springframework.data.domain.Page;"));
        assert!(tools.diagnostics(&page).iter().any(|d| d.code.as_deref() == Some("page-without-pageable")));
        host.set_setting("springData.checkPageable", "false");
        assert!(tools.diagnostics(&page).iter().all(|d| d.code.as_deref() != Some("page-without-pageable")));
        host.set_setting("springData.checkQueryTypes", "false");
        assert!(tools.inlay_hints(&doc("Person findByName(String value);"), all()).is_empty());
    }
    #[test]
    fn unsaved_entity_snapshot_updates_without_reading_saved_source() {
        let (_host, mut tools) = setup();
        let repository = doc("Person findByName(String value);");
        assert!(tools.diagnostics(&repository).is_empty());
        let mut entity = Document {
            path: "/project/Person.java".into(),
            language: "java".into(),
            version: 14,
            text: "package app; class Person {String renamed;}".into(),
        };
        tools.diagnostics(&entity);
        assert!(tools.diagnostics(&repository).iter().any(|d| d.code.as_deref() == Some("unknown-property")));
        entity.version += 1;
        entity.text = ENTITY.into();
        tools.diagnostics(&entity);
        assert!(tools.diagnostics(&repository).is_empty());
    }
    #[test]
    fn concatenated_or_dynamic_queries_do_not_claim_binding_absence() {
        let (_host, mut tools) = setup();
        for value in ["BASE_QUERY", "\"select p from Person p where p.name=:missing\" + EXTRA"] {
            let mut d = doc(&format!("@Query({value}) Person find();"));
            d.text = d.text.replace(IMPORT, &format!("{IMPORT} import org.springframework.data.jpa.repository.Query;"));
            assert!(tools.diagnostics(&d).is_empty());
        }
    }
    #[test]
    fn qualified_foreign_domain_or_association_is_not_rebound_by_simple_name() {
        let (host, mut tools) = setup();
        let mut foreign = doc("Person findByNmae(String value);");
        foreign.text = foreign.text.replace("JpaRepository<Person,Long>", "JpaRepository<other.Person,Long>");
        assert!(tools.diagnostics(&foreign).is_empty());
        foreign.text = foreign.text.replace("JpaRepository<other.Person,Long>", "other.JpaRepository<Person,Long>");
        assert!(tools.diagnostics(&foreign).is_empty());
        host.add_file("Person.java", "package app; class Person {other.Owner owner;}");
        host.add_file("Owner.java", "package app; class Owner {String name;}");
        tools.live_events(&[Event::FileSaved("/project/Person.java".into()), Event::FileSaved("/project/Owner.java".into())]);
        assert!(tools.diagnostics(&doc("Person findByOwnerNmae(String value);")).is_empty());
        assert!(tools.inlay_hints(&doc("Person findByOwnerName(String value);"), all()).is_empty());
    }
    #[test]
    fn kotlin_unsaved_repository_uses_the_same_owned_source_contract() {
        let (_host, mut tools) = setup();
        let mut d=Document {path:"/project/PersonRepository.kt".into(),language:"kotlin".into(),version:2,text:"package app\nimport org.springframework.data.jpa.repository.JpaRepository\ninterface PersonRepository : JpaRepository<Person, Long> { fun findByNmae(value: String): Person }".into()};
        assert!(tools.diagnostics(&d).iter().any(|d| d.code.as_deref() == Some("unknown-property")));
        d.version += 1;
        d.text = d.text.replace("Nmae", "Name");
        assert!(tools.diagnostics(&d).is_empty());
        assert_eq!(tools.inlay_hints(&d, all()).len(), 1);
    }
    fn definition_doc(method: &str) -> Document {
        Document { path:"/project/PersonRepository.java".into(),language:"java".into(),version:20,
            text:format!("package app; import org.springframework.data.repository.RepositoryDefinition; @RepositoryDefinition(domainClass = Person.class, idClass = Long.class) interface PersonRepository {{ {method} }}") }
    }
    #[test]
    fn repository_definition_known_owner_drives_live_completion_navigation_hover_and_inlays() {
        let (_host,mut tools)=setup();
        let d=definition_doc("/* 😀 */ Person findByName(String value);");
        assert!(tools.diagnostics(&d).is_empty());
        let at=openidea_plugin::text::position(&d.text,d.text.find("findByName").unwrap());
        assert_eq!(tools.hover(&d,at).unwrap().range.unwrap().start,at);
        assert_eq!(tools.inlay_hints(&d,all()).len(),1);
        let property=openidea_plugin::text::position(&d.text,d.text.find("findByName").unwrap()+6);
        let targets=tools.definition(&d,property);
        assert_eq!(targets.len(),1);assert_eq!(targets[0].path,"/project/Person.java");
        let incomplete=definition_doc("Person findBy(String value);");
        assert!(names(&mut tools,&incomplete).contains(&"findByName".into()));
        let typo=definition_doc("Person findByNmae(String value);");
        assert!(tools.diagnostics(&typo).iter().any(|d|d.code.as_deref()==Some("unknown-property")));
    }
    #[test]
    fn repository_definition_rejects_unowned_duplicate_dynamic_and_competing_facts() {
        let (_host,mut tools)=setup();
        let d=definition_doc("Person findByNmae(String value);");
        for text in [
            d.text.replace("import org.springframework.data.repository.RepositoryDefinition;",""),
            d.text.replace("org.springframework.data.repository.RepositoryDefinition;","other.RepositoryDefinition;"),
            d.text.replace("domainClass = Person.class","domainClass = DOMAIN"),
            d.text.replace("domainClass = Person.class","domainClass = other.Person.class"),
            d.text.replace("idClass = Long.class","idClass = ID"),
            d.text.replace("idClass = Long.class","domainClass = Person.class"),
            d.text.replace("interface PersonRepository", "class RepositoryDefinition {} interface PersonRepository"),
            d.text.replace("interface PersonRepository", "interface PersonRepository<T>"),
            d.text.replace("interface PersonRepository", "interface PersonRepository extends LocalBase"),
            d.text.replace("@RepositoryDefinition(","@RepositoryDefinition(domainClass = Person.class, idClass = Long.class) @RepositoryDefinition("),
        ] {
            let invalid=Document{text,..d.clone()};
            assert!(tools.diagnostics(&invalid).is_empty(),"{}",invalid.text);
            assert!(tools.inlay_hints(&invalid,all()).is_empty());
            assert!(names(&mut tools,&invalid).is_empty());
        }
    }
    #[test]
    fn repository_definition_kotlin_and_fqn_annotations_keep_domain_import_scope() {
        let (host,mut tools)=setup();
        host.add_file("other/Person.java","package other; class Person {String external;}");
        tools.live_events(&[Event::FileSaved("/project/other/Person.java".into())]);
        let java=definition_doc("Person findBy(String value);");
        let imported=Document{text:java.text.replace("package app;","package app; import other.Person;"),..java.clone()};
        assert!(names(&mut tools,&java).contains(&"findByName".into()));
        assert!(names(&mut tools,&imported).contains(&"findByExternal".into()));
        assert!(!names(&mut tools,&imported).contains(&"findByName".into()));
        let fqn=Document{text:java.text.replace("import org.springframework.data.repository.RepositoryDefinition;", "").replace("@RepositoryDefinition(","@org.springframework.data.repository.RepositoryDefinition("),..java};
        assert!(names(&mut tools,&fqn).contains(&"findByName".into()));
        let kotlin=Document{path:"/project/PersonRepository.kt".into(),language:"kotlin".into(),version:1,text:"package app\nimport org.springframework.data.repository.RepositoryDefinition\n@RepositoryDefinition(domainClass = Person::class, idClass = Long::class)\ninterface PersonRepository { fun findByNmae(value: String): Person }".into()};
        assert!(tools.diagnostics(&kotlin).iter().any(|d|d.code.as_deref()==Some("unknown-property")));
        let fixed=Document{text:kotlin.text.replace("Nmae","Name"),version:2,..kotlin};
        assert!(tools.diagnostics(&fixed).is_empty());assert_eq!(tools.inlay_hints(&fixed,all()).len(),1);
    }
    #[test]
    fn repository_definition_cold_or_ambiguous_domain_never_discovers_or_guesses() {
        let host=MockHost::install();host.add_file("Person.java",ENTITY);
        let mut tools=DataTools::default();let d=definition_doc("Person findByNmae(String value);");
        assert!(names(&mut tools,&d).is_empty());
        tools.activate_live();
        assert!(!tools.diagnostics(&d).is_empty());
        host.add_file("Late.java","package app; class Late {String name;}");
        let late=Document{text:d.text.replace("domainClass = Person.class","domainClass = Late.class"),..d.clone()};
        assert!(tools.diagnostics(&late).is_empty());assert!(names(&mut tools,&late).is_empty());
        host.add_file("duplicate/Person.java",ENTITY);tools.live_events(&[Event::FileSaved("/project/duplicate/Person.java".into())]);
        assert!(tools.diagnostics(&d).is_empty());assert!(names(&mut tools,&d).is_empty());
    }

    fn unknown_property(tools: &mut DataTools, d: &Document) -> bool {
        tools.diagnostics(d).iter().any(|d| d.code.as_deref() == Some("unknown-property"))
    }
    #[test]
    fn derived_query_warnings_survive_ttl() {
        let (_host, mut tools) = setup();
        let d = doc("Person findByNmae(String value);");
        assert!(unknown_property(&mut tools, &d));
        tools.live.files.for_cached_document(&d.path, &d.text).unwrap().mark_stale();
        assert!(unknown_property(&mut tools, &d));
        assert!(tools.live.files.cached(&d.path).unwrap().fresh());
    }
    #[test]
    fn deleted_modified_path_no_global_reset() {
        let (_host, mut tools) = setup();
        let d = doc("Person findByNmae(String value);");
        let gone = "/project/Gone.java".to_owned();
        tools.live_events(&[Event::FilesChanged(openidea_plugin::FilesChanged { created: vec![], modified: vec![gone], removed: vec![] })]);
        let index = tools.live.files.cached(&d.path).expect("the index is kept");
        assert!(index.files().contains_key("/project/Person.java"));
        assert!(unknown_property(&mut tools, &d));
    }
    #[test]
    fn duplicate_unrelated_fqn_keeps_checks() {
        let host = MockHost::install();
        host.add_file("Person.java", ENTITY);
        for dir in ["a", "b"] {
            host.add_file(&format!("{dir}/Dup.java"), "package dup; class Dup {}");
            host.add_file(&format!("{dir}/Same.java"), &format!("package {dir}; @Entity class Same {{}}"));
        }
        let mut tools = DataTools::default();
        tools.activate_live();
        assert!(unknown_property(&mut tools, &doc("Person findByNmae(String value);")));
        let duplicated = Document { text: doc("Dup findByNmae(String value);").text.replace("Person,", "Dup,"), ..doc("") };
        assert!(!unknown_property(&mut tools, &duplicated));
    }
    #[test]
    fn dynamic_projection_method_checked() {
        let (_host, mut tools) = setup();
        assert!(unknown_property(&mut tools, &doc("<T> List<T> findByNmae(String value, Class<T> type);")));
        let local = Document { text: doc("<T> List<T> findByNmae(String value, Class<T> type);").text + " class Class<T> {}", ..doc("") };
        assert!(!unknown_property(&mut tools, &local));
    }
    #[test]
    fn wildcard_import_keeps_pageable_methods() {
        let (_host, mut tools) = setup();
        let mut d = doc("Page<Person> findByNmae(String value, Pageable page);");
        d.text = d.text.replace(IMPORT, &format!("{IMPORT} import org.springframework.data.domain.*;"));
        assert!(unknown_property(&mut tools, &d));
        let foreign = Document { text: d.text.replace("domain.*;", "domain.*; import other.Pageable;"), ..d };
        assert!(!unknown_property(&mut tools, &foreign));
    }
    #[test]
    fn parsed_file_and_model_facts_are_cached_per_version_and_generation() {
        let (host, mut tools) = setup();
        let d = doc("Person findByNmae(String value);");
        assert!(unknown_property(&mut tools, &d));
        let file = tools.repository_file(&d);
        let facts = tools.live.facts.as_ref().map(|(_, facts)| facts.clone()).unwrap();
        host.add_file("Person.java", "package app; class Person {String nmae;}");
        assert!(unknown_property(&mut tools, &d));
        assert!(Rc::ptr_eq(&file, &tools.repository_file(&d)));
        assert!(Rc::ptr_eq(&facts, &tools.live.facts.as_ref().unwrap().1));
        let next = Document { version: d.version + 1, ..d };
        assert!(!Rc::ptr_eq(&file, &tools.repository_file(&next)));
    }

}
