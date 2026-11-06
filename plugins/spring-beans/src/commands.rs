//! Editor commands that add a bean dependency or an event listener to the class at the
//! caret, and the active profiles the plugin's own profile-scoped checks consider.

use crate::{Navigator, World, describe, file_name, indent_of, merge_into};
use openidea_plugin::text::offset;
use openidea_plugin::{Error, Invocation, NotificationLevel, PickItem, TextEdit, TextEditor, host};
use spring_text::index::COMMAND_BUDGET;
use spring_text::java::{self, JavaFile, MemberKind, TypeDecl};
use spring_text::{decapitalize, range};
use std::collections::BTreeSet;

const ACTIVE_PROFILES: &str = "activeProfiles";
const AUTOWIRED: &str = "org.springframework.beans.factory.annotation.Autowired";
const EVENT_LISTENER: &str = "org.springframework.context.event.EventListener";
const MAX_CHOICES: usize = 500;
const TITLE: &str = "Spring Beans";

#[derive(Clone, Copy)]
pub enum Generate {
    Setter,
    Constructor,
    Autowire,
    Listener,
}

pub fn active_profiles() -> Vec<String> {
    let stored = host::storage::get(ACTIVE_PROFILES).ok().flatten().unwrap_or_default();
    stored.split(',').filter(|p| !p.is_empty()).map(str::to_owned).collect()
}

pub fn change_active_profiles(nav: &mut Navigator, ctx: &Invocation) -> Result<Option<String>, Error> {
    let project = ctx.project().ok_or("Open a project first.")?;
    let mut names: BTreeSet<String> = active_profiles().into_iter().collect();
    let world = World::new(nav.indexes.get(&project, COMMAND_BUDGET));
    let declared = world.beans().flat_map(|(_, b)| &b.profiles);
    names.extend(declared.map(|p| p.trim_start_matches('!')).filter(|p| !p.is_empty() && !p.contains("${") && !p.contains("#{")).map(str::to_owned));
    for extension in ["yml", "yaml", "properties"] {
        for file in project.find_files(&format!("**/application-*.{extension}"), 200).unwrap_or_default() {
            let profile = file_name(&file).strip_prefix("application-").and_then(|n| n.strip_suffix(&format!(".{extension}")));
            names.extend(profile.filter(|p| !p.is_empty()).map(str::to_owned));
        }
    }
    if names.is_empty() {
        host::notify(NotificationLevel::Info, TITLE, "No profiles found in @Profile annotations, <beans profile> or application-*.yml/properties files.");
        return Ok(None);
    }

    let items: Vec<PickItem> = names.iter().map(|n| PickItem::new(n, n)).collect();
    let current = active_profiles();
    let title = if current.is_empty() { "Active Spring profiles".to_owned() } else { format!("Active Spring profiles (now: {})", current.join(", ")) };
    let Some(chosen) = host::pick_many(&title, &items)? else { return Ok(None) };

    host::storage::set(ACTIVE_PROFILES, &chosen.join(","))?;
    let message = if chosen.is_empty() { "No active profile: every bean is considered.".to_owned() } else { format!("Active profiles: {}.", chosen.join(", ")) };
    host::notify(NotificationLevel::Info, TITLE, &message);
    Ok(None)
}

