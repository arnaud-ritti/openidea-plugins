//! Problems in an OpenAPI document: unresolved `$ref`s, duplicate operationIds
//! and path parameters that disagree with their path template.

use crate::spec::{Index, Key, Operation, Spec, exact, operations, refs, scan, split_ref};
use openidea_plugin::{Diagnostic, Severity};
use spring_text::range;
use std::collections::{HashMap, HashSet};

type Keys<'k> = HashMap<&'k str, &'k Key>;

struct Parameter {
    name: String,
    location: String,
    declared_at: Option<(usize, usize)>,
}

pub enum Fetched {
    Text(String),
    Missing,
    /// The file exists but can't be read; nothing is reported about refs into it.
    Unreadable,
}

enum Loaded {
    Keys(Vec<Key>),
    Missing,
    Unreadable,
}

pub fn check(text: &str, spec: &Spec, read: &dyn Fn(&str) -> Fetched) -> Vec<Diagnostic> {
    let ops = operations(text, &spec.keys);
    let by_key = exact(&spec.keys);
    let index = Index::new(&spec.keys);
    let mut found = unresolved_refs(text, &index, read);
    found.extend(duplicate_operation_ids(text, &by_key, &ops));
    found.extend(path_parameters(text, &by_key, &index, &ops));
    found
}

/// A file that looks like a spec without its root key: a `paths` object of `/…` items holding HTTP methods.
pub fn spec_candidate(text: &str) -> Vec<Diagnostic> {
    if !text.contains("paths") {
        return Vec::new();
    }

    let keys = scan(text);
    if keys.iter().any(|k| k.key == "openapi" || k.key == "swagger") {
        return Vec::new();
    }

    let Some(paths) = keys.iter().find(|k| k.key == "paths") else {
        return Vec::new();
    };

    if operations(text, &keys).is_empty() {
        return Vec::new();
    }

    let message = "This file has a `paths` object but no `openapi` or `swagger` key, so it is not treated as an API description".to_owned();

    vec![problem(text, (paths.at, paths.end), Severity::Info, "missing-openapi-root", message)]
}

fn problem(text: &str, (at, end): (usize, usize), severity: Severity, code: &str, message: String) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(range(text, at, end), severity, message);
    diagnostic.code = Some(code.into());
    diagnostic
}

fn unresolved_refs(text: &str, local: &Index, read: &dyn Fn(&str) -> Fetched) -> Vec<Diagnostic> {
    let found_refs = refs(text);
    let mut files: HashMap<String, Loaded> = HashMap::new();
    for r in &found_refs {
        let Some((file, _)) = split_ref(&r.target) else {
            continue;
        };
        if file.is_empty() {
            continue;
        }

        files.entry(file.to_owned()).or_insert_with(|| match read(file) {
            Fetched::Missing => Loaded::Missing,
            Fetched::Unreadable => Loaded::Unreadable,
            Fetched::Text(content) if content.trim_start().starts_with('[') => Loaded::Unreadable,
            Fetched::Text(content) => Loaded::Keys(scan(&content)),
        });
    }

    let indexes: HashMap<&str, Index> = files
        .iter()
        .filter_map(|(file, loaded)| match loaded {
            Loaded::Keys(keys) => Some((file.as_str(), Index::new(keys))),
            _ => None,
        })
        .collect();

    let mut found = Vec::new();
    for r in found_refs {
        let Some((file, fragment)) = split_ref(&r.target) else {
            continue;
        };

        let message = if file.is_empty() {
            let resolves = fragment.is_empty() || local.target(fragment).is_some();
            (!resolves).then(|| format!("Cannot find `{}`", r.target))
        } else {
            match (&files[file], indexes.get(file)) {
                (Loaded::Missing, _) => Some(format!("Cannot find file `{file}`")),
                (Loaded::Unreadable, _) => None,
                (Loaded::Keys(_), _) if fragment.is_empty() => None,
                (Loaded::Keys(_), index) => index
                    .is_some_and(|i| i.target(fragment).is_none())
                    .then(|| format!("Cannot find `{}`", r.target)),
            }
        };

        if let Some(message) = message {
            found.push(problem(text, (r.at, r.end), Severity::Error, "unresolved-ref", message));
        }
    }

    found
}

fn duplicate_operation_ids(text: &str, by_key: &Keys, ops: &[Operation]) -> Vec<Diagnostic> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in ops.iter().filter_map(|op| op.operation_id.as_deref()).filter(|id| !id.is_empty()) {
        *counts.entry(id).or_default() += 1;
    }

    ops.iter()
        .filter_map(|op| {
            let id = op.operation_id.as_deref().filter(|id| counts.get(id).is_some_and(|&n| n > 1))?;
            let value = by_key.get(format!("{}.operationId", op.key).as_str())?;
            let message = format!("operationId `{id}` is used by more than one operation");

            Some(problem(text, (value.value_at, value.value_end), Severity::Error, "duplicate-operation-id", message))
        })
        .collect()
}

