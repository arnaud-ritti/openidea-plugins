//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

mod ui_group;
mod checks;
mod structure;
mod try_http;
mod compat;
mod diff;
mod http;
mod hover;
mod json;
mod keys;
mod refs;
mod spec;
mod yaml_pointer;
mod yaml_hover;
mod pointer_status;
mod component_occurrences;
mod component_rename;
mod codegen;
mod codegen_options;
mod draft;
mod json_actions;
mod operation_actions;
mod section_actions;
mod yaml_scan;
mod search;

use checks::Fetched;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, HoverAction, HoverActions, Invocation, Location, LogLevel, NotificationLevel, PickItem, Plugin, Position,
    Project, Range, TextEdit, host, register_plugin,
};
use ui_kit::routes::{self, Handler, Route, RouteEvent, RouteList};

#[derive(Default)]
pub struct OpenApi {
    try_http: try_http::State,
    codegen: codegen::State,
    project_files: Vec<String>,
    /// Status line and changes of the last comparison.
    comparison: Option<Comparison>,
    routes: Option<RouteList>,
    search: search::Index,
}

struct Comparison {
    root: String,
    path: String,
    revision: String,
    version: u64,
    base_path: String,
    base: String,
    status: String,
    items: Vec<ListItem>,
}
fn refresh_comparison() {
    crate::ui_group::refresh_view(COMPARE);
}
fn comparison_owner(ctx: &Invocation, path: &str) -> Result<String, Error> {
    let root = ctx.project().ok_or(Error::Stale)?.root()?;
    if !root.starts_with('/') || root.chars().any(char::is_control) || root.split('/').any(|part| matches!(part, "." | ".."))
        || !path.starts_with(&format!("{}/", root.trim_end_matches('/')))
        || path.split('/').any(|part| matches!(part, "." | ".."))
        || host::project_of(path).and_then(|project| project.root().ok()).as_deref() != Some(root.as_str())
        || !host::projects()?.iter().any(|project| project.root().ok().as_deref() == Some(root.as_str()))
    { return Err(Error::Stale); }
    Ok(root)
}
impl Comparison {
    fn current(&self, ctx: &Invocation) -> bool {
        if comparison_owner(ctx, &self.path).ok().as_deref() != Some(self.root.as_str()) { return false; }
        if !self.base_path.starts_with("HEAD:") && comparison_owner(ctx, &self.base_path).ok().as_deref() != Some(self.root.as_str()) {return false;}
        ctx.editor().is_some_and(|editor| editor.path() == self.path && editor.document().is_ok_and(|doc| doc.version == self.version && doc.text == self.revision))
    }
}

fn project_for(path: &str) -> Result<Project, Error> {
    host::projects()?
        .into_iter()
        .find(|p| p.root().is_ok_and(|root| path.starts_with(&format!("{}/", root.trim_end_matches('/')))))
        .ok_or_else(|| format!("{path} isn't in an open project.").into())
}

fn is_yaml(path: &str) -> bool {
    path.ends_with(".yaml") || path.ends_with(".yml")
}

fn is_spec_file(path: &str) -> bool {
    is_yaml(path) || path.ends_with(".json")
}

pub(crate) const REF_MARKER: &str = "icons/marker-ref.svg";
pub(crate) const REFERENCED_BY_MARKER: &str = "icons/openapiinitiative.svg";
const MAX_LINE_TARGETS: usize = 200;
const PREVIEWS: [&str; 2] = ["preview", "preview-any"];

fn escape_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }

    out
}

const MAX_PREVIEW_FILES: usize = 50;
const MAX_PREVIEW_BYTES: usize = 512 * 1024;

fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));

    format!("{hash:016x}")
}

/// The files `$ref`s of `text` point at, keyed as written, for the preview to resolve them.
/// `None` when they are the ones the page already has (`known` is its fingerprint). The reply
/// is broadcast to every preview, so it names the requesting page by `id`.
fn preview_files(project: &Project, path: &str, text: &str, id: &str, known: &str) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut members = Vec::new();
    let mut bytes = 0;
    let mut skipped = 0;
    for r in spec::refs(text) {
        let Some((file, _)) = spec::split_ref(&r.target) else {
            continue;
        };
        if file.is_empty() || seen.iter().any(|s| s == file) {
            continue;
        }

        seen.push(file.to_owned());
        let Fetched::Text(content) = fetch(project.read_file(&resolve_path(path, file))) else {
            continue;
        };
        if members.len() >= MAX_PREVIEW_FILES || bytes + content.len() > MAX_PREVIEW_BYTES {
            skipped += 1;
            continue;
        }

        bytes += content.len();
        members.push(format!("\"{}\":\"{}\"", escape_json(file), escape_json(&content)));
    }

    let files = format!("{{\"skipped\":{skipped},\"files\":{{{}}}", members.join(","));
    let hash = fingerprint(&files);
    if hash == known {
        return None;
    }

    Some(format!("{{\"id\":\"{}\",{},\"hash\":\"{hash}\"}}", escape_json(id), &files[1..]))
}

/// `./schemas/pet.yaml`, or `../common/pet.yaml`, from the directory of `from_file`.
fn relative_path(from_file: &str, target: &str) -> String {
    let from: Vec<&str> = from_file.split('/').collect();
    let to: Vec<&str> = target.split('/').collect();
    let from_dir = &from[..from.len() - 1];
    let common = from_dir.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let ups = from_dir.len() - common;
    let rest = to[common..].join("/");

    match ups {
        0 => format!("./{rest}"),
        _ => format!("{}{rest}", "../".repeat(ups)),
    }
}

pub fn resolve_path(from_file: &str, rel: &str) -> String {
    if rel.starts_with('/') {
        return rel.to_owned();
    }

    let mut parts: Vec<&str> = from_file.split('/').collect();
    parts.pop();
    for part in rel.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }

    parts.join("/")
}

// The host reports a missing file as `Invalid("…: no such file")`, as it does a path outside the project.
fn fetch(result: Result<String, Error>) -> Fetched {
    match result {
        Ok(content) => Fetched::Text(content),
        Err(Error::NotFound(_)) => Fetched::Missing,
        Err(Error::Invalid(message)) if message.ends_with("no such file") => Fetched::Missing,
        Err(_) => Fetched::Unreadable,
    }
}

fn spec_candidate_hint(text: &str) -> Vec<Diagnostic> {
    if host::setting_bool("checkSpecCandidates") == Some(false) {
        return Vec::new();
    }

    if text.len() > MAX_SCANNED_FILE_BYTES {
        return Vec::new();
    }

    checks::spec_candidate(text)
}

fn problems(path: &str) -> Result<Vec<Diagnostic>, Error> {
    let project = host::project_of(path).ok_or_else(|| Error::NotFound("No source project".into()))?;
    let text = project.read_file(path)?;
    if !canonical_reference_path(path) || text.len()>MAX_SCANNED_FILE_BYTES || bounded_reference_keys(&text).is_none() { return Ok(Vec::new()); }
    let Some(spec) = spec_or_fragment(path, &text) else {
        return Ok(spec_candidate_hint(&text));
    };

    let doc=Document {path:path.into(),language:if spec::is_json(&text) {"json"} else {"yaml"}.into(),text:text.clone(),version:0};
    let read = |file: &str| read_sibling_result(&doc,file);
    let mut found = checks::check(&text, &spec, &read);
    if spec.version.is_empty() {
        found.retain(|d| !d.message.starts_with("Cannot find `#"));
    }

    Ok(found)
}

/// A document with an `openapi`/`swagger` key, or a file the `specGlobs` setting names (a fragment, whose local pointers can point into another file).
fn spec_or_fragment(path: &str, text: &str) -> Option<spec::Spec> {
    spec::parse(text).or_else(|| {
        (text.len() <= MAX_SCANNED_FILE_BYTES && is_spec_file(path) && matches_spec_globs(path))
            .then(|| spec::Spec { version: String::new(), keys: spec::scan(text) })
    })
}

impl OpenApi {
    fn refresh_project_files(&mut self, project: &Project) {
        let Ok(root)=project.root() else { self.project_files.clear(); return; };
        let mut files = Vec::new();
        for pattern in ["**/*.yaml", "**/*.yml", "**/*.json"] {
            match project.find_files(pattern, 500) {
                Ok(found) => files.extend(found),
                Err(error) => host::log(LogLevel::Warn, &format!("openapi: listing {pattern}: {error}")),
            }
        }

        files.retain(|path| !in_hidden_dir(&root, path) && canonical_reference_path(path) && host::project_of(path).and_then(|p|p.root().ok()).as_deref()==Some(root.as_str()));
        files.sort();files.dedup();
        self.project_files = files;
    }

    fn ref_file_items(&self, doc: &Document, typed_file: &str) -> Vec<String> {
        if typed_file.starts_with("http://") || typed_file.starts_with("https://")
            || !canonical_reference_path(&doc.path) || doc.text.len()>MAX_SCANNED_FILE_BYTES {
            return Vec::new();
        }

        let Some(project)=host::project_of(&doc.path) else { return Vec::new(); };
        let Ok(root)=project.root() else { return Vec::new(); };
        self.project_files.iter().filter(|path| **path != doc.path && canonical_reference_path(path)
            && host::project_of(path).and_then(|p|p.root().ok()).as_deref()==Some(root.as_str())
            && project.stat(path).is_ok_and(|s|s.kind==openidea_plugin::FileKind::File && s.size<=MAX_SCANNED_FILE_BYTES as u64))
            .map(|path| relative_path(&doc.path, path)).collect()
    }
}

fn copy_or_explain(text: &str, done: &str, denied: &str) -> Result<Option<String>, Error> {
    match host::copy_to_clipboard(text) {
        Ok(()) => host::notify(NotificationLevel::Info, "OpenAPI", done),
        Err(Error::Denied(_)) => host::notify(NotificationLevel::Warning, "OpenAPI", denied),
        Err(error) => return Err(error),
    }

    Ok(None)
}

fn sibling_file(path: &str, suffix: &str) -> String {
    let (stem, extension) = path.rsplit_once('.').unwrap_or((path, ""));

    suffix.replace("{stem}", stem).replace("{ext}", extension)
}

/// Writes generated text through the owning project's native file API, with clipboard fallback.
fn save_or_copy(target: &str, text: &str, what: &str) -> Result<Option<String>, Error> {
    let name = target.rsplit('/').next().unwrap_or(target);
    let written = (|| {
        if !canonical_reference_path(target) { return Err(Error::Invalid("The output path is not canonical.".into())); }
        let project = project_for(target)?;
        let root = project.root()?;
        if host::project_of(target).and_then(|p| p.root().ok()).as_deref() != Some(root.as_str()) { return Err(Error::Stale); }
        let snapshot = |project: &Project| -> Result<Option<String>, Error> {
            match project.read_file(target) {
                Ok(text) => Ok(Some(text)),
                Err(Error::NotFound(_)) => Ok(None),
                Err(error) => Err(error),
            }
        };
        let before = snapshot(&project)?;
        if before.is_some() && !host::confirm("Overwrite file", &format!("{name} already exists and is replaced."))? {
            return Err(Error::Cancelled);
        }
        // Prompts may outlive a project switch or a target edit. Never overwrite
        // a different owner or content than the user just reviewed.
        if project.root()? != root
            || host::project_of(target).and_then(|p| p.root().ok()).as_deref() != Some(root.as_str())
            || snapshot(&project)? != before { return Err(Error::Stale); }
        project.write_file(target, text)
    })();
    match written {
        Ok(()) => {
            host::notify(NotificationLevel::Info, "OpenAPI", &format!("{what} written to {name}."));
            host::open_location(target, 0, 0)?;
            Ok(None)
        }
        Err(Error::Cancelled) => Ok(None),
        Err(Error::Denied(_)) => copy_or_explain(
            text,
            &format!("{what} copied to the clipboard."),
            &ui_kit::denied::message("Writing the file", "project.write", "OpenAPI"),
        ),
        Err(error) => Err(error),
    }
}

impl OpenApi {
    fn compare_specs(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let buffer = editor.document()?;
        let version = buffer.version;
        let revision = buffer.text;
        if spec::parse(&revision).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        comparison_owner(ctx, &path)?;
        let project = ctx.project().ok_or(Error::Stale)?;
        self.refresh_project_files(&project);
        let root = project.root()?;
        let mut candidates = Vec::new();
        for file in self.project_files.iter().filter(|file| **file != path) {
            let Ok(text) = project.read_file(file) else {
                continue;
            };
            if text.len() <= MAX_SCANNED_FILE_BYTES && spec::parse(&text).is_some() {
                let relative = file.strip_prefix(root.trim_end_matches('/')).unwrap_or(file).trim_start_matches('/');
                candidates.push(PickItem::new(file, relative));
            }
        }
        if candidates.is_empty() {
            host::notify(NotificationLevel::Info, "OpenAPI", "No other OpenAPI file in this project to compare with.");
            return Ok(None);
        }
        let Some(base_path) = host::pick("Compare the current spec with (base)", &candidates)? else {
            return Ok(None);
        };
        if !candidates.iter().any(|item| item.key == base_path) { return Err(Error::Stale); }
        comparison_owner(ctx, &base_path)?;
        let base = project.read_file(&base_path)?;
        self.show_comparison(ctx, &base_path, &base, &path, &revision, version)?;
        crate::ui_group::reveal_view(COMPARE)?;
        Ok(None)
    }

    fn compatibility_report(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let buffer = editor.document()?;
        if spec::parse(&buffer.text).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        if buffer.text.len() > MAX_SCANNED_FILE_BYTES {
            return Err(Error::Invalid("The current spec exceeds the 1 MiB bound.".into()));
        }
        comparison_owner(ctx, &path)?;
        let project = ctx.project().ok_or(Error::Stale)?;
        self.refresh_project_files(&project);
        let root = project.root()?;
        let mut candidates = Vec::new();
        for file in self.project_files.iter().filter(|file| **file != path) {
            let Ok(text) = project.read_file(file) else { continue };
            if text.len() <= MAX_SCANNED_FILE_BYTES && spec::parse(&text).is_some() {
                let relative = file.strip_prefix(root.trim_end_matches('/')).unwrap_or(file).trim_start_matches('/');
                candidates.push(PickItem::new(file, relative));
            }
        }
        if candidates.is_empty() {
            host::notify(NotificationLevel::Info, "OpenAPI", "No other OpenAPI file in this project to compare with.");
            return Ok(None);
        }
        let Some(old_path) = host::pick("Old spec (the previous version) to compare the current spec with", &candidates)? else {
            return Ok(None);
        };
        if !candidates.iter().any(|item| item.key == old_path) { return Err(Error::Stale); }
        comparison_owner(ctx, &old_path)?;
        let old = project.read_file(&old_path)?;
        if old.len() > MAX_SCANNED_FILE_BYTES {
            return Err(Error::Invalid("The old spec exceeds the 1 MiB bound.".into()));
        }
        let current = ctx.editor().ok_or(Error::Stale)?.document()?;
        if current.version != buffer.version || current.text != buffer.text {
            return Err(Error::Stale);
        }
        let findings = compat::compare(&old_path, &old, &path, &buffer.text).map_err(Error::Invalid)?;
        let breaking = findings.iter().filter(|f| f.severity == compat::Severity::Breaking).count();
        host::notify(NotificationLevel::Info, "OpenAPI", &format!("{breaking} breaking changes, {} findings in total.", findings.len()));
        let report = compat::render(&old_path, &path, &findings);

        save_or_copy(&sibling_file(&path, "{stem}.compatibility.md"), &report, "OpenAPI compatibility report")
    }

    fn compare_last_revision(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let buffer = editor.document()?;
        let version = buffer.version;
        let revision = buffer.text;
        if spec::parse(&revision).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        comparison_owner(ctx, &path)?;
        let project = ctx.project().ok_or(Error::Stale)?;
        let root = project.root()?;
        let relative = path.strip_prefix(root.trim_end_matches('/')).unwrap_or(&path).trim_start_matches('/');
        let args = ["show".to_owned(), format!("HEAD:./{relative}")];
        let committed = match project.run_process("git", &args, None) {
            Ok(output) if output.status == Some(0) => output.stdout,
            Ok(_) => {
                host::notify(NotificationLevel::Info, "OpenAPI", &format!("{relative} has no committed version in HEAD to compare with."));
                return Ok(None);
            }
            Err(Error::Denied(_)) => {
                host::notify(
                    NotificationLevel::Warning,
                    "OpenAPI",
                    &ui_kit::denied::message("Comparing with the last revision", "process:git", "OpenAPI"),
                );
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        self.show_comparison(ctx, &format!("HEAD:{relative}"), &committed, &path, &revision, version)?;
        crate::ui_group::reveal_view(COMPARE)?;
        Ok(None)
    }

    fn export_self_contained(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let text = editor.document()?.text;
        if spec::parse(&text).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        let project = project_for(&path)?;
        let read = |file: &str| fetch(project.read_file(file));
        let bundled = match refs::bundle(&text, &path, &read) {
            Ok(Some(bundled)) => bundled,
            Ok(None) => {
                host::notify(
                    NotificationLevel::Info,
                    "OpenAPI",
                    "This file has no references to other files; it is already self-contained.",
                );
                return Ok(None);
            }
            Err(message) => return Err(message.into()),
        };

        save_or_copy(&sibling_file(&path, "{stem}.bundled.{ext}"), &bundled, "The self-contained spec")
    }

    fn copy_reference(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let text = editor.document()?.text;
        let caret = editor.selections()?.first().map(|range| range.start).ok_or("There is no caret.")?;
        let pointer = refs::pointer_at(&text, caret).ok_or("There is no unambiguous source key at the caret in a supported complete OpenAPI document.")?;
        let reference = match spec::is_json(&text) {
            true => format!("\"$ref\": {}", serde_json::to_string(&pointer).map_err(|_|Error::Invalid("Could not serialize the source pointer.".into()))?),
            false => format!("$ref: '{}'", pointer.replace('\'',"''")),
        };

        copy_or_explain(
            &reference,
            &format!("Copied {reference}"),
            &ui_kit::denied::message("Copying the reference", "clipboard", "OpenAPI"),
        )
    }

    fn show_comparison(&mut self, ctx: &Invocation, base_path: &str, base: &str, path: &str, revision: &str, version: u64) -> Result<(), Error> {
        if base.len() > MAX_SCANNED_FILE_BYTES || revision.len() > MAX_SCANNED_FILE_BYTES {return Err(Error::Invalid("Comparison sources exceed the 1 MiB per-file bound.".into()));}
        let root = comparison_owner(ctx, path)?;
        if !base_path.starts_with("HEAD:") {comparison_owner(ctx, base_path)?;}
        let current = ctx.editor().ok_or(Error::Stale)?;
        let doc = current.document()?;
        if current.path() != path || doc.version != version || doc.text != revision {return Err(Error::Stale);}
        let changes = diff::compare(base_path, base, path, revision);
        let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
        let status = format!("{} to {}: {} changes", name(base_path), name(path), changes.len());
        let items = changes.iter().take(1000).map(|c| ListItem::new(&c.location, &c.label).detail(&c.detail)).collect();
        let status = if changes.len() > 1000 {format!("{status} · first 1,000 changes shown")} else {status};
        self.comparison = Some(Comparison {root,path:path.into(),revision:revision.into(),version,base_path:base_path.into(),base:base.into(),status:format!("{status} · captured comparison; recompute after source changes"),items});
        refresh_comparison();
        Ok(())
    }
}

impl Plugin for OpenApi {
    fn activate(&mut self, context: &openidea_plugin::Context) -> Result<(), Error> {
        self.try_http.activate(context);
        // Only comparison cache retirement consumes filesystem events here.
        let _ = host::subscribe(openidea_plugin::EventKind::FilesChanged);
        let _ = host::subscribe(openidea_plugin::EventKind::EditorFocused);
        Ok(())
    }

    fn handle_events(&mut self, events: &[Event], ctx: &Invocation) {
        self.codegen.observe(events);
        let may_hold_spec = |path: &String| is_spec_file(path) || !path.rsplit('/').next().unwrap_or_default().contains('.');
        if events.iter().any(|event| match event {
            Event::FileSaved(path) => may_hold_spec(path),
            Event::FilesChanged(batch) => batch.created.iter().chain(&batch.modified).chain(&batch.removed).any(may_hold_spec),
            _ => false,
        }) {
            self.search.forget();
        }
        if events.iter().any(|event| matches!(event, Event::EditorFocused(_) | Event::FileOpened(_) | Event::FileSaved(_))) {
            let _ = host::set_context(SPEC_CONTEXT, ctx.editor().is_some_and(|editor| editor.document().is_ok_and(|doc| is_spec_text(&editor.path(), &doc.text))));
        }
        if self.comparison.as_ref().is_some_and(|comparison| events.iter().any(|event| {
            let changed = |path: &str| !path.is_empty() && (path == comparison.path || path == comparison.base_path
                || comparison.path.starts_with(&format!("{}/", path.trim_end_matches('/')))
                || comparison.base_path.starts_with(&format!("{}/", path.trim_end_matches('/'))));
            match event {
                Event::FileSaved(path) => changed(path),
                Event::FilesChanged(batch) => batch.created.iter().chain(&batch.modified).chain(&batch.removed).any(|path| changed(path)),
                _ => false,
            }
        })) {
            self.comparison = None;
            refresh_comparison();
        } else if self.comparison.is_some() && events.iter().any(|event| matches!(event, Event::EditorFocused(_))) {
            // Each receiving frame rechecks its own active source; retain another
            // owning window's captured comparison instead of deleting on focus.
            refresh_comparison();
        }
        let mut paths: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                Event::FileOpened(path) | Event::FileSaved(path) => Some(path.as_str()),
                _ => None,
            })
            .filter(|path| is_spec_file(path))
            .collect();
        paths.sort();
        paths.dedup();

        if let Some(project) = paths.first().and_then(|path| host::project_of(path)) {
            self.refresh_project_files(&project);
        }

        for path in paths {
            let result = problems(path).and_then(|found| host::publish_diagnostics(path, &found));
            if let Err(error) = result {
                host::log(LogLevel::Warn, &format!("openapi: problems for {path}: {error}"));
            }
        }
    }

