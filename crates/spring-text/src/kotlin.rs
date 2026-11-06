//! A Kotlin declaration scanner producing the same model as [`crate::java`]:
//! classes, interfaces, objects (`object`, `companion object` as `Companion`)
//! with their annotations and supertypes; `fun`s as methods, `val`/`var` as
//! fields, the primary constructor as a constructor whose `val`/`var`
//! parameters are fields too. Bodies are skipped. Offsets are UTF-8 bytes.

use crate::java::{
    Annotation, Call, JavaFile, Member, MemberKind, Param, Tok, TypeDecl, TypeRef, annotation, closing, split_commas,
    tokens, type_ref,
};

const MODIFIERS: [&str; 30] = [
    "public",
    "private",
    "protected",
    "internal",
    "open",
    "abstract",
    "final",
    "sealed",
    "data",
    "enum",
    "annotation",
    "inner",
    "companion",
    "override",
    "lateinit",
    "const",
    "suspend",
    "inline",
    "operator",
    "infix",
    "tailrec",
    "external",
    "value",
    "expect",
    "actual",
    "vararg",
    "noinline",
    "crossinline",
    "reified",
    "fun", // `fun interface`; a function's own `fun` is checked first
];
/// Tokens that start a declaration on a new line: where an expression body ends.
const DECLARATION_START: [&str; 9] = ["fun", "val", "var", "class", "interface", "object", "constructor", "init", "@"];
const NOT_CALLS: [&str; 6] = ["when", "fun", "if", "while", "for", "catch"];

fn newline_before(text: &str, toks: &[Tok], i: usize) -> bool {
    i > 0 && i < toks.len() && text[toks[i - 1].end()..toks[i].at].contains('\n')
}

/// The index of the first token after an expression starting at `from`: a
/// `;` or `}` at its level, or a declaration starting on a new line.
fn expression_end(text: &str, toks: &[Tok], from: usize) -> usize {
    let mut depth = 0i32;
    let mut k = from;
    while k < toks.len() {
        let t = toks[k].s;
        if depth == 0
            && k > from
            && (t == ";"
                || t == "}"
                || newline_before(text, toks, k) && (DECLARATION_START.contains(&t) || MODIFIERS.contains(&t) && t != "fun"))
        {
            return k;
        }
        match t {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return k;
        }
        k += 1;
    }
    k
}

