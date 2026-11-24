//! Explicit source-only channel creation. No runtime/job/classpath operations.
use crate::*;
use openidea_plugin::file_actions::{FileAction, FileActions};
use openidea_plugin::{TextEdit, text};
const MAX_TEXT: usize = 12 << 10;
const BEANS_NS: &str = "http://www.springframework.org/schema/beans";
#[derive(Clone)]
struct Pending {
    root: String,
    doc: Document,
    at: usize,
    kind: ActionKind,
}
#[derive(Clone, Copy)]
enum ActionKind {
    Channel,
    Bridge,
}
#[derive(Default)]
pub(crate) struct Creation {
    next: u64,
    pending: BTreeMap<String, Pending>,
}
struct Target {
    prefix: String,
    at: usize,
    indent: String,
    eol: &'static str,
}
fn path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 512
        && !path.contains(['\\', '*', '?', '[', ']', '{', '}'])
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "." || part == "..")
}
fn name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 128
        && !matches!(name, "nullChannel" | "errorChannel")
        && bytes
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}
/// Strict XML event validation precedes use of the existing offset scanner.
fn target(doc: &Document, at: usize) -> Option<Target> {
    if doc.language != "xml"
        || !doc.path.ends_with(".xml")
        || !path(&doc.path)
        || doc.text.len() > MAX_TEXT
        || doc.text.contains('&')
        || beans::ignored(&doc.path)
    {
        return None;
    }
    if !spring_text::xml::generation_valid(&doc.text) {
        return None;
    }
    let elements = xml::elements(&doc.text);
    let root = elements.first()?;
    if root.name != "beans"
        || root
            .attrs
            .iter()
            .find(|attribute| attribute.name == "xmlns")?
            .value
            != BEANS_NS
    {
        return None;
    }
    if elements.iter().skip(1).any(|element| {
        element.local() == "beans"
            || element.attrs.iter().any(|attribute| {
                attribute.name == "xmlns"
                    || attribute.name.starts_with("xmlns:")
                    || attribute.name.starts_with("xml:")
            })
    }) {
        return None;
    }
    if root.attrs.iter().any(|attribute| {
        attribute.name == "xmlns:xmlns"
            || attribute.name == "xmlns:xml"
                && attribute.value != "http://www.w3.org/XML/1998/namespace"
    }) {
        return None;
    }
    let declared: BTreeSet<_> = root
        .attrs
        .iter()
        .filter_map(|attribute| attribute.name.strip_prefix("xmlns:"))
        .chain(std::iter::once("xml"))
        .collect();
    if elements.iter().any(|element| {
        element
            .name
            .split_once(':')
            .is_some_and(|(prefix, _)| !declared.contains(prefix))
            || element.attrs.iter().any(|attribute| {
                attribute
                    .name
                    .split_once(':')
                    .is_some_and(|(prefix, _)| prefix != "xmlns" && !declared.contains(prefix))
            })
    }) {
        return None;
    }
    let prefix = root.attrs.iter().find_map(|attribute| {
        let prefix = attribute.name.strip_prefix("xmlns:")?;
        (attribute.value == NAMESPACE
            && name(prefix)
            && !matches!(prefix, "xml" | "xmlns")
            && !prefix.contains(['.', '-']))
        .then(|| prefix.to_string())
    })?;
    let close = doc.text[..root.end].rfind("</beans>")?;
    if close < root.tag_end
        || at < root.start
        || at > root.end
        || xml::element_at(&elements, at).is_some_and(|index| index != 0)
    {
        return None;
    }
    let line = doc.text[..close].rfind('\n').map_or(0, |at| at + 1);
    let closing_indent = &doc.text[line..close];
    let (at, indent) = if closing_indent.chars().all(|ch| ch == ' ' || ch == '\t') {
        (line, format!("{closing_indent}  "))
    } else {
        (close, "  ".into())
    };
    Some(Target {
        prefix,
        at,
        indent,
        eol: if doc.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        },
    })
}
fn current(pending: &Pending, doc: &Document) -> Result<Project, Error> {
    if doc.path != pending.doc.path
        || doc.language != pending.doc.language
        || doc.version != pending.doc.version
        || doc.text != pending.doc.text
    {
        return Err(Error::Stale);
    }
    let project = host::project_of(&doc.path).ok_or(Error::Stale)?;
    if project.root()? != pending.root || project.read_file(&doc.path)? != doc.text {
        return Err(Error::Stale);
    }
    Ok(project)
}
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Inventory {
    pub ids: BTreeSet<String>,
    pub types: BTreeSet<(String, String)>,
}
fn names(project: &Project, doc: &Document) -> Result<BTreeSet<String>, Error> {
    Ok(inventory_inner(project, doc, false)?.ids)
}
pub(crate) fn inventory(project: &Project, doc: &Document) -> Result<Inventory, Error> {
    inventory_inner(project, doc, true)
}
fn inventory_inner(
    project: &Project,
    doc: &Document,
    include_types: bool,
) -> Result<Inventory, Error> {
    // A task gives the call 60 s and 12 times the fuel of a plain call: enough to read all 8 MiB.
    let _task = host::begin_task("Checking that the name is free", false);
    let started = std::time::Instant::now();
    let root = project.root()?;
    let mut paths = BTreeSet::new();
    for glob in &beans::GLOBS {
        let found = project.find_files(glob, 1025)?;
        if found.len() > 1024 {
            return Err("Source inventory capped; cannot safely check channel IDs.".into());
        }
        paths.extend(found);
        if paths.len() > 1024 {
            return Err("Source inventory capped; cannot safely check channel IDs.".into());
        }
    }
    paths.insert(doc.path.clone());
    let mut names = BTreeSet::new();
    let mut types = BTreeSet::new();
    let mut type_bytes = 0usize;
    let mut bytes = 0usize;
    let mut name_bytes = 0usize;
    for path in paths {
        if started.elapsed() > std::time::Duration::from_secs(20) {
            return Err("Checking that the name is free took over 20 seconds, so nothing was changed.".into());
        }
        if beans::ignored(&path) {
            continue;
        }
        if !path
            .strip_prefix(&root)
            .is_some_and(|tail| tail.starts_with('/'))
        {
            return Err(Error::Stale);
        }
        if host::project_of(&path)
            .and_then(|project| project.root().ok())
            .as_deref()
            != Some(root.as_str())
        {
            continue;
        }
        let source = project.read_file(&path)?;
        bytes = bytes.saturating_add(source.len());
        if source.len() > 512 * 1024 || bytes > 8 * 1024 * 1024 {
            return Err("The project sources are too large to check that the name is free (over 512 KiB in one file or 8 MiB in total).".into());
        }
        let mut extra_names=Vec::new();
        if path.ends_with(".java") || path.ends_with(".kt") {
            if path.ends_with(".java") && source.contains("\\u")
                || path.ends_with(".kt") && spring_text::java::tokens(&source).iter().any(|t|matches!(t.s,"as"|"typealias")) {
                return Err("Unresolved source preprocessing/aliases prevent a literal collision check.".into());
            }
            let file = spring_text::parse_source(&path, &source);
            for annotation in file.types.iter().flat_map(|t| {
                t.annotations
                    .iter()
                    .chain(t.members.iter().flat_map(|m| m.annotations.iter()))
            }) {
                if annotation.name == "Bean"
                    || spring_text::java::STEREOTYPES.contains(&annotation.name.as_str())
                {
                    for arg in annotation
                        .args
                        .iter()
                        .filter(|a| matches!(a.name.as_str(), "name" | "value"))
                    {
                        if spring_text::java::tokens(&arg.text).iter().any(|t| {
                            t.string().is_none() && !matches!(t.s, "{" | "}" | "[" | "]" | ",")
                        }) {
                            return Err("Dynamic/compound source bean names prevent a literal collision check.".into());
                        }
                    }
                }
            }
            if include_types {
                let tokens = spring_text::java::tokens(&source);
                for pair in tokens.windows(2).filter(|p| {
                    matches!(
                        p[0].s,
                        "class" | "interface" | "record" | "object" | "typealias"
                    ) && p[1].is_ident()
                }) {
                    let item = (file.package.clone(), pair[1].s.to_string());
                    if types.insert(item.clone()) {
                        type_bytes = type_bytes.saturating_add(item.0.len() + item.1.len());
                    }
                    if types.len() > 8192 || type_bytes > 1024 * 1024 {
                        return Err("The project has too many types to check that the name is free (over 8,192).".into());
                    }
                }
            }
            if source.contains("IntegrationFlow") || source.contains("MessageChannels") {
                let calls=spring_text::source_calls(&path,&source);
                let in_channel=|at:usize|calls.iter().any(|call|call.name=="channel" && call.args.first().is_some_and(|arg|arg.at<=at && at<arg.end));
                for call in &calls {
                    let factory=CHANNEL_FACTORIES.contains(&call.name.as_str()) && (call.receiver.as_deref()==Some("MessageChannels") || in_channel(call.at));
                    if call.name!="channel" && !factory {continue;}
                    let Some(argument)=call.args.first() else{continue;};
                    if let Some(value)=&argument.string {
                        if value.value.contains('\\') || path.ends_with(".kt") && value.value.contains('$') || beans::is_expression(&value.value) {return Err("Escaped/interpolated DSL names prevent a literal collision check.".into());}
                        extra_names.push(Ref{name:value.value.clone(),role:Role::BeanCandidate,what:"Possible DSL channel name".into(),range:range(&source,value.at,value.end),checked:false,owner:0});
                    } else if call.name=="channel" && calls.iter().any(|nested|argument.at<=nested.at && nested.at<argument.end && CHANNEL_FACTORIES.contains(&nested.name.as_str()) && (argument.text.contains("->") || nested.end==argument.end && source.get(argument.at..nested.at).is_some_and(|prefix|spring_text::java::tokens(prefix).iter().all(|token|token.s=="." || token.is_ident())))) {
                        // Keep all nested literal factory names, even when the
                        // read-only model cannot prove the lambda receiver type.
                    } else {return Err("Dynamic/unknown DSL channel names prevent a literal collision check.".into());}
                }
            }
        }
        let mut references=parse(&path,&source);
        references.extend(extra_names);
        if path.ends_with(".xml") {
            let elements=xml::elements(&source);
            if source.contains("www.springframework.org/schema/") || elements.first().is_some_and(|root|root.local()=="beans") {
                if !spring_text::xml::generation_valid(&source) {return Err("Unsupported XML envelope/entities prevent a complete literal collision check.".into());}
                // Collision safety intentionally overincludes raw candidate IDs
                // even when the read-only source model cannot prove ownership.
                references.extend(xml::bean_ids(&elements).into_iter().map(|attribute|Ref{name:attribute.value.clone(),role:Role::BeanCandidate,what:"XML collision candidate".into(),range:range(&source,attribute.value_at,attribute.value_end),checked:false,owner:0}));
            }
        }
        for reference in references
            .into_iter()
            .filter(|reference| matches!(reference.role, Role::Declares | Role::Bean | Role::BeanCandidate))
        {
            if reference.name.contains('\\') || path.ends_with(".kt") && reference.name.contains('$') {
                return Err("Escaped/interpolated source bean names prevent a literal collision check.".into());
            }
            if beans::is_expression(&reference.name) {
                return Err("Dynamic source bean names prevent a literal collision check.".into());
            }
            let decoded = quick_xml::escape::unescape(&reference.name)
                .map_err(|_| Error::Invalid("Undecodable source bean name.".into()))?;
            for name in decoded
                .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
                .filter(|name| !name.is_empty())
            {
                if names.insert(name.to_string()) {
                    name_bytes = name_bytes.saturating_add(name.len());
                }
                if names.len() > 8192 || name_bytes > 1024 * 1024 {
                    return Err("The project has too many bean names to check that the name is free (over 8,192).".into());
                }
            }
        }
    }
    Ok(Inventory { ids: names, types })
}