    fn run_command(&mut self, id: &str, ctx: &Invocation, arg: Option<&str>) -> Result<Option<String>, Error> {
        if id == "try-local-read" {
            return match self.try_local(ctx, arg) {
                Err(Error::Cancelled) => Ok(None),
                result => result,
            };
        }
        if id == "copy-route-curl" || id == "create-route-http" {
            let route = match arg {
                Some(key) => self.route_list().route(key).cloned().ok_or_else(|| Error::Invalid("That endpoint is no longer listed. Refresh the OpenAPI Endpoints window and try again.".into()))?,
                None => match pick_route(ctx)? {
                    Some(route) => route,
                    None => return Ok(None),
                },
            };
            if id == "copy-route-curl" {
                routes::copy_as_curl(&route, &route_base(&route), "OpenAPI")?;
            } else {
                let directory = route.location.as_ref().and_then(|location| location.path.rsplit_once('/')).map(|(directory, _)| directory);
                routes::create_http_file(&route, &route_base(&route), directory, "OpenAPI")?;
            }
            return Ok(None);
        }
        if id == "export-comparison" {
            let comparison = self.comparison.as_ref().filter(|comparison| comparison.current(ctx))
                .ok_or("Run a current OpenAPI comparison before exporting its report.")?;
            if !comparison.base_path.starts_with("HEAD:")
                && ctx.project().ok_or(Error::Stale)?.read_file(&comparison.base_path)? != comparison.base {
                return Err(Error::Stale);
            }
            let code = |value: &str| format!("`{}`", value.replace('`', "’").replace(['\r','\n'], " "));
            let mut report = format!("# OpenAPI comparison\n\nBaseline: {}\n\nCurrent: {}\n\n{}\n\n", code(&comparison.base_path), code(&comparison.path), code(&comparison.status));
            if comparison.items.is_empty() { report.push_str("No differences.\n"); }
            for item in &comparison.items {
                report.push_str(&format!("- {}", code(&item.label)));
                if let Some(detail) = &item.detail { report.push_str(&format!(" — {}", code(detail))); }
                report.push('\n');
            }
            return save_or_copy(&sibling_file(&comparison.path, "{stem}.comparison.md"), &report, "OpenAPI comparison report");
        }
        if id == "compare-specs" {
            return self.compare_specs(ctx);
        }
        if id == "compare-specs-breaking" {
            return self.compatibility_report(ctx);
        }
        if id == "draft-from-source" {
            return draft::run(ctx);
        }
        if id == "export-self-contained" {
            return self.export_self_contained(ctx);
        }
        if id == "generate-code" { return codegen::run(ctx,&mut self.codegen); }
        if id == "forget-codegen-stage" {return self.codegen.forget(ctx);}
        if id == "generate-code-configured" { return codegen::run_configured(ctx,&mut self.codegen); }
        if id == "component-project-usages" { return component_occurrences::project_usages(ctx); }
        if id == "component-occurrences" { return component_occurrences::run(ctx); }
        if id == "copy-reference" {
            return self.copy_reference(ctx);
        }
        if id == "compare-last-revision" {
            return self.compare_last_revision(ctx);
        }
        if id != "copy-http-requests" {
            return Err(Error::NotFound(format!("command {id}")));
        }

        let editor = ctx.editor().ok_or("No file is open.")?;
        let document = editor.document()?;
        let text = document.text;
        let spec = spec::parse(&text).filter(|_| is_spec_file(&editor.path())).ok_or("The current file is not an OpenAPI document.")?;
        let count = spec::operations(&text, &spec.keys).len();
        if count == 0 {
            return Err("The current file has no operations.".into());
        }
        let requests = http::requests(&text, &spec);

        save_or_copy(&sibling_file(&editor.path(), "{stem}.http"), &requests, &format!("{count} requests"))
    }
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => (0..=text.len()).any(|skip| glob_match(rest, &text[skip..])),
        Some((b'?', rest)) => !text.is_empty() && glob_match(rest, &text[1..]),
        Some((byte, rest)) => text.first() == Some(byte) && glob_match(rest, &text[1..]),
    }
}

fn matches_glob(pattern: &str, path: &str) -> bool {
    if !pattern.contains('/') {
        let name = path.rsplit('/').next().unwrap_or(path);
        return glob_match(pattern.as_bytes(), name.as_bytes());
    }

    let anchored = if pattern.starts_with('/') { pattern.to_owned() } else { format!("*/{pattern}") };

    glob_match(anchored.as_bytes(), path.as_bytes())
}

fn matches_spec_globs(path: &str) -> bool {
    let Some(globs) = host::setting_string("specGlobs") else {
        return false;
    };

    globs.split(',').map(str::trim).filter(|glob| !glob.is_empty()).any(|glob| matches_glob(glob, path))
}

fn is_spec_name(path: &str) -> bool {
    if is_spec_file(path) && matches_spec_globs(path) {
        return true;
    }

    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    if !matches!(extension, "yaml" | "yml" | "json") {
        return false;
    }

    ["openapi", "swagger"].iter().any(|word| stem == *word || stem.ends_with(&format!(".{word}")))
}

const SKELETONS: [(&str, &str, &str, &str); 4] = [
    (
        "oas3",
        "An OpenAPI 3.1 root with info and a GET path",
        "openapi: 3.1.0\ninfo:\n  title: ${1:API}\n  version: ${2:1.0.0}\npaths:\n  /${3:path}:\n    get:\n      responses:\n        '200':\n          description: ${4:OK}\n$0",
        "{\n  \"openapi\": \"3.1.0\",\n  \"info\": {\n    \"title\": \"${1:API}\",\n    \"version\": \"${2:1.0.0}\"\n  },\n  \"paths\": {\n    \"/${3:path}\": {\n      \"get\": {\n        \"responses\": {\n          \"200\": {\n            \"description\": \"${4:OK}\"\n          }\n        }\n      }\n    }\n  }\n}$0",
    ),
    (
        "opnp",
        "A path item with a get operation and a 200 response",
        "/${1:path}:\n  get:\n    summary: ${2:Summary}\n    responses:\n      '200':\n        description: ${3:OK}\n$0",
        "\"/${1:path}\": {\n  \"get\": {\n    \"summary\": \"${2:Summary}\",\n    \"responses\": {\n      \"200\": {\n        \"description\": \"${3:OK}\"\n      }\n    }\n  }\n}$0",
    ),
    (
        "endp",
        "An operation with summary, operationId, parameters and responses",
        "${1:get}:\n  summary: ${2:Summary}\n  operationId: ${3:operationId}\n  parameters:\n    - name: ${4:id}\n      in: ${5:query}\n      schema:\n        type: string\n  responses:\n    '200':\n      description: ${6:OK}\n$0",
        "\"${1:get}\": {\n  \"summary\": \"${2:Summary}\",\n  \"operationId\": \"${3:operationId}\",\n  \"parameters\": [\n    {\n      \"name\": \"${4:id}\",\n      \"in\": \"${5:query}\",\n      \"schema\": {\n        \"type\": \"string\"\n      }\n    }\n  ],\n  \"responses\": {\n    \"200\": {\n      \"description\": \"${6:OK}\"\n    }\n  }\n}$0",
    ),
    (
        "swag",
        "A Swagger 2.0 root with info and paths",
        "swagger: \"2.0\"\ninfo:\n  title: ${1:API}\n  version: ${2:1.0.0}\npaths:\n  $0",
        "{\n  \"swagger\": \"2.0\",\n  \"info\": {\n    \"title\": \"${1:API}\",\n    \"version\": \"${2:1.0.0}\"\n  },\n  \"paths\": {\n    $0\n  }\n}",
    ),
];

fn skeleton_items(doc: &Document) -> Vec<CompletionItem> {
    let json = doc.path.ends_with(".json") || spec::is_json(&doc.text);
    SKELETONS
        .iter()
        .map(|(label, detail, yaml_body, json_body)| {
            let mut item = CompletionItem::new(label, CompletionKind::Snippet);
            item.detail = Some((*detail).to_owned());
            item.insert_text = (if json { json_body } else { yaml_body }).to_string();
            item.snippet = true;
            item
        })
        .collect()
}

fn parse_or_fragment(doc: &Document) -> Option<spec::Spec> {
    spec::parse(&doc.text).or_else(|| is_spec_name(&doc.path).then(|| spec::Spec { version: String::new(), keys: spec::scan(&doc.text) }))
}

fn keyword_items(doc: &Document, at: Position) -> Vec<CompletionItem> {
    let skeletons = skeleton_items(doc);
    let json = spec::is_json(&doc.text) || doc.path.ends_with(".json");
    let Some(version) = spec::parse(&doc.text).map(|spec| spec.version) else {
        return skeletons;
    };
    let Some(caret) = offset(&doc.text, at) else {
        return skeletons;
    };
    let version = version.as_str();
    let drop_null = !version.starts_with("3.1");
    let range = |from: usize| spring_text::range(&doc.text, from, caret);

    if json {
        return match keys::complete_json(&doc.text, caret, version) {
            None => skeletons,
            Some((items, from, is_value)) => {
                let mut all: Vec<CompletionItem> = items
                    .into_iter()
                    .filter(|key| !(is_value && drop_null && key.label == "null"))
                    .map(|key| {
                        let kind = if is_value { CompletionKind::EnumMember } else { CompletionKind::Property };
                        let mut item = CompletionItem::new(key.label, kind);
                        item.insert_text = key.insert;
                        item.replace = Some(range(from));
                        item.filter_text = Some(key.label.to_owned());
                        item
                    })
                    .collect();
                if !is_value {
                    all.extend(skeletons);
                }
                all
            }
        };
    }

    match keys::complete(&doc.text, caret, version) {
        None => skeletons,
        Some(keys::Completion::Values { items, from }) => items
            .into_iter()
            .filter(|value| !(drop_null && *value == "null"))
            .map(|value| {
                let mut item = CompletionItem::new(value, CompletionKind::EnumMember);
                item.replace = Some(range(from));
                item
            })
            .collect(),
        Some(keys::Completion::Keys { items, from }) => {
            let mut all: Vec<CompletionItem> = items
                .into_iter()
                .map(|key| {
                    let mut item = CompletionItem::new(key.label, CompletionKind::Property);
                    item.insert_text = key.insert;
                    item.replace = Some(range(from));
                    item.filter_text = Some(key.label.to_owned());
                    item
                })
                .collect();
            all.extend(skeletons);
            all
        }
    }
}

fn ref_at(text: &str, at: Position) -> Option<spec::Ref> {
    let at = offset(text, at)?;
    spec::refs(text).into_iter().find(|r| r.at <= at && at <= r.end)
}

fn bounded_reference_keys(text:&str)->Option<Vec<spec::Key>> {
    if text.len()>MAX_SCANNED_FILE_BYTES {return None;}
    if spec::is_json(text) {json::partial_bounded_keys(text,8192,4<<20)}
    else {yaml_scan::keys(text,8192,4<<20,48)}
}
fn in_hidden_dir(root: &str, path: &str) -> bool {
    let Some(relative) = path.strip_prefix(root) else {
        return false;
    };

    relative.split('/').any(|segment| segment.starts_with('.') && segment.len() > 1)
}
fn canonical_reference_path(path: &str) -> bool {
        path.starts_with('/') && path.len()<=4096 && !path.chars().any(char::is_control)
            && !path.contains('\\') && !path[1..].split('/').any(|p| matches!(p,""|"."|".."))
}
fn try_read_sibling(doc: &Document, file: &str) -> Result<String,Fetched> {
    fn rebased(text: &str) -> Option<bool> {
        Some(bounded_reference_keys(text)?.iter().any(|k| {
            let raw=&text[k.at..k.end];
            if spec::is_json(text) { json::unescape(raw)=="$id" } else { raw.trim_matches(['\'', '"'])=="$id" }
        }))
    }
    // These locations represent physical source siblings, not arbitrary URI,
    // classpath or JSON Schema base-URI resolution.
    if file.is_empty() || file.len()>4096 || file.starts_with('/') || file.trim()!=file
        || file.chars().any(char::is_control) || file.contains([':', '\\', '%', '?', '#'])
        || doc.text.len()>MAX_SCANNED_FILE_BYTES || !canonical_reference_path(&doc.path) || rebased(&doc.text)!=Some(false) { return Err(Fetched::Unreadable); }
    let project=host::project_of(&doc.path).ok_or(Fetched::Unreadable)?;
    let root=project.root().map_err(|_|Fetched::Unreadable)?;
    let path=resolve_path(&doc.path,file);
    if !canonical_reference_path(&path) || !path.strip_prefix(root.trim_end_matches('/')).is_some_and(|tail|tail.starts_with('/'))
        || host::project_of(&path).and_then(|p|p.root().ok()).as_deref()!=Some(root.as_str()) { return Err(Fetched::Unreadable); }
    let metadata=project.stat(&path).map_err(|error|match error { Error::NotFound(_)=>Fetched::Missing,_=>Fetched::Unreadable })?;
    if metadata.kind!=openidea_plugin::FileKind::File || metadata.size>MAX_SCANNED_FILE_BYTES as u64 { return Err(Fetched::Unreadable); }
    // read_file prefers unsaved editor text; the second bound covers that text
    // and growth after stat. Never assume disk metadata bounds the editor copy.
    let content=project.read_file(&path).map_err(|_|Fetched::Unreadable)?;
    if content.len()>MAX_SCANNED_FILE_BYTES || rebased(&content)!=Some(false) { return Err(Fetched::Unreadable); }
    Ok(content)
}
fn read_sibling_result(doc: &Document, file: &str) -> Fetched {
    match try_read_sibling(doc,file) { Ok(content)=>Fetched::Text(content),Err(result)=>result }
}
fn read_sibling(doc: &Document, file: &str) -> Option<String> {
    match read_sibling_result(doc,file) { Fetched::Text(content)=>Some(content),_=>None }
}

