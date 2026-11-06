//! Beans and injection points of a Kotlin file: annotated classes and their
//! primary or secondary constructors, `@Bean fun`s, injected properties.
//! Declarations only; function bodies are skipped.

use crate::model::{self, FileModel, INJECT, Pointcut, TypeInfo};
use spring_text::java::{Annotation, Tok, TypeRef, annotation, closing, split_commas, tokens, type_ref};
use spring_text::range;

const MODIFIERS: [&str; 21] = [
    "public", "private", "protected", "internal", "open", "abstract", "final", "data", "sealed", "inner",
    "override", "lateinit", "const", "suspend", "inline", "operator", "companion", "enum", "value", "annotation",
    "tailrec",
];
const DECLARATIONS: [&str; 7] = ["class", "interface", "object", "fun", "val", "var", "constructor"];
const USE_SITE_TARGETS: [&str; 5] = ["field", "get", "set", "param", "property"];

/// Where a type expression ends: before a body, default value, delegation or the next declaration.
fn type_end(toks: &[Tok], mut j: usize) -> usize {
    while j < toks.len() {
        let s = toks[j].s;
        if matches!(s, "{" | "}" | "=" | "@" | "by" | ")" | ",") || DECLARATIONS.contains(&s) || MODIFIERS.contains(&s) {
            break;
        }
        j = if s == "<" || s == "(" { closing(toks, j) + 1 } else { j + 1 };
    }
    j
}

struct Param {
    annotations: Vec<Annotation>,
    name: String,
    ty: spring_text::java::TypeRef,
}

/// `(@Qualifier("a") private val a: A, b: List<B> = listOf())`, the parentheses at `open`.
fn params(text: &str, toks: &[Tok], open: usize) -> (Vec<Param>, usize) {
    let close = closing(toks, open);
    let mut out = Vec::new();
    for part in split_commas(&toks[open + 1..close.max(open + 1)], true) {
        let mut annotations = Vec::new();
        let mut k = 0;
        while k < part.len() {
            match part[k].s {
                "@" => k = annotation(text, part, k, &mut annotations),
                s if MODIFIERS.contains(&s) || s == "val" || s == "var" || s == "vararg" => k += 1,
                _ => break,
            }
        }
        let Some(name) = part.get(k).filter(|t| t.is_ident()) else { continue };
        if part.get(k + 1).is_none_or(|t| t.s != ":") {
            continue;
        }
        let end = type_end(part, k + 2);
        if let Some(ty) = type_ref(text, &part[k + 2..end]) {
            out.push(Param { annotations, name: name.s.to_owned(), ty });
        }
    }
    (out, close)
}

struct Class {
    name: String,
    owner: Option<usize>,
    profiles: Vec<String>,
    depth: usize,
}

