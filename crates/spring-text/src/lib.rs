//! Text-level helpers shared by the Spring plugins: Java and Kotlin declaration
//! scanners, an XML element scanner, `.properties` / YAML key paths and a
//! project index read a little at a time. No PSI: everything works on the
//! source as written, types by simple name.

pub mod beans;
pub mod config;
pub mod diagram;
pub mod index;
pub mod java;
pub mod kotlin;
pub mod metadata;
pub mod tree;
pub mod xml;

use openidea_plugin::text::position;
use openidea_plugin::{Position, Range};

/// Globs of the JVM sources the scanners read.
pub const SOURCE_GLOBS: &[&str] = &["**/*.java", "**/*.kt"];

/// Whether `path` is a Kotlin source.
pub fn is_kotlin(path: &str) -> bool {
    path.ends_with(".kt") || path.ends_with(".kts")
}

/// The declarations of a Java or Kotlin source, by its extension.
pub fn parse_source(path: &str, text: &str) -> java::JavaFile {
    if is_kotlin(path) { kotlin::parse(text) } else { java::parse(text) }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decapitalizes_like_spring() {
        assert_eq!(decapitalize("UserService"), "userService");
        assert_eq!(decapitalize("URLMapper"), "URLMapper");
        assert_eq!(decapitalize("A"), "a");
        assert_eq!(decapitalize(""), "");
    }
}
