//! Explicit one-way DOM scaffold from a literal standalone XSD global QName.
use crate::*;
use openidea_plugin::file_actions::{FileAction, FileActions, FileOperation};
use openidea_plugin::{FileKind, PickItem};
use std::collections::BTreeMap;
const XSD: &str = "http://www.w3.org/2001/XMLSchema";
const MAX_FILES: usize = 512;
#[derive(Clone, PartialEq, Eq, Debug)]
struct QName {
    namespace: String,
    local: String,
}
#[derive(Clone)]
struct Pending {
    root: String,
    doc: Document,
    qname: QName,
}
#[derive(Default)]
pub(crate) struct Generation {
    next: u64,
    pending: BTreeMap<String, Pending>,
    /// Per root, when [`source_paths`] last ran and whether it hit the cap: carets don't re-list the project.
    capped: BTreeMap<String, (Instant, bool)>,
}
const CAP_TTL: Duration = Duration::from_secs(60);
#[derive(Clone, PartialEq, Eq)]
struct Package {
    name: String,
    directory: String,
    source: String,
}
struct Inventory {
    files: Vec<(String, String)>,
    packages: Vec<Package>,
    types: BTreeSet<String>,
    mappings: BTreeSet<(String, String)>,
}
fn path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && !path.chars().any(char::is_control)
        && !path.contains(['\\', '*', '?', '[', ']', '{', '}'])
        && !path.split('/').any(|part| part == "." || part == "..")
}
fn ident(name: &str) -> bool {
    let mut chars = name.bytes();
    name.len() <= 128
        && chars.next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && chars.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && ![
            "abstract",
            "assert",
            "boolean",
            "break",
            "byte",
            "case",
            "catch",
            "char",
            "class",
            "const",
            "continue",
            "default",
            "do",
            "double",
            "else",
            "enum",
            "extends",
            "final",
            "finally",
            "float",
            "for",
            "goto",
            "if",
            "implements",
            "import",
            "instanceof",
            "int",
            "interface",
            "long",
            "native",
            "new",
            "package",
            "private",
            "protected",
            "public",
            "return",
            "short",
            "static",
            "strictfp",
            "super",
            "switch",
            "synchronized",
            "this",
            "throw",
            "throws",
            "transient",
            "try",
            "void",
            "volatile",
            "while",
            "true",
            "false",
            "null",
            "record",
            "sealed",
            "permits",
            "var",
            "yield",
            "_",
        ]
        .contains(&name)
}
fn local(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 128
        && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}
