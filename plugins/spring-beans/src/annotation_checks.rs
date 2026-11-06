//! Annotation misuse in Java code: `@Profile` expressions and `@Async` return
//! types. Quiet for foreign annotations and non-literal arguments.

use crate::checks::spring;
use crate::model::is_repository;
use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::java::{self, Annotation, JavaFile, Member, TypeDecl};
use spring_text::sibling::SiblingCall;

const CACHE_ANNOTATIONS: [&str; 3] = ["Cacheable", "CachePut", "CacheEvict"];
const CACHE_NAME_ARGS: [&str; 3] = ["value", "cacheNames", "cacheResolver"];

const NON_FUTURE_RETURNS: [&str; 18] = [
    "boolean", "byte", "short", "char", "int", "long", "float", "double", "String", "Integer", "Long", "Double",
    "Boolean", "Object", "List", "Set", "Map", "Optional",
];

#[derive(Clone, Copy, PartialEq)]
enum Token {
    Name,
    Not,
    And,
    Or,
    Open,
    Close,
}

fn tokens(value: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut in_name = false;
    for c in value.chars() {
        let token = match c {
            '!' => Token::Not,
            '&' => Token::And,
            '|' => Token::Or,
            '(' => Token::Open,
            ')' => Token::Close,
            c => {
                if !c.is_whitespace() && !in_name {
                    out.push(Token::Name);
                    in_name = true;
                }
                continue;
            }
        };
        in_name = false;
        out.push(token);
    }
    out
}

/// What is wrong with a profile expression such as `a & !b`, if anything.
/// Repeated or trailing operators and juxtaposed operands stay quiet: Spring's
/// own parser may accept them.
pub fn profile_expression_problem(value: &str) -> Option<&'static str> {
    let tokens = tokens(value);
    if tokens.is_empty() {
        return Some("A profile name can't be empty: Spring rejects the value when the context is built.");
    }
    let mut at = 0;
    if let Err(problem) = expression(&tokens, &mut at) {
        return problem;
    }
    (tokens.get(at) == Some(&Token::Close)).then_some(STRAY_CLOSE)
}

const STRAY_CLOSE: &str = "A profile expression has a closing parenthesis without an opening one.";

/// `Err(None)` is a shape Spring may still accept.
type Parsed = Result<(), Option<&'static str>>;

fn expression(tokens: &[Token], at: &mut usize) -> Parsed {
    term(tokens, at)?;
    let mut operator = None;
    while let Some(&op) = tokens.get(*at).filter(|t| matches!(t, Token::And | Token::Or)) {
        if operator.is_some_and(|first| first != op) {
            return Err(Some("A profile expression can't mix & and | at one level: wrap one of them in parentheses, as in (a & b) | c."));
        }
        operator = Some(op);
        *at += 1;
        term(tokens, at)?;
    }
    Ok(())
}

fn term(tokens: &[Token], at: &mut usize) -> Parsed {
    let token = tokens.get(*at).copied();
    *at += 1;
    match token {
        Some(Token::Name) => Ok(()),
        Some(Token::Not) => {
            if !matches!(tokens.get(*at), Some(Token::Name | Token::Not | Token::Open)) {
                return Err(Some("A ! in a profile expression needs a profile name or a parenthesised expression after it."));
            }
            term(tokens, at)
        }
        Some(Token::Open) => {
            if tokens.get(*at) == Some(&Token::Close) {
                return Err(Some("A profile expression has empty parentheses: put a profile name inside."));
            }
            expression(tokens, at)?;
            match tokens.get(*at) {
                Some(Token::Close) => {
                    *at += 1;
                    Ok(())
                }
                None => Err(Some("A profile expression has an opening parenthesis without a closing one.")),
                Some(_) => Err(None),
            }
        }
        _ => Err(None),
    }
}

fn profile_values(file: &JavaFile, annotations: &[Annotation], out: &mut Vec<Finding>) {
    let Some(profile) = spring(file, annotations, "Profile") else { return };
    let Some(value) = profile.arg("value") else { return };
    if value.text.contains('+') {
        return;
    }
    for literal in &value.strings {
        if literal.value.contains("${") || literal.value.contains("#{") {
            continue;
        }
        let Some(problem) = profile_expression_problem(&literal.value) else { continue };
        out.push(Finding::new(literal.at, literal.end, "profile-expression", problem).with_severity(Severity::Error));
    }
}

pub fn check_profiles(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    profile_values(file, &ty.annotations, out);
    for method in ty.methods() {
        profile_values(file, &method.annotations, out);
    }
}