/// `None` when the parameters can't be read reliably: flow style, or a `$ref` that isn't local.
fn parameters(index: &Index, by_key: &Keys, list: &str) -> Option<Vec<Parameter>> {
    if by_key.get(list).is_some_and(|k| !k.value.is_empty()) {
        return None;
    }

    let mut found = Vec::new();
    for i in 0.. {
        let item = format!("{list}[{i}]");
        if !index.contains(&item) {
            break;
        }

        if let Some(reference) = by_key.get(format!("{item}.$ref").as_str()) {
            let (file, fragment) = split_ref(&reference.value)?;
            if !file.is_empty() {
                return None;
            }

            let referenced = &index.target(fragment)?.key;
            let name = by_key.get(format!("{referenced}.name").as_str())?;
            let location = by_key.get(format!("{referenced}.in").as_str())?;
            found.push(Parameter { name: name.value.clone(), location: location.value.clone(), declared_at: None });
            continue;
        }

        if let (Some(name), Some(location)) =
            (by_key.get(format!("{item}.name").as_str()), by_key.get(format!("{item}.in").as_str()))
        {
            found.push(Parameter {
                name: name.value.clone(),
                location: location.value.clone(),
                declared_at: Some((name.value_at, name.value_end)),
            });
        }
    }

    Some(found)
}

fn template_names(path: &str) -> Vec<&str> {
    path.split('{').skip(1).filter_map(|rest| rest.split_once('}').map(|(name, _)| name)).collect()
}

