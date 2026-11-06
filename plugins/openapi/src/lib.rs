//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

mod checks;
mod diff;
mod http;
mod json;
mod keys;
mod refs;
mod spec;

use checks::Fetched;
use ui_kit::routes::{self, Handler, Route, RouteEvent, RouteList};
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, PickItem, Plugin, Position, Project, Range,
    TextEdit, host, register_plugin,
};

#[derive(Default)]
pub struct OpenApi {
    project_files: Vec<String>,
    /// Status line and changes of the last comparison.
    comparison: Option<(String, Vec<ListItem>)>,
    routes: Option<RouteList>,
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
    let project = project_for(path)?;
    let text = project.read_file(path)?;
    let Some(spec) = spec_or_fragment(path, &text) else {
        return Ok(spec_candidate_hint(&text));
    };

    let read = |file: &str| fetch(project.read_file(&resolve_path(path, file)));
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
        let mut files = Vec::new();
        for pattern in ["**/*.yaml", "**/*.yml", "**/*.json"] {
            match project.find_files(pattern, 500) {
                Ok(found) => files.extend(found),
                Err(error) => host::log(LogLevel::Warn, &format!("openapi: listing {pattern}: {error}")),
            }
        }

        files.sort();
        self.project_files = files;
    }

    fn ref_file_items(&self, doc: &Document, typed_file: &str) -> Vec<String> {
        if typed_file.starts_with("http://") || typed_file.starts_with("https://") {
            return Vec::new();
        }

        self.project_files.iter().filter(|path| **path != doc.path).map(|path| relative_path(&doc.path, path)).collect()
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

const TEE: &str = "tee";

fn sibling_file(path: &str, suffix: &str) -> String {
    let (stem, extension) = path.rsplit_once('.').unwrap_or((path, ""));

    suffix.replace("{stem}", stem).replace("{ext}", extension)
}

/// Writes `text` to a file beside the spec through `tee`, or copies it to the clipboard when `tee` is not granted.
fn save_or_copy(target: &str, text: &str, what: &str) -> Result<Option<String>, Error> {
    let name = target.rsplit('/').next().unwrap_or(target);
    let exists = project_for(target).is_ok_and(|project| project.read_file(target).is_ok());
    if exists && !host::confirm("Overwrite file", &format!("{name} already exists and is replaced."))? {
        return Ok(None);
    }

    let written = host::run_process(TEE, &[target.to_owned()], Some(text)).and_then(|out| match out.status {
        Some(0) => Ok(()),
        _ => Err(Error::Invalid(format!("tee failed: {}", out.stderr.trim()))),
    });
    match written {
        Ok(()) => {
            host::notify(NotificationLevel::Info, "OpenAPI", &format!("{what} written to {name}."));
            host::open_location(target, 0, 0)?;
            Ok(None)
        }
        Err(Error::Denied(_)) => copy_or_explain(
            text,
            &format!("{what} copied to the clipboard."),
            "Allow writing files (tee) or the clipboard for OpenAPI in Settings › Plugins.",
        ),
        Err(error) => Err(error),
    }
}

impl OpenApi {
    fn compare_specs(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let revision = editor.document()?.text;
        if spec::parse(&revision).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        let project = project_for(&path)?;
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
        let Some(base_path) = host::pick("Compare the current spec with (base)", &candidates)? else { return Ok(None) };
        let base = project.read_file(&base_path)?;
        self.show_comparison(&base_path, &base, &path, &revision);
        host::reveal_view(COMPARE)?;
        Ok(None)
    }

    fn compare_last_revision(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let path = editor.path();
        let revision = editor.document()?.text;
        if spec::parse(&revision).is_none() {
            return Err("The current file is not an OpenAPI document.".into());
        }
        let project = project_for(&path)?;
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
                host::notify(NotificationLevel::Warning, "OpenAPI", "Allow git for OpenAPI in Settings › Plugins to compare with the last revision.");
                return Ok(None);
            }
            Err(error) => return Err(error),
        };
        self.show_comparison(&format!("HEAD:{relative}"), &committed, &path, &revision);
        host::reveal_view(COMPARE)?;
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
                host::notify(NotificationLevel::Info, "OpenAPI", "This file has no references to other files; it is already self-contained.");
                return Ok(None);
            }
            Err(message) => return Err(message.into()),
        };

        save_or_copy(&sibling_file(&path, "{stem}.bundled.{ext}"), &bundled, "The self-contained spec")
    }

    fn copy_reference(&mut self, ctx: &Invocation) -> Result<Option<String>, Error> {
        let editor = ctx.editor().ok_or("No file is open.")?;
        let text = editor.document()?.text;
        let spec = spec::parse(&text).ok_or("The current file is not an OpenAPI document.")?;
        let caret = editor.selections()?.first().map(|range| range.start.line).ok_or("There is no caret.")?;
        let pointer = refs::pointer_at(&text, &spec.keys, caret).ok_or("There is no key on the caret's line.")?;
        let reference = match spec::is_json(&text) {
            true => format!("\"$ref\": \"{pointer}\""),
            false => format!("$ref: '{pointer}'"),
        };

        copy_or_explain(&reference, &format!("Copied {reference}"), "Allow the clipboard for OpenAPI in Settings › Plugins to copy the reference.")
    }

    fn show_comparison(&mut self, base_path: &str, base: &str, path: &str, revision: &str) {
        let changes = diff::compare(base_path, base, path, revision);
        let name = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
        let status = format!("{} to {}: {} changes", name(base_path), name(path), changes.len());
        let items = changes.iter().map(|c| ListItem::new(&c.location, &c.label).detail(&c.detail)).collect();
        self.comparison = Some((status, items));
        host::refresh_view(COMPARE);
    }
}

