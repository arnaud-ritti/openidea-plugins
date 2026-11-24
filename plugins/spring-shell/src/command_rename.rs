//! Explicit reviewed source operation. No automatic host string Rename graph.
use super::*;
use openidea_plugin::{TextEdit, file_actions::FileAction, host};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::BuildHasher;
#[derive(Clone, PartialEq, Eq)]
struct CommandName {
    class: String,
    name: String,
    definition: Range,
    insertion: Option<(Range, String, String)>,
    uses: Vec<Range>,
    names: BTreeSet<String>,
}
#[derive(Clone)]
struct Pending {
    root: String,
    doc: Document,
    trigger: Option<Document>,
    command: CommandName,
}
#[derive(Default)]
pub(super) struct State {
    pending: BTreeMap<String, Pending>,
}
fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.split(' ').all(|word| {
            !word.is_empty()
                && word.as_bytes()[0].is_ascii_lowercase()
                && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}
fn method_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b == b'$')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
}
fn strings<'a>(doc: &Document, a: &'a Annotation, field: &str) -> Option<Vec<&'a Str>> {
    let arg = a.arg(field)?;
    let tokens = java::tokens(&arg.text);
    if !hover::literal_values(arg)
        || arg.strings.is_empty()
        || tokens
            .iter()
            .any(|t| t.s.starts_with("\"\"\"") || t.s.starts_with('\''))
    {
        return None;
    }
    arg.strings
        .iter()
        .all(|s| {
            s.at < s.end
                && doc.text.get(s.at..s.end) == Some(s.value.as_str())
                && (name(&s.value) || method_name(&s.value) || s.value == "*")
        })
        .then(|| arg.strings.iter().collect())
}
// The shared lexer intentionally tolerates editing fragments. This operation
// requires all comments/quoted units closed before treating the class as complete.
fn closed_java(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"//") {
            at += bytes[at..]
                .iter()
                .position(|b| *b == b'\n')
                .unwrap_or(bytes.len() - at);
            continue;
        }
        if bytes[at..].starts_with(b"/*") {
            let Some(end) = bytes[at + 2..].windows(2).position(|s| s == b"*/") else {
                return false;
            };
            at += end + 4;
            continue;
        }
        if bytes[at..].starts_with(b"\"\"\"") {
            return false;
        }
        if matches!(bytes[at], b'\'' | b'\"') {
            let quote = bytes[at];
            at += 1;
            let mut closed = false;
            while at < bytes.len() {
                if bytes[at] == quote {
                    at += 1;
                    closed = true;
                    break;
                }
                if matches!(bytes[at], b'\n' | b'\r') {
                    return false;
                }
                at += if bytes[at] == b'\\' { 2 } else { 1 };
            }
            if !closed {
                return false;
            }
        } else {
            at += 1;
        }
    }
    true
}
fn key_insertion(doc:&Document,a:&Annotation,field:&str)->Option<(Range,String,String)> {
    let name_end=a.name_at+a.name.len();
    let tail=doc.text.get(name_end..a.end)?;
    let tokens=java::tokens(tail);
    if a.args.is_empty() {
        return Some((range(&doc.text,name_end,a.end),format!("({field}=\""),"\")".into()));
    }
    let open=tokens.first().filter(|t|t.s=="(")?.at+name_end+1;
    let named=tokens.get(1).is_some_and(|t|t.is_ident()) && tokens.get(2).is_some_and(|t|t.s=="=");
    Some((range(&doc.text,open,open),format!("{field}=\""),if named {"\", ".into()} else {"\", value=".into()}))
}
fn model(doc: &Document) -> Vec<CommandName> {
    let legacy=model_source(doc, false);
    if legacy.is_empty() {modern_model(doc,false)} else {legacy}
}
fn model_source(doc: &Document, _observe_implicit: bool) -> Vec<CommandName> {
    if !((doc.language == "java" && doc.path.ends_with(".java"))
        || (doc.language == "kotlin" && doc.path.ends_with(".kt")))
        || doc.text.len() > 64 << 10
        || !closed_java(&doc.text)
    {
        return vec![];
    }
    let Some(f) = document_model(doc) else {
        return vec![];
    };
    let tokens = java::tokens(&doc.text);
    if tokens.len() > 8192
        || tokens
            .iter()
            .any(|t| t.s == "<" || t.s == "new" || t.s == "\\")
        || doc.text.contains("\\u")
        || f.types.len() != 1
        || f.types.iter().any(|t| t.outer.is_some())
        || f.imports.iter().any(|i| {
            i.path.ends_with(".*") || i.is_static || i.path.rsplit('.').next() == Some("org")
        })
    {
        return vec![];
    }
    let mut out = Vec::new();
    for (ti, t) in f.types.iter().enumerate() {
        if t.kind != "class"
            || t.supers().next().is_some()
            || t.annotations.len() != 1
            || t.annotations[0].name != "ShellComponent"
            || !t.annotations[0].args.is_empty()
            || !rules::supported(&f, &doc.text, &t.annotations[0])
            || f.types.iter().filter(|o| o.name == t.name).count() != 1
        {
            continue;
        }
        let methods: Vec<_> = t.methods().collect();
        if methods.len() > 64
            || methods
                .iter()
                .any(|m| methods.iter().filter(|o| o.name == m.name).count() != 1)
        {
            continue;
        }
        let annotations: BTreeSet<_> = t
            .annotations
            .iter()
            .chain(t.members.iter().flat_map(|m| {
                m.annotations
                    .iter()
                    .chain(m.params.iter().flat_map(|p| &p.annotations))
            }))
            .map(|a| a.at)
            .collect();
        if tokens.iter().any(|tok| {
            t.start <= tok.at && tok.at < t.end && tok.s == "@" && !annotations.contains(&tok.at)
        }) {
            continue;
        }
        let mut defs = BTreeMap::new();
        let mut refs = Vec::new();
        let mut valid = true;
        for m in &methods {
            if m.annotations.iter().any(|a| {
                m.annotations.iter().filter(|b| b.name == a.name).count() != 1
                    || a.args
                        .iter()
                        .any(|v| a.args.iter().filter(|o| o.name == v.name).count() != 1)
            }) {
                valid = false;
                break;
            }
            if m.annotations.iter().any(|a| {
                !matches!(a.name.as_str(), "ShellMethod" | "ShellMethodAvailability")
                    || !rules::supported(&f, &doc.text, a)
            }) {
                valid = false;
                break;
            }
            if let Some(a) = m.annotation("ShellMethod") {
                if m.annotations
                    .iter()
                    .filter(|a| a.name == "ShellMethod")
                    .count()
                    != 1
                {
                    valid = false;
                    break;
                }
                if a.arg("key").is_none() && m.name.is_ascii() {
                    let key = kebab(&m.name);
                    let Some(insertion)=key_insertion(doc,a,"key") else {valid=false;break;};
                    if !name(&key) || defs.insert(key, (range(&doc.text, m.at, m.at + m.name.len()),Some(insertion))).is_some() {
                        valid = false;
                    }
                } else {
                    let Some(keys) = strings(doc, a, "key")
                        .filter(|v| !v.is_empty() && v.iter().all(|s| name(&s.value)))
                    else { valid = false; break; };
                    for s in keys {
                        if defs.insert(s.value.clone(), (range(&doc.text, s.at, s.end),None)).is_some() {
                            valid = false;
                            break;
                        }
                    }
                }
                if !valid { break; }
                // This annotation role names an indicator method, never the command key.
                if let Some(a) = m.annotation("ShellMethodAvailability") {
                    let Some(values) =
                        strings(doc, a, "value").filter(|v| v.len() == 1 && v[0].value != "*")
                    else {
                        valid = false;
                        break;
                    };
                    if !methods.iter().any(|other| {
                        other.name == values[0].value
                            && other.params.is_empty()
                            && other.annotation("ShellMethod").is_none()
                    }) {
                        valid = false;
                        break;
                    }
                }
            } else if let Some(a) = m.annotation("ShellMethodAvailability") {
                if m.annotations.len() != 1 {
                    valid = false;
                    break;
                }
                let Some(values) = strings(doc, a, "value") else {
                    valid = false;
                    break;
                };
                if values.iter().any(|s| s.value == "*") {
                    if values.len() != 1 {
                        valid = false;
                        break;
                    }
                } else {
                    for s in values {
                        refs.push((s.value.clone(), range(&doc.text, s.at, s.end)));
                    }
                }
            }
        }
        if !valid || defs.is_empty() || refs.iter().any(|(n, _)| !defs.contains_key(n)) {
            continue;
        }
        let names: BTreeSet<String> = defs.keys().cloned().collect();
        for (n, (definition,insertion)) in defs {
            out.push(CommandName {
                class: f.qualified(ti),
                uses: refs
                    .iter()
                    .filter(|(key, _)| *key == n)
                    .map(|(_, r)| *r)
                    .collect(),
                name: n,
                definition,
                insertion,
                names: names.clone(),
            });
            if out.len() == 64 {
                return out;
            }
        }
    }
    out
}
// Modern explicit keys/aliases have no legacy availability-selector role.
// Offer only source-local declaration edits; no runtime/global Rename graph.
fn modern_model(doc:&Document,observations:bool)->Vec<CommandName> {
    if !matches!(doc.language.as_str(),"java"|"kotlin") || doc.text.len()>64<<10 || !closed_java(&doc.text) {return vec![];}
    let Some(f)=document_model(doc) else{return vec![];};
    if f.types.len()!=1 || java::tokens(&doc.text).iter().any(|t|matches!(t.s,"<"|"\\")) {return vec![];}
    let ty=&f.types[0];
    if ty.kind!="class" || ty.outer.is_some() || ty.supers().next().is_some() {return vec![];}
    if ty.annotations.len()!=1 || !ty.annotations.iter().all(|a|
        (rules::modern_field(&f,&doc.text,a)==Some("command") && a.arg("command").is_none() && a.arg("alias").is_none())
        || (a.name=="Component" && rules::owned(&f,&doc.text,a,"org.springframework.stereotype.Component"))) {return vec![];}
    if ty.methods().any(|m| ty.methods().filter(|other|other.name==m.name).count()!=1
        || m.annotations.iter().any(|a|rules::modern_field(&f,&doc.text,a).is_none()
            && !(a.name=="CommandAvailability"&&rules::supported(&f,&doc.text,a)))) {return vec![];}
    for method in ty.methods() {
        for a in &method.annotations {
            if let Some(field)=rules::modern_field(&f,&doc.text,a) {
                if [field,"alias"].iter().any(|field|a.arg(field).is_some() && rules::literals(a,field).is_none()) {return vec![];}
            }
        }
    }
    let commands=commands(&f,&doc.text);
    let names:BTreeSet<_>=commands.iter().map(|c|c.key.clone()).collect();
    if names.len()!=commands.len() {return vec![];}
    commands.into_iter().filter_map(|command| {
        let start=offset(&doc.text,command.anchor.start)?;let end=offset(&doc.text,command.anchor.end)?;
        let physical=doc.text.get(start..end)?;
        let explicit=start>0 && doc.text.as_bytes().get(start-1)==Some(&b'"')
            && doc.text.as_bytes().get(end)==Some(&b'"') && physical==command.key;
        let insertion=if !explicit && command.anchor==command.target {
            let method=ty.methods().find(|m|range(&doc.text,m.at,m.at+m.name.len())==command.target)?;
            let annotation=method.annotation("Command")?;
            let field=rules::modern_field(&f,&doc.text,annotation)?;
            if annotation.arg(field).is_some() {None} else {key_insertion(doc,annotation,field)}
        } else {None};
        if !name(&command.key) || (!observations && !explicit && insertion.is_none()) {return None;}
        Some(CommandName{class:command.class,name:command.key,definition:command.anchor,insertion,uses:vec![],names:names.clone()})
    }).collect()
}
fn selected(span: Range, selection: Range) -> bool {
    if selection.start == selection.end {
        span.start <= selection.start && selection.start < span.end
    } else {
        span.start < selection.end && selection.start < span.end
    }
}
fn rename_preview(
    doc: &Document,
    command: &CommandName,
    new: &str,
    edits: &[TextEdit],
    after: &str,
    inherited: &[String],
) -> String {
    let inherited = if inherited.is_empty() {
        String::new()
    } else {
        format!("\n\nAlso updates availability selectors in subclasses:\n{}", inherited.join("\n"))
    };
    let spans = edits
        .iter()
        .map(|e| format!("{}:{}", e.range.start.line + 1, e.range.start.character + 1))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Rename command \"{}\" to \"{}\" in {} (class {}).\nChanges ({}) at line:column {}{}\n\nOther files, wildcard selectors and availability method names are not changed.\n\nResulting source:\n{}",
        command.name,
        new,
        doc.path,
        command.class,
        edits.len(),
        spans,
        inherited,
        after
    )
}
impl ShellTools {
    /// Subclass selectors (`path:line:col`) naming a command this class declares. Refuses when
    /// a subclass chain, selector or command name cannot be proven literal and collision free.
    fn inherited_selectors(&mut self, doc: &Document, command: &CommandName, new: &str, project: &host::Project) -> Result<Vec<(String, String, Range)>, Error> {
        let unproven = || Error::Invalid("A subclass of this class can't be read completely, so the rename could miss commands. Rename it by hand.".into());
        // Under a task the call gets 60 s and fuel refilled by each is_cancelled: read on until the pass ends.
        let task = host::begin_task("Reading Spring Shell commands", true);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < until
            && !task.is_cancelled()
            && self.indexes.for_document(&doc.path, &doc.text, COMMAND_BUDGET).is_some_and(|index| index.scanning())
        {}
        let index = self.indexes.for_document(&doc.path, &doc.text, COMMAND_BUDGET).ok_or_else(unproven)?;
        if !trustworthy(index) {
            return Err(Error::Invalid("Spring Shell commands are still being read after 20 seconds, so nothing was renamed. Try again once the project has been read.".into()));
        }
        let mut subclasses = Vec::new();
        for (path, shell) in index.iter() {
            for class in shell.classes.iter().filter(|c| c.component && c.kind == "class" && c.qualified != command.class) {
                let (chain, complete) = walk(index, path, &shell.classes, class);
                if !chain.iter().any(|(_, info)| info.qualified == command.class) {
                    continue;
                }
                if !complete {
                    return Err(unproven());
                }
                subclasses.push((path.to_owned(), class.qualified.clone()));
            }
        }
        let derived = derived(index);
        let inheriting: Vec<_> = derived.inherited.iter().filter(|(_, declaring, c)| *declaring == doc.path && c.key == command.name && subclasses.iter().any(|(_, q)| *q == c.class)).collect();
        let collides = all_commands(index).map(|(_, c)| c).chain(derived.inherited.iter().map(|(_, _, c)| c)).any(|c| c.key == new && subclasses.iter().any(|(_, q)| *q == c.class));
        if collides {
            return Err(Error::Invalid("A subclass already has a command with that key.".into()));
        }
        let mut out = Vec::new();
        for (owner, _, inherited) in inheriting.iter().copied() {
            let sub = Document {
                path: owner.clone(),
                language: if owner.ends_with(".kt") { "kotlin" } else { "java" }.into(),
                version: 0,
                text: project.read_file(&owner)?,
            };
            let f = document_model(&sub).ok_or_else(unproven)?;
            for a in annotations(&f).filter(|a| a.name == "ShellMethodAvailability") {
                let Some(arg) = a.arg("value") else { continue };
                if availability_owner(&f, &sub.text, a).is_some() || class_at(&f, a.at) != inherited.class {
                    continue;
                }
                if !hover::literal_values(arg) {
                    return Err(unproven());
                }
                for s in arg.strings.iter().filter(|s| words(&[s]) == command.name) {
                    if s.value != command.name {
                        return Err(unproven());
                    }
                    out.push((owner.clone(), sub.text.clone(), range(&sub.text, s.at, s.end)));
                }
            }
        }
        Ok(out)
    }