fn path_parameters(text: &str, by_key: &Keys, index: &Index, ops: &[Operation]) -> Vec<Diagnostic> {
    let mut found = Vec::new();

    for op in ops {
        let path_item = &op.key[..op.key.len() - op.method.len() - 1];
        let (Some(shared), Some(own)) = (
            parameters(index, by_key, &format!("{path_item}.parameters")),
            parameters(index, by_key, &format!("{}.parameters", op.key)),
        ) else {
            continue;
        };

        let declared: Vec<&Parameter> = shared.iter().chain(&own).filter(|p| p.location == "path").collect();
        let template = template_names(&op.path);
        let declared_names: HashSet<&str> = declared.iter().map(|p| p.name.as_str()).collect();

        for name in template.iter().filter(|name| !declared_names.contains(**name)) {
            let message = format!("Path parameter `{name}` is not declared for {} {}", op.method, op.path);
            found.push(problem(text, (op.at, op.end), Severity::Error, "path-parameter", message));
        }

        for parameter in &declared {
            let Some(at) = parameter.declared_at else {
                continue;
            };
            if template.contains(&parameter.name.as_str()) {
                continue;
            }

            let message = format!("Path parameter `{}` is not in the path {}", parameter.name, op.path);
            found.push(problem(text, at, Severity::Warning, "path-parameter", message));
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{PETSTORE, parse};
    use openidea_plugin::Severity;

    const COMMON: &str = "Pet:\n  type: object\n";

    fn found(text: &str) -> Vec<(u32, Severity, String, String)> {
        let read = |f: &str| match f {
            "common.yaml" => Fetched::Text(COMMON.to_owned()),
            "locked.yaml" => Fetched::Unreadable,
            _ => Fetched::Missing,
        };
        check(text, &parse(text).unwrap(), &read)
            .into_iter()
            .map(|d| (d.range.start.line, d.severity, d.code.unwrap(), d.message))
            .collect()
    }

    #[test]
    fn the_petstore_is_clean() {
        assert!(found(PETSTORE).is_empty());
    }

    #[test]
    fn reports_unresolved_local_and_file_refs() {
        let text = "openapi: 3.0.0\npaths: {}\ncomponents:\n  schemas:\n    A:\n      $ref: '#/components/schemas/B'\n    C:\n      $ref: 'common.yaml#/Missing'\n    D:\n      $ref: 'common.yaml#/Pet'\n    E:\n      $ref: 'nope.yaml'\n    F:\n      $ref: 'https://example.com/x.yaml#/Y'\n";
        let e = |line, msg: &str| (line, Severity::Error, "unresolved-ref".to_owned(), msg.to_owned());
        assert_eq!(found(text), [
            e(5, "Cannot find `#/components/schemas/B`"),
            e(7, "Cannot find `common.yaml#/Missing`"),
            e(11, "Cannot find file `nope.yaml`"),
        ]);
    }

    #[test]
    fn a_ref_into_an_unreadable_file_is_not_reported() {
        let text = "openapi: 3.0.0\npaths: {}\nx:\n  $ref: 'locked.yaml#/Pet'\ny:\n  $ref: 'locked.yaml'\n";
        assert!(found(text).is_empty());
    }

    #[test]
    fn reports_every_duplicate_operation_id() {
        let text = "openapi: 3.0.0\npaths:\n  /a:\n    get:\n      operationId: same\n    post:\n      operationId: same\n";
        let lines: Vec<_> =
            found(text).into_iter().filter(|f| f.2 == "duplicate-operation-id").map(|f| f.0).collect();
        assert_eq!(lines, [4, 6]);
    }

    #[test]
    fn checks_path_parameters_against_the_template() {
        let text = "openapi: 3.0.0\npaths:\n  /a/{id}/{x}:\n    get:\n      parameters:\n        - name: id\n          in: path\n        - name: y\n          in: path\n      responses: {}\n";
        assert_eq!(found(text), [
            (3, Severity::Error, "path-parameter".into(), "Path parameter `x` is not declared for GET /a/{id}/{x}".into()),
            (7, Severity::Warning, "path-parameter".into(), "Path parameter `y` is not in the path /a/{id}/{x}".into()),
        ]);
    }

    #[test]
    fn skips_the_parameter_check_when_parameters_cannot_be_read() {
        let flow = "openapi: 3.0.0\npaths:\n  /a/{id}:\n    get:\n      parameters: [{ $ref: '#/components/parameters/Id' }]\n      responses: {}\ncomponents:\n  parameters:\n    Id: { name: id, in: path }\n";
        assert!(found(flow).iter().all(|f| f.2 != "path-parameter"));
        let external = "openapi: 3.0.0\npaths:\n  /a/{id}:\n    get:\n      parameters:\n        - $ref: 'common.yaml#/Pet'\n      responses: {}\n";
        assert!(found(external).iter().all(|f| f.2 != "path-parameter"));
    }

    #[test]
    fn checks_a_large_spec_quickly() {
        let mut text = String::from("openapi: 3.0.0\npaths:\n");
        for i in 0..3500 {
            text.push_str(&format!(
                "  /p{i}/{{id}}:\n    get:\n      operationId: op{i}\n      parameters:\n        - name: id\n          in: path\n      responses:\n        '200':\n          $ref: '#/components/schemas/S{}'\n",
                i % 200
            ));
        }
        text.push_str("components:\n  schemas:\n");
        for i in 0..200 {
            text.push_str(&format!("    S{i}:\n      type: object\n"));
        }
        assert!(text.lines().count() > 30_000);

        let started = std::time::Instant::now();
        let problems = found(&text);
        let elapsed = started.elapsed();
        assert!(problems.is_empty());
        assert!(elapsed.as_millis() < 1000, "took {elapsed:?}");
        eprintln!("large spec check: {elapsed:?}");
    }

    fn template(name: &str) -> String {
        std::fs::read_to_string(format!("{}/templates/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn templates_pass_the_openapi_checks() {
        for name in ["openapi.yaml", "openapi.json"] {
            let text = template(name);
            let spec = parse(&text).unwrap_or_else(|| panic!("{name} is not detected as an OpenAPI document"));
            assert_eq!(spec.version, "3.1.0", "{name}");
            assert!(found(&text).is_empty(), "{name}");
            assert_eq!(operations(&text, &spec.keys).len(), 1, "{name}");
        }
    }

    #[test]
    fn swagger_templates_are_registered_and_exist() {
        let manifest = std::fs::read_to_string(format!("{}/plugin.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let ids: Vec<&str> = manifest.lines().filter_map(|l| l.strip_prefix("id = \"")?.strip_suffix('"')).collect();
        for id in ["openapi-yaml", "openapi-json", "asyncapi-yaml", "swagger-yaml", "swagger-json"] {
            assert!(ids.contains(&id), "{id}");
        }
        assert_eq!(manifest.matches("[[contributes.fileTemplates]]").count(), 5);
        for name in ["swagger.yaml", "swagger.json"] {
            assert!(manifest.contains(&format!("path = \"templates/{name}\"")), "{name}");
            let text = template(name);
            assert!(text.contains("$NAME$"), "{name}");
            assert!(!text.replace("$NAME$", "").replace("$END$", "").contains('$'), "{name}");
        }
    }

    #[test]
    fn swagger_templates_are_detected_as_specs() {
        for name in ["swagger.yaml", "swagger.json"] {
            let text = template(name).replace("$NAME$", "Demo");
            let spec = parse(&text).unwrap_or_else(|| panic!("{name} is not detected as a spec"));
            assert_eq!(spec.version, "2.0", "{name}");
            assert!(found(&text).is_empty(), "{name}");
            assert_eq!(operations(&text, &spec.keys).len(), 1, "{name}");
        }
    }

    #[test]
    fn asyncapi_template_is_not_checked_as_openapi() {
        assert!(parse(&template("asyncapi.yaml")).is_none());
    }

    #[test]
    fn every_template_is_registered_and_exists() {
        let manifest = std::fs::read_to_string(format!("{}/plugin.toml", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut registered: Vec<&str> = manifest.lines().filter_map(|l| l.strip_prefix("path = \"templates/")?.strip_suffix('"')).collect();
        let mut files: Vec<String> = std::fs::read_dir(format!("{}/templates", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        registered.sort();
        files.sort();
        assert_eq!(registered, ["asyncapi.yaml", "openapi.json", "openapi.yaml", "swagger.json", "swagger.yaml"]);
        assert_eq!(files, registered);
        for name in registered {
            let text = template(name);
            for (at, _) in text.match_indices('$') {
                let word: String = text[at + 1..].chars().take_while(|c| c.is_ascii_uppercase() || *c == '_').collect();
                let closed = !word.is_empty() && text[at + 1 + word.len()..].starts_with('$');
                assert!(!closed || word == "NAME" || word == "END", "{name} has the placeholder ${word}$");
            }
        }
    }
}
