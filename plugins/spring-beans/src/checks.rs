//! Inspections over one Java file and the project's beans. Text-based: a
//! check stays quiet when an annotation comes from a library other than
//! Spring or a name is not a plain string literal.

use crate::aop;
use crate::model::{Bean, class_bean, method_bean, short_name};
use crate::{World, file_name};
use openidea_plugin::{Severity, host};
use spring_text::finding::Finding;
use spring_text::java::{self, Annotation, JavaFile, Member, STEREOTYPES, TypeDecl};

const PROXY_ANNOTATIONS: [&str; 11] = [
    "Transactional",
    "Async",
    "Cacheable",
    "CacheEvict",
    "CachePut",
    "Caching",
    "PreAuthorize",
    "PostAuthorize",
    "PreFilter",
    "PostFilter",
    "Retryable",
];
const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const ASPECTJ_WEAVING: [&str; 4] = ["aspectj-maven-plugin", "aspectj-gradle-plugin", "freefair.aspectj", "aspectjtools"];
const ASPECTJ_PACKAGE: &str = "org.aspectj.";
const POINTCUT_ANNOTATIONS: [&str; 6] = ["Pointcut", "Before", "After", "AfterReturning", "AfterThrowing", "Around"];
const SPRING_PACKAGES: [&str; 3] = ["org.springframework.", "jakarta.transaction.", "javax.transaction."];

/// Which groups of checks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Groups {
    pub proxy_methods: bool,
    pub scheduled: bool,
    pub duplicate_names: bool,
    pub configuration: bool,
    pub naming: bool,
    pub self_invocation: bool,
    pub unresolved_references: bool,
    pub aop: bool,
    pub xml: bool,
}

impl Default for Groups {
    fn default() -> Self {
        Self {
            proxy_methods: true,
            scheduled: true,
            duplicate_names: true,
            configuration: true,
            naming: true,
            self_invocation: false,
            unresolved_references: false,
            aop: true,
            xml: true,
        }
    }
}

impl Groups {
    pub fn from_settings() -> Self {
        let defaults = Self::default();
        let on = |key: &str, default: bool| host::setting_bool(&format!("springBeans.{key}")).unwrap_or(default);
        Self {
            proxy_methods: on("checkProxyMethods", defaults.proxy_methods),
            scheduled: on("checkScheduled", defaults.scheduled),
            duplicate_names: on("checkDuplicateNames", defaults.duplicate_names),
            configuration: on("checkConfiguration", defaults.configuration),
            naming: on("checkNaming", defaults.naming),
            self_invocation: on("checkSelfInvocation", defaults.self_invocation),
            unresolved_references: on("checkUnresolvedReferences", defaults.unresolved_references),
            aop: on("checkAop", defaults.aop),
            xml: on("checkXml", defaults.xml),
        }
    }
}

pub fn check_java(path: &str, file: &JavaFile, text: &str, world: &World, groups: Groups) -> Vec<Finding> {
    let mut out = Vec::new();
    for ty in file.types.iter().filter(|t| t.kind == "class") {
        if groups.proxy_methods {
            proxy_methods(file, ty, &mut out);
        }
        if groups.scheduled {
            scheduled(file, ty, &mut out);
        }
        if groups.configuration {
            configuration(file, ty, &mut out);
        }
        if groups.naming {
            naming(file, ty, &mut out);
        }
        if groups.duplicate_names {
            duplicate_names(path, file, ty, text, world, &mut out);
        }
        if groups.self_invocation {
            self_invocation(file, ty, text, &mut out);
        }
        if groups.aop {
            pointcut_designators(path, file, ty, &mut out);
        }
        if groups.unresolved_references && world.complete {
            depends_on(file, ty, world, &mut out);
            component_scan(file, ty, world, &mut out);
        }
    }
    out
}

/// The annotation `name` unless the file imports another one of that name.
fn spring<'a>(file: &JavaFile, annotations: &'a [Annotation], name: &str) -> Option<&'a Annotation> {
    let annotation = annotations.iter().find(|a| a.name == name)?;
    let suffix = format!(".{name}");
    let foreign = file
        .imports
        .iter()
        .filter(|i| !i.is_static && i.path.ends_with(&suffix))
        .any(|i| !SPRING_PACKAGES.iter().any(|p| i.path.starts_with(p)));
    (!foreign).then_some(annotation)
}