pub fn check_async(file: &JavaFile, ty: &TypeDecl, out: &mut Vec<Finding>) {
    for method in ty.methods() {
        if spring(file, &method.annotations, "Async").is_none() {
            continue;
        }
        let Some(returned) = &method.ty else { continue };
        if !NON_FUTURE_RETURNS.contains(&returned.name.as_str()) {
            continue;
        }
        out.push(Finding::new(
            returned.at,
            returned.at + returned.name.len(),
            "async-return-type",
            format!("An @Async method should return void or a Future: the caller gets null instead of this {}, because the call returns before the work runs.", returned.name),
        ));
    }
}

fn lacks_cache_name(annotation: &Annotation) -> bool {
    CACHE_NAME_ARGS.iter().all(|name| annotation.arg(name).is_none())
}

/// Cache annotations on `annotations`, as findings on the annotation itself.
fn cache_annotations<'a>(file: &JavaFile, annotations: &'a [Annotation]) -> Vec<&'a Annotation> {
    CACHE_ANNOTATIONS.iter().filter_map(|name| spring(file, annotations, name)).collect()
}

fn check_cache_config(file: &JavaFile, ty: &TypeDecl, subtyped: bool, out: &mut Vec<Finding>) {
    let Some(config) = spring(file, &ty.annotations, "CacheConfig") else { return };
    if let (Some(_), Some(other)) = (config.arg("cacheManager"), config.arg("cacheResolver")) {
        out.push(Finding::new(other.at, other.end, "cache-attribute-conflict", "@CacheConfig sets both cacheManager and cacheResolver: Spring rejects the configuration, use one.").with_severity(Severity::Error));
    }
    let used = ty.methods().any(|m| !cache_annotations(file, &m.annotations).is_empty() || spring(file, &m.annotations, "Caching").is_some());
    if used || subtyped || ty.kind != "class" || ty.modifiers.iter().any(|m| m == "abstract") {
        return;
    }
    out.push(Finding::new(config.at, config.end, "cache-config-unused", "@CacheConfig only supplies defaults to cache annotations on the methods of this class, and none has one.").with_severity(Severity::Hint));
}

pub fn check_cache(file: &JavaFile, ty: &TypeDecl, subtyped: bool, out: &mut Vec<Finding>) {
    if ty.kind == "interface" {
        if is_repository(&ty.kind, ty.supers().map(|s| s.name.as_str())) {
            return;
        }
        let on_type = cache_annotations(file, &ty.annotations);
        let on_methods = ty.methods().flat_map(|m| cache_annotations(file, &m.annotations));
        for annotation in on_type.into_iter().chain(on_methods) {
            out.push(Finding::new(
                annotation.at,
                annotation.end,
                "cache-annotation-on-interface",
                "Cache annotations on an interface only take effect with interface-based (JDK) proxies; class-based proxies and AspectJ mode miss them. Annotate the concrete class instead.",
            ).with_severity(Severity::Hint));
        }
        return;
    }

    check_cache_config(file, ty, subtyped, out);
    for method in ty.methods() {
        cache_expressions(&method.annotations, file, out);
    }

    // A supertype may carry the @CacheConfig: Spring looks it up through the hierarchy.
    let names_supplied_by_class = spring(file, &ty.annotations, "CacheConfig").is_some() || ty.supers().next().is_some();
    for method in ty.methods() {
        if spring(file, &method.annotations, "Caching").is_some() {
            continue;
        }
        let cache_annotations = cache_annotations(file, &method.annotations);
        if !names_supplied_by_class {
            for annotation in cache_annotations.iter().filter(|a| lacks_cache_name(a)) {
                out.push(Finding::new(
                    annotation.at,
                    annotation.end,
                    "cache-names-missing",
                    format!("@{} names no cache: set value or cacheNames, or put @CacheConfig(cacheNames = ...) on the class, or Spring fails when the method runs.", annotation.name),
                ));
            }
        }
        if let Some(put) = cache_annotations.iter().find(|a| a.name == "CachePut") {
            let Some(cacheable) = cache_annotations.iter().find(|a| a.name == "Cacheable") else { continue };
            let second = if put.at > cacheable.at { put } else { cacheable };
            out.push(Finding::new(
                second.at,
                second.end,
                "cacheable-and-cache-put",
                "@Cacheable skips the method when the entry exists, while @CachePut always runs it: combining them on one method gives surprising results.",
            ));
        }
    }
}