/// The tokens of a type after `:` from `from`: up to `{`, `=`, `by`, `where`
/// or a new line outside brackets. The index after it.
fn type_end(text: &str, toks: &[Tok], from: usize) -> usize {
    let mut depth = 0i32;
    let mut k = from;
    while k < toks.len() {
        let t = toks[k].s;
        if depth == 0 && k > from && (matches!(t, "{" | "=" | "by" | "where" | ")" | ",") || newline_before(text, toks, k))
        {
            return k;
        }
        match t {
            "<" | "(" => depth += 1,
            ">" | ")" => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    k
}

/// Parameters of a primary constructor or function: each with whether it is
/// a `val`/`var` property.
fn params(src: &str, toks: &[Tok]) -> Vec<(Param, bool)> {
    split_commas(toks, true)
        .into_iter()
        .filter_map(|p| {
            let mut annotations = Vec::new();
            let mut property = false;
            let mut k = 0;
            while k < p.len() {
                match p[k].s {
                    "@" => k = annotation(src, p, k, &mut annotations),
                    "val" | "var" => {
                        property = true;
                        k += 1;
                    }
                    s if MODIFIERS.contains(&s) => k += 1,
                    _ => break,
                }
            }
            let name = p.get(k).filter(|t| t.is_ident())?;
            if p.get(k + 1).is_none_or(|t| t.s != ":") {
                return None;
            }
            let end = p.iter().skip(k + 2).position(|t| t.s == "=").map_or(p.len(), |e| k + 2 + e);
            let ty = type_ref(src, &p[k + 2..end])?;
            Some((Param { annotations, ty, name: name.s.to_owned(), at: name.at }, property))
        })
        .collect()
}

/// Whether a string literal's raw text has a `$name` or `${…}` template.
fn is_template(raw: &str) -> bool {
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'$' if b.get(i + 1).is_some_and(|c| *c == b'{' || *c == b'_' || c.is_ascii_alphabetic()) => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

fn drop_templates(text: &str, annotations: &mut [Annotation]) {
    for a in annotations {
        for arg in &mut a.args {
            arg.strings.retain(|s| !is_template(&text[s.at..s.end]));
        }
    }
}

/// Parses the declarations of a Kotlin source. A string with a template
/// (`"$prefix-orders"`) is no literal: annotation strings drop it.
pub fn parse(text: &str) -> JavaFile {
    let mut file = parse_declarations(text);
    drop_templates(text, &mut file.package_annotations);
    for t in &mut file.types {
        drop_templates(text, &mut t.annotations);
        for p in &mut t.components {
            drop_templates(text, &mut p.annotations);
        }
        for m in &mut t.members {
            drop_templates(text, &mut m.annotations);
            for p in &mut m.params {
                drop_templates(text, &mut p.annotations);
            }
        }
    }
    file
}

fn parse_declarations(text: &str) -> JavaFile {
    let toks = tokens(text);
    let mut out = JavaFile::default();
    let mut annotations: Vec<Annotation> = Vec::new();
    let mut modifiers: Vec<String> = Vec::new();
    let mut start: Option<usize> = None;
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut depth = 0;
    let mut i = 0;
    let line_end = |from: usize| (from..toks.len()).find(|&k| k > from && newline_before(text, &toks, k)).unwrap_or(toks.len());
    while i < toks.len() {
        let t = toks[i];
        let member_level = stack.last().is_none_or(|&(_, d)| d == depth);
        if depth == 0 && (t.s == "package" || t.s == "import") {
            let end = line_end(i);
            let path_end = (i + 1..end).find(|&k| toks[k].s == "as" || toks[k].s == ";").unwrap_or(end);
            if i + 1 < path_end {
                let path: String = toks[i + 1..path_end].iter().map(|t| t.s).collect();
                if t.s == "package" {
                    out.package = path;
                    out.package_annotations = std::mem::take(&mut annotations);
                } else {
                    let (at, to) = (toks[i + 1].at, toks[path_end - 1].end());
                    out.imports.push(crate::java::Import { path, is_static: false, at, end: to });
                }
            }
            i = end;
            continue;
        }
        if t.s == "@" {
            start.get_or_insert(t.at);
            i = annotation(text, &toks, i, &mut annotations).max(i + 1);
            continue;
        }
        let next = toks.get(i + 1).map(|n| n.s);
        if member_level
            && MODIFIERS.contains(&t.s)
            && next.is_some_and(|n| !matches!(n, "(" | "." | ":" | "=" | ")" | ","))
            && !(t.s == "fun" && next.is_some_and(|n| n != "interface"))
        {
            start.get_or_insert(t.at);
            modifiers.push(t.s.to_owned());
            i += 1;
            continue;
        }
        if member_level && matches!(t.s, "class" | "interface" | "object") && (i == 0 || toks[i - 1].s != ".") {
            let named = toks.get(i + 1).is_some_and(|n| n.is_ident());
            if !named && t.s != "object" {
                i += 1;
                continue;
            }
            let (name, name_at, mut j) = if named {
                (toks[i + 1].s.to_owned(), toks[i + 1].at, i + 2)
            } else {
                ("Companion".to_owned(), t.at, i + 1)
            };
            let kind = match t.s {
                "class" if modifiers.iter().any(|m| m == "enum") => "enum",
                "class" if modifiers.iter().any(|m| m == "annotation") => "@interface",
                other => other,
            };
            let mut decl = TypeDecl {
                name,
                at: name_at,
                kind: kind.to_owned(),
                annotations: std::mem::take(&mut annotations),
                modifiers: std::mem::take(&mut modifiers),
                extends: Vec::new(),
                implements: Vec::new(),
                components: Vec::new(),
                members: Vec::new(),
                outer: stack.last().map(|&(o, _)| o),
                start: start.take().unwrap_or(t.at),
                end: text.len(),
            };
            if toks.get(j).is_some_and(|t| t.s == "<") {
                j = closing(&toks, j) + 1;
            }
            // Primary constructor: `@Inject constructor(…)` or `(…)`.
            let mut ctor_annotations = Vec::new();
            let mut k = j;
            while k < toks.len() && !newline_before(text, &toks, k).then_some(()).is_some_and(|_| k > j) {
                match toks[k].s {
                    "@" => k = annotation(text, &toks, k, &mut ctor_annotations),
                    s if MODIFIERS.contains(&s) || s == "constructor" => k += 1,
                    _ => break,
                }
            }
            if toks.get(k).is_some_and(|t| t.s == "(") {
                let close = closing(&toks, k);
                let params = params(text, &toks[k + 1..close.max(k + 1)]);
                for (p, property) in &params {
                    if *property {
                        decl.members.push(Member {
                            kind: MemberKind::Field,
                            name: p.name.clone(),
                            at: p.at,
                            annotations: p.annotations.clone(),
                            modifiers: Vec::new(),
                            ty: Some(p.ty.clone()),
                            params: Vec::new(),
                            body: None,
                            start: p.at,
                            end: p.at + p.name.len(),
                        });
                    }
                }
                decl.members.push(Member {
                    kind: MemberKind::Constructor,
                    name: decl.name.clone(),
                    at: decl.at,
                    annotations: ctor_annotations,
                    modifiers: Vec::new(),
                    ty: None,
                    params: params.into_iter().map(|(p, _)| p).collect(),
                    body: None,
                    start: toks[k].at,
                    end: toks[close].end(),
                });
                j = close + 1;
            }
            if toks.get(j).is_some_and(|t| t.s == ":") {
                let end = (j + 1..toks.len())
                    .find(|&k| toks[k].s == "{" || toks[k].s == "where" || newline_before(text, &toks, k) && !matches!(toks[k - 1].s, "," | ":"))
                    .unwrap_or(toks.len());
                for part in split_commas(&toks[j + 1..end], true) {
                    let part = part.iter().position(|t| t.s == "by").map_or(part, |b| &part[..b]);
                    let call = part.iter().position(|t| t.s == "(");
                    let Some(ty) = type_ref(text, &part[..call.unwrap_or(part.len())]) else { continue };
                    if kind == "interface" || call.is_some() {
                        decl.extends.push(ty);
                    } else {
                        decl.implements.push(ty);
                    }
                }
                j = end;
            }
            let is_enum = decl.kind == "enum";
            out.types.push(decl);
            let index = out.types.len() - 1;
            if toks.get(j).is_some_and(|t| t.s == "{") {
                depth += 1;
                stack.push((index, depth));
                i = j + 1;
                if is_enum {
                    i = skip_enum_entries(&toks, i);
                }
            } else {
                out.types[index].end = toks.get(j.saturating_sub(1)).map_or(text.len(), |t| t.end());
                i = j;
            }
            continue;
        }
        if member_level && !stack.is_empty() && matches!(t.s, "fun" | "val" | "var" | "constructor") {
            let (o, _) = *stack.last().unwrap();
            let (member, next) = member(text, &toks, i, &out.types[o].name);
            if let Some(mut member) = member {
                member.annotations.splice(0..0, std::mem::take(&mut annotations));
                member.modifiers.splice(0..0, std::mem::take(&mut modifiers));
                member.start = start.take().unwrap_or(member.start);
                out.types[o].members.push(member);
            }
            annotations.clear();
            modifiers.clear();
            start = None;
            i = next.max(i + 1);
            continue;
        }
        match t.s {
            "init" if member_level && next == Some("{") => {
                i = closing(&toks, i + 1) + 1;
                continue;
            }
            "{" => depth += 1,
            "}" => {
                if let Some(&(o, d)) = stack.last()
                    && d == depth
                {
                    out.types[o].end = t.end();
                    stack.pop();
                }
                depth = depth.saturating_sub(1);
            }
            "fun" | "val" | "var" if member_level => {
                // Top-level declarations: skip them whole.
                let (_, next) = member(text, &toks, i, "");
                i = next.max(i + 1);
                annotations.clear();
                modifiers.clear();
                start = None;
                continue;
            }
            _ => {}
        }
        annotations.clear();
        modifiers.clear();
        start = None;
        i += 1;
    }
    out
}

fn skip_enum_entries(toks: &[Tok], mut i: usize) -> usize {
    while i < toks.len() {
        match toks[i].s {
            "(" | "{" => i = closing(toks, i) + 1,
            ";" => return i + 1,
            "}" => return i,
            _ => i += 1,
        }
    }
    i
}

/// Parses the `fun`, `val`/`var` or `constructor` at `i`: it (if well formed) and the index after it.
fn member(src: &str, toks: &[Tok], i: usize, class: &str) -> (Option<Member>, usize) {
    let keyword = toks[i].s;
    let mut j = i + 1;
    if toks.get(j).is_some_and(|t| t.s == "<") {
        j = closing(toks, j) + 1;
    }
    if keyword == "constructor" {
        if toks.get(j).is_none_or(|t| t.s != "(") {
            return (None, j);
        }
        let close = closing(toks, j);
        let params = params(src, &toks[j + 1..close.max(j + 1)]).into_iter().map(|(p, _)| p).collect();
        let mut end = close + 1;
        if toks.get(end).is_some_and(|t| t.s == ":") {
            end = expression_end(src, toks, end + 1).min(
                (end..toks.len()).find(|&k| toks[k].s == "{").unwrap_or(toks.len()),
            );
        }
        let (body, next) = body(src, toks, end);
        let member = Member {
            kind: MemberKind::Constructor,
            name: class.to_owned(),
            at: toks[i].at,
            annotations: Vec::new(),
            modifiers: Vec::new(),
            ty: None,
            params,
            body,
            start: toks[i].at,
            end: toks.get(next.saturating_sub(1)).map_or(src.len(), |t| t.end()),
        };
        return (Some(member), next);
    }
    // The name: the last identifier before `(` (fun) or `:` / `=` / `by` (property),
    // skipping an extension receiver (`fun String.slug()`).
    let stop = (j..toks.len())
        .find(|&k| matches!(toks[k].s, "(" | ":" | "=" | "{" | "}" | "by") || k > j && newline_before(src, toks, k))
        .unwrap_or(toks.len());
    let Some(name) = toks[j..stop].iter().rev().find(|t| t.is_ident()).copied() else {
        return (None, expression_end(src, toks, j));
    };
    let mut k = stop;
    let mut params_out = Vec::new();
    if keyword == "fun" {
        if toks.get(k).is_none_or(|t| t.s != "(") {
            return (None, expression_end(src, toks, k));
        }
        let close = closing(toks, k);
        params_out = params(src, &toks[k + 1..close.max(k + 1)]).into_iter().map(|(p, _)| p).collect();
        k = close + 1;
    }
    let mut ty = None;
    if toks.get(k).is_some_and(|t| t.s == ":") {
        let end = type_end(src, toks, k + 1);
        ty = type_ref(src, &toks[k + 1..end.max(k + 1)]);
        k = end;
    } else if keyword == "fun" {
        ty = Some(TypeRef { name: "Unit".into(), text: "Unit".into(), at: name.at, args: Vec::new(), array: false });
    }
    let (body, next) = body(src, toks, k);
    let member = Member {
        kind: if keyword == "fun" { MemberKind::Method } else { MemberKind::Field },
        name: name.s.to_owned(),
        at: name.at,
        annotations: Vec::new(),
        modifiers: Vec::new(),
        ty,
        params: params_out,
        body,
        start: toks[i].at,
        end: toks.get(next.saturating_sub(1)).map_or(src.len(), |t| t.end()),
    };
    (Some(member), next)
}

/// A `{ … }` block, an `= expression` or `by delegate` at `k`, or nothing:
/// its byte range and the index after it.
fn body(src: &str, toks: &[Tok], k: usize) -> (Option<(usize, usize)>, usize) {
    match toks.get(k).map(|t| t.s) {
        Some("{") => {
            let close = closing(toks, k);
            (Some((toks[k].at, toks[close].end())), close + 1)
        }
        Some("=") | Some("by") => {
            let end = expression_end(src, toks, k + 1);
            let range = (end > k + 1).then(|| (toks[k + 1].at, toks[end - 1].end()));
            (range, end)
        }
        _ => (None, k),
    }
}

/// Every call of a Kotlin source; a capitalised callee without a receiver
/// counts as a constructor call (`Queue("q")`, `StepBuilder("s", repo)`).
pub fn calls(text: &str) -> Vec<Call> {
    crate::java::calls(text)
        .into_iter()
        .filter(|c| !NOT_CALLS.contains(&c.name.as_str()))
        .map(|mut c| {
            for a in &mut c.args {
                if a.string.as_ref().is_some_and(|s| is_template(&text[s.at..s.end])) {
                    a.string = None;
                }
            }
            if c.receiver.is_none() && !c.chained && c.name.starts_with(|ch: char| ch.is_uppercase()) {
                c.is_new = true;
            }
            c
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"@file:JvmName("Users")
package com.acme.users

import org.springframework.stereotype.Service
import com.acme.Clock as AppClock

@Service("users")
class UserService(
    private val repo: UserRepository,
    @Qualifier("mail") val notifiers: List<Notifier>,
    plain: String = "x",
) : BaseService<User>(repo), Auditable {
    @Autowired
    lateinit var clock: AppClock

    @Value("\${app.size:10}")
    private val size: Int = 10

    val names by lazy { repo.findAll().map { it.name } }

    @Transactional
    fun rename(id: Long, name: String): User? {
        val user = repo.findById(id)
        return user
    }

    @Bean fun mailNotifier(clock: AppClock): Notifier = MailNotifier(clock)

    fun String.slug() = lowercase()

    companion object {
        const val PREFIX = "users"
    }
}

interface UserRepository : JpaRepository<User, Long> {
    fun findByName(name: String): List<User>
}

@Configuration
open class AppConfig {
    @Bean
    open fun queue() = Queue("orders")
}

enum class Kind(val code: String) { A("a"), B("b"); fun x() = 1 }

data class User(@Id val id: Long, var name: String)

fun main(args: Array<String>) { runApplication<App>(*args) }
"#;

    #[test]
    fn parses_declarations() {
        let f = parse(SRC);
        assert_eq!(f.package, "com.acme.users");
        assert_eq!(f.imports.iter().map(|i| i.path.as_str()).collect::<Vec<_>>(), [
            "org.springframework.stereotype.Service",
            "com.acme.Clock"
        ]);
        let types: Vec<_> = f.types.iter().map(|t| (t.kind.as_str(), t.name.as_str(), t.outer)).collect();
        assert_eq!(types, [
            ("class", "UserService", None),
            ("object", "Companion", Some(0)),
            ("interface", "UserRepository", None),
            ("class", "AppConfig", None),
            ("enum", "Kind", None),
            ("class", "User", None),
        ]);
        let s = &f.types[0];
        assert_eq!(s.annotation("Service").unwrap().strings(&["value"])[0].value, "users");
        assert_eq!(s.extends[0].name, "BaseService");
        assert_eq!(s.extends[0].args[0].name, "User");
        assert_eq!(s.implements[0].name, "Auditable");
        let members: Vec<_> = s.members.iter().map(|m| (m.kind, m.name.as_str())).collect();
        assert_eq!(members, [
            (MemberKind::Field, "repo"),
            (MemberKind::Field, "notifiers"),
            (MemberKind::Constructor, "UserService"),
            (MemberKind::Field, "clock"),
            (MemberKind::Field, "size"),
            (MemberKind::Field, "names"),
            (MemberKind::Method, "rename"),
            (MemberKind::Method, "mailNotifier"),
            (MemberKind::Method, "slug"),
        ]);
        let ctor = &s.members[2];
        assert_eq!(ctor.params.iter().map(|p| (p.name.as_str(), p.ty.name.as_str())).collect::<Vec<_>>(), [
            ("repo", "UserRepository"),
            ("notifiers", "List"),
            ("plain", "String")
        ]);
        assert_eq!(ctor.params[1].ty.args[0].name, "Notifier");
        assert_eq!(s.members[1].annotations[0].name, "Qualifier");
        assert_eq!(s.members[3].annotations[0].name, "Autowired");
        assert_eq!(s.members[3].ty.as_ref().unwrap().name, "AppClock");
        assert_eq!(s.members[4].annotation("Value").unwrap().strings(&["value"])[0].value, "${app.size:10}");
        let rename = &s.members[6];
        assert_eq!(rename.ty.as_ref().unwrap().name, "User");
        assert_eq!(rename.params.len(), 2);
        assert!(rename.body.is_some());
        assert_eq!(&SRC[rename.at..rename.at + 6], "rename");
        assert_eq!(s.members[7].ty.as_ref().unwrap().name, "Notifier");
        assert!(s.members[7].has(&["Bean"]));
        assert_eq!(f.types[2].extends[0].name, "JpaRepository");
        assert_eq!(f.types[2].members[0].name, "findByName");
        assert!(f.types[2].members[0].body.is_none());
        assert_eq!(f.types[3].members[0].name, "queue");
        assert_eq!(f.types[4].members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["code", "Kind", "x"]);
        assert_eq!(f.types[5].fields().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["id", "name"]);
        assert_eq!(f.qualified(1), "com.acme.users.UserService.Companion");
    }

    #[test]
    fn finds_calls_and_beans() {
        let calls = calls(SRC);
        let queue = calls.iter().find(|c| c.name == "Queue").unwrap();
        assert!(queue.is_new);
        assert_eq!(queue.args[0].string.as_ref().unwrap().value, "orders");
        assert!(calls.iter().any(|c| c.name == "findById" && c.receiver.as_deref() == Some("repo")));
        assert!(!calls.iter().any(|c| c.name == "rename" || c.name == "when"));
        let beans: Vec<_> = crate::java::bean_names(&parse(SRC)).into_iter().map(|b| b.name).collect();
        assert_eq!(beans, ["users", "mailNotifier", "appConfig", "queue"]);
    }

    #[test]
    fn survives_every_prefix() {
        for end in (0..=SRC.len()).filter(|&e| SRC.is_char_boundary(e)) {
            parse(&SRC[..end]);
            calls(&SRC[..end]);
        }
    }

    #[test]
    fn kotlin_template_literals_are_not_plain_literals() {
        let src = r#"@KafkaListener(topics = ["$p-a", "b-$env", "${x}", "plain", "\$esc", "5$", "$ 1", "$5"])
class A { fun f() { t.send("$p-orders", 1); t.send("orders-$env", 1); t.send("a${b}", 1); t.send("ok\$x", 1); t.send("c$", 1) } }"#;
        let f = parse(src);
        let got: Vec<_> = f.types[0].annotation("KafkaListener").unwrap().strings(&["topics"]).iter().map(|s| s.value.clone()).collect();
        assert_eq!(got, ["plain", "$esc", "5$", "$ 1", "$5"]);
        let sent: Vec<_> = calls(src).iter().filter(|c| c.name == "send").map(|c| c.args[0].string.as_ref().map(|s| s.value.clone())).collect();
        assert_eq!(sent, [None, None, None, Some("ok$x".to_owned()), Some("c$".to_owned())]);
    }
}