fn proxy_annotation<'a>(file: &JavaFile, method: &'a Member) -> Option<&'a Annotation> {
    PROXY_ANNOTATIONS.iter().find_map(|name| spring(file, &method.annotations, name))
}

fn name_span(method: &Member) -> (usize, usize) {
    (method.at, method.at + method.name.len())
}

fn modifier(method: &Member, name: &str) -> bool {
    method.modifiers.iter().any(|m| m == name)
}

fn proxy_methods(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for method in ty.methods() {
        let Some(annotation) = proxy_annotation(file, method) else { continue };
        let reason = ["private", "final", "static"].into_iter().find(|m| modifier(method, m));
        let Some(reason) = reason else { continue };
        let (start, end) = name_span(method);
        out.push(Finding::new(
            start,
            end,
            "non-proxyable-method",
            format!(
                "@{} has no effect on a {reason} method: Spring applies it through a proxy that can only intercept overridable instance methods.",
                annotation.name
            ),
        ));
    }
}

fn scheduled(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for method in ty.methods().filter(|m| !m.params.is_empty()) {
        if spring(file, &method.annotations, "Scheduled").is_none() {
            continue;
        }
        let (start, end) = name_span(method);
        out.push(Finding::new(
            start,
            end,
            "scheduled-signature",
            "A @Scheduled method can't declare parameters: the scheduler calls it with no arguments and rejects it at startup.",
        ));
    }
}

fn configuration(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let Some(configuration) = spring(file, &ty.annotations, "Configuration") else { return };
    let proxied = configuration.arg("proxyBeanMethods").is_none_or(|a| a.text.trim() == "true");
    if !proxied {
        return;
    }
    for method in ty.methods().filter(|m| spring(file, &m.annotations, "Bean").is_some()) {
        if modifier(method, "static") {
            continue;
        }
        let Some(reason) = ["private", "final"].into_iter().find(|m| modifier(method, m)) else { continue };
        let (start, end) = name_span(method);
        out.push(Finding::new(
            start,
            end,
            "configuration-proxy-method",
            format!(
                "A @Bean method of a proxied @Configuration class can't be {reason}: the class is subclassed so bean calls return the shared instance. Drop the modifier or set proxyBeanMethods = false."
            ),
        ));
    }
}

fn is_dynamic(annotation: &Annotation, keys: &[&str]) -> bool {
    let named = keys.iter().find_map(|k| annotation.arg(k));
    named.is_some_and(|a| a.strings.is_empty())
}

pub(crate) fn capitalised(name: &str) -> bool {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) => a.is_uppercase() && !b.is_uppercase(),
        (Some(a), None) => a.is_uppercase(),
        _ => false,
    }
}

fn naming(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let stereotype = STEREOTYPES.iter().filter(|s| **s != "SpringBootApplication").find_map(|s| spring(file, &ty.annotations, s));
    let methods = ty.methods().filter_map(|m| spring(file, &m.annotations, "Bean"));
    let declared = stereotype.map(|a| (a, &["value"][..])).into_iter().chain(methods.map(|a| (a, &["name", "value"][..])));
    for (annotation, keys) in declared {
        for name in annotation.strings(keys).into_iter().filter(|n| capitalised(&n.value)) {
            let message = format!(
                "Bean names conventionally start with a lowercase letter (\"{}\"), as the default names Spring derives do.",
                spring_text::decapitalize(&name.value)
            );
            out.push(Finding::new(name.at, name.end, "bean-name-convention", message).with_severity(Severity::Hint));
        }
    }
}

pub(crate) fn names_of(bean: &Bean) -> impl Iterator<Item = &str> {
    std::iter::once(bean.name.as_str()).chain(bean.aliases.iter().map(String::as_str))
}

pub(crate) fn profiles_overlap(a: &[String], b: &[String]) -> bool {
    if a.is_empty() || b.is_empty() {
        return true;
    }
    if a.iter().chain(b).any(|p| p.starts_with('!')) {
        return false;
    }
    a.iter().any(|p| b.contains(p))
}

fn is_test_source(path: &str) -> bool {
    path.contains("/src/test/")
}

/// The nearest ancestor directory holding a build file.
fn build_module(path: &str) -> Option<String> {
    let project = spring_text::index::project_of(path)?;
    let root = project.root().ok()?;
    let mut dir = path;
    while let Some((parent, _)) = dir.rsplit_once('/') {
        if !parent.starts_with(&root) {
            return None;
        }
        if BUILD_FILES.iter().any(|f| project.read_file(&format!("{parent}/{f}")).is_ok()) {
            return Some(parent.to_owned());
        }
        dir = parent;
    }
    None
}

