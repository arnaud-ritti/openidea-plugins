//! Spring Security Tools for OpenIDEA: flags configuration that Spring
//! Security 6 removed or deprecated (with quick fixes where the change is
//! mechanical), redundant `ROLE_` prefixes and plain-text password encoders,
//! and completes SpEL in method security annotations. Text-based: Java and
//! Kotlin sources, and the security namespace of Spring XML files.

use openidea_plugin::providers::{CodeAction, CompletionItem, CompletionKind, Document, LineMarker, Providers};
use openidea_plugin::text::offset;
use openidea_plugin::{Diagnostic, Error, Invocation, Plugin, Position, Range, Severity, TextEdit, host, register_plugin};
use spring_text::index::{Indexes, PROVIDER_BUDGET};
use spring_text::java::{self, Annotation, JavaFile};
use spring_text::{range, xml};
use std::collections::BTreeSet;

const METHOD_SECURITY: [&str; 4] = ["PreAuthorize", "PostAuthorize", "PreFilter", "PostFilter"];
const GLOBAL_METHOD_SECURITY: &str =
    "org.springframework.security.config.annotation.method.configuration.EnableGlobalMethodSecurity";
const ROLE_PREFIX: &str = "ROLE_";
const SECURITY_NAMESPACE: &str = "http://www.springframework.org/schema/security";
const WEB_EXPRESSION_FUNCTIONS: [&str; 12] = [
    "hasRole",
    "hasAnyRole",
    "hasAuthority",
    "hasAnyAuthority",
    "hasIpAddress",
    "hasPermission",
    "isAnonymous",
    "isAuthenticated",
    "isFullyAuthenticated",
    "isRememberMe",
    "permitAll",
    "denyAll",
];
const ARGUMENT_FUNCTIONS: [&str; 5] = ["hasRole", "hasAnyRole", "hasAuthority", "hasAnyAuthority", "hasIpAddress"];
/// SpEL completions: label, snippet, the annotations it applies to (empty: all).
const SPEL: [(&str, &str, &[&str]); 15] = [
    ("hasRole", "hasRole('$1')", &[]),
    ("hasAnyRole", "hasAnyRole($1)", &[]),
    ("hasAuthority", "hasAuthority('$1')", &[]),
    ("hasAnyAuthority", "hasAnyAuthority($1)", &[]),
    ("hasPermission", "hasPermission($1, '$2')", &[]),
    ("isAuthenticated", "isAuthenticated()", &[]),
    ("isAnonymous", "isAnonymous()", &[]),
    ("isFullyAuthenticated", "isFullyAuthenticated()", &[]),
    ("isRememberMe", "isRememberMe()", &[]),
    ("permitAll", "permitAll", &[]),
    ("denyAll", "denyAll", &[]),
    ("principal", "principal", &[]),
    ("authentication", "authentication", &[]),
    ("returnObject", "returnObject", &["PostAuthorize", "PostFilter"]),
    ("filterObject", "filterObject", &["PreFilter", "PostFilter"]),
];

/// A problem and its quick fix: title and edits of the document.
struct Problem {
    diagnostic: Diagnostic,
    fix: Option<(String, Vec<TextEdit>)>,
}

/// The quoted arguments of `hasRole(…)`/`hasAnyRole(…)` in a SpEL expression: byte offset and value.
fn spel_roles(expr: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    for call in ["hasRole(", "hasAnyRole("] {
        for (i, _) in expr.match_indices(call) {
            let from = i + call.len();
            let to = expr[from..].find(')').map_or(expr.len(), |e| from + e);
            let mut at = from;
            while let Some(q) = expr[at..to].find(['\'', '"']) {
                let start = at + q + 1;
                let Some(len) = expr[start..to].find(&expr[at + q..start]) else { break };
                out.push((start, &expr[start..start + len]));
                at = start + len + 1;
            }
        }
    }
    out.sort();
    out
}

fn annotations(file: &JavaFile) -> impl Iterator<Item = &Annotation> {
    file.types.iter().flat_map(|t| t.annotations.iter().chain(t.members.iter().flat_map(|m| &m.annotations)))
}

/// The role names a Java source uses, `ROLE_` stripped.
fn roles(_path: &str, text: &str) -> Option<Vec<String>> {
    if !text.contains("Role") && !text.contains("Secured") {
        return None;
    }
    let file = java::parse(text);
    let mut out = BTreeSet::new();
    for a in annotations(&file) {
        if a.name == "Secured" || a.name == "RolesAllowed" {
            out.extend(a.strings(&["value"]).iter().map(|s| s.value.clone()));
        }
        if METHOD_SECURITY.contains(&a.name.as_str()) {
            for s in a.strings(&["value"]) {
                out.extend(spel_roles(&s.value).into_iter().map(|(_, r)| r.to_owned()));
            }
        }
    }
    for c in java::calls(text).iter().filter(|c| c.name == "hasRole" || c.name == "hasAnyRole") {
        out.extend(c.args.iter().filter_map(|a| a.string.as_ref()).map(|s| s.value.clone()));
    }
    let out: Vec<String> = out
        .into_iter()
        .map(|r| r.strip_prefix(ROLE_PREFIX).map(str::to_owned).unwrap_or(r))
        .filter(|r| !r.is_empty() && !r.contains(['$', '#', '{']))
        .collect();
    (!out.is_empty()).then_some(out)
}

fn problem(text: &str, from: usize, to: usize, severity: Severity, code: &str, message: String) -> Diagnostic {
    Diagnostic::new(range(text, from, to), severity, message).code(code)
}

fn edit(text: &str, from: usize, to: usize, new: &str) -> TextEdit {
    TextEdit { range: range(text, from, to), text: new.to_owned() }
}

fn role_prefix(text: &str, at: usize, role: &str) -> Problem {
    let message = format!("hasRole and hasAnyRole add the `ROLE_` prefix themselves: write `{role}`.");
    Problem {
        diagnostic: problem(text, at, at + ROLE_PREFIX.len() + role.len(), Severity::Warning, "role-prefix", message),
        fix: Some(("Remove the ROLE_ prefix".into(), vec![edit(text, at, at + ROLE_PREFIX.len(), "")])),
    }
}

/// Quotes or parentheses left open in a SpEL expression.
fn is_unbalanced(expr: &str) -> bool {
    let mut quote = None;
    let mut depth = 0i32;
    for c in expr.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => {}
        }
    }
    quote.is_some() || depth != 0
}

/// Problems of a SpEL web security expression: byte span, code, severity, message.
fn expression_problems(expr: &str) -> Vec<(usize, usize, &'static str, Severity, String)> {
    if is_unbalanced(expr) {
        return vec![(0, expr.len(), "unbalanced-expression", Severity::Error, "Unbalanced quotes or parentheses in this expression.".into())];
    }
    let mut out = Vec::new();
    let bytes = expr.as_bytes();
    let mut quote = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
        }
        let starts_word = c.is_ascii_alphabetic() && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || b"_.@#$".contains(&bytes[i - 1])));
        if !starts_word {
            i += 1;
            continue;
        }
        let end = expr[i..].find(|ch: char| !(ch.is_alphanumeric() || ch == '_')).map_or(expr.len(), |n| i + n);
        let name = &expr[i..end];
        let after = expr[end..].trim_start();
        if after.starts_with('(') {
            let open = expr.len() - after.len();
            let known = WEB_EXPRESSION_FUNCTIONS.contains(&name);
            let close = expr[open..].find(')').map_or(expr.len(), |n| open + n);
            if known && ARGUMENT_FUNCTIONS.contains(&name) && expr[open + 1..close].trim().is_empty() {
                out.push((i, close + 1, "empty-security-function", Severity::Error, format!("`{name}` needs an argument.")));
            }
            let looks_like_check = ["has", "is"].iter().any(|p| name.strip_prefix(p).is_some_and(|rest| rest.starts_with(|ch: char| ch.is_uppercase())));
            if !known && looks_like_check {
                let message = format!("`{name}` is not a Spring Security web expression function.");
                out.push((i, end, "unknown-security-function", Severity::Warning, message));
            }
        }
        i = end;
    }
    for (at, role) in spel_roles(expr) {
        if let Some(bare) = role.strip_prefix(ROLE_PREFIX) {
            let message = format!("hasRole and hasAnyRole add the `ROLE_` prefix themselves: write `{bare}`.");
            out.push((at, at + role.len(), "role-prefix", Severity::Warning, message));
        }
    }
    out.sort_by_key(|p| p.0);
    out
}

