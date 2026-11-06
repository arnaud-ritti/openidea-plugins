//! Autowiring misuse in Java code: injection annotations on static members and
//! several `@Autowired` constructors. Quiet for foreign annotations and
//! non-literal `required` arguments.

use crate::checks::spring;
use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::java::{Annotation, JavaFile, Member, MemberKind, TypeDecl};

const SPRING_ANNOTATIONS: [&str; 2] = ["Autowired", "Value"];
const JSR_ANNOTATIONS: [&str; 2] = ["Inject", "Resource"];
const JSR_PACKAGES: [&str; 4] = ["jakarta.inject.", "javax.inject.", "jakarta.annotation.", "javax.annotation."];

pub fn check(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    static_members(file, ty, out);
    autowired_constructors(file, ty, out);
}

fn jsr<'a>(file: &JavaFile, annotations: &'a [Annotation], name: &str) -> Option<&'a Annotation> {
    let annotation = annotations.iter().find(|a| a.name == name)?;
    let suffix = format!(".{name}");
    let foreign = file
        .imports
        .iter()
        .filter(|i| !i.is_static && i.path.ends_with(&suffix))
        .any(|i| !JSR_PACKAGES.iter().any(|p| i.path.starts_with(p)));
    (!foreign).then_some(annotation)
}

fn injection_annotation<'a>(file: &JavaFile, member: &'a Member) -> Option<&'a Annotation> {
    let spring_found = SPRING_ANNOTATIONS.iter().find_map(|n| spring(file, &member.annotations, n));
    spring_found.or_else(|| JSR_ANNOTATIONS.iter().find_map(|n| jsr(file, &member.annotations, n)))
}

fn static_members(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let members = ty.members.iter().filter(|m| m.kind != MemberKind::Constructor);
    for member in members.filter(|m| m.modifiers.iter().any(|x| x == "static")) {
        let Some(annotation) = injection_annotation(file, member) else { continue };
        let (start, end) = (member.at, member.at + member.name.len());
        let spel = member.kind == MemberKind::Field
            && annotation.name == "Value"
            && annotation.args.iter().any(|a| a.strings.iter().any(|s| s.value.contains("#{")));
        if spel {
            out.push(Finding::new(
                start,
                end,
                "static-spel-injection",
                "A SpEL expression in @Value is never evaluated for a static field: Spring injects instance fields only, so the field keeps its default.",
            ));
            continue;
        }
        let what = if member.kind == MemberKind::Field { "field" } else { "method" };
        out.push(Finding::new(
            start,
            end,
            "static-injection",
            format!("@{} is not supported on a static {what}: Spring injects instance members of a bean only, so this is ignored.", annotation.name),
        ));
    }
}

fn autowired_constructors(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    let annotated: Vec<(&Member, &Annotation)> = ty
        .members
        .iter()
        .filter(|m| m.kind == MemberKind::Constructor)
        .filter_map(|m| Some((m, spring(file, &m.annotations, "Autowired")?)))
        .collect();
    if annotated.len() < 2 {
        return;
    }
    let mut required = Vec::new();
    for (constructor, annotation) in &annotated {
        match annotation.arg("required").map(|a| a.text.trim()) {
            None | Some("true") => required.push(*constructor),
            Some("false") => {}
            Some(_) => return,
        }
    }
    let Some(first) = required.first() else { return };
    for (constructor, _) in annotated.iter().filter(|(c, _)| !std::ptr::eq(*c, *first)) {
        out.push(Finding::new(
            constructor.at,
            constructor.at + constructor.name.len(),
            "autowired-constructors",
            "Only one constructor may be @Autowired with required = true: Spring can't tell which one to use and fails when the bean is created.",
        ).with_severity(Severity::Error));
    }
}
