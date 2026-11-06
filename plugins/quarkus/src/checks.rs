//! Checks of Quarkus configuration files that the key catalog alone cannot do:
//! duplicate keys, lines without a separator, empty and ill-typed values and
//! `${...}` expressions that name nothing. Written from the Quarkus configuration
//! reference and the MicroProfile Config specification.

use spring_text::finding::Finding;
use spring_text::config::{self, Key};
use spring_text::metadata::{Catalog, Lookup, Prop, strip_profile};
use std::collections::BTreeSet;

/// The check groups that are on.
#[derive(Clone, Copy)]
pub struct Groups {
    pub syntax: bool,
    pub values: bool,
    pub expressions: bool,
}

impl Default for Groups {
    fn default() -> Self {
        Self { syntax: true, values: true, expressions: true }
    }
}

const SYSTEM_PREFIXES: [&str; 9] = ["java.", "user.", "os.", "file.", "path.", "line.", "sun.", "jdk.", "quarkus."];

/// The problems of a `.properties` file.
pub fn check_properties(text: &str, meta: &Catalog) -> Vec<Finding> {
    check(text, false, &Lookup(vec![meta]), Groups::default())
}

/// The problems of a YAML application file, over its keys flattened to dotted names.
pub fn check_yaml(text: &str, meta: &Catalog) -> Vec<Finding> {
    check(text, true, &Lookup(vec![meta]), Groups::default())
}

pub fn check(text: &str, yaml: bool, lookup: &Lookup, groups: Groups) -> Vec<Finding> {
    let keys = if yaml { config::yaml(text) } else { config::properties(text) };
    let mut out = Vec::new();
    if groups.syntax {
        duplicates(&keys, &mut out);
        if !yaml {
            missing_separators(text, &keys, &mut out);
        }
    }
    let defined: BTreeSet<&str> = keys.iter().map(|k| strip_profile(&k.key)).collect();
    for (i, k) in keys.iter().enumerate() {
        if continued(text, k) || is_parent(&keys, i) && yaml {
            continue;
        }
        let base = strip_profile(&k.key);
        let prop = known(lookup, base);
        if groups.syntax && k.value.is_empty() && has_separator(text, k, yaml) && needs_value(prop) {
            out.push(Finding::new(k.at, k.end, "unassigned-value", format!("`{base}` has no value.")));
        }
        if k.value.is_empty() {
            continue;
        }
        if groups.expressions {
            expressions(k, &defined, lookup, &mut out);
        }
        if groups.values && !k.value.contains("${") {
            if let Some(message) = invalid(lookup, base, prop, &k.value) {
                out.push(Finding::new(k.value_at, k.value_end, "invalid-value", message));
            }
        }
    }
    out
}

fn known<'l>(lookup: &'l Lookup, key: &str) -> Option<&'l Prop> {
    let is_env = key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if is_env || key.contains('"') || key.contains('[') {
        return None;
    }
    lookup.prop(key)
}

fn is_parent(keys: &[Key], i: usize) -> bool {
    let key = &keys[i].key;
    keys.iter().any(|o| o.key.len() > key.len() && o.key.starts_with(key.as_str()) && matches!(o.key.as_bytes()[key.len()], b'.' | b'['))
}

/// A value running on to the next line, or a YAML block scalar, is not read as a whole.
fn continued(text: &str, k: &Key) -> bool {
    let after = &text[k.value_end..];
    let rest = after.split('\n').next().unwrap_or("");
    let backslashes = text[..k.value_end].chars().rev().take_while(|&c| c == '\\').count();
    (backslashes % 2 == 1 && rest.trim().is_empty()) || k.value.starts_with(['|', '>'])
}

fn has_separator(text: &str, k: &Key, yaml: bool) -> bool {
    yaml || text[k.end..].trim_start_matches([' ', '\t']).starts_with(['=', ':'])
}

/// An empty value is fine for a free-text property that has no default: it is how it is left unset.
fn needs_value(prop: Option<&Prop>) -> bool {
    let Some(prop) = prop else { return false };
    if prop.open() {
        return false;
    }
    !prop.default.is_empty() || !matches!(prop.ty.as_str(), "string" | "String" | "path" | "")
}

fn duplicates(keys: &[Key], out: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    for k in keys {
        if k.key.is_empty() || seen.insert(k.key.as_str()) {
            continue;
        }
        out.push(Finding::new(k.at, k.end, "duplicate-key", format!("`{}` is already set in this file.", k.key)));
    }
}

fn missing_separators(text: &str, keys: &[Key], out: &mut Vec<Finding>) {
    for k in keys {
        let rest = text[k.end..].split('\n').next().unwrap_or("").trim();
        if rest.is_empty() && !has_separator(text, k, false) {
            out.push(Finding::new(k.at, k.end, "missing-separator", format!("`{}` has no `=` or `:` and no value.", k.key)));
        }
    }
}