/// `text` with Kotlin comments and string literals blanked out (same byte offsets).
fn kotlin_code_only(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut [u8], from: usize, to: usize| {
        for byte in &mut out[from..to] {
            if *byte != b'\n' && byte.is_ascii() {
                *byte = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        let start = i;
        if b[i..].starts_with(b"//") {
            i = text[i..].find('\n').map_or(b.len(), |n| i + n);
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if b[i..].starts_with(b"\"\"\"") {
            i = text[i + 3..].find("\"\"\"").map_or(b.len(), |n| i + 3 + n + 3);
        } else if b[i] == b'\'' {
            let mut j = i + 1;
            while j < b.len() && j <= i + 8 && b[j] != b'\'' && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            if j >= b.len() || j > i + 8 || b[j] != b'\'' {
                i += 1;
                continue;
            }
            i = j + 1;
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
        } else {
            i += 1;
            continue;
        }
        blank(&mut out, start, i.min(b.len()));
    }
    String::from_utf8(out).unwrap_or_default()
}

fn xml_problems(text: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    if !text.contains(SECURITY_NAMESPACE) {
        return out;
    }
    let elements = xml::elements(text);
    let diagnostic = |from: usize, to: usize, severity: Severity, code: &str, message: String| Problem {
        diagnostic: problem(text, from, to, severity, code, message),
        fix: None,
    };
    for (i, e) in elements.iter().enumerate() {
        if xml::namespace(&elements, i) != Some(SECURITY_NAMESPACE) {
            continue;
        }
        if e.local() == "global-method-security" {
            let message = "<global-method-security> is deprecated: use <method-security>.".to_owned();
            let mut found = diagnostic(e.at, e.at + e.name.len(), Severity::Warning, "global-method-security", message);
            found.diagnostic.deprecated = true;
            out.push(found);
        }
        if e.local() == "debug" && host::setting_bool("springSecurity.checkDebug").unwrap_or(true) {
            let message = "Security debug logging is on: it logs request details and headers. Turn it off in production.".to_owned();
            out.push(diagnostic(e.at, e.at + e.name.len(), Severity::Warning, "debug-enabled", message));
        }
        if e.local() != "http" {
            continue;
        }
        let expressions = e.value("use-expressions") != Some("false");
        if let Some(a) = e.attr("use-expressions").filter(|_| !expressions) {
            let message = "`use-expressions=\"false\"` is deprecated: write access rules as expressions (hasRole('ADMIN')).".to_owned();
            let mut found = diagnostic(a.at, a.value_end + 1, Severity::Warning, "use-expressions-false", message);
            found.diagnostic.deprecated = true;
            out.push(found);
        }
        let urls: Vec<&xml::Element> = elements
            .iter()
            .enumerate()
            .filter(|(j, u)| u.parent == Some(i) && u.local() == "intercept-url" && xml::namespace(&elements, *j) == Some(SECURITY_NAMESPACE))
            .map(|(_, u)| u)
            .collect();
        let mut seen: Vec<(&str, &str, &str)> = Vec::new();
        let ant_patterns = e.attr("request-matcher").is_none_or(|a| a.value == "ant" || a.value == "mvc") && e.attr("request-matcher-ref").is_none();
        for (n, u) in urls.iter().enumerate() {
            if let Some(access) = u.attr("access").filter(|_| expressions) {
                let exact = !access.value.contains('&');
                for (from, to, code, severity, message) in expression_problems(&access.value) {
                    let (from, to) = if exact { (access.value_at + from, access.value_at + to) } else { (access.value_at, access.value_end) };
                    out.push(diagnostic(from, to, severity, code, message));
                }
            }
            let Some(pattern) = u.attr("pattern") else { continue };
            let key = (pattern.value.as_str(), u.value("method").unwrap_or(""), u.value("servlet-path").unwrap_or(""));
            if seen.contains(&key) {
                let message = format!("The pattern `{}` is already matched by an earlier <intercept-url>.", pattern.value);
                out.push(diagnostic(pattern.value_at, pattern.value_end, Severity::Warning, "duplicate-pattern", message));
            }
            seen.push(key);
            let catch_all = ant_patterns && matches!(pattern.value.as_str(), "/**" | "**") && u.attr("method").is_none() && u.attr("servlet-path").is_none();
            if catch_all && urls[n + 1..].iter().any(|later| later.value("pattern").is_some_and(|p| p != pattern.value)) {
                let message = "This catch-all pattern comes before more specific ones, which can never match: move it last.".to_owned();
                out.push(diagnostic(pattern.value_at, pattern.value_end, Severity::Warning, "unreachable-patterns", message));
            }
        }
    }
    out
}

const MATCHER_CALLS: [&str; 4] = ["requestMatchers", "antMatchers", "mvcMatchers", "regexMatchers"];
const RULE_BLOCKS: [&str; 2] = ["authorizeHttpRequests", "authorizeRequests"];

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// The index of the bracket closing the one at `open`, in comment- and string-free code.
fn closing(code: &str, open: usize) -> usize {
    let mut depth = 0;
    for (i, b) in code.bytes().enumerate().skip(open) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    code.len()
}

/// Where a legacy `authorizeRequests().a().b();` chain ends.
fn statement_end(code: &str, from: usize) -> usize {
    let mut depth = 0;
    for (i, b) in code.bytes().enumerate().skip(from) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' if depth == 0 => return i,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth == 0 => return i,
            _ => {}
        }
    }
    code.len()
}

/// Byte ranges of the top-level arguments between the brackets `open..close`.
fn argument_ranges(code: &str, open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut depth, mut from) = (0, open + 1);
    for (i, b) in code.bytes().enumerate().take(close).skip(open + 1) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push((from, i));
                from = i + 1;
            }
            _ => {}
        }
    }
    if from < close {
        out.push((from, close));
    }
    out
}

fn trimmed(text: &str, (from, to): (usize, usize)) -> (usize, usize) {
    let slice = &text[from..to];
    let start = from + (slice.len() - slice.trim_start().len());
    (start, start + slice.trim().len())
}

struct Rule {
    at: usize,
    name_end: usize,
    any_request: bool,
    method: Option<String>,
    /// Ant-style literal patterns: start, end (inside the quotes), value.
    patterns: Vec<(usize, usize, String)>,
}

/// The `HttpMethod` and the literal patterns that start an argument list; stops at the first dynamic argument.
fn leading_patterns(text: &str, args: &[(usize, usize)]) -> (Option<String>, Vec<(usize, usize, String)>) {
    let mut method = None;
    let mut patterns = Vec::new();
    for (n, range) in args.iter().enumerate() {
        let (from, to) = trimmed(text, *range);
        let raw = &text[from..to];
        if n == 0
            && let Some(name) = raw.strip_prefix("HttpMethod.")
        {
            method = Some(name.to_owned());
            continue;
        }
        let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else { break };
        if inner.contains(['"', '\\', '$']) {
            break;
        }
        patterns.push((from + 1, to - 1, inner.to_owned()));
    }
    (method, patterns)
}