/// A bridge is offered only for source-written, self-closing bare channels under
/// this same root. Their source form is DirectChannel; no poller/type inference.
fn bridge_channels(doc: &Document) -> Vec<String> {
    let elements = xml::elements(&doc.text);
    let mut counts = BTreeMap::<String, usize>::new();
    for (i, e) in elements.iter().enumerate() {
        for a in &e.attrs {
            if a.name == "id"
                || a.name == "alias"
                || a.name == "name" && xml::namespace(&elements, i) == Some(BEANS_NS)
            {
                for value in a
                    .value
                    .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';'))
                {
                    if !value.is_empty() {
                        *counts.entry(value.into()).or_default() += 1;
                    }
                }
            }
        }
    }
    let mut channels = Vec::new();
    for (i, e) in elements.iter().enumerate() {
        if e.parent == Some(0)
            && e.local() == "channel"
            && xml::namespace(&elements, i) == Some(NAMESPACE)
            && e.end == e.tag_end
            && e.attrs.len() == 1
            && e.attrs[0].name == "id"
            && name(&e.attrs[0].value)
            && counts.get(&e.attrs[0].value) == Some(&1)
        {
            channels.push(e.attrs[0].value.clone());
        }
    }
    if channels.len() > 64 {
        return Vec::new();
    }
    channels.sort();
    channels
}
fn resolve_bridge(pending: &Pending, doc: &Document, target: &Target) -> Result<FileAction, Error> {
    let channels = bridge_channels(doc);
    if channels.len() < 2 {
        return Err(Error::Stale);
    }
    let items: Vec<_> = channels
        .iter()
        .map(|id| openidea_plugin::PickItem::new(id, id))
        .collect();
    let Some(input) = host::pick(
        "Bridge input: local bare DirectChannel (adds a consumer)",
        &items,
    )?
    else {
        return Err(Error::Cancelled);
    };
    if !channels.contains(&input) {
        return Err(Error::Stale);
    }
    current(pending, doc)?;
    let outputs: Vec<_> = channels
        .iter()
        .filter(|id| **id != input)
        .map(|id| openidea_plugin::PickItem::new(id, id))
        .collect();
    let Some(output) = host::pick("Bridge output: another local bare DirectChannel", &outputs)?
    else {
        return Err(Error::Cancelled);
    };
    if input == output || !channels.contains(&output) {
        return Err(Error::Stale);
    }
    current(pending, doc)?;
    let elements = xml::elements(&doc.text);
    if elements.iter().enumerate().any(|(i, e)| {
        e.parent == Some(0)
            && e.local() == "bridge"
            && xml::namespace(&elements, i) == Some(NAMESPACE)
            && e.value("input-channel") == Some(&input)
            && e.value("output-channel") == Some(&output)
    }) {
        return Err(
            "This source already contains that explicit bridge; no duplicate is inserted.".into(),
        );
    }
    let leading = if target.at > 0 && !doc.text[..target.at].ends_with('\n') {
        target.eol
    } else {
        ""
    };
    let inserted = format!(
        "{leading}{}<{}:bridge input-channel=\"{input}\" output-channel=\"{output}\"/>{}",
        target.indent, target.prefix, target.eol
    );
    let mut after = doc.text.clone();
    after.insert_str(target.at, &inserted);
    let preview = format!(
        "Source: {}\nVersion: {}\nBridge: {input} -> {output}\n\nAdd one source-level bridge consuming the selected local bare channel and sending to the other. This adds a consumer; existing endpoints remain unchanged. No polling configuration, runtime context/type resolution, imported/global wiring or whole-flow loop analysis is inferred.\n\nExact resulting XML ({} bytes):\n{after}",
        doc.path,
        doc.version,
        after.len()
    );
    if !ui_kit::prompts::confirm_exact("Link local XML channels with a bridge?", &preview)? {
        return Err(Error::Cancelled);
    }
    current(pending, doc)?;
    if bridge_channels(doc) != channels {
        return Err(Error::Stale);
    }
    Ok(FileAction {
        title: format!("Bridge {input} to {output}"),
        kind: Some("refactor".into()),
        edits: vec![(
            doc.path.clone(),
            vec![TextEdit {
                range: range(&doc.text, target.at, target.at),
                text: inserted,
            }],
        )],
        ..Default::default()
    })
}
impl FileActions for Integration {
    fn file_actions(&mut self, doc: &Document, selection: Range) -> Vec<FileAction> {
        if matches!(doc.language.as_str(), "java" | "kotlin") {
            return self.source_channel_actions(doc, selection);
        }

        let Some(at) = text::offset(&doc.text, selection.start) else {
            return vec![];
        };
        if target(doc, at).is_none() {
            return vec![];
        }
        let Some(root) = host::project_of(&doc.path).and_then(|project| project.root().ok()) else {
            return vec![];
        };
        if !path(&root)
            || !doc
                .path
                .strip_prefix(&root)
                .is_some_and(|tail| tail.starts_with('/'))
        {
            return vec![];
        }
        let mut kinds = vec![(ActionKind::Channel, "Create XML message channel…")];
        if bridge_channels(doc).len() >= 2 {
            kinds.push((ActionKind::Bridge, "Link local XML channels with bridge…"));
        }
        if self.creation.pending.len() + kinds.len() > 8 {
            self.creation.pending.clear();
        }
        let mut actions = Vec::new();
        for (kind, title) in kinds {
            let Some(next) = self.creation.next.checked_add(1) else {
                break;
            };
            self.creation.next = next;
            let data = format!("integration-source:{next}:{}", doc.version);
            self.creation.pending.insert(
                data.clone(),
                Pending {
                    root: root.clone(),
                    doc: doc.clone(),
                    at,
                    kind,
                },
            );
            actions.push(FileAction {
                title: title.into(),
                kind: Some("refactor".into()),
                data: Some(data),
                ..Default::default()
            });
        }
        actions
    }
    fn resolve_file_action(
        &mut self,
        doc: &Document,
        action: FileAction,
    ) -> Result<FileAction, Error> {
        if action
            .data
            .as_deref()
            .is_some_and(|d| d.starts_with("source-channel:"))
        {
            return self.resolve_source_channel(doc, action);
        }
        let pending = self
            .creation
            .pending
            .remove(action.data.as_deref().ok_or(Error::Stale)?)
            .ok_or(Error::Stale)?;
        let project = current(&pending, doc)?;
        let target = target(doc, pending.at).ok_or(Error::Stale)?;
        if matches!(pending.kind, ActionKind::Bridge) {
            return resolve_bridge(&pending, doc, &target);
        }
        let Some(entered) = host::input("New literal XML message channel ID", None)? else {
            return Err(Error::Cancelled);
        };
        if !name(&entered) {
            return Err("Use an ASCII channel ID up to 128 bytes (letter/_ then letters/digits/_.-); default channels are reserved.".into());
        }
        current(&pending, doc)?;
        let before = names(&project, doc)?;
        current(&pending, doc)?;
        if before.contains(&entered) {
            return Err(
                "A possible source bean/channel/alias collision prevents this ID; nothing is overwritten."
                    .into(),
            );
        }
        let leading = if target.at > 0 && !doc.text[..target.at].ends_with('\n') {
            target.eol
        } else {
            ""
        };
        let inserted = format!(
            "{leading}{}<{}:channel id=\"{entered}\"/>{}",
            target.indent, target.prefix, target.eol
        );
        let edit = TextEdit {
            range: range(&doc.text, target.at, target.at),
            text: inserted,
        };
        let mut after = doc.text.clone();
        after.insert_str(target.at, &edit.text);
        let preview = format!(
            "Source: {}\nVersion: {}\nNew ID: {entered}\nExisting Integration prefix: {}\n\nCreate one explicit bare channel declaration in this source root (DirectChannel source form). Existing endpoints/references remain unchanged; no inferred runtime wiring, classpath, job or module actions. No files are created or overwritten.\n\nExact resulting XML ({} bytes):\n{after}",
            doc.path,
            doc.version,
            target.prefix,
            after.len()
        );
        if !ui_kit::prompts::confirm_exact("Create XML message channel?", &preview)? {
            return Err(Error::Cancelled);
        }
        current(&pending, doc)?;
        if names(&project, doc)? != before {
            return Err(Error::Stale);
        }
        current(&pending, doc)?;
        Ok(FileAction {
            title: format!("Create XML channel {entered}"),
            kind: Some("refactor".into()),
            edits: vec![(doc.path.clone(), vec![edit])],
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    const PATH: &str = "/project/src/channels.xml";
    fn doc(body: &str) -> Document {
        Document {
            path: PATH.into(),
            language: "xml".into(),
            version: 1,
            text: format!(
                "<beans xmlns=\"{BEANS_NS}\" xmlns:int=\"{NAMESPACE}\">\n{body}\n</beans>\n"
            ),
        }
    }
    fn action(owner: &mut impl FileActions, doc: &Document) -> FileAction {
        owner.file_actions(doc, Range::default()).remove(0)
    }
    fn resulting(doc: &Document, action: &FileAction) -> String {
        let edit = &action.edits[0].1[0];
        let at = text::offset(&doc.text, edit.range.start).unwrap();
        assert_eq!(edit.range.start, edit.range.end);
        let mut after = doc.text.clone();
        after.insert_str(at, &edit.text);
        after
    }
    #[test]
    fn registered_owner_returns_one_atomic_channel_in_the_existing_prefix() {
        let host = MockHost::install();
        let mut doc = doc("<!-- café 😀 -->");
        doc.text = doc.text.replace('\n', "\r\n");
        host.add_file(PATH, "old disk source");
        host.open_editor(PATH, "xml", &doc.text, &[]);
        let mut owner = structure::Structure::default();
        let lazy = action(&mut owner, &doc);
        assert!(lazy.operations.is_empty() && lazy.edits.is_empty());
        host.answer_next_prompt(Some(&["orders"]));
        host.answer_next_prompt(Some(&["yes"]));
        let resolved = owner.resolve_file_action(&doc, lazy).unwrap();
        assert!(resolved.operations.is_empty());
        assert_eq!(resolved.edits.len(), 1);
        assert_eq!(resolved.edits[0].0, PATH);
        let after = resulting(&doc, &resolved);
        assert!(after.contains("café 😀"));
        assert!(after.contains("  <int:channel id=\"orders\"/>\r\n</beans>"));
        assert!(!after.replace("\r\n", "").contains('\n'));
        assert_eq!(host.editor_text(), doc.text);
        assert_eq!(
            parse_xml(&after)
                .iter()
                .filter(|reference| reference.name == "orders" && reference.role == Role::Declares)
                .count(),
            1
        );
    }
    #[test]
    fn cancel_at_either_prompt_returns_no_edit_and_tokens_are_single_use() {
        let host = MockHost::install();
        let doc = doc("");
        host.add_file(PATH, &doc.text);
        let mut owner = Integration::default();
        let lazy = action(&mut owner, &doc);
        let replay = lazy.clone();
        assert!(matches!(
            owner.resolve_file_action(&doc, lazy),
            Err(Error::Cancelled)
        ));
        assert!(matches!(
            owner.resolve_file_action(&doc, replay),
            Err(Error::Stale)
        ));
        let lazy = action(&mut owner, &doc);
        host.answer_next_prompt(Some(&["orders"]));
        assert!(matches!(
            owner.resolve_file_action(&doc, lazy),
            Err(Error::Cancelled)
        ));
        assert!(
            owner
                .resolve_file_action(
                    &doc,
                    FileAction {
                        data: Some("forged".into()),
                        ..Default::default()
                    }
                )
                .is_err()
        );
    }
    #[test]
    fn source_version_content_and_project_changes_are_stale() {
        let host = MockHost::install();
        let doc = doc("");
        host.add_file(PATH, &doc.text);
        let mut owner = Integration::default();
        let lazy = action(&mut owner, &doc);
        let mut changed = doc.clone();
        changed.version += 1;
        assert!(matches!(
            owner.resolve_file_action(&changed, lazy),
            Err(Error::Stale)
        ));
        let lazy = action(&mut owner, &doc);
        host.add_file(PATH, "changed");
        assert!(matches!(
            owner.resolve_file_action(&doc, lazy),
            Err(Error::Stale)
        ));
        host.add_file(PATH, &doc.text);
        let lazy = action(&mut owner, &doc);
        host.set_project_root(Some("/other"));
        assert!(owner.resolve_file_action(&doc, lazy).is_err());
    }
    #[test]
    fn literal_name_and_local_project_alias_collisions_are_refused() {
        let host = MockHost::install();
        let doc = doc("<bean id=\"existing\" class=\"x.Bean\" name=\"one, two; orders\"/>");
        host.add_file(PATH, &doc.text);
        let mut owner = Integration::default();
        for name in [
            "orders",
            "existing",
            "nullChannel",
            "errorChannel",
            "../escape",
            "a:b",
            "${dynamic}",
            "with space",
            "",
            "a\"b",
        ] {
            let lazy = action(&mut owner, &doc);
            host.answer_next_prompt(Some(&[name]));
            assert!(owner.resolve_file_action(&doc, lazy).is_err(), "{name}");
        }
        assert!(!name(&"a".repeat(129)));
        let empty = self::doc("");
        host.add_file(PATH, &empty.text);
        host.add_file(
            "src/alias.xml",
            &format!(
                "<beans xmlns=\"{BEANS_NS}\"><alias name=\"other\" alias=\"ord&#101;rs\"/></beans>"
            ),
        );
        let lazy = action(&mut owner, &empty);
        host.answer_next_prompt(Some(&["orders"]));
        assert!(owner.resolve_file_action(&empty, lazy).is_err());
    }
    #[test]
    fn malformed_namespace_profile_and_large_sources_are_quiet() {
        let _host = MockHost::install();
        let good = doc("");
        assert!(target(&good, 0).is_some());
        for text in [
            good.text.replace(
                NAMESPACE,
                "http://www.springframework.org/schema/integration/http",
            ),
            good.text.replace("xmlns:int", "xmlns:xml"),
            good.text.replace("</beans>", ""),
            good.text.replace("</beans>", "</beans><other/>"),
            doc("<beans profile=\"dev\"/>").text,
            doc("<int:bridge xmlns:int=\"foreign\"/>").text,
            doc("<bad:bridge/>").text,
            doc("<bean id=\"a\" id=\"b\"/>").text,
            doc("<int:bridge channel=\"a&amp;b\"/>").text,
            format!("<!DOCTYPE beans SYSTEM \"x\">{}", good.text),
            doc(&format!("<!-- {} -->", "x".repeat(MAX_TEXT))).text,
        ] {
            assert!(
                target(
                    &Document {
                        text,
                        ..good.clone()
                    },
                    0
                )
                .is_none()
            );
        }
        let ignored = Document {
            path: "/project/target/channels.xml".into(),
            ..good.clone()
        };
        assert!(target(&ignored, 0).is_none());
        let long = Document {
            path: format!("/project/{}.xml", "a".repeat(513)),
            ..good
        };
        assert!(target(&long, 0).is_none());
    }
    #[test]
    fn source_inventory_caps_dynamic_names_and_oversized_files_refuse_creation() {
        let host = MockHost::install();
        let doc = doc("");
        host.add_file(PATH, &doc.text);
        let mut owner = Integration::default();
        host.add_file(
            "src/dynamic.xml",
            &format!(
                "<beans xmlns=\"{BEANS_NS}\"><bean id=\"${{dynamic}}\" class=\"x.B\"/></beans>"
            ),
        );
        let lazy = action(&mut owner, &doc);
        host.answer_next_prompt(Some(&["orders"]));
        assert!(owner.resolve_file_action(&doc, lazy).is_err());
        host.add_file("src/dynamic.xml", &"x".repeat(512 * 1024 + 1));
        let lazy = action(&mut owner, &doc);
        host.answer_next_prompt(Some(&["orders"]));
        assert!(owner.resolve_file_action(&doc, lazy).is_err());
        host.add_file("src/dynamic.xml", "");
        for n in 0..10_000 {
            host.add_file(&format!("src/C{n}.java"), "");
        }
        let lazy = action(&mut owner, &doc);
        host.answer_next_prompt(Some(&["orders"]));
        assert!(owner.resolve_file_action(&doc, lazy).is_err());
    }
    #[test]
    fn creation_keeps_uncertain_bean_collisions_and_refuses_hidden_source_names() {
        for (path,source) in [
            ("Other.java","package other; import fake.Bean; class Other { @Bean String helper(){return null;} }"),
            ("Other.java","package other; import org.springframework.context.annotation.Bean; class Other { @Bean(name=\"\\u0068elper\") String build(){return null;} }"),
            ("Other.kt","package other\nimport org.springframework.context.annotation.Bean as Factory\nclass Other { @Factory(\"helper\") fun build(): String = \"value\" }"),
        ] {
            let host=MockHost::install();let document=doc("");host.add_file(PATH,&document.text);host.add_file(path,source);
            let mut owner=Integration::default();let lazy=action(&mut owner,&document);
            host.answer_next_prompt(Some(&["helper"]));host.answer_next_prompt(Some(&[]));
            assert!(owner.resolve_file_action(&document,lazy).is_err(),"unproved names cannot authorize helper creation: {source}");
            assert_eq!(host.invocation().project().unwrap().read_file(PATH).unwrap(),document.text);
            assert!(host.process_runs().is_empty());
        }
    }
    #[test]
    fn creation_does_not_treat_withheld_or_legacy_xml_bean_names_as_empty() {
        for source in ["<beans xmlns='http://www.springframework.org/schema/beans'><bean id='h&#101;lper'/></beans>","<beans><bean id='helper'/></beans>"] {
            let host=MockHost::install();let document=doc("");host.add_file(PATH,&document.text);host.add_file("src/other.xml",source);
            let mut owner=Integration::default();let lazy=action(&mut owner,&document);host.answer_next_prompt(Some(&["helper"]));host.answer_next_prompt(Some(&[]));
            assert!(owner.resolve_file_action(&document,lazy).is_err(),"a withheld/legacy source cannot prove helper available: {source}");
            assert_eq!(host.invocation().project().unwrap().read_file(PATH).unwrap(),document.text);
        }
    }
    #[test]
    fn creation_keeps_unproved_dsl_name_candidates_out_of_the_available_name_set() {
        for body in ["IntegrationFlow.from(\"orders\").channel(c -> c.queue(\"audit\")).get()","IntegrationFlow.from(\"orders\").handle(message -> {}).channel(\"audit\").get()","IntegrationFlow.from(\"orders\").channel(CHANNEL_NAME).get()"] {
            let host=MockHost::install();let document=doc("");host.add_file(PATH,&document.text);
            host.add_file("src/Flow.java",&format!("package app; import org.springframework.context.annotation.Configuration; import org.springframework.context.annotation.Bean; import org.springframework.integration.dsl.IntegrationFlow; @Configuration class FlowConfig {{ @Bean IntegrationFlow flow(){{return {body};}} }}"));
            let mut owner=Integration::default();let lazy=action(&mut owner,&document);host.answer_next_prompt(Some(&["audit"]));host.answer_next_prompt(Some(&[]));
            assert!(owner.resolve_file_action(&document,lazy).is_err(),"unproved DSL names cannot authorize audit creation: {body}");
            assert_eq!(host.invocation().project().unwrap().read_file(PATH).unwrap(),document.text);
        }
        for body in ["IntegrationFlow.from(\"orders\").channel(c -> c.queue(\"audit\")).get()","IntegrationFlow.from(\"orders\").channel(MessageChannels.queue(\"audit\")).get()"] {
            let host=MockHost::install();let document=doc("");host.add_file(PATH,&document.text);
            host.add_file("src/Flow.java",&format!("package app; import org.springframework.context.annotation.Bean; import org.springframework.integration.dsl.IntegrationFlow; import org.springframework.integration.dsl.MessageChannels; class FlowConfig {{ @Bean IntegrationFlow flow(){{return {body};}} }}"));
            let mut owner=Integration::default();let lazy=action(&mut owner,&document);host.answer_next_prompt(Some(&["fresh"]));host.answer_next_prompt(Some(&[]));
            let resolved=owner.resolve_file_action(&document,lazy).unwrap();assert!(resulting(&document,&resolved).contains("id=\"fresh\""));
        }
    }
    #[test]
    fn no_permissions_and_non_root_caret_do_not_offer_source_actions() {
        let host = MockHost::install();
        let doc = doc("<int:bridge input-channel=\"in\" output-channel=\"out\"/>");
        let at = text::position(&doc.text, doc.text.find("input-channel").unwrap());
        let mut owner = Integration::default();
        assert!(owner.file_actions(&doc, Range::new(at, at)).is_empty());
        let _denied = host.with_capabilities(&[]);
        assert!(owner.file_actions(&doc, Range::default()).is_empty());
    }
    #[test]
    fn cumulative_source_budget_refuses_an_incomplete_collision_proof() {
        let host = MockHost::install();
        let doc = doc("");
        host.add_file(PATH, &doc.text);
        for n in 0..17 {
            host.add_file(
                &format!("src/Comment{n}.java"),
                &format!("/*{}*/", "x".repeat(512 * 1024 - 4)),
            );
        }
        assert!(
            matches!(names(&host.invocation().project().unwrap(),&doc),Err(Error::Failed(message)) if message.contains("too large to check that the name is free"))
        );
    }
    #[test]
    fn reserved_uri_alias_refuses_channel_generation_without_changing_legal_xml_prefix() {
        let _host = MockHost::install();
        let mut owner = Integration::default();
        let d = doc("");
        let bad = Document {
            text: d.text.replace(
                "<beans ",
                "<beans xmlns:q='http://www.w3.org/XML/1998/namespace' ",
            ),
            ..d.clone()
        };
        assert!(owner.file_actions(&bad, Range::default()).is_empty());
        let legal = Document {
            text: d.text.replace(
                "<beans ",
                "<beans xmlns:xml='http://www.w3.org/XML/1998/namespace' xml:lang='en' ",
            ),
            ..d
        };
        assert_eq!(owner.file_actions(&legal, Range::default()).len(), 1);
    }
    fn bridge_action(owner: &mut impl FileActions, doc: &Document) -> FileAction {
        owner
            .file_actions(doc, Range::default())
            .into_iter()
            .find(|a| a.title.starts_with("Link local"))
            .expect("bridge action")
    }
    #[test]
    fn wrapper_bridge_resolves_two_known_local_channels_to_one_exact_atomic_edit() {
        let host = MockHost::install();
        let mut d = doc("<!-- 😀 café -->\n<int:channel id=\"in\"/>\n<int:channel id=\"out\"/>");
        d.text = d.text.replace('\n', "\r\n");
        host.add_file(PATH, "saved old text");
        host.open_editor(PATH, "xml", &d.text, &[]);
        let mut owner = structure::Structure::default();
        let lazy = bridge_action(&mut owner, &d);
        assert!(lazy.edits.is_empty() && lazy.operations.is_empty());
        host.answer_next_prompt(Some(&["in"]));
        host.answer_next_prompt(Some(&["out"]));
        host.answer_next_prompt(Some(&["yes"]));
        let resolved = owner.resolve_file_action(&d, lazy).unwrap();
        assert!(resolved.operations.is_empty());
        assert_eq!(resolved.edits.len(), 1);
        let after = resulting(&d, &resolved);
        assert!(
            after.contains(
                "  <int:bridge input-channel=\"in\" output-channel=\"out\"/>\r\n</beans>"
            )
        );
        assert!(after.contains("😀 café"));
        assert!(!after.replace("\r\n", "").contains('\n'));
        let references = parse_xml(&after);
        assert!(
            references
                .iter()
                .any(|r| r.name == "in" && r.role == Role::Consumes)
        );
        assert!(
            references
                .iter()
                .any(|r| r.name == "out" && r.role == Role::Produces)
        );
        assert_eq!(host.editor_text(), d.text);
        assert!(host.requests().is_empty() && host.process_runs().is_empty());
    }
    #[test]
    fn bridge_cancel_at_each_prompt_is_quiet_atomic_and_single_use() {
        for accepted in 0..3 {
            let host = MockHost::install();
            let d = doc("<int:channel id=\"in\"/><int:channel id=\"out\"/>");
            host.add_file(PATH, &d.text);
            let mut owner = Integration::default();
            let lazy = bridge_action(&mut owner, &d);
            let replay = lazy.clone();
            if accepted >= 1 {
                host.answer_next_prompt(Some(&["in"]));
            }
            if accepted >= 2 {
                host.answer_next_prompt(Some(&["out"]));
            }
            assert!(matches!(
                owner.resolve_file_action(&d, lazy),
                Err(Error::Cancelled)
            ));
            assert!(matches!(
                owner.resolve_file_action(&d, replay),
                Err(Error::Stale)
            ));
            assert_eq!(
                host.invocation()
                    .project()
                    .unwrap()
                    .read_file(PATH)
                    .unwrap(),
                d.text
            );
        }
    }
    #[test]
    fn bridge_candidates_exclude_duplicates_aliases_queues_foreign_and_unknown_shapes() {
        let _host = MockHost::install();
        let mut owner = Integration::default();
        for body in [
            "<int:channel id='a'/><int:channel id='a'/>",
            "<int:channel id='a'/><int:channel id='b'><int:queue/></int:channel>",
            "<int:channel id='a'/><int:publish-subscribe-channel id='b'/>",
            "<int:channel id='a'/><int:channel id='b' datatype='x.X'/>",
            "<int:channel id='a'/><int:channel id='b'/><bean name='b' class='x.B'/>",
            "<int:channel id='a'/><int:channel id='b'/><alias name='elsewhere' alias='b'/>",
            "<int:channel id='a'/><int:channel id='${b}'/>",
        ] {
            let d = doc(body);
            assert!(
                !owner
                    .file_actions(&d, Range::default())
                    .iter()
                    .any(|a| a.title.starts_with("Link local")),
                "{body}"
            );
        }
        let d = doc("<int:channel id='a'/><int:channel id='b'/>");
        let foreign = Document {
            text: d.text.replace(NAMESPACE, "urn:foreign"),
            ..d.clone()
        };
        assert!(owner.file_actions(&foreign, Range::default()).is_empty());
        let capped = doc(&(0..65)
            .map(|i| format!("<int:channel id='c{i}'/>"))
            .collect::<String>());
        assert!(
            !owner
                .file_actions(&capped, Range::default())
                .iter()
                .any(|a| a.title.starts_with("Link local"))
        );
    }
    #[test]
    fn bridge_stale_source_owner_and_duplicate_pair_refuse_all_edits() {
        let host = MockHost::install();
        let d = doc("<int:channel id='in'/><int:channel id='out'/>");
        host.add_file(PATH, &d.text);
        let mut owner = Integration::default();
        let lazy = bridge_action(&mut owner, &d);
        let changed = Document {
            version: d.version + 1,
            ..d.clone()
        };
        assert!(matches!(
            owner.resolve_file_action(&changed, lazy),
            Err(Error::Stale)
        ));
        let lazy = bridge_action(&mut owner, &d);
        host.add_file(PATH, "changed");
        assert!(matches!(
            owner.resolve_file_action(&d, lazy),
            Err(Error::Stale)
        ));
        host.add_file(PATH, &d.text);
        let lazy = bridge_action(&mut owner, &d);
        host.set_project_root(Some("/other"));
        assert!(owner.resolve_file_action(&d, lazy).is_err());
        host.set_project_root(Some("/project"));
        let existing = doc(
            "<int:channel id='in'/><int:channel id='out'/><int:bridge input-channel='in' output-channel='out'/>",
        );
        host.add_file(PATH, &existing.text);
        let lazy = bridge_action(&mut owner, &existing);
        host.answer_next_prompt(Some(&["in"]));
        host.answer_next_prompt(Some(&["out"]));
        assert!(
            matches!(owner.resolve_file_action(&existing,lazy),Err(Error::Failed(message)) if message.contains("already contains"))
        );
    }
}
