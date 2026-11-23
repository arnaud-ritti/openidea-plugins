//! Text-level helpers shared by the Spring plugins: Java and Kotlin declaration
//! scanners, an XML element scanner, `.properties` / YAML key paths and a
//! project index read a little at a time. No PSI: everything works on the
//! source as written, types by simple name.

pub mod beans;
pub mod build;
pub mod config;
pub mod constants;
pub mod diagram;
pub mod finding;
pub mod index;
pub mod java;
pub mod kotlin;
pub mod metadata;
pub mod provenance;
pub mod scope;
pub mod search;
pub mod sibling;
pub mod text;
pub mod tree;
pub mod xml;
pub mod xml_check;

use openidea_plugin::text::position;
use openidea_plugin::{Error, Position, Range};

/// Globs of the JVM sources the scanners read.
pub const SOURCE_GLOBS: &[&str] = &["**/*.java", "**/*.kt"];

/// Whether `path` is a Kotlin source.
pub fn is_kotlin(path: &str) -> bool {
    path.ends_with(".kt") || path.ends_with(".kts")
}

thread_local! {
    static LAST_PARSE: std::cell::RefCell<Option<(bool, String, java::JavaFile)>> = const { std::cell::RefCell::new(None) };
}

/// The declarations of a Java or Kotlin source, by its extension. The last
/// source parsed is remembered, so several readers of one file parse it once.
pub fn parse_source(path: &str, text: &str) -> java::JavaFile {
    let kotlin = is_kotlin(path);
    let remembered = LAST_PARSE.with_borrow(|last| {
        last.as_ref().filter(|(was_kotlin, source, _)| *was_kotlin == kotlin && source == text).map(|(_, _, file)| file.clone())
    });
    if let Some(file) = remembered {
        return file;
    }
    let file = if kotlin { kotlin::parse(text) } else { java::parse(text) };
    if text.len() <= 512 * 1024 {
        LAST_PARSE.with_borrow_mut(|last| *last = Some((kotlin, text.to_owned(), file.clone())));
    }
    file
}

/// The calls of a Java or Kotlin source, by its extension.
pub fn source_calls(path: &str, text: &str) -> Vec<java::Call> {
    if is_kotlin(path) { kotlin::calls(text) } else { java::calls(text) }
}

/// The editor range of the bytes `from..to` of `text`.
pub fn range(text: &str, from: usize, to: usize) -> Range {
    Range::new(position(text, from), position(text, to))
}

/// Whether `p` is inside `r`, ends included (a caret right after a word is on it).
pub fn contains(r: Range, p: Position) -> bool {
    r.start <= p && p <= r.end
}

/// Spring's default bean name for a class: `UserService` → `userService`,
/// `URLMapper` → `URLMapper` (two leading capitals are kept).
pub fn decapitalize(name: &str) -> String {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) if a.is_uppercase() && b.is_uppercase() => name.to_owned(),
        (Some(a), _) => a.to_lowercase().chain(name[a.len_utf8()..].chars()).collect(),
        _ => String::new(),
    }
}

/// Whether `err` means the path does not exist. Project reads say `NotFound`;
/// `host::fs` outside the project says `Invalid("…: no such file")`.
pub fn fs_missing(err: &Error) -> bool {
    match err {
        Error::NotFound(_) => true,
        Error::Invalid(message) => message.ends_with("no such file") || message.ends_with("no such directory"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_the_same_source_again_gives_the_same_file() {
        let source = "package a; class A {}";
        assert_eq!(parse_source("A.java", source), parse_source("A.java", source));
        assert_eq!(parse_source("A.kt", source), kotlin::parse(source), "the language is part of what is remembered");
    }

    #[test]
    fn decapitalizes_like_spring() {
        assert_eq!(decapitalize("UserService"), "userService");
        assert_eq!(decapitalize("URLMapper"), "URLMapper");
        assert_eq!(decapitalize("A"), "a");
        assert_eq!(decapitalize(""), "");
    }

    #[test]
    fn fs_missing_invalid_no_such_file() {
        assert!(fs_missing(&Error::Invalid("x: no such file".into())));
        assert!(fs_missing(&Error::Invalid("x: no such directory".into())));
        assert!(fs_missing(&Error::NotFound("x".into())));
        assert!(!fs_missing(&Error::Invalid("bad".into())));
        assert!(!fs_missing(&Error::Denied("x: no such file".into())));
    }
}