fn pattern_covers(broad: &str, narrow: &str) -> bool {
    if broad == narrow || broad == "/**" || broad == "**" {
        return true;
    }
    let Some(prefix) = broad.strip_suffix("**") else { return false };
    prefix.ends_with('/') && !prefix.contains(['*', '?', '{']) && (narrow.starts_with(prefix) || narrow == prefix.trim_end_matches('/'))
}

/// The rules of one `authorizeHttpRequests`/`authorizeRequests` configuration, in source order.
fn rules_in(text: &str, code: &str, kotlin: bool, scope: (usize, usize)) -> Vec<Rule> {
    let mut out = Vec::new();
    let bytes = code.as_bytes();
    let mut i = scope.0;
    while i < scope.1 {
        if !is_ident_byte(bytes[i]) || (i > 0 && is_ident_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let end = (i..scope.1).find(|&j| !is_ident_byte(bytes[j])).unwrap_or(scope.1);
        let name = &code[i..end];
        let open = end + code[end..].len() - code[end..].trim_start().len();
        let has_args = bytes.get(open) == Some(&b'(');
        let args = || {
            let close = closing(code, open);
            (close, argument_ranges(code, open, close))
        };
        let rule = |any_request: bool, method, patterns| Rule { at: i, name_end: end, any_request, method, patterns };
        if name == "anyRequest" {
            out.push(rule(true, None, Vec::new()));
        } else if has_args && MATCHER_CALLS.contains(&name) {
            let (_, ranges) = args();
            let (method, patterns) = if name == "regexMatchers" { (None, Vec::new()) } else { leading_patterns(text, &ranges) };
            out.push(rule(false, method, patterns));
        } else if kotlin && has_args && name == "authorize" {
            let (close, ranges) = args();
            if ranges.first().is_some_and(|r| text[trimmed(text, *r).0..trimmed(text, *r).1] == *"anyRequest") {
                out.push(rule(true, None, Vec::new()));
            } else {
                let (method, patterns) = leading_patterns(text, &ranges);
                out.push(rule(false, method, patterns));
            }
            i = close;
            continue;
        }
        i = end;
    }
    out
}

fn matcher_problems(text: &str, code: &str, kotlin: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut warn = |from: usize, to: usize, code: &str, message: String| {
        out.push(Problem { diagnostic: problem(text, from, to, Severity::Warning, code, message), fix: None });
    };
    for block in RULE_BLOCKS {
        for (at, _) in code.match_indices(block) {
            let after = at + block.len();
            if at > 0 && is_ident_byte(code.as_bytes()[at - 1]) || code.as_bytes().get(after).is_some_and(|b| is_ident_byte(*b)) {
                continue;
            }
            let open = after + code[after..].len() - code[after..].trim_start().len();
            let scope = match code.as_bytes().get(open) {
                Some(b'{') => (open + 1, closing(code, open)),
                Some(b'(') => {
                    let close = closing(code, open);
                    if code[open + 1..close].trim().is_empty() { (close + 1, statement_end(code, close + 1)) } else { (open + 1, close) }
                }
                _ => continue,
            };
            let mut any_request = false;
            let mut earlier: Vec<&Rule> = Vec::new();
            let rules = rules_in(text, code, kotlin, scope);
            for rule in &rules {
                if any_request {
                    let message = "Spring Security rejects this at startup: `anyRequest()` must be the last rule, nothing can follow it.";
                    warn(rule.at, rule.name_end, "matchers-after-any-request", message.to_owned());
                    continue;
                }
                any_request = rule.any_request;
                for (from, to, pattern) in &rule.patterns {
                    let covering = earlier.iter().find(|e| {
                        (e.method.is_none() || e.method == rule.method) && e.patterns.iter().any(|(_, _, broad)| pattern_covers(broad, pattern))
                    });
                    if let Some(first) = covering.and_then(|e| e.patterns.iter().find(|(_, _, broad)| pattern_covers(broad, pattern))) {
                        let message = format!("`{pattern}` is never reached: the earlier rule for `{}` already matches it. Put the narrower pattern first.", first.2);
                        warn(*from, *to, "matcher-order", message);
                    }
                }
                earlier.push(rule);
            }
        }
    }
    out
}

const PATH_PATTERN_CALLS: [&str; 3] = ["requestMatchers", "mvcMatchers", "securityMatcher"];

fn path_pattern_problems(text: &str, code: &str, kotlin: bool) -> Vec<Problem> {
    let mut out = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !is_ident_byte(bytes[i]) || (i > 0 && is_ident_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let end = (i..bytes.len()).find(|&j| !is_ident_byte(bytes[j])).unwrap_or(bytes.len());
        let name = &code[i..end];
        let open = end + code[end..].len() - code[end..].trim_start().len();
        let is_call = PATH_PATTERN_CALLS.contains(&name) || kotlin && name == "authorize";
        i = end;
        if !is_call || bytes.get(open) != Some(&b'(') {
            continue;
        }
        let close = closing(code, open);
        let (_, patterns) = leading_patterns(text, &argument_ranges(code, open, close));
        for (from, _, pattern) in patterns {
            let segments: Vec<&str> = pattern.split('/').collect();
            let mut offset = from;
            for (n, segment) in segments.iter().enumerate() {
                if *segment == "**" && n + 1 < segments.len() {
                    let message = "A `**` before the last path segment is rejected by PathPatternParser, which Spring MVC request matching uses, and is not supported there: `**` may only end a pattern.";
                    let diagnostic = problem(text, offset, offset + 2, Severity::Warning, "obsolete-path-pattern", message.to_owned());
                    out.push(Problem { diagnostic, fix: None });
                }
                offset += segment.len() + 1;
            }
        }
    }
    out
}

fn debug_problems(text: &str, code: &str) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut warn = |from: usize, to: usize| {
        let message = "Security debug logging is on: it logs request details and headers. Turn it off in production.";
        out.push(Problem { diagnostic: problem(text, from, to, Severity::Warning, "debug-enabled", message.into()), fix: None });
    };
    for (at, _) in code.match_indices("@EnableWebSecurity") {
        let after = at + "@EnableWebSecurity".len();
        let open = after + code[after..].len() - code[after..].trim_start().len();
        if code.as_bytes().get(open) != Some(&b'(') {
            continue;
        }
        let close = closing(code, open);
        let debug_on = argument_ranges(code, open, close).into_iter().any(|(from, to)| {
            code[from..to].trim().strip_prefix("debug").and_then(|r| r.trim_start().strip_prefix('=')).is_some_and(|v| v.trim() == "true")
        });
        if debug_on {
            warn(at, (close + 1).min(text.len()));
        }
    }
    if code.contains("WebSecurityCustomizer") {
        for (at, _) in code.match_indices(".debug(") {
            let open = at + ".debug".len();
            let close = closing(code, open);
            if code[open + 1..close].trim() == "true" {
                warn(at + 1, close + 1);
            }
        }
    }
    out
}

