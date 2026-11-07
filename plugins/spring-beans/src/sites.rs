//! Strings that name beans or hold expressions: qualifiers and other bean
//! names, SpEL, AspectJ pointcuts, and the references of Spring XML files.

use spring_text::{java, xml};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Site {
    /// A bean name (`@Qualifier("…")`, `ref="…"`); XML lists are comma separated.
    BeanRef,
    /// A SpEL expression with its `#variables`.
    Spel(Vec<String>),
    Pointcut,
    /// A fully qualified class name (XML `class="…"`).
    Class,
    /// A bean scope (`@Scope`, XML `scope`).
    Scope,
    /// A profile name or expression (`@Profile`, `@ActiveProfiles`, `<beans profile>`).
    Profile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub site: Site,
    /// The bytes between the quotes.
    pub from: usize,
    pub to: usize,
}

const ADVICE: [&str; 6] = ["Pointcut", "Before", "After", "AfterReturning", "AfterThrowing", "Around"];
const CACHE: [&str; 3] = ["Cacheable", "CachePut", "CacheEvict"];
const SECURITY: [&str; 4] = ["PreAuthorize", "PostAuthorize", "PreFilter", "PostFilter"];
const BEAN_NAMES: [&str; 4] = ["Qualifier", "Named", "DependsOn", "Lookup"];

fn site(annotation: &str, arg: &str, value: &str, params: &[String]) -> Option<Site> {
    let with = |extra: &[&str]| Site::Spel(params.iter().cloned().chain(extra.iter().map(|s| s.to_string())).collect());
    match (annotation, arg) {
        (a, "value") if BEAN_NAMES.contains(&a) => Some(Site::BeanRef),
        ("Scope", "value" | "scopeName") => Some(Site::Scope),
        ("Profile", "value") | ("ActiveProfiles", "value" | "profiles") => Some(Site::Profile),
        ("Resource", "name") => Some(Site::BeanRef),
        (a, "value" | "pointcut") if ADVICE.contains(&a) => Some(Site::Pointcut),
        (a, "key" | "condition" | "unless") if CACHE.contains(&a) => Some(with(&["root", "result"])),
        ("EventListener" | "TransactionalEventListener", "condition") => Some(with(&["root"])),
        (a, "value") if SECURITY.contains(&a) => Some(with(&["root", "returnObject", "filterObject"])),
        ("ConditionalOnExpression", "value") => Some(Site::Spel(Vec::new())),
        _ if value.contains("#{") => Some(Site::Spel(Vec::new())),
        _ => None,
    }
}

fn annotations(out: &mut Vec<Hit>, anns: &[java::Annotation], params: &[String]) {
    for a in anns {
        for arg in &a.args {
            for s in &arg.strings {
                if let Some(site) = site(&a.name, &arg.name, &s.value, params) {
                    out.push(Hit { site, from: s.at, to: s.end });
                }
            }
        }
    }
}

pub fn java(text: &str) -> Vec<Hit> {
    let file = java::parse(text);
    let mut out = Vec::new();
    for t in &file.types {
        annotations(&mut out, &t.annotations, &[]);
        for p in &t.components {
            annotations(&mut out, &p.annotations, &[]);
        }
        for m in &t.members {
            let params: Vec<String> = m.params.iter().map(|p| p.name.clone()).collect();
            annotations(&mut out, &m.annotations, &params);
            for p in &m.params {
                annotations(&mut out, &p.annotations, &[]);
            }
        }
    }
    for call in java::calls(text).into_iter().filter(|c| c.name == "getBean" || c.name == "containsBean") {
        if let Some(s) = call.args.first().and_then(|a| a.string.clone()) {
            out.push(Hit { site: Site::BeanRef, from: s.at, to: s.end });
        }
    }
    out
}

pub fn xml(text: &str) -> Vec<Hit> {
    let els = xml::elements(text);
    if xml::root(&els) != Some("beans") {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, e) in els.iter().enumerate() {
        for a in &e.attrs {
            let site = if crate::model::is_xml_ref(&els, i, a) {
                Site::BeanRef
            } else if a.local() == "class" {
                Site::Class
            } else if a.name == "scope" && e.local() == "bean" {
                Site::Scope
            } else if a.name == "profile" && e.local() == "beans" {
                Site::Profile
            } else if a.value.contains("#{") {
                Site::Spel(Vec::new())
            } else {
                continue;
            };
            out.push(Hit { site, from: a.value_at, to: a.value_end });
        }
    }
    out
}