fn namespace(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        && !value.contains(['\\', '"', '{', '}'])
}
/// One well-formed, namespace-valid root without a DOCTYPE, within the schema size bound.
pub(super) fn xml_valid(text: &str) -> bool {
    text.len() <= MAX_SCHEMA_BYTES && !text.to_ascii_lowercase().contains("<!doctype") && spring_text::xml::analysis_valid(text)
}
fn decode(value: &str) -> Option<String> {
    quick_xml::escape::unescape(value).ok().map(|value| value.into_owned())
}
fn declarations(text: &str) -> Option<Vec<(QName, usize, usize, bool)>> {
    if !xml_valid(text) {
        return None;
    }
    let elements = xml::elements(text);
    let bound = |mut index: usize, prefix: &str| {
        if prefix == "xml" {
            return true;
        }
        let attribute = format!("xmlns:{prefix}");
        loop {
            if let Some(declaration) = elements[index].attrs.iter().find(|value| value.name == attribute) {
                return !declaration.value.is_empty();
            }
            let Some(parent) = elements[index].parent else {
                return false;
            };
            index = parent;
        }
    };
    for (index, element) in elements.iter().enumerate() {
        if element.name.matches(':').count() > 1
            || element.name.split_once(':').is_some_and(|(prefix, _)| prefix == "xmlns" || !bound(index, prefix))
        {
            return None;
        }
        for attribute in &element.attrs {
            if attribute.name.matches(':').count() > 1
                || attribute.name == "xmlns:xmlns"
                || attribute.name == "xmlns:xml" && attribute.value != "http://www.w3.org/XML/1998/namespace"
            {
                return None;
            }
            if attribute.name.split_once(':').is_some_and(|(prefix, _)| prefix != "xmlns" && !bound(index, prefix)) {
                return None;
            }
        }
    }
    let mut found = Vec::new();
    for (index, schema) in
        elements.iter().enumerate().filter(|(index, schema)| schema.local() == "schema" && xml_namespace(&elements, *index) == Some(XSD))
    {
        let namespace = schema
            .attrs
            .iter()
            .find(|attribute| attribute.name == "targetNamespace")
            .map(|attribute| decode(&attribute.value))
            .transpose_option()?;
        let namespace = namespace.unwrap_or_default();
        if !namespace.is_empty() && !self::namespace(&namespace) {
            return None;
        }
        for (child, element) in
            elements.iter().enumerate().filter(|(_, element)| element.parent == Some(index) && element.local() == "element")
        {
            if xml_namespace(&elements, child) != Some(XSD) {
                continue;
            }
            let Some(name) = element.attrs.iter().find(|attribute| attribute.name == "name") else {
                continue;
            };
            let local = decode(&name.value)?;
            if !self::local(&local) {
                return None;
            }
            let abstract_ = element
                .attrs
                .iter()
                .find(|attribute| attribute.name == "abstract")
                .is_some_and(|attribute| !matches!(attribute.value.as_str(), "false" | "0"));
            let abstract_ = abstract_ || element.attrs.iter().any(|attribute| attribute.name == "ref");
            found.push((QName { namespace: namespace.clone(), local }, name.value_at, name.value_end, abstract_));
        }
    }
    Some(found)
}
// Keep optional absent namespace distinct from an undecodable declaration.
trait OptionalDecode<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> OptionalDecode<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            Some(Some(value)) => Some(Some(value)),
            Some(None) => None,
            None => Some(None),
        }
    }
}
fn selected(doc: &Document, at: usize) -> Option<QName> {
    if doc.language != "xml" || !doc.path.ends_with(".xsd") || !path(&doc.path) || !xml_valid(&doc.text) {
        return None;
    }
    let elements = xml::elements(&doc.text);
    let root = elements.first()?;
    if root.local() != "schema" || xml_namespace(&elements, 0) != Some(XSD) {
        return None;
    }
    if !elements.iter().enumerate().any(|(index, element)| {
        element.parent == Some(0)
            && element.local() == "element"
            && xml_namespace(&elements, index) == Some(XSD)
            && element.attrs.iter().any(|attribute| attribute.name == "name" && attribute.value_at <= at && at <= attribute.value_end)
    }) {
        return None;
    }
    let declarations = declarations(&doc.text)?;
    let (qname, _, _, abstract_) = declarations.iter().find(|(_, from, to, _)| *from <= at && at <= *to)?;
    if *abstract_
        || !namespace(&qname.namespace)
        || !local(&qname.local)
        || declarations.iter().filter(|(other, _, _, _)| other == qname).count() != 1
    {
        return None;
    }
    Some(qname.clone())
}
fn literal(arg: &Arg) -> Option<String> {
    let tokens = java::tokens(&arg.text);
    let [token] = tokens.as_slice() else {
        return None;
    };
    let value = token.string()?;
    let raw = arg.text.get(value.at..value.end)?;
    (raw == value.value && !raw.contains(['\\', '$']) && !raw.chars().any(char::is_control))
        .then_some(value.value)
}

fn source_paths(project: &openidea_plugin::Project) -> Result<BTreeSet<String>, Error> {
    let mut paths = BTreeSet::new();
    for glob in ["**/*.java", "**/*.kt", "**/*.xsd", "**/*.wsdl"] {
        let found = project.find_files(glob, (MAX_FILES + 1) as u32)?;
        if found.len() > MAX_FILES {
            return Err(format!("The project has too many source files to check for conflicts (over {MAX_FILES}).").into());
        }
        paths.extend(found);
        if paths.len() > MAX_FILES {
            return Err(format!("The project has too many source files to check for conflicts (over {MAX_FILES}).").into());
        }
    }
    Ok(paths)
}