fn problems(path: &str, text: &str) -> Vec<Problem> {
    if path.ends_with(".xml") {
        return xml_problems(text);
    }
    let file = spring_text::parse_source(path, text);
    let mut out = Vec::new();
    for e in file.types.iter().flat_map(|t| &t.extends).filter(|e| e.name == "WebSecurityConfigurerAdapter") {
        let message = "WebSecurityConfigurerAdapter was removed in Spring Security 6: declare a SecurityFilterChain @Bean instead.";
        out.push(Problem {
            diagnostic: problem(text, e.at, e.at + e.name.len(), Severity::Warning, "web-security-configurer-adapter", message.into()),
            fix: None,
        });
    }
    for a in annotations(&file) {
        if a.name == "EnableGlobalMethodSecurity" {
            let kept: Vec<String> = a
                .args
                .iter()
                .filter(|arg| !(arg.name == "prePostEnabled" && arg.text == "true"))
                .map(|arg| format!("{} = {}", arg.name, arg.text))
                .collect();
            let replacement =
                if kept.is_empty() { "EnableMethodSecurity".to_owned() } else { format!("EnableMethodSecurity({})", kept.join(", ")) };
            let mut edits = vec![edit(text, a.name_at, a.end, &replacement)];
            if let Some(i) = file.imports.iter().find(|i| i.path == GLOBAL_METHOD_SECURITY) {
                edits.push(edit(text, i.at, i.end, &GLOBAL_METHOD_SECURITY.replace("EnableGlobal", "Enable")));
            }
            let message = "@EnableGlobalMethodSecurity is deprecated: use @EnableMethodSecurity (pre/post annotations are on by default).";
            let mut diagnostic = problem(text, a.at, a.end, Severity::Warning, "enable-global-method-security", message.into());
            diagnostic.deprecated = true;
            out.push(Problem { diagnostic, fix: Some(("Replace with @EnableMethodSecurity".into(), edits)) });
        }
        if METHOD_SECURITY.contains(&a.name.as_str()) {
            for s in a.strings(&["value"]).into_iter().filter(|s| s.value.len() == s.end - s.at) {
                for (at, role) in spel_roles(&s.value) {
                    if let Some(bare) = role.strip_prefix(ROLE_PREFIX) {
                        out.push(role_prefix(text, s.at + at, bare));
                    }
                }
            }
        }
    }
    let security = file.imports.iter().any(|i| i.path.starts_with("org.springframework.security"));
    for c in spring_text::source_calls(path, text) {
        let name_end = c.at + c.name.len();
        let renamed = match c.name.as_str() {
            "antMatchers" | "mvcMatchers" | "regexMatchers" if security => Some("requestMatchers"),
            "authorizeRequests" if security => Some("authorizeHttpRequests"),
            _ => None,
        };
        if let Some(new) = renamed {
            let (message, deprecated) = match c.name.as_str() {
                "authorizeRequests" => ("`authorizeRequests` is deprecated: use `authorizeHttpRequests`.".to_owned(), true),
                "regexMatchers" => (
                    "`regexMatchers` was removed in Spring Security 6: use `requestMatchers(RegexRequestMatcher.regexMatcher(…))`.".into(),
                    false,
                ),
                old => (format!("`{old}` was removed in Spring Security 6: use `requestMatchers`."), false),
            };
            let code = if deprecated { "authorize-requests" } else { "removed-request-matcher" };
            let mut diagnostic = problem(text, c.at, name_end, Severity::Warning, code, message);
            diagnostic.deprecated = deprecated;
            let fix = (c.name != "regexMatchers").then(|| (format!("Rename to {new}"), vec![edit(text, c.at, name_end, new)]));
            out.push(Problem { diagnostic, fix });
        }
        if security && (c.name == "hasRole" || c.name == "hasAnyRole") {
            for s in c.args.iter().filter_map(|a| a.string.as_ref()) {
                if let Some(bare) = s.value.strip_prefix(ROLE_PREFIX)
                    && s.value.len() == s.end - s.at
                {
                    out.push(role_prefix(text, s.at, bare));
                }
            }
        }
        if c.name == "getInstance" && c.receiver.as_deref() == Some("NoOpPasswordEncoder") {
            let message = "NoOpPasswordEncoder keeps passwords in plain text: use PasswordEncoderFactories.createDelegatingPasswordEncoder() or BCryptPasswordEncoder.";
            out.push(Problem {
                diagnostic: problem(text, text[..c.at].rfind("NoOpPasswordEncoder").unwrap_or(c.at), c.end, Severity::Warning, "no-op-password-encoder", message.into()),
                fix: None,
            });
        }
        if c.name == "password"
            && let Some(s) = c.args.first().and_then(|a| a.string.as_ref())
            && s.value.starts_with("{noop}")
        {
            let message = "This `{noop}` password is stored and compared in plain text.".to_owned();
            out.push(Problem { diagnostic: problem(text, s.at, s.end, Severity::Info, "plain-text-password", message), fix: None });
        }
    }
    let kotlin = spring_text::is_kotlin(path);
    if security {
        let code = kotlin_code_only(text);
        if host::setting_bool("springSecurity.checkMatchers").unwrap_or(true) {
            out.extend(matcher_problems(text, &code, kotlin));
        }
        if host::setting_bool("springSecurity.checkPathPatterns").unwrap_or(true) {
            out.extend(path_pattern_problems(text, &code, kotlin));
        }
        if host::setting_bool("springSecurity.checkDebug").unwrap_or(true) {
            out.extend(debug_problems(text, &code));
        }
    }
    if security && kotlin {
        let code = kotlin_code_only(text);
        for (at, _) in code.match_indices("authorizeRequests") {
            let after = at + "authorizeRequests".len();
            let standalone = !code[..at].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            if standalone && code[after..].trim_start().starts_with('{') {
                let message = "`authorizeRequests` is deprecated: use `authorizeHttpRequests`.".to_owned();
                let mut diagnostic = problem(text, at, after, Severity::Warning, "authorize-requests", message);
                diagnostic.deprecated = true;
                let fix = Some(("Rename to authorizeHttpRequests".to_owned(), vec![edit(text, at, after, "authorizeHttpRequests")]));
                out.push(Problem { diagnostic, fix });
            }
        }
        out.sort_by_key(|p| p.diagnostic.range.start);
    }
    out
}

pub struct SecurityTools {
    roles: Indexes<Vec<String>>,
}

impl Default for SecurityTools {
    fn default() -> Self {
        Self { roles: Indexes::new(&["**/*.java"], roles) }
    }
}

fn overlaps(a: Range, b: Range) -> bool {
    a.start <= b.end && b.start <= a.end
}

impl Providers for SecurityTools {
    fn line_markers(&mut self, doc: &Document) -> Vec<LineMarker> {
        let found: Vec<Diagnostic> = problems(&doc.path, &doc.text).into_iter().map(|p| p.diagnostic).collect();
        let _ = host::publish_diagnostics(&doc.path, &found);
        Vec::new()
    }

    fn code_actions(&mut self, doc: &Document, selection: Range) -> Vec<CodeAction> {
        problems(&doc.path, &doc.text)
            .into_iter()
            .filter(|p| overlaps(p.diagnostic.range, selection))
            .filter_map(|p| p.fix)
            .map(|(title, edits)| CodeAction {
                title,
                kind: Some("quickfix".into()),
                preferred: true,
                edits: vec![(doc.path.clone(), edits)],
                data: None,
            })
            .collect()
    }

    fn completion(&mut self, doc: &Document, at: Position, _trigger: Option<&str>) -> Vec<CompletionItem> {
        let Some(off) = offset(&doc.text, at) else { return Vec::new() };
        if spring_text::is_kotlin(&doc.path) {
            return Vec::new();
        }
        let file = java::parse(&doc.text);
        let Some((annotation, s)) = annotations(&file)
            .filter(|a| METHOD_SECURITY.contains(&a.name.as_str()))
            .find_map(|a| a.strings(&["value"]).into_iter().find(|s| s.holds(off)).map(|s| (a.name.clone(), s.clone())))
        else {
            return Vec::new();
        };
        let before = &doc.text[s.at..off];
        let open_call = ["hasRole(", "hasAnyRole("].iter().filter_map(|c| before.rfind(c)).max();
        let in_role = open_call.is_some_and(|p| !before[p..].contains(')') && before[p..].matches('\'').count() % 2 == 1);
        if in_role {
            let Some(index) = self.roles.for_document(&doc.path, &doc.text, PROVIDER_BUDGET) else { return Vec::new() };
            let roles: BTreeSet<&String> = index.iter().flat_map(|(_, r)| r).collect();
            return roles
                .into_iter()
                .map(|r| CompletionItem { detail: Some("role".into()), ..CompletionItem::new(r, CompletionKind::Value) })
                .collect();
        }
        SPEL.iter()
            .filter(|(_, _, only)| only.is_empty() || only.contains(&annotation.as_str()))
            .map(|(label, snippet, _)| CompletionItem {
                insert_text: (*snippet).into(),
                snippet: snippet.contains('$'),
                filter_text: Some((*label).into()),
                ..CompletionItem::new(snippet.replace("$1", "…").replace("$2", "…").as_str(), CompletionKind::Function)
            })
            .collect()
    }
}

