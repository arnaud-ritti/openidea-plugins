//! devcontainer.json is JSONC (comments and trailing commas are legal), so the
//! file is read with a small tolerant scanner instead of a strict JSON parser.

use std::ops::Range;

use openidea_plugin::Severity;

pub const UNKNOWN_PROPERTY: &str = "unknown-property";
pub const FEATURES_MALFORMED: &str = "features-malformed";
pub const FOLDER_STRUCTURE: &str = "folder-structure";

/// The properties of the Development Containers specification (containers.dev/implementors/json_reference).
const KNOWN: &[&str] = &[
    "image", "build", "dockerFile", "context", "dockerComposeFile", "service", "runServices", "workspaceFolder", "workspaceMount",
    "shutdownAction", "overrideCommand", "name", "forwardPorts", "portsAttributes", "otherPortsAttributes", "appPort", "containerEnv",
    "remoteEnv", "containerUser", "remoteUser", "updateRemoteUserUID", "userEnvProbe", "features", "overrideFeatureInstallOrder",
    "customizations", "mounts", "init", "privileged", "capAdd", "securityOpt", "runArgs", "onCreateCommand", "updateContentCommand",
    "postCreateCommand", "postStartCommand", "postAttachCommand", "waitFor", "initializeCommand", "hostRequirements", "secrets",
];

pub struct Finding {
    pub start: usize,
    pub end: usize,
    pub code: &'static str,
    pub message: String,
    pub severity: Severity,
}

impl Finding {
    fn warning(range: Range<usize>, code: &'static str, message: String) -> Self {
        Self { start: range.start, end: range.end, code, message, severity: Severity::Warning }
    }
}

pub enum Context {
    Key,
    Value { key: String },
}

struct Member {
    key: String,
    key_range: Range<usize>,
    value_range: Range<usize>,
}

pub fn in_valid_location(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let mut segments = path.rsplit('/');

    match segments.next() {
        Some(".devcontainer.json") => true,
        Some("devcontainer.json") => segments.take(2).any(|folder| folder == ".devcontainer"),
        _ => false,
    }
}

pub fn folder_structure(path: &str) -> Option<Finding> {
    if !path.replace('\\', "/").ends_with("/devcontainer.json") && path != "devcontainer.json" || in_valid_location(path) {
        return None;
    }

    Some(Finding {
        start: 0,
        end: 0,
        code: FOLDER_STRUCTURE,
        message: "Dev Container tools only read devcontainer.json from .devcontainer/, from one folder below it (.devcontainer/<name>/), or as .devcontainer.json at the project root.".into(),
        severity: Severity::Info,
    })
}

pub fn first_token(text: &str) -> Range<usize> {
    let bytes = text.as_bytes();
    let start = skip_trivia(bytes, if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 });
    let width = text[start..].chars().next().map_or(0, char::len_utf8);

    start..start + width
}

const SHUTDOWN_ACTIONS: &[&str] = &["none", "stopContainer", "stopCompose"];
const ENV_PROBES: &[&str] = &["none", "loginShell", "loginInteractiveShell", "interactiveShell"];
const WAIT_FOR: &[&str] =
    &["initializeCommand", "onCreateCommand", "updateContentCommand", "postCreateCommand", "postStartCommand", "postAttachCommand"];

/// Each value with whether it is written as a JSON string.
pub fn value_choices(key: &str) -> Vec<(&'static str, bool)> {
    let strings: &[&'static str] = match key {
        "shutdownAction" => SHUTDOWN_ACTIONS,
        "userEnvProbe" => ENV_PROBES,
        "waitFor" => WAIT_FOR,
        "updateRemoteUserUID" | "overrideCommand" | "init" | "privileged" => return vec![("true", false), ("false", false)],
        _ => &[],
    };

    strings.iter().map(|value| (*value, true)).collect()
}

/// The properties not yet present at the top level (the one being typed at `offset` does not count as present).
pub fn missing_properties(text: &str, offset: usize) -> Vec<&'static str> {
    let bytes = text.as_bytes();
    let start = skip_trivia(bytes, if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 });
    let present: Vec<String> = members(bytes, start)
        .into_iter()
        .filter(|m| !(m.key_range.start < offset && offset <= m.key_range.end))
        .map(|m| m.key)
        .collect();

    KNOWN.iter().copied().filter(|name| !present.iter().any(|key| key == name)).collect()
}