/// Like `spring_text::sibling::sibling_calls`, but a name shared by overloads counts when every
/// overload is a plain target: the text cannot tell which one a call picks.
pub fn overloaded_sibling_calls<'a>(
    ty: &'a TypeDecl,
    text: &str,
    is_target: impl Fn(&Member) -> bool,
    excused: impl Fn(&Member, &Member) -> bool,
) -> Vec<SiblingCall<'a>> {
    let mut out = Vec::new();
    if !ty.extends.is_empty() {
        return out;
    }
    let plain = |m: &Member| !["private", "final", "static"].iter().any(|k| m.modifiers.iter().any(|x| x == k));
    let mut seen = Vec::new();
    for callee in ty.methods() {
        if seen.contains(&callee.name.as_str()) {
            continue;
        }
        seen.push(callee.name.as_str());
        if !ty.methods().filter(|m| m.name == callee.name).all(|m| is_target(m) && plain(m)) {
            continue;
        }
        for caller in ty.methods().filter(|m| m.name != callee.name && !excused(m, callee)) {
            let Some((from, to)) = caller.body else { continue };
            let Some(body) = text.get(from..to) else { continue };
            for call in java::calls(body).into_iter().filter(|c| c.name == callee.name && !c.is_new && !c.chained) {
                if call.receiver.as_deref().is_some_and(|r| r != "this") {
                    continue;
                }
                let at = from + call.at;
                out.push(SiblingCall { callee, caller, at, end: at + call.name.len() });
            }
        }
    }
    out
}

pub fn check_bean_method_calls(file: &JavaFile, ty: &TypeDecl, text: &str, out: &mut Vec<Finding>) {
    let Some(configuration) = spring(file, &ty.annotations, "Configuration") else { return };
    if configuration.arg("proxyBeanMethods").is_none_or(|a| a.text.trim() != "false") {
        return;
    }
    let is_bean = |m: &Member| spring(file, &m.annotations, "Bean").is_some();
    let excused = |caller: &Member, _: &Member| !is_bean(caller);
    for call in overloaded_sibling_calls(ty, text, is_bean, excused) {
        out.push(Finding::new(
            call.at,
            call.end,
            "unproxied-bean-method-call",
            format!(
                "{}() is called directly in a @Configuration(proxyBeanMethods = false) class, so every call creates a new instance instead of returning the shared bean. Take it as a parameter of this @Bean method instead.",
                call.callee.name
            ),
        ));
    }
}

const SPEL_ARGS: [&str; 3] = ["key", "condition", "unless"];

/// What is wrong with the brackets and quotes of a SpEL expression, if anything.
pub fn spel_problem(expr: &str) -> Option<&'static str> {
    let mut open = Vec::new();
    let mut quoted = false;
    for c in expr.chars() {
        if quoted {
            quoted = c != '\'';
            continue;
        }
        match c {
            '\'' => quoted = true,
            '(' | '[' | '{' => open.push(c),
            ')' | ']' | '}' => {
                let wanted = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if open.pop() != Some(wanted) {
                    return Some("has a closing bracket without a matching opening one");
                }
            }
            _ => {}
        }
    }
    if quoted {
        return Some("has a string literal that is never closed");
    }
    (!open.is_empty()).then_some("has an opening bracket that is never closed")
}

fn cache_expressions(annotations: &[Annotation], file: &JavaFile, out: &mut Vec<Finding>) {
    for annotation in cache_annotations(file, annotations) {
        for name in SPEL_ARGS {
            let Some(arg) = annotation.arg(name) else { continue };
            let [literal] = arg.strings.as_slice() else { continue };
            if arg.text.matches('"').count() != 2 || !arg.text.starts_with('"') || !arg.text.ends_with('"') {
                continue;
            }
            let Some(problem) = spel_problem(&literal.value) else { continue };
            out.push(Finding::new(literal.at, literal.end, "cache-spel-syntax", format!("The {name} expression {problem}: Spring fails to parse it when the method first runs.")).with_severity(Severity::Error));
        }
        for (first, second) in [("key", "keyGenerator"), ("cacheManager", "cacheResolver")] {
            let (Some(_), Some(other)) = (annotation.arg(first), annotation.arg(second)) else { continue };
            out.push(Finding::new(other.at, other.end, "cache-attribute-conflict", format!("@{} sets both {first} and {second}: Spring rejects the configuration, use one.", annotation.name)).with_severity(Severity::Error));
        }
    }
}