#[cfg(test)]
mod sibling_ownership_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn source(target: &str) -> Document {
        Document {path:"/project/spec/openapi.json".into(),language:"json".into(),version:3,
            text:format!(r##"{{"openapi":"3.1.1","x":{{"$ref":"{target}#/Pet"}}}}"##)}
    }
    #[test]
    fn sibling_read_rejects_nested_project_target_and_oversize_source() {
        let h=MockHost::install().with_capabilities(&["project.read"]);
        h.add_file("spec/nested/schema.json",r#"{"Pet":{"type":"string"}}"#);
        h.add_project("/project/spec/nested");
        let doc=source("nested/schema.json");
        assert!(read_sibling(&doc,"nested/schema.json").is_none());
        h.add_file("spec/large.json",&format!("{{\"Pet\":{{}}}}{}"," ".repeat(MAX_SCANNED_FILE_BYTES)));
        assert!(read_sibling(&doc,"large.json").is_none());
    }
    #[test]
    fn sibling_definition_and_completion_preserve_same_owner_unsaved_targets_and_denied_reads() {
        for caps in [vec![],vec!["project.read","editor.read"]] {
            let h=MockHost::install().with_capabilities(&caps);
            h.add_file("spec/common.json",r#"{"Pet":{"type":"string"}}"#);
            h.open_editor("/project/spec/common.json","json",r#"{"Unsaved":{"type":"integer"}}"#,&[]);
            let doc=source("common.json");
            let read=read_sibling(&doc,"common.json");
            if caps.is_empty() { assert!(read.is_none()); } else {
                assert!(read.unwrap().contains("Unsaved"));
                let changed=source("common.json");
                let changed=Document {text:changed.text.replace("#/Pet","#/Unsaved"),..changed};
                let caret=position(&changed.text,changed.text.find("common.json").unwrap());
                let found=OpenApi::default().definition(&changed,caret);
                assert_eq!(found.len(),1);assert_eq!(found[0].path,"/project/spec/common.json");
                assert!(!OpenApi::default().completion(&changed,caret,None).is_empty());
            }
        }
    }
    #[test]
    fn physical_sibling_navigation_refuses_uri_aliases_rebased_documents_and_unsaved_growth() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);
        h.add_file("spec/common.json",r#"{"Pet":{}}"#);
        let doc=source("common.json");
        for path in ["/project/spec/common.json","../other/missing.json","file:common.json","common%2ejson","common.json?x"," common.json","common\\.json"] {
            assert!(read_sibling(&doc,path).is_none(),"{path}");
        }
        let rebased=Document {text:doc.text.replace("\"openapi\":",r#""$id":"https://example.test/root/","openapi":"#),..doc.clone()};
        assert!(read_sibling(&rebased,"common.json").is_none());
        h.open_editor("/project/spec/common.json","json",r#"{"$id":"other.json","Pet":{}}"#,&[]);
        assert!(read_sibling(&doc,"common.json").is_none());
        h.open_editor("/project/spec/common.json","json",&" ".repeat(MAX_SCANNED_FILE_BYTES+1),&[]);
        assert!(read_sibling(&doc,"common.json").is_none());
        assert!(h.requests().is_empty());assert!(h.process_runs().is_empty());
    }
    #[test]
    fn cross_file_diagnostics_keep_unknown_ownership_quiet_but_report_known_missing_files() {
        let h=MockHost::install().with_capabilities(&["project.read"]);
        let doc=source("nested/schema.json");
        h.add_file("spec/openapi.json",&doc.text);
        h.add_file("spec/nested/schema.json",r#"{"Other":{}}"#);
        h.add_project("/project/spec/nested");
        assert!(problems(&doc.path).unwrap().iter().all(|d|d.code.as_deref()!=Some("unresolved-ref")));
        h.add_file("spec/openapi.json",&source("missing.json").text);
        assert!(problems(&doc.path).unwrap().iter().any(|d|d.message.contains("Cannot find file `missing.json`")));
        h.add_file("spec/large.json",&" ".repeat(MAX_SCANNED_FILE_BYTES+1));
        h.add_file("spec/openapi.json",&source("large.json").text);
        assert!(problems(&doc.path).unwrap().iter().all(|d|d.code.as_deref()!=Some("unresolved-ref")));
    }
    #[test]
    fn discovered_reference_files_are_owned_and_revalidated_after_nested_project_open() {
        let h=MockHost::install().with_capabilities(&["project.read"]);
        h.add_file("spec/common.json",r#"{"Pet":{}}"#);
        h.add_file("spec/nested/child.yaml","Pet: {}\n");
        let doc=source("common.json");
        let project=host::project_of(&doc.path).unwrap();
        let mut plugin=OpenApi::default();plugin.refresh_project_files(&project);
        assert!(plugin.ref_file_items(&doc,"").iter().any(|p|p=="./nested/child.yaml"));
        h.add_project("/project/spec/nested");
        assert!(!plugin.ref_file_items(&doc,"").iter().any(|p|p.contains("nested/")));
        plugin.refresh_project_files(&project);
        assert!(!plugin.project_files.iter().any(|p|p.contains("nested/")));
        assert!(plugin.ref_file_items(&doc,"").iter().any(|p|p=="./common.json"));
        let _h=h.with_capabilities(&[]);
        assert!(plugin.ref_file_items(&doc,"").is_empty());
    }
    #[test]
    fn file_open_events_discover_the_innermost_source_project_inventory() {
        let h=MockHost::install().with_capabilities(&["project.read","events"]);
        h.add_file("/project/spec/nested/openapi.json",r#"{"openapi":"3.1.1","paths":{}}"#);
        h.add_file("/project/spec/nested/common.json",r#"{"Pet":{}}"#);
        h.add_file("/project/spec/parent.json",r#"{"Parent":{}}"#);
        h.add_project("/project/spec/nested");
        let mut plugin=OpenApi::default();
        plugin.handle_events(&[Event::FileOpened("/project/spec/nested/openapi.json".into())],&h.invocation());
        assert!(plugin.project_files.iter().any(|p|p=="/project/spec/nested/common.json"));
        assert!(!plugin.project_files.iter().any(|p|p=="/project/spec/parent.json"));
    }
    #[test]
    fn incomplete_json_completion_remains_available_but_amplified_sibling_metadata_is_unknown() {
        let h=MockHost::install().with_capabilities(&["project.read"]);
        h.add_file("spec/common.yaml","Pet: {}\n");
        let text=r##"{"openapi":"3.1.1","x":{"$ref":"common.yaml#/Pe""##;
        let doc=Document {path:"/project/spec/openapi.json".into(),language:"json".into(),text:text.into(),version:1};
        let items=OpenApi::default().completion(&doc,position(text,text.rfind("Pe").unwrap()+2),None);
        assert!(items.iter().any(|item|item.label=="common.yaml#/Pet"));
        let mut amplified=String::from("Pet: {}\n");
        for depth in 0..30 {amplified.push_str(&format!("{}{}:\n"," ".repeat(depth*2),"x".repeat(512)));}
        for n in 0..800 {amplified.push_str(&format!("{}n{n}: value\n"," ".repeat(60)));}
        h.add_file("spec/common.yaml",&amplified);
        assert!(read_sibling(&doc,"common.yaml").is_none());
        assert!(OpenApi::default().completion(&doc,position(text,text.rfind("Pe").unwrap()+2),None).is_empty());
        assert!(h.requests().is_empty());assert!(h.process_runs().is_empty());
    }
}

impl Providers for OpenApi {
    fn resolve_code_action(&mut self,doc:&Document,action:CodeAction)->Result<CodeAction,Error>{
        if action.data.as_deref().is_some_and(|data|data.contains("physical-component-rename")){component_rename::resolve(doc,action)}else{Ok(action)}
    }
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !spec::maybe_spec(&doc.text) && !is_spec_name(&doc.path) {
            return Vec::new();
        }

        if bounded_reference_keys(&doc.text).is_none() {return Vec::new();}
        let Some(typed) = ref_at(&doc.text, at) else {
            return keyword_items(doc, at);
        };
        let source_proof=pointer_status::Proof::parse(&doc.text);
        // An empty YAML scalar, including an unquoted # prefix interpreted as
        // a comment by the semantic parser, is an intentional typing context.
        let partial_yaml_value=!spec::is_json(&doc.text) && (typed.at==typed.end ||
            (typed.target.starts_with('#') && typed.at.checked_sub(1).and_then(|index|doc.text.as_bytes().get(index)).is_some_and(|byte|!matches!(byte,b'\''|b'"'))));
        if !partial_yaml_value && source_proof.as_ref().is_some_and(|proof| offset(&doc.text,at).and_then(|caret|proof.navigation_ref_at(caret)).is_none()) {return Vec::new();}
        let Some((file, _)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(spec) = parse_or_fragment(doc) else {
            return Vec::new();
        };

        let mut labels: Vec<String> = if file.is_empty() {
            spec::ref_targets(&doc.text, &spec.keys).into_iter()
                .filter(|target| source_proof.as_ref().is_none_or(|proof| target.strip_prefix('#').and_then(|pointer|proof.pointer_location(pointer)).is_some()))
                .collect()
        } else {
            read_sibling(doc, file)
                .map(|content| {
                    let Some(proof)=pointer_status::Proof::parse(&content) else {return Vec::new();};
                    spec::ref_targets(&content, &spec::scan(&content)).into_iter()
                        .filter(|target| target.strip_prefix('#').and_then(|pointer|proof.pointer_location(pointer)).is_some())
                        .map(|target| format!("{file}{target}")).collect()
                })
                .unwrap_or_default()
        };
        if !typed.target.contains('#') {
            labels.extend(self.ref_file_items(doc, file));
        }

        let replace = spring_text::range(&doc.text, typed.at, typed.end);
        let quoted = typed.at > 0 && matches!(doc.text.as_bytes()[typed.at - 1], b'\'' | b'"');
        labels
            .into_iter()
            .map(|label| {
                let mut item = CompletionItem::new(&label, CompletionKind::Reference);
                item.replace = Some(replace);
                if !quoted && label.starts_with('#') {
                    item.insert_text = format!("'{label}'");
                }
                item.filter_text = Some(label);
                item
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if !spec::maybe_spec(&doc.text) && !is_spec_name(&doc.path) {
            return Vec::new();
        }

        if spec::is_json(&doc.text) && (doc.text.len()>512<<10 || json::bounded_keys(&doc.text,8192,4<<20).is_none()) {return Vec::new();}
        if !spec::is_json(&doc.text) && bounded_reference_keys(&doc.text).is_none() {return Vec::new();}
        let Some(typed) = pointer_status::Proof::parse(&doc.text).and_then(|proof| proof.navigation_ref_at(offset(&doc.text,at)?)) else {
            return Vec::new();
        };
        let Some((file, fragment)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(_) = parse_or_fragment(doc) else {
            return Vec::new();
        };

        let (path, content) = if file.is_empty() {
            (doc.path.clone(), doc.text.clone())
        } else {
            let Some(content) = read_sibling(doc, file) else {
                return Vec::new();
            };
            (resolve_path(&doc.path, file), content)
        };

        if fragment.is_empty() {
            return vec![Location { path, range: Range::default() }];
        }

        let range=if spec::is_json(&content) { hover::pointer_definition(&content,fragment) }
            else { yaml_pointer::definition(&content,fragment) };
        range.map(|range| Location { path, range })
            .into_iter()
            .collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        if !spec::maybe_spec(&doc.text) {
            return Vec::new();
        }

        let Some(model)=pointer_status::Proof::parse(&doc.text) else {return Vec::new();};
        let Some(spec) = spec::parse(&doc.text) else {return Vec::new();};
        // Same icon on one line: the IDE merges them into one button with a target chooser.
        let operations=spec::operations(&doc.text,&spec.keys).into_iter().filter_map(|operation| {
            let references=model.operation_references(&operation);
            let targets:Vec<_>=references.into_iter().map(|(reference,range)|(format!("{} {}: {reference}",operation.method,operation.path),range)).collect();
            (!targets.is_empty()).then(||(position(&doc.text,operation.at).line,REF_MARKER,targets))
        });
        let components=model.component_usages().into_iter().map(|(base,declaration,uses)| {
            let targets=uses.into_iter().map(|range|(format!("#{base} is referenced on line {}",range.start.line+1),range)).collect();
            (declaration.start.line,REFERENCED_BY_MARKER,targets)
        });
        operations.chain(components).take(256).flat_map(|(line,icon,targets):(u32,&str,Vec<(String,Range)>)| {
            let total=targets.len();
            targets.into_iter().take(MAX_LINE_TARGETS).map(move |(tooltip,range)| LineMarker {
                line,
                icon:Some(icon.into()),
                tooltip:if total>MAX_LINE_TARGETS {format!("{tooltip} (first {MAX_LINE_TARGETS} of {total} shown)")} else {tooltip},
                target:Some(Location {path:doc.path.clone(),range}),
            })
        }).collect()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        let Some(spec) = spec::parse(&doc.text) else {
            return Vec::new();
        };

        let mut actions = Vec::new();
        if let Some(edits) = refs::extract_schema(&doc.text, &spec.keys, selection.start.line) {
            actions.push(CodeAction {
                title: "Extract inline schema to components/schemas".into(),
                kind: Some("refactor.extract".into()),
                preferred: false,
                edits: vec![(doc.path.clone(), edits)],
                data: None,
            });
        }

        let modeline = spec::modeline(&doc.text, &spec.version).filter(|_| !spec::is_json(&doc.text));
        if let Some(line) = modeline {
            let top = Range::new(Position::new(0, 0), Position::new(0, 0));
            actions.push(CodeAction {
                title: "Add the OpenAPI schema for the YAML language server".into(),
                kind: Some("source".into()),
                preferred: false,
                edits: vec![(doc.path.clone(), vec![TextEdit { range: top, text: line }])],
                data: None,
            });
        }

        actions.extend(section_actions::actions(doc, selection));
        actions.extend(operation_actions::actions(doc, selection));
        if let Some(rename)=component_rename::offer(doc,selection){actions.push(rename);}
        actions
    }
}

#[cfg(test)]
mod json_definition_integrity_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn go(text: &str) -> Vec<Location> {
        let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text:text.into(),version:5};
        OpenApi::default().definition(&doc,position(text,text.rfind("#/").unwrap()))
    }
    #[test]
    fn duplicate_json_ancestors_and_dotted_impostors_do_not_establish_definition_owners() {
        let _h=MockHost::install().with_capabilities(&[]);
        for text in [
            r##"{"openapi":"3.1.1","components":{},"components":{"schemas":{"Pet":{}}},"x":{"$ref":"#/components/schemas/Pet"}}"##,
            r##"{"openapi":"3.1.1","components.schemas.Pet":{},"x":{"$ref":"#/components/schemas/Pet"}}"##,
            r##"{"openapi":"3.1.1","components":{"schemas":{"Pet":{},"Pet":{}}},"x":{"$ref":"#/components/schemas/Pet"}}"##,
        ] { assert!(go(text).is_empty(),"{text}"); }
    }
    #[test]
    fn source_backed_json_pointer_definition_preserves_escaped_names_and_utf16_ranges() {
        let _h=MockHost::install().with_capabilities(&[]);
        let text=r##"{"openapi":"3.1.1","info":{"title":"😀"},"components":{"schemas":{"A/B":{"properties":{"~id":{"type":"integer"}}}}},"x":{"$ref":"#/components/schemas/A~1B/properties/~0id"}}"##;
        let found=go(text);assert_eq!(found.len(),1);
        let r=found[0].range;
        assert_eq!(offset(text,r.start).map(|a|&text[a..offset(text,r.end).unwrap()]),Some("~id"));
        assert_eq!(r.start.character as usize,text[..text.find("~id").unwrap()].encode_utf16().count());
    }
    #[test]
    fn sibling_json_targets_need_unique_valid_object_key_source_chains() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);
        let text=r##"{"openapi":"3.1.1","x":{"$ref":"common.json#/Pet"}}"##;
        h.add_file("common.json",r#"{"Pet":{"description":"old"}}"#);
        let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text:text.into(),version:1};
        let caret=position(text,text.find("common.json").unwrap());
        assert_eq!(OpenApi::default().definition(&doc,caret).len(),1);
        for target in [r#"{"Pet":{},"Pet":{}}"#,r#"{"Pet":{"type":"string"}"#,r#"{"Pet.type":"string"}"#] {
            h.open_editor("/project/common.json","json",target,&[]);
            assert!(OpenApi::default().definition(&doc,caret).is_empty(),"{target}");
        }
        assert!(h.requests().is_empty());assert!(h.process_runs().is_empty());
    }
    #[test]
    fn unsupported_json_encoded_array_and_malformed_pointer_owners_are_withheld() {
        let _h=MockHost::install().with_capabilities(&[]);
        for text in [
            r##"{"openapi":"3.1.1","components":{"schemas":{"Pet":{}}},"x":{"$ref":"#/components/schemas/P%65t"}}"##,
            r##"{"openapi":"3.1.1","components":{"schemas":{"P~2t":{}}},"x":{"$ref":"#/components/schemas/P~2t"}}"##,
            r##"{"openapi":"3.1.1","components":{"schemas":{"Pet":{"allOf":[{"type":"string"}]}}},"x":{"$ref":"#/components/schemas/Pet/allOf/0/type"}}"##,
            r##"{"openapi":"3.1.1","$id":"other.json","Pet":{},"x":{"$ref":"#/Pet"}}"##,
        ] { assert!(go(text).is_empty(),"{text}"); }
        let oversized=format!(r##"{{"openapi":"3.1.1","Pet":{{}},"description":"{}","x":{{"$ref":"#/Pet"}}}}"##,"a".repeat(512<<10));
        assert!(go(&oversized).is_empty());
    }
}

#[cfg(test)]
mod yaml_definition_integrity_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    fn go(text: &str) -> Vec<Location> {
        let doc=Document {path:"/project/openapi.yaml".into(),language:"yaml".into(),text:text.into(),version:5};
        OpenApi::default().definition(&doc,position(text,text.rfind("#/").unwrap()))
    }
    #[test]
    fn duplicate_yaml_owners_and_flattened_impostors_do_not_establish_definition_locations() {
        let _h=MockHost::install().with_capabilities(&[]);
        for text in [
            "openapi: 3.1.1\ncomponents: {}\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: '#/components/schemas/Pet'\n",
            "openapi: 3.1.1\ncomponents.schemas.Pet: {}\nx:\n  $ref: '#/components/schemas/Pet'\n",
            "openapi: 3.1.1\ncomponents:\n  schemas:\n    Pet: {}\n    Pet: {}\nx:\n  $ref: '#/components/schemas/Pet'\n",
        ] { assert!(go(text).is_empty(),"{text}"); }
    }
    #[test]
    fn literal_yaml_key_chain_has_exact_utf16_location_and_canonical_pointer_escaping() {
        let _h=MockHost::install().with_capabilities(&[]);
        let text="openapi: 3.1.1\ninfo:\n  title: 😀\ncomponents:\n  schemas:\n    'A/B':\n      properties:\n        '~id':\n          type: integer\nx:\n  $ref: '#/components/schemas/A~1B/properties/~0id'\n";
        let found=go(text);assert_eq!(found.len(),1);
        let range=found[0].range;
        assert_eq!(offset(text,range.start).map(|a|&text[a..offset(text,range.end).unwrap()]),Some("~id"));
    }
    #[test]
    fn json_and_yaml_sources_use_current_same_owner_yaml_fragment_locations() {
        let h=MockHost::install().with_capabilities(&["project.read","editor.read"]);
        h.add_file("common.yaml","Pet: {}\n");
        for (extension,text) in [
            ("yaml","openapi: 3.1.1\nx:\n  $ref: 'common.yaml#/Pet'\n"),
            ("json",r##"{"openapi":"3.1.1","x":{"$ref":"common.yaml#/Pet"}}"##),
        ] {
            h.open_editor("/project/common.yaml","yaml","Pet:\n  description: Current unsaved\n",&[]);
            let doc=Document {path:format!("/project/openapi.{extension}"),language:extension.into(),text:text.into(),version:2};
            let caret=position(text,text.find("common.yaml").unwrap());
            let found=OpenApi::default().definition(&doc,caret);assert_eq!(found.len(),1);
            assert_eq!(found[0].path,"/project/common.yaml");assert_eq!(found[0].range,Range::new(Position::new(0,0),Position::new(0,3)));
            h.open_editor("/project/common.yaml","yaml","Pet: {}\nPet: {}\n",&[]);
            assert!(OpenApi::default().definition(&doc,caret).is_empty());
        }
        assert!(h.requests().is_empty());assert!(h.process_runs().is_empty());
    }
}

#[cfg(test)]
mod marker_integrity_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn hidden_dir_check_is_relative_to_the_root() {
        assert!(!in_hidden_dir("/x/.claude/w/project", "/x/.claude/w/project/api/o.yaml"));
        assert!(in_hidden_dir("/x/.claude/w/project", "/x/.claude/w/project/.openidea/s/o.yaml"));
        assert!(!in_hidden_dir("/x/.claude/w/project", "/elsewhere/.git/o.yaml"));
    }
    #[test]
    fn line_markers_skip_non_openapi_json() {
        let _host=MockHost::install().with_capabilities(&[]);
        let doc=Document {path:"/project/i18n.json".into(),language:"json".into(),text:r##"{"hello":{"$ref":"#/x"}}"##.into(),version:1};
        assert!(OpenApi::default().line_markers(&doc).is_empty());
    }
    #[test]
    fn codegen_stage_files_not_ref_candidates() {
        let host=MockHost::install().with_capabilities(&["project.read"]);
        let spec="openapi: 3.0.0\npaths: {}\n";
        host.add_file("/project/api/openapi.yaml", spec);
        host.add_file("/project/.openidea/codegen-stage-1/openapi.yaml", spec);
        let project=host.invocation().project().unwrap();
        let mut plugin=OpenApi::default();plugin.refresh_project_files(&project);
        assert!(plugin.project_files.iter().any(|p|p=="/project/api/openapi.yaml"),"{:?}",plugin.project_files);
        assert!(!plugin.project_files.iter().any(|p|p.contains("/.openidea/")),"{:?}",plugin.project_files);
        assert!(project_routes(&project).unwrap().iter().all(|r|!format!("{:?}",r.location).contains(".openidea")));
    }
    #[test]
    fn duplicate_or_dotted_target_owners_do_not_create_operation_navigation_markers() {
        let _host=MockHost::install().with_capabilities(&[]);
        for text in [
            r##"{"openapi":"3.1.1","paths":{"/p":{"get":{"responses":{"200":{"$ref":"#/components/schemas/Pet"}}}}},"components":{"schemas":{"Pet":{}}},"components":{"schemas":{"Other":{}}}}"##,
            r##"{"openapi":"3.1.1","paths":{"/p":{"get":{"responses":{"200":{"$ref":"#/components/schemas/Pet"}}}}},"components.schemas.Pet":{}}"##,
        ] {
            let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text:text.into(),version:1};
            assert!(OpenApi::default().line_markers(&doc).is_empty(),"{text}");
        }
    }
    #[test]
    fn markers_keep_actual_source_order_current_text_and_exact_utf16_target_ranges_without_io() {
        let host=MockHost::install().with_capabilities(&[]);
        let text=r##"{"openapi":"3.1.1","paths":{"/😀pets":{"get":{"z":{"$ref":"#/components/schemas/😀A~1B"},"a":{"$ref":"#/components/schemas/Later"}}}},"components":{"schemas":{"😀A/B":{"type":"object"},"Later":{}}}}"##;
        let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text:text.into(),version:1};
        let mut plugin=OpenApi::default();
        let refs=|markers:Vec<LineMarker>|markers.into_iter().filter(|m|m.icon.as_deref()==Some(REF_MARKER)).collect::<Vec<_>>();
        let markers=refs(plugin.line_markers(&doc));assert_eq!(markers.len(),2);
        assert!(markers[0].tooltip.contains("😀A~1B"));assert!(markers[1].tooltip.contains("Later"));
        assert_eq!(markers[0].line,markers[1].line);
        let range=markers[0].target.as_ref().unwrap().range;
        assert_eq!(&text[offset(text,range.start).unwrap()..offset(text,range.end).unwrap()],"😀A/B");
        assert_eq!(range.end.character-range.start.character,5);
        let changed=Document {text:text.replace("#/components/schemas/😀A~1B","#/Missing"),version:2,..doc};
        let markers=refs(plugin.line_markers(&changed));assert_eq!(markers.len(),1);assert!(markers[0].tooltip.contains("Later"));
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());assert!(host.opened_locations().is_empty());
    }
    #[test]
    fn singleton_yaml_flow_refs_keep_owned_markers_but_unproved_flow_children_do_not() {
        let _host=MockHost::install().with_capabilities(&[]);
        let text="openapi: 3.1.1\npaths:\n  /p:\n    get:\n      responses:\n        '200':\n          schema: { $ref: '#/components/schemas/Pet' }\ncomponents:\n  schemas:\n    Pet: {}\n";
        let doc=Document {path:"/project/openapi.yaml".into(),language:"yaml".into(),text:text.into(),version:1};
        assert_eq!(OpenApi::default().line_markers(&doc).len(),1);
        let complex=Document {text:text.replace("schemas/Pet' }","schemas/Pet', title: extra }"),..doc};
        assert!(OpenApi::default().line_markers(&complex).is_empty());
    }
    #[test]
    fn unknown_marker_contexts_and_marker_result_budget_stay_quiet_and_bounded() {
        let _host=MockHost::install().with_capabilities(&[]);
        for text in [
            "openapi: 3.1.1\npaths:\n  /p:\n    get:\n      description: |\n        $ref: '#/components/schemas/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n",
            "openapi: 3.1.1\npaths:\n  /p:\n    $ref: 'other.yaml#/Path'\n    get:\n      x:\n        $ref: '#/components/schemas/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n",
            "openapi: 3.1.1\npaths:\n  /p:\n    get:\n      x:\n        $ref: 'common.yaml#/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n",
            "openapi: 3.1.1\npaths:\n  /p:\n    get:\n      x:\n        $ref: '#/components/schemas/P%65t'\ncomponents:\n  schemas:\n    Pet: {}\n",
        ] {
            let doc=Document {path:"/project/openapi.yaml".into(),language:"yaml".into(),text:text.into(),version:1};
            assert!(OpenApi::default().line_markers(&doc).iter().all(|m|m.icon.as_deref()!=Some(REF_MARKER)),"{text}");
        }
        let paths=(0..300).map(|n|(format!("/p{n}"),serde_json::json!({"get":{"x":{"$ref":"#/components/schemas/Pet"}}}))).collect::<serde_json::Map<_,_>>();
        let text=serde_json::json!({"openapi":"3.1.1","paths":paths,"components":{"schemas":{"Pet":{}}}}).to_string();
        let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text,version:1};
        assert_eq!(OpenApi::default().line_markers(&doc).len(),256);
    }
}

#[cfg(test)]
mod reference_provenance_tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    #[test]
    fn descriptions_and_dotted_source_keys_are_not_definition_reference_fields() {
        let _host=MockHost::install().with_capabilities(&[]);
        for (language,text) in [
            ("yaml","openapi: 3.1.1\ndescription: |\n  $ref: '#/components/schemas/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n"),
            ("yaml","openapi: 3.1.1\nx.$ref: '#/components/schemas/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n"),
            ("json",r##"{"openapi":"3.1.1","description":"$ref: '#/components/schemas/Pet'","components":{"schemas":{"Pet":{}}}}"##),
        ] {
            let doc=Document {path:format!("/project/openapi.{language}"),language:language.into(),text:text.into(),version:1};
            assert!(OpenApi::default().definition(&doc,position(text,text.find("#/components").unwrap())).is_empty(),"{text}");
        }
    }
    #[test]
    fn complete_source_completion_requires_reference_and_target_owners() {
        let _host=MockHost::install().with_capabilities(&[]);
        for text in [
            "openapi: 3.1.1\ndescription: |\n  $ref: '#/components/schemas/Pe'\ncomponents:\n  schemas:\n    Pet: {}\n",
            "openapi: 3.1.1\nx.$ref: '#/components/schemas/Pe'\ncomponents:\n  schemas:\n    Pet: {}\n",
            "openapi: 3.1.1\nx:\n  $ref: '#/components/schemas/Pe'\ncomponents.schemas:\n  Pet: {}\n",
        ] {
            let doc=Document {path:"/project/openapi.yaml".into(),language:"yaml".into(),text:text.into(),version:1};
            assert!(OpenApi::default().completion(&doc,position(text,text.find("#/components").unwrap()),None).is_empty(),"{text}");
        }
    }
    #[test]
    fn actual_array_flow_and_escaped_reference_fields_keep_definition_navigation() {
        let _host=MockHost::install().with_capabilities(&[]);
        for (language,text) in [
            ("json",r##"{"openapi":"3.1.1","x":[{"\u0024ref":"#/components/schemas/Pet"}],"components":{"schemas":{"Pet":{}}}}"##),
            ("yaml","openapi: 3.1.1\nx:\n  - $ref: '#/components/schemas/Pet'\ncomponents:\n  schemas:\n    Pet: {}\n"),
            ("yaml","openapi: 3.1.1\nx: {$ref: '#/components/schemas/Pet'}\ncomponents:\n  schemas:\n    Pet: {}\n"),
        ] {
            let doc=Document {path:format!("/project/openapi.{language}"),language:language.into(),text:text.into(),version:1};
            let locations=OpenApi::default().definition(&doc,position(text,text.find("#/components").unwrap()));
            assert_eq!(locations.len(),1,"{text}");
            let range=locations[0].range;
            assert_eq!(&text[offset(text,range.start).unwrap()..offset(text,range.end).unwrap()],"Pet");
        }
    }
    #[test]
    fn complete_external_target_completion_does_not_invent_a_dotted_namespace_owner() {
        let h=MockHost::install().with_capabilities(&["project.read"]);
        h.add_file("common.yaml","components.schemas:\n  Pet: {}\n");
        let text=r##"{"openapi":"3.1.1","x":{"$ref":"common.yaml#/comp""##;
        let doc=Document {path:"/project/openapi.json".into(),language:"json".into(),text:text.into(),version:1};
        assert!(OpenApi::default().completion(&doc,position(text,text.rfind("comp").unwrap()+4),None).is_empty());
        h.add_file("common.yaml","components:\n  schemas:\n    Pet: {}\n");
        let items=OpenApi::default().completion(&doc,position(text,text.rfind("comp").unwrap()+4),None);
        assert!(items.iter().any(|item|item.label=="common.yaml#/components/schemas/Pet"));
    }
}

const MAX_SCANNED_FILE_BYTES: usize = 1 << 20;

pub const ENDPOINTS: &str = "endpoints";
pub const COMPARE: &str = "compare";
pub const OPERATIONS_PANEL: &str = "operations";
/// `plugin.aritti.openapi.spec`: the active editor shows a spec, for the operations panel's `when`.
const SPEC_CONTEXT: &str = "spec";

fn is_spec_text(path: &str, text: &str) -> bool {
    is_spec_file(path) && text.len() <= MAX_SCANNED_FILE_BYTES && spec_or_fragment(path, text).is_some()
}

/// The editor panel beside a spec. `{op:"outline",id,path}` answers the operations of the file's
/// current text (broadcast to every panel frame, so `id` names the asking one);
/// `{op:"open",path,line,column}` moves the caret to an operation.
fn operations_panel(request: &str) -> Result<(), Error> {
    let request: serde_json::Value = serde_json::from_str(request).map_err(|_| Error::Invalid("Malformed panel request.".into()))?;
    let path = request["path"]
        .as_str()
        .filter(|path| is_spec_file(path) && !path.chars().any(char::is_control) && !path.split('/').any(|part| matches!(part, "." | "..")))
        .ok_or_else(|| Error::Invalid("The panel needs a YAML or JSON file.".into()))?;
    match request["op"].as_str() {
        Some("open") => {
            let number = |field: &str| request[field].as_u64().and_then(|n| u32::try_from(n).ok());
            let line = number("line").ok_or_else(|| Error::Invalid("The panel sent no line.".into()))?;
            project_for(path)?;
            host::open_location(path, line, number("column").unwrap_or(0))
        }
        Some("outline") => {
            // Every request is answered, even an unreadable file, so the page never waits in vain.
            let mut reply = match project_for(path).and_then(|project| project.read_file(path)) {
                Err(error) => serde_json::json!({ "message": format!("Cannot read this file: {error}") }),
                Ok(text) if text.len() > MAX_SCANNED_FILE_BYTES => {
                    serde_json::json!({ "message": "This file is over 1 MiB, too large to list its operations." })
                }
                Ok(text) => match spec_or_fragment(path, &text) {
                    None => serde_json::json!({ "message": "Not an OpenAPI document." }),
                    Some(spec) => {
                        let ends = operation_end_lines(&text, &spec);
                        let operations: Vec<_> = routes(path, &text, &spec)
                            .into_iter()
                            .zip(ends)
                            .map(|(route, end_line)| {
                                let at = route.location.map(|location| location.range.start).unwrap_or_default();
                                serde_json::json!({
                                    "method": route.method.to_ascii_uppercase(),
                                    "path": route.path,
                                    "tag": route.handler.class,
                                    "operationId": route.handler.member,
                                    "line": at.line,
                                    "column": at.character,
                                    "endLine": end_line,
                                })
                            })
                            .collect();
                        serde_json::json!({ "operations": operations })
                    }
                },
            };
            reply["id"] = request["id"].as_str().filter(|id| id.len() <= 64).into();
            reply["path"] = path.into();
            host::post_view_message(OPERATIONS_PANEL, &reply.to_string())
        }
        _ => Err(Error::Invalid("Unknown panel request.".into())),
    }
}

/// The last line of each operation (its deepest key or value), in [`spec::operations`] order, so
/// the panel can tell which operation holds the caret or a problem.
fn operation_end_lines(text: &str, spec: &spec::Spec) -> Vec<u32> {
    let mut keys: Vec<(&str, usize)> = spec.keys.iter().map(|key| (key.key.as_str(), key.end.max(key.value_end))).collect();
    keys.sort_unstable();
    spec::operations(text, &spec.keys)
        .into_iter()
        .map(|operation| {
            let prefix = format!("{}.", operation.key);
            let from = keys.partition_point(|(key, _)| *key < prefix.as_str());
            let end = keys[from..].iter().take_while(|(key, _)| key.starts_with(&prefix)).map(|(_, end)| *end).max();
            position(text, end.unwrap_or(operation.end)).line
        })
        .collect()
}

const DEFAULT_BASE: &str = "http://localhost:8080";
const RENDERER_KEY: &str = "previewRenderer";

/// The route's spec server (as Write Requests uses it), else the local default.
fn pick_route(ctx: &Invocation) -> Result<Option<Route>, Error> {
    let project = ctx.project().ok_or("Open a project to choose an endpoint.")?;
    let routes = project_routes(&project)?;
    if routes.is_empty() {
        return Err("No OpenAPI operations found in this project.".into());
    }
    let items: Vec<_> = routes
        .iter()
        .enumerate()
        .map(|(at, route)| PickItem::new(&at.to_string(), &format!("{} {}", route.method, route.path)).description(&route.handler.class))
        .collect();
    let Some(chosen) = host::pick("Choose an endpoint", &items)? else {
        return Ok(None);
    };
    Ok(chosen.parse::<usize>().ok().and_then(|at| routes.into_iter().nth(at)))
}

fn route_base(route: &routes::Route) -> String {
    route
        .location
        .as_ref()
        .and_then(|location| host::project_of(&location.path)?.read_file(&location.path).ok())
        .and_then(|text| spec::parse(&text))
        .map(|spec| http::base_url(&spec))
        .unwrap_or_else(|| DEFAULT_BASE.into())
}
const NO_TAG: &str = "(no tag)";

fn routes(path: &str, text: &str, spec: &spec::Spec) -> Vec<Route> {
    let by_key = spec::exact(&spec.keys);
    spec::operations(text, &spec.keys)
        .into_iter()
        .map(|operation| {
            let tag = by_key.get(format!("{}.tags[0]", operation.key).as_str()).map(|k| k.value.clone()).or_else(|| {
                let flow = &by_key.get(format!("{}.tags", operation.key).as_str())?.value;
                let first = flow.trim().strip_prefix('[')?.split([',', ']']).next()?;
                Some(first.trim().trim_matches(['"', '\'']).to_owned())
            });
            let at = position(text, operation.at);
            Route {
                method: operation.method,
                path: operation.path,
                handler: Handler {
                    class: tag.filter(|tag| !tag.is_empty()).unwrap_or_else(|| NO_TAG.to_owned()),
                    member: operation.operation_id,
                },
                name: None,
                location: Some(Location { path: path.into(), range: Range::new(at, at) }),
            }
        })
        .collect()
}

/// The YAML and JSON files of the project that may hold a spec, outside hidden folders.
fn spec_paths(project: &Project) -> Result<Vec<String>, Error> {
    let mut paths = project.find_files("**/*.yaml", 500)?;
    paths.extend(project.find_files("**/*.yml", 500)?);
    paths.extend(project.find_files("**/*.json", 500)?);

    let root = project.root()?;
    paths.retain(|path| !in_hidden_dir(&root, path));
    Ok(paths)
}

/// The operations of one file, none when it is unreadable, too large or not a spec.
fn file_routes(project: &Project, path: &str) -> Vec<Route> {
    let Ok(text) = project.read_file(path) else {
        return Vec::new();
    };
    if text.len() > MAX_SCANNED_FILE_BYTES {
        return Vec::new();
    }

    spec_or_fragment(path, &text).map(|spec| routes(path, &text, &spec)).unwrap_or_default()
}

// ponytail: re-scans the project on every render; cache per file version if large projects make it slow.
fn project_routes(project: &Project) -> Result<Vec<Route>, Error> {
    Ok(spec_paths(project)?.iter().flat_map(|path| file_routes(project, path)).collect())
}

impl OpenApi {
    fn route_list(&mut self) -> &mut RouteList {
        self.routes.get_or_insert_with(|| RouteList::new("openapi.routes", "aritti.openapi"))
    }
}

impl Views for OpenApi {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        (|| -> Result<View, Error> {
            if view == "try-response" {
                return Ok(self.try_http.view(ctx));
            }
            if view == COMPARE {
                return Ok(match &self.comparison {
                    None => View::message("Run OpenAPI: Compare With Another Spec…"),
                    Some(comparison) if !comparison.current(ctx) => View::message("The compared document changed or is no longer focused. Focus it and compare again."),
                    Some(comparison) if comparison.items.is_empty() => View::message("No differences.").status(&comparison.status),
                    Some(comparison) => View::new(Body::List(comparison.items.clone())).status(&comparison.status),
                });
            }
            let Some(project) = ctx.project() else {
                return Ok(View::message("Open a project to list its endpoints."));
            };

            let refresh = Button::new("refresh", "Refresh").icon("refresh");
            let list = self.route_list();
            list.set_routes(project_routes(&project)?);
            Ok(list.render(View::new(Body::List(Vec::new())).button(refresh)))
        })().map(ui_kit::actions::available)
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if ui_kit::web::handle(self, view, &event, ctx, include_str!("ui_config.json"))? {
            return Ok(());
        }
        if view == "try-response" {
            return match event {
                ViewEvent::Button(key) => self.send_prepared(ctx, &key),
                _ => Err(Error::Invalid("Response/request fields are read-only.".into())),
            };
        }
        if view == OPERATIONS_PANEL {
            return match event {
                ViewEvent::Message(request) => operations_panel(&request),
                _ => Ok(()),
            };
        }


        if PREVIEWS.contains(&view) {
            let ViewEvent::Message(request) = event else {
                return Ok(());
            };
            if request == "try" {
                return match self.try_local(ctx, None) {
                    Err(Error::Cancelled) => Ok(()),
                    result => result.map(|_| ()),
                };
            }
            if let Some(renderer) = request.strip_prefix("renderer\t") {
                if matches!(renderer, "swagger" | "redoc") {
                    let _ = host::storage::set(RENDERER_KEY, renderer);
                }
                return Ok(());
            }
            let mut parts = request.splitn(3, '\t');
            let (Some("files"), Some(id), Some(known)) = (parts.next(), parts.next(), parts.next()) else {
                return Ok(());
            };
            let editor = ctx.editor().ok_or_else(|| Error::Invalid("the preview needs its document".into()))?;
            let path = editor.path();
            let project = project_for(&path)?;
            return match preview_files(&project, &path, &editor.document()?.text, id, known) {
                Some(files) => match host::storage::get(RENDERER_KEY).ok().flatten().filter(|renderer| renderer == "redoc") {
                    Some(renderer) => crate::ui_group::post_view_message(view, &format!("{{\"renderer\":\"{renderer}\",{}", &files[1..])),
                    None => crate::ui_group::post_view_message(view, &files),
                },
                None => Ok(()),
            };
        }

        match event {
            ViewEvent::Activate(key) if view == COMPARE => {
                let comparison = self.comparison.as_ref().ok_or(Error::Stale)?;
                if !comparison.current(ctx) || !comparison.items.iter().any(|item| item.key == key) {refresh_comparison();return Err(Error::Stale);}
                let mut parts = key.splitn(3, ':');
                let (Some(line), Some(column), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                    return Ok(());
                };
                let (Ok(line), Ok(column)) = (line.parse(), column.parse()) else {
                    return Ok(());
                };
                comparison_owner(ctx, path)?;
                if path != comparison.path && (path != comparison.base_path || ctx.project().ok_or(Error::Stale)?.read_file(path)? != comparison.base) {
                    refresh_comparison();return Err(Error::Stale);
                }
                host::open_location(path, line, column)
            }
            ViewEvent::Button(button) if button == "refresh" => {
                crate::ui_group::refresh_view(ENDPOINTS);
                Ok(())
            }
            event => match self.route_list().handle(&event) {
                RouteEvent::Changed => {
                    crate::ui_group::refresh_view(ENDPOINTS);
                    Ok(())
                }
                RouteEvent::Open(route) => {
                    let Some(location) = route.location else {
                        return Ok(());
                    };
                    let start = location.range.start;
                    host::open_location(&location.path, start.line, start.character)
                }
                RouteEvent::Ignored => Ok(()),
            },
        }
    }
}

impl openidea_plugin::diagnostics::DiagnosticsProvider for OpenApi {
    fn diagnostics(&mut self, doc: &Document) -> Vec<Diagnostic> {
        if doc.language != "json" { return Vec::new(); }
        structure::diagnostics(&doc.text)
    }
}

impl openidea_plugin::hover::HoverProvider for OpenApi {
    fn hover(&mut self, doc: &Document, at: Position) -> Option<openidea_plugin::hover::Hover> {
        hover::hover(doc, at)
    }
}

register_plugin!(OpenApi, providers, views, endpoints, diagnostics, hover, file_actions, search, hover_actions);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;
    use openidea_plugin::providers::{CompletionKind, Document, Providers};
    use openidea_plugin::text::position;
    use openidea_plugin::{NotificationLevel, Position, ProcessOutput, Range};
    use spec::PETSTORE;

    fn doc(text: &str) -> Document {
        Document { path: "/project/api.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str, plus: usize) -> Position {
        position(text, text.find(needle).unwrap() + plus)
    }

    fn writing_host() -> MockHost {
        MockHost::install().with_capabilities(&["editor.read", "project.read", "project.write", "clipboard"])
    }

    fn generated(path: &str) -> String { project_for(path).unwrap().read_file(path).unwrap() }

    #[test]
    fn writes_the_requests_beside_the_spec_and_opens_them() {
        let host = writing_host();
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();

        assert!(generated("/project/api.http").starts_with("### listPets\n"));
        assert!(host.process_runs().is_empty());
        assert_eq!(host.opened_locations()[0].0, "/project/api.http");
        assert!(host.clipboard().is_none());
    }

    #[test]
    fn an_existing_http_file_is_replaced_only_after_confirming() {
        let host = writing_host();
        host.add_file("/project/api.http", "old");
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert_eq!(generated("/project/api.http"), "old");

        host.answer_next_prompt(Some(&[]));
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert!(generated("/project/api.http").starts_with("### listPets\n"));
    }

    #[test]
    fn copies_the_requests_when_project_write_is_denied_and_explains_without_clipboard() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read", "clipboard"]);
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert!(host.clipboard().unwrap().starts_with("### listPets\n"));

        let host = MockHost::install().with_capabilities(&["editor.read"]);
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert_eq!(host.notifications()[0].level, NotificationLevel::Warning);

        host.open_editor("/project/k8s.yaml", "yaml", "kind: Pod\n", &[(0, 0, 0, 0)]);
        assert!(OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).is_err());
    }

    #[test]
    fn native_export_refuses_dirty_or_noncanonical_targets_without_fallback() {
        let host = writing_host();
        host.add_file("/project/api.http", "saved");
        host.open_editor("/project/api.http", "http", "unsaved", &[(0, 0, 0, 0)]);
        host.answer_next_prompt(Some(&[]));
        assert!(matches!(save_or_copy("/project/api.http", "replacement", "Requests"), Err(Error::Invalid(_))));
        assert!(host.clipboard().is_none());
        assert!(host.process_runs().is_empty());
        assert!(matches!(save_or_copy("/project/../outside.http", "replacement", "Requests"), Err(Error::Invalid(_))));
    }

    #[test]
    fn resolves_relative_paths() {
        assert_eq!(resolve_path("/p/api/v1/openapi.yaml", "../common/pet.yaml"), "/p/api/common/pet.yaml");
        assert_eq!(resolve_path("/p/api.yaml", "./pet.yaml"), "/p/pet.yaml");
        assert_eq!(resolve_path("/p/api.yaml", "/abs/x.yaml"), "/abs/x.yaml");
    }

    #[test]
    fn publishes_problems_when_a_spec_is_opened() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.add_file("/project/api.yaml", "openapi: 3.0.0\npaths: {}\nx:\n  $ref: '#/nope'\n");
        OpenApi::default().handle_events(&[Event::FileOpened("/project/api.yaml".into())], &host.invocation());
        assert_eq!(host.diagnostics()["/project/api.yaml"][0].code.as_deref(), Some("unresolved-ref"));
    }

    #[test]
    fn clears_problems_of_plain_yaml() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.add_file("/project/k8s.yaml", "kind: Pod\nx:\n  $ref: '#/nope'\n");
        OpenApi::default().handle_events(&[Event::FileSaved("/project/k8s.yaml".into())], &host.invocation());
        assert!(host.diagnostics().get("/project/k8s.yaml").is_none_or(Vec::is_empty));
    }

    #[test]
    fn completes_local_refs_inside_the_quotes() {
        let _host = MockHost::install();
        let text = PETSTORE.replace("$ref: '#/components/responses/Pet'", "$ref: '#/comp");
        let from = text.find("#/comp\n").unwrap();
        let items = OpenApi::default().completion(&doc(&text), position(&text, from + 6), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"#/components/schemas/Pet") && labels.contains(&"#/components/responses/Pet"));
        assert_eq!(items[0].replace, Some(spring_text::range(&text, from, from + 6)));
        assert_eq!(items[0].kind, Some(CompletionKind::Reference));
    }

    #[test]
    fn the_replace_range_stops_before_an_auto_inserted_closing_quote() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: '#/comp'\n";
        let items = OpenApi::default().completion(&doc(text), at(text, "#/comp", 6), None);
        let from = text.find("#/comp").unwrap();
        assert_eq!(items[0].replace, Some(spring_text::range(text, from, from + 6)));
    }

    #[test]
    fn completes_pointers_into_the_named_file() {
        let host = MockHost::install();
        host.add_file("/project/common.yaml", "Pet:\n  type: object\n");
        let text = "openapi: 3.0.0\npaths: {}\nx:\n  $ref: common.yaml#/\n";
        let items = OpenApi::default().completion(&doc(text), at(text, "#/\n", 2), None);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["common.yaml#/Pet"]);
    }

