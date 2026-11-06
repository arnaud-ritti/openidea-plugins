//! Annotation attributes that name another bean, a method or a resource of the
//! project. Quiet for foreign annotations, computed values and anything a
//! library could provide.

use crate::World;
use crate::checks::{build_module, spring, unresolved_bean};
use spring_text::finding::Finding;
use spring_text::java::{Annotation, JavaFile, TypeDecl};

const RESOURCE_ANNOTATIONS: [(&str, &[&str]); 5] = [
    ("ImportResource", &["locations", "value"]),
    ("PropertySource", &["value"]),
    ("TestPropertySource", &["locations", "value"]),
    ("ContextConfiguration", &["locations", "value"]),
    ("Sql", &["scripts", "value"]),
];

/// The values of the first of `keys` that is set, as `(value, start, end)`: string literals, or a project constant.
fn values(world: &World, annotation: &Annotation, keys: &[&str], own: &str) -> Vec<(String, usize, usize)> {
    let Some(arg) = keys.iter().find_map(|k| annotation.arg(k)) else { return Vec::new() };
    let lone_literal = arg.strings.len() == 1 && arg.text.matches('"').count() == 2 && arg.text.starts_with('"') && arg.text.ends_with('"');
    if arg.text.trim_start().starts_with('{') || lone_literal {
        return arg.strings.iter().map(|s| (s.value.clone(), s.at, s.end)).collect();
    }
    world.constant(&arg.text, own).map(|v| (v, arg.at, arg.end)).into_iter().collect()
}

fn has_value(annotation: &Annotation, key: &str, expected: &str) -> bool {
    annotation.arg(key).is_some_and(|a| a.text.trim() == expected)
}

pub fn unresolved_targets(path: &str, file: &JavaFile, ty: &TypeDecl, world: &World, out: &mut Vec<Finding>) {
    let on_type = ty.annotations.iter().map(|a| (a, None));
    let on_members = ty.members.iter().flat_map(|m| m.annotations.iter().map(move |a| (a, Some(m))));
    for (annotation, _) in on_type.chain(on_members) {
        let bean_names: &[&str] = match annotation.name.as_str() {
            "Lookup" => &["value"],
            "Scheduled" => &["scheduler"],
            "MockitoSpyBean" | "MockitoBean" => &["name", "value"],
            _ => &[],
        };
        if !bean_names.is_empty() && spring(file, std::slice::from_ref(annotation), &annotation.name).is_some() {
            for (name, from, to) in values(world, annotation, bean_names, &ty.name) {
                if name.is_empty() || name.contains("${") || name.contains("#{") || !unresolved_bean(world, &name) {
                    continue;
                }
                out.push(Finding::new(from, to, "unresolved-bean-name", format!("No bean named \"{name}\" is defined in the project. Libraries and auto-configuration can still provide it.")));
            }
        }
        if annotation.name == "ContextConfiguration" {
            default_context_file(path, file, ty, annotation, out);
        }
        let Some((_, keys)) = RESOURCE_ANNOTATIONS.iter().find(|(n, _)| *n == annotation.name) else { continue };
        if spring(file, std::slice::from_ref(annotation), &annotation.name).is_none() || has_value(annotation, "ignoreResourceNotFound", "true") {
            continue;
        }
        for (location, from, to) in values(world, annotation, keys, &ty.name) {
            if resource_exists(path, &file.package, &annotation.name, &location) != Some(false) {
                continue;
            }
            out.push(Finding::new(from, to, "resource-not-found", format!("No file \"{location}\" in the resource folders of this module. A build step can still generate it.")));
        }
    }
}

fn default_context_file(path: &str, file: &JavaFile, ty: &TypeDecl, annotation: &Annotation, out: &mut Vec<Finding>) {
    let bare = annotation.args.is_empty();
    let has_nested = file.types.iter().any(|t| t.outer.is_some_and(|o| file.types[o].name == ty.name));
    if !bare || has_nested || !ty.extends.is_empty() || ty.outer.is_some() || spring(file, std::slice::from_ref(annotation), "ContextConfiguration").is_none() {
        return;
    }
    let relative = format!("{}/{}-context.xml", file.package.replace('.', "/"), ty.name);
    if !matches!(read_classpath(path, &relative), Some(None)) {
        return;
    }
    let message = format!("@ContextConfiguration names no configuration, no nested @Configuration class exists and {relative} is missing from the resource folders.");
    out.push(Finding::new(annotation.at, annotation.end, "resource-not-found", message).with_severity(openidea_plugin::Severity::Info));
}