/// The string (quotes included) holding `offset` when it is a key of the top-level `features` object.
pub fn feature_key_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let bom = if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    let features = members(bytes, skip_trivia(bytes, bom))
        .into_iter()
        .find(|m| m.key == "features" && bytes.get(m.value_range.start) == Some(&b'{') && m.value_range.start < offset && offset <= m.value_range.end)?;

    let line_start = text.get(..offset)?.rfind('\n').map_or(0, |i| i + 1);
    let before = &text[line_start..offset];
    if before.matches('"').count() % 2 == 0 {
        return None;
    }
    let quote = line_start + before.rfind('"')?;

    let previous = text[..quote].trim_end();
    let at_key = match previous.as_bytes().last() {
        Some(b'{') => previous.len() - 1 == features.value_range.start,
        Some(b',') => members(bytes, features.value_range.start)
            .iter()
            .any(|m| skip_trivia(bytes, m.value_range.end) == previous.len() - 1),
        _ => false,
    };
    if !at_key {
        return None;
    }

    let end = match text[offset..].find(['"', '\n']) {
        Some(i) if text.as_bytes()[offset + i] == b'"' => offset + i + 1,
        _ => offset,
    };

    Some(quote..end)
}

/// The word (with its quotes) around `offset`, replaced when a completion is accepted.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let bytes = text.as_bytes();
    let part = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'"' | b'_' | b'-' | b'.');
    let mut start = offset;
    let mut end = offset;

    while start > 0 && part(&bytes[start - 1]) {
        start -= 1;
    }
    while end < bytes.len() && part(&bytes[end]) {
        end += 1;
    }

    start..end
}

/// Where `offset` sits in the top-level object: a key, or the value of a key.
/// Quiet (`None`) inside comments, nested values and unreadable text.
pub fn context_at(text: &str, offset: usize) -> Option<Context> {
    let bytes = text.as_bytes();
    if offset > bytes.len() {
        return None;
    }

    let bom = if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    let open = trivia_outside_comment(bytes, bom, offset)?;
    if bytes.get(open) != Some(&b'{') || offset <= open {
        return None;
    }

    let mut i = open + 1;
    loop {
        let j = trivia_outside_comment(bytes, i, offset)?;
        let quoted = bytes.get(j) == Some(&b'"');

        if offset < j || (offset == j && !quoted) {
            return Some(Context::Key);
        }
        if !quoted {
            return None;
        }

        let key_end = string_end(bytes, j);
        if offset <= key_end && (offset < key_end || bytes.get(key_end - 1) != Some(&b'"') || key_end - j < 2) {
            return Some(Context::Key);
        }

        let key = String::from_utf8_lossy(&bytes[j + 1..key_end.saturating_sub(1).max(j + 1)]).into_owned();
        let colon = trivia_outside_comment(bytes, key_end, offset)?;
        if offset <= colon || bytes.get(colon) != Some(&b':') {
            return None;
        }

        let value = trivia_outside_comment(bytes, colon + 1, offset)?;
        if offset <= value {
            return Some(Context::Value { key });
        }

        let value_stop = match bytes.get(value) {
            None | Some(b',' | b'}' | b']') => value,
            Some(_) => value_end(bytes, value),
        };
        if offset <= value_stop {
            return match bytes[value] {
                b'{' | b'[' => None,
                _ => Some(Context::Value { key }),
            };
        }

        let after = trivia_outside_comment(bytes, value_stop, offset)?;
        if offset <= after {
            return None;
        }

        i = after + usize::from(bytes.get(after) == Some(&b','));
    }
}

/// Like `skip_trivia`, but `None` when `offset` lies inside one of the comments skipped.
fn trivia_outside_comment(bytes: &[u8], mut at: usize, offset: usize) -> Option<usize> {
    loop {
        let next = skip_one_trivia(bytes, at);
        if next == at {
            return Some(at);
        }

        let inside = match (bytes[at], bytes.get(at + 1)) {
            (b'/', Some(b'/')) => at < offset && offset <= next,
            (b'/', Some(b'*')) => at < offset && (offset < next || !bytes[..next].ends_with(b"*/")),
            _ => false,
        };
        if inside {
            return None;
        }

        at = next;
    }
}

fn skip_one_trivia(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at) {
        Some(b' ' | b'\t' | b'\r' | b'\n') => at + 1,
        Some(b'/') if matches!(bytes.get(at + 1), Some(b'/' | b'*')) => skip_trivia_comment(bytes, at),
        _ => at,
    }
}

fn skip_trivia_comment(bytes: &[u8], at: usize) -> usize {
    let mut i = at;
    if bytes[at + 1] == b'/' {
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        return i;
    }

    i += 2;
    while i < bytes.len() && !bytes[i..].starts_with(b"*/") {
        i += 1;
    }

    (i + 2).min(bytes.len())
}