    #[test]
    fn nothing_outside_openapi_files() {
        let _host = MockHost::install();
        let plain = "kind: Pod\nx:\n  $ref: '#/\n";
        assert!(OpenApi::default().completion(&doc(plain), at(plain, "#/", 2), None).is_empty());
        assert!(OpenApi::default().code_actions(&doc(plain), Range::default()).is_empty());
        let outside_a_ref = OpenApi::default().completion(&doc(PETSTORE), at(PETSTORE, "title", 0), None);
        assert!(outside_a_ref.iter().all(|i| i.kind != Some(CompletionKind::Reference)));
    }

    #[test]
    fn goes_to_local_and_file_targets() {
        let host = MockHost::install();
        let local = OpenApi::default().definition(&doc(PETSTORE), at(PETSTORE, "#/components/schemas/Pet", 5));
        assert_eq!((local[0].path.as_str(), local[0].range.start), ("/project/api.yaml", Position::new(36, 4)));
        host.add_file("/project/common.yaml", "Other:\n  type: string\nPet:\n  type: object\n");
        let text = "openapi: 3.0.0\npaths: {}\na:\n  $ref: 'common.yaml#/Pet'\nb:\n  $ref: common.yaml\n";
        let file = OpenApi::default().definition(&doc(text), at(text, "common.yaml#", 1));
        assert_eq!((file[0].path.as_str(), file[0].range.start.line), ("/project/common.yaml", 2));
        let whole = OpenApi::default().definition(&doc(text), at(text, "common.yaml\n", 1));
        assert_eq!((whole[0].path.as_str(), whole[0].range.start.line), ("/project/common.yaml", 0));
    }