fn inventory(project: &openidea_plugin::Project, root: &str, qname: &QName, source: &Document) -> Result<Inventory, Error> {
    // A task gives the call 60 s and 12 times the fuel of a plain call: enough to read all 8 MiB.
    let _task = host::begin_task("Checking the project for conflicts", false);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut paths = source_paths(project)?;
    paths.insert(source.path.clone());
    let mut result = Inventory { files: vec![], packages: vec![], types: BTreeSet::new(), mappings: BTreeSet::new() };
    let mut bytes = 0;
    let mut qnames = 0;
    for path_ in paths {
        if Instant::now() >= deadline {
            return Err("Checking the project for conflicts took over 20 seconds, so nothing was changed.".into());
        }
        if !path(&path_) || !path_.strip_prefix(root).is_some_and(|tail| tail.starts_with('/')) {
            return Err(Error::Stale);
        }
        if project_of(&path_).and_then(|project| project.root().ok()).as_deref() != Some(root) {
            continue;
        }
        let text = project.read_file(&path_)?;
        bytes += text.len();
        if text.len() > MAX_SCHEMA_BYTES || bytes > 8 * 1024 * 1024 {
            return Err("The project sources are too large to check for conflicts (over 128 KiB in one file or 8 MiB in total).".into());
        }
        if path_.ends_with(".xsd") || path_.ends_with(".wsdl") {
            let declarations = declarations(&text).ok_or("A schema file can't be parsed, so the element can't be checked for duplicates. Fix that schema first.")?;
            qnames += declarations.iter().filter(|(other, _, _, _)| other == qname).count();
        } else {
            if text.contains("\\u") || text.contains('`') {
                return Err("Unicode escapes or backticks in a source file prevent checking for conflicts.".into());
            }
            let file = parse_source(&path_, &text);
            let ownership = annotation_owners::Owners::new(&file, &text, path_.ends_with(".kt"));
            for ty in &file.types {
                result.types.insert(format!("{}.{}", file.package, ty.name));
                for method in ty.methods() {
                    for annotation in
                        method.annotations.iter().filter(|annotation| matches!(annotation.name.as_str(), "PayloadRoot" | "PayloadRoots"))
                    {
                        let members = payload_annotations(annotation, &text, path_.ends_with(".kt"));
                        if members.is_empty() {
                            return Err("A @PayloadRoot mapping can't be read, so existing handlers can't be checked.".into());
                        }
                        for member in members {
                            let local = member
                                .args
                                .iter()
                                .find(|arg| arg.name == "localPart")
                                .and_then(literal)
                                .ok_or("A @PayloadRoot localPart is not a plain string, so existing handlers can't be checked.")?;
                            if local != qname.local {
                                continue;
                            }
                            let namespace = match member.args.iter().find(|arg| arg.name == "namespace") {
                                Some(arg) => literal(arg).ok_or("A @PayloadRoot namespace is not a plain string, so existing handlers can't be checked.")?,
                                None => String::new(),
                            };
                            result.mappings.insert((namespace, local));
                        }
                    }
                }
            }
            if path_.ends_with(".java") && file.package.split('.').all(ident) && !file.package.is_empty() {
                let tail = format!("{}/{}", file.package.replace('.', "/"), path_.rsplit('/').next().unwrap_or_default());
                let prefix = path_
                    .strip_suffix(&tail)
                    .filter(|prefix| prefix.ends_with("/src/main/java/") || *prefix == format!("{root}/src/main/java/"));
                if prefix.is_some()
                    && file.types.iter().any(|ty| ty.outer.is_none() && ty.kind == "class" && ownership.endpoint(&text, ty))
                {
                    let directory = path_.rsplit_once('/').ok_or(Error::Stale)?.0.to_string();
                    result.packages.push(Package { name: file.package.clone(), directory, source: path_.clone() });
                }
            }
        }
        result.files.push((path_, text));
    }
    if qnames != 1 {
        return Err("The element must be declared exactly once in the project's schemas.".into());
    }
    if result.mappings.contains(&(qname.namespace.clone(), qname.local.clone())) {
        return Err("An endpoint method already handles this element.".into());
    }
    result.packages.sort_by(|a, b| a.directory.cmp(&b.directory));
    result.packages.dedup_by(|a, b| a.directory == b.directory);
    Ok(result)
}
fn current(pending: &Pending, doc: &Document) -> Result<openidea_plugin::Project, Error> {
    if pending.doc.path != doc.path
        || pending.doc.language != doc.language
        || pending.doc.version != doc.version
        || pending.doc.text != doc.text
    {
        return Err(Error::Stale);
    }
    let project = project_of(&doc.path).ok_or(Error::Stale)?;
    if project.root()? != pending.root || project.read_file(&doc.path)? != doc.text {
        return Err(Error::Stale);
    }
    Ok(project)
}
fn vacant(project: &openidea_plugin::Project, root: &str, package: &Package, class: &str) -> Result<String, Error> {
    let directory = &package.directory;
    if !path(directory) || !directory.strip_prefix(root).is_some_and(|tail| tail.starts_with('/')) {
        return Err(Error::Stale);
    }
    let mut current = root.to_string();
    for part in directory.strip_prefix(&format!("{root}/")).ok_or(Error::Stale)?.split('/') {
        current.push('/');
        current.push_str(part);
        if project.stat(&current)?.kind != FileKind::Directory {
            return Err("The package folder can't hold a new file.".into());
        }
    }
    let target = format!("{directory}/{class}.java");
    if !path(&target) {
        return Err("The new file path is too long.".into());
    }
    for ext in ["java", "kt"] {
        let path = format!("{directory}/{class}.{ext}");
        match project.stat(&path) {
            Err(Error::NotFound(_)) => {}
            Ok(_) => return Err("A file with this class name already exists in this package.".into()),
            Err(error) => return Err(error),
        }
        match project.read_file(&path) {
            Err(Error::NotFound(_)) => {}
            Ok(_) => return Err("An open editor already uses this file name.".into()),
            Err(error) => return Err(error),
        }
    }
    Ok(target)
}
fn scaffold(package: &str, class: &str, method: &str, qname: &QName, eol: &str) -> String {
    format!("package {package};\n\n@org.springframework.ws.server.endpoint.annotation.Endpoint\npublic class {class} {{\n    @org.springframework.ws.server.endpoint.annotation.PayloadRoot(namespace = \"{}\", localPart = \"{}\")\n    public void {method}(\n            @org.springframework.ws.server.endpoint.annotation.RequestPayload org.w3c.dom.Element request) {{\n        // TODO Implement one-way request handling; no response contract is inferred.\n    }}\n}}\n",qname.namespace,qname.local).replace('\n',eol)
}
impl FileActions for WsTools {
    fn file_actions(&mut self, doc: &Document, selection: Range) -> Vec<FileAction> {
        let Some(at) = openidea_plugin::text::offset(&doc.text, selection.start) else {
            return vec![];
        };
        let Some(qname) = selected(doc, at) else {
            return vec![];
        };
        let Some(root) = project_of(&doc.path).and_then(|project| project.root().ok()) else {
            return vec![];
        };
        if !path(&root) {
            return vec![];
        }
        let Some(project) = project_of(&doc.path) else {
            return vec![];
        };
        let capped = match self.generation.capped.get(&root).filter(|(at, _)| at.elapsed() < CAP_TTL) {
            Some((_, capped)) => *capped,
            None => {
                let capped = source_paths(&project).is_err();
                self.generation.capped.insert(root.clone(), (Instant::now(), capped));
                capped
            }
        };
        if capped {
            return vec![];
        }
        if self.generation.pending.len() >= 8 {
            self.generation.pending.clear();
        }
        let Some(next) = self.generation.next.checked_add(1) else {
            return vec![];
        };
        self.generation.next = next;
        let key = format!("ws-dom-create:{next}:{}", doc.version);
        self.generation.pending.insert(key.clone(), Pending { root, doc: doc.clone(), qname });
        vec![FileAction {
            title: "Create Spring-WS Endpoint for Element…".into(),
            kind: Some("refactor".into()),
            data: Some(key),
            ..Default::default()
        }]
    }
    fn resolve_file_action(&mut self, doc: &Document, action: FileAction) -> Result<FileAction, Error> {
        let pending = self.generation.pending.remove(action.data.as_deref().ok_or(Error::Stale)?).ok_or(Error::Stale)?;
        let project = current(&pending, doc)?;
        let before = inventory(&project, &pending.root, &pending.qname, doc)?;
        if before.packages.is_empty() {
            return Err("No Spring-WS endpoint package found under src/main/java. Create one @Endpoint class there first.".into());
        }
        let choices: Vec<_> = before
            .packages
            .iter()
            .enumerate()
            .map(|(index, package)| PickItem::new(&index.to_string(), &format!("{} — {}", package.name, package.directory)))
            .collect();
        let Some(choice) = host::pick("Package for the new endpoint", &choices)? else {
            return Err(Error::Cancelled);
        };
        let package = before.packages.get(choice.parse::<usize>().map_err(|_| Error::Stale)?).ok_or(Error::Stale)?;
        let Some(class) = host::input("Endpoint class name", Some("PayloadEndpoint"))? else {
            return Err(Error::Cancelled);
        };
        if !ident(&class) {
            return Err("Use a valid Java class name: ASCII letters, digits and _, not a keyword, up to 128 characters.".into());
        }
        let Some(method) = host::input("Handler method name", Some("handlePayload"))? else {
            return Err(Error::Cancelled);
        };
        if !ident(&method) {
            return Err("Use a valid Java method name: ASCII letters, digits and _, not a keyword, up to 128 characters.".into());
        }
        if class == "org" || before.types.contains(&format!("{}.org", package.name)) {
            return Err("A class named org in this package would hide the org.* names the endpoint uses. Choose another package.".into());
        }
        if before.types.contains(&format!("{}.{class}", package.name)) {
            return Err("A class with this name already exists in this package.".into());
        }
        let target = vacant(&project, &pending.root, package, &class)?;
        let content = scaffold(&package.name, &class, &method, &pending.qname, if doc.text.contains("\r\n") { "\r\n" } else { "\n" });
        current(&pending, doc)?;
        let preview = format!(
            "Element: {{{}}}{}\nSchema: {}\nNew file: {target}\n\nThe handler takes the request as a DOM Element and returns nothing (one-way). Add the response and the business logic yourself.\n\n{content}",
            pending.qname.namespace,
            pending.qname.local,
            doc.path
        );
        if !ui_kit::prompts::confirm_exact(&format!("Create endpoint {class}?"), &preview)? {
            return Err(Error::Cancelled);
        }
        current(&pending, doc)?;
        vacant(&project, &pending.root, package, &class)?;
        Ok(FileAction {
            title: format!("Create one-way endpoint {class}"),
            kind: Some("refactor".into()),
            operations: vec![FileOperation::Create { path: target.clone(), text: content, overwrite: false }],
            open: Some(target),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    const SCHEMA: &str = "/project/src/main/resources/request.xsd";
    fn doc() -> Document {
        Document {
            path: SCHEMA.into(),
            language: "xml".into(),
            version: 1,
            text: format!(
                "<!-- é😀 -->\n<xs:schema xmlns:xs=\"{XSD}\" targetNamespace=\"urn:acme\">\n  <xs:element name=\"CreateRequest\" type=\"xs:string\"/>\n</xs:schema>\n"
            ),
        }
    }
    fn setup() -> (MockHost, Document) {
        let host = MockHost::install();
        let doc = doc();
        host.add_file(SCHEMA, &doc.text);
        host.add_file(
            "src/main/java/com/acme/Existing.java",
            "package com.acme; import org.springframework.ws.server.endpoint.annotation.Endpoint; @Endpoint public class Existing {}",
        );
        for dir in ["src", "src/main", "src/main/java", "src/main/java/com", "src/main/java/com/acme"] {
            host.add_dir(dir);
        }
        (host, doc)
    }
    fn lazy(plugin: &mut WsTools, doc: &Document) -> FileAction {
        let at = openidea_plugin::text::position(&doc.text, doc.text.find("CreateRequest").unwrap() + 2);
        plugin.file_actions(doc, Range::new(at, at)).remove(0)
    }
    fn answer(host: &MockHost) {
        for value in ["0", "CreateEndpoint", "handleCreate", "yes"] {
            host.answer_next_prompt(Some(&[value]));
        }
    }
    #[test]
    fn qname_to_one_way_dom_scaffold_is_exact_atomic_and_non_overwriting() {
        let (host, mut doc) = setup();
        doc.text = doc.text.replace('\n', "\r\n");
        host.open_editor(SCHEMA, "xml", &doc.text, &[]);
        let mut plugin = WsTools::default();
        let offered = lazy(&mut plugin, &doc);
        assert!(offered.operations.is_empty());
        answer(&host);
        let resolved = plugin.resolve_file_action(&doc, offered).unwrap();
        assert!(resolved.edits.is_empty());
        let [FileOperation::Create { path, text, overwrite }] = resolved.operations.as_slice() else { panic!("one Create") };
        assert_eq!(path, "/project/src/main/java/com/acme/CreateEndpoint.java");
        assert!(!overwrite);
        assert_eq!(resolved.open.as_deref(), Some(path.as_str()));
        assert!(text.contains("public void handleCreate("));
        assert!(text.contains("@org.springframework.ws.server.endpoint.annotation.RequestPayload org.w3c.dom.Element request"));
        assert!(text.contains("namespace = \"urn:acme\", localPart = \"CreateRequest\""));
        assert!(!text.contains("ResponsePayload"));
        assert!(!text.replace("\r\n", "").contains('\n'));
        let endpoints = endpoints_with_syntax(&java::parse(text), text, false);
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].local, "CreateRequest");
        assert_eq!(endpoints[0].namespace.as_deref(), Some("urn:acme"));
        assert_eq!(host.editor_text(), doc.text);
        assert!(host.invocation().project().unwrap().read_file(path).is_err());
    }
    #[test]
    fn cancellation_at_each_prompt_and_forged_replay_return_no_edit() {
        for stage in 0..4 {
            let (host, doc) = setup();
            let mut plugin = WsTools::default();
            let offered = lazy(&mut plugin, &doc);
            let replay = offered.clone();
            for value in ["0", "CreateEndpoint", "handleCreate"].iter().take(stage) {
                host.answer_next_prompt(Some(&[value]));
            }
            host.answer_next_prompt(None);
            assert!(matches!(plugin.resolve_file_action(&doc, offered), Err(Error::Cancelled)));
            assert!(matches!(plugin.resolve_file_action(&doc, replay), Err(Error::Stale)));
        }
    }
    #[test]
    fn stale_source_version_content_and_project_do_not_use_old_qnames() {
        let (host, doc) = setup();
        let mut plugin = WsTools::default();
        let offered = lazy(&mut plugin, &doc);
        let mut changed = doc.clone();
        changed.version += 1;
        assert!(matches!(plugin.resolve_file_action(&changed, offered), Err(Error::Stale)));
        let offered = lazy(&mut plugin, &doc);
        host.add_file(SCHEMA, "changed");
        assert!(matches!(plugin.resolve_file_action(&doc, offered), Err(Error::Stale)));
        host.add_file(SCHEMA, &doc.text);
        let offered = lazy(&mut plugin, &doc);
        host.set_project_root(Some("/other"));
        assert!(plugin.resolve_file_action(&doc, offered).is_err());
    }
    #[test]
    fn only_well_formed_global_xsd_qnames_can_offer_scaffolds() {
        let _host = MockHost::install();
        let good = doc();
        let at = good.text.find("CreateRequest").unwrap() + 2;
        assert!(selected(&good, at).is_some());
        for text in [
            good.text.replace(XSD, "foreign"),
            good.text.replace("targetNamespace=\"urn:acme\"", ""),
            good.text.replace("name=\"CreateRequest\"", "ref=\"CreateRequest\""),
            good.text.replace("type=\"xs:string\"", "abstract=\"true\""),
            good.text.replace("</xs:schema>", ""),
            good.text.replace("<xs:element", "<x:element"),
            good.text.replace("type=\"xs:string\"", "ref=\"other:Request\""),
            good.text.replace("</xs:schema>", "<bad:other/></xs:schema>"),
            good.text.replace("</xs:schema>", "<xs:annotation>&unknownEntity;</xs:annotation></xs:schema>"),
            good.text
                .replace("<xs:element", "<xs:complexType><xs:sequence><xs:element")
                .replace("</xs:schema>", "</xs:sequence></xs:complexType></xs:schema>"),
            good.text.replace("targetNamespace=\"urn:acme\"", "targetNamespace=\"${dynamic}\""),
            format!("<!DOCTYPE xs:schema SYSTEM \"remote\">{}", good.text),
            format!("<!doctype xs:schema SYSTEM \"remote\">{}", good.text),
        ] {
            let changed = Document { text, ..good.clone() };
            let at = changed.text.find("CreateRequest").unwrap() + 2;
            assert!(selected(&changed, at).is_none(), "{}", changed.text);
        }
        let mut encoded = good.clone();
        encoded.text = encoded.text.replace("CreateRequest", "Create&#82;equest");
        let at = encoded.text.find("Create&#").unwrap() + 2;
        assert_eq!(selected(&encoded, at).unwrap().local, "CreateRequest");
    }
    #[test]
    fn duplicate_qnames_and_literal_or_unknown_handler_mappings_refuse_generation() {
        let (host, doc) = setup();
        let project = host.invocation().project().unwrap();
        let qname = selected(&doc, doc.text.find("CreateRequest").unwrap()).unwrap();
        host.add_file("src/main/resources/duplicate.xsd", &doc.text);
        assert!(inventory(&project, "/project", &qname, &doc).is_err());
        host.add_file("src/main/resources/duplicate.xsd", &doc.text.replace("CreateRequest", "OtherRequest"));
        for namespace in ["\"urn:acme\"", "NAMESPACE", "runtimeNamespace()"] {
            host.add_file("src/main/java/com/acme/Handlers.java",&format!("package com.acme; @Endpoint class Handlers {{ @PayloadRoot(namespace={namespace},localPart=\"CreateRequest\") public void handle() {{}} }}"));
            assert!(inventory(&project, "/project", &qname, &doc).is_err());
        }
        host.add_file("src/main/java/com/acme/Handlers.java","package com.acme; @Endpoint class Handlers { @PayloadRoot(namespace=NAMESPACE,localPart=\"OtherRequest\") public void handle() {} }");
        assert!(inventory(&project, "/project", &qname, &doc).is_ok(), "different literal local part cannot collide with selected QName");
    }
    #[test]
    fn target_file_directory_type_invalid_names_and_unknown_package_are_refused() {
        let (host, doc) = setup();
        let mut plugin = WsTools::default();
        host.add_dir("src/main/java/com/acme/CreateEndpoint.java");
        let offered = lazy(&mut plugin, &doc);
        answer(&host);
        assert!(plugin.resolve_file_action(&doc, offered).is_err());
        for invalid in ["../Escape", "class", "record", "Foo.Bar", "bad-name", ""] {
            assert!(!ident(invalid));
        }
        let (host, doc) = setup();
        host.add_file("src/main/java/com/acme/Other.java", "package com.acme; class CreateEndpoint {}");
        let mut plugin = WsTools::default();
        let offered = lazy(&mut plugin, &doc);
        answer(&host);
        assert!(plugin.resolve_file_action(&doc, offered).is_err());
        let (host, doc) = setup();
        host.add_file("src/main/java/com/acme/Existing.java", "package com.acme; import other.Endpoint; @Endpoint class Existing {}");
        let mut plugin = WsTools::default();
        let offered = lazy(&mut plugin, &doc);
        assert!(plugin.resolve_file_action(&doc, offered).is_err());
    }
    #[test]
    fn fresh_inventory_is_bounded_and_captures_package_or_source_changes() {
        let (host, doc) = setup();
        let project = host.invocation().project().unwrap();
        let qname = selected(&doc, doc.text.find("CreateRequest").unwrap()).unwrap();
        let before = inventory(&project, "/project", &qname, &doc).unwrap();
        assert_eq!(before.packages.len(), 1);
        host.add_file("src/main/java/com/acme/New.java", "package com.acme; class New {}");
        let after = inventory(&project, "/project", &qname, &doc).unwrap();
        assert_ne!(before.files, after.files);
        for n in 0..MAX_FILES {
            host.add_file(&format!("src/C{n}.java"), "");
        }
        assert!(inventory(&project, "/project", &qname, &doc).is_err());
    }
#[test]
fn escaped_or_interpolated_existing_mapping_cannot_prove_qname_availability() {
    for (namespace, local) in [
        (r"urn:\141cme", "CreateRequest"),
        ("urn:acme", r"Create\122equest"),
    ] {
        let (host, doc) = setup();
        host.add_file("src/main/java/com/acme/Handlers.java",&format!(r#"package com.acme; @org.springframework.ws.server.endpoint.annotation.Endpoint class Handlers {{ @org.springframework.ws.server.endpoint.annotation.PayloadRoot(namespace="{namespace}",localPart="{local}") void handle(){{}} }}"#));
        let project = host.invocation().project().unwrap();
        let qname = selected(&doc, doc.text.find("CreateRequest").unwrap()).unwrap();
        assert!(
            inventory(&project, "/project", &qname, &doc).is_err(),
            "namespace={namespace}, local={local}"
        );
    }
    let (host, doc) = setup();
    host.add_file("src/main/kotlin/com/acme/Handlers.kt",r#"package com.acme
@org.springframework.ws.server.endpoint.annotation.Endpoint class Handlers {
companion object {const val NS="urn:acme"}
@org.springframework.ws.server.endpoint.annotation.PayloadRoot(namespace="$NS",localPart="CreateRequest") fun handle(){}
}"#);
    let project = host.invocation().project().unwrap();
    let qname = selected(&doc, doc.text.find("CreateRequest").unwrap()).unwrap();
    assert!(inventory(&project, "/project", &qname, &doc).is_err());
}
#[test]
fn conflicting_static_annotation_import_cannot_establish_generation_package() {
    let (host, doc) = setup();
    host.add_file("src/main/java/com/acme/Existing.java","package com.acme; import org.springframework.ws.server.endpoint.annotation.Endpoint; import static foreign.Holder.Endpoint; @Endpoint public class Existing {}");
    let mut plugin = WsTools::default();
    let action = lazy(&mut plugin, &doc);
    answer(&host);
    assert!(plugin.resolve_file_action(&doc, action).is_err());
}
    #[test]
    fn scaffold_not_offered_over_limit() {
        let (host, doc) = setup();
        let mut plugin = WsTools::default();
        let at = openidea_plugin::text::position(&doc.text, doc.text.find("CreateRequest").unwrap() + 2);
        assert_eq!(plugin.file_actions(&doc, Range::new(at, at)).len(), 1);
        for n in 0..MAX_FILES {
            host.add_file(&format!("src/C{n}.java"), "");
        }
        assert_eq!(plugin.file_actions(&doc, Range::new(at, at)).len(), 1, "the verdict is cached per root, not re-listed per caret");
        assert!(WsTools::default().file_actions(&doc, Range::new(at, at)).is_empty());
    }
}
