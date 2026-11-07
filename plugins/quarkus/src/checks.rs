//! Checks of Quarkus configuration files that the key catalog alone cannot do:
//! duplicate keys, lines without a separator, empty and ill-typed values and
//! `${...}` expressions that name nothing. Written from the Quarkus configuration
//! reference and the MicroProfile Config specification.

use openidea_plugin::Severity;
use spring_text::finding::Finding;
use spring_text::config::{self, Key};
use spring_text::metadata::{Catalog, Lookup, Prop, strip_profile};
use std::borrow::Cow;
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
        duplicates(text, yaml, &keys, &mut out);
        if !yaml {
            missing_separators(text, &keys, &mut out);
        }
    }
    let defined: BTreeSet<&str> = keys.iter().map(|k| strip_profile(&k.key)).collect();
    for (i, k) in keys.iter().enumerate() {
        if continued(text, k) || is_parent(&keys, i) && yaml {
            continue;
        }
        let written = strip_profile(&k.key);
        let base = &*normalize(lookup, written);
        let prop = known(lookup, base);
        if groups.syntax && k.value.is_empty() && has_separator(text, k, yaml) {
            if let Some(prop) = prop.filter(|p| !p.open() && !p.default.is_empty()) {
                let message = format!("`{written}` is empty, so Quarkus treats it as unset and uses the default `{}`.", prop.default);
                out.push(Finding::new(k.at, k.end, "unassigned-value", message).with_severity(Severity::Hint));
            }
        }
        if k.value.is_empty() {
            continue;
        }
        if groups.expressions {
            expressions(k, &defined, lookup, &mut out);
        }
        if groups.values && !k.value.contains("${") {
            if let Some(message) = invalid(lookup, written, base, prop, &k.value) {
                out.push(Finding::new(k.value_at, k.value_end, "invalid-value", message));
            }
        }
    }
    out
}

/// `quarkus.datasource."users".jdbc.url` is the property `quarkus.datasource.jdbc.url` of a named
/// instance. Only the roots the metadata lists as named are rewritten (`quarkus.log.category."a.b".level` is not).
pub fn normalize<'k>(lookup: &Lookup, key: &'k str) -> Cow<'k, str> {
    let Some(open) = key.find(".\"") else { return Cow::Borrowed(key) };
    let Some(close) = key[open + 2..].find('"').map(|i| open + 2 + i) else { return Cow::Borrowed(key) };
    let rest = &key[close + 1..];
    if !rest.starts_with('.') || !lookup.values("!named").iter().any(|root| root == &key[..open]) {
        return Cow::Borrowed(key);
    }
    Cow::Owned(format!("{}{rest}", &key[..open]))
}

/// The key without case, dots, dashes and underscores, so `quarkus.http.port` and `QUARKUS_HTTP_PORT` agree.
pub fn squash(key: &str) -> String {
    key.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

fn known<'l>(lookup: &'l Lookup, key: &str) -> Option<&'l Prop> {
    let is_env = key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if is_env || key.contains('"') || key.contains('[') {
        return None;
    }
    lookup.prop(key)
}

