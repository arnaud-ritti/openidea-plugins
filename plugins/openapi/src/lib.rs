//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

mod checks;
mod http;
mod json;
mod spec;

use checks::Fetched;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, NotificationLevel, Plugin, Position, Project, Range,
    TextEdit, host, register_plugin,
};

#[derive(Default)]
pub struct OpenApi {
    project_files: Vec<String>,
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
/// `None` when they are the ones the page already has (`known` is its fingerprint).
fn preview_files(project: &Project, path: &str, text: &str, known: &str) -> Option<String> {
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

    Some(format!("{files},\"hash\":\"{hash}\"}}"))
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

fn problems(path: &str) -> Result<Vec<Diagnostic>, Error> {
    let project = project_for(path)?;
    let text = project.read_file(path)?;
    let Some(spec) = spec::parse(&text) else {
        return Ok(Vec::new());
    };

    let read = |file: &str| fetch(project.read_file(&resolve_path(path, file)));

    Ok(checks::check(&text, &spec, &read))
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

    fn run_command(&mut self, id: &str, ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
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

        match host::copy_to_clipboard(&requests) {
            Ok(()) => host::notify(
                NotificationLevel::Info,
                "OpenAPI",
                &format!("{count} requests copied to the clipboard."),
            ),
            Err(Error::Denied(_)) => host::notify(
                NotificationLevel::Warning,
                "OpenAPI",
                "Allow the clipboard for OpenAPI in Settings › Plugins to copy requests.",
            ),
            Err(e) => return Err(e),
        }

        Ok(None)
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
        if !spec::maybe_spec(&doc.text) {
            return Vec::new();
        }

        let Some(typed) = ref_at(&doc.text, at) else {
            return Vec::new();
        };
        let Some((file, _)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(spec) = spec::parse(&doc.text) else {
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
        if !spec::maybe_spec(&doc.text) {
            return Vec::new();
        }

        let Some(typed) = ref_at(&doc.text, at) else {
            return Vec::new();
        };
        let Some((file, fragment)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };
        let Some(spec) = spec::parse(&doc.text) else {
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

    fn code_actions(&mut self, doc: &Document, _selection: Range) -> Vec<CodeAction> {
        let Some(spec) = spec::parse(&doc.text).filter(|_| !spec::is_json(&doc.text)) else {
            return Vec::new();
        };
        let Some(line) = spec::modeline(&doc.text, &spec.version) else {
            return Vec::new();
        };

        let top = Range::new(Position::new(0, 0), Position::new(0, 0));
        vec![CodeAction {
            title: "Add the OpenAPI schema for the YAML language server".into(),
            kind: Some("source".into()),
            preferred: false,
            edits: vec![(doc.path.clone(), vec![TextEdit { range: top, text: line }])],
            data: None,
        }]
    }
}

pub const ENDPOINTS: &str = "endpoints";

// ponytail: re-scans the project on every render; cache per file version if large projects make it slow.
fn endpoint_items(project: &Project) -> Result<(Vec<ListItem>, usize), Error> {
    let mut paths = project.find_files("**/*.yaml", 500)?;
    paths.extend(project.find_files("**/*.yml", 500)?);

    let mut items = Vec::new();
    let mut files = 0;
    for path in paths {
        let Ok(text) = project.read_file(&path) else {
            continue;
        };
        let Some(spec) = spec::parse(&text) else {
            continue;
        };

        files += 1;
        let file_name = path.rsplit('/').next().unwrap_or(&path);
        for operation in spec::operations(&text, &spec.keys) {
            let at = position(&text, operation.at);
            let detail = match &operation.operation_id {
                Some(id) => format!("{id} · {file_name}"),
                None => file_name.to_owned(),
            };
            let key = format!("{}:{}:{path}", at.line, at.character);
            items.push(ListItem::new(&key, &format!("{} {}", operation.method, operation.path)).detail(&detail));
        }
    }

    Ok((items, files))
}

impl Views for OpenApi {
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = ctx.project() else {
            return Ok(View::message("Open a project to list its endpoints."));
        };

        let refresh = Button::new("refresh", "Refresh").icon("refresh");
        let (items, files) = endpoint_items(&project)?;
        if items.is_empty() {
            return Ok(View::message("No OpenAPI files in this project.").button(refresh));
        }

        let status = format!("{} endpoints in {files} files", items.len());
        Ok(View::new(Body::List(items)).button(refresh).status(&status))
    }

    fn handle(&mut self, view: &str, event: ViewEvent, ctx: &Invocation) -> Result<(), Error> {
        if PREVIEWS.contains(&view) {
            let ViewEvent::Message(request) = event else {
                return Ok(());
            };
            let mut parts = request.splitn(3, '\t');
            let (Some("files"), Some(known), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
                return Ok(());
            };
            let editor = ctx.editor().ok_or_else(|| Error::Invalid("the preview needs its document".into()))?;
            let path = editor.path();
            if path.rsplit('/').next() != Some(name) {
                return Ok(());
            }

            let project = project_for(&path)?;
            return match preview_files(&project, &path, &editor.document()?.text, known) {
                Some(files) => host::post_view_message(view, &files),
                None => Ok(()),
            };
        }

        match event {
            ViewEvent::Activate(key) => {
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
            _ => Ok(()),
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
    use openidea_plugin::{NotificationLevel, Position, Range};
    use spec::PETSTORE;

    fn doc(text: &str) -> Document {
        Document { path: "/project/api.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str, plus: usize) -> Position {
        position(text, text.find(needle).unwrap() + plus)
    }

    #[test]
    fn copies_the_requests_or_explains_without_clipboard() {
        let host = MockHost::install();
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
        assert!(OpenApi::default().completion(&doc(PETSTORE), at(PETSTORE, "title", 0), None).is_empty());
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
    fn lists_endpoints_of_every_spec_and_opens_them() {
        let host = MockHost::install();
        host.add_file("/project/api/openapi.yaml", PETSTORE);
        host.add_file("/project/k8s.yml", "kind: Pod\n");
        let view = OpenApi::default().render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["GET /pets", "GET /pets/{petId}", "DELETE /pets/{petId}"]
        );
        assert_eq!(items[0].detail.as_deref(), Some("listPets · openapi.yaml"));
        OpenApi::default().handle(ENDPOINTS, ViewEvent::Activate(items[1].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/api/openapi.yaml".to_owned(), 21, 4)]);
        OpenApi::default().handle(ENDPOINTS, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), [ENDPOINTS]);
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

    fn request(hash: &str, name: &str) -> ViewEvent {
        ViewEvent::Message(format!("files\t{hash}\t{name}"))
    }

    #[test]
    fn preview_receives_referenced_files() {
        let host = MockHost::install();
        host.open_editor("/project/api.yaml", "yaml", PREVIEW_TEXT, &[(0, 0, 0, 0)]);
        host.add_file("/project/schemas/pet.yaml", "Pet:\n  type: \"object\"\n");
        let mut plugin = OpenApi::default();
        plugin.handle("preview", request("", "api.yaml"), &host.invocation()).unwrap();

        let messages = host.view_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "preview");
        let prefix = r#"{"skipped":0,"files":{"./schemas/pet.yaml":"Pet:\n  type: \"object\"\n"},"hash":""#;
        assert!(messages[0].1.starts_with(prefix), "{}", messages[0].1);

        let hash = messages[0].1[prefix.len()..].trim_end_matches("\"}").to_owned();
        plugin.handle("preview", request(&hash, "api.yaml"), &host.invocation()).unwrap();
        assert_eq!(host.view_messages().len(), 1);

        host.add_file("/project/schemas/pet.yaml", "Pet: {}\n");
        plugin.handle("preview", request(&hash, "api.yaml"), &host.invocation()).unwrap();
        assert_eq!(host.view_messages().len(), 2);
    }

    #[test]
    fn preview_answers_only_for_its_own_document() {
        let host = MockHost::install();
        host.open_editor("/project/api.yaml", "yaml", PREVIEW_TEXT, &[(0, 0, 0, 0)]);
        host.add_file("/project/schemas/pet.yaml", "Pet: {}\n");
        OpenApi::default().handle("preview", request("", "other.yaml"), &host.invocation()).unwrap();
        assert!(host.view_messages().is_empty());
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
        OpenApi::default().handle("preview", request("", "api.yaml"), &host.invocation()).unwrap();

        let message = &host.view_messages()[0].1;
        assert!(message.starts_with(r#"{"skipped":4,"#), "{}", &message[..40]);
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
}