    #[test]
    fn offers_the_schema_modeline_once() {
        let _host = MockHost::install();
        let actions = OpenApi::default().code_actions(&doc(PETSTORE), Range::default());
        assert_eq!(actions[0].edits[0].1[0].text, "# yaml-language-server: $schema=https://spec.openapis.org/oas/3.0/schema/2024-10-18\n");
        let with = format!("{}{PETSTORE}", actions[0].edits[0].1[0].text);
        assert!(OpenApi::default().code_actions(&doc(&with), Range::default()).is_empty());
        assert_eq!(spec::schema_url("2.0"), Some("https://spec.openapis.org/oas/2.0/schema/2017-08-27"));
        assert_eq!(spec::schema_url("3.1.1"), Some("https://spec.openapis.org/oas/3.1/schema/2025-11-23"));
        assert_eq!(spec::schema_url("3.2.0"), Some("https://spec.openapis.org/oas/3.2/schema/2025-11-23"));
        assert_eq!(spec::schema_url("4.0.0"), None);
    }

    #[test]
    fn endpoints_list_operations_flat_with_their_tag() {
        let host = MockHost::install();
        let tagged = PETSTORE.replace("      operationId: listPets\n", "      operationId: listPets\n      tags:\n        - Pets\n");
        host.add_file("/project/api/openapi.yaml", &tagged);
        host.add_file("/project/k8s.yml", "kind: Pod\n");
        let mut plugin = OpenApi::default();
        let view = plugin.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["/pets", "/pets/{petId}", "/pets/{petId}"]);
        assert_eq!(items[0].detail.as_deref(), Some("[GET] Pets@listPets"));
        plugin.handle(ENDPOINTS, ViewEvent::Activate(items[0].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/api/openapi.yaml".to_owned(), 8, 4)]);
        assert_eq!(items[0].icon.as_deref(), Some("web"));
        assert_eq!(items[2].icon.as_deref(), Some("web"));
    }

    #[test]
    fn operations_panel_lists_the_spec_operations_and_opens_one() {
        let host = MockHost::install();
        let tagged = PETSTORE.replace("      operationId: listPets\n", "      operationId: listPets\n      tags:\n        - Pets\n");
        host.add_file("/project/api/openapi.yaml", &tagged);
        let mut plugin = OpenApi::default();
        let ask = r#"{"op":"outline","id":"f1","path":"/project/api/openapi.yaml"}"#;
        plugin.handle(OPERATIONS_PANEL, ViewEvent::Message(ask.into()), &host.invocation()).unwrap();
        let (view, reply) = host.view_messages().pop().unwrap();
        let reply: serde_json::Value = serde_json::from_str(&reply).unwrap();
        assert_eq!((view.as_str(), reply["id"].as_str(), reply["path"].as_str()), (OPERATIONS_PANEL, Some("f1"), Some("/project/api/openapi.yaml")));
        let operations = reply["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 3);
        assert_eq!(operations[0], serde_json::json!({"method":"GET","path":"/pets","tag":"Pets","operationId":"listPets","line":8,"column":4,"endLine":19}));
        assert_eq!((operations[1]["line"].as_u64(), operations[1]["endLine"].as_u64()), (Some(23), Some(27)));
        assert_eq!(operations[2]["method"], "DELETE");
        assert_eq!((operations[2]["line"].as_u64(), operations[2]["endLine"].as_u64()), (Some(28), Some(35)));
        let open = r#"{"op":"open","path":"/project/api/openapi.yaml","line":8,"column":4}"#;
        plugin.handle(OPERATIONS_PANEL, ViewEvent::Message(open.into()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/api/openapi.yaml".to_owned(), 8, 4)]);
    }

    #[test]
    fn operations_panel_explains_plain_files_and_refuses_bad_requests() {
        let host = MockHost::install();
        host.add_file("/project/k8s.yaml", "kind: Pod\n");
        let mut plugin = OpenApi::default();
        let ask = r#"{"op":"outline","id":"f1","path":"/project/k8s.yaml"}"#;
        plugin.handle(OPERATIONS_PANEL, ViewEvent::Message(ask.into()), &host.invocation()).unwrap();
        let reply: serde_json::Value = serde_json::from_str(&host.view_messages()[0].1).unwrap();
        assert_eq!(reply["message"], "Not an OpenAPI document.");
        assert!(reply.get("operations").is_none());
        for bad in [r#"{"op":"open","path":"/project/../etc/a.yaml","line":1}"#, r#"{"op":"open","path":"/project/a.txt","line":1}"#, r#"{"op":"open","path":"/project/k8s.yaml"}"#, "nope"] {
            assert!(plugin.handle(OPERATIONS_PANEL, ViewEvent::Message(bad.into()), &host.invocation()).is_err(), "{bad}");
        }
        assert!(host.opened_locations().is_empty());
    }

    #[test]
    fn spec_context_follows_the_focused_editor() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read"]);
        let mut plugin = OpenApi::default();
        host.open_editor("/project/api/openapi.yaml", "yaml", PETSTORE, &[]);
        plugin.handle_events(&[Event::EditorFocused(Some("/project/api/openapi.yaml".into()))], &host.invocation());
        assert_eq!(host.context_keys().get(SPEC_CONTEXT), Some(&openidea_plugin::ContextValue::from(true)));
        host.open_editor("/project/k8s.yaml", "yaml", "kind: Pod\n", &[]);
        plugin.handle_events(&[Event::EditorFocused(Some("/project/k8s.yaml".into()))], &host.invocation());
        assert_eq!(host.context_keys().get(SPEC_CONTEXT), Some(&openidea_plugin::ContextValue::from(false)));
    }

    #[test]
    fn create_route_http_writes_beside_the_spec() {
        let host = MockHost::install().with_capabilities(&["project.read", "project.write", "clipboard"]);
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        let mut openapi = OpenApi::default();
        let view = openapi.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        host.answer_next_prompt(Some(&["http://localhost:8080"]));
        openapi.run_command("create-route-http", &host.invocation(), Some(&items[0].key)).unwrap();
        assert_eq!(host::projects().unwrap()[0].read_file("api/get-pets.http").unwrap(), "### GET /pets\nGET http://localhost:8080/pets\n");
        assert_eq!(route_base(openapi.route_list().route(&items[0].key).unwrap()), "https://api.example.com/v1");
        assert!(host.process_runs().is_empty());
    }

    #[test]
    fn copy_route_curl_copies_with_the_prompted_base() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        let mut openapi = OpenApi::default();
        let view = openapi.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        openapi.run_command("copy-route-curl", &host.invocation(), Some(&items[0].key)).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:9000/pets'"));
    }

    #[test]
    fn copy_route_curl_from_the_palette_picks_an_endpoint() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        host.answer_next_prompt(Some(&["0"]));
        host.answer_next_prompt(Some(&["http://localhost:9000"]));
        OpenApi::default().run_command("copy-route-curl", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().as_deref(), Some("curl -X GET 'http://localhost:9000/pets'"));
    }

    #[test]
    fn copy_route_curl_from_the_palette_does_nothing_when_the_pick_is_dismissed() {
        let host = MockHost::install().with_capabilities(&["project.read", "clipboard"]);
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        assert_eq!(OpenApi::default().run_command("copy-route-curl", &host.invocation(), None).unwrap(), None);
        assert_eq!(host.clipboard(), None);
    }

    #[test]
    fn route_commands_reject_an_unknown_row() {
        let host = MockHost::install();
        let result = OpenApi::default().run_command("copy-route-curl", &host.invocation(), Some("nope"));
        assert!(matches!(result, Err(Error::Invalid(_))));
    }

    #[test]
    fn first_tag_is_read_from_json_and_flow_style_specs() {
        let host = MockHost::install();
        let flow = PETSTORE.replace("      operationId: listPets\n", "      operationId: listPets\n      tags: [Pets]\n");
        host.add_file("/project/a/openapi.yaml", &flow);
        let json = r#"{"openapi":"3.0.0","info":{"title":"t","version":"1"},"paths":{"/pets":{"get":{"operationId":"listPets","tags":["Pets"],"responses":{"200":{"description":"ok"}}}}}}"#;
        host.add_file("/project/b/openapi.json", json);
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        let tagged: Vec<_> = items.iter().filter(|i| i.detail.as_deref() == Some("[GET] Pets@listPets")).collect();
        assert_eq!(tagged.len(), 2, "{:?}", items.iter().map(|i| &i.detail).collect::<Vec<_>>());
    }

    #[test]
    fn untagged_operations_group_under_no_tag() {
        let host = MockHost::install();
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(items.iter().map(|i| i.detail.as_deref().unwrap()).collect::<Vec<_>>(), ["[GET] (no tag)@listPets", "[GET] (no tag)@showPet", "[DELETE] (no tag)@deletePet"]);
    }

    #[test]
    fn open_goes_to_the_operation_in_the_spec() {
        let host = MockHost::install();
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        let mut openapi = OpenApi::default();
        let view = openapi.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        openapi.handle(ENDPOINTS, ViewEvent::Activate(items[1].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/api/openapi.yaml".to_owned(), 21, 4)]);
        openapi.handle(ENDPOINTS, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), [ENDPOINTS]);
    }

    fn prepared_comparison() -> (MockHost, OpenApi) {
        let host=MockHost::install();
        let revision=PETSTORE.replace("listPets","changedPets");
        host.add_file("/project/spec/base.yaml",PETSTORE);
        host.add_file("/project/spec/api.yaml",&revision);
        host.open_editor("/project/spec/api.yaml","yaml",&revision,&[(0,0,0,0)]);
        host.answer_next_prompt(Some(&["/project/spec/base.yaml"]));
        let mut plugin=OpenApi::default();plugin.run_command("compare-specs",&host.invocation(),None).unwrap();
        (host,plugin)
    }
    #[test]
    fn exports_current_comparison_markdown_and_rejects_stale_source() {
        let (host, mut plugin) = prepared_comparison();
        plugin.run_command("export-comparison", &host.invocation(), None).unwrap();
        let report = host.clipboard().unwrap();
        assert!(report.starts_with("# OpenAPI comparison\n"));
        assert!(report.contains("/project/spec/base.yaml") && report.contains("GET /pets"));
        host.type_in_editor("changed source");
        assert!(plugin.run_command("export-comparison", &host.invocation(), None).is_err());
        assert_eq!(host.clipboard().unwrap(), report);
        let (host, mut plugin) = prepared_comparison();
        host.add_file("/project/spec/base.yaml", "changed baseline");
        assert!(plugin.run_command("export-comparison", &host.invocation(), None).is_err());
    }
    fn comparison_messages(host: &MockHost) -> usize {
        host.view_messages().iter().filter(|(view,raw)| view==ENDPOINTS && serde_json::from_str::<serde_json::Value>(raw).is_ok_and(|message| message["channel"]=="ui-kit-refresh" && message["targetView"]==COMPARE)).count()
    }
    #[test]
    fn external_comparison_replacement_notifies_page_and_render_never_notifies_or_fetches() {
        let (host,mut plugin)=prepared_comparison();
        assert_eq!(comparison_messages(&host),1);
        host.on_fetch(|_|panic!("comparison render cannot fetch"));
        for _ in 0..3 {assert!(matches!(plugin.render(COMPARE,None,&host.invocation()).unwrap().body,Body::List(_)));}
        assert_eq!(comparison_messages(&host),1);
        host.answer_next_prompt(Some(&["/project/spec/base.yaml"]));
        plugin.run_command("compare-specs",&host.invocation(),None).unwrap();
        assert_eq!(comparison_messages(&host),2);
        assert!(host.requests().is_empty());
    }
    #[test]
    fn foreign_missing_denied_nested_owner_and_unsaved_revision_replace_rows_with_quiet_model() {
        for case in 0..5 {
            let (host,mut plugin)=prepared_comparison();
            let host=match case {
                0=>{host.add_project("/project");host.set_project_root(Some("/foreign"));host},
                1=>{host.set_project_root(None);host},
                2=>host.with_capabilities(&["editor.read"]),
                3=>{host.add_project("/project/spec");host},
                _=>{host.open_editor("/project/spec/api.yaml","yaml","openapi: 3.0.0\n#unsaved",&[(0,0,0,0)]);host},
            };
            assert!(matches!(plugin.render(COMPARE,None,&host.invocation()).unwrap().body,Body::Message(_)),"case{case}");
            assert!(host.opened_locations().is_empty());
            assert_eq!(comparison_messages(&host),1,"render must not invalidate itself");
        }
    }
    #[test]
    fn source_save_clears_comparison_and_invalidates_but_unrelated_save_does_not() {
        let (host,mut plugin)=prepared_comparison();
        plugin.handle_events(&[Event::FileSaved("/project/unrelated.yaml".into())],&host.invocation());
        assert!(plugin.comparison.is_some());assert_eq!(comparison_messages(&host),1);
        plugin.handle_events(&[Event::FileSaved("/project/spec/base.yaml".into())],&host.invocation());
        assert!(plugin.comparison.is_none());assert_eq!(comparison_messages(&host),2);
        assert!(matches!(plugin.render(COMPARE,None,&host.invocation()).unwrap().body,Body::Message(_)));
        assert!(host.requests().is_empty());
    }
    #[test]
    fn comparison_actions_require_owned_current_member_and_reject_forged_location() {
        let (host,mut plugin)=prepared_comparison();
        for key in ["0:0:/project/other.yaml","0:0:/foreign/secret.yaml"] {
            assert!(plugin.handle(COMPARE,ViewEvent::Activate(key.into()),&host.invocation()).is_err());
        }
        assert!(host.opened_locations().is_empty());
        let key=plugin.comparison.as_ref().unwrap().items[0].key.clone();
        host.open_editor("/project/spec/api.yaml","yaml","openapi: 3.0.0\n#changed",&[(0,0,0,0)]);
        assert!(plugin.handle(COMPARE,ViewEvent::Activate(key),&host.invocation()).is_err());
        assert!(host.opened_locations().is_empty());
    }
    #[test]
    fn comparison_focus_event_refreshes_current_frame_and_version_only_change_withholds_rows() {
        let (host,mut plugin)=prepared_comparison();
        host.type_in_editor(""); // Same bytes, new version still invalidates the captured source.
        plugin.handle_events(&[Event::EditorFocused(Some("/project/spec/api.yaml".into()))],&host.invocation());
        assert_eq!(comparison_messages(&host),2);
        assert!(matches!(plugin.render(COMPARE,None,&host.invocation()).unwrap().body,Body::Message(_)));
        assert!(plugin.comparison.is_some(),"focus in one window must not delete another window's owned snapshot");
    }
    #[test]
    fn removed_source_event_retires_comparison_and_unrelated_paths_do_not() {
        let (host,mut plugin)=prepared_comparison();
        plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged {removed:vec!["/project/spec/api.yaml".into()],..Default::default()})],&host.invocation());
        assert!(plugin.comparison.is_none());assert_eq!(comparison_messages(&host),2);
    }
    #[test]
    fn source_parent_directory_removal_retires_capture_without_reading_old_editor_bytes() {
        let (host,mut plugin)=prepared_comparison();
        plugin.handle_events(&[Event::FilesChanged(openidea_plugin::FilesChanged {removed:vec!["/project/spec".into()],..Default::default()})],&host.invocation());
        assert!(plugin.comparison.is_none());assert_eq!(comparison_messages(&host),2);
        assert!(matches!(plugin.render(COMPARE,None,&host.invocation()).unwrap().body,Body::Message(_)));
    }
    #[test]
    fn stale_local_base_cannot_navigate_removed_row_to_changed_document() {
        let (host,mut plugin)=prepared_comparison();
        let revision=PETSTORE.replace("listPets","changedPets");
        let base=format!("{PETSTORE}\ncomponents:\n  schemas:\n    Removed:\n      type: string\n");
        host.add_file("/project/spec/base.yaml",&base);
        plugin.show_comparison(&host.invocation(),"/project/spec/base.yaml",&base,"/project/spec/api.yaml",&revision,host.editor_version()).unwrap();
        let key=plugin.comparison.as_ref().unwrap().items.iter().find(|item|item.label.starts_with("- ")).unwrap().key.clone();
        host.add_file("/project/spec/base.yaml",PETSTORE);
        assert!(plugin.handle(COMPARE,ViewEvent::Activate(key),&host.invocation()).is_err());
        assert!(host.opened_locations().is_empty());
    }
    #[test]
    fn comparison_source_and_visible_row_bounds_are_explicit_and_overflow_preserves_old_capture() {
        let (host,mut plugin)=prepared_comparison();
        let revision=PETSTORE.replace("listPets","changedPets");
        let oversized="x".repeat(MAX_SCANNED_FILE_BYTES+1);
        assert!(plugin.show_comparison(&host.invocation(),"/project/spec/base.yaml",&oversized,"/project/spec/api.yaml",&revision,host.editor_version()).is_err());
        assert_eq!(comparison_messages(&host),1);
        let mut many=PETSTORE.to_owned();
        for index in 0..1200 {many.push_str(&format!("x-extra-{index}: added\n"));}
        host.open_editor("/project/spec/api.yaml","yaml",&many,&[(0,0,0,0)]);
        plugin.show_comparison(&host.invocation(),"/project/spec/base.yaml",PETSTORE,"/project/spec/api.yaml",&many,host.editor_version()).unwrap();
        let shown=plugin.render(COMPARE,None,&host.invocation()).unwrap();
        let Body::List(items)=shown.body else {panic!("expected bounded rows")};
        assert_eq!(items.len(),1000);assert!(shown.status.unwrap().contains("first 1,000"));
    }
    #[test]
    fn forged_picker_cannot_replace_owned_comparison_with_foreign_source() {
        let (host,mut plugin)=prepared_comparison();
        host.add_file("/foreign/secret.yaml",PETSTORE);
        host.answer_next_prompt(Some(&["/foreign/secret.yaml"]));
        assert!(plugin.run_command("compare-specs",&host.invocation(),None).is_err());
        assert_eq!(plugin.comparison.as_ref().unwrap().base_path,"/project/spec/base.yaml");
        assert_eq!(comparison_messages(&host),1);
    }
    #[test]
    fn compares_with_the_picked_spec_and_opens_a_change() {
        let host = MockHost::install();
        let revision = PETSTORE.replace("listPets", "listAllPets");
        host.add_file("/project/old/openapi.yaml", PETSTORE);
        host.add_file("/project/api.yaml", &revision);
        host.open_editor("/project/api.yaml", "yaml", &revision, &[(0, 0, 0, 0)]);
        let mut openapi = OpenApi::default();

        host.answer_next_prompt(Some(&["/project/old/openapi.yaml"]));
        openapi.run_command("compare-specs", &host.invocation(), None).unwrap();
        assert_eq!(host.revealed_views(), [ENDPOINTS]);
        assert_eq!(ui_kit::web::active_view(ENDPOINTS,include_str!("ui_config.json")).unwrap(),COMPARE);
        let view = openapi.render(COMPARE, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(items[0].label, "~ GET /pets");
        openapi.handle(COMPARE, ViewEvent::Activate(items[0].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations()[0].0, "/project/api.yaml");
    }

    fn open_changed_petstore(host: &MockHost) -> Invocation<'_> {
        let revision = PETSTORE.replace("listPets", "listAllPets");
        host.add_file("/project/api/openapi.yaml", &revision);
        host.open_editor("/project/api/openapi.yaml", "yaml", &revision, &[(0, 0, 0, 0)]);
        host.invocation()
    }

    #[test]
    fn compares_with_the_version_in_head() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "process:git"]);
        host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(0), stdout: PETSTORE.into(), stderr: String::new() }));
        let invocation = open_changed_petstore(&host);
        let mut openapi = OpenApi::default();

        openapi.run_command("compare-last-revision", &invocation, None).unwrap();

        assert_eq!(host.process_runs()[0].args, ["show", "HEAD:./api/openapi.yaml"]);
        let Body::List(items) = openapi.render(COMPARE, None, &invocation).unwrap().body else { panic!("expected a list") };
        assert_eq!(items[0].label, "~ GET /pets");
    }

    #[test]
    fn an_untracked_file_has_nothing_to_compare_with() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "process:git"]);
        host.on_process("git", |_, _| Ok(ProcessOutput { status: Some(128), stdout: String::new(), stderr: "fatal".into() }));
        let invocation = open_changed_petstore(&host);

        OpenApi::default().run_command("compare-last-revision", &invocation, None).unwrap();

        assert_eq!(host.notifications()[0].level, NotificationLevel::Info);
        assert!(host.revealed_views().is_empty());
    }

    #[test]
    fn comparing_with_the_last_revision_denied_names_permission() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read"]);
        let invocation = open_changed_petstore(&host);

        OpenApi::default().run_command("compare-last-revision", &invocation, None).unwrap();

        let notification = &host.notifications()[0];
        assert_eq!(notification.level, NotificationLevel::Warning);
        assert!(notification.body.contains("needs the process:git permission"), "{}", notification.body);
        assert!(notification.body.contains("Settings › Plugins › OpenAPI"));
        assert!(host.revealed_views().is_empty());
    }

    #[test]
    fn dismissing_the_second_spec_chooser_compares_nothing() {
        let host = MockHost::install();
        host.add_file("/project/old/openapi.yaml", PETSTORE);
        host.add_file("/project/api.yaml", PETSTORE);
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        let mut openapi = OpenApi::default();

        host.answer_next_prompt(None);
        openapi.run_command("compare-specs", &host.invocation(), None).unwrap();
        assert!(host.revealed_views().is_empty());
        assert!(openapi.comparison.is_none());
    }

    #[test]
    fn says_so_when_no_spec_is_found() {
        let host = MockHost::install();
        host.add_file("/project/k8s.yaml", "kind: Pod\n");
        assert!(matches!(OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap().body, Body::Message(_)));
    }

    #[test]
    fn quotes_a_ref_completed_without_quotes() {
        let _host = MockHost::install();
        let block = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: \n";
        let items = OpenApi::default().completion(&doc(block), at(block, "$ref: ", 6), None);
        assert_eq!(items[0].insert_text, "'#/components/schemas/Pet'");
        assert_eq!(items[0].filter_text.as_deref(), Some("#/components/schemas/Pet"));

        let flow = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx: { $ref: }\n";
        let items = OpenApi::default().completion(&doc(flow), at(flow, "$ref: ", 6), None);
        assert_eq!(items[0].insert_text, "'#/components/schemas/Pet'");

        let unquoted = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: #/c\n";
        let items = OpenApi::default().completion(&doc(unquoted), at(unquoted, "#/c\n", 3), None);
        assert_eq!(items[0].insert_text, "'#/components/schemas/Pet'");

        let quoted = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: '#/comp\n";
        let items = OpenApi::default().completion(&doc(quoted), at(quoted, "#/comp", 6), None);
        assert_eq!(items[0].insert_text, "#/components/schemas/Pet");
    }

    #[test]
    fn only_a_missing_file_is_reported_missing() {
        let invalid = |m: &str| Err(Error::Invalid(m.into()));
        assert!(matches!(fetch(invalid("/p/x.yaml: no such file")), Fetched::Missing));
        assert!(matches!(fetch(Err(Error::NotFound("x".into()))), Fetched::Missing));
        assert!(matches!(fetch(invalid("/etc/x.yaml: access outside the project root is not allowed")), Fetched::Unreadable));
        assert!(matches!(fetch(Err(Error::Failed("boom".into()))), Fetched::Unreadable));
        assert!(matches!(fetch(Ok("a: 1".into())), Fetched::Text(_)));
    }

    const JSON_SPEC: &str = r##"{
  "openapi": "3.0.0",
  "paths": {
    "/pets": {
      "get": {
        "operationId": "a",
        "responses": {
          "200": { "$ref": "#/components/schemas/Pet" },
          "404": { "$ref": "#/components/schemas/Nope" },
          "500": { "$ref": "pet.json#/Missing" },
          "501": { "$ref": "pet.json#/Pet" }
        }
      },
      "post": { "operationId": "a" }
    }
  },
  "components": { "schemas": { "Pet": { "type": "object" } } }
}
"##;

    fn line_of(text: &str, needle: &str) -> u32 {
        position(text, text.find(needle).unwrap()).line
    }

    #[test]
    fn json_ref_resolves_and_reports_unresolved() {
        let host = MockHost::install();
        host.add_file("/project/api.json", JSON_SPEC);
        host.add_file("/project/pet.json", "{\"Pet\": {\"type\": \"object\"}}");
        OpenApi::default().handle_events(&[Event::FileOpened("/project/api.json".into())], &host.invocation());

        let mut found: Vec<_> =
            host.diagnostics()["/project/api.json"].iter().map(|d| (d.range.start.line, d.code.clone().unwrap())).collect();
        found.sort();
        let (dup, unresolved) = ("duplicate-operation-id".to_owned(), "unresolved-ref".to_owned());
        assert_eq!(
            found,
            [
                (line_of(JSON_SPEC, "\"operationId\": \"a\",\n"), dup.clone()),
                (line_of(JSON_SPEC, "Nope"), unresolved.clone()),
                (line_of(JSON_SPEC, "pet.json#/Missing"), unresolved),
                (line_of(JSON_SPEC, "\"post\""), dup),
            ]
        );

        let json_doc = Document { path: "/project/api.json".into(), language: "json".into(), version: 1, text: JSON_SPEC.into() };
        let target = OpenApi::default().definition(&json_doc, at(JSON_SPEC, "#/components/schemas/Pet", 5));
        assert_eq!(target[0].range.start, position(JSON_SPEC, JSON_SPEC.find("\"Pet\": {").unwrap() + 1));
        let file = OpenApi::default().definition(&json_doc, at(JSON_SPEC, "pet.json#/Pet", 1));
        assert_eq!((file[0].path.as_str(), file[0].range.start.line), ("/project/pet.json", 0));

        let typing = JSON_SPEC.replace("\"#/components/schemas/Pet\"", "\"#/comp\"");
        let items = OpenApi::default().completion(&Document { text: typing.clone(), ..json_doc }, at(&typing, "#/comp", 6), None);
        assert!(items.iter().any(|i| i.label == "#/components/schemas/Pet"));
    }

    #[test]
    fn ref_file_completion_uses_cached_file_list() {
        let host = MockHost::install();
        host.add_file("/project/api.yaml", "openapi: 3.0.0\nx:\n  $ref: './sc\n");
        host.add_file("/project/schemas/pet.yaml", "Pet: {}\n");
        host.add_file("/project/schemas/readme.md", "x");
        host.add_file("/project/shared/err.json", "{}");
        let mut plugin = OpenApi::default();
        plugin.handle_events(&[Event::FileOpened("/project/api.yaml".into())], &host.invocation());
        host.add_file("/project/schemas/late.yaml", "Late: {}\n");

        let text = "openapi: 3.0.0\nx:\n  $ref: './sc\n";
        let items = plugin.completion(&doc(text), at(text, "./sc", 4), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["./schemas/pet.yaml", "./shared/err.json"]);
        assert_eq!(items[0].replace, Some(spring_text::range(text, text.find("./sc").unwrap(), text.find("./sc").unwrap() + 4)));

        assert!(OpenApi::default().completion(&doc(text), at(text, "./sc", 4), None).is_empty());
    }

    const PREVIEW_TEXT: &str = "openapi: 3.0.0\nx:\n  $ref: './schemas/pet.yaml#/Pet'\ny:\n  $ref: 'gone.yaml'\nz:\n  $ref: '#/x'\n";

    fn request(hash: &str, id: &str) -> ViewEvent {
        ViewEvent::Message(format!("files\t{id}\t{hash}"))
    }

    #[test]
    fn preview_remembers_the_chosen_renderer() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "storage"]);
        host.open_editor("/project/api.yaml", "yaml", "openapi: 3.0.0\n", &[(0, 0, 0, 0)]);
        let mut openapi = OpenApi::default();
        openapi.handle("preview", ViewEvent::Message("renderer\tredoc".into()), &host.invocation()).unwrap();
        openapi.handle("preview", request("", "a"), &host.invocation()).unwrap();
        assert!(host.view_messages()[0].1.starts_with("{\"renderer\":\"redoc\",\"id\":\"a\""), "{:?}", host.view_messages());
        openapi.handle("preview", ViewEvent::Message("renderer\tswagger".into()), &host.invocation()).unwrap();
        openapi.handle("preview", request("", "b"), &host.invocation()).unwrap();
        assert!(!host.view_messages()[1].1.contains("renderer"));
    }

    #[test]
    fn preview_receives_referenced_files() {
        let host = MockHost::install();
        host.open_editor("/project/api.yaml", "yaml", PREVIEW_TEXT, &[(0, 0, 0, 0)]);
        host.add_file("/project/schemas/pet.yaml", "Pet:\n  type: \"object\"\n");
        let mut plugin = OpenApi::default();
        plugin.handle("preview", request("", "a"), &host.invocation()).unwrap();

        let messages = host.view_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "preview");
        let prefix = r#"{"id":"a","skipped":0,"files":{"./schemas/pet.yaml":"Pet:\n  type: \"object\"\n"},"hash":""#;
        assert!(messages[0].1.starts_with(prefix), "{}", messages[0].1);

        let hash = messages[0].1[prefix.len()..].trim_end_matches("\"}").to_owned();
        plugin.handle("preview", request(&hash, "a"), &host.invocation()).unwrap();
        assert_eq!(host.view_messages().len(), 1);

        host.add_file("/project/schemas/pet.yaml", "Pet: {}\n");
        plugin.handle("preview", request(&hash, "a"), &host.invocation()).unwrap();
        assert_eq!(host.view_messages().len(), 2);
    }

    #[test]
    fn reply_carries_its_document_identity() {
        let host = MockHost::install();
        host.open_editor("/project/api.yaml", "yaml", PREVIEW_TEXT, &[(0, 0, 0, 0)]);
        let mut plugin = OpenApi::default();
        plugin.handle("preview", request("", "first"), &host.invocation()).unwrap();
        plugin.handle("preview", request("", "second"), &host.invocation()).unwrap();

        let messages = host.view_messages();
        assert!(messages[0].1.starts_with(r#"{"id":"first","#));
        assert!(messages[1].1.starts_with(r#"{"id":"second","#));
    }

    #[test]
    fn preview_files_are_capped() {
        let host = MockHost::install();
        let mut text = String::from("openapi: 3.0.0\n");
        for i in 0..53 {
            text.push_str(&format!("a{i}:\n  $ref: 'f{i}.yaml'\n"));
            host.add_file(&format!("/project/f{i}.yaml"), "A: {}\n");
        }
        text.push_str("big:\n  $ref: 'big.yaml'\n");
        host.add_file("/project/big.yaml", &"x".repeat(600 * 1024));
        host.open_editor("/project/api.yaml", "yaml", &text, &[(0, 0, 0, 0)]);
        OpenApi::default().handle("preview", request("", "a"), &host.invocation()).unwrap();

        let message = &host.view_messages()[0].1;
        assert!(message.starts_with(r#"{"id":"a","skipped":4,"#), "{}", &message[..60]);
        assert_eq!(message.matches(".yaml\":\"A: {}").count(), 50);
        assert!(!message.contains("big.yaml"));
    }

    #[test]
    fn relative_path_handles_parent_and_nested() {
        assert_eq!(relative_path("/p/api.yaml", "/p/schemas/pet.yaml"), "./schemas/pet.yaml");
        assert_eq!(relative_path("/p/a/s.yaml", "/p/a/b/c/d.json"), "./b/c/d.json");
        assert_eq!(relative_path("/p/a/s.yaml", "/p/x.yaml"), "../x.yaml");
        assert_eq!(relative_path("/p/api/v1/openapi.yaml", "/p/common/pet.yaml"), "../../common/pet.yaml");
    }

    #[test]
    fn json_ref_positions_survive_multibyte_and_escapes() {
        let host = MockHost::install();
        let text = "{\"description\": \"é \\\"q\\\" \\\\ 日本 😀\", \"é日本\": {\"x\": \"a\\\\b\"}, \"openapi\": \"3.0.0\", \"paths\": {}, \"components\": {\"schemas\": {\"Pet\": {\"$ref\": \"#/components/schemas/Nope\"}, \"日本\": {\"$ref\": \"#/é\"}}}}";
        host.add_file("/project/api.json", text);
        OpenApi::default().handle_events(&[Event::FileOpened("/project/api.json".into())], &host.invocation());

        let found = &host.diagnostics()["/project/api.json"];
        assert_eq!(found.len(), 2);
        for (target, diagnostic) in ["#/components/schemas/Nope", "#/é"].iter().zip(found) {
            let from = text.find(target).unwrap();
            assert_eq!(diagnostic.range, spring_text::range(text, from, from + target.len()));
            assert_eq!(diagnostic.message, format!("Cannot find `{target}`"));
        }
        for r in spec::refs(text) {
            assert_eq!(&text[r.at..r.end], r.target);
        }

        for end in (0..=text.len()).filter(|&e| text.is_char_boundary(e)) {
            let prefix = &text[..end];
            for r in spec::refs(prefix) {
                let _ = &prefix[r.at..r.end];
            }
            if let Some(spec) = spec::parse(prefix) {
                spec::ref_targets(prefix, &spec.keys);
            }
        }
        assert!(text.len() > text.chars().count());
    }

    #[test]
    fn json_refs_with_escaped_slashes_resolve() {
        let host = MockHost::install();
        let text = "{\n  \"openapi\": \"3.0.0\",\n  \"paths\": { \"\\/pets\": { \"get\": { \"operationId\": \"a\", \"responses\": { \"200\": {\n    \"$ref\": \"#\\/components\\/schemas\\/Pet\" } } } } },\n  \"components\": { \"schemas\": { \"Pet\": {}, \"Bad\": { \"$ref\": \"#\\/components\\/schemas\\/Nope\" } } }\n}\n";
        host.add_file("/project/api.json", text);
        OpenApi::default().handle_events(&[Event::FileOpened("/project/api.json".into())], &host.invocation());
        let found = &host.diagnostics()["/project/api.json"];
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].message, "Cannot find `#/components/schemas/Nope`");

        let json_doc = Document { path: "/project/api.json".into(), language: "json".into(), version: 1, text: text.into() };
        let target = OpenApi::default().definition(&json_doc, at(text, "schemas\\/Pet", 10));
        assert_eq!(target.len(), 1);
        let ops = spec::operations(text, &spec::parse(text).unwrap().keys);
        assert_eq!(ops[0].path, "/pets");
    }

    #[test]
    fn non_spec_json_is_ignored() {
        let host = MockHost::install();
        let documents = [
            (
                "/project/package.json",
                "{\n  \"name\": \"x\",\n  \"dependencies\": { \"a\": \"1\" },\n  \"r\": { \"$ref\": \"#/nope\" }\n}\n",
            ),
            ("/project/tsconfig.json", "{ \"compilerOptions\": { \"strict\": true } }"),
            ("/project/nested.json", "{ \"info\": { \"openapi\": \"3.0.0\" }, \"r\": { \"$ref\": \"#/nope\" } }"),
        ];
        for (path, text) in documents {
            host.add_file(path, text);
            OpenApi::default().handle_events(&[Event::FileOpened(path.into())], &host.invocation());
            assert!(host.diagnostics().get(path).is_none_or(Vec::is_empty), "{path}");

            let json_doc = Document { path: path.into(), language: "json".into(), version: 1, text: text.into() };
            let cursor = text.find("#/nope").map_or(1, |i| i + 3);
            assert!(OpenApi::default().completion(&json_doc, position(text, cursor), None).is_empty(), "{path}");
            assert!(OpenApi::default().definition(&json_doc, position(text, cursor)).is_empty(), "{path}");
            assert!(OpenApi::default().code_actions(&json_doc, Range::default()).is_empty(), "{path}");
        }
    }

    #[test]
    fn leaves_the_clipboard_alone_without_operations() {
        let host = MockHost::install();
        host.open_editor("/project/empty.yaml", "yaml", "openapi: 3.0.0\npaths: {}\n", &[(0, 0, 0, 0)]);
        let error = OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap_err();
        assert!(error.to_string().contains("no operations"));
        assert!(host.clipboard().is_none());

        host.open_editor("/project/api.json", "json", "openapi: 3.0.0\n", &[(0, 0, 0, 0)]);
        assert!(OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).is_err());
        assert!(host.clipboard().is_none());
    }

    const FRAGMENT: &str = "paths:\n  /pets:\n    get:\n      operationId: list\n";
    const FRAGMENT_JSON: &str = "{\n  \"paths\": {\n    \"/pets\": { \"get\": {} }\n  }\n}\n";

    fn published(host: &MockHost, path: &str, text: &str) -> Vec<Diagnostic> {
        host.add_file(path, text);
        OpenApi::default().handle_events(&[Event::FileSaved(path.into())], &host.invocation());
        host.diagnostics().get(path).cloned().unwrap_or_default()
    }

    #[test]
    fn yaml_with_paths_but_no_root_key_gets_a_hint() {
        let host = MockHost::install();
        let found = published(&host, "/project/frag.yaml", FRAGMENT);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("missing-openapi-root"));
        assert_eq!(found[0].severity, openidea_plugin::Severity::Info);
        assert_eq!((found[0].range.start.line, found[0].range.start.character), (0, 0));
    }

    #[test]
    fn json_with_paths_but_no_root_key_gets_a_hint() {
        let host = MockHost::install();
        let found = published(&host, "/project/frag.json", FRAGMENT_JSON);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].code.as_deref(), Some("missing-openapi-root"));
        assert_eq!(found[0].range.start.line, 1);
    }

    #[test]
    fn hint_disappears_when_the_root_key_is_added() {
        let host = MockHost::install();
        assert_eq!(published(&host, "/project/frag.yaml", FRAGMENT).len(), 1);
        assert!(published(&host, "/project/frag.yaml", &format!("openapi: 3.0.0\n{FRAGMENT}")).is_empty());
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        published(&host, "/project/frag.yaml", FRAGMENT);
        host.add_file("/project/frag.yaml", &format!("swagger: '2.0'\n{FRAGMENT}"));
        OpenApi::default().handle_events(&[Event::FileSaved("/project/frag.yaml".into())], &host.invocation());
        assert!(host.diagnostics().get("/project/frag.yaml").is_none_or(Vec::is_empty));
    }

    #[test]
    fn spec_candidate_is_quiet_for_library_or_dynamic_name() {
        let host = MockHost::install();
        let quiet = [
            ("/project/a.yaml", "paths:\n  - src\n  - lib\n"),
            ("/project/b.yaml", "paths:\n  src:\n    get: x\n"),
            ("/project/c.yaml", "paths:\n  /x:\n    summary: no method\n"),
            ("/project/d.yaml", "paths:\n  /x: {}\n"),
            (
                "/project/package.json",
                "{\n  \"name\": \"x\",\n  \"paths\": [\"a\"],\n  \"compilerOptions\": { \"paths\": { \"/x\": { \"get\": 1 } } }\n}\n",
            ),
        ];
        for (path, text) in quiet {
            assert!(published(&host, path, text).is_empty(), "{path}");
        }
    }

    #[test]
    fn candidate_hint_ignores_unrelated_yaml() {
        let host = MockHost::install();
        let compose = "services:\n  web:\n    image: nginx\n    volumes:\n      - paths:/data\n";
        let workflow = "name: ci\non:\n  push:\n    paths:\n      - 'src/**'\njobs:\n  t:\n    runs-on: ubuntu-latest\n";
        assert!(published(&host, "/project/docker-compose.yaml", compose).is_empty());
        assert!(published(&host, "/project/.github/ci.yml", workflow).is_empty());
    }

    #[test]
    fn only_root_level_keys_decide_the_hint() {
        let host = MockHost::install();
        let nested_root = "info:\n  openapi: 3.0.0\non:\n  push:\n    paths:\n      /x:\n        get: {}\npaths:\n  /pets:\n    get: {}\n";
        let found = published(&host, "/project/n.yaml", nested_root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].range.start.line, 7);

        let nested_paths = "wrapper:\n  paths:\n    /x:\n      get: {}\n";
        assert!(published(&host, "/project/w.yaml", nested_paths).is_empty());
    }

    #[test]
    fn candidate_hint_skips_files_over_the_size_cap() {
        let host = MockHost::install();
        let padding = format!("# {}\n", "x".repeat(MAX_SCANNED_FILE_BYTES));
        assert!(published(&host, "/project/big.yaml", &format!("{FRAGMENT}{padding}")).is_empty());
        assert_eq!(published(&host, "/project/small.yaml", FRAGMENT).len(), 1);
    }

    #[test]
    fn candidate_check_can_be_switched_off() {
        let host = MockHost::install();
        host.set_setting("checkSpecCandidates", "false");
        assert!(published(&host, "/project/frag.yaml", FRAGMENT).is_empty());
    }

    #[test]
    fn ranges_use_utf16_columns_with_multibyte_text() {
        let host = MockHost::install();
        let text = "\u{1F600}: 1\npaths:\n  /p:\n    get: {}\n";
        assert_eq!(published(&host, "/project/u.yaml", text)[0].range.start.line, 1);

        let json = "{ \"\u{1F600}\": 1, \"paths\": { \"/p\": { \"get\": {} } } }";
        let found = published(&host, "/project/u.json", json);
        assert_eq!(found[0].range.start.character, 12);
    }

    #[test]
    fn crlf_files_report_correct_lines() {
        let host = MockHost::install();
        let text = "info: x\r\nnote: y\r\npaths:\r\n  /pets:\r\n    get: {}\r\n";
        let found = published(&host, "/project/crlf.yaml", text);
        assert_eq!(found[0].range.start.line, 2);
        assert_eq!(found[0].range.start.character, 0);
    }

    #[test]
    fn lists_endpoints_of_json_specs() {
        let host = MockHost::install();
        host.add_file("/project/a.yaml", PETSTORE);
        host.add_file("/project/b/api.json", JSON_SPEC);
        let mut openapi = OpenApi::default();
        let view = openapi.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(items.len(), 5);
        openapi.handle(ENDPOINTS, ViewEvent::Activate(items[3].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations()[0].0, "/project/b/api.json");
    }

    #[test]
    fn endpoints_skip_non_spec_json_and_huge_files() {
        let host = MockHost::install();
        host.add_file("/project/package.json", "{\"name\": \"x\"}");
        let huge = format!("{{\"openapi\": \"3.0.0\", \"paths\": {{\"/p\": {{\"get\": {{}}}}}}, \"pad\": \"{}\"}}", "x".repeat(2 << 20));
        host.add_file("/project/huge.json", &huge);
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        assert!(matches!(view.body, Body::Message(_)));
    }

    #[test]
    fn endpoints_view_counts_yaml_and_json_files() {
        let host = MockHost::install();
        host.add_file("/project/a.yaml", PETSTORE);
        host.add_file("/project/b.json", JSON_SPEC);
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        assert_eq!(view.status.as_deref(), Some("5 endpoints"));
    }

    #[test]
    fn missing_file_is_treated_as_absent() {
        let _host = MockHost::install();
        for error in [Error::NotFound("x".into()), Error::Invalid("/project/x.yaml: no such file".into())] {
            assert!(matches!(fetch(Err(error)), Fetched::Missing));
        }
    }

    fn named(path: &str, text: &str) -> Document {
        Document { path: path.into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn skeletons(path: &str, text: &str) -> Vec<CompletionItem> {
        OpenApi::default()
            .completion(&named(path, text), position(text, text.len()), None)
            .into_iter()
            .filter(|item| item.kind == Some(CompletionKind::Snippet))
            .collect()
    }

    fn complete_at_end(text: &str) -> Vec<CompletionItem> {
        let _host = MockHost::install();
        OpenApi::default().completion(&named("/p/openapi.yaml", text), position(text, text.len()), None)
    }

    fn labels_at_end(text: &str) -> Vec<String> {
        complete_at_end(text).into_iter().filter(|i| i.kind != Some(CompletionKind::Snippet)).map(|i| i.label).collect()
    }

    #[test]
    fn skeleton_items_are_offered_in_a_spec_yaml_and_json() {
        let _host = MockHost::install();
        let yaml = skeletons("/project/openapi.yaml", "openapi: 3.0.0\n");
        let labels: Vec<_> = yaml.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["oas3", "opnp", "endp", "swag"]);
        assert!(yaml.iter().all(|i| i.kind == Some(CompletionKind::Snippet) && i.snippet && i.insert_text.contains("$0")));
        assert!(yaml[0].insert_text.starts_with("openapi: 3.1.0\ninfo:\n") && yaml[0].insert_text.contains("paths:\n"));
        assert!(yaml[1].insert_text.contains("\n  get:\n") && yaml[1].insert_text.contains("'200':"));
        assert!(yaml[2].insert_text.contains("operationId: ${") && yaml[2].insert_text.contains("parameters:"));
        assert!(yaml[3].insert_text.starts_with("swagger: \"2.0\"\ninfo:\n"));

        let json_text = "{\"openapi\": \"3.0.0\"}";
        let json = skeletons("/project/openapi.json", json_text);
        for item in &json {
            let body = item.insert_text.replace("$0", "").replace("${1:", "").replace("${2:", "").replace("${3:", "").replace("${4:", "");
            let body = body.replace("}\"", "\"");
            assert!(item.insert_text.contains("$0"), "{}", item.label);
            assert!(!body.trim_start().starts_with("swagger:"), "{}", item.label);
        }
        let swagger = json.iter().find(|i| i.label == "swag").unwrap();
        let plain = swagger.insert_text.replace("$0", "").replace("${1:API}", "API").replace("${2:1.0.0}", "1.0.0");
        let paths: Vec<_> = crate::json::keys(&plain).into_iter().map(|k| k.key).collect();
        assert!(["swagger", "info", "info.title", "info.version", "paths"].iter().all(|k| paths.iter().any(|p| p == k)), "{paths:?}");
        let root = json.iter().find(|i| i.label == "oas3").unwrap();
        let plain = root
            .insert_text
            .replace("$0", "")
            .replace("${1:API}", "API")
            .replace("${2:1.0.0}", "1.0.0")
            .replace("${3:path}", "path")
            .replace("${4:OK}", "OK");
        let paths: Vec<_> = crate::json::keys(&plain).into_iter().map(|k| k.key).collect();
        assert!(
            ["openapi", "info.title", "paths./path.get.responses.200.description"].iter().all(|k| paths.iter().any(|p| p == k)),
            "{paths:?}"
        );
        assert!(json.iter().find(|i| i.label == "opnp").unwrap().insert_text.contains("\"get\": {"));
    }

    #[test]
    fn root_keywords_are_offered_at_the_top_level() {
        let labels = labels_at_end("openapi: 3.0.0\n");
        assert!(["info", "paths", "components", "servers", "tags"].iter().all(|k| labels.iter().any(|l| l == k)), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "get"));
    }

    #[test]
    fn keywords_follow_the_indentation_into_an_operation() {
        let text = "openapi: 3.0.0\npaths:\n  /pets:\n    get:\n      ";
        let labels = labels_at_end(text);
        assert!(["summary", "operationId", "responses", "parameters"].iter().all(|k| labels.iter().any(|l| l == k)), "{labels:?}");
        assert!(!labels.iter().any(|l| l == "paths"));

        let path_item = labels_at_end("openapi: 3.0.0\npaths:\n  /pets:\n    ");
        assert!(path_item.iter().any(|l| l == "get") && path_item.iter().any(|l| l == "post"));
        assert!(labels_at_end("openapi: 3.0.0\npaths:\n  ").is_empty());
    }

    #[test]
    fn keywords_inside_a_list_item_are_those_of_the_item() {
        let text = "openapi: 3.0.0\npaths:\n  /p:\n    get:\n      parameters:\n        - name: id\n          ";
        let labels = labels_at_end(text);
        assert!(["in", "required", "schema", "style"].iter().all(|k| labels.iter().any(|l| l == k)), "{labels:?}");
        let new_item = labels_at_end("openapi: 3.0.0\npaths:\n  /p:\n    get:\n      parameters:\n        - name: id\n        - ");
        assert!(new_item.iter().any(|l| l == "in"));
        let compact = labels_at_end("openapi: 3.0.0\npaths:\n  /p:\n    get:\n      parameters:\n      - name: id\n        ");
        assert!(compact.iter().any(|l| l == "in"), "{compact:?}");
    }

    #[test]
    fn response_codes_and_schema_keywords_are_offered() {
        let codes = complete_at_end("openapi: 3.0.0\npaths:\n  /p:\n    get:\n      responses:\n        ");
        let ok = codes.iter().find(|i| i.label == "200").unwrap();
        assert_eq!(ok.insert_text, "'200':");
        assert!(codes.iter().any(|i| i.label == "default" && i.insert_text == "default:"));

        let schema = labels_at_end("openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet:\n      ");
        assert!(["type", "properties", "required", "allOf"].iter().all(|k| schema.iter().any(|l| l == k)), "{schema:?}");
        let property = labels_at_end("openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet:\n      properties:\n        name:\n          ");
        assert!(property.iter().any(|l| l == "format"));
    }

    #[test]
    fn enum_values_are_offered_after_the_key() {
        let in_values = labels_at_end("openapi: 3.0.0\npaths:\n  /p:\n    get:\n      parameters:\n        - name: id\n          in: ");
        assert_eq!(in_values, ["query", "header", "path", "cookie"]);

        let types = complete_at_end("openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet:\n      type: ob");
        assert!(types.iter().any(|i| i.label == "object") && !types.iter().any(|i| i.label == "null"));
        assert!(types.iter().all(|i| i.kind == Some(CompletionKind::EnumMember)));
        let range = types[0].replace.unwrap();
        assert_eq!((range.start.character, range.end.character), (12, 14));

        let nullable = complete_at_end("openapi: 3.1.0\ncomponents:\n  schemas:\n    Pet:\n      type: ");
        assert!(nullable.iter().any(|i| i.label == "null"));
        assert!(labels_at_end("openapi: 3.0.0\ninfo:\n  title: ").is_empty());
    }

    #[test]
    fn keywords_are_quiet_in_scalars_flow_style_json_and_swagger_2() {
        let block = "openapi: 3.0.0\ninfo:\n  description: |\n    ";
        assert!(labels_at_end(block).is_empty());
        assert!(labels_at_end("openapi: 3.0.0\ninfo:\n  title: long text\n    ").is_empty());
        assert!(labels_at_end("openapi: 3.0.0\ninfo: {ti").is_empty());
        assert!(labels_at_end("swagger: '2.0'\n").is_empty());
        assert!(labels_at_end("openapi: 3.0.0\n# comment ").is_empty());
        assert!(labels_at_end("openapi: 3.0.0\nunknown:\n  ").is_empty());
        let json = "{\"openapi\": \"3.0.0\",\n  \"";
        let items = OpenApi::default().completion(&named("/p/openapi.json", json), position(json, json.len()), None);
        assert!(items.iter().any(|i| i.label == "info" && i.insert_text == "info"));
        let value = "{\"openapi\": \"3.0.0\", \"info\": {\"title\": \"";
        let items = OpenApi::default().completion(&named("/p/openapi.json", value), position(value, value.len()), None);
        assert!(items.iter().all(|i| i.kind == Some(CompletionKind::Snippet)));
    }

    #[test]
    fn json_keys_and_enum_values_follow_the_object_under_the_caret() {
        let labels = |text: &str| labels_at_end(text);
        let root = labels("{\"openapi\": \"3.0.0\",\n  ");
        assert!(root.iter().any(|l| l == "paths") && root.iter().any(|l| l == "components"), "{root:?}");
        let inside = labels("{\"openapi\": \"3.0.0\", \"paths\": {\"/p\": {\"get\": {\"parameters\": [{\"name\": \"id\", \"");
        assert!(inside.iter().any(|l| l == "in") && inside.iter().any(|l| l == "required"), "{inside:?}");
        let responses = labels("{\"openapi\": \"3.0.0\", \"paths\": {\"/p\": {\"get\": {\"responses\": {");
        assert!(responses.iter().any(|l| l == "404"));

        let values = labels("{\"openapi\": \"3.0.0\", \"paths\": {\"/p\": {\"get\": {\"parameters\": [{\"in\": ");
        assert_eq!(values, ["query", "header", "path", "cookie"]);
        let typed = complete_at_end("{\"openapi\": \"3.0.0\", \"paths\": {\"/p\": {\"get\": {\"parameters\": [{\"in\": \"he");
        assert_eq!(typed[0].insert_text, "query");
        let bare = complete_at_end("{\"openapi\": \"3.0.0\", \"paths\": {\"/p\": {\"get\": {\"parameters\": [{\"in\": ");
        assert_eq!(bare[0].insert_text, "\"query\"");
        let key = complete_at_end("{\"openapi\": \"3.0.0\", \"info\": {\"ti");
        assert_eq!(key.iter().find(|i| i.label == "title").unwrap().insert_text, "title");
    }

    #[test]
    fn json_completion_is_quiet_after_a_complete_value() {
        let after_value = labels_at_end("{\"openapi\": \"3.0.0\", \"info\": {\"title\": \"x\" ");
        assert!(after_value.iter().all(|l| l.starts_with("openapi-") || l == "swag"), "{after_value:?}");
    }

    #[test]
    fn offers_to_extract_an_inline_schema_beside_the_modeline_action() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet:\n      properties:\n        owner:\n          type: object\n";
        let line = 5;
        let actions =
            OpenApi::default().code_actions(&named("/p/openapi.yaml", text), Range::new(Position::new(line, 4), Position::new(line, 4)));
        let titles: Vec<_> = actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Extract inline schema to components/schemas", "Add the OpenAPI schema for the YAML language server"]);
        assert_eq!(actions[0].edits[0].1.len(), 1);
        let elsewhere = OpenApi::default().code_actions(&named("/p/openapi.yaml", text), Range::default());
        assert_eq!(elsewhere.iter().map(|action|action.title.as_str()).collect::<Vec<_>>(), ["Add the OpenAPI schema for the YAML language server", "Add OpenAPI info section", "Add OpenAPI paths section"]);
    }

    #[test]
    fn writes_a_self_contained_spec_or_says_there_is_nothing_to_bundle() {
        let host = writing_host();
        host.add_file("/project/common.yaml", "Pet:\n  type: object\n");
        let text = "openapi: 3.0.0\nx:\n  $ref: './common.yaml#/Pet'\n";
        host.open_editor("/project/api.yaml", "yaml", text, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("export-self-contained", &host.invocation(), None).unwrap();
        let bundled = generated("/project/api.bundled.yaml");
        assert!(bundled.contains("$ref: '#/components/schemas/Pet'") && bundled.ends_with("    Pet:\n      type: object\n"), "{bundled}");

        host.open_editor("/project/api.yaml", "yaml", "openapi: 3.0.0\n", &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("export-self-contained", &host.invocation(), None).unwrap();
        assert_eq!(generated("/project/api.bundled.yaml"), bundled);

        host.open_editor("/project/api.yaml", "yaml", "openapi: 3.0.0\nx:\n  $ref: './gone.yaml#/Pet'\n", &[(0, 0, 0, 0)]);
        assert!(OpenApi::default().run_command("export-self-contained", &host.invocation(), None).is_err());
    }

    #[test]
    fn bundles_a_json_spec_to_the_clipboard_when_project_write_is_denied() {
        let host = MockHost::install().with_capabilities(&["editor.read", "project.read", "clipboard"]);
        host.add_file("/project/common.json", "{\"Pet\": {\"type\": \"object\"}}");
        let text = "{\"openapi\": \"3.0.0\", \"x\": {\"$ref\": \"./common.json#/Pet\"}}";
        host.open_editor("/project/api.json", "json", text, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("export-self-contained", &host.invocation(), None).unwrap();
        let copied = host.clipboard().unwrap();
        assert!(copied.contains("\"$ref\": \"#/components/schemas/Pet\"") && copied.contains("\"Pet\": {"), "{copied}");
    }

    #[test]
    fn copies_the_reference_to_the_key_under_the_caret() {
        let host = MockHost::install();
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(36, 4, 36, 4)]);
        OpenApi::default().run_command("copy-reference", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().unwrap(), "$ref: '#/components/schemas/Pet'");

        let json = "{\"openapi\": \"3.0.0\",\n \"components\": {}}";
        host.open_editor("/project/api.json", "json", json, &[(1, 3, 1, 3)]);
        OpenApi::default().run_command("copy-reference", &host.invocation(), None).unwrap();
        assert_eq!(host.clipboard().unwrap(), "\"$ref\": \"#/components\"");
    }

    #[test]
    fn copy_reference_uses_the_actual_caret_key_and_literal_array_ancestry() {
        let host=MockHost::install();
        for (language,text,needle,expected) in [
            ("json",r#"{"openapi":"3.1.1","first":{},"second":{}}"#,"second",r##""$ref": "#/second""##),
            ("json",r#"{"openapi":"3.1.1","x":[{"a.b[0]/~":1}]}"#,"a.b",r##""$ref": "#/x/0/a.b%5B0%5D~1~0""##),
            ("yaml","openapi: 3.1.1\nx:\n  - a.b[0]/~: 1\n","a.b","$ref: '#/x/0/a.b%5B0%5D~1~0'"),
            ("json",r#"{"openapi":"3.1.1","a\"b%2F#":1}"#,"a\\",r##""$ref": "#/a%22b%252F%23""##),
            ("yaml","openapi: 3.1.1\nowner's key: 1\n","owner","$ref: '#/owner%27s%20key'"),
            ("json",r#"{"openapi":"3.1.1","😀":[{"é":1}]}"#,"é",r##""$ref": "#/%F0%9F%98%80/0/%C3%A9""##),
        ] {
            let caret=position(text,text.find(needle).unwrap());
            host.open_editor(&format!("/project/api.{language}"),language,text,&[(caret.line,caret.character,caret.line,caret.character)]);
            OpenApi::default().run_command("copy-reference",&host.invocation(),None).unwrap();
            assert_eq!(host.clipboard().as_deref(),Some(expected),"{text}");
        }
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
    #[test]
    fn copy_reference_unknown_source_preserves_clipboard_and_uses_current_text() {
        let host=MockHost::install();
        let original=r#"{"openapi":"3.1.1","a":{"b":1}}"#;
        let caret=position(original,original.find("b\"").unwrap());
        host.open_editor("/project/api.json","json",original,&[(caret.line,caret.character,caret.line,caret.character)]);
        OpenApi::default().run_command("copy-reference",&host.invocation(),None).unwrap();
        assert_eq!(host.clipboard().as_deref(),Some("\"$ref\": \"#/a/b\""));
        let ambiguous=r#"{"openapi":"3.1.1","a.b":1,"a":{"b":2}}"#;
        let caret=position(ambiguous,ambiguous.find("a.b").unwrap());
        host.open_editor("/project/api.json","json",ambiguous,&[(caret.line,caret.character,caret.line,caret.character)]);
        assert!(OpenApi::default().run_command("copy-reference",&host.invocation(),None).is_err());
        assert_eq!(host.clipboard().as_deref(),Some("\"$ref\": \"#/a/b\""));
        let changed=r#"{"openapi":"3.1.1","a.b":1}"#;
        let caret=position(changed,changed.find("a.b").unwrap());
        host.open_editor("/project/api.json","json",changed,&[(caret.line,caret.character,caret.line,caret.character)]);
        OpenApi::default().run_command("copy-reference",&host.invocation(),None).unwrap();
        assert_eq!(host.clipboard().as_deref(),Some("\"$ref\": \"#/a.b\""));
        assert!(host.requests().is_empty());assert!(host.process_runs().is_empty());
    }
    #[test]
    fn skeletons_are_offered_in_an_empty_openapi_named_file() {
        let _host = MockHost::install();
        for path in ["/p/openapi.yaml", "/p/swagger.json", "/p/pets.openapi.yml", "/p/pets.swagger.json"] {
            assert_eq!(skeletons(path, "").len(), 4, "{path}");
        }
    }

    #[test]
    fn skeletons_are_quiet_in_unrelated_yaml_and_json() {
        let _host = MockHost::install();
        assert!(skeletons("/p/docker-compose.yml", "services:\n  web:\n    image: nginx\n").is_empty());
        assert!(skeletons("/p/package.json", "{\"name\": \"x\"}").is_empty());
        assert!(skeletons("/p/docker-compose.yml", "").is_empty());
    }

    #[test]
    fn skeletons_do_not_hide_ref_completion() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: '#/comp";
        let items = complete_at_end(text);
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| i.kind == Some(CompletionKind::Reference)));
    }

    #[test]
    fn skeletons_ignore_the_trigger_character() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\n";
        let d = named("/project/openapi.yaml", text);
        let with = OpenApi::default().completion(&d, position(text, text.len()), Some("/"));
        assert_eq!(with, complete_at_end(text));
    }

    #[test]
    fn spec_globs_extend_the_files_treated_as_specs() {
        let host = MockHost::install();
        assert!(skeletons("/p/api/pets.yaml", "").is_empty());
        host.set_setting("specGlobs", "\"api/*.yaml, *.contract.json\"");
        assert_eq!(skeletons("/p/api/pets.yaml", "").len(), 4);
        assert_eq!(skeletons("/p/pets.contract.json", "").len(), 4);
        assert!(skeletons("/p/other/pets.yaml", "").is_empty());
        assert!(skeletons("/p/api/pets.txt", "").is_empty());
    }

    #[test]
    fn spec_globs_bring_fragments_into_problems_and_the_endpoints_view() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.set_setting("specGlobs", "\"api/*.yaml\"");
        let fragment = "paths:\n  /a:\n    get:\n      operationId: dup\n      responses:\n        '200':\n          $ref: '#/components/responses/Ok'\n  /b:\n    get:\n      operationId: dup\n      responses:\n        '200':\n          $ref: './gone.yaml#/Ok'\n";
        host.add_file("/project/api/part.yaml", fragment);
        host.add_file("/project/other/part.yaml", fragment);
        OpenApi::default().handle_events(&[Event::FileOpened("/project/api/part.yaml".into())], &host.invocation());
        let mut codes: Vec<_> = host.diagnostics()["/project/api/part.yaml"].iter().filter_map(|d| d.code.clone()).collect();
        codes.sort();
        assert_eq!(codes, ["duplicate-operation-id", "duplicate-operation-id", "unresolved-ref"]);

        OpenApi::default().handle_events(&[Event::FileOpened("/project/other/part.yaml".into())], &host.invocation());
        let other: Vec<_> = host.diagnostics()["/project/other/part.yaml"].iter().filter_map(|d| d.code.clone()).collect();
        assert_eq!(other, ["missing-openapi-root"]);

        assert_eq!(project_routes(&host.invocation().project().unwrap()).unwrap().len(), 2);
    }

    #[test]
    fn spec_globs_enable_go_to_definition_in_a_fragment() {
        let host = MockHost::install();
        let text = "Pet:\n  type: object\nOwner:\n  $ref: '#/Pet'\n";
        let fragment = named("/p/api/pets.yaml", text);
        let on_ref = at(text, "#/Pet", 3);
        assert!(OpenApi::default().definition(&fragment, on_ref).is_empty());
        host.set_setting("specGlobs", "\"api/*.yaml\"");
        assert_eq!(OpenApi::default().definition(&fragment, on_ref).len(), 1);
    }

    #[test]
    fn globs_match_names_and_path_suffixes() {
        assert!(matches_glob("*.yaml", "/p/a/b.yaml"));
        assert!(matches_glob("api/*.yaml", "/p/api/b.yaml"));
        assert!(!matches_glob("api/*.yaml", "/p/xapi/b.yaml"));
        assert!(matches_glob("/p/a?.json", "/p/ab.json"));
    }

    #[test]
    fn operations_get_a_marker_that_navigates_to_their_schema() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\npaths:\n  /pets:\n    get:\n      responses:\n        '200':\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/Pet'\n    post:\n      responses: {}\ncomponents:\n  schemas:\n    Pet:\n      type: object\n";
        let markers: Vec<_> = OpenApi::default().line_markers(&named("/p/openapi.yaml", text)).into_iter().filter(|m| m.icon.as_deref() == Some(REF_MARKER)).collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 3);
        assert_eq!(markers[0].icon.as_deref(), Some(REF_MARKER));
        assert_eq!(markers[0].tooltip, "GET /pets: #/components/schemas/Pet");
        let target = markers[0].target.as_ref().unwrap();
        assert_eq!(target.range.start.line, 14);
    }

    #[test]
    fn every_ref_of_an_operation_is_a_same_icon_marker_on_its_line() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\npaths:\n  /pets:\n    post:\n      requestBody:\n        $ref: '#/components/requestBodies/NewPet'\n      responses:\n        '200':\n          $ref: '#/components/responses/Pet'\n        '404':\n          $ref: '#/components/responses/Pet'\ncomponents:\n  requestBodies:\n    NewPet: {}\n  responses:\n    Pet: {}\n";
        let markers: Vec<_> = OpenApi::default().line_markers(&named("/p/openapi.yaml", text)).into_iter().filter(|m| m.icon.as_deref() == Some(REF_MARKER)).collect();
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().all(|m| m.line == 3));
        let targets: Vec<_> = markers.iter().map(|m| m.target.as_ref().unwrap().range).collect();
        assert_eq!(targets[0].start.line, 13);
        assert_eq!(targets[1].start.line, 15);
        assert!(targets.iter().all(|r| r.start != r.end));
    }

    #[test]
    fn component_declarations_list_their_local_refs() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\npaths:\n  /a:\n    get:\n      x:\n        $ref: '#/components/schemas/Pet'\n  /b:\n    get:\n      x:\n        $ref: '#/components/schemas/Pet/properties/id'\ncomponents:\n  schemas:\n    Pet:\n      properties:\n        id: {}\n    Pets:\n      $ref: '#/components/schemas/Pet'\n    Unused: {}\n";
        let markers: Vec<_> = OpenApi::default().line_markers(&named("/p/openapi.yaml", text)).into_iter().filter(|m| m.icon.as_deref() == Some(REFERENCED_BY_MARKER)).collect();
        assert_eq!(markers.len(), 3);
        assert!(markers.iter().all(|m| m.line == 12));
        let lines: Vec<_> = markers.iter().map(|m| m.target.as_ref().unwrap().range.start.line).collect();
        assert_eq!(lines, [5, 9, 16]);
        for marker in &markers {
            let range = marker.target.as_ref().unwrap().range;
            assert_eq!(range.start.line, range.end.line);
            assert!(text.lines().nth(range.start.line as usize).unwrap()[range.start.character as usize..range.end.character as usize].contains("#/components/schemas/Pet"));
        }
        let single = text.replace("$ref: '#/components/schemas/Pet/properties/id'", "type: string").replace("      $ref: '#/components/schemas/Pet'\n    Unused", "      type: array\n    Unused");
        let markers: Vec<_> = OpenApi::default().line_markers(&named("/p/openapi.yaml", &single)).into_iter().filter(|m| m.icon.as_deref() == Some(REFERENCED_BY_MARKER)).collect();
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].tooltip, "#/components/schemas/Pet is referenced on line 6");
    }

    #[test]
    fn markers_on_one_line_are_bounded_and_say_so() {
        let _host = MockHost::install();
        let uses = (0..250).map(|n| (format!("u{n}"), serde_json::json!({"$ref": "#/components/schemas/Pet"}))).collect::<serde_json::Map<_, _>>();
        let text = serde_json::json!({"openapi": "3.1.0", "paths": {}, "x": uses, "components": {"schemas": {"Pet": {}}}}).to_string();
        let markers = OpenApi::default().line_markers(&named("/p/openapi.json", &text));
        assert_eq!(markers.len(), 200);
        assert!(markers.iter().all(|m| m.tooltip.ends_with("(first 200 of 250 shown)")));
    }

    #[test]
    fn no_markers_outside_specs_or_for_unresolved_refs() {
        let _host = MockHost::install();
        let plain = "a:\n  get:\n    x: 1\n";
        assert!(OpenApi::default().line_markers(&named("/p/a.yaml", plain)).is_empty());
        let dangling = "openapi: 3.0.0\npaths:\n  /p:\n    get:\n      x:\n        $ref: '#/nope'\n";
        assert!(OpenApi::default().line_markers(&named("/p/openapi.yaml", dangling)).is_empty());
    }

    #[test]
    fn preview_page_has_a_reload_button() {
        let html = include_str!("../web/index.html");
        let js = include_str!("../web/main.js");
        assert!(html.contains("id=\"reload\""));
        assert!(js.contains("getElementById(\"reload\")"));
        assert!(js.contains("known = \"\""));
        assert!(js.contains("document.get().then((doc) => render(doc.text))"));
    }

    #[test]
    fn preview_try_button_prepares_the_existing_try_flow_without_sending() {
        let html = include_str!("../web/index.html");
        let js = include_str!("../web/main.js");
        assert!(html.contains("id=\"try\"") && html.contains("Try…"));
        assert!(js.contains("getElementById(\"try\")") && js.contains("postMessage(\"try\")"));
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "events", "net:127.0.0.1"]);
        let spec = "openapi: 3.1.0\ninfo: {title: T, version: '1'}\nservers:\n  - url: http://127.0.0.1:32109\npaths:\n  /pets:\n    get:\n      responses: {200: {description: OK}}\n";
        host.open_editor("/project/openapi.yaml", "yaml", spec, &[]);
        let mut plugin = OpenApi::default();
        plugin.activate(&openidea_plugin::Context { api_version: "1.10".into(), capabilities: vec!["net:127.0.0.1".into()], ..Default::default() }).unwrap();
        plugin.handle("preview", ViewEvent::Message("try".into()), &host.invocation()).unwrap(); // Dismissed picker.
        host.answer_next_prompt(Some(&["0"]));
        plugin.handle("preview", ViewEvent::Message("try".into()), &host.invocation()).unwrap();
        let Body::Form(fields) = plugin.render("try-response", None, &host.invocation()).unwrap().body else { panic!() };
        let openidea_plugin::views::FieldValue::Other { data, .. } = &fields[0].value else { panic!() };
        assert!(data.contains("GET http://127.0.0.1:32109/pets") && data.contains("Not sent yet"), "{data}");
        assert!(host.requests().is_empty());
        plugin.activate(&openidea_plugin::Context { api_version: "1.10".into(), ..Default::default() }).unwrap();
        assert!(plugin.handle("preview", ViewEvent::Message("try".into()), &host.invocation()).is_err());
    }

    #[test]
    fn preview_page_can_switch_to_redoc() {
        let html = include_str!("../web/index.html");
        let js = include_str!("../web/main.js");
        let redoc = include_str!("../web/vendor/redoc.standalone.js");
        assert!(html.contains("id=\"switch\"") && html.contains("id=\"redoc\""));
        assert!(js.contains("getElementById(\"switch\")"));
        assert!(js.contains("./vendor/redoc.standalone.js") && js.contains("Redoc.init("));
        assert!(js.contains("disableSearch: true"));
        assert!(redoc.contains("exports.Redoc="));
    }
}