pub fn model(text: &str) -> FileModel {
    let toks = tokens(text);
    let mut out = FileModel::default();
    let package = toks
        .iter()
        .position(|t| t.s == "package")
        .map(|i| toks[i + 1..].iter().take_while(|t| t.is_ident() || t.s == ".").map(|t| t.s).collect::<String>())
        .unwrap_or_default();
    let mut anns: Vec<Annotation> = Vec::new();
    let mut classes: Vec<Class> = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    let inject = |out: &mut FileModel, ps: &[Param], owner: Option<usize>| {
        for p in ps {
            model::push_injection(text, out, &p.ty, &p.name, &p.annotations, owner, None);
        }
    };
    while i < toks.len() {
        let t = toks[i];
        let prev = if i > 0 { toks[i - 1].s } else { "" };
        let next_ident = toks.get(i + 1).is_some_and(|t| t.is_ident());
        match t.s {
            "@" => {
                i = annotation(text, &toks, i, &mut anns);
                if toks.get(i).is_some_and(|t| t.s == ":") && anns.last().is_some_and(|a| USE_SITE_TARGETS.contains(&a.name.as_str())) {
                    anns.pop();
                    i = annotation(text, &toks, i, &mut anns);
                }
                continue;
            }
            "class" | "interface" | "object" if next_ident && prev != ":" && prev != "." => {
                let name = toks[i + 1];
                let mut j = i + 2;
                if toks.get(j).is_some_and(|t| t.s == "<") {
                    j = closing(&toks, j) + 1;
                }
                let mut ctor_anns = Vec::new();
                while let Some(t) = toks.get(j) {
                    match t.s {
                        "@" => j = annotation(text, &toks, j, &mut ctor_anns),
                        s if MODIFIERS.contains(&s) || s == "constructor" => j += 1,
                        _ => break,
                    }
                }
                let mut ctor = Vec::new();
                if toks.get(j).is_some_and(|t| t.s == "(") {
                    let (ps, close) = params(text, &toks, j);
                    ctor = ps;
                    j = close + 1;
                }
                let mut supers = Vec::new();
                if toks.get(j).is_some_and(|t| t.s == ":") {
                    j += 1;
                    let mut want = true;
                    while let Some(t) = toks.get(j) {
                        if t.s == "{" || t.s == "@" || DECLARATIONS.contains(&t.s) || MODIFIERS.contains(&t.s) {
                            break;
                        }
                        match t.s {
                            "(" | "<" => j = closing(&toks, j),
                            "," => want = true,
                            _ if want && t.is_ident() => {
                                while toks.get(j + 1).is_some_and(|t| t.s == ".") && toks.get(j + 2).is_some_and(|t| t.is_ident()) {
                                    j += 2;
                                }
                                supers.push(toks[j].s.to_owned());
                                want = false;
                            }
                            _ => {}
                        }
                        j += 1;
                    }
                }
                let qualified = if package.is_empty() { name.s.to_owned() } else { format!("{package}.{}", name.s) };
                out.types.push(TypeInfo {
                    name: name.s.to_owned(),
                    qualified,
                    supers: supers.clone(),
                    range: range(text, name.at, name.end()),
                });
                let kind = if t.s == "interface" { "interface" } else { "class" };
                let repository = model::is_repository(kind, supers.iter().map(String::as_str));
                let bean = if prev == "annotation" { None } else { model::class_bean(text, name.s, name.s, name.at, &anns, repository) };
                let profiles = bean.as_ref().map(|b| b.profiles.clone()).unwrap_or_default();
                let owner = bean.map(|b| {
                    out.beans.push(b);
                    out.beans.len() - 1
                });
                if owner.is_some() {
                    inject(&mut out, &ctor, owner);
                }
                anns.clear();
                if toks.get(j).is_some_and(|t| t.s == "{") {
                    depth += 1;
                    classes.push(Class { name: name.s.to_owned(), owner, profiles, depth });
                    j += 1;
                }
                i = j;
                continue;
            }
            "fun" => {
                let mut j = i + 1;
                if toks.get(j).is_some_and(|t| t.s == "<") {
                    j = closing(&toks, j) + 1;
                }
                while toks.get(j).is_some_and(|t| t.s != "(" && t.s != "{" && t.s != "}") {
                    j += 1;
                }
                if toks.get(j).is_none_or(|t| t.s != "(") {
                    anns.clear();
                    i = j;
                    continue;
                }
                let name = toks[j - 1];
                let (ps, close) = params(text, &toks, j);
                j = close + 1;
                let mut ret = None;
                if toks.get(j).is_some_and(|t| t.s == ":") {
                    let end = type_end(&toks, j + 1);
                    ret = type_ref(text, &toks[j + 1..end]);
                    j = end;
                } else if toks.get(j).is_some_and(|t| t.s == "=") {
                    ret = constructor_call_type(text, &toks, j + 1);
                }
                let class = classes.last();
                let has = |n: &[&str]| anns.iter().any(|a| n.contains(&a.name.as_str()));
                if has(&["Bean"]) {
                    let ty = ret.as_ref().map(|t| t.name.as_str()).unwrap_or_default();
                    let profiles = class.map(|c| c.profiles.clone()).unwrap_or_default();
                    out.beans.push(model::method_bean(text, name.s, name.at, ty, &anns, &profiles));
                    let owner = Some(out.beans.len() - 1);
                    inject(&mut out, &ps, owner);
                } else if has(&["Autowired", "Inject"]) {
                    inject(&mut out, &ps, class.and_then(|c| c.owner));
                } else if has(&["Pointcut"]) {
                    out.pointcuts.push(Pointcut {
                        name: name.s.to_owned(),
                        class: class.map(|c| c.name.clone()).unwrap_or_default(),
                        range: range(text, name.at, name.end()),
                    });
                }
                anns.clear();
                i = if toks.get(j).is_some_and(|t| t.s == "{") { closing(&toks, j) + 1 } else { j };
                continue;
            }
            "constructor" if toks.get(i + 1).is_some_and(|t| t.s == "(") => {
                let (ps, close) = params(text, &toks, i + 1);
                if let Some(owner) = classes.last().and_then(|c| c.owner) {
                    inject(&mut out, &ps, Some(owner));
                }
                anns.clear();
                let j = close + 1;
                i = if toks.get(j).is_some_and(|t| t.s == "{") { closing(&toks, j) + 1 } else { j };
                continue;
            }
            "val" | "var" if next_ident && toks.get(i + 2).is_some_and(|t| t.s == ":") => {
                let name = toks[i + 1];
                let end = type_end(&toks, i + 3);
                let injected = anns.iter().find(|a| INJECT.contains(&a.name.as_str()));
                if let (Some(_), Some(ty)) = (injected, type_ref(text, &toks[i + 3..end])) {
                    let annotation = injected.filter(|a| a.name != "Resource").map(|a| range(text, a.at, a.end));
                    let owner = classes.last().and_then(|c| c.owner);
                    model::push_injection(text, &mut out, &ty, name.s, &anns, owner, annotation);
                }
                anns.clear();
                i = end;
                continue;
            }
            "{" => depth += 1,
            "}" => {
                depth = depth.saturating_sub(1);
                if classes.last().is_some_and(|c| c.depth > depth) {
                    classes.pop();
                }
            }
            s if MODIFIERS.contains(&s) => {}
            _ => anns.clear(),
        }
        i += 1;
    }
    out
}

/// The type of an expression body `= Foo(…)`: only a bare constructor call of
/// a capitalised name that ends the declaration, never a chain or other expression.
fn constructor_call_type(text: &str, toks: &[Tok], at: usize) -> Option<TypeRef> {
    toks.get(at).filter(|t| t.is_ident() && t.s.starts_with(|c: char| c.is_ascii_uppercase()))?;
    toks.get(at + 1).filter(|t| t.s == "(")?;
    let next = toks.get(closing(toks, at + 1) + 1);
    let ends = next.is_none_or(|t| matches!(t.s, "@" | "}") || DECLARATIONS.contains(&t.s) || MODIFIERS.contains(&t.s));
    if !ends {
        return None;
    }
    type_ref(text, &toks[at..at + 1])
}
