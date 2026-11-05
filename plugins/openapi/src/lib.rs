//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

mod checks;
mod spec;

use checks::Fetched;
use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, Providers};
use openidea_plugin::text::{offset, position};
use openidea_plugin::views::{Body, Button, ListItem, View, ViewEvent, Viewport, Views};
use openidea_plugin::{
    Diagnostic, Error, Event, Invocation, Location, LogLevel, Plugin, Position, Project, Range, TextEdit, host,
    register_plugin,
};
use spring_text::config::yaml;

#[derive(Default)]
pub struct OpenApi;

fn project_for(path: &str) -> Result<Project, Error> {
    host::projects()?
        .into_iter()
        .find(|p| p.root().is_ok_and(|root| path.starts_with(&format!("{}/", root.trim_end_matches('/')))))
        .ok_or_else(|| format!("{path} isn't in an open project.").into())
}

fn is_yaml(path: &str) -> bool {
    path.ends_with(".yaml") || path.ends_with(".yml")
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

fn problems(path: &str) -> Result<Vec<Diagnostic>, Error> {
    let project = project_for(path)?;
    let text = project.read_file(path)?;
    let Some(spec) = spec::parse(&text) else {
        return Ok(Vec::new());
    };

    // The host reports a missing file as `Invalid`, one that exists but can't be read as `Failed`.
    let read = |file: &str| match project.read_file(&resolve_path(path, file)) {
        Ok(content) => Fetched::Text(content),
        Err(Error::NotFound(_) | Error::Invalid(_)) => Fetched::Missing,
        Err(_) => Fetched::Unreadable,
    };

    Ok(checks::check(&text, &spec, &read))
}

impl Plugin for OpenApi {
    fn handle_events(&mut self, events: &[Event], _ctx: &Invocation) {
        let mut paths: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                Event::FileOpened(path) | Event::FileSaved(path) => Some(path.as_str()),
                _ => None,
            })
            .filter(|path| is_yaml(path))
            .collect();
        paths.sort();
        paths.dedup();

        for path in paths {
            let result = problems(path).and_then(|found| host::publish_diagnostics(path, &found));
            if let Err(error) = result {
                host::log(LogLevel::Warn, &format!("openapi: problems for {path}: {error}"));
            }
        }
    }

    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
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
        if spec::parse(&doc.text).is_none() {
            return Vec::new();
        }
        let Some(typed) = ref_at(&doc.text, at) else {
            return Vec::new();
        };
        let Some((file, _)) = spec::split_ref(&typed.target) else {
            return Vec::new();
        };

        let labels: Vec<String> = if file.is_empty() {
            spec::ref_targets(&doc.text, &yaml(&doc.text))
        } else {
            let Some(content) = read_sibling(doc, file) else {
                return Vec::new();
            };
            spec::ref_targets(&content, &yaml(&content)).into_iter().map(|t| format!("{file}{t}")).collect()
        };

        let replace = spring_text::range(&doc.text, typed.at, typed.end);
        labels
            .into_iter()
            .map(|label| {
                let mut item = CompletionItem::new(&label, CompletionKind::Reference);
                item.replace = Some(replace);
                item.filter_text = Some(label);
                item
            })
            .collect()
    }

    fn definition(&mut self, doc: &Document, at: Position) -> Vec<Location> {
        if spec::parse(&doc.text).is_none() {
            return Vec::new();
        }
        let Some(typed) = ref_at(&doc.text, at) else {
            return Vec::new();
        };
        let Some((file, fragment)) = spec::split_ref(&typed.target) else {
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

        let keys = yaml(&content);
        spec::target(&keys, fragment)
            .map(|key| Location { path, range: spring_text::range(&content, key.at, key.end) })
            .into_iter()
            .collect()
    }

    fn code_actions(&mut self, doc: &Document, _selection: Range) -> Vec<CodeAction> {
        let Some(spec) = spec::parse(&doc.text) else {
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
    fn render(&mut self, _view: &str, _viewport: Option<Viewport>, _ctx: &Invocation) -> Result<View, Error> {
        let Some(project) = host::projects()?.into_iter().next() else {
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

    fn handle(&mut self, _view: &str, event: ViewEvent, _ctx: &Invocation) -> Result<(), Error> {
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
            ViewEvent::Button(button) if button == "refresh" => Ok(host::refresh_view(ENDPOINTS)),
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
    use openidea_plugin::{Position, Range};
    use spec::PETSTORE;

    fn doc(text: &str) -> Document {
        Document { path: "/project/api.yaml".into(), language: "yaml".into(), version: 1, text: text.into() }
    }

    fn at(text: &str, needle: &str, plus: usize) -> Position {
        position(text, text.find(needle).unwrap() + plus)
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
        OpenApi.handle_events(&[Event::FileOpened("/project/api.yaml".into())], &host.invocation());
        assert_eq!(host.diagnostics()["/project/api.yaml"][0].code.as_deref(), Some("unresolved-ref"));
    }

    #[test]
    fn clears_problems_of_plain_yaml() {
        let host = MockHost::install().with_capabilities(&["project.read", "events"]);
        host.add_file("/project/k8s.yaml", "kind: Pod\nx:\n  $ref: '#/nope'\n");
        OpenApi.handle_events(&[Event::FileSaved("/project/k8s.yaml".into())], &host.invocation());
        assert!(host.diagnostics().get("/project/k8s.yaml").is_none_or(Vec::is_empty));
    }

    #[test]
    fn completes_local_refs_inside_the_quotes() {
        let _host = MockHost::install();
        let text = PETSTORE.replace("$ref: '#/components/responses/Pet'", "$ref: '#/comp");
        let from = text.find("#/comp\n").unwrap();
        let items = OpenApi.completion(&doc(&text), position(&text, from + 6), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"#/components/schemas/Pet") && labels.contains(&"#/components/responses/Pet"));
        assert_eq!(items[0].replace, Some(spring_text::range(&text, from, from + 6)));
        assert_eq!(items[0].kind, Some(CompletionKind::Reference));
    }

    #[test]
    fn the_replace_range_stops_before_an_auto_inserted_closing_quote() {
        let _host = MockHost::install();
        let text = "openapi: 3.0.0\ncomponents:\n  schemas:\n    Pet: {}\nx:\n  $ref: '#/comp'\n";
        let items = OpenApi.completion(&doc(text), at(text, "#/comp", 6), None);
        let from = text.find("#/comp").unwrap();
        assert_eq!(items[0].replace, Some(spring_text::range(text, from, from + 6)));
    }

    #[test]
    fn completes_pointers_into_the_named_file() {
        let host = MockHost::install();
        host.add_file("/project/common.yaml", "Pet:\n  type: object\n");
        let text = "openapi: 3.0.0\npaths: {}\nx:\n  $ref: common.yaml#/\n";
        let items = OpenApi.completion(&doc(text), at(text, "#/\n", 2), None);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["common.yaml#/Pet"]);
    }

    #[test]
    fn nothing_outside_openapi_files() {
        let _host = MockHost::install();
        let plain = "kind: Pod\nx:\n  $ref: '#/\n";
        assert!(OpenApi.completion(&doc(plain), at(plain, "#/", 2), None).is_empty());
        assert!(OpenApi.code_actions(&doc(plain), Range::default()).is_empty());
        assert!(OpenApi.completion(&doc(PETSTORE), at(PETSTORE, "title", 0), None).is_empty());
    }

    #[test]
    fn goes_to_local_and_file_targets() {
        let host = MockHost::install();
        let local = OpenApi.definition(&doc(PETSTORE), at(PETSTORE, "#/components/schemas/Pet", 5));
        assert_eq!((local[0].path.as_str(), local[0].range.start), ("/project/api.yaml", Position::new(36, 4)));
        host.add_file("/project/common.yaml", "Other:\n  type: string\nPet:\n  type: object\n");
        let text = "openapi: 3.0.0\npaths: {}\na:\n  $ref: 'common.yaml#/Pet'\nb:\n  $ref: common.yaml\n";
        let file = OpenApi.definition(&doc(text), at(text, "common.yaml#", 1));
        assert_eq!((file[0].path.as_str(), file[0].range.start.line), ("/project/common.yaml", 2));
        let whole = OpenApi.definition(&doc(text), at(text, "common.yaml\n", 1));
        assert_eq!((whole[0].path.as_str(), whole[0].range.start.line), ("/project/common.yaml", 0));
    }

    #[test]
    fn offers_the_schema_modeline_once() {
        let _host = MockHost::install();
        let actions = OpenApi.code_actions(&doc(PETSTORE), Range::default());
        assert_eq!(
            actions[0].edits[0].1[0].text,
            "# yaml-language-server: $schema=https://spec.openapis.org/oas/3.0/schema/2024-10-18\n"
        );
        let with = format!("{}{PETSTORE}", actions[0].edits[0].1[0].text);
        assert!(OpenApi.code_actions(&doc(&with), Range::default()).is_empty());
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
        let view = OpenApi.render(ENDPOINTS, None, &host.invocation()).unwrap();
        let Body::List(items) = view.body else { panic!("expected a list") };
        assert_eq!(
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            ["GET /pets", "GET /pets/{petId}", "DELETE /pets/{petId}"]
        );
        assert_eq!(items[0].detail.as_deref(), Some("listPets · openapi.yaml"));
        OpenApi.handle(ENDPOINTS, ViewEvent::Activate(items[1].key.clone()), &host.invocation()).unwrap();
        assert_eq!(host.opened_locations(), [("/project/api/openapi.yaml".to_owned(), 21, 4)]);
        OpenApi.handle(ENDPOINTS, ViewEvent::Button("refresh".into()), &host.invocation()).unwrap();
        assert_eq!(host.refreshed_views(), [ENDPOINTS]);
    }

    #[test]
    fn says_so_when_no_spec_is_found() {
        let host = MockHost::install();
        host.add_file("/project/k8s.yaml", "kind: Pod\n");
        assert!(matches!(OpenApi.render(ENDPOINTS, None, &host.invocation()).unwrap().body, Body::Message(_)));
    }
}