impl Plugin for SecurityTools {
    fn run_command(&mut self, id: &str, _ctx: &Invocation, _arg: Option<&str>) -> Result<Option<String>, Error> {
        Err(Error::NotFound(format!("command {id}")))
    }
}

register_plugin!(SecurityTools, providers);

#[cfg(test)]
mod tests {
    use super::*;
    use openidea_plugin::mock::MockHost;

    const CONFIG: &str = r#"package com.acme;

import org.springframework.security.config.annotation.method.configuration.EnableGlobalMethodSecurity;
import org.springframework.security.config.annotation.web.builders.HttpSecurity;

@Configuration
@EnableGlobalMethodSecurity(prePostEnabled = true)
public class SecurityConfig extends WebSecurityConfigurerAdapter {
    @Override
    protected void configure(HttpSecurity http) throws Exception {
        http.authorizeRequests()
            .antMatchers("/admin/**").hasRole("ROLE_ADMIN")
            .mvcMatchers("/api/**").hasAnyRole("USER", "ROLE_STAFF")
            .regexMatchers("/x.*").permitAll();
    }

    @Bean
    PasswordEncoder encoder() { return NoOpPasswordEncoder.getInstance(); }

    @Bean
    UserDetailsService users() {
        return new InMemoryUserDetailsManager(User.withUsername("u").password("{noop}secret").roles("USER").build());
    }
}
"#;
    const SERVICE: &str = r#"package com.acme;

@Service
class Orders {
    @PreAuthorize("hasRole('ROLE_ADMIN') or hasAnyRole('MANAGER', 'ROLE_AUDITOR')")
    void cancel() {}

    @Secured({"ROLE_BILLING"})
    void bill() {}

    @PostAuthorize("returnObject.owner == authentication.name")
    Order get() { return null; }
}
"#;

    fn doc(name: &str, text: &str) -> Document {
        Document { path: format!("/project/src/main/java/com/acme/{name}"), language: "java".into(), version: 1, text: text.into() }
    }

    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let mut out = text.to_owned();
        let mut edits = edits.to_vec();
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        for e in edits {
            let (from, to) = (offset(text, e.range.start).unwrap(), offset(text, e.range.end).unwrap());
            out.replace_range(from..to, &e.text);
        }
        out
    }

    fn found(host: &MockHost, name: &str, text: &str) -> Vec<(String, String)> {
        host.diagnostics()[&format!("/project/src/main/java/com/acme/{name}")]
            .iter()
            .map(|d| {
                let span = &text[offset(text, d.range.start).unwrap()..offset(text, d.range.end).unwrap()];
                (d.code.clone().unwrap(), span.to_owned())
            })
            .collect()
    }

    #[test]
    fn flags_removed_and_risky_configuration() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        assert!(tools.line_markers(&doc("SecurityConfig.java", CONFIG)).is_empty());
        let codes = found(&host, "SecurityConfig.java", CONFIG);
        let codes: Vec<(&str, &str)> = codes.iter().map(|(c, s)| (c.as_str(), s.as_str())).collect();
        assert_eq!(codes, [
            ("web-security-configurer-adapter", "WebSecurityConfigurerAdapter"),
            ("enable-global-method-security", "@EnableGlobalMethodSecurity(prePostEnabled = true)"),
            ("authorize-requests", "authorizeRequests"),
            ("removed-request-matcher", "antMatchers"),
            ("role-prefix", "ROLE_ADMIN"),
            ("removed-request-matcher", "mvcMatchers"),
            ("role-prefix", "ROLE_STAFF"),
            ("removed-request-matcher", "regexMatchers"),
            ("no-op-password-encoder", "NoOpPasswordEncoder.getInstance()"),
            ("plain-text-password", "{noop}secret"),
        ]);
        assert!(host.diagnostics()["/project/src/main/java/com/acme/SecurityConfig.java"][1].deprecated);

        tools.line_markers(&doc("Orders.java", SERVICE));
        let codes = found(&host, "Orders.java", SERVICE);
        assert_eq!(codes, [("role-prefix".to_owned(), "ROLE_ADMIN".to_owned()), ("role-prefix".into(), "ROLE_AUDITOR".into())]);
    }

    #[test]
    fn quick_fixes_rewrite_the_source() {
        let _host = MockHost::install();
        let mut tools = SecurityTools::default();
        let d = doc("SecurityConfig.java", CONFIG);
        let fix = |tools: &mut SecurityTools, needle: &str| {
            let at = CONFIG.find(needle).unwrap() + 1;
            let p = openidea_plugin::text::position(CONFIG, at);
            let actions = tools.code_actions(&d, Range::new(p, p));
            assert_eq!(actions.len(), 1, "{needle}");
            apply(CONFIG, &actions[0].edits[0].1)
        };
        let fixed = fix(&mut tools, "@EnableGlobal");
        assert!(fixed.contains("\n@EnableMethodSecurity\n"));
        assert!(fixed.contains("import org.springframework.security.config.annotation.method.configuration.EnableMethodSecurity;"));
        assert!(fix(&mut tools, "antMatchers").contains(".requestMatchers(\"/admin/**\")"));
        assert!(fix(&mut tools, "authorizeRequests").contains("http.authorizeHttpRequests()"));
        assert!(fix(&mut tools, "ROLE_STAFF").contains("hasAnyRole(\"USER\", \"STAFF\")"));
        let at = openidea_plugin::text::position(CONFIG, CONFIG.find("regexMatchers").unwrap());
        assert!(tools.code_actions(&d, Range::new(at, at)).is_empty());

        let other = "@EnableGlobalMethodSecurity(securedEnabled = true, prePostEnabled = true) class C {}";
        let actions = tools.code_actions(&doc("C.java", other), Range::new(Position::new(0, 1), Position::new(0, 1)));
        assert_eq!(apply(other, &actions[0].edits[0].1), "@EnableMethodSecurity(securedEnabled = true) class C {}");
    }

    #[test]
    fn completes_security_expressions_and_known_roles() {
        let host = MockHost::install();
        host.add_file("src/main/java/com/acme/Orders.java", SERVICE);
        let mut tools = SecurityTools::default();
        let text = "class A {\n    @PostFilter(\"\")\n    List<Order> all() { return null; }\n}\n";
        let items = tools.completion(&doc("A.java", text), Position::new(1, 17), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"hasRole('…')"));
        assert!(labels.contains(&"filterObject"));
        assert!(labels.contains(&"returnObject"));
        let has_role = items.iter().find(|i| i.label == "hasRole('…')").unwrap();
        assert_eq!((has_role.insert_text.as_str(), has_role.snippet), ("hasRole('$1')", true));

        let text = "class A {\n    @PreAuthorize(\"isAuthenticated() and hasRole('\")\n    void f() {}\n}\n";
        let items = tools.completion(&doc("A.java", text), Position::new(1, 50), None);
        let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["ADMIN", "AUDITOR", "BILLING", "MANAGER"]);

        assert!(tools.completion(&doc("A.java", "class A { String s = \"\"; }"), Position::new(0, 22), None).is_empty());
    }

    const SECURITY_XML: &str = r#"<beans xmlns="http://www.springframework.org/schema/beans"
       xmlns:sec="http://www.springframework.org/schema/security">
    <sec:http pattern="/api/**">
        <sec:intercept-url pattern="/api/**" access="hasRole('ROLE_API')"/>
        <sec:intercept-url pattern="/api/**" access="permitAll"/>
    </sec:http>
    <sec:http>
        <sec:intercept-url pattern="/admin/**" access="hasRole('ADMIN') and hasRoles('X')"/>
        <sec:intercept-url pattern="/a" access="hasRole()"/>
        <sec:intercept-url pattern="/b" access="hasAnyRole('A', 'ROLE_B"/>
        <sec:intercept-url pattern="/c" access="(isAuthenticated()"/>
        <sec:intercept-url pattern="/ok" access="hasAuthority(&apos;x&apos;) and authentication.name.isEmpty() and hasIpAddress('10.0.0.0/8')"/>
        <sec:intercept-url pattern="/**" access="permitAll"/>
        <sec:intercept-url pattern="/late" access="denyAll"/>
    </sec:http>
    <sec:http use-expressions="false">
        <sec:intercept-url pattern="/x" access="ROLE_X,ROLE_Y"/>
    </sec:http>
    <sec:global-method-security pre-post-annotations="enabled"/>