pub fn check(text: &str) -> Vec<Finding> {
    let mut findings = vec![];
    let bytes = text.as_bytes();
    let bom = if bytes.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
    let start = skip_trivia(bytes, bom);

    for member in members(bytes, start) {
        if member.key == "features" {
            check_features(text, &member, &mut findings);
            continue;
        }

        if let Some(known) = typo_of(&member.key) {
            findings.push(Finding::warning(
                member.key_range.clone(),
                UNKNOWN_PROPERTY,
                format!("Unknown property `{}`; did you mean `{known}`?", member.key),
            ));
        }
    }

    findings
}

fn check_features(text: &str, member: &Member, findings: &mut Vec<Finding>) {
    let bytes = text.as_bytes();

    if bytes[member.value_range.start] != b'{' {
        findings.push(Finding::warning(
            member.value_range.clone(),
            FEATURES_MALFORMED,
            "`features` must be an object mapping feature ids to options".into(),
        ));
        return;
    }

    for feature in members(bytes, member.value_range.start) {
        if feature.key.is_empty() || feature.key.chars().any(char::is_whitespace) {
            findings.push(Finding::warning(
                feature.key_range.clone(),
                FEATURES_MALFORMED,
                "A feature id must not be empty or contain whitespace".into(),
            ));
        }

        let value_ok = match bytes[feature.value_range.start] {
            b'{' | b'"' => true,
            b't' | b'f' => matches!(&text[feature.value_range.clone()], "true" | "false"),
            b'n' => true,
            _ => false,
        };
        if !value_ok {
            findings.push(Finding::warning(
                feature.value_range.clone(),
                FEATURES_MALFORMED,
                format!("The options of feature `{}` must be an object, a version string or a boolean", feature.key),
            ));
        }
    }
}

fn typo_of(key: &str) -> Option<&'static str> {
    if key.starts_with("x-") || key.starts_with('$') || KNOWN.contains(&key) {
        return None;
    }

    KNOWN
        .iter()
        .map(|known| (distance(key, known), *known))
        .filter(|(distance, _)| *distance <= 2)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, known)| known)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();

    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1).min(row[j] + 1).min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }

    row[b.len()]
}

fn skip_trivia(bytes: &[u8], mut at: usize) -> usize {
    loop {
        match bytes.get(at) {
            Some(b' ' | b'\t' | b'\r' | b'\n') => at += 1,
            Some(b'/') if bytes.get(at + 1) == Some(&b'/') => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            Some(b'/') if bytes.get(at + 1) == Some(&b'*') => {
                at += 2;
                while at < bytes.len() && !bytes[at..].starts_with(b"*/") {
                    at += 1;
                }
                at = (at + 2).min(bytes.len());
            }
            _ => return at,
        }
    }
}

/// End offset (after the closing quote) of the string starting at `at`.
fn string_end(bytes: &[u8], at: usize) -> usize {
    let mut i = at + 1;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            b'\n' => return i,
            _ => i += 1,
        }
    }

    bytes.len()
}

/// End offset of the value starting at `at` (a scalar, string, object or array).
fn value_end(bytes: &[u8], at: usize) -> usize {
    match bytes[at] {
        b'"' => string_end(bytes, at),
        b'{' | b'[' => {
            let mut depth = 0;
            let mut i = at;

            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        i = string_end(bytes, i);
                        continue;
                    }
                    b'/' if matches!(bytes.get(i + 1), Some(b'/' | b'*')) => {
                        i = skip_trivia(bytes, i);
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            bytes.len()
        }
        _ => {
            let mut i = at;
            while i < bytes.len() && !matches!(bytes[i], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n' | b'/') {
                i += 1;
            }
            i.max(at + 1)
        }
    }
}

/// The members of the object starting at `at`; stops quietly at anything malformed.
fn members(bytes: &[u8], at: usize) -> Vec<Member> {
    let mut found = vec![];
    if bytes.get(at) != Some(&b'{') {
        return found;
    }

    let mut i = at + 1;
    loop {
        i = skip_trivia(bytes, i);
        if bytes.get(i) != Some(&b'"') {
            return found;
        }

        let key_end = string_end(bytes, i);
        if key_end < i + 2 || bytes[key_end - 1] != b'"' {
            return found;
        }
        let key = String::from_utf8_lossy(&bytes[i + 1..key_end - 1]).into_owned();
        let key_range = i..key_end;

        i = skip_trivia(bytes, key_end);
        if bytes.get(i) != Some(&b':') {
            return found;
        }
        i = skip_trivia(bytes, i + 1);
        if i >= bytes.len() || matches!(bytes[i], b',' | b'}' | b']') {
            return found;
        }

        let end = value_end(bytes, i);
        found.push(Member { key, key_range, value_range: i..end });

        i = skip_trivia(bytes, end);
        if bytes.get(i) == Some(&b',') {
            i += 1;
        }
    }
}