impl Plugin for OpenApi {
    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
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

        if let Some(project) = paths.first().and_then(|path| project_for(path).ok()) {
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
        if id == "copy-route-curl" || id == "create-route-http" {
            let route = arg
                .and_then(|key| self.route_list().route(key).cloned())
                .ok_or_else(|| Error::Invalid("select an endpoint row".into()))?;
            if id == "copy-route-curl" {
                routes::copy_as_curl(&route, DEFAULT_BASE)?;
            } else {
                routes::create_http_file(&route, DEFAULT_BASE)?;
            }
            return Ok(None);
        }
        if id == "compare-specs" {
            return self.compare_specs(ctx);
        }
        if id == "export-self-contained" {
            return self.export_self_contained(ctx);
        }
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
        let spec = spec::parse(&text)
            .filter(|_| is_spec_file(&editor.path()))
            .ok_or("The current file is not an OpenAPI document.")?;
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
    spec::parse(&doc.text).or_else(|| {
        is_spec_name(&doc.path).then(|| spec::Spec { version: String::new(), keys: spec::scan(&doc.text) })
    })
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

fn read_sibling(doc: &Document, file: &str) -> Option<String> {
    project_for(&doc.path).ok()?.read_file(&resolve_path(&doc.path, file)).ok()
}

impl Providers for OpenApi {
    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        if !spec::maybe_spec(&doc.text) && !is_spec_name(&doc.path) {
            return Vec::new();
        }

        let Some(typed) = ref_at(&doc.text, at) else {
            return keyword_items(doc, at);
        };
        let Some((file, _)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(spec) = parse_or_fragment(doc) else {
            return Vec::new();
        };

        let mut labels: Vec<String> = if file.is_empty() {
            spec::ref_targets(&doc.text, &spec.keys)
        } else {
            read_sibling(doc, file)
                .map(|content| {
                    spec::ref_targets(&content, &spec::scan(&content)).into_iter().map(|t| format!("{file}{t}")).collect()
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

        let Some(typed) = ref_at(&doc.text, at) else {
            return Vec::new();
        };
        let Some((file, fragment)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(spec) = parse_or_fragment(doc) else {
            return Vec::new();
        };

        let (path, content, keys) = if file.is_empty() {
            (doc.path.clone(), doc.text.clone(), spec.keys)
        } else {
            let Some(content) = read_sibling(doc, file) else {
                return Vec::new();
            };
            let keys = spec::scan(&content);
            (resolve_path(&doc.path, file), content, keys)
        };

        if fragment.is_empty() {
            return vec![Location { path, range: Range::default() }];
        }

        spec::target(&keys, fragment)
            .map(|key| Location { path, range: spring_text::range(&content, key.at, key.end) })
            .into_iter()
            .collect()
    }

    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let Some(spec) = spec::parse(&doc.text) else {
            return Vec::new();
        };

        spec::operation_schema_refs(&doc.text, &spec.keys)
            .into_iter()
            .map(|(operation, schema, target)| LineMarker {
                line: position(&doc.text, operation.at).line,
                icon: Some("navigate".into()),
                tooltip: format!("{} {}: {schema}", operation.method, operation.path),
                target: Some(Location { path: doc.path.clone(), range: spring_text::range(&doc.text, target.at, target.end) }),
            })
            .collect()
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

        actions
    }
}

const MAX_SCANNED_FILE_BYTES: usize = 1 << 20;

pub const ENDPOINTS: &str = "endpoints";
pub const COMPARE: &str = "compare";

const DEFAULT_BASE: &str = "http://localhost:8080";
const NO_TAG: &str = "(no tag)";

fn routes(path: &str, text: &str, spec: &spec::Spec) -> Vec<Route> {
    let by_key = spec::exact(&spec.keys);
    spec::operations(text, &spec.keys)
        .into_iter()
        .map(|operation| {
            let tag = by_key.get(format!("{}.tags[0]", operation.key).as_str()).map(|k| k.value.clone());
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

// ponytail: re-scans the project on every render; cache per file version if large projects make it slow.
fn project_routes(project: &Project) -> Result<Vec<Route>, Error> {
    let mut paths = project.find_files("**/*.yaml", 500)?;
    paths.extend(project.find_files("**/*.yml", 500)?);
    paths.extend(project.find_files("**/*.json", 500)?);

    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = project.read_file(&path) else {
            continue;
        };
        if text.len() > MAX_SCANNED_FILE_BYTES {
            continue;
        }

        let Some(spec) = spec_or_fragment(&path, &text) else {
            continue;
        };

        found.extend(routes(&path, &text, &spec));
    }

    Ok(found)
}

impl OpenApi {
    fn route_list(&mut self) -> &mut RouteList {
        self.routes.get_or_insert_with(|| RouteList::new("openapi.routes", "aritti.openapi"))
    }
}

impl Views for OpenApi {
    fn render(&mut self, view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        if view == COMPARE {
            return Ok(match &self.comparison {
                None => View::message("Run OpenAPI: Compare With Another Spec…"),
                Some((status, items)) if items.is_empty() => View::message("No differences.").status(status),
                Some((status, items)) => View::new(Body::List(items.clone())).status(status),
            });
        }
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to list its endpoints."));
        };

        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let list = self.route_list();
        list.set_routes(project_routes(&project)?);
        Ok(list.render(View::new(Body::List(Vec::new())).button(refresh)))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if PREVIEWS.contains(&view) {
            let ViewEvent::Message(request) = event else {
                return Ok(());
            };
            let mut parts = request.splitn(3, '\t');
            let (Some("files"), Some(id), Some(known)) = (parts.next(), parts.next(), parts.next()) else {
                return Ok(());
            };
            let editor = ctx.editor().ok_or_else(|| Error::Invalid("the preview needs its document".into()))?;
            let path = editor.path();
            let project = project_for(&path)?;
            return match preview_files(&project, &path, &editor.document()?.text, id, known) {
                Some(files) => host::post_view_message(view, &files),
                None => Ok(()),
            };
        }

        match event {
            ViewEvent::Activate(key) if view == COMPARE => {
                let mut parts = key.splitn(3, ':');
                let (Some(line), Some(column), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                    return Ok(());
                };
                let (Ok(line), Ok(column)) = (line.parse(), column.parse()) else {
                    return Ok(());
                };
                host::open_location(path, line, column)
            }
            ViewEvent::Button(button) if button == "refresh" => {
                host::refresh_view(ENDPOINTS);
                Ok(())
            }
            event => match self.route_list().handle(&event) {
                RouteEvent::Changed => {
                    host::refresh_view(ENDPOINTS);
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

register_plugin!(OpenApi, providers, views);

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

    fn tee_host() -> MockHost {
        MockHost::install().with_capabilities(&["editor.read", "project.read", "clipboard", "process:tee"])
    }

    fn recording_tee(host: &MockHost) -> std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>> {
        let written = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = written.clone();
        host.on_process("tee", move |args, stdin| {
            sink.borrow_mut().push((args[0].clone(), stdin.unwrap_or_default().to_owned()));
            Ok(ProcessOutput { status: Some(0), stdout: String::new(), stderr: String::new() })
        });

        written
    }

    #[test]
    fn writes_the_requests_beside_the_spec_and_opens_them() {
        let host = tee_host();
        let written = recording_tee(&host);
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();

        assert_eq!(written.borrow()[0].0, "/project/api.http");
        assert!(written.borrow()[0].1.starts_with("### listPets\n"));
        assert_eq!(host.opened_locations()[0].0, "/project/api.http");
        assert!(host.clipboard().is_none());
    }

    #[test]
    fn an_existing_http_file_is_replaced_only_after_confirming() {
        let host = tee_host();
        let written = recording_tee(&host);
        host.add_file("/project/api.http", "old");
        host.open_editor("/project/api.yaml", "yaml", PETSTORE, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert!(written.borrow().is_empty());

        host.answer_next_prompt(Some(&[]));
        OpenApi::default().run_command("copy-http-requests", &host.invocation(), None).unwrap();
        assert_eq!(written.borrow().len(), 1);
    }

    #[test]
    fn copies_the_requests_when_tee_is_denied_and_explains_without_clipboard() {
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
        assert_eq!(
            actions[0].edits[0].1[0].text,
            "# yaml-language-server: $schema=https://spec.openapis.org/oas/3.0/schema/2024-10-18\n"
        );
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
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["/pets", "/pets/{petId}", "/pets/{petId}"]
        );
        assert_eq!(items[0].detail.as_deref(), Some("[GET] Pets"));
        assert_eq!(items[0].icon.as_deref(), Some("ext:fileIcon/aritti.openapi/route-get"));
        assert_eq!(items[2].icon.as_deref(), Some("ext:fileIcon/aritti.openapi/route-delete"));
    }

    #[test]
    fn untagged_operations_group_under_no_tag() {
        let host = MockHost::install();
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert!(items.iter().all(|i| i.detail.as_deref().unwrap().ends_with("(no tag)")));
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
        assert_eq!(host.revealed_views(), [COMPARE]);
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
    fn denied_git_asks_for_the_permission() {
        let host = MockHost::install().with_capabilities(&["project.read", "editor.read"]);
        let invocation = open_changed_petstore(&host);

        OpenApi::default().run_command("compare-last-revision", &invocation, None).unwrap();

        assert_eq!(host.notifications()[0].level, NotificationLevel::Warning);
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
        assert!(matches!(
            fetch(invalid("/etc/x.yaml: access outside the project root is not allowed")),
            Fetched::Unreadable
        ));
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
        assert_eq!(found, [
            (line_of(JSON_SPEC, "\"operationId\": \"a\",\n"), dup.clone()),
            (line_of(JSON_SPEC, "Nope"), unresolved.clone()),
            (line_of(JSON_SPEC, "pet.json#/Missing"), unresolved),
            (line_of(JSON_SPEC, "\"post\""), dup),
        ]);

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
            ("/project/package.json", "{\n  \"name\": \"x\",\n  \"dependencies\": { \"a\": \"1\" },\n  \"r\": { \"$ref\": \"#/nope\" }\n}\n"),
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
            ("/project/package.json", "{\n  \"name\": \"x\",\n  \"paths\": [\"a\"],\n  \"compilerOptions\": { \"paths\": { \"/x\": { \"get\": 1 } } }\n}\n"),
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
        let plain = root.insert_text.replace("$0", "").replace("${1:API}", "API").replace("${2:1.0.0}", "1.0.0").replace("${3:path}", "path").replace("${4:OK}", "OK");
        let paths: Vec<_> = crate::json::keys(&plain).into_iter().map(|k| k.key).collect();
        assert!(["openapi", "info.title", "paths./path.get.responses.200.description"].iter().all(|k| paths.iter().any(|p| p == k)), "{paths:?}");
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
        let actions = OpenApi::default().code_actions(&named("/p/openapi.yaml", text), Range::new(Position::new(line, 4), Position::new(line, 4)));
        let titles: Vec<_> = actions.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Extract inline schema to components/schemas", "Add the OpenAPI schema for the YAML language server"]);
        assert_eq!(actions[0].edits[0].1.len(), 1);
        let elsewhere = OpenApi::default().code_actions(&named("/p/openapi.yaml", text), Range::default());
        assert_eq!(elsewhere.len(), 1);
    }

    #[test]
    fn writes_a_self_contained_spec_or_says_there_is_nothing_to_bundle() {
        let host = tee_host();
        let written = recording_tee(&host);
        host.add_file("/project/common.yaml", "Pet:\n  type: object\n");
        let text = "openapi: 3.0.0\nx:\n  $ref: './common.yaml#/Pet'\n";
        host.open_editor("/project/api.yaml", "yaml", text, &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("export-self-contained", &host.invocation(), None).unwrap();
        let (target, bundled) = written.borrow()[0].clone();
        assert_eq!(target, "/project/api.bundled.yaml");
        assert!(bundled.contains("$ref: '#/components/schemas/Pet'") && bundled.ends_with("    Pet:\n      type: object\n"), "{bundled}");

        host.open_editor("/project/api.yaml", "yaml", "openapi: 3.0.0\n", &[(0, 0, 0, 0)]);
        OpenApi::default().run_command("export-self-contained", &host.invocation(), None).unwrap();
        assert_eq!(written.borrow().len(), 1);

        host.open_editor("/project/api.yaml", "yaml", "openapi: 3.0.0\nx:\n  $ref: './gone.yaml#/Pet'\n", &[(0, 0, 0, 0)]);
        assert!(OpenApi::default().run_command("export-self-contained", &host.invocation(), None).is_err());
    }

    #[test]
    fn bundles_a_json_spec_to_the_clipboard_when_tee_is_denied() {
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
        let markers = OpenApi::default().line_markers(&named("/p/openapi.yaml", text));
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].line, 3);
        assert_eq!(markers[0].icon.as_deref(), Some("navigate"));
        assert_eq!(markers[0].tooltip, "GET /pets: #/components/schemas/Pet");
        let target = markers[0].target.as_ref().unwrap();
        assert_eq!(target.range.start.line, 14);
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
    fn preview_page_can_switch_to_redoc() {
        let html = include_str!("../web/index.html");
        let js = include_str!("../web/main.js");
        let redoc = include_str!("../web/vendor/redoc.standalone.js");
        assert!(html.contains("id=\"switch\"") && html.contains("id=\"redoc\""));
        assert!(js.contains("getElementById(\"switch\")"));
        assert!(js.contains("./vendor/redoc.standalone.js") && js.contains("Redoc.init("));
        assert!(js.contains("disableSearch: true"));
        assert!(redoc.contains("exports.Redoc="));
        assert!(include_str!("../web/vendor/redoc-LICENSE").starts_with("The MIT License"));
        assert!(include_str!("../NOTICE.md").contains("Redoc"));
    }
}
