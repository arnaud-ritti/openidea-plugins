//! devcontainer.json is JSONC (comments and trailing commas are legal), so the
//! file is read with a small tolerant scanner instead of a strict JSON parser.

use std::ops::Range;

use openidea_plugin::Severity;

pub const UNKNOWN_PROPERTY: &str = "unknown-property";
pub const FEATURES_MALFORMED: &str = "features-malformed";

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

struct Member {
    key: String,
    key_range: Range<usize>,
    value_range: Range<usize>,
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
