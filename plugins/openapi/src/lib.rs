//! OpenAPI: editing support for OpenAPI 3 and Swagger 2 files.

mod checks;
#[allow(dead_code)]
mod spec;

use openidea_plugin::{Diagnostic, Error, Event, Invocation, LogLevel, Plugin, Project, host, register_plugin};

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

    let read = |file: &str| project.read_file(&resolve_path(path, file)).ok();

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

register_plugin!(OpenApi);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

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
}