/// A key the metadata lists as required (no default, not Optional) that the file leaves out although
/// a sibling of it is set. `elsewhere` holds the squashed keys of every other config file and `.env`.
pub fn missing_required(keys: &[Key], elsewhere: &BTreeSet<String>, lookup: &Lookup) -> Vec<Finding> {
    let written: Vec<(&Key, String)> = keys.iter().map(|k| (k, normalize(lookup, strip_profile(&k.key)).into_owned())).collect();
    let mut out = Vec::new();
    for required in lookup.values("!required") {
        let Some((group, _)) = required.rsplit_once('.') else { continue };
        let wanted = squash(&required);
        if elsewhere.contains(&wanted) || written.iter().any(|(_, key)| squash(key) == wanted) {
            continue;
        }
        let sibling = |key: &str| key.rsplit_once('.').is_some_and(|(parent, _)| squash(parent) == squash(group));
        let Some((anchor, _)) = written.iter().find(|(_, key)| sibling(key)) else { continue };
        let message = format!("`{required}` is required when `{}` is set. Quarkus may still read it from the environment or another source.", anchor.key);
        out.push(Finding::new(anchor.at, anchor.end, "missing-required", message).with_severity(Severity::Info));
    }
    out
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

fn duplicates(text: &str, yaml: bool, keys: &[Key], out: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    for k in keys {
        let document = if yaml { text[..k.at].lines().filter(|l| l.starts_with("---")).count() } else { 0 };
        if k.key.is_empty() || seen.insert((document, k.key.as_str())) {
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

/// Only a typo candidate is reported: a name that is not defined but is one or two edits away from a
/// key defined in the same file. Anything else may come from another config file, `.env`,
/// an environment variable or `-D`.
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
        if env || system || defined.contains(name) || lookup.prop(name).is_some() || name.len() < 6 {
            continue;
        }
        let Some(near) = defined.iter().find(|d| edit_distance(d, name) <= 2) else { continue };
        out.push(Finding::new(k.value_at + open, k.value_at + close + 1, "undefined-expression", format!("`{name}` is not defined in this file. Did you mean `{near}`?")));
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 2 {
        return 3;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb)).min(above + 1).min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
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

const JUL_LEVELS: [&str; 14] = ["ALL", "OFF", "WARNING", "SEVERE", "CONFIG", "FINE", "FINER", "FINEST", "FATAL", "ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

/// Booleans are never reported: SmallRye reads anything that is not true/yes/y/1/on as false.
/// Enums are checked only when the type is a closed enum of the metadata (not a level: levels also
/// take the JUL names and numbers), ignoring case, `-` and `_`; the property's own default is always valid.
fn invalid(lookup: &Lookup, written: &str, key: &str, prop: Option<&Prop>, value: &str) -> Option<String> {
    let prop = prop?;
    let value = value.trim();
    if value.contains('\\') || prop.open() || same_word(&prop.default, value) {
        return None;
    }
    let level = prop.ty == "Level" || key.ends_with(".level") || key.ends_with("min-level");
    let ok = match prop.ty.as_str() {
        "int" | "Integer" | "long" | "Long" | "short" | "Short" => value.parse::<i64>().is_ok(),
        "double" | "Double" | "float" | "Float" => value.parse::<f64>().is_ok(),
        "Duration" => duration(value),
        "MemorySize" => memory_size(value),
        _ if level => value.parse::<i32>().is_ok() || JUL_LEVELS.iter().any(|l| l.eq_ignore_ascii_case(value)),
        "boolean" | "Boolean" | "string" | "String" | "path" | "Path" | "" => true,
        ty if ty.contains('<') => true,
        _ => {
            let values = lookup.values(key);
            values.is_empty() || values.iter().any(|v| same_word(v, value))
        }
    };
    if ok {
        return None;
    }
    Some(format!("`{value}` is not a valid `{}` for `{written}`.", prop.ty))
}

fn same_word(a: &str, b: &str) -> bool {
    let clean = |s: &str| s.chars().filter(|c| !matches!(c, '-' | '_')).flat_map(|c| c.to_lowercase()).collect::<String>();
    clean(a) == clean(b)
}

/// A number (milliseconds), an ISO-8601 `PT1H30M`, or a run of `<number><unit>` pairs
/// (`30s`, `1h30m`, `1.5s`, `2d`; units ms, s, m, h, d) separated by optional blanks.
pub(crate) fn duration(value: &str) -> bool {
    if value.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if value.starts_with(['P', 'p']) {
        return value.len() > 2 && value[1..].chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    }
    let mut rest = value.trim();
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
        if digits == 0 || rest[..digits].parse::<f64>().is_err() {
            return false;
        }
        let after = &rest[digits..];
        let unit = after.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(after.len());
        if !["ms", "s", "m", "h", "d"].iter().any(|u| u.eq_ignore_ascii_case(&after[..unit])) {
            return false;
        }
        rest = after[unit..].trim_start();
    }
    true
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
    fn empty_value_hints_only_when_metadata_has_a_default() {
        let text = "quarkus.http.port=\nquarkus.datasource.password=\napp.free=\nquarkus.http.root-path:\nquarkus.http.read-timeout=\n";
        let found = check_properties(text, bundled());
        let hinted: Vec<_> = found.iter().map(|f| (f.code, f.severity, &text[f.start..f.end])).collect();
        assert_eq!(hinted, [
            ("unassigned-value", Severity::Hint, "quarkus.http.port"),
            ("unassigned-value", Severity::Hint, "quarkus.http.root-path"),
            ("unassigned-value", Severity::Hint, "quarkus.http.read-timeout"),
        ]);
    }

    #[test]
    fn enum_value_not_in_metadata_warns() {
        let found = run("quarkus.http.ssl.client-auth=sometimes\nquarkus.http.ssl.client-auth=Required\nquarkus.http.ssl.client-auth=none\nquarkus.http.port=abc\nquarkus.http.cors.enabled=yes\nquarkus.http.cors.enabled=whatever\n");
        let invalid: Vec<_> = found.iter().filter(|(c, _)| *c == "invalid-value").map(|(_, t)| t.as_str()).collect();
        assert_eq!(invalid, ["sometimes", "abc"]);
    }

    #[test]
    fn log_levels_take_the_jul_names_and_defaults() {
        let ok = "quarkus.log.level=WARNING\nquarkus.log.console.level=ALL\nquarkus.log.console.level=OFF\nquarkus.log.min-level=finest\nquarkus.log.file.level=SEVERE\nquarkus.log.category.\"a.b\".level=700\n";
        assert!(!run(ok).iter().any(|(c, _)| *c == "invalid-value"), "{:?}", run(ok));
        assert_eq!(run("quarkus.log.level=LOUD\n"), [("invalid-value", "LOUD".into())]);
    }

    #[test]
    fn duration_and_memory_size_values_are_accepted() {
        let text = "%a.quarkus.http.read-timeout=1h30m\n%b.quarkus.http.read-timeout=1.5s\n%c.quarkus.http.read-timeout=2d\n%d.quarkus.http.read-timeout=500\n%e.quarkus.http.read-timeout=PT1H30M\n%f.quarkus.http.read-timeout=30s\n%g.quarkus.http.read-timeout=PT1M\n%h.quarkus.http.read-timeout=1h\nquarkus.http.limits.max-body-size=10M\nquarkus.http.limits.max-form-attribute-size=512K\nquarkus.http.limits.max-chunk-size=1G\n";
        assert!(run(text).is_empty(), "{:?}", run(text));
        assert_eq!(codes(&run("quarkus.http.read-timeout=abc\nquarkus.http.limits.max-body-size=big\n")), ["invalid-value", "invalid-value"]);
    }

    #[test]
    fn unknown_extension_value_is_quiet() {
        assert!(run("quarkus.acme.mode=whatever\nquarkus.acme.size=abc\nQUARKUS_HTTP_PORT=abc\nquarkus.log.category.\"io.acme\".level=DEBUG\n").is_empty());
    }

    #[test]
    fn unresolved_expression_warns_unless_env_or_system_property() {
        let text = "app.greeting=hi\napp.a=${app.greting}\napp.b=${HOSTNAME}\napp.c=${user.home}/x\napp.d=${missing:fallback}\napp.e=${app.greeting}\napp.f=${quarkus.http.port}\napp.g=${app.name}\n";
        assert_eq!(run(text), [("undefined-expression", "${app.greting}".into())]);
    }

    #[test]
    fn duplicates_across_yaml_documents_are_quiet() {
        assert!(run_yaml("a:\n  b: 1\n---\na:\n  b: 2\n").is_empty());
    }

    #[test]
    fn yaml_files_are_checked_the_same_way() {
        let text = "quarkus:\n  http:\n    port: abc\n    cors:\n      enabled: true\n  log:\n    level: DEBUG\napp:\n  name1: x\n  a: ${app.nme1}\n  b:\n";
        assert_eq!(run_yaml(text), [("invalid-value", "abc".into()), ("undefined-expression", "${app.nme1}".into())]);
        assert_eq!(codes(&run_yaml("a:\n  b: 1\n  b: 2\n")), ["duplicate-key"]);
    }

    #[test]
    fn ranges_survive_comments_continuations_escapes_and_multibyte_text() {
        let text = "# é😀\r\nlong=a\\\r\n  b\r\nlonger.key=1\r\nke\\:y=${longer.kez}\r\nquarkus.http.port=zz\r\n";
        let found = check_properties(text, bundled());
        let d: Vec<_> = found.iter().map(|f| f.diagnostic(text)).collect();
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].range.start.line, d[0].range.start.character), (4, 6));
        assert_eq!((d[1].range.start.line, d[1].range.start.character, d[1].range.end.character), (5, 18, 20));
        let emoji = "app.é😀=1\napp.é😀x=${app.é😀y}\n";
        let d = check_properties(emoji, bundled())[0].diagnostic(emoji);
        assert_eq!((d.range.start.line, d.range.start.character), (1, 9));
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