/// Whether the module's build file names an AspectJ weaving plugin, which makes every designator valid.
pub fn weaving_plugin_in_build(path: &str) -> bool {
    let Some(module) = build_module(path) else { return false };
    let Some(project) = spring_text::index::project_of(path) else { return false };
    BUILD_FILES
        .iter()
        .filter_map(|f| project.read_file(&format!("{module}/{f}")).ok())
        .any(|build| ASPECTJ_WEAVING.iter().any(|marker| build.contains(marker)))
}

/// The AspectJ annotation `name` unless the file imports another one of that name.
fn aspectj<'a>(file: &JavaFile, annotations: &'a [Annotation], name: &str) -> Option<&'a Annotation> {
    let annotation = annotations.iter().find(|a| a.name == name)?;
    let suffix = format!(".{name}");
    let foreign = file.imports.iter().filter(|i| !i.is_static && i.path.ends_with(&suffix)).any(|i| !i.path.starts_with(ASPECTJ_PACKAGE));
    (!foreign).then_some(annotation)
}

fn pointcut_designators(path: &str, file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    if aspectj(file, &ty.annotations, "Aspect").is_none() {
        return;
    }
    let mut found = Vec::new();
    for method in ty.methods() {
        for name in POINTCUT_ANNOTATIONS {
            let Some(annotation) = aspectj(file, &method.annotations, name) else { continue };
            let Some(arg) = ["value", "pointcut"].iter().find_map(|k| annotation.arg(k)) else { continue };
            let [literal] = arg.strings.as_slice() else { continue };
            if arg.text.contains('\\') || arg.text.matches('"').count() != 2 || !arg.text.starts_with('"') || !arg.text.ends_with('"') {
                continue;
            }
            found.extend(aop::findings(&literal.value, literal.at));
        }
    }
    if !found.is_empty() && !weaving_plugin_in_build(path) {
        out.extend(found);
    }
}

/// Beans only clash inside one build module and one source set (main or test).
pub(crate) fn same_scope(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if is_test_source(a) != is_test_source(b) {
        return false;
    }
    match (build_module(a), build_module(b)) {
        (Some(x), Some(y)) => x == y,
        _ => true,
    }
}

fn static_name(annotations: &[Annotation], keys: &[&str]) -> bool {
    !annotations.iter().any(|a| is_dynamic(a, keys))
}

fn duplicate_names(path: &str, file: &JavaFile, ty: &TypeDecl, text: &str, world: &World, out: &mut Vec<Finding>) {
    let mut mine: Vec<(Bean, usize, usize)> = Vec::new();
    let stereotype = STEREOTYPES.iter().any(|s| spring(file, &ty.annotations, s).is_some());
    let class = stereotype.then(|| class_bean(text, &ty.name, &short_name(file, ty), ty.at, &ty.annotations, false)).flatten();
    let class_profiles = class.as_ref().map(|b| b.profiles.clone()).unwrap_or_default();
    let class_conditional = class.as_ref().is_some_and(|b| b.conditional);
    if let Some(bean) = class.filter(|_| static_name(&ty.annotations, &["value"])) {
        mine.push((bean, ty.at, ty.at + ty.name.len()));
    }
    for method in ty.methods().filter(|m| spring(file, &m.annotations, "Bean").is_some()) {
        if !static_name(&method.annotations, &["name", "value"]) || !static_name(&ty.annotations, &["value"]) {
            continue;
        }
        let returns = method.ty.as_ref().map(|t| t.name.as_str()).unwrap_or_default();
        let mut bean = method_bean(text, &method.name, method.at, returns, &method.annotations, &class_profiles);
        bean.conditional |= class_conditional;
        let (start, end) = name_span(method);
        mine.push((bean, start, end));
    }
    for (bean, start, end) in mine.into_iter().filter(|(b, _, _)| !b.conditional) {
        let clash = world.beans().find(|(other_path, other)| {
            let same = *other_path == path && other.range == bean.range;
            let overload = *other_path == path && other.kind == "@Bean" && bean.kind == "@Bean" && other.name == bean.name;
            let shares_name = names_of(other).any(|n| names_of(&bean).any(|m| m == n));
            !same
                && !overload
                && !other.conditional
                && shares_name
                && profiles_overlap(&bean.profiles, &other.profiles)
                && same_scope(path, other_path)
        });
        let Some((other_path, other)) = clash else { continue };
        out.push(Finding::new(
            start,
            end,
            "duplicate-bean-name",
            format!(
                "Another bean named \"{}\" is defined in {}: two beans with one name make the context fail or silently replace one.",
                bean.name,
                if other_path == path { other.name.as_str() } else { file_name(other_path) }
            ),
        ));
    }
}