/// Whether `location` exists in the module's resources; none when it can't be told.
fn resource_exists(source: &str, package: &str, annotation: &str, location: &str) -> Option<bool> {
    if location.is_empty() || location.contains(['*', '$', '{', '#', '?', '\\']) || location.starts_with("classpath*:") {
        return None;
    }
    let relative = if let Some(rest) = location.strip_prefix("classpath:") {
        rest.trim_start_matches('/').to_owned()
    } else if location.contains(':') {
        return None;
    } else if let Some(rest) = location.strip_prefix('/') {
        rest.to_owned()
    } else if annotation == "PropertySource" {
        location.to_owned()
    } else {
        format!("{}/{location}", package.replace('.', "/"))
    };
    read_classpath(source, &relative).map(|found| found.is_some())
}

/// The text of `relative` in the resource folders of the module holding `source`: none without a project, `Some(None)` when absent.
pub(crate) fn read_classpath(source: &str, relative: &str) -> Option<Option<String>> {
    let project = spring_text::index::project_of(source)?;
    let root = project.root().ok()?;
    let module = build_module(source).unwrap_or_else(|| root.trim_end_matches('/').to_owned());
    Some(["src/main/resources", "src/test/resources"].iter().find_map(|dir| project.read_file(&format!("{module}/{dir}/{relative}")).ok()))
}

/// `initMethod`, `destroyMethod` and `@TestBean(methodName)` naming a method the project's classes lack.
pub fn lifecycle_methods(file: &JavaFile, ty: &TypeDecl, world: &World, out: &mut Vec<Finding>) {
    for method in ty.methods() {
        let (Some(bean), Some(returned)) = (spring(file, &method.annotations, "Bean"), method.ty.as_ref()) else { continue };
        let Some(chain) = world.member_chain(&returned.name, None) else { continue };
        // Spring looks the method up on the created object, so a subtype may define it.
        let subtyped = world.files.values().flat_map(|m| &m.types).any(|t| t.supers.contains(&returned.name));
        if chain[0].is_abstract || subtyped {
            continue;
        }
        for key in ["initMethod", "destroyMethod"] {
            for (name, from, to) in values(world, bean, &[key], &ty.name) {
                if name.is_empty() || name == "(inferred)" || name.contains("${") || chain.iter().any(|t| t.methods.iter().any(|m| m.name == name)) {
                    continue;
                }
                out.push(Finding::new(from, to, "bean-method-missing", format!("{} has no method {name}(), so Spring fails to create the bean.", returned.name)));
            }
        }
    }
    let test_class = ty.extends.is_empty() && ty.outer.is_none();
    for field in ty.fields().filter(|_| test_class) {
        let Some(test_bean) = spring(file, &field.annotations, "TestBean") else { continue };
        for (name, from, to) in values(world, test_bean, &["methodName"], &ty.name) {
            if ty.methods().any(|m| m.name == name) {
                continue;
            }
            out.push(Finding::new(from, to, "bean-method-missing", format!("{} has no method {name}() to create the @TestBean override.", ty.name)));
        }
    }
}

pub fn required_setters(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for method in ty.methods() {
        let Some(required) = spring(file, &method.annotations, "Required") else { continue };
        if method.name.starts_with("set") && method.name.len() > 3 && method.params.len() == 1 {
            continue;
        }
        out.push(Finding::new(required.at, required.end, "required-not-setter", "@Required only applies to a bean property setter (setX with one parameter); Spring ignores it here."));
    }
}

pub const SPRING_META_FILES: [&str; 2] = ["META-INF/spring.handlers", "META-INF/spring.schemas"];

fn root_of(qualified: &str) -> String {
    qualified.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// `META-INF/spring.handlers` classes and `META-INF/spring.schemas` files that the project should hold but doesn't.
pub fn spring_meta_findings(path: &str, text: &str, world: &World) -> Vec<Finding> {
    let mut out = Vec::new();
    if !world.complete {
        return out;
    }
    let handlers = path.ends_with("spring.handlers");
    let Some((resources, _)) = path.rsplit_once("/META-INF/") else { return out };
    let Some(project) = spring_text::index::project_of(path) else { return out };
    for key in spring_text::config::properties(text) {
        let value = key.value.trim();
        if value.is_empty() || value.contains(['$', '*']) {
            continue;
        }
        if handlers {
            let ours = world.files.values().flat_map(|m| &m.types).any(|t| root_of(&t.qualified) == root_of(value));
            if ours && world.project_type(&value.replace('$', ".")).is_none() {
                out.push(Finding::new(key.value_at, key.value_end, "spring-handler-class-missing", format!("No class {value} in the project's sources.")));
            }
        } else if !value.contains(':') && project.read_file(&format!("{resources}/{value}")).is_err() {
            out.push(Finding::new(key.value_at, key.value_end, "spring-schema-file-missing", format!("No file {value} in the resources folder this META-INF belongs to.")));
        }
    }
    out
}