pub fn generate(nav: &mut Navigator, ctx: &Invocation, kind: Generate) -> Result<Option<String>, Error> {
    let editor = ctx.editor().ok_or("Open a Java class first.")?;
    let path = editor.path();
    let (_, text, _, _) = locate(&editor)?;

    let ty = match kind {
        Generate::Listener => {
            let Some(entered) = host::input("Event type", None)? else { return Ok(None) };
            let ty = entered.trim().trim_end_matches(".class").to_owned();
            if ty.is_empty() || !ty.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
                return Err(format!("{ty:?} is not a type name.").into());
            }
            ty
        }
        _ => {
            let Some(ty) = pick_dependency(nav, &path, &text)? else { return Ok(None) };
            ty
        }
    };
    let qualified = qualified_in(nav, &path, &text, &ty);

    let (version, text, file, class_at) = locate(&editor)?;
    let class = &file.types[class_at];
    let spot = Spot::of(&text, class).ok_or("The class has no body.")?;
    let name = decapitalize(&ty);
    let i = &spot.indent;
    let mut inserts: Vec<(usize, String)> = Vec::new();
    let mut imports: Vec<&str> = qualified.as_deref().into_iter().collect();
    let mut extra: Vec<TextEdit> = Vec::new();
    let has_field = class.fields().any(|f| f.name == name);

    match kind {
        Generate::Autowire => {
            if has_field {
                return Err(format!("{} already has a field named {name}.", class.name).into());
            }
            inserts.push((spot.fields_end, format!("\n{i}@Autowired\n{i}private {ty} {name};")));
            imports.push(AUTOWIRED);
        }
        Generate::Setter => {
            let setter = format!("set{}", capitalize(&name));
            if has_field || class.methods().any(|m| m.name == setter) {
                return Err(format!("{} already has {name} or {setter}.", class.name).into());
            }
            inserts.push((spot.fields_end, format!("\n{i}private {ty} {name};")));
            let method = format!("\n\n{i}@Autowired\n{i}public void {setter}({ty} {name}) {{\n{i}    this.{name} = {name};\n{i}}}");
            inserts.push((spot.members_end, method));
            imports.push(AUTOWIRED);
        }
        Generate::Constructor => {
            if has_field {
                return Err(format!("{} already has a field named {name}.", class.name).into());
            }
            inserts.push((spot.fields_end, format!("\n{i}private final {ty} {name};")));
            let mut constructors = class.members.iter().filter(|m| m.kind == MemberKind::Constructor);
            match (constructors.next(), constructors.next()) {
                (Some(_), Some(_)) => return Err("The class has several constructors; add the dependency to the one to use.".into()),
                (Some(existing), None) => {
                    let params = [(ty.clone(), name.clone())];
                    extra = merge_into(&text, existing, &params).ok_or("Can't extend the existing constructor.")?;
                }
                (None, _) => {
                    let visibility = if class.modifiers.iter().any(|m| m == "public") { "public " } else { "" };
                    let body = format!("{i}    this.{name} = {name};");
                    inserts.push((spot.fields_end, format!("\n\n{i}{visibility}{}({ty} {name}) {{\n{body}\n{i}}}", class.name)));
                }
            }
        }
        Generate::Listener => {
            let method = format!("on{ty}");
            if class.methods().any(|m| m.name == method) {
                return Err(format!("{} already has a method named {method}.", class.name).into());
            }
            inserts.push((spot.members_end, format!("\n\n{i}@EventListener\n{i}public void {method}({ty} event) {{\n{i}}}")));
            imports.push(EVENT_LISTENER);
        }
    }

    inserts.extend(imports.into_iter().filter_map(|q| import_edit(&text, &file, q)));
    if class.members.is_empty() && !text[spot.open..].trim_start_matches([' ', '\t']).starts_with(['\n', '\r']) {
        inserts.push((spot.open, "\n".to_owned()));
    }
    let mut edits = insertions(&text, inserts);
    edits.extend(extra);

    match editor.apply_edits(version, &edits) {
        Ok(_) => {}
        Err(Error::Denied(_)) => host::notify(NotificationLevel::Warning, TITLE, "Allow editor changes for Spring Beans in Settings › Plugins to add code."),
        Err(e) => return Err(e),
    }
    Ok(None)
}

/// Where a class takes new members: after its fields, after its last member, at its first line.
struct Spot {
    open: usize,
    fields_end: usize,
    members_end: usize,
    indent: String,
}