impl openidea_plugin::search::SearchProvider for OpenApi {
    fn search(&mut self, category: &str, query: &str, limit: u32, ctx: &Invocation) -> Result<openidea_plugin::search::SearchPage, Error> {
        if category != search::CATEGORY {
            return Ok(Default::default());
        }
        self.search.search(query, limit, ctx)
    }
}

impl HoverActions for OpenApi {
    /// "Try GET /pets…" on each operation line that Try it out can send.
    fn hover_actions(&mut self, doc: &Document) -> Vec<HoverAction> {
        if !is_spec_file(&doc.path) || !spec::maybe_spec(&doc.text) {
            return Vec::new();
        }
        let tryable = self.tryable_operations(&doc.text);
        if tryable.is_empty() {
            return Vec::new();
        }
        let Some(spec) = spec::parse(&doc.text) else {
            return Vec::new();
        };
        spec::operations(&doc.text, &spec.keys)
            .into_iter()
            .filter(|operation| tryable.contains(&(operation.method.clone(), operation.path.clone())))
            .map(|operation| {
                let line = position(&doc.text, operation.at).line;
                let request = format!("{} {}", operation.method, operation.path);
                HoverAction {
                    line,
                    start_line: line,
                    end_line: line,
                    icon: Some("run".into()),
                    label: format!("Try {request}…"),
                    tooltip: Some(format!("Prepare {request} in Try it out. Nothing is sent until you press Send.")),
                    command: "try-local-read".into(),
                    argument: Some(request),
                }
            })
            .collect()
    }
}