    fn inherited_origin(&mut self, doc: &Document, selection: Range) -> Option<(Document, CommandName)> {
        let f = document_model(doc)?;
        let key = annotations(&f)
            .filter(|a| a.name == "ShellMethodAvailability" && availability_owner(&f, &doc.text, a).is_none())
            .filter_map(|a| Some((a, a.arg("value").filter(|arg| hover::literal_values(arg))?)))
            .find_map(|(a, arg)| {
                let s = arg.strings.iter().find(|s| selected(range(&doc.text, s.at, s.end), selection))?;
                Some((class_at(&f, a.at), s.value.clone()))
            })?;
        let project = host::project_of(&doc.path)?;
        let index = self.indexes.for_document(&doc.path, &doc.text, PROVIDER_BUDGET)?;
        let found = named(index, &key.0, &key.1);
        let [(path, _)] = found.as_slice() else { return None };
        if *path == doc.path {
            return None;
        }
        let declaring = Document {
            path: path.clone(),
            language: if path.ends_with(".kt") { "kotlin" } else { "java" }.into(),
            version: 0,
            text: project.read_file(path).ok()?,
        };
        let command = model(&declaring).into_iter().find(|c| c.name == key.1)?;
        Some((declaring, command))
    }

    pub(super) fn command_rename_actions(
        &mut self,
        doc: &Document,
        selection: Range,
    ) -> Vec<FileAction> {
        let Some(root) = actions::root(doc) else {
            return vec![];
        };
        let mut commands: Vec<_> = model(doc)
            .into_iter()
            .filter(|c| selected(c.definition, selection) || c.uses.iter().any(|r| selected(*r, selection)))
            .map(|c| (None, c))
            .collect();
        if commands.is_empty() {
            commands = self.inherited_origin(doc, selection).into_iter().map(|(d, c)| (Some(d), c)).collect();
        }
        let mut out = Vec::new();
        for (declaring, command) in commands.into_iter().take(4) {
            if self.command_renames.pending.len() >= 8 {
                self.command_renames.pending.clear();
            }
            let random = std::collections::hash_map::RandomState::new();
            let token = format!(
                "shell-command:{:016x}{:016x}",
                random.hash_one(0u8),
                random.hash_one(1u8)
            );
            self.command_renames.pending.insert(
                token.clone(),
                Pending {
                    root: root.clone(),
                    doc: declaring.clone().unwrap_or_else(|| doc.clone()),
                    trigger: declaring.map(|_| doc.clone()),
                    command,
                },
            );
            out.push(FileAction {
                title: "Rename command key…".into(),
                kind: Some("refactor.rename".into()),
                data: Some(token),
                ..Default::default()
            });
        }
        out
    }
    pub(super) fn resolve_command_rename(
        &mut self,
        trigger_doc: &Document,
        action: FileAction,
    ) -> Result<FileAction, Error> {
        let token = action.data.as_deref().ok_or(Error::Stale)?;
        let pending = self
            .command_renames
            .pending
            .get(token)
            .cloned()
            .ok_or(Error::Stale)?;
        let origin = pending.trigger.as_ref().unwrap_or(&pending.doc);
        let doc = &pending.doc;
        let fresh = |d: &Document| {
            d.path == origin.path
                && d.language == origin.language
                && d.version == origin.version
                && d.text == origin.text
                && actions::root(d).as_deref() == Some(&pending.root)
        };
        let project = host::project_of(&doc.path).ok_or(Error::Stale)?;
        let current = |project: &host::Project| -> Result<bool, Error> {
            Ok(project.read_file(&doc.path)? == doc.text && model(doc).contains(&pending.command))
        };
        if !fresh(trigger_doc) {
            return Err(Error::Stale);
        }
        if project.read_file(&trigger_doc.path)? != trigger_doc.text {
            return Err(Error::Invalid("Save the file first".into()));
        }
        if !current(&project)? {
            return Err(Error::Stale);
        }
        let new=host::input("New command name (lowercase words separated by spaces, up to 64 characters)",Some(&pending.command.name))?.ok_or(Error::Cancelled)?;
        if !name(&new) || new == pending.command.name || pending.command.names.contains(&new) {
            return Err(Error::Invalid("Use a new command name that no other command uses: lowercase words of letters, digits and hyphens, each starting with a letter, up to 64 characters.".into()));
        }
        let declaration=match &pending.command.insertion {
            Some((range,before,after))=>TextEdit{range:*range,text:format!("{before}{new}{after}")},
            None=>TextEdit{range:pending.command.definition,text:new.clone()},
        };
        let mut edits:Vec<_>=std::iter::once(declaration).chain(pending.command.uses.iter().map(|range|TextEdit{range:*range,text:new.clone()})).collect();
        edits.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        let mut after = doc.text.clone();
        for e in edits.iter().rev() {
            after.replace_range(
                offset(&doc.text, e.range.start).ok_or(Error::Stale)?
                    ..offset(&doc.text, e.range.end).ok_or(Error::Stale)?,
                &e.text,
            );
        }
        let selectors = self.inherited_selectors(doc, &pending.command, &new, &project)?;
        let labels: Vec<_> = selectors.iter().map(|(path, _, r)| format!("{}:{}:{}", path, r.start.line + 1, r.start.character + 1)).collect();
        let preview = rename_preview(doc, &pending.command, &new, &edits, &after, &labels);
        if !ui_kit::prompts::confirm_exact("Rename command key?", &preview)? {
            return Err(Error::Cancelled);
        }
        if !fresh(trigger_doc) || !current(&project)? || project.read_file(&trigger_doc.path)? != trigger_doc.text {
            return Err(Error::Stale);
        }
        let mut files = vec![(doc.path.clone(), edits)];
        for (path, text, span) in selectors {
            if project.read_file(&path)? != text {
                return Err(Error::Stale);
            }
            let edit = TextEdit { range: span, text: new.clone() };
            match files.iter_mut().find(|(p, _)| *p == path) {
                Some((_, list)) => list.push(edit),
                None => files.push((path, vec![edit])),
            }
        }
        self.command_renames.pending.remove(token);
        Ok(FileAction {
            title: "Rename command key".into(),
            kind: Some("refactor.rename".into()),
            edits: files,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::{file_actions::FileActions, mock::MockHost};
    const TEXT: &str = "import org.springframework.shell.standard.ShellMethod;\r\nimport org.springframework.shell.standard.ShellMethodAvailability;\r\n@org.springframework.shell.standard.ShellComponent class C {\r\n /*😀*/ @ShellMethod(key=\"run\",value=\"Run\") @ShellMethodAvailability(\"run\") void execute(){}\r\n Availability run(){return null;}\r\n @ShellMethodAvailability({\"run\"}) Availability gate(){return null;}\r\n @ShellMethodAvailability(\"*\") Availability wildcard(){return null;}\r\n @ShellMethod(key=\"other\",value=\"Other\") void other(){}\r\n}";
    fn doc(text: &str) -> Document {
        Document {
            path: "/project/C.java".into(),
            language: "java".into(),
            version: 7,
            text: text.into(),
        }
    }
    fn selection(d: &Document) -> Range {
        let at = position(&d.text, d.text.find("\"run\"").unwrap() + 1);
        Range::new(at, at)
    }
    fn pending(tools: &mut ShellTools, d: &Document) -> FileAction {
        tools
            .file_actions(d, selection(d))
            .into_iter()
            .find(|a| {
                a.data
                    .as_deref()
                    .is_some_and(|s| s.starts_with("shell-command:"))
            })
            .unwrap()
    }
    #[test]
    fn compact_review_starts_with_scope_and_lists_exact_spans_and_one_complete_result() {
        let _h = MockHost::install();
        let d = doc(TEXT);
        let command = model(&d).into_iter().find(|c| c.name == "run").unwrap();
        let from = TEXT.find("key=\"run\"").unwrap() + 5;
        let reference = TEXT.find("{\"run\"}").unwrap() + 2;
        let edits = vec![
            TextEdit {
                range: range(TEXT, from, from + 3),
                text: "launch".into(),
            },
            TextEdit {
                range: range(TEXT, reference, reference + 3),
                text: "launch".into(),
            },
        ];
        let after = TEXT.replacen("key=\"run\"", "key=\"launch\"", 1).replacen(
            "{\"run\"}",
            "{\"launch\"}",
            1,
        );
        let preview = rename_preview(&d, &command, "launch", &edits, &after, &[]);
        assert!(preview.starts_with("Rename command \"run\" to \"launch\""));
        assert!(preview.contains("Changes (2) at line:column"));
        for e in &edits {
            let exact = format!("{}:{}", e.range.start.line + 1, e.range.start.character + 1);
            assert!(preview.contains(&exact));
        }
        assert_eq!(preview.matches("Resulting source:").count(), 1);
        assert!(preview.ends_with(&after));
        assert_eq!(preview.matches(&after).count(), 1);
        assert!(!preview.contains("Complete source before:"));
        assert!(!preview.contains(TEXT));
        assert!(
            preview
                .find("Other files, wildcard selectors")
                .unwrap()
                < preview.find("Resulting source:").unwrap()
        );
    }
    #[test]
    fn registered_action_previews_atomic_exact_literals_and_preserves_other_roles_and_wildcard() {
        let h = MockHost::install();
        h.add_file("C.java", TEXT);
        let mut tools = ShellTools::default();
        let d = doc(TEXT);
        let action = pending(&mut tools, &d);
        let replay = action.clone();
        assert!(action.edits.is_empty() && action.operations.is_empty());
        h.answer_next_prompt(Some(&["launch"]));
        h.answer_next_prompt(Some(&["yes"]));
        let resolved = tools.resolve_file_action(&d, action).unwrap();
        assert!(resolved.operations.is_empty());
        assert_eq!(resolved.edits.len(), 1);
        assert_eq!(resolved.edits[0].1.len(), 2);
        let mut after = d.text.clone();
        for e in resolved.edits[0].1.iter().rev() {
            assert_eq!(
                &d.text[offset(&d.text, e.range.start).unwrap()
                    ..offset(&d.text, e.range.end).unwrap()],
                "run"
            );
            after.replace_range(
                offset(&d.text, e.range.start).unwrap()..offset(&d.text, e.range.end).unwrap(),
                &e.text,
            );
        }
        assert!(after.contains("key=\"launch\""));
        assert!(after.contains("Availability({\"launch\"})"));
        assert!(after.contains("@ShellMethodAvailability(\"run\") void execute"));
        assert!(after.contains("Availability run()"));
        assert!(after.contains("Availability(\"*\")"));
        assert!(after.contains("/*😀*/"));
        assert!(after.contains("\r\n"));
        assert_eq!(
            host::project_of(&d.path)
                .unwrap()
                .read_file(&d.path)
                .unwrap(),
            TEXT
        );
        assert!(tools.resolve_file_action(&d, replay).is_err());
        assert!(h.process_runs().is_empty());
        assert_eq!(h.fetches(), 0);
    }
    #[test]
    fn camelcase_indicator_methods_do_not_block_command_identity() {
        let _h = MockHost::install();
        let text = TEXT.replace("Availability(\"run\") void execute", "Availability(\"isReady\") void execute")
            .replace("Availability run()", "Availability isReady()");
        let commands = model(&doc(&text));
        assert_eq!(commands.len(), 2);
        let indexed = public_edges(&openidea_plugin::indexing::Document {
            path: "/project/C.java".into(), text: text.clone(), symbols: vec![],
        });
        let indicator = indexed.definitions.iter().find(|a| a.key.contains(":indicator:")).unwrap();
        assert_eq!(&text[offset(&text, indicator.range.start).unwrap()..offset(&text, indicator.range.end).unwrap()], "isReady");
        assert_eq!(indexed.references.iter().filter(|a| a.key == indicator.key).count(), 1);
        assert!(indexed.definitions.iter().any(|a| !a.key.contains(":indicator:") && a.key.ends_with(":run")));
    }
    #[test]
    fn multiword_alias_rename_updates_only_selected_alias_and_its_selectors() {
        let h = MockHost::install();
        let text = TEXT.replace("key=\"run\"", "key={\"run task\",\"rt\"}")
            .replace("Availability({\"run\"})", "Availability({\"run task\",\"rt\"})");
        h.add_file("C.java", &text);
        let d = doc(&text);
        let commands = model(&d);
        assert_eq!(commands.len(), 3);
        let command = commands.iter().find(|c| c.name == "run task").unwrap();
        assert_eq!(command.uses.len(), 1);
        let mut tools = ShellTools::default();
        let action = tools.command_rename_actions(&d, command.definition).remove(0);
        h.answer_next_prompt(Some(&["execute task"]));
        h.answer_next_prompt(Some(&["yes"]));
        let resolved = tools.resolve_file_action(&d, action).unwrap();
        assert_eq!(resolved.edits[0].1.len(), 2);
        for edit in &resolved.edits[0].1 {
            assert_eq!(&text[offset(&text, edit.range.start).unwrap()..offset(&text, edit.range.end).unwrap()], "run task");
            assert_eq!(edit.text, "execute task");
        }
        for invalid in [" run", "run ", "run  task", "run\ttask", "run 1task"] {
            assert!(!name(invalid));
        }
        assert!(h.process_runs().is_empty());
    }
    #[test]
    fn incomplete_alias_duplicate_foreign_inherited_generic_and_selector_owners_refuse() {
        let _h = MockHost::install();
        for text in [
            TEXT.replace("key=\"run\"", "key={\"run\",\"run\"}"),
            TEXT.replace("key=\"run\"", "key=PREFIX"),
            TEXT.replace("key=\"other\"", "key=\"run\""),
            TEXT.replace("Availability({\"run\"})", "Availability(KEY)"),
            TEXT.replace("Availability({\"run\"})", "Availability({\"run\",\"*\"})"),
            TEXT.replace("Availability({\"run\"})", "Availability(\"unknown\")"),
            TEXT.replace("class C", "class C extends Base"),
            TEXT.replace("class C", "class C<T>"),
            TEXT.replace("class C", "class C {} class C"),
            TEXT.replace(
                "org.springframework.shell.standard.ShellMethod;",
                "foreign.ShellMethod;",
            ),
            TEXT.replace(
                "org.springframework.shell.standard.ShellMethod;",
                "org.springframework.shell.standard.*;",
            ),
            TEXT.replace("key=\"run\"", "key=\"run\\n\""),
            TEXT.replace("Availability run(){return null;}", ""),
            TEXT.replace(
                "@ShellMethodAvailability({\"run\"})",
                "@Foreign @ShellMethodAvailability({\"run\"})",
            ),
            TEXT.replace("void execute", "<T> void execute"),
            format!("{TEXT}\nclass ShellMethod {{}}"),
            TEXT.replace("class C", "class Outer { class C") + "}",
            TEXT.replace("key=\"run\"", "key=\"run\",key=\"hidden\""),
        ] {
            assert!(model(&doc(&text)).is_empty(), "{text}");
        }
        assert!(model(&doc(&format!("import foreign.org;\n{TEXT}"))).is_empty());
        assert!(model(&doc(&TEXT[..TEXT.len() - 1])).is_empty());
        assert!(model(&doc(&format!("{TEXT} /* unfinished"))).is_empty());
    }
    #[test]
    fn validated_new_names_collision_and_cancel_never_return_edits() {
        for input in [
            "",
            "other",
            "run",
            "UPPER",
            "-start",
            "two  words",
            "quote\"",
            "back\\slash",
            "new\nline",
            "é",
            "new$key",
        ] {
            let h = MockHost::install();
            h.add_file("C.java", TEXT);
            let mut tools = ShellTools::default();
            let d = doc(TEXT);
            let a = pending(&mut tools, &d);
            h.answer_next_prompt(Some(&[input]));
            h.answer_next_prompt(Some(&["yes"]));
            assert!(tools.resolve_file_action(&d, a).is_err(), "{input:?}");
            assert_eq!(
                host::project_of(&d.path)
                    .unwrap()
                    .read_file(&d.path)
                    .unwrap(),
                TEXT
            );
        }
        for cancel_input in [true, false] {
            let h = MockHost::install();
            h.add_file("C.java", TEXT);
            let mut tools = ShellTools::default();
            let d = doc(TEXT);
            let a = pending(&mut tools, &d);
            if cancel_input {
                h.answer_next_prompt(None);
            } else {
                h.answer_next_prompt(Some(&["launch"]));
                h.answer_next_prompt(None);
            }
            assert!(matches!(
                tools.resolve_file_action(&d, a),
                Err(Error::Cancelled)
            ));
        }
    }
    #[test]
    fn source_project_path_version_class_update_and_delete_are_revalidated() {
        for mode in 0..6 {
            let h = MockHost::install();
            h.add_file("C.java", TEXT);
            let mut tools = ShellTools::default();
            let mut d = doc(TEXT);
            let a = pending(&mut tools, &d);
            match mode {
                0 => d.version += 1,
                1 => d.path = "/project/Renamed.java".into(),
                2 => d.text = d.text.replace("class C", "class Changed"),
                3 => h.set_project_root(Some("/other")),
                4 => h.add_file("C.java", "changed"),
                _ => d.text.clear(),
            }
            h.answer_next_prompt(Some(&["launch"]));
            h.answer_next_prompt(Some(&["yes"]));
            assert!(tools.resolve_file_action(&d, a).is_err());
        }
        let h = MockHost::install();
        let before = doc(TEXT);
        let changed = doc(&TEXT
            .replace("key=\"run\"", "key=\"launch\"")
            .replace("Availability({\"run\"})", "Availability({\"launch\"})"));
        assert!(model(&before).iter().any(|c| c.name == "run"));
        assert!(!model(&changed).iter().any(|c| c.name == "run"));
        assert!(model(&changed).iter().any(|c| c.name == "launch"));
        assert!(model(&doc("")).is_empty());
        h.set_project_root(None);
        assert!(
            ShellTools::default()
                .file_actions(&before, selection(&before))
                .is_empty()
        );
    }
    #[test]
    fn duplicate_class_owners_nested_projects_denied_identity_and_reclaimed_tokens_refuse() {
        let h = MockHost::install();
        h.add_file("C.java", TEXT);
        let mut tools = ShellTools::default();
        let d = doc(TEXT);
        assert!(model(&doc(&format!("{TEXT} class Other {{}}"))).is_empty());
        let old = pending(&mut tools, &d);
        let mut reclaimed = ShellTools::default();
        let fresh = pending(&mut reclaimed, &d);
        assert_ne!(old.data, fresh.data);
        assert!(reclaimed.resolve_file_action(&d, old).is_err());
        h.add_file("nested/C.java", TEXT);
        let mut nested = d.clone();
        nested.path = "/project/nested/C.java".into();
        let old = pending(&mut tools, &nested);
        h.add_project("/project/nested");
        assert!(tools.resolve_file_action(&nested, old).is_err());
        let _denied = MockHost::install().with_capabilities(&[]);
        assert!(
            ShellTools::default()
                .file_actions(&d, selection(&d))
                .is_empty()
        );
    }
    #[test]
    fn unsaved_visible_source_supported_but_oversized_complete_preview_refuses() {
        let h = MockHost::install();
        h.add_file("C.java", "different disk");
        let d = doc(TEXT);
        h.open_editor(&d.path, "java", &d.text, &[]);
        let mut tools = ShellTools::default();
        let a = pending(&mut tools, &d);
        h.answer_next_prompt(Some(&["launch"]));
        h.answer_next_prompt(Some(&["yes"]));
        assert_eq!(
            tools.resolve_file_action(&d, a).unwrap().edits[0].1.len(),
            2
        );
        let text = TEXT.replace("/*😀*/", &format!("/*{}*/", "x".repeat(18000)));
        let d = doc(&text);
        h.open_editor(&d.path, "java", &d.text, &[]);
        let a = pending(&mut tools, &d);
        h.answer_next_prompt(Some(&["launch"]));
        h.answer_next_prompt(Some(&["yes"]));
        assert!(tools.resolve_file_action(&d, a).is_err());
        assert!(!include_str!("../plugin.toml").contains("contributes.documentIndexers"));
        let registration = include_str!("lib.rs")
            .lines()
            .find(|s| s.starts_with("register_plugin!("))
            .unwrap();
        assert!(!registration.contains("indexing"));
    }
}

/// Reuse the closed single-class transaction model for read-only occurrences. A path
/// scope prevents equal command keys in unrelated ShellComponents from merging.
pub(super) fn public_edges(doc: &openidea_plugin::indexing::Document) -> openidea_plugin::indexing::Edges {
        use openidea_plugin::indexing::{Anchor, Edges};
        let source = Document { path: doc.path.clone(), text: doc.text.clone(), language: if doc.path.ends_with(".java") { "java" } else if doc.path.ends_with(".kt") { "kotlin" } else { "unknown" }.into(), version: 0 };
        let mut edges = Edges::default();
        let commands = {let legacy=model_source(&source,true);if legacy.is_empty(){modern_model(&source,true)}else{legacy}};
        if commands.is_empty() { return edges; }
        // Indicator method names are a separate role from command keys. Never
        // merge a selector merely because it spells the same word as a method.
        if let Some(file) = document_model(&source) {
            let mut indicators = BTreeMap::<String, (Range, Vec<Range>)>::new();
            for ty in &file.types {
                for method in ty.methods().filter(|m|m.annotation("ShellMethod").is_some()) {
                    let Some(annotation) = method.annotation("ShellMethodAvailability") else { continue };
                    let Some(values) = strings(&source, annotation, "value") else { continue };
                    let [value] = values.as_slice() else { continue };
                    let Some(target) = ty.methods().find(|m|m.name==value.value && m.params.is_empty() && m.annotation("ShellMethod").is_none()) else { continue };
                    indicators.entry(value.value.clone()).or_insert_with(||(range(&doc.text,target.at,target.at+target.name.len()),Vec::new())).1.push(range(&doc.text,value.at,value.end));
                }
            }
            for (name,(definition,uses)) in indicators {
                let key=format!("spring-shell:indicator:{}:{}:{name}",doc.path.len(),doc.path);
                edges.definitions.push(Anchor {key:key.clone(),range:definition});
                edges.references.extend(uses.into_iter().map(|range|Anchor {key:key.clone(),range}));
            }
        }
        for command in commands {
            let key = format!("spring-shell:{}:{}:{}:{}", doc.path.len(), doc.path, command.class, command.name);
            edges.definitions.push(Anchor { key: key.clone(), range: command.definition });
            edges.references.extend(command.uses.into_iter().map(|range| Anchor { key: key.clone(), range }));
        }
        edges
}
#[cfg(test)]
mod public_anchor_tests {
    use super::*;
    use openidea_plugin::indexing::{Document as IndexedDocument};
    fn source(path: &str, text: &str) -> IndexedDocument { IndexedDocument { path: path.into(), text: text.into(), symbols: vec![] } }
    #[test]
    fn anchors_separate_command_selectors_indicator_names_and_wildcards() {
        let text = "import org.springframework.shell.standard.ShellMethod; import org.springframework.shell.standard.ShellMethodAvailability; @org.springframework.shell.standard.ShellComponent class C { @ShellMethod(key=\"run\") @ShellMethodAvailability(\"ready\") void execute(){} Availability ready(){return null;} @ShellMethodAvailability(\"run\") Availability gate(){return null;} @ShellMethodAvailability(\"*\") Availability all(){return null;} }";
        let edges = public_edges(&source("/p/C.java", text));
        assert_eq!(edges.definitions.len(), 2);
        assert_eq!(edges.references.len(), 2);
        for a in edges.definitions.iter().chain(&edges.references) {
            let start = offset(text, a.range.start).unwrap(); let end = offset(text, a.range.end).unwrap();
            assert_eq!(&text[start..end], if a.key.contains(":indicator:") { "ready" } else { "run" });
        }
        assert_eq!(edges.definitions[0].key, edges.references[0].key);
        assert_ne!(edges.definitions[0].key, public_edges(&source("/other/C.java", text)).definitions[0].key);
        assert!(public_edges(&source("/p/C.java", &text.replace("key=\"run\"", "key=PREFIX"))).definitions.is_empty());
        assert!(public_edges(&source("/p/C.kt", text)).definitions.is_empty());
    }
}

pub(super) fn occurrences(ctx: &Invocation,at_override:Option<Position>) -> Result<Option<String>,Error> {
    let editor=ctx.editor().ok_or("Open a Java or Kotlin @ShellComponent class and put the caret on a command or availability name.")?;
    let snapshot=editor.document()?;let path=editor.path();
    let at=if let Some(at)=at_override{at}else{editor.selections()?.first().map(|r|r.start).ok_or_else(||Error::Invalid("Put the caret on a Spring Shell command key first.".into()))?};
    let project=ctx.project().ok_or(Error::Stale)?;let root=project.root()?;
    if root=="/" || !path.starts_with(&format!("{root}/")) || host::project_of(&path).and_then(|p|p.root().ok()).as_deref()!=Some(root.as_str()) {return Err(Error::Stale);}
    let edges=public_edges(&openidea_plugin::indexing::Document {path:path.clone(),text:snapshot.text.clone(),symbols:vec![]});
    let selected=edges.definitions.iter().chain(&edges.references).find(|a|a.range.start<=at && at<a.range.end).ok_or("Put the caret on a Spring Shell command key first.")?;
    let mut spans:Vec<_>=edges.definitions.iter().chain(&edges.references).filter(|a|a.key==selected.key).map(|a|a.range).collect();
    spans.sort_by_key(|r|(r.start.line,r.start.character));
    let items:Vec<_>=spans.iter().enumerate().map(|(i,r)|openidea_plugin::PickItem::new(&i.to_string(),&format!("Line {}, column {}",r.start.line+1,r.start.character+1)).description("In this file")).collect();
    let Some(choice)=host::pick("Command usages in this file",&items)? else {return Ok(None);};
    let destination=spans.get(choice.parse::<usize>().map_err(|_|Error::Stale)?).ok_or(Error::Stale)?.start;
    let current=ctx.editor().ok_or(Error::Stale)?;let fresh=current.document()?;
    if ctx.project().ok_or(Error::Stale)?.root()?!=root || host::project_of(&path).and_then(|p|p.root().ok()).as_deref()!=Some(root.as_str()) || current.path()!=path || fresh.version!=snapshot.version || fresh.text!=snapshot.text {return Err(Error::Stale);}
    host::open_location(&path,destination.line,destination.character)?;Ok(None)
}

#[cfg(test)]
mod occurrence_command_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn command_occurrences_without_selection_explains() {
        let h=MockHost::install();
        h.open_editor("/project/C.java","java","class C {}",&[]);
        let error=ShellTools::default().run_command("command-occurrences",&h.invocation(),None).unwrap_err();
        assert!(matches!(error,Error::Invalid(ref message) if message=="Put the caret on a Spring Shell command key first."),"{error:?}");
    }
    #[test]
    fn command_occurrences_with_the_caret_off_a_key_explains() {
        let h=MockHost::install();
        h.open_editor("/project/C.java","java","class C {}",&[(0,0,0,0)]);
        let error=ShellTools::default().run_command("command-occurrences",&h.invocation(),None).unwrap_err();
        assert!(error.to_string().contains("Put the caret on a Spring Shell command key first."),"{error:?}");
    }
    #[test]
    fn registered_picker_keeps_selector_and_indicator_roles_separate() {
        let h=MockHost::install();
        let source="import org.springframework.shell.standard.ShellMethod; import org.springframework.shell.standard.ShellMethodAvailability; @org.springframework.shell.standard.ShellComponent class C { @ShellMethod(key=\"run\") @ShellMethodAvailability(\"ready\") void execute(){} Availability ready(){return null;} @ShellMethodAvailability(\"run\") Availability gate(){return null;} }";
        let at=position(source,source.find("\"run\"").unwrap()+1);
        h.open_editor("/project/C.java","java",source,&[(at.line,at.character,at.line,at.character)]);
        h.answer_next_prompt(Some(&["1"]));ShellTools::default().run_command("command-occurrences",&h.invocation(),None).unwrap();
        let destination=position(source,source.rfind("\"run\"").unwrap()+1);
        assert_eq!(h.opened_locations().last(),Some(&(String::from("/project/C.java"),destination.line,destination.character)));
        h.answer_next_prompt(Some(&["999"]));assert!(ShellTools::default().run_command("command-occurrences",&h.invocation(),None).is_err());
        assert_eq!(h.opened_locations().len(),1);assert!(h.requests().is_empty() && h.process_runs().is_empty());
    }
}

#[cfg(test)]
mod kotlin_implicit_tests {
    use super::*;
    use openidea_plugin::{file_actions::FileActions, mock::MockHost};
    #[test]
    fn kotlin_explicit_aliases_use_reviewed_closed_source_transactions() {
        let h = MockHost::install();
        let text = "import org.springframework.shell.standard.ShellComponent\nimport org.springframework.shell.standard.ShellMethod\nimport org.springframework.shell.standard.ShellMethodAvailability\n@ShellComponent class C { @ShellMethod(key=[\"run task\",\"rt\"]) fun runTask() {} @ShellMethodAvailability(\"run task\") fun ready(): Availability { return Availability.available() } }";
        let d = Document { path: "/project/C.kt".into(), text: text.into(), language: "kotlin".into(), version: 8 };
        h.add_file("C.kt", text);
        let commands = model(&d);
        assert_eq!(commands.len(), 2);
        let command = commands.iter().find(|c| c.name == "run task").unwrap();
        assert_eq!(command.uses.len(), 1);
        let mut tools = ShellTools::default();
        let action = tools.command_rename_actions(&d, command.definition).remove(0);
        h.answer_next_prompt(Some(&["execute task"]));
        h.answer_next_prompt(Some(&["yes"]));
        let resolved = tools.resolve_file_action(&d, action).unwrap();
        assert_eq!(resolved.edits[0].1.len(), 2);
        assert!(resolved.edits[0].1.iter().all(|e| e.text == "execute task"));
        for replacement in ["import foreign.ShellMethod\n", "import org.springframework.shell.standard.ShellMethod as Other\n"] {
            let foreign = Document {text: text.replace("import org.springframework.shell.standard.ShellMethod\n", replacement), ..d.clone()};
            assert!(model(&foreign).is_empty());
        }
    }
    #[test]
    fn implicit_occurrences_use_method_declaration_without_exposing_method_rename() {
        let _h = MockHost::install();
        let text = "import org.springframework.shell.standard.ShellComponent; import org.springframework.shell.standard.ShellMethod; import org.springframework.shell.standard.ShellMethodAvailability; @ShellComponent class C { @ShellMethod(value=\"Run\") void runTask() {} @ShellMethodAvailability(\"run-task\") Availability ready(){return null;} }";
        let d = Document {path:"/project/C.java".into(),text:text.into(),language:"java".into(),version:1};
        assert!(model(&d)[0].insertion.is_some());
        let edges = public_edges(&openidea_plugin::indexing::Document {path:d.path,text:text.into(),symbols:vec![]});
        assert_eq!(edges.definitions.len(), 1);
        assert_eq!(edges.references.len(), 1);
        assert_eq!(edges.definitions[0].key, edges.references[0].key);
        let span = edges.definitions[0].range;
        assert_eq!(&text[offset(text,span.start).unwrap()..offset(text,span.end).unwrap()],"runTask");
    }
}

#[cfg(test)]
mod modern_transaction_tests {
    use super::*;
    use openidea_plugin::{file_actions::FileActions,mock::MockHost};
    #[test]
    fn java_core_and_kotlin_modern_aliases_offer_only_explicit_local_key_edits() {
        let h=MockHost::install();
        for (language,path,text) in [
            ("java","/project/C.java","import org.springframework.stereotype.Component; import org.springframework.shell.core.command.annotation.Command; @Component class C { @Command(name=\"run task\",alias={\"rt\",\"execute task\"}) void runTask(){} @Command void showTasks(){} }"),
            ("kotlin","/project/C.kt","import org.springframework.shell.command.annotation.Command\n@Command class C { @Command(command=\"run task\",alias=[\"rt\",\"execute task\"]) fun runTask(){} @Command fun showTasks(){} }")
        ] {
            h.add_file(path,text);
            let d=Document{path:path.into(),language:language.into(),text:text.into(),version:1};
            let models=model(&d);assert_eq!(models.len(),4);
            let alias=models.iter().find(|m|m.name=="rt").unwrap();
            let mut nav=ShellTools::default();let action=nav.command_rename_actions(&d,alias.definition).remove(0);
            h.answer_next_prompt(Some(&["run-now"]));h.answer_next_prompt(Some(&["yes"]));
            let result=nav.resolve_file_action(&d,action).unwrap();assert_eq!(result.edits[0].1.len(),1);
            let edit=&result.edits[0].1[0];assert_eq!(&text[offset(text,edit.range.start).unwrap()..offset(text,edit.range.end).unwrap()],"rt");
            assert_eq!(edit.text,"run-now");
            let observations=modern_model(&d,true);assert_eq!(observations.len(),4);
            assert!(models.iter().find(|m|m.name=="show-tasks").unwrap().insertion.is_some());
        }
    }
    #[test]
    fn modern_prefixes_dynamic_keys_and_duplicate_routes_do_not_offer_partial_transactions() {
        let base="import org.springframework.shell.command.annotation.Command; @Command class C { @Command(command=\"run\",alias=\"go\") void run(){} }";
        for text in [base.replace("@Command class", "@Command(command=\"prefix\") class"),base.replace("command=\"run\"","command=PREFIX"),base.replace("alias=\"go\"","alias=\"run\""),base.replace("import org.springframework.shell.command.annotation.Command;","import foreign.Command;")] {
            let d=Document{path:"/project/C.java".into(),language:"java".into(),text,version:1};
            assert!(model(&d).is_empty());
        }
    }
}

#[cfg(test)]
mod implicit_transaction_tests {
    use super::*;
    use openidea_plugin::{file_actions::FileActions,mock::MockHost};
    #[test]
    fn implicit_command_rename_materializes_a_key_and_preserves_method_and_description() {
        let h=MockHost::install();
        for (language,path,text) in [
            ("java","/project/C.java","import org.springframework.shell.standard.ShellComponent; import org.springframework.shell.standard.ShellMethod; import org.springframework.shell.standard.ShellMethodAvailability; @ShellComponent class C { @ShellMethod(\"Run task\") void runTask(){} @ShellMethodAvailability(\"run-task\") Availability ready(){return null;} }"),
            ("java","/project/C.java","import org.springframework.stereotype.Component; import org.springframework.shell.core.command.annotation.Command; @Component class C { @Command void runTask(){} }"),
            ("kotlin","/project/C.kt","import org.springframework.shell.standard.ShellComponent\nimport org.springframework.shell.standard.ShellMethod\n@ShellComponent class C { @ShellMethod(value=\"Run task\") fun runTask(){} }")
        ] {
            h.add_file(path,text);
            let d=Document{path:path.into(),language:language.into(),text:text.into(),version:1};
            let old=model(&d).into_iter().find(|m|m.name=="run-task").unwrap();assert!(old.insertion.is_some());
            let mut nav=ShellTools::default();let action=nav.command_rename_actions(&d,old.definition).remove(0);
            h.answer_next_prompt(Some(&["execute task"]));h.answer_next_prompt(Some(&["yes"]));
            let resolved=nav.resolve_file_action(&d,action).unwrap();
            let mut after=text.to_owned();for edit in resolved.edits[0].1.iter().rev(){after.replace_range(offset(text,edit.range.start).unwrap()..offset(text,edit.range.end).unwrap(),&edit.text);}
            assert!(after.contains("runTask()"));
            assert!(!after.contains("run-task"));
            if text.contains("Run task") {assert!(after.contains("value=\"Run task\""));}
            let fresh=Document{text:after,..d};let new=model(&fresh).into_iter().find(|m|m.name=="execute task").unwrap();assert!(new.insertion.is_none());
        }
    }
    const BASE: &str = "package p; import org.springframework.shell.standard.ShellMethod; import org.springframework.shell.standard.ShellMethodAvailability;\n@org.springframework.shell.standard.ShellComponent class Base {\n @ShellMethod(key=\"shared\",value=\"S\") void shared(){}\n}";
    const CHILD: &str = "package p; import org.springframework.shell.standard.ShellMethodAvailability;\n@org.springframework.shell.standard.ShellComponent class Child extends Base {\n @ShellMethodAvailability(\"shared\") Availability gate(){return null;}\n}";

    fn inherited(base: &str, child: &str, new: &str) -> Result<Vec<(String, String, Range)>, Error> {
        let h = MockHost::install();
        h.add_file("src/main/java/p/Base.java", base);
        h.add_file("src/main/java/p/Child.java", child);
        let d = Document { path: "/project/src/main/java/p/Base.java".into(), language: "java".into(), version: 1, text: base.into() };
        let command = model(&d).into_iter().next().unwrap();
        let mut tools = ShellTools::default();
        let project = host::project_of(&d.path).unwrap();
        tools.inherited_selectors(&d, &command, new, &project)
    }

    #[test]
    fn superclass_command_rename_lists_subclass_selectors_and_refuses_unproven_chains() {
        let listed = inherited(BASE, CHILD, "common").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].0.as_str(), listed[0].2.start.line, listed[0].2.start.character), ("/project/src/main/java/p/Child.java", 2, 27));
        let dynamic = CHILD.replace("(\"shared\")", "(KEY)");
        assert!(inherited(BASE, &dynamic, "common").is_err());
        let colliding = CHILD.replace("extends Base {", "extends Base { @org.springframework.shell.standard.ShellMethod(key=\"common\",value=\"C\") void c(){}");
        assert!(inherited(BASE, &colliding, "common").is_err());
    }

    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let mut out = text.to_owned();
        let mut sorted: Vec<_> = edits.iter().collect();
        sorted.sort_by_key(|e| (e.range.start.line, e.range.start.character));
        for e in sorted.iter().rev() {
            out.replace_range(offset(text, e.range.start).unwrap()..offset(text, e.range.end).unwrap(), &e.text);
        }
        out
    }

    fn rename_from(child_selector: bool, child: &str) -> Result<FileAction, Error> {
        let h = MockHost::install();
        h.add_file("src/main/java/p/Base.java", BASE);
        h.add_file("src/main/java/p/Child.java", child);
        let (path, text) = if child_selector {
            ("/project/src/main/java/p/Child.java", child)
        } else {
            ("/project/src/main/java/p/Base.java", BASE)
        };
        let d = Document { path: path.into(), language: "java".into(), version: 1, text: text.into() };
        let mut tools = ShellTools::default();
        let at = position(text, text.find("\"shared\"").unwrap() + 1);
        let a = tools.file_actions(&d, Range::new(at, at)).into_iter().find(|a| a.data.as_deref().is_some_and(|s| s.starts_with("shell-command:"))).unwrap();
        h.answer_next_prompt(Some(&["common"]));
        h.answer_next_prompt(Some(&["yes"]));
        tools.resolve_file_action(&d, a)
    }

    #[test]
    fn superclass_rename_edits_declaration_and_inheriting_selectors_from_either_side() {
        for from_child in [false, true] {
            let done = rename_from(from_child, CHILD).unwrap();
            assert_eq!(done.edits.len(), 2);
            for (path, edits) in &done.edits {
                assert_eq!(edits.len(), 1);
                let source = if path.ends_with("Base.java") { BASE } else { CHILD };
                let after = apply(source, edits);
                assert!(after.contains("\"common\"") && !after.contains("\"shared\""));
            }
        }
    }

    #[test]
    fn superclass_rename_refuses_non_literal_selectors_and_collisions() {
        assert!(rename_from(false, &CHILD.replace("(\"shared\")", "(KEY)")).is_err());
        let colliding = CHILD.replace("extends Base {", "extends Base { @org.springframework.shell.standard.ShellMethod(key=\"common\",value=\"C\") void c(){}");
        assert!(rename_from(false, &colliding).is_err());
    }

    #[test]
    fn rename_reads_a_cold_large_project_to_the_end_instead_of_asking_to_retry() {
        let h = MockHost::install();
        let body = (0..40).map(|m| format!("    String f{m}(String a) {{ return a + \"{m}\"; }}\n")).collect::<String>();
        for n in 0..3_000 {
            h.add_file(&format!("module{}/src/main/java/p{n}/S{n}.java", n % 30), &format!("package p{n};\nclass S{n} {{\n{body}}}\n"));
        }
        h.add_file("src/main/java/p/Base.java", BASE);
        h.add_file("src/main/java/p/Child.java", CHILD);
        let d = Document { path: "/project/src/main/java/p/Base.java".into(), language: "java".into(), version: 1, text: BASE.into() };
        let command = model(&d).into_iter().next().unwrap();
        let project = host::project_of(&d.path).unwrap();

        let selectors = ShellTools::default().inherited_selectors(&d, &command, "common", &project).unwrap();

        assert_eq!(selectors.len(), 1, "{selectors:?}");
        assert!(h.tasks().iter().any(|task| task.title == "Reading Spring Shell commands" && task.ended));
    }

    #[test]
    fn rename_refuses_incomplete_index() {
        let h = MockHost::install();
        h.add_file("src/main/java/p/Base.java", BASE);
        h.add_file("src/main/java/p/Child.java", CHILD);
        h.add_file("src/main/java/p/Broken.java", "class Broken {");
        let d = Document { path: "/project/src/main/java/p/Base.java".into(), language: "java".into(), version: 1, text: BASE.into() };
        let command = model(&d).into_iter().next().unwrap();
        let project = host::project_of(&d.path).unwrap();
        let result = ShellTools::default().inherited_selectors(&d, &command, "common", &project);
        assert!(matches!(result, Err(Error::Invalid(_))), "{result:?}");
    }

    #[test]
    fn rename_on_unsaved_buffer_says_save_first() {
        let h = MockHost::install();
        h.add_file("src/main/java/p/Base.java", BASE);
        let unsaved = format!("{BASE}\n// unsaved");
        let d = Document { path: "/project/src/main/java/p/Base.java".into(), language: "java".into(), version: 1, text: unsaved.clone() };
        let mut tools = ShellTools::default();
        let at = position(&unsaved, unsaved.find("\"shared\"").unwrap() + 1);
        let a = tools.file_actions(&d, Range::new(at, at)).into_iter().find(|a| a.data.as_deref().is_some_and(|s| s.starts_with("shell-command:"))).unwrap();
        let result = tools.resolve_file_action(&d, a);
        assert!(matches!(&result, Err(Error::Invalid(m)) if m == "Save the file first"), "{result:?}");
    }
}