impl Spot {
    fn of(text: &str, class: &TypeDecl) -> Option<Self> {
        let open = class.at + text.get(class.at..)?.find('{')? + 1;
        let fields_end = class.fields().last().map_or(open, |f| f.end);
        let members_end = class.members.iter().map(|m| m.end).max().unwrap_or(open);
        let indent = match class.members.first() {
            Some(first) => indent_of(text, first.start),
            None => format!("{}    ", indent_of(text, class.start)),
        };
        Some(Self { open, fields_end, members_end, indent })
    }
}

fn locate(editor: &TextEditor) -> Result<(u64, String, JavaFile, usize), Error> {
    if !editor.path().ends_with(".java") {
        return Err("Open a Java class first.".into());
    }
    let doc = editor.document()?;
    let file = java::parse(&doc.text);
    let caret = editor.selections()?.first().and_then(|r| offset(&doc.text, r.start)).unwrap_or(0);
    let around = file.types.iter().enumerate().filter(|(_, t)| t.kind == "class" && t.start <= caret && caret <= t.end);
    let class = around
        .max_by_key(|(_, t)| t.start)
        .map(|(at, _)| at)
        .or_else(|| file.types.iter().position(|t| t.kind == "class"))
        .ok_or("The open file has no class to add to.")?;
    Ok((doc.version, doc.text, file, class))
}

fn pick_dependency(nav: &mut Navigator, path: &str, text: &str) -> Result<Option<String>, Error> {
    let index = nav.indexes.for_document(path, text, COMMAND_BUDGET).ok_or("Open the file inside a project.")?;
    let world = World::new(index);
    let typed: Vec<_> = world.beans().filter(|(_, b)| !b.ty.is_empty()).take(MAX_CHOICES).collect();
    if typed.is_empty() {
        return Err("No beans with a known type in this project.".into());
    }

    let items: Vec<PickItem> = typed
        .iter()
        .enumerate()
        .map(|(at, (p, b))| PickItem { key: at.to_string(), label: b.ty.clone(), description: Some(format!("{} · {}", describe(b), file_name(p))) })
        .collect();
    let Some(chosen) = host::pick("Choose a bean", &items)? else { return Ok(None) };
    Ok(chosen.parse::<usize>().ok().and_then(|at| typed.get(at)).map(|(_, b)| b.ty.clone()))
}

fn qualified_in(nav: &mut Navigator, path: &str, text: &str, ty: &str) -> Option<String> {
    let world = World::new(nav.indexes.for_document(path, text, COMMAND_BUDGET)?);
    let found = world.files.values().flat_map(|m| &m.types).find(|t| t.name == ty)?;
    found.qualified.contains('.').then(|| found.qualified.clone())
}

fn import_edit(text: &str, file: &JavaFile, qualified: &str) -> Option<(usize, String)> {
    let (package, _) = qualified.rsplit_once('.')?;
    let imported = file.imports.iter().any(|i| !i.is_static && (i.path == qualified || i.path.strip_suffix(".*") == Some(package)));
    if imported || package == file.package || package == "java.lang" {
        return None;
    }
    let statement = format!("import {qualified};");
    if let Some(last) = file.imports.last() {
        let after = last.end + text.get(last.end..)?.find(';')? + 1;
        return Some((after, format!("\n{statement}")));
    }
    if file.package.is_empty() {
        return Some((0, format!("{statement}\n\n")));
    }
    let declaration = text.find("package ")?;
    let after = declaration + text[declaration..].find(';')? + 1;
    Some((after, format!("\n\n{statement}")))
}

fn insertions(text: &str, mut inserts: Vec<(usize, String)>) -> Vec<TextEdit> {
    inserts.sort_by_key(|(at, _)| *at);
    let mut merged: Vec<(usize, String)> = Vec::new();
    for (at, added) in inserts {
        match merged.last_mut() {
            Some((previous, joined)) if *previous == at => joined.push_str(&added),
            _ => merged.push((at, added)),
        }
    }
    merged.into_iter().map(|(at, added)| TextEdit { range: range(text, at, at), text: added }).collect()
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}