impl openidea_plugin::endpoints::EndpointsProvider for OpenApi {
    fn endpoints(&mut self, ctx: &Invocation) -> Result<Vec<openidea_plugin::endpoints::Endpoint>, Error> {
        let Some(project) = ctx.project() else {
            return Ok(Vec::new());
        };
        Ok(ui_kit::routes::endpoints(&project_routes(&project)?, "OpenAPI"))
    }
}

#[cfg(test)]
mod marker_icon_tests {
    use super::*;

    #[test]
    fn ref_marker_icon_is_declared_in_the_manifest() {
        let manifest = include_str!("../plugin.toml");
        for icon in [REF_MARKER, REFERENCED_BY_MARKER] {
            assert!(manifest.contains(&format!("icon = \"{icon}\"")));
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(icon).is_file());
        }
    }
}

#[cfg(test)]
mod try_it_out_boundary_tests {
    use super::*;
    use openidea_plugin::{diagnostics::DiagnosticsProvider, hover::HoverProvider, mock::MockHost};
    #[test]
    fn rendered_and_unsaved_specs_do_not_execute_requests_even_with_network_grants() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read", "events", "storage", "net:localhost", "net:127.0.0.1", "net:example.test"]);
        let text = r#"{"openapi":"3.0.4","info":{"title":"Test","version":"1"},"servers":[{"url":"http://localhost:8080"}],"paths":{"/test":{"get":{"summary":"Local","responses":{}}}}}"#;
        host.add_file("api.json", text);
        host.open_editor("/project/api.json", "json", text, &[]);
        let mut plugin = OpenApi::default();
        plugin.render("endpoints", None, &host.invocation()).unwrap();
        let doc = Document { path: "/project/api.json".into(), language: "json".into(), version: 2, text: text.replace("Local", "Unsaved") };
        plugin.diagnostics(&doc);
        plugin.hover(&doc, position(&doc.text, doc.text.find("Unsaved").unwrap()));
        plugin.completion(&doc, Position::new(0, 1), None);
        plugin.line_markers(&doc);
        assert!(matches!(plugin.run_command("try-it-out", &host.invocation(), None), Err(Error::NotFound(_))));
        assert!(host.requests().is_empty(), "source/render/provider work must never execute a spec request");
        assert!(host.terminals().is_empty());
        assert!(!include_str!("../plugin.toml").contains("net:*"));
        assert!(!include_str!("../plugin.toml").contains("net:localhost"));
    }
}