pub fn at(hits: Vec<Hit>, cursor: usize) -> Option<Hit> {
    hits.into_iter().find(|h| h.from <= cursor && cursor <= h.to)
}

fn word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80
}

/// The identifier around `at` in `text`, as `(start, end)` bytes.
pub fn word(text: &str, from: usize, at: usize, to: usize) -> (usize, usize) {
    let b = text.as_bytes();
    let mut s = at;
    while s > from && word_byte(b[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < to && word_byte(b[e]) {
        e += 1;
    }
    (s, e)
}

/// The profile name around `at`: expression operators and separators end it.
pub fn word_in(text: &str, from: usize, at: usize, to: usize) -> (usize, usize) {
    let sep = |c: u8| matches!(c, b',' | b';' | b' ' | b'&' | b'|' | b'!' | b'(' | b')');
    let b = text.as_bytes();
    let mut s = at;
    while s > from && !sep(b[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < to && !sep(b[e]) {
        e += 1;
    }
    (s, e)
}

/// The comma-separated item of a bean-name list around `at`.
pub fn list_item(text: &str, from: usize, at: usize, to: usize) -> (usize, usize) {
    let sep = |c: u8| matches!(c, b',' | b';' | b' ');
    let b = text.as_bytes();
    let mut s = at;
    while s > from && !sep(b[s - 1]) {
        s -= 1;
    }
    let mut e = at;
    while e < to && !sep(b[e]) {
        e += 1;
    }
    (s, e)
}

/// What the identifier at `start..end` of a site is.
#[derive(Debug, PartialEq, Eq)]
pub enum Word<'a> {
    /// `@name` in SpEL or `bean(name)` in a pointcut.
    Bean,
    /// `#name` in SpEL.
    Variable,
    /// `name()` or `Aspect.name()` in a pointcut: (qualifier, name).
    Pointcut(Option<&'a str>),
    Designator,
    Other,
}

pub fn classify<'a>(text: &'a str, hit: &Hit, start: usize, end: usize) -> Word<'a> {
    let before = &text[hit.from..start];
    let prev = before.as_bytes().last().copied();
    match &hit.site {
        Site::Spel(_) => match prev {
            Some(b'@') => Word::Bean,
            Some(b'#') => Word::Variable,
            _ => Word::Other,
        },
        Site::Pointcut => {
            let mut open = Vec::new();
            for (k, c) in before.char_indices() {
                match c {
                    '(' => open.push(k),
                    ')' => {
                        open.pop();
                    }
                    _ => {}
                }
            }
            if let Some(&o) = open.last() {
                let head = before[..o].trim_end();
                if head.ends_with("bean") {
                    return Word::Bean;
                }
                if head.as_bytes().last().is_some_and(|&c| word_byte(c)) {
                    return Word::Other; // a designator's arguments
                }
            }
            let call = text[end..hit.to].trim_start().starts_with('(');
            if prev == Some(b'.') {
                if !call {
                    return Word::Other;
                }
                let (q_start, _) = word(text, hit.from, start - 1, start - 1);
                return Word::Pointcut(Some(&text[q_start..start - 1]));
            }
            if call && !DESIGNATOR_NAMES.contains(&&text[start..end]) {
                return Word::Pointcut(None);
            }
            Word::Designator
        }
        _ => Word::Other,
    }
}

pub const DESIGNATOR_NAMES: [&str; 9] =
    ["execution", "within", "this", "target", "args", "bean", "annotation", "withincode", "cflow"];

/// Pointcut designators: (label, snippet). `@`-forms are offered after an `@`.
pub const DESIGNATORS: [(&str, &str); 7] = [
    ("execution", "execution(${1:* com.example..*.*(..)})"),
    ("within", "within(${1:com.example..*})"),
    ("this", "this(${1:Type})"),
    ("target", "target(${1:Type})"),
    ("args", "args(${1:..})"),
    ("bean", "bean(${1:name})"),
    ("annotation", "annotation(${1:Annotation})"),
];