fn self_invocation(file: &JavaFile, ty: &TypeDecl, text: &str, out: &mut Vec<Finding>) {
    if !ty.extends.is_empty() {
        return;
    }
    for callee in ty.methods() {
        let Some(annotation) = proxy_annotation(file, callee) else { continue };
        let plain = !["private", "final", "static"].iter().any(|m| modifier(callee, m));
        let unique = ty.methods().filter(|m| m.name == callee.name).count() == 1;
        if !plain || !unique {
            continue;
        }
        for caller in ty.methods().filter(|m| m.name != callee.name) {
            let Some((from, to)) = caller.body else { continue };
            let Some(body) = text.get(from..to) else { continue };
            if caller.annotations.iter().any(|a| a.name == annotation.name) {
                continue;
            }
            for call in java::calls(body).into_iter().filter(|c| c.name == callee.name && !c.is_new && !c.chained) {
                if call.receiver.as_deref().is_some_and(|r| r != "this") {
                    continue;
                }
                let at = from + call.at;
                out.push(Finding::new(
                    at,
                    at + call.name.len(),
                    "self-invocation",
                    format!(
                        "Calling @{} method {}() from the same class bypasses the Spring proxy, so the annotation does not apply. Call it through another bean.",
                        annotation.name, callee.name
                    ),
                ));
            }
        }
    }
}

fn project_types<'a>(world: &'a World) -> impl Iterator<Item = (&'a str, &'a str)> {
    world.files.values().flat_map(|m| m.types.iter().map(|t| (t.name.as_str(), t.qualified.as_str())))
}

/// A name that, ignoring case, is a project class or bean: a typo or a class that was never made a bean.
fn project_shaped(world: &World, name: &str) -> bool {
    project_types(world).any(|(simple, _)| simple.eq_ignore_ascii_case(name))
        || world.beans().any(|(_, b)| names_of(b).any(|n| n.eq_ignore_ascii_case(name)))
}

fn depends_on(file: &JavaFile, ty: &TypeDecl, world: &World, out: &mut Vec<Finding>) {
    let auto_configured = world.files.values().any(|m| m.auto_configuration)
        || world.beans().any(|(_, b)| b.kind == "@SpringBootApplication");
    let declared = ty.annotations.iter().chain(ty.methods().flat_map(|m| m.annotations.iter()));
    for annotation in declared.filter(|a| a.name == "DependsOn") {
        if spring(file, std::slice::from_ref(annotation), "DependsOn").is_none() || is_dynamic(annotation, &["value"]) {
            continue;
        }
        for name in annotation.strings(&["value"]) {
            if !world.named(&name.value).is_empty() || (auto_configured && !project_shaped(world, &name.value)) {
                continue;
            }
            out.push(Finding::new(
                name.at,
                name.end,
                "unresolved-depends-on",
                format!("No bean named \"{}\" is defined in the project. Libraries and auto-configuration can still provide it.", name.value),
            ));
        }
    }
}

/// The first two segments: `com.acme` for `com.acme.billing.Invoice`.
fn root(qualified: &str) -> String {
    qualified.split('.').take(2).collect::<Vec<_>>().join(".")
}

fn component_scan(file: &JavaFile, ty: &TypeDecl, world: &World, out: &mut Vec<Finding>) {
    let Some(scan) = spring(file, &ty.annotations, "ComponentScan") else { return };
    if scan.arg("basePackageClasses").is_some() || is_dynamic(scan, &["basePackages", "value"]) {
        return;
    }
    for package in scan.strings(&["basePackages", "value"]) {
        let wanted = package.value.trim();
        let ours = project_types(world).any(|(_, q)| root(q) == root(wanted));
        let present = project_types(world).any(|(_, q)| q.starts_with(&format!("{wanted}.")));
        if wanted.is_empty() || wanted.contains(['$', '*']) || !ours || present {
            continue;
        }
        out.push(Finding::new(
            package.at,
            package.end,
            "component-scan-package-missing",
            format!("No source of the project is in package \"{wanted}\", so this scan finds no component there."),
        ));
    }
}