</beans>
"#;

    #[test]
    fn intercept_url_pattern_and_access_are_checked() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let d = Document { path: "/project/src/main/resources/security.xml".into(), language: "xml".into(), version: 1, text: SECURITY_XML.into() };
        tools.line_markers(&d);
        let found: Vec<(String, String)> = host.diagnostics()[&d.path]
            .iter()
            .map(|x| {
                let span = &SECURITY_XML[offset(SECURITY_XML, x.range.start).unwrap()..offset(SECURITY_XML, x.range.end).unwrap()];
                (x.code.clone().unwrap(), span.to_owned())
            })
            .collect();
        let found: Vec<(&str, &str)> = found.iter().map(|(c, s)| (c.as_str(), s.as_str())).collect();
        assert_eq!(found, [
            ("role-prefix", "ROLE_API"),
            ("duplicate-pattern", "/api/**"),
            ("unknown-security-function", "hasRoles"),
            ("empty-security-function", "hasRole()"),
            ("unbalanced-expression", "hasAnyRole('A', 'ROLE_B"),
            ("unbalanced-expression", "(isAuthenticated()"),
            ("unreachable-patterns", "/**"),
            ("use-expressions-false", "use-expressions=\"false\""),
            ("global-method-security", "sec:global-method-security"),
        ]);
        assert!(host.diagnostics()[&d.path][8].deprecated);

        let plain = "<beans xmlns=\"http://www.springframework.org/schema/beans\"><http xmlns=\"http://example.com/other\"><intercept-url pattern=\"/a\" access=\"hasRoles(\"/></http></beans>";
        let d = Document { path: "/project/other.xml".into(), language: "xml".into(), version: 1, text: plain.into() };
        tools.line_markers(&d);
        assert!(host.diagnostics().get(&d.path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn kotlin_http_dsl_flags_deprecated_matchers() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let text = r#"package com.acme

import org.springframework.security.config.annotation.web.invoke
import org.springframework.security.config.annotation.method.configuration.EnableGlobalMethodSecurity

@EnableGlobalMethodSecurity(prePostEnabled = true)
class SecurityConfig : WebSecurityConfigurerAdapter() {
    fun chain(http: HttpSecurity): SecurityFilterChain {
        http {
            authorizeRequests {
                authorize("/x", permitAll)
                authorize("/y", hasRole("ROLE_ADMIN"))
            }
        }
        http.authorizeRequests().antMatchers("/a").permitAll().mvcMatchers("/b").permitAll().regexMatchers("/c.*").denyAll()
        return http.build()
    }
}
"#;
        let d = Document { path: "/project/src/main/kotlin/com/acme/SecurityConfig.kt".into(), language: "kotlin".into(), version: 1, text: text.into() };
        tools.line_markers(&d);
        let found: Vec<(String, String)> = host.diagnostics()[&d.path]
            .iter()
            .map(|x| (x.code.clone().unwrap(), text[offset(text, x.range.start).unwrap()..offset(text, x.range.end).unwrap()].to_owned()))
            .collect();
        let found: Vec<(&str, &str)> = found.iter().map(|(c, s)| (c.as_str(), s.as_str())).collect();
        assert_eq!(found, [
            ("enable-global-method-security", "@EnableGlobalMethodSecurity(prePostEnabled = true)"),
            ("web-security-configurer-adapter", "WebSecurityConfigurerAdapter"),
            ("authorize-requests", "authorizeRequests"),
            ("role-prefix", "ROLE_ADMIN"),
            ("authorize-requests", "authorizeRequests"),
            ("removed-request-matcher", "antMatchers"),
            ("removed-request-matcher", "mvcMatchers"),
            ("removed-request-matcher", "regexMatchers"),
        ]);
        let p = openidea_plugin::text::position(text, text.find("antMatchers").unwrap());
        assert_eq!(tools.code_actions(&d, Range::new(p, p)).len(), 1);
    }

    #[test]
    fn kotlin_authorize_requests_in_comments_and_strings_is_quiet() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let import = "import org.springframework.security.config.annotation.web.invoke\n";
        let body = "// authorizeRequests { }\n/* a /* nested */ authorizeRequests { } */\nval s = \"authorizeRequests { } .antMatchers(\\\"/x\\\")\"\nval r = \"\"\"\nauthorizeRequests {\n\"\"\"\n// http.antMatchers(\"/a\")\nval ok = myauthorizeRequests { }\n";
        let mut quiet = |name: &str, text: String| {
            let d = Document { path: format!("/project/{name}.kt"), language: "kotlin".into(), version: 1, text };
            tools.line_markers(&d);
            host.diagnostics().get(&d.path).is_none_or(Vec::is_empty)
        };
        assert!(quiet("A", format!("{import}{body}")));
        assert!(!quiet("P", format!("{import}http.authorizeRequests {{ }}\n{body}")));
        assert!(quiet("B", "val x = http.authorizeRequests { }.antMatchers(\"/a\")\n".into()));
    }

    fn xml_codes(xml: &str) -> Vec<String> {
        let host = MockHost::install();
        let d = Document { path: "/project/s.xml".into(), language: "xml".into(), version: 1, text: xml.into() };
        SecurityTools::default().line_markers(&d);
        host.diagnostics().get(&d.path).map(|v| v.iter().map(|x| x.code.clone().unwrap()).collect()).unwrap_or_default()
    }

    #[test]
    fn xml_namespaces_and_legitimate_patterns() {
        let ns = "http://www.springframework.org/schema/security";
        let default_ns = format!("<beans xmlns=\"{ns}\"><http><intercept-url pattern=\"/a\" access=\"hasRoles('X')\"/></http></beans>");
        assert_eq!(xml_codes(&default_ns), ["unknown-security-function"]);
        let prefixed = format!("<beans xmlns:security=\"{ns}\"><security:http><security:intercept-url pattern=\"/a\" access=\"hasRoles('X')\"/></security:http></beans>");
        assert_eq!(xml_codes(&prefixed), ["unknown-security-function"]);
        let quiet = |inner: &str, http: &str| xml_codes(&format!("<beans xmlns=\"{ns}\"><http{http}>{inner}</http></beans>"));
        let url = |extra: &str| format!("<intercept-url pattern=\"/a\" access=\"permitAll\"{extra}/>");
        assert!(quiet(&format!("{}{}", url(" method=\"GET\""), url(" method=\"POST\"")), "").is_empty());
        assert!(quiet(&format!("{}{}", url(" servlet-path=\"/x\""), url(" servlet-path=\"/y\"")), "").is_empty());
        assert_eq!(quiet(&format!("{}{}", url(""), url(" servlet-path=\"/y\"")), ""), Vec::<String>::new());
        let catch_all = |extra: &str| format!("<intercept-url pattern=\"/**\" access=\"permitAll\"{extra}/><intercept-url pattern=\"/b\" access=\"denyAll\"/>");
        assert!(quiet(&catch_all(" method=\"GET\""), "").is_empty());
        assert!(quiet(&catch_all(""), " request-matcher=\"regex\"").is_empty());
        assert!(quiet(&catch_all(""), " request-matcher-ref=\"m\"").is_empty());
        assert_eq!(quiet(&catch_all(""), ""), ["unreachable-patterns"]);
    }

    #[test]
    fn kotlin_char_literal_quote_does_not_open_a_string() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let import = "import org.springframework.security.config.annotation.web.invoke\n";
        let mut codes = |name: &str, body: &str| {
            let d = Document { path: format!("/project/{name}.kt"), language: "kotlin".into(), version: 1, text: format!("{import}{body}") };
            tools.line_markers(&d);
            host.diagnostics().get(&d.path).map(|v| v.len()).unwrap_or(0)
        };
        assert_eq!(codes("A", "val q = '\"'; http.authorizeRequests { }\n"), 1);
        assert_eq!(codes("B", "'\"'.also { println(\"authorizeRequests {\") }\n"), 0);
        assert_eq!(codes("C", "val a = '\\n'; val b = '\\''; val c = '\\\\'; http.authorizeRequests { }\n"), 1);
        assert_eq!(codes("D", "val s = \"it's\"; don't; http.authorizeRequests { }\n"), 1);
        assert_eq!(codes("E", "x = '\n"), 0);
    }

    const SECURITY_IMPORT: &str = "import org.springframework.security.config.annotation.web.builders.HttpSecurity;\n";

    fn spans(path: &str, text: &str, code: &str) -> Vec<String> {
        let _host = MockHost::install();
        problems(path, text)
            .into_iter()
            .filter(|p| p.diagnostic.code.as_deref() == Some(code))
            .map(|p| {
                let r = p.diagnostic.range;
                text[offset(text, r.start).unwrap()..offset(text, r.end).unwrap()].to_owned()
            })
            .collect()
    }

    fn java_spans(body: &str, code: &str) -> Vec<String> {
        spans("/project/C.java", &format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) throws Exception {{\n{body}\n}} }}"), code)
    }

    #[test]
    fn request_matchers_after_any_request_warn() {
        let lambda = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/a\").permitAll().anyRequest().authenticated().requestMatchers(\"/b\").denyAll().anyRequest().denyAll());";
        assert_eq!(java_spans(lambda, "matchers-after-any-request"), ["requestMatchers", "anyRequest"]);
        let chained = "http.authorizeHttpRequests().anyRequest().authenticated().antMatchers(\"/b\").denyAll();\nhttp.authorizeHttpRequests().requestMatchers(\"/ok\").permitAll();";
        assert_eq!(java_spans(chained, "matchers-after-any-request"), ["antMatchers"]);
    }

    #[test]
    fn narrow_matcher_after_broader_matcher_warns() {
        let body = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/**\").permitAll().requestMatchers(\"/admin/**\").hasRole(\"ADMIN\"));";
        assert_eq!(java_spans(body, "matcher-order"), ["/admin/**"]);
        let nested = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/api/**\").authenticated().requestMatchers(\"/api/v1/users\", \"/other\").permitAll().requestMatchers(\"/api\").denyAll());";
        assert_eq!(java_spans(nested, "matcher-order"), ["/api/v1/users", "/api"]);
        let method = "http.authorizeHttpRequests(a -> a.requestMatchers(HttpMethod.GET, \"/api/**\").permitAll().requestMatchers(\"/api/x\").denyAll().requestMatchers(HttpMethod.POST, \"/api/y\").denyAll());";
        assert!(java_spans(method, "matcher-order").is_empty());
    }

    #[test]
    fn ordered_matchers_are_quiet() {
        let body = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/admin/**\").hasRole(\"ADMIN\").requestMatchers(\"/api/**\").authenticated().requestMatchers(\"/**\").permitAll().anyRequest().denyAll());";
        assert!(java_spans(body, "matcher-order").is_empty());
        assert!(java_spans(body, "matchers-after-any-request").is_empty());
    }

    #[test]
    fn matcher_checks_are_quiet_for_library_or_dynamic_name() {
        let wrong_order = "http.authorizeHttpRequests(a -> a.anyRequest().authenticated().requestMatchers(\"/a\").denyAll().requestMatchers(\"/**\").permitAll().requestMatchers(\"/b\").denyAll());";
        let library = format!("class C {{ void f(Lib http) {{\n{wrong_order}\n}} }}");
        assert!(spans("/project/C.java", &library, "matchers-after-any-request").is_empty());
        assert!(spans("/project/C.java", &library, "matcher-order").is_empty());
        let dynamic = "http.authorizeHttpRequests(a -> a.requestMatchers(PUBLIC).permitAll().requestMatchers(\"/x\").denyAll().requestMatchers(regexMatcher(\"/.*\")).denyAll().regexMatchers(\"/.*\").denyAll().requestMatchers(\"/y\").denyAll());";
        assert!(java_spans(dynamic, "matcher-order").is_empty());
        let masked = "// http.authorizeHttpRequests(a -> a.anyRequest().denyAll().requestMatchers(\"/a\"))\nString s = \"authorizeHttpRequests(a -> a.anyRequest().requestMatchers(1))\";\nhttp.authorizeHttpRequests(a -> a.requestMatchers(\"/a\").permitAll()/* .anyRequest() */.requestMatchers(\"/b\").denyAll().anyRequest().authenticated());";
        assert!(java_spans(masked, "matchers-after-any-request").is_empty());
        assert!(java_spans(masked, "matcher-order").is_empty());
    }

    #[test]
    fn enable_web_security_debug_true_warns() {
        let on = format!("{SECURITY_IMPORT}@EnableWebSecurity(debug = true)\nclass C {{}}");
        assert_eq!(spans("/project/C.java", &on, "debug-enabled"), ["@EnableWebSecurity(debug = true)"]);
        let customizer = format!("{SECURITY_IMPORT}class C {{ WebSecurityCustomizer c() {{ return web -> web.debug(true); }} }}");
        assert_eq!(spans("/project/C.java", &customizer, "debug-enabled"), ["debug(true)"]);
        let xml = "<beans xmlns:sec=\"http://www.springframework.org/schema/security\"><sec:debug/></beans>";
        assert_eq!(xml_codes(xml), ["debug-enabled"]);
    }

    #[test]
    fn debug_enabled_is_quiet_for_library_or_dynamic_name() {
        for text in [
            "@EnableWebSecurity(debug = false)\nclass C {}",
            "@EnableWebSecurity(debug = DEBUG)\nclass C {}",
            "@EnableWebSecurity\nclass C {}",
            "// @EnableWebSecurity(debug = true)\nString s = \"@EnableWebSecurity(debug = true)\";\nclass C { void f() { logger.debug(true); } }",
        ] {
            assert!(spans("/project/C.java", &format!("{SECURITY_IMPORT}{text}"), "debug-enabled").is_empty(), "{text}");
        }
        assert!(spans("/project/C.java", "@EnableWebSecurity(debug = true)\nclass C {}", "debug-enabled").is_empty());
        let other_namespace = "<beans xmlns:x=\"urn:other\"><x:debug/></beans>";
        assert!(xml_codes(other_namespace).is_empty());
    }

    #[test]
    fn lambda_dsl_and_kotlin_dsl_are_both_read() {
        let kotlin = "import org.springframework.security.config.annotation.web.invoke\n@EnableWebSecurity(debug = true)\nclass C {\n    fun f(http: HttpSecurity) {\n        http {\n            authorizeHttpRequests {\n                authorize(\"/**\", permitAll)\n                authorize(\"/admin/**\", hasRole(\"ADMIN\"))\n                authorize(anyRequest, authenticated)\n                authorize(\"/late\", denyAll)\n            }\n        }\n    }\n}\n";
        assert_eq!(spans("/project/C.kt", kotlin, "matcher-order"), ["/admin/**"]);
        assert_eq!(spans("/project/C.kt", kotlin, "matchers-after-any-request"), ["authorize"]);
        assert_eq!(spans("/project/C.kt", kotlin, "debug-enabled"), ["@EnableWebSecurity(debug = true)"]);
        let java = "http.authorizeHttpRequests(a -> a.anyRequest().authenticated().requestMatchers(\"/late\").denyAll());";
        assert_eq!(java_spans(java, "matchers-after-any-request"), ["requestMatchers"]);
    }

    #[test]
    fn fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let bad = format!("{SECURITY_IMPORT}@EnableWebSecurity(debug = true)\nclass C {{ void f(HttpSecurity http) {{ http.authorizeHttpRequests(a -> a.anyRequest().denyAll().requestMatchers(\"/a\").denyAll()); }} }}");
        let d = Document { path: "/project/C.java".into(), language: "java".into(), version: 1, text: bad };
        tools.line_markers(&d);
        assert_eq!(host.diagnostics()[&d.path].len(), 2);
        let fixed = Document { text: format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ http.authorizeHttpRequests(a -> a.requestMatchers(\"/a\").denyAll().anyRequest().denyAll()); }} }}"), ..d };
        tools.line_markers(&fixed);
        assert!(host.diagnostics().get(&fixed.path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn matcher_and_debug_groups_can_be_switched_off() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        host.set_setting("springSecurity.checkMatchers", "false");
        host.set_setting("springSecurity.checkDebug", "false");
        let text = format!("{SECURITY_IMPORT}@EnableWebSecurity(debug = true)\nclass C {{ void f(HttpSecurity http) {{ http.authorizeHttpRequests(a -> a.anyRequest().denyAll().requestMatchers(\"/a\").denyAll()); }} }}");
        let d = Document { path: "/project/C.java".into(), language: "java".into(), version: 1, text };
        tools.line_markers(&d);
        assert!(host.diagnostics().get(&d.path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn matcher_ranges_are_utf16_columns() {
        let text = format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ /* \u{e9}\u{1F600} */ http.authorizeHttpRequests(a -> a.anyRequest().denyAll().requestMatchers(\"/a\").denyAll()); }} }}");
        let _host = MockHost::install();
        let found = problems("/project/C.java", &text);
        let d = &found[0].diagnostic;
        let line = text.lines().nth(1).unwrap();
        let expected = line.encode_utf16().count() - line[line.find("requestMatchers").unwrap()..].encode_utf16().count();
        assert_eq!(d.range.start.character as usize, expected);
    }

    #[test]
    fn double_star_before_the_last_segment_warns() {
        let body = "http.securityMatcher(\"/a/**/b\").authorizeHttpRequests(a -> a.requestMatchers(\"/**/*.css\", \"/ok/**\").permitAll());";
        assert_eq!(java_spans(body, "obsolete-path-pattern"), ["**", "**"]);
        let located = java_spans("http.securityMatcher(\"/a/**/b\");", "obsolete-path-pattern");
        assert_eq!(located, ["**"]);
    }

    #[test]
    fn trailing_double_star_is_quiet() {
        let body = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/a/**\", \"/**\", \"/a/*\").permitAll());";
        assert!(java_spans(body, "obsolete-path-pattern").is_empty());
    }

    #[test]
    fn kotlin_authorize_pattern_warns() {
        let kotlin = "import org.springframework.security.config.annotation.web.invoke\nclass C {\n    fun f(http: HttpSecurity) {\n        http { authorizeHttpRequests { authorize(HttpMethod.GET, \"/x/**/y\", permitAll) } }\n    }\n}\n";
        assert_eq!(spans("/project/C.kt", kotlin, "obsolete-path-pattern"), ["**"]);
    }

    #[test]
    fn path_patterns_are_quiet_for_library_or_dynamic_name() {
        let wrong = "http.authorizeHttpRequests(a -> a.requestMatchers(\"/a/**/b\").permitAll());";
        let library = format!("class C {{ void f(Lib http) {{\n{wrong}\n}} }}");
        assert!(spans("/project/C.java", &library, "obsolete-path-pattern").is_empty());
        for body in [
            "http.authorizeHttpRequests(a -> a.requestMatchers(antMatcher(\"/a/**/b\")).permitAll());",
            "http.authorizeHttpRequests(a -> a.requestMatchers(PATTERN).permitAll());",
            "http.authorizeHttpRequests(a -> a.requestMatchers(\"/a/**\" + \"/b\").permitAll());",
            "http.authorizeHttpRequests(a -> a.requestMatchers(\"${p}/**/b\").permitAll());",
            "// http.requestMatchers(\"/a/**/b\")\nString s = \"requestMatchers(\\\"/a/**/b\\\")\";",
        ] {
            assert!(java_spans(body, "obsolete-path-pattern").is_empty(), "{body}");
        }
        let method = "http.authorizeHttpRequests(a -> a.requestMatchers(HttpMethod.GET, \"/a/**/b\").permitAll());";
        assert_eq!(java_spans(method, "obsolete-path-pattern"), ["**"]);
    }

    #[test]
    fn ant_matchers_are_not_reported_twice() {
        let body = "http.authorizeHttpRequests(a -> a.antMatchers(\"/a/**/b\").permitAll().regexMatchers(\"/a/**/b\").denyAll());";
        assert!(java_spans(body, "obsolete-path-pattern").is_empty());
    }

    #[test]
    fn path_pattern_group_can_be_switched_off() {
        let host = MockHost::install();
        host.set_setting("springSecurity.checkPathPatterns", "false");
        let text = format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ http.securityMatcher(\"/a/**/b\"); }} }}");
        assert!(problems("/project/C.java", &text).iter().all(|p| p.diagnostic.code.as_deref() != Some("obsolete-path-pattern")));
    }

    #[test]
    fn path_pattern_fixed_file_publishes_empty_list() {
        let host = MockHost::install();
        let mut tools = SecurityTools::default();
        let bad = format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ http.securityMatcher(\"/a/**/b\"); }} }}");
        let d = Document { path: "/project/C.java".into(), language: "java".into(), version: 1, text: bad };
        tools.line_markers(&d);
        assert_eq!(host.diagnostics()[&d.path].len(), 1);
        let fixed = Document { text: format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ http.securityMatcher(\"/a/**\"); }} }}"), ..d };
        tools.line_markers(&fixed);
        assert!(host.diagnostics().get(&fixed.path).is_none_or(Vec::is_empty));
    }

    #[test]
    fn path_pattern_ranges_use_utf16_columns_with_multibyte_text() {
        let text = format!("{SECURITY_IMPORT}class C {{ void f(HttpSecurity http) {{ http.securityMatcher(\"/\u{e9}\u{1F600}/**/b\"); }} }}");
        let _host = MockHost::install();
        let found = problems("/project/C.java", &text);
        let d = &found.iter().find(|p| p.diagnostic.code.as_deref() == Some("obsolete-path-pattern")).unwrap().diagnostic;
        let line = text.lines().nth(1).unwrap();
        let expected = line.encode_utf16().count() - line[line.find("**").unwrap()..].encode_utf16().count();
        assert_eq!(d.range.start.character as usize, expected);
        assert_eq!(d.range.end.character as usize, expected + 2);
    }

    #[test]
    fn path_pattern_crlf_files_report_correct_lines() {
        let text = format!("{SECURITY_IMPORT}class C {{\n    void f(HttpSecurity http) {{\n        http.securityMatcher(\"/a/**/b\");\n    }}\n}}").replace('\n', "\r\n");
        let _host = MockHost::install();
        let found = problems("/project/C.java", &text);
        let d = &found.iter().find(|p| p.diagnostic.code.as_deref() == Some("obsolete-path-pattern")).unwrap().diagnostic;
        assert_eq!(d.range.start.line, 3);
        assert_eq!(d.range.start.character, 33);
    }
}