fn expressions(k: &Key, defined: &BTreeSet<&str>, lookup: &Lookup, out: &mut Vec<Finding>) {
    let value = k.value.as_str();
    let mut from = 0;
    while let Some(open) = value[from..].find("${").map(|i| i + from) {
        from = open + 2;
        if value[..open].ends_with('\\') {
            continue;
        }
        let Some(close) = closing(value, open + 2) else { return };
        let inner = &value[open + 2..close];
        from = close + 1;
        if inner.contains("${") || inner.contains(':') || inner.is_empty() {
            continue;
        }
        let name = inner.trim();
        let env = name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        let system = SYSTEM_PREFIXES.iter().any(|p| name.starts_with(p));
        if env || system || defined.contains(name) || lookup.prop(name).is_some() {
            continue;
        }
        out.push(Finding::new(k.value_at + open, k.value_at + close + 1, "undefined-expression", format!("`{name}` is not defined in this file, as a property or as an environment variable.")));
    }
}

/// The `}` that closes the expression whose body starts at `from`.
fn closing(value: &str, from: usize) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in value[from..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(from + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn invalid(lookup: &Lookup, key: &str, prop: Option<&Prop>, value: &str) -> Option<String> {
    let prop = prop?;
    let value = value.trim();
    if value.contains('\\') || prop.open() {
        return None;
    }
    let ok = match prop.ty.as_str() {
        "boolean" | "Boolean" => matches!(value.to_ascii_lowercase().as_str(), "true" | "false"),
        "int" | "Integer" | "long" | "Long" | "short" | "Short" => value.parse::<i64>().is_ok(),
        "double" | "Double" | "float" | "Float" => value.parse::<f64>().is_ok(),
        "Duration" => duration(value),
        "MemorySize" => memory_size(value),
        "string" | "String" | "path" | "Path" | "" => true,
        ty if ty.contains('<') => true,
        _ => {
            let values = lookup.values(key);
            values.is_empty() || values.iter().any(|v| same_word(v, value))
        }
    };
    if ok {
        return None;
    }
    Some(format!("`{value}` is not a valid `{}` for `{key}`.", prop.ty))
}

fn same_word(a: &str, b: &str) -> bool {
    let clean = |s: &str| s.chars().filter(|c| !matches!(c, '-' | '_')).flat_map(|c| c.to_lowercase()).collect::<String>();
    clean(a) == clean(b)
}

/// Seconds as a number, `30s`, `1h 30m`, or an ISO-8601 `PT1M`.
fn duration(value: &str) -> bool {
    if value.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if value.starts_with(['P', 'p']) {
        return value.len() > 2 && value[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    }
    value.split_whitespace().all(|part| {
        let digits = part.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let unit = &part[digits.len()..];
        !digits.is_empty() && digits.parse::<f64>().is_ok() && matches!(unit.to_ascii_lowercase().as_str(), "ms" | "s" | "m" | "h" | "d")
    })
}

/// `512`, `10M`, `1G`, `10240K`, `1GiB`.
fn memory_size(value: &str) -> bool {
    let digits = value.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = value[digits.len()..].to_ascii_lowercase();
    !digits.is_empty()
        && digits.parse::<f64>().is_ok()
        && matches!(unit.as_str(), "" | "b" | "k" | "kb" | "kib" | "m" | "mb" | "mib" | "g" | "gb" | "gib" | "t" | "tb" | "tib" | "p" | "pb" | "pib" | "e" | "eb" | "eib" | "z" | "zb" | "zib" | "y" | "yb" | "yib")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundled;
    use spring_text::metadata::{Meta, parse_tsv};

    fn run(text: &str) -> Vec<(&'static str, String)> {
        check_properties(text, bundled()).iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    fn run_yaml(text: &str) -> Vec<(&'static str, String)> {
        check_yaml(text, bundled()).iter().map(|f| (f.code, text[f.start..f.end].to_owned())).collect()
    }

    fn codes(found: &[(&'static str, String)]) -> Vec<&'static str> {
        found.iter().map(|(c, _)| *c).collect()
    }

    #[test]
    fn duplicate_key_in_same_profile_warns() {
        let found = run("quarkus.http.port=1\napp.x=1\nquarkus.http.port=2\n%dev.app.y=1\n%dev.app.y=2\n");
        assert_eq!(found, [("duplicate-key", "quarkus.http.port".into()), ("duplicate-key", "%dev.app.y".into())]);
    }

    #[test]
    fn same_key_in_different_profiles_is_quiet() {
        assert!(run("quarkus.http.port=1\n%dev.quarkus.http.port=2\n%test.quarkus.http.port=3\n%dev,test.quarkus.http.port=4\n").is_empty());
    }

    #[test]
    fn line_without_equals_warns() {
        let found = run("# note\n! note\n\nquarkus.http.port\napp.x 1\nok=1\n");
        assert_eq!(found, [("missing-separator", "quarkus.http.port".into())]);
    }

    #[test]
    fn empty_value_warns_unless_optional_in_metadata() {
        let found = run("quarkus.http.port=\nquarkus.http.root-path=\nquarkus.datasource.password=\napp.free=\nquarkus.log.level:\n");
        assert_eq!(codes(&found), ["unassigned-value", "unassigned-value", "unassigned-value"]);
        assert_eq!(found[0].1, "quarkus.http.port");
        assert_eq!(found[2].1, "quarkus.log.level");
    }

    #[test]
    fn enum_value_not_in_metadata_warns() {
        let found = run("quarkus.log.level=LOUD\nquarkus.log.level=DEBUG\nquarkus.http.port=abc\nquarkus.http.cors.enabled=yes\nquarkus.http.ssl.client-auth=sometimes\nquarkus.http.ssl.client-auth=Required\n");
        let invalid: Vec<_> = found.iter().filter(|(c, _)| *c == "invalid-value").map(|(_, t)| t.as_str()).collect();
        assert_eq!(invalid, ["LOUD", "abc", "yes", "sometimes"]);
    }

    #[test]
    fn duration_and_memory_size_values_are_accepted() {
        let text = "quarkus.http.read-timeout=30s\nquarkus.http.idle-timeout=PT1M\nquarkus.http.so-linger=1h\nquarkus.http.limits.max-body-size=10M\nquarkus.http.limits.max-form-attribute-size=512K\nquarkus.http.limits.max-chunk-size=1G\n";
        assert!(run(text).is_empty(), "{:?}", run(text));
        assert_eq!(codes(&run("quarkus.http.read-timeout=soon\nquarkus.http.limits.max-body-size=big\n")), ["invalid-value", "invalid-value"]);
    }

    #[test]
    fn unknown_extension_value_is_quiet() {
        assert!(run("quarkus.acme.mode=whatever\nquarkus.acme.size=abc\nQUARKUS_HTTP_PORT=abc\nquarkus.log.category.\"io.acme\".level=DEBUG\n").is_empty());
    }

    #[test]
    fn unresolved_expression_warns_unless_env_or_system_property() {
        let text = "app.a=${nowhere}\napp.b=${HOME}\napp.c=${user.home}/x\napp.d=${missing:fallback}\napp.e=${app.b}\napp.f=${quarkus.http.port}\napp.g=${java.io.tmpdir}\napp.h=${other.missing}\n";
        assert_eq!(run(text), [("undefined-expression", "${nowhere}".into()), ("undefined-expression", "${other.missing}".into())]);
    }

    #[test]
    fn yaml_files_are_checked_the_same_way() {
        let text = "quarkus:\n  http:\n    port: abc\n    cors:\n      enabled: true\n  log:\n    level: DEBUG\napp:\n  a: ${nowhere}\n  b:\n";
        assert_eq!(run_yaml(text), [("invalid-value", "abc".into()), ("undefined-expression", "${nowhere}".into())]);
        assert_eq!(codes(&run_yaml("a:\n  b: 1\n  b: 2\n")), ["duplicate-key"]);
    }

    #[test]
    fn ranges_survive_comments_continuations_escapes_and_multibyte_text() {
        let text = "# é😀\r\nlong=a\\\r\n  b\r\nke\\:y=${gone}\r\nquarkus.http.port=zz\r\n";
        let found = check_properties(text, bundled());
        let d: Vec<_> = found.iter().map(|f| f.diagnostic(text)).collect();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].range.start.line, d[0].range.start.character), (3, 6));
        assert_eq!((d[1].range.start.line, d[1].range.start.character, d[1].range.end.character), (4, 18, 20));
        let emoji = "app.é😀=${gone}\n";
        let d = check_properties(emoji, bundled())[0].diagnostic(emoji);
        assert_eq!(d.range.start.character, 8);
    }

    #[test]
    fn every_check_is_quiet_for_library_or_dynamic_names() {
        let text = "%prod.QUARKUS_HTTP_PORT=${PORT}\napp.url=${SCHEME}://x\nquarkus.acme.\"a\".size=\nunknown.thing=${HOST:localhost}\n";
        assert!(run(text).is_empty(), "{:?}", run(text));
        let mut meta = Meta::default();
        meta.props = parse_tsv("app.a\tint\t\t\tx\n").props;
        let mut c = Catalog::default();
        c.add(&meta);
        assert_eq!(check_properties("app.a=1\napp.a=2\n", &c).len(), 1);
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        assert!(!run("quarkus.http.port=abc\n").is_empty());
        assert!(run("quarkus.http.port=8080\n").is_empty());
    }
}
