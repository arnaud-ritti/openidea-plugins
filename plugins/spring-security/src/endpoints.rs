//! The request mappings of the project's controllers and the Ant patterns of
//! request matchers that cover them.

use openidea_plugin::Range;
use spring_text::java;
use spring_text::{parse_source, range};

const MAPPING_ANNOTATIONS: [&str; 6] = ["RequestMapping", "GetMapping", "PostMapping", "PutMapping", "DeleteMapping", "PatchMapping"];
const VERBS: [&str; 7] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// Empty: any HTTP method.
    pub verb: String,
    pub path: String,
    pub range: Range,
}

pub fn mappings(path: &str, text: &str) -> Option<Vec<Mapping>> {
    if !text.contains("Controller") {
        return None;
    }
    let mut out = Vec::new();
    for class in parse_source(path, text).types.iter().filter(|t| t.has(&["Controller", "RestController"])) {
        for m in class.methods().filter(|m| m.has(&MAPPING_ANNOTATIONS)) {
            let at = range(text, m.at, m.at + m.name.len());
            out.extend(java::mappings(class, m).into_iter().map(|(verb, path)| Mapping { verb, path, range: at }));
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The HTTP method of a matcher argument such as `HttpMethod.GET`.
pub fn verb_of(argument: &str) -> Option<&str> {
    let name = argument.rsplit('.').next()?.trim();
    VERBS.contains(&name).then_some(name)
}

/// Whether an Ant pattern covers a mapping's URI template; a `{variable}` of the mapping stands for any segment.
pub fn covers(pattern: &str, mapping: &str) -> bool {
    let segments = |p: &str| p.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    go(&segments(pattern), &segments(mapping))
}

fn go(pattern: &[String], mapping: &[String]) -> bool {
    let Some((head, rest)) = pattern.split_first() else { return mapping.is_empty() };
    if head == "**" {
        return (0..=mapping.len()).any(|i| go(rest, &mapping[i..]));
    }
    let Some((segment, tail)) = mapping.split_first() else { return false };
    if segment.starts_with("{*") {
        return true;
    }
    segment_matches(head, segment) && go(rest, tail)
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    if segment.contains('{') || pattern == "*" || (pattern.starts_with('{') && pattern.ends_with('}')) {
        return true;
    }
    match pattern.split_once('*') {
        Some((prefix, suffix)) => segment.len() >= prefix.len() + suffix.len() && segment.starts_with(prefix) && segment.ends_with(suffix),
        None => pattern == segment,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ant_patterns_cover_uri_templates() {
        assert!(covers("/api/**", "/api/users/{id}"));
        assert!(covers("/api/**", "/api"));
        assert!(covers("/api/*/x", "/api/{v}/x"));
        assert!(covers("/users/{id}", "/users/{userId}"));
        assert!(covers("/files/*.css", "/files/app.css"));
        assert!(covers("/**", "/anything/{a}"));
        assert!(!covers("/api/*", "/api/a/b"));
        assert!(!covers("/admin/**", "/api/users"));
        assert!(!covers("/users", "/users/{id}"));
    }

    #[test]
    fn reads_controller_mappings_and_matcher_verbs() {
        let src = "@RestController @RequestMapping(\"/u\") class U { @GetMapping(\"/{id}\") void a() {} void plain() {} }";
        let found = mappings("U.java", src).unwrap();
        assert_eq!((found[0].verb.as_str(), found[0].path.as_str()), ("GET", "/u/{id}"));
        assert_eq!(found.len(), 1);
        assert_eq!(verb_of("HttpMethod.GET"), Some("GET"));
        assert_eq!(verb_of("\"/x\""), None);
    }
}
