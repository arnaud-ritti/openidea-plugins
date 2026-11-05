//! Problems in an OpenAPI document: unresolved `$ref`s, duplicate operationIds
//! and path parameters that disagree with their path template.

use crate::spec::{Key, Operation, Spec, operations, refs, split_ref, target};
use openidea_plugin::{Diagnostic, Severity};
use spring_text::range;
use std::collections::{HashMap, HashSet};

type Keys<'k> = HashMap<&'k str, &'k Key>;

struct Parameter {
    name: String,
    location: String,
    declared_at: Option<(usize, usize)>,
}

pub fn check(text: &str, spec: &Spec, read: &dyn Fn(&str) -> Option<String>) -> Vec<Diagnostic> {
    let ops = operations(text, &spec.keys);
    let mut found = unresolved_refs(text, spec, read);
    found.extend(duplicate_operation_ids(text, spec, &ops));
    found.extend(path_parameters(text, spec, &ops));
    found
}

fn problem(text: &str, (at, end): (usize, usize), severity: Severity, code: &str, message: String) -> Diagnostic {
    let mut diagnostic = Diagnostic::new(range(text, at, end), severity, message);
    diagnostic.code = Some(code.into());
    diagnostic
}

fn unresolved_refs(text: &str, spec: &Spec, read: &dyn Fn(&str) -> Option<String>) -> Vec<Diagnostic> {
    let mut files: HashMap<String, Option<Vec<Key>>> = HashMap::new();
    let mut found = Vec::new();

    for r in refs(text) {
        let Some((file, fragment)) = split_ref(&r.target) else {
            continue;
        };

        let message = if file.is_empty() {
            let resolves = fragment.is_empty() || target(&spec.keys, fragment).is_some();
            (!resolves).then(|| format!("Cannot find `{}`", r.target))
        } else {
            let keys = files.entry(file.to_owned()).or_insert_with(|| {
                let content = read(file)?;
                Some(if file.ends_with(".json") { Vec::new() } else { spring_text::config::yaml(&content) })
            });
            match keys {
                None => Some(format!("Cannot find file `{file}`")),
                Some(_) if file.ends_with(".json") || fragment.is_empty() => None,
                Some(keys) => target(keys, fragment).is_none().then(|| format!("Cannot find `{}`", r.target)),
            }
        };

        if let Some(message) = message {
            found.push(problem(text, (r.at, r.end), Severity::Error, "unresolved-ref", message));
        }
    }

    found
}

fn duplicate_operation_ids(text: &str, spec: &Spec, ops: &[Operation]) -> Vec<Diagnostic> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for id in ops.iter().filter_map(|op| op.operation_id.as_deref()).filter(|id| !id.is_empty()) {
        *counts.entry(id).or_default() += 1;
    }

    ops.iter()
        .filter_map(|op| {
            let id = op.operation_id.as_deref().filter(|id| counts.get(id).is_some_and(|&n| n > 1))?;
            let value = spec.keys.iter().find(|k| k.key == format!("{}.operationId", op.key))?;
            let message = format!("operationId `{id}` is used by more than one operation");

            Some(problem(text, (value.value_at, value.value_end), Severity::Error, "duplicate-operation-id", message))
        })
        .collect()
}

/// `None` when the parameters can't be read reliably: flow style, or a `$ref` that isn't local.
fn parameters(keys: &[Key], by_key: &Keys, list: &str) -> Option<Vec<Parameter>> {
    if by_key.get(list).is_some_and(|k| !k.value.is_empty()) {
        return None;
    }

    let mut found = Vec::new();
    for i in 0.. {
        let item = format!("{list}[{i}]");
        let item_children = format!("{item}.");
        if !keys.iter().any(|k| k.key == item || k.key.starts_with(&item_children)) {
            break;
        }

        if let Some(reference) = by_key.get(format!("{item}.$ref").as_str()) {
            let (file, fragment) = split_ref(&reference.value)?;
            if !file.is_empty() {
                return None;
            }

            let referenced = &target(keys, fragment)?.key;
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

fn path_parameters(text: &str, spec: &Spec, ops: &[Operation]) -> Vec<Diagnostic> {
    let by_key: Keys = spec.keys.iter().map(|k| (k.key.as_str(), k)).collect();
    let mut found = Vec::new();

    for op in ops {
        let path_item = &op.key[..op.key.len() - op.method.len() - 1];
        let (Some(shared), Some(own)) = (
            parameters(&spec.keys, &by_key, &format!("{path_item}.parameters")),
            parameters(&spec.keys, &by_key, &format!("{}.parameters", op.key)),
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
        let read = |f: &str| (f == "common.yaml").then(|| COMMON.to_owned());
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
}
